use super::com;
use crate::live::helper::{Platform, Waker};
use crate::live::Hints;
use crate::platform::Screen;
use anyhow::{bail, Context, Result};
use scooch_core::RgbImage;
use scooch_live::Target;
use std::path::Path;
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ::windows::core::{w, PCWSTR};
use ::windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use ::windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, MapWindowPoints, ValidateRect, DEVMODEW, ENUM_CURRENT_SETTINGS};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::System::Threading::GetCurrentThreadId;
use ::windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetClientRect, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsWindow, IsWindowVisible, MsgWaitForMultipleObjects, PeekMessageW, PostThreadMessageW, RegisterClassExW, SendMessageTimeoutW, SetLayeredWindowAttributes, SetParent,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, EVENT_OBJECT_CREATE, EVENT_OBJECT_REORDER, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, HTTRANSPARENT, LWA_ALPHA, MA_NOACTIVATE, MSG, PM_NOREMOVE,
    PM_REMOVE, QS_ALLINPUT, SMTO_NORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNA, WINDOW_EX_STYLE, WINEVENT_OUTOFCONTEXT, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCHITTEST,
    WM_NULL, WM_PAINT, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

pub const CLASS: PCWSTR = w!("ScoochLayer");
const SPAWN_MESSAGE: u32 = 0x052C;
const LOOK_AGAIN: Duration = Duration::from_millis(100);
const LOOK_FOR: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    Bare,
    Window,
    Visual,
}

impl Method {
    pub const ALL: [Method; 3] = [Method::Bare, Method::Window, Method::Visual];

    pub fn hint(self) -> &'static str {
        match self {
            Method::Bare => "bare",
            Method::Window => "window",
            Method::Visual => "visual",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Method::Bare => "wgpu DirectComposition visual on a window without a redirection bitmap",
            Method::Window => "wgpu surface from the window",
            Method::Visual => "wgpu surface through a DirectComposition visual",
        }
    }

    pub fn from_hints(hints: &Hints) -> Result<Method> {
        match hints.present.as_deref() {
            None | Some("bare") => Ok(Method::Bare),
            Some("window") => Ok(Method::Window),
            Some("visual") => Ok(Method::Visual),
            Some(other) => bail!("the live layer has no presentation called {other}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    InProgman { progman: isize, icons: isize, wallpaper: isize },
    InWorkerW { progman: isize, icons: isize, wallpaper: isize },
    ProgmanOnly { progman: isize, icons: isize },
}

impl Layout {
    pub fn describe(&self) -> &'static str {
        match self {
            Layout::InProgman { .. } => "icons and the wallpaper WorkerW are both children of Progman",
            Layout::InWorkerW { .. } => "icons live in a top level WorkerW with the wallpaper WorkerW behind it",
            Layout::ProgmanOnly { .. } => "icons are a child of Progman and Progman paints the wallpaper itself, with no WorkerW",
        }
    }

    fn parent(&self) -> isize {
        match self {
            Layout::InProgman { progman, .. } | Layout::ProgmanOnly { progman, .. } => *progman,
            Layout::InWorkerW { wallpaper, .. } => *wallpaper,
        }
    }

    fn above(&self) -> Option<isize> {
        match self {
            Layout::InProgman { icons, .. } | Layout::ProgmanOnly { icons, .. } => Some(*icons),
            Layout::InWorkerW { .. } => None,
        }
    }

    pub fn explorer(&self) -> u32 {
        let progman = match self {
            Layout::InProgman { progman, .. } | Layout::InWorkerW { progman, .. } | Layout::ProgmanOnly { progman, .. } => *progman,
        };
        let mut process = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd(progman), Some(&mut process)) };
        process
    }
}

static REPAIRS: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static ORDERED: RefCell<Vec<(isize, isize)>> = const { RefCell::new(Vec::new()) };
}

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

pub fn class_of(window: HWND) -> String {
    let mut buffer = [0u16; 128];
    let length = unsafe { GetClassNameW(window, &mut buffer) }.clamp(0, 128) as usize;
    String::from_utf16_lossy(&buffer[..length])
}

fn children(parent: HWND) -> Vec<HWND> {
    let mut found = Vec::new();
    let mut next = unsafe { GetWindow(parent, GW_CHILD) }.ok();
    while let Some(child) = next {
        found.push(child);
        next = unsafe { GetWindow(child, GW_HWNDNEXT) }.ok();
    }
    found
}

fn child_of_class(parent: HWND, class: &str) -> Option<HWND> {
    children(parent).into_iter().find(|child| class_of(*child) == class)
}

