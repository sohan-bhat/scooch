use super::sys::{self, Owned, Ref};
use super::Screen;
use anyhow::{anyhow, bail, Context, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey, NSWorkspaceDesktopImageScalingKey};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSString, NSURL};
use scooch_core::{Rect, RgbImage};
use std::path::{Path, PathBuf};
use std::ptr::null;

const FILL: usize = 3;

pub fn screens(marker: MainThreadMarker) -> Result<Vec<Screen>> {
    let mut found = Vec::new();
    let displays = NSScreen::screens(marker);
    if displays.is_empty() {
        bail!("macOS reports no screens, so try again once a display is connected.");
    }
    for screen in displays {
        let Some(display) = number(&screen) else { continue };
        let bounds = unsafe { sys::CGDisplayBounds(display) };
        let frame = Rect { x: bounds.origin.x as f32, y: bounds.origin.y as f32, w: bounds.size.width as f32, h: bounds.size.height as f32 };
        let display = identifier(display);
        let space = match super::spaces::showing(&display) {
            super::spaces::Showing::FullScreen => continue,
            super::spaces::Showing::Desktop(space) => Some(space),
            super::spaces::Showing::Unknown => None,
        };
        found.push(Screen { id: super::spaces::join(&display, space.as_deref()), frame, scale: screen.backingScaleFactor() as f32 });
    }
    Ok(found)
}

pub fn shown(marker: MainThreadMarker) -> Vec<(String, Option<PathBuf>)> {
    screens(marker).unwrap_or_default().into_iter().map(|screen| {
        let reported = reported(marker, &screen.id).ok().flatten();
        (screen.id, reported)
    }).collect()
}

pub fn display_ids(marker: MainThreadMarker) -> Vec<String> {
    NSScreen::screens(marker).into_iter().filter_map(|screen| number(&screen)).map(identifier).collect()
}

fn number(screen: &NSScreen) -> Option<u32> {
    let value: Option<Retained<AnyObject>> = unsafe { msg_send![&*screen.deviceDescription(), objectForKey: ns_string!("NSScreenNumber")] };
    value.and_then(|value| value.downcast_ref::<NSNumber>().map(|number| number.unsignedIntValue()))
}

fn identifier(display: u32) -> String {
    let named = Owned::new(unsafe { sys::CGDisplayCreateUUIDFromDisplayID(display) })
        .and_then(|uuid| Owned::new(unsafe { sys::CFUUIDCreateString(null(), uuid.0) }))
        .map(|text| unsafe { &*(text.0 as *const NSString) }.to_string());
    named.unwrap_or_else(|| format!("display-{display}"))
}

pub fn screen_for(marker: MainThreadMarker, id: &str) -> Result<(Retained<NSScreen>, u32)> {
    let screen = find(marker, id)?;
    let display = number(&screen).context("macOS gave that screen no display number")?;
    Ok((screen, display))
}

fn find(marker: MainThreadMarker, id: &str) -> Result<Retained<NSScreen>> {
    let (display, space) = super::spaces::split(id);
    let screen = NSScreen::screens(marker)
        .into_iter()
        .find(|screen| number(screen).is_some_and(|number| identifier(number) == display))
        .ok_or_else(|| anyhow!("The screen {display} is not connected, so reconnect it and try again."))?;
    if let Some(space) = space {
        if super::spaces::current(display).as_deref() != Some(space) {
            bail!("That desktop is no longer showing, so Scooch will finish when it shows again.");
        }
    }
    Ok(screen)
}

pub fn reported(marker: MainThreadMarker, id: &str) -> Result<Option<PathBuf>> {
    let screen = find(marker, id)?;
    let url = NSWorkspace::sharedWorkspace().desktopImageURLForScreen(&screen);
    Ok(url.and_then(|url| url.path()).map(|path| PathBuf::from(path.to_string())))
}

