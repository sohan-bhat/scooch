use super::com;
use crate::platform::Screen;
use anyhow::{anyhow, bail, Context, Result};
use scooch_live::{Pattern, Renderer, Target};
use std::cell::Cell;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use ::windows::core::{w, Interface, PCWSTR};
use ::windows::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use ::windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use ::windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
use ::windows::Win32::Graphics::Dwm::DwmFlush;
use ::windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_DESC, DXGI_SAMPLE_DESC};
use ::windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIFactory, IDXGISwapChain, DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT};
use ::windows::Win32::Graphics::Gdi::{GetDC, MapWindowPoints, ReleaseDC, SetDIBitsToDevice, ValidateRect, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetMessageW, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsWindow, IsWindowVisible, PostMessageW, PostQuitMessage, RegisterClassExW, SendMessageTimeoutW, SetLayeredWindowAttributes, SetParent, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, EVENT_OBJECT_CREATE, EVENT_OBJECT_REORDER, GWL_STYLE, GW_CHILD, GW_HWNDNEXT, HTTRANSPARENT, LWA_ALPHA, MA_NOACTIVATE, MSG, SMTO_NORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNA, WINDOW_EX_STYLE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_DESTROY, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

pub const CLASS: PCWSTR = w!("ScoochLayer");
const CLOSE: u32 = WM_APP + 40;
const SPAWN_MESSAGE: u32 = 0x052C;
const LOOK_AGAIN: Duration = Duration::from_millis(100);
const LOOK_FOR: Duration = Duration::from_secs(2);
const READY_WAIT: Duration = Duration::from_secs(10);
const BAR_HALF_WIDTH: i32 = 20;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    Hwnd,
    Visual,
    VisualNoRedirection,
    Copy,
    Gdi,
}

impl Method {
    pub const ALL: [Method; 5] = [Method::Hwnd, Method::Visual, Method::VisualNoRedirection, Method::Copy, Method::Gdi];

    pub fn name(self) -> &'static str {
        match self {
            Method::Hwnd => "wgpu surface from the window",
            Method::Visual => "wgpu surface through a DirectComposition visual",
            Method::VisualNoRedirection => "wgpu DirectComposition visual on a window without a redirection bitmap",
            Method::Copy => "Direct3D 11 swap chain in copy mode",
            Method::Gdi => "GDI from a CPU buffer paced by DwmFlush",
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

#[derive(Debug, Default)]
pub struct Stats {
    pub first: Option<Duration>,
    pub presents: Vec<Duration>,
    pub adapter: String,
    pub error: Option<String>,
}

pub struct Layer {
    pub window: isize,
    pub layout: Layout,
    pub notes: Vec<String>,
    pub stats: Arc<Mutex<Stats>>,
    stop: Arc<AtomicBool>,
    window_thread: Option<(JoinHandle<()>, mpsc::Receiver<()>)>,
    render_thread: Option<(JoinHandle<()>, mpsc::Receiver<()>)>,
}

const STOP_WAIT: Duration = Duration::from_secs(3);

fn finish(thread: Option<(JoinHandle<()>, mpsc::Receiver<()>)>) -> bool {
    let Some((handle, done)) = thread else { return true };
    match done.recv_timeout(STOP_WAIT) {
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = handle.join();
            true
        }
        Err(mpsc::RecvTimeoutError::Timeout) => false,
    }
}

static REPAIRS: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static OURS: Cell<isize> = const { Cell::new(0) };
    static ICONS: Cell<isize> = const { Cell::new(0) };
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
    for (wparam, lparam) in [(0usize, 0isize), (0xD, 0x1)] {
        if let Some(found) = spawn(progman, wparam, lparam, &mut notes) {
            return Ok((found, notes));
        }
        if child_of_class(progman, "SHELLDLL_DefView").is_none() && icons_host().is_some() {
            break;
        }
    }
    let icons = child_of_class(progman, "SHELLDLL_DefView").context("the desktop icons are neither in Progman nor in a WorkerW")?;
    notes.push("attaching under Progman directly, below the icons".into());
    Ok((Layout::ProgmanOnly { progman: progman.0 as isize, icons: icons.0 as isize }, notes))
}

fn in_place(ours: HWND, icons: HWND) -> bool {
    unsafe { GetWindow(icons, GW_HWNDNEXT) }.is_ok_and(|next| next == ours)
}