fn top_level() -> Vec<HWND> {
    unsafe extern "system" fn each(window: HWND, data: LPARAM) -> ::windows::core::BOOL {
        let found = unsafe { &mut *(data.0 as *mut Vec<isize>) };
        found.push(window.0 as isize);
        true.into()
    }
    let mut found: Vec<isize> = Vec::new();
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut found as *mut Vec<isize> as isize)) };
    found.into_iter().map(hwnd).collect()
}

fn describe(window: HWND) -> String {
    let mut frame = RECT::default();
    let _ = unsafe { GetWindowRect(window, &mut frame) };
    let visible = unsafe { IsWindowVisible(window) }.as_bool();
    format!("{} {:#x} visible {visible} at {},{} {}x{}", class_of(window), window.0 as isize, frame.left, frame.top, frame.right - frame.left, frame.bottom - frame.top)
}

pub fn tree() -> Vec<String> {
    let mut lines = Vec::new();
    match unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) } {
        Ok(progman) => {
            lines.push(describe(progman));
            for child in children(progman) {
                lines.push(format!("  {}", describe(child)));
                for grandchild in children(child) {
                    lines.push(format!("    {}", describe(grandchild)));
                }
            }
        }
        Err(error) => lines.push(format!("no Progman {error}")),
    }
    for window in top_level().into_iter().filter(|window| class_of(*window) == "WorkerW") {
        lines.push(format!("top level {}", describe(window)));
        for child in children(window) {
            lines.push(format!("  {}", describe(child)));
        }
    }
    lines
}

fn icons_host() -> Option<HWND> {
    top_level().into_iter().find(|window| class_of(*window) == "WorkerW" && child_of_class(*window, "SHELLDLL_DefView").is_some())
}

fn wallpaper_behind(host: HWND) -> Option<HWND> {
    let mut next = unsafe { GetWindow(host, GW_HWNDNEXT) }.ok();
    while let Some(window) = next {
        let class = class_of(window);
        if class == "Progman" {
            return None;
        }
        if class == "WorkerW" && child_of_class(window, "SHELLDLL_DefView").is_none() && unsafe { IsWindowVisible(window) }.as_bool() {
            return Some(window);
        }
        next = unsafe { GetWindow(window, GW_HWNDNEXT) }.ok();
    }
    None
}

fn seen(progman: HWND) -> Option<Layout> {
    if let Some(icons) = child_of_class(progman, "SHELLDLL_DefView") {
        let wallpaper = child_of_class(progman, "WorkerW")?;
        return Some(Layout::InProgman { progman: progman.0 as isize, icons: icons.0 as isize, wallpaper: wallpaper.0 as isize });
    }
    let host = icons_host()?;
    let wallpaper = wallpaper_behind(host)?;
    Some(Layout::InWorkerW { progman: progman.0 as isize, icons: host.0 as isize, wallpaper: wallpaper.0 as isize })
}

fn spawn(progman: HWND, wparam: usize, lparam: isize, notes: &mut Vec<String>) -> Option<Layout> {
    let mut result = 0usize;
    let sent = unsafe { SendMessageTimeoutW(progman, SPAWN_MESSAGE, WPARAM(wparam), LPARAM(lparam), SMTO_NORMAL, 1000, Some(&mut result)) };
    let started = Instant::now();
    while started.elapsed() < LOOK_FOR {
        if let Some(found) = seen(progman) {
            notes.push(format!("sent 0x052C with {wparam:#x} and {lparam:#x}, returned {} with result {result}, a wallpaper WorkerW appeared after {} ms", sent.0, started.elapsed().as_millis()));
            return Some(found);
        }
        std::thread::sleep(LOOK_AGAIN);
    }
    notes.push(format!("sent 0x052C with {wparam:#x} and {lparam:#x}, returned {} with result {result}, no wallpaper WorkerW within two seconds", sent.0));
    None
}

pub fn layout() -> Result<(Layout, Vec<String>)> {
    let progman = unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.context("Explorer has no Progman window")?;
    let mut notes = Vec::new();
    if let Some(found) = seen(progman) {
        return Ok((found, notes));
    }
    if let Some(found) = spawn(progman, 0xD, 0x1, &mut notes) {
        return Ok((found, notes));
    }
    let icons = child_of_class(progman, "SHELLDLL_DefView").context("the desktop icons are neither in Progman nor in a WorkerW")?;
    notes.push("attaching under Progman directly, below the icons".into());
    Ok((Layout::ProgmanOnly { progman: progman.0 as isize, icons: icons.0 as isize }, notes))
}

