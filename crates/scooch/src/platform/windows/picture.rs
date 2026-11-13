use super::com::{self, take};
use super::layout::{self, SPAN};
use crate::platform::Screen;
use anyhow::{bail, Context, Result};
use scooch_core::{Rect, RgbImage};
use std::path::{Path, PathBuf};
use ::windows::core::{HSTRING, PCWSTR};
use ::windows::Win32::Foundation::{GENERIC_READ, RECT};
use ::windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, IWICImagingFactory, IWICPalette, GUID_WICPixelFormat24bppRGB, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use ::windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, CLSCTX_INPROC_SERVER};
use ::windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper, DSS_SLIDESHOW, DWPOS_SPAN};

pub fn object() -> Result<IDesktopWallpaper> {
    com::ready()?;
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }.context("Scooch could not reach the Windows wallpaper settings, so sign out and back in, then try again.")
}

pub fn frame(rect: RECT) -> Rect {
    Rect { x: rect.left as f32, y: rect.top as f32, w: (rect.right - rect.left) as f32, h: (rect.bottom - rect.top) as f32 }
}

pub fn monitors(wallpaper: &IDesktopWallpaper) -> Result<Vec<Screen>> {
    let count = unsafe { wallpaper.GetMonitorDevicePathCount() }.context("Windows reports no screens, so try again once a display is connected.")?;
    let mut found = Vec::new();
    for index in 0..count {
        let Ok(path) = (unsafe { wallpaper.GetMonitorDevicePathAt(index) }) else { continue };
        let id = take(path).to_string_lossy().into_owned();
        let Ok(rect) = (unsafe { wallpaper.GetMonitorRECT(&HSTRING::from(id.as_str())) }) else { continue };
        if rect.right > rect.left && rect.bottom > rect.top {
            found.push(Screen { id, frame: frame(rect), scale: 1.0 });
        }
    }
    Ok(found)
}

pub fn spanned(wallpaper: &IDesktopWallpaper) -> bool {
    unsafe { wallpaper.GetPosition() }.is_ok_and(|position| position == DWPOS_SPAN)
}

pub fn screens() -> Result<Vec<Screen>> {
    let wallpaper = object()?;
    let found = monitors(&wallpaper)?;
    let frames: Vec<Rect> = found.iter().map(|screen| screen.frame).collect();
    let Some(whole) = layout::union(&frames) else { bail!("Windows reports no screens, so try again once a display is connected.") };
    if spanned(&wallpaper) {
        return Ok(vec![Screen { id: SPAN.into(), frame: whole, scale: 1.0 }]);
    }
    Ok(found)
}

fn monitor(screen: &Screen) -> Option<HSTRING> {
    (screen.id != SPAN).then(|| HSTRING::from(screen.id.as_str()))
}

fn pointer(id: &Option<HSTRING>) -> PCWSTR {
    id.as_ref().map_or(PCWSTR::null(), |id| PCWSTR(id.as_ptr()))
}

pub fn slideshow(wallpaper: &IDesktopWallpaper) -> bool {
    unsafe { wallpaper.GetStatus() }.is_ok_and(|state| state.0 & DSS_SLIDESHOW.0 != 0)
}

pub fn wallpaper(screen: &Screen) -> Result<Option<PathBuf>> {
    let wallpaper = object()?;
    if slideshow(&wallpaper) {
        return Ok(None);
    }
    let id = monitor(screen);
    let path = unsafe { wallpaper.GetWallpaper(pointer(&id)) }.context("Scooch could not read the wallpaper, so check that the screen is still connected.")?;
    let path = take(path);
    if path.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(path);
    if themes().is_some_and(|themes| layout::inside(&path, &themes)) {
        return keep(&path).map(Some);
    }
    Ok(Some(path))
}

pub fn themes() -> Option<PathBuf> {
    dirs::config_dir().map(|roaming| roaming.join("Microsoft").join("Windows").join("Themes"))
}

pub fn keep(path: &Path) -> Result<PathBuf> {
    let bytes = std::fs::read(path).with_context(|| format!("Scooch could not read {}, so pick the wallpaper again in Settings.", path.display()))?;
    let folder = crate::state::Store::open().root.join("picked");
    let target = folder.join(layout::picked_name(path, &bytes, crate::cache::digest(&bytes)));
    if target.is_file() {
        return Ok(target);
    }
    std::fs::create_dir_all(&folder).with_context(|| format!("Scooch could not make {}, so check the disk.", folder.display()))?;
    let partial = target.with_extension("part");
    let written = std::fs::write(&partial, &bytes).and_then(|()| std::fs::rename(&partial, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written.with_context(|| format!("Scooch could not copy {}, so check that the disk has room.", path.display()))?;
    Ok(target)
}

pub fn set_wallpaper(screen: &Screen, path: &Path) -> Result<()> {
    let wallpaper = object()?;
    let id = monitor(screen);
    let file = HSTRING::from(layout::plain(path).as_path());
    unsafe { wallpaper.SetWallpaper(pointer(&id), &file) }.with_context(|| format!("Windows did not show {} as the wallpaper, so pick another picture.", path.display()))
}

pub fn decode(path: &Path) -> Result<RgbImage> {
    if let Ok(image) = scooch_core::open(path) {
        return Ok(image);
    }
    system_decode(path).with_context(|| format!("{} is not a picture Windows can read, so pick a JPEG, PNG or HEIC file.", path.display()))
}

pub fn system_decode(path: &Path) -> Result<RgbImage> {
    com::ready()?;
    let factory: IWICImagingFactory = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }?;
    let name = HSTRING::from(layout::plain(path).as_path());
    let decoder = unsafe { factory.CreateDecoderFromFilename(&name, None, GENERIC_READ, WICDecodeMetadataCacheOnDemand) }?;
    let frame = unsafe { decoder.GetFrame(0) }?;
    let converter = unsafe { factory.CreateFormatConverter() }?;
    unsafe { converter.Initialize(&frame, &GUID_WICPixelFormat24bppRGB, WICBitmapDitherTypeNone, None::<&IWICPalette>, 0.0, WICBitmapPaletteTypeCustom) }?;
    let (mut width, mut height) = (0u32, 0u32);
    unsafe { converter.GetSize(&mut width, &mut height) }?;
    let stride = u64::from(width) * 3;
    let size = stride * u64::from(height);
    if size == 0 || size > u64::from(u32::MAX) {
        bail!("The picture has a size Scooch cannot hold.");
    }
    let mut pixels = vec![0u8; size as usize];
    unsafe { converter.CopyPixels(std::ptr::null(), stride as u32, &mut pixels) }?;
    RgbImage::from_raw(width, height, pixels).context("The picture did not fit in memory.")
}