pub fn set_wallpaper(marker: MainThreadMarker, id: &str, path: &Path) -> Result<()> {
    let screen = find(marker, id)?;
    let url = NSURL::from_file_path(path).ok_or_else(|| anyhow!("{} is not a usable path, so pick another picture.", path.display()))?;
    let fill: Retained<AnyObject> = NSNumber::new_usize(FILL).into();
    let clip: Retained<AnyObject> = NSNumber::new_bool(true).into();
    let options = NSDictionary::from_slices(&[unsafe { NSWorkspaceDesktopImageScalingKey }, unsafe { NSWorkspaceDesktopImageAllowClippingKey }], &[&*fill, &*clip]);
    unsafe { NSWorkspace::sharedWorkspace().setDesktopImageURL_forScreen_options_error(&url, &screen, &options) }
        .map_err(|error| anyhow!("macOS did not take {} as the wallpaper and said {}", path.display(), error.localizedDescription()))
}

pub fn decode(path: &Path) -> Result<RgbImage> {
    match matched(path) {
        Ok(image) => Ok(image),
        Err(error) => scooch_core::open(path).map_err(|_| error),
    }
}

pub fn profile() -> Option<Vec<u8>> {
    let space = Owned::new(unsafe { sys::CGColorSpaceCreateWithName(sys::kCGColorSpaceSRGB) })?;
    let data = Owned::new(unsafe { sys::CGColorSpaceCopyICCData(space.0) })?;
    let length = usize::try_from(unsafe { sys::CFDataGetLength(data.0) }).ok()?;
    let bytes = unsafe { sys::CFDataGetBytePtr(data.0) };
    (!bytes.is_null() && length > 0).then(|| unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec())
}

pub fn decode_exact(path: &Path) -> Result<RgbImage> {
    let url = NSURL::from_file_path(path).ok_or_else(|| anyhow!("{} is not a usable path.", path.display()))?;
    let source = Owned::new(unsafe { sys::CGImageSourceCreateWithURL(&*url as *const NSURL as Ref, null()) });
    let source = source.with_context(|| format!("{} is not a picture macOS can read.", path.display()))?;
    let image = Owned::new(unsafe { sys::CGImageSourceCreateImageAtIndex(source.0, 0, null()) }).with_context(|| format!("{} could not be decoded.", path.display()))?;
    drop(source);
    pixels(&image)
}

fn matched(path: &Path) -> Result<RgbImage> {
    let url = NSURL::from_file_path(path).ok_or_else(|| anyhow!("{} is not a usable path.", path.display()))?;
    let source = Owned::new(unsafe { sys::CGImageSourceCreateWithURL(&*url as *const NSURL as Ref, null()) });
    let source = source.with_context(|| format!("{} is not a picture macOS can read.", path.display()))?;
    let (wide, tall) = size(&source).with_context(|| format!("{} has no readable size.", path.display()))?;
    let longest: Retained<AnyObject> = NSNumber::new_usize(wide.max(tall)).into();
    let yes: Retained<AnyObject> = NSNumber::new_bool(true).into();
    let keys = unsafe { [key(sys::kCGImageSourceCreateThumbnailFromImageAlways), key(sys::kCGImageSourceCreateThumbnailWithTransform), key(sys::kCGImageSourceThumbnailMaxPixelSize)] };
    let options = NSDictionary::from_slices(&keys, &[&*yes, &*yes, &*longest]);
    let image = Owned::new(unsafe { sys::CGImageSourceCreateThumbnailAtIndex(source.0, 0, &*options as *const NSDictionary<NSString, AnyObject> as Ref) })
        .with_context(|| format!("{} could not be decoded.", path.display()))?;
    drop(source);
    pixels(&image)
}

fn key(name: Ref) -> &'static NSString {
    unsafe { &*(name as *const NSString) }
}

fn property(source: &Owned, name: Ref) -> Option<usize> {
    let properties = Owned::new(unsafe { sys::CGImageSourceCopyPropertiesAtIndex(source.0, 0, null()) })?;
    let properties = unsafe { &*(properties.0 as *const AnyObject) };
    let value: Option<Retained<AnyObject>> = unsafe { msg_send![properties, objectForKey: key(name)] };
    value?.downcast_ref::<NSNumber>().map(|number| number.as_usize())
}

fn size(source: &Owned) -> Option<(usize, usize)> {
    unsafe { Some((property(source, sys::kCGImagePropertyPixelWidth)?, property(source, sys::kCGImagePropertyPixelHeight)?)) }
}