fn in_place(ours: HWND, icons: HWND) -> bool {
    unsafe { GetWindow(icons, GW_HWNDNEXT) }.is_ok_and(|next| next == ours)
}

unsafe extern "system" fn reordered(_hook: HWINEVENTHOOK, _kind: u32, _window: HWND, _object: i32, _child: i32, _thread: u32, _time: u32) {
    let pairs = ORDERED.with(|ordered| ordered.borrow().clone());
    for (ours, icons) in pairs {
        if !unsafe { IsWindow(Some(hwnd(ours))) }.as_bool() || !unsafe { IsWindow(Some(hwnd(icons))) }.as_bool() {
            continue;
        }
        if !in_place(hwnd(ours), hwnd(icons)) {
            let _ = unsafe { SetWindowPos(hwnd(ours), Some(hwnd(icons)), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
            REPAIRS.fetch_add(1, Ordering::SeqCst);
        }
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let _ = unsafe { ValidateRect(Some(window), None) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn frame_in_parent(screen: &Screen, parent: HWND) -> RECT {
    let mut corners = [POINT { x: screen.frame.x as i32, y: screen.frame.y as i32 }, POINT { x: (screen.frame.x + screen.frame.w) as i32, y: (screen.frame.y + screen.frame.h) as i32 }];
    unsafe { MapWindowPoints(None, Some(parent), &mut corners) };
    RECT { left: corners[0].x, top: corners[0].y, right: corners[1].x, bottom: corners[1].y }
}

fn make_window(layout: Layout, area: RECT, method: Method) -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
    let mut extended = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
    if method == Method::Bare {
        extended |= WS_EX_NOREDIRECTIONBITMAP;
    }
    let (width, height) = (area.right - area.left, area.bottom - area.top);
    let window = unsafe { CreateWindowExW(WINDOW_EX_STYLE(extended.0), CLASS, w!("Scooch layer"), WS_POPUP, area.left, area.top, width, height, None, None, Some(instance.into()), None) }?;
    let built = (|| -> Result<()> {
        unsafe { SetLayeredWindowAttributes(window, COLORREF(0), 255, LWA_ALPHA) }?;
        unsafe { SetParent(window, Some(hwnd(layout.parent()))) }?;
        let style = unsafe { GetWindowLongPtrW(window, GWL_STYLE) };
        unsafe { SetWindowLongPtrW(window, GWL_STYLE, (style & !(WS_POPUP.0 as isize)) | WS_CHILD.0 as isize) };
        let after = layout.above().map(hwnd);
        unsafe { SetWindowPos(window, after, area.left, area.top, width, height, SWP_NOACTIVATE | SWP_FRAMECHANGED) }?;
        if method == Method::Bare {
            let _ = unsafe { ShowWindow(window, SW_SHOWNA) };
        }
        Ok(())
    })();
    if let Err(error) = built {
        let _ = unsafe { DestroyWindow(window) };
        return Err(error);
    }
    Ok(window)
}

pub fn repairs() -> u32 {
    REPAIRS.load(Ordering::SeqCst)
}

pub struct Host {
    thread: u32,
    instance: isize,
}

pub struct Window {
    window: HWND,
    icons: Option<isize>,
    hook: HWINEVENTHOOK,
    method: Method,
}

impl Host {
    pub fn new() -> Result<Host> {
        com::aware();
        let mut message = MSG::default();
        let _ = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
        let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
        let class = WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: instance.into(), lpszClassName: CLASS, ..Default::default() };
        unsafe { RegisterClassExW(&class) };
        Ok(Host { thread: unsafe { GetCurrentThreadId() }, instance: instance.0 as isize })
    }
}

impl Platform for Host {
    type Layer = Window;

    fn composition(&self, hints: &Hints) -> Result<bool> {
        Ok(Method::from_hints(hints)? != Method::Window)
    }

    fn open(&mut self, screen: &Screen, hints: &Hints) -> Result<Window> {
        let method = Method::from_hints(hints)?;
        let (layout, _) = layout()?;
        let area = frame_in_parent(screen, hwnd(layout.parent()));
        let window = make_window(layout, area, method)?;
        let icons = layout.above();
        if let Some(icons) = icons {
            ORDERED.with(|ordered| ordered.borrow_mut().push((window.0 as isize, icons)));
        }
        let hook = unsafe { SetWinEventHook(EVENT_OBJECT_CREATE, EVENT_OBJECT_REORDER, None, Some(reordered), layout.explorer(), 0, WINEVENT_OUTOFCONTEXT) };
        Ok(Window { window, icons, hook, method })
    }

    fn target(&self, layer: &Window) -> Target {
        Target::Window { window: layer.window.0 as isize, instance: self.instance, composition: layer.method != Method::Window }
    }

    fn pixels(&self, layer: &Window) -> (u32, u32) {
        let mut client = RECT::default();
        let _ = unsafe { GetClientRect(layer.window, &mut client) };
        ((client.right - client.left).max(1) as u32, (client.bottom - client.top).max(1) as u32)
    }

    fn show(&mut self, layer: &Window) -> Result<()> {
        if layer.method != Method::Bare {
            let _ = unsafe { ShowWindow(layer.window, SW_SHOWNA) };
        }
        if !unsafe { IsWindowVisible(layer.window) }.as_bool() {
            bail!("the layer window is not visible");
        }
        if let Some(icons) = layer.icons {
            if !in_place(layer.window, hwnd(icons)) {
                let _ = unsafe { SetWindowPos(layer.window, Some(hwnd(icons)), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
                if !in_place(layer.window, hwnd(icons)) {
                    bail!("the layer could not be ordered under the icons");
                }
            }
        }
        Ok(())
    }

    fn close(&mut self, layer: Window) {
        if !layer.hook.is_invalid() {
            let _ = unsafe { UnhookWinEvent(layer.hook) };
        }
        ORDERED.with(|ordered| ordered.borrow_mut().retain(|(ours, _)| *ours != layer.window.0 as isize));
        let _ = unsafe { ShowWindow(layer.window, SW_HIDE) };
        let _ = unsafe { DestroyWindow(layer.window) };
    }

    fn period(&self, _layer: &Window) -> Duration {
        let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        let read = unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool();
        let rate = if read && mode.dmDisplayFrequency > 1 { mode.dmDisplayFrequency } else { 60 };
        Duration::from_secs_f64(1.0 / f64::from(rate))
    }

    fn decode(path: &Path) -> Result<RgbImage> {
        super::picture::decode(path)
    }

    fn wait(&mut self, timeout: Duration) {
        let started = Instant::now();
        unsafe { MsgWaitForMultipleObjects(None, false, timeout.as_millis().min(u128::from(u32::MAX)) as u32, QS_ALLINPUT) };
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            let _ = unsafe { TranslateMessage(&message) };
            unsafe { DispatchMessageW(&message) };
            if started.elapsed() > timeout {
                break;
            }
        }
    }

    fn waker(&self) -> Waker {
        let thread = self.thread;
        Arc::new(move || {
            let _ = unsafe { PostThreadMessageW(thread, WM_NULL, WPARAM(0), LPARAM(0)) };
        })
    }
}

pub fn layers() -> Vec<isize> {
    unsafe extern "system" fn each(window: HWND, data: LPARAM) -> ::windows::core::BOOL {
        let found = unsafe { &mut *(data.0 as *mut Vec<isize>) };
        if class_of(window) == "ScoochLayer" {
            found.push(window.0 as isize);
        }
        true.into()
    }
    let mut found: Vec<isize> = Vec::new();
    let mut parents: Vec<HWND> = top_level().into_iter().filter(|window| class_of(*window) == "WorkerW").collect();
    parents.extend(unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok());
    for parent in parents {
        let _ = unsafe { EnumChildWindows(Some(parent), Some(each), LPARAM(&mut found as *mut Vec<isize> as isize)) };
    }
    if let Ok(top) = unsafe { FindWindowExW(None, None, CLASS, PCWSTR::null()) } {
        found.push(top.0 as isize);
    }
    found.sort_unstable();
    found.dedup();
    found
}

pub fn ours_left() -> usize {
    layers().len()
}

pub fn under_icons(window: isize) -> bool {
    let Ok(progman) = (unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }) else { return false };
    match seen(progman) {
        Some(Layout::InProgman { icons, .. }) | Some(Layout::ProgmanOnly { icons, .. }) => in_place(hwnd(window), hwnd(icons)),
        Some(Layout::InWorkerW { wallpaper, .. }) => children(hwnd(wallpaper)).contains(&hwnd(window)),
        None => false,
    }
}

pub fn serve() -> std::process::ExitCode {
    match Host::new() {
        Ok(host) => crate::live::helper::serve(host),
        Err(error) => crate::live::helper::refuse(&format!("the live layer could not start {error:#}")),
    }
}