unsafe extern "system" fn reordered(_hook: HWINEVENTHOOK, _kind: u32, _window: HWND, _object: i32, _child: i32, _thread: u32, _time: u32) {
    let (ours, icons) = (OURS.with(Cell::get), ICONS.with(Cell::get));
    if ours == 0 || icons == 0 || !unsafe { IsWindow(Some(hwnd(ours))) }.as_bool() {
        return;
    }
    if !in_place(hwnd(ours), hwnd(icons)) {
        let _ = unsafe { SetWindowPos(hwnd(ours), Some(hwnd(icons)), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
        REPAIRS.fetch_add(1, Ordering::SeqCst);
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
        CLOSE => {
            let _ = unsafe { ShowWindow(window, SW_HIDE) };
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
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

fn make_window(layout: Layout, area: RECT, no_redirection: bool) -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
    let class = WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: instance.into(), lpszClassName: CLASS, ..Default::default() };
    unsafe { RegisterClassExW(&class) };
    let mut extended = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
    if no_redirection {
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
        let _ = unsafe { ShowWindow(window, SW_SHOWNA) };
        Ok(())
    })();
    if let Err(error) = built {
        let _ = unsafe { DestroyWindow(window) };
        return Err(error);
    }
    Ok(window)
}

fn window_thread(layout: Layout, area: RECT, no_redirection: bool, ready: mpsc::Sender<Result<isize>>) {
    let window = match make_window(layout, area, no_redirection) {
        Ok(window) => window,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    OURS.with(|ours| ours.set(window.0 as isize));
    ICONS.with(|icons| icons.set(layout.above().unwrap_or(0)));
    let hook = unsafe { SetWinEventHook(EVENT_OBJECT_CREATE, EVENT_OBJECT_REORDER, None, Some(reordered), layout.explorer(), 0, WINEVENT_OUTOFCONTEXT) };
    let _ = ready.send(Ok(window.0 as isize));
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
    if !hook.is_invalid() {
        let _ = unsafe { UnhookWinEvent(hook) };
    }
    OURS.with(|ours| ours.set(0));
}

pub fn bgra(pattern: &Pattern) -> Vec<u8> {
    let mut out = pattern.rgba.clone();
    for pixel in out.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    out
}

pub fn paint(base: &[u8], width: i32, height: i32, bar: Option<f32>, frame: &mut Vec<u8>) {
    frame.clear();
    frame.extend_from_slice(base);
    let stride = width as usize * 4;
    if let Some(bar) = bar {
        let center = (bar * width as f32) as i32;
        let (left, right) = ((center - BAR_HALF_WIDTH).max(0), (center + BAR_HALF_WIDTH).min(width));
        for y in 0..height as usize {
            for x in left..right {
                let at = y * stride + x as usize * 4;
                frame[at..at + 4].copy_from_slice(&[255, 0, 255, 255]);
            }
        }
    }
    for y in 9..24usize.min(height as usize) {
        for x in (width - 23).max(0)..(width - 8).max(0) {
            let at = y * stride + x as usize * 4;
            frame[at..at + 4].copy_from_slice(&[0, 255, 0, 255]);
        }
    }
}

trait Presenter {
    fn present(&mut self, bar: Option<f32>) -> Result<()>;
    fn adapter(&self) -> String;
}

struct Gpu(Renderer);

impl Presenter for Gpu {
    fn present(&mut self, bar: Option<f32>) -> Result<()> {
        self.0.frame(bar).map_err(|error| anyhow!(error))
    }

    fn adapter(&self) -> String {
        self.0.adapter.clone()
    }
}

struct Gdi {
    window: HWND,
    width: i32,
    height: i32,
    base: Vec<u8>,
    frame: Vec<u8>,
}

impl Presenter for Gdi {
    fn present(&mut self, bar: Option<f32>) -> Result<()> {
        paint(&self.base, self.width, self.height, bar, &mut self.frame);
        let header = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: self.width, biHeight: -self.height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() };
        let info = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let dc = unsafe { GetDC(Some(self.window)) };
        if dc.is_invalid() {
            bail!("GetDC on the layer returned nothing");
        }
        let lines = unsafe { SetDIBitsToDevice(dc, 0, 0, self.width as u32, self.height as u32, 0, 0, 0, self.height as u32, self.frame.as_ptr() as *const c_void, &info, DIB_RGB_COLORS) };
        unsafe { ReleaseDC(Some(self.window), dc) };
        if lines == 0 {
            bail!("SetDIBitsToDevice copied no lines");
        }
        unsafe { DwmFlush() }.context("DwmFlush failed")?;
        Ok(())
    }

    fn adapter(&self) -> String {
        "GDI".into()
    }
}

struct Blit {
    context: ID3D11DeviceContext,
    chain: IDXGISwapChain,
    width: i32,
    height: i32,
    base: Vec<u8>,
    frame: Vec<u8>,
    driver: &'static str,
}

impl Blit {
    fn new(window: HWND, width: i32, height: i32, base: Vec<u8>) -> Result<Self> {
        let mut failures = Vec::new();
        for (driver, name) in [(D3D_DRIVER_TYPE_HARDWARE, "hardware"), (D3D_DRIVER_TYPE_WARP, "WARP")] {
            match Self::with(window, width, height, driver) {
                Ok((context, chain)) => return Ok(Self { context, chain, width, height, frame: Vec::with_capacity(base.len()), base, driver: name }),
                Err(error) => failures.push(format!("{name} {error}")),
            }
        }
        bail!("no Direct3D 11 swap chain, {}", failures.join(", "))
    }

    fn with(window: HWND, width: i32, height: i32, driver: D3D_DRIVER_TYPE) -> Result<(ID3D11DeviceContext, IDXGISwapChain)> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        unsafe { D3D11CreateDevice(None, driver, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)) }?;
        let device = device.context("no device")?;
        let context = context.context("no context")?;
        let factory: IDXGIFactory = unsafe { device.cast::<IDXGIDevice>()?.GetAdapter()?.GetParent() }?;
        let description = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC { Width: width as u32, Height: height as u32, Format: DXGI_FORMAT_B8G8R8A8_UNORM, ..Default::default() },
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 1,
            OutputWindow: window,
            Windowed: true.into(),
            SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
            Flags: 0,
        };
        let mut chain: Option<IDXGISwapChain> = None;
        unsafe { factory.CreateSwapChain(&device, &description, &mut chain) }.ok()?;
        Ok((context, chain.context("no swap chain")?))
    }
}

