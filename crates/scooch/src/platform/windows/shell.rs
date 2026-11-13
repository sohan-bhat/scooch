use super::com::{self, take, Pidl};
use super::layout::{self, Cell, Label, Spacing, SPACING};
use anyhow::{bail, Context, Result};
use scooch_core::Rect;
use std::ffi::c_void;
use std::mem::{size_of, ManuallyDrop};
use ::windows::core::{w, Interface, PCWSTR, PWSTR};
use ::windows::Win32::Foundation::{HWND, POINT, RECT, S_OK};
use ::windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, DrawTextW, GetDC, GetTextMetricsW, MapWindowPoints, ReleaseDC, SelectObject, DT_CALCRECT, DT_CENTER, DT_NOPREFIX, DT_WORDBREAK, HDC, HFONT, HGDIOBJ,
    LOGFONTW, TEXTMETRICW,
};
use ::windows::Win32::System::Com::{CoCreateInstance, IServiceProvider, CLSCTX_ALL};
use ::windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
use ::windows::Win32::UI::Shell::Common::STRRET;
use ::windows::Win32::UI::Shell::{
    IEnumIDList, IFolderView2, IShellBrowser, IShellFolder, IShellView, IShellWindows, ShellWindows, StrRetToStrW, CSIDL_DESKTOP, FOLDERVIEWMODE, SHGDN_NORMAL, SID_STopLevelBrowser, SVGIO_ALLVIEW, SWC_DESKTOP,
    SWFO_NEEDDISPATCH,
};
use ::windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use ::windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetShellWindow, IsWindowVisible, SystemParametersInfoW, SPI_GETICONTITLELOGFONT, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};

pub const UNREADABLE: &str = "Scooch could not read the desktop icons, so it will try again at the next change.";
const BATCH: usize = 64;

pub struct View {
    pub folder: IFolderView2,
    pub shell: IShellView,
    pub host: HWND,
    pub list: HWND,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    pub at: POINT,
}

pub fn explorer() -> bool {
    !unsafe { GetShellWindow() }.is_invalid()
}

pub fn desktop_place() -> VARIANT {
    let inner = VARIANT_0_0 { vt: VT_I4, Anonymous: VARIANT_0_0_0 { lVal: CSIDL_DESKTOP as i32 }, ..Default::default() };
    VARIANT { Anonymous: VARIANT_0 { Anonymous: ManuallyDrop::new(inner) } }
}

pub fn view() -> Result<View> {
    com::ready()?;
    let windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }?;
    let place = desktop_place();
    let root = VARIANT::default();
    let mut handle = 0i32;
    let dispatch = unsafe { windows.FindWindowSW(&place, &root, SWC_DESKTOP, &mut handle, SWFO_NEEDDISPATCH) }?;
    let provider: IServiceProvider = dispatch.cast()?;
    let browser: IShellBrowser = unsafe { provider.QueryService(&SID_STopLevelBrowser) }?;
    let shell = unsafe { browser.QueryActiveShellView() }?;
    let folder: IFolderView2 = shell.cast()?;
    let host = unsafe { shell.GetWindow() }?;
    let list = unsafe { FindWindowExW(Some(host), None, w!("SysListView32"), PCWSTR::null()) }?;
    Ok(View { folder, shell, host, list })
}

pub fn visible(view: &View) -> bool {
    unsafe { IsWindowVisible(view.list) }.as_bool()
}

pub fn mode(view: &View) -> Result<(FOLDERVIEWMODE, i32)> {
    let mut mode = FOLDERVIEWMODE::default();
    let mut icon = 0i32;
    unsafe { view.folder.GetViewModeAndIconSize(&mut mode, &mut icon) }?;
    Ok((mode, icon))
}

pub fn spacing(view: &View) -> Result<POINT> {
    let mut spacing = POINT::default();
    unsafe { view.folder.GetSpacing(&mut spacing) }?;
    Ok(spacing)
}

pub fn cell(view: &View, line: i32) -> Result<Cell> {
    let (_, icon) = mode(view)?;
    let spacing = spacing(view)?;
    Ok(Cell { width: spacing.x as f32, icon: icon as f32, line: line as f32 })
}

pub fn pidls(view: &View) -> Result<Vec<Pidl>> {
    let list: IEnumIDList = unsafe { view.folder.Items(SVGIO_ALLVIEW) }?;
    let mut found = Vec::new();
    loop {
        let mut batch = [std::ptr::null_mut(); BATCH];
        let mut fetched = 0u32;
        let result = unsafe { list.Next(&mut batch, Some(&mut fetched)) };
        let fetched = (fetched as usize).min(BATCH);
        found.extend(batch.iter().take(fetched).map(|pidl| Pidl(*pidl)));
        if result != S_OK || fetched == 0 {
            break;
        }
    }
    Ok(found)
}