fn pixels(image: &Owned) -> Result<RgbImage> {
    let (wide, tall) = unsafe { (sys::CGImageGetWidth(image.0), sys::CGImageGetHeight(image.0)) };
    if wide == 0 || tall == 0 {
        bail!("The picture is empty, so pick another one.");
    }
    let mut canvas = vec![0u8; wide * tall * 4];
    let space = Owned::new(unsafe { sys::CGColorSpaceCreateWithName(sys::kCGColorSpaceSRGB) }).context("macOS has no sRGB color space.")?;
    let context = Owned::new(unsafe { sys::CGBitmapContextCreate(canvas.as_mut_ptr().cast(), wide, tall, 8, wide * 4, space.0, sys::BITMAP_RGBX) }).context("macOS could not make a drawing surface.")?;
    let whole = CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: wide as f64, height: tall as f64 } };
    unsafe {
        sys::CGContextSetInterpolationQuality(context.0, sys::INTERPOLATION_HIGH);
        sys::CGContextDrawImage(context.0, whole, image.0);
    }
    drop(context);
    pack(&mut canvas);
    RgbImage::from_raw(wide as u32, tall as u32, canvas).context("The picture did not fit in memory.")
}

pub fn pack(canvas: &mut Vec<u8>) {
    let count = canvas.len() / 4;
    for pixel in 0..count {
        canvas.copy_within(pixel * 4..pixel * 4 + 3, pixel * 3);
    }
    canvas.truncate(count * 3);
    canvas.shrink_to_fit();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p3_strip(path: &Path, colors: &[[u8; 3]]) {
        let space = Owned::new(unsafe { sys::CGColorSpaceCreateWithName(sys::kCGColorSpaceDisplayP3) }).unwrap();
        let wide = colors.len();
        let mut canvas: Vec<u8> = colors.iter().flat_map(|[red, green, blue]| [*red, *green, *blue, 255]).collect();
        let context = Owned::new(unsafe { sys::CGBitmapContextCreate(canvas.as_mut_ptr().cast(), wide, 1, 8, wide * 4, space.0, sys::BITMAP_RGBX) }).unwrap();
        let image = Owned::new(unsafe { sys::CGBitmapContextCreateImage(context.0) }).unwrap();
        let url = NSURL::from_file_path(path).unwrap();
        let kind = NSString::from_str("public.png");
        let destination = Owned::new(unsafe { sys::CGImageDestinationCreateWithURL(&*url as *const NSURL as Ref, &*kind as *const NSString as Ref, 1, null()) }).unwrap();
        unsafe {
            sys::CGImageDestinationAddImage(destination.0, image.0, null());
            assert_eq!(sys::CGImageDestinationFinalize(destination.0), 1);
        }
    }

    fn near(found: &[u8], wanted: [u8; 3], slack: u8) -> bool {
        found.iter().zip(wanted).all(|(a, b)| a.abs_diff(b) <= slack)
    }

    #[test]
    fn display_p3_pictures_are_matched_into_srgb() {
        let folder = crate::state::scratch("p3");
        let path = folder.join("p3.png");
        p3_strip(&path, &[[128, 128, 128], [204, 77, 51], [255, 0, 0]]);
        let image = decode(&path).unwrap();
        let raw = image.as_raw();
        assert_eq!(image.dimensions(), (3, 1));
        assert!(near(&raw[0..3], [128, 128, 128], 2), "{:?}", &raw[0..3]);
        assert!(near(&raw[3..6], [221, 64, 37], 4), "{:?}", &raw[3..6]);
        assert!(raw[6] >= 250 && raw[7] <= 20 && raw[8] <= 20, "{:?}", &raw[6..9]);
    }

    #[test]
    fn the_srgb_profile_is_available_to_embed() {
        let profile = profile().unwrap();
        assert!(profile.len() > 128);
        assert_eq!(&profile[36..40], b"acsp");
    }

    #[test]
    fn packing_drops_the_fourth_byte_in_place() {
        let mut canvas = vec![1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0];
        pack(&mut canvas);
        assert_eq!(canvas, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