impl Presenter for Blit {
    fn present(&mut self, bar: Option<f32>) -> Result<()> {
        paint(&self.base, self.width, self.height, bar, &mut self.frame);
        let buffer: ID3D11Texture2D = unsafe { self.chain.GetBuffer(0) }?;
        unsafe { self.context.UpdateSubresource(&buffer, 0, None, self.frame.as_ptr() as *const c_void, self.width as u32 * 4, 0) };
        unsafe { self.chain.Present(1, DXGI_PRESENT(0)) }.ok()?;
        Ok(())
    }

    fn adapter(&self) -> String {
        format!("Direct3D 11 {}", self.driver)
    }
}

fn presenter(method: Method, window: isize, pattern: &Pattern) -> Result<Box<dyn Presenter>> {
    let (width, height) = (pattern.width as i32, pattern.height as i32);
    Ok(match method {
        Method::Hwnd | Method::Visual | Method::VisualNoRedirection => {
            let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
            let target = Target::Window { window, instance: instance.0 as isize, composition: method != Method::Hwnd };
            Box::new(Gpu(Renderer::new(target, pattern).map_err(|error| anyhow!(error))?))
        }
        Method::Copy => Box::new(Blit::new(hwnd(window), width, height, bgra(pattern))?),
        Method::Gdi => Box::new(Gdi { window: hwnd(window), width, height, base: bgra(pattern), frame: Vec::new() }),
    })
}

fn render_thread(method: Method, window: isize, pattern: Pattern, cycle: Duration, stop: Arc<AtomicBool>, stats: Arc<Mutex<Stats>>, ready: mpsc::Sender<Result<()>>) {
    let opened = Instant::now();
    let mut presenter = match presenter(method, window, &pattern) {
        Ok(presenter) => presenter,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    drop(pattern);
    if let Err(error) = presenter.present(Some(0.0)) {
        let _ = ready.send(Err(error));
        return;
    }
    let first = opened.elapsed();
    if let Ok(mut stats) = stats.lock() {
        stats.first = Some(first);
        stats.adapter = presenter.adapter();
        stats.presents.push(Duration::ZERO);
    }
    let _ = ready.send(Ok(()));
    let started = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let elapsed = started.elapsed();
        let bar = (elapsed.as_secs_f32() % cycle.as_secs_f32()) / cycle.as_secs_f32();
        let result = presenter.present(Some(bar));
        if let Ok(mut stats) = stats.lock() {
            match result {
                Ok(()) => stats.presents.push(started.elapsed()),
                Err(error) => {
                    stats.error.get_or_insert(error.to_string());
                }
            }
        }
    }
}