pub fn name(folder: &IShellFolder, pidl: &Pidl) -> String {
    let mut name = STRRET::default();
    if unsafe { folder.GetDisplayNameOf(pidl.0, SHGDN_NORMAL, &mut name) }.is_err() {
        return String::new();
    }
    let mut text = PWSTR::null();
    if unsafe { StrRetToStrW(&mut name, Some(pidl.0), &mut text) }.is_err() {
        return String::new();
    }
    take(text).to_string_lossy().into_owned()
}

pub fn items(view: &View) -> Result<Vec<Item>> {
    let folder: IShellFolder = unsafe { view.folder.GetFolder() }?;
    let pidls = pidls(view)?;
    let mut names = Vec::with_capacity(pidls.len());
    let mut points = Vec::with_capacity(pidls.len());
    for pidl in &pidls {
        points.push(unsafe { view.folder.GetItemPosition(pidl.0) }?);
        names.push(name(&folder, pidl));
    }
    if !points.is_empty() {
        unsafe { MapWindowPoints(Some(view.list), None, &mut points) };
    }
    Ok(names.into_iter().zip(points).map(|(name, at)| Item { name, at }).collect())
}

pub struct Font {
    dc: HDC,
    font: HFONT,
    old: HGDIOBJ,
    pub line: i32,
}

impl Font {
    pub fn open(view: &View) -> Result<Font> {
        Self::for_dpi(unsafe { GetDpiForWindow(view.list) })
    }

    pub fn for_dpi(dpi: u32) -> Result<Font> {
        let mut logical = LOGFONTW::default();
        let pointer = Some(&mut logical as *mut LOGFONTW as *mut c_void);
        let size = size_of::<LOGFONTW>() as u32;
        if dpi == 0 || unsafe { SystemParametersInfoForDpi(SPI_GETICONTITLELOGFONT.0, size, pointer, 0, dpi) }.is_err() {
            unsafe { SystemParametersInfoW(SPI_GETICONTITLELOGFONT, size, Some(&mut logical as *mut LOGFONTW as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }?;
        }
        let font = unsafe { CreateFontIndirectW(&logical) };
        if font.is_invalid() {
            bail!("Windows has no icon title font.");
        }
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            let _ = unsafe { DeleteObject(font.into()) };
            bail!("Windows gave Scooch no screen to measure text on.");
        }
        let old = unsafe { SelectObject(dc, font.into()) };
        let mut metrics = TEXTMETRICW::default();
        let measured = unsafe { GetTextMetricsW(dc, &mut metrics) }.as_bool();
        let opened = Font { dc, font, old, line: metrics.tmHeight };
        if !measured || metrics.tmHeight <= 0 {
            bail!("Windows could not measure the icon title font.");
        }
        Ok(opened)
    }

    pub fn measure(&self, name: &str, wrap: i32) -> Label {
        let mut text: Vec<u16> = name.encode_utf16().collect();
        let mut room = RECT { left: 0, top: 0, right: wrap.max(1), bottom: 0 };
        if !text.is_empty() {
            unsafe { DrawTextW(self.dc, &mut text, &mut room, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX | DT_CENTER) };
        }
        Label { width: (room.right - room.left).max(0) as f32, height: (room.bottom - room.top).max(self.line) as f32 }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe { SelectObject(self.dc, self.old) };
        let _ = unsafe { DeleteObject(self.font.into()) };
        unsafe { ReleaseDC(None, self.dc) };
    }
}

pub fn rects(items: &[Item], cell: &Cell, font: &Font, spacing: &Spacing) -> Vec<Rect> {
    let wrap = cell.wrap(spacing).round() as i32;
    items.iter().map(|item| layout::item_rect((item.at.x as f32, item.at.y as f32), cell, font.measure(&item.name, wrap), spacing)).collect()
}

pub fn icons() -> Result<Vec<Rect>> {
    if !explorer() {
        return Ok(Vec::new());
    }
    let view = view().context(UNREADABLE)?;
    if !visible(&view) {
        return Ok(Vec::new());
    }
    let items = items(&view).context(UNREADABLE)?;
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let font = Font::open(&view).context(UNREADABLE)?;
    let cell = cell(&view, font.line).context(UNREADABLE)?;
    Ok(rects(&items, &cell, &font, &SPACING))
}