pub fn repairs() -> u32 {
    REPAIRS.load(Ordering::SeqCst)
}

pub fn open(screen: &Screen, method: Method, pattern: Pattern, cycle: Duration) -> Result<Layer> {
    com::aware();
    let (layout, notes) = layout()?;
    let area = frame_in_parent(screen, hwnd(layout.parent()));
    let (window_ready, window_wait) = mpsc::channel();
    let no_redirection = method == Method::VisualNoRedirection;
    let (window_done, window_finished) = mpsc::channel();
    let window_thread = std::thread::Builder::new().name("scooch-layer-window".into()).spawn(move || {
        window_thread(layout, area, no_redirection, window_ready);
        let _ = window_done.send(());
    })?;
    let window = match window_wait.recv_timeout(READY_WAIT) {
        Ok(Ok(window)) => window,
        Ok(Err(error)) => {
            finish(Some((window_thread, window_finished)));
            return Err(error.context("the layer window could not be made"));
        }
        Err(_) => bail!("the layer window took over ten seconds to appear"),
    };
    let stats = Arc::new(Mutex::new(Stats::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let (render_ready, render_wait) = mpsc::channel();
    let (render_stop, render_stats) = (stop.clone(), stats.clone());
    let (render_done, render_finished) = mpsc::channel();
    let render_thread = std::thread::Builder::new().name("scooch-layer-render".into()).spawn(move || {
        render_thread(method, window, pattern, cycle, render_stop, render_stats, render_ready);
        let _ = render_done.send(());
    });
    let mut layer = Layer { window, layout, notes, stats, stop, window_thread: Some((window_thread, window_finished)), render_thread: render_thread.ok().map(|handle| (handle, render_finished)) };
    match render_wait.recv_timeout(READY_WAIT) {
        Ok(Ok(())) => Ok(layer),
        Ok(Err(error)) => {
            layer.close();
            Err(error)
        }
        Err(_) => {
            layer.close();
            bail!("the first frame took over ten seconds")
        }
    }
}

pub fn ours_left() -> usize {
    unsafe extern "system" fn each(window: HWND, data: LPARAM) -> ::windows::core::BOOL {
        let count = unsafe { &mut *(data.0 as *mut usize) };
        if class_of(window) == "ScoochLayer" {
            *count += 1;
        }
        true.into()
    }
    let mut count = 0usize;
    let mut parents: Vec<HWND> = top_level().into_iter().filter(|window| class_of(*window) == "WorkerW").collect();
    parents.extend(unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok());
    for parent in parents {
        let _ = unsafe { EnumChildWindows(Some(parent), Some(each), LPARAM(&mut count as *mut usize as isize)) };
    }
    count + unsafe { FindWindowExW(None, None, CLASS, PCWSTR::null()) }.map_or(0, |_| 1)
}

impl Layer {
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let rendered = finish(self.render_thread.take());
        let _ = unsafe { PostMessageW(Some(hwnd(self.window)), CLOSE, WPARAM(0), LPARAM(0)) };
        let windowed = finish(self.window_thread.take());
        if !rendered || !windowed {
            if let Ok(mut stats) = self.stats.lock() {
                stats.error.get_or_insert(format!("a layer thread did not stop within three seconds, render {rendered} window {windowed}"));
            }
        }
    }

    pub fn in_place(&self) -> bool {
        match self.layout {
            Layout::InProgman { icons, .. } | Layout::ProgmanOnly { icons, .. } => in_place(hwnd(self.window), hwnd(icons)),
            Layout::InWorkerW { .. } => unsafe { IsWindow(Some(hwnd(self.window))) }.as_bool(),
        }
    }
}

impl Drop for Layer {
    fn drop(&mut self) {
        self.close();
    }
}
