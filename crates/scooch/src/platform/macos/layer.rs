use super::store::{convert, Plist};
use super::sys::{self, Ref};
use crate::platform::Screen;
use anyhow::{Context, Result};
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSBackingStoreType, NSColor, NSView, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowOcclusionState, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect};
use objc2_core_graphics::{kCGColorSpaceSRGB, CGColorSpace};
use objc2_quartz_core::{CAMetalLayer, CATransaction};
use scooch_core::Rect;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const NO_ROOM: &str = "macOS shows no wallpaper window below and icon window above the desktop level on this screen, so Scooch keeps the plain swap.";
pub const HIDDEN: &str = "macOS never reported the live layer as visible, so Scooch keeps the plain swap.";
const SHOW_WAIT: Duration = Duration::from_millis(400);

const ON_SCREEN: u32 = 1;
const EVERY_WINDOW: u32 = 0;
const DESKTOP_KEY: i32 = 2;
const ICON_KEY: i32 = 18;

#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub owner: String,
    pub pid: i64,
    pub layer: i64,
    pub number: i64,
    pub name: String,
    pub bounds: Rect,
    pub on_screen: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Smpte {
    subframes: i16,
    divisor: i16,
    counter: u32,
    kind: u32,
    flags: u32,
    hours: i16,
    minutes: i16,
    seconds: i16,
    frames: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TimeStamp {
    version: u32,
    scale: i32,
    video: i64,
    host: u64,
    rate: f64,
    period: i64,
    smpte: Smpte,
    flags: u64,
    reserved: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VideoTime {
    value: i64,
    scale: i32,
    flags: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Timebase {
    numer: u32,
    denom: u32,
}

type Output = extern "C" fn(*mut c_void, *const TimeStamp, *const TimeStamp, u64, *mut u64, *mut c_void) -> i32;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative: u32) -> Ref;
    fn CGWindowLevelForKey(key: i32) -> i32;
    fn CGMainDisplayID() -> u32;
}

pub fn main_display() -> u32 {
    unsafe { CGMainDisplayID() }
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVDisplayLinkCreateWithCGDisplay(display: u32, link: *mut *mut c_void) -> i32;
    fn CVDisplayLinkSetOutputCallback(link: *mut c_void, callback: Output, context: *mut c_void) -> i32;
    fn CVDisplayLinkStart(link: *mut c_void) -> i32;
    fn CVDisplayLinkStop(link: *mut c_void) -> i32;
    fn CVDisplayLinkRelease(link: *mut c_void);
    fn CVDisplayLinkGetNominalOutputVideoRefreshPeriod(link: *mut c_void) -> VideoTime;
}

extern "C" {
    fn mach_timebase_info(info: *mut Timebase) -> i32;
    fn mach_absolute_time() -> u64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: Ref;
    fn CFRunLoopRunInMode(mode: Ref, seconds: f64, after_source: u8) -> i32;
}

pub fn spin(seconds: f64) {
    unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 0) };
}

pub fn wait(timeout: Duration) {
    unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, timeout.as_secs_f64(), 1) };
}

extern "C" fn nothing(_context: *mut c_void) {}

pub fn nudge() {
    unsafe { super::sys::dispatch_async_f(super::sys::main_queue(), std::ptr::null_mut(), nothing) };
}

pub fn period(display: u32) -> Option<Duration> {
    let mut link: *mut c_void = std::ptr::null_mut();
    if unsafe { CVDisplayLinkCreateWithCGDisplay(display, &mut link) } != 0 || link.is_null() {
        return None;
    }
    let period = unsafe { CVDisplayLinkGetNominalOutputVideoRefreshPeriod(link) };
    unsafe { CVDisplayLinkRelease(link) };
    (period.value > 0 && period.scale > 0).then(|| Duration::from_secs_f64(period.value as f64 / period.scale as f64))
}

pub fn levels() -> (i64, i64) {
    unsafe { (CGWindowLevelForKey(DESKTOP_KEY) as i64, CGWindowLevelForKey(ICON_KEY) as i64) }
}

fn number(entry: &Plist, key: &str) -> f64 {
    entry.get(key).and_then(Plist::number).unwrap_or_default()
}

fn read(list: Ref, on_screen: bool) -> Vec<Seen> {
    if list.is_null() {
        return Vec::new();
    }
    let parsed = convert(unsafe { &*(list as *const AnyObject) });
    unsafe { sys::CFRelease(list) };
    parsed
        .items()
        .unwrap_or(&[])
        .iter()
        .map(|entry| {
            let bounds = entry.get("kCGWindowBounds");
            let edge = |key: &str| bounds.map_or(0.0, |bounds| number(bounds, key)) as f32;
            Seen {
                owner: entry.get("kCGWindowOwnerName").and_then(Plist::text).unwrap_or_default().to_string(),
                pid: number(entry, "kCGWindowOwnerPID") as i64,
                layer: number(entry, "kCGWindowLayer") as i64,
                number: number(entry, "kCGWindowNumber") as i64,
                name: entry.get("kCGWindowName").and_then(Plist::text).unwrap_or_default().to_string(),
                bounds: Rect { x: edge("X"), y: edge("Y"), w: edge("Width"), h: edge("Height") },
                on_screen: on_screen || entry.get("kCGWindowIsOnscreen").and_then(Plist::number).is_some_and(|value| value != 0.0),
            }
        })
        .collect()
}

pub fn listing() -> Vec<Seen> {
    read(unsafe { CGWindowListCopyWindowInfo(ON_SCREEN, 0) }, true)
}

pub fn every_window() -> Vec<Seen> {
    read(unsafe { CGWindowListCopyWindowInfo(EVERY_WINDOW, 0) }, false)
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

pub fn is_picture(window: &Seen) -> bool {
    window.owner == "Dock" || window.owner.contains("Wallpaper") || window.name.starts_with("Wallpaper")
}

pub fn choose(seen: &[Seen], screen: &Rect, desktop: i64, ours: i64) -> Option<i64> {
    let here = |window: &&Seen| window.pid != ours && overlaps(&window.bounds, screen);
    let picture = seen.iter().filter(here).any(|window| window.layer < desktop && is_picture(window));
    let icons = seen.iter().filter(here).any(|window| window.layer > desktop && window.owner == "Finder");
    (picture && icons).then_some(desktop)
}

pub struct Traces {
    window: Weak<NSWindow>,
    view: Weak<NSView>,
    metal: Weak<CAMetalLayer>,
}

fn held<T: Message>(weak: &Weak<T>) -> Option<usize> {
    weak.load().map(|object| {
        let count: usize = unsafe { objc2::msg_send![&*object, retainCount] };
        count.saturating_sub(1)
    })
}

impl Traces {
    pub fn held(&self) -> [Option<usize>; 3] {
        [held(&self.window), held(&self.view), held(&self.metal)]
    }
}

pub struct Layer {
    window: ManuallyDrop<Retained<NSWindow>>,
    view: ManuallyDrop<Retained<NSView>>,
    metal: ManuallyDrop<Retained<CAMetalLayer>>,
    pub level: i64,
    pub pixels: (u32, u32),
    pub display: u32,
}

impl Layer {
    pub fn surface(&self) -> *mut c_void {
        Retained::as_ptr(&self.metal) as *mut c_void
    }

    pub fn number(&self) -> i64 {
        self.window.windowNumber() as i64
    }

    pub fn traces(&self) -> Traces {
        Traces { window: Weak::from_retained(&self.window), view: Weak::from_retained(&self.view), metal: Weak::from_retained(&self.metal) }
    }

    pub fn close(self) {
        drop(self);
    }

    pub fn commit(&self) {
        CATransaction::flush();
    }

    pub fn reveal(&self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.metal.setHidden(false);
        CATransaction::commit();
        CATransaction::flush();
    }

    pub fn visible(&self) -> bool {
        self.window.occlusionState().contains(NSWindowOcclusionState::Visible)
    }

    pub fn shown_after(&self) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < SHOW_WAIT {
            if self.visible() {
                return Some(started.elapsed());
            }
            spin(0.01);
        }
        None
    }
}

impl Drop for Layer {
    fn drop(&mut self) {
        objc2::rc::autoreleasepool(|_| {
            self.window.orderOut(None);
            self.window.close();
            unsafe {
                ManuallyDrop::drop(&mut self.metal);
                ManuallyDrop::drop(&mut self.view);
                ManuallyDrop::drop(&mut self.window);
            }
            CATransaction::flush();
        });
    }
}

pub fn present<T>(work: impl FnOnce() -> T) -> T {
    objc2::rc::autoreleasepool(|_| work())
}

pub fn open(marker: MainThreadMarker, screen: &Screen) -> Result<Layer> {
    objc2::rc::autoreleasepool(|_| build(marker, screen))
}

fn build(marker: MainThreadMarker, screen: &Screen) -> Result<Layer> {
    let (display, number) = super::picture::screen_for(marker, &screen.id)?;
    let (desktop, _) = levels();
    let level = choose(&listing(), &screen.frame, desktop, std::process::id() as i64).context(NO_ROOM)?;
    let frame = display.frame();
    let scale = display.backingScaleFactor();
    let local = NSRect::new(NSPoint::new(0.0, 0.0), frame.size);
    let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer_screen(NSWindow::alloc(marker), local, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false, Some(&display)) };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setFrame_display(frame, false);
    window.setLevel(level as isize);
    window.setIgnoresMouseEvents(true);
    window.setHasShadow(false);
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setExcludedFromWindowsMenu(true);
    window.setCollectionBehavior(NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle | NSWindowCollectionBehavior::FullScreenNone);
    window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    let view = NSView::initWithFrame(NSView::alloc(marker), NSRect::new(NSPoint::new(0.0, 0.0), frame.size));
    let metal = CAMetalLayer::new();
    metal.setContentsScale(scale);
    metal.setOpaque(true);
    metal.setColorspace(CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).as_deref());
    metal.setHidden(true);
    view.setLayer(Some(&**metal));
    view.setWantsLayer(true);
    metal.setDelegate(None);
    window.setContentView(Some(&view));
    window.orderFrontRegardless();
    CATransaction::flush();
    let pixels = ((frame.size.width * scale).round() as u32, (frame.size.height * scale).round() as u32);
    let layer = Layer { window: ManuallyDrop::new(window), view: ManuallyDrop::new(view), metal: ManuallyDrop::new(metal), level, pixels, display: number };
    layer.shown_after().context(HIDDEN)?;
    Ok(layer)
}

pub fn seconds(host: u64) -> f64 {
    let mut base = Timebase::default();
    unsafe { mach_timebase_info(&mut base) };
    if base.denom == 0 {
        return host as f64 / 1e9;
    }
    host as f64 * base.numer as f64 / base.denom as f64 / 1e9
}

type Ticks = Mutex<Vec<(u64, u64)>>;

const HOST_VALID: u64 = 1 << 1;

extern "C" fn tick(_link: *mut c_void, _now: *const TimeStamp, output: *const TimeStamp, _flags: u64, _out: *mut u64, context: *mut c_void) -> i32 {
    if output.is_null() || context.is_null() {
        return 0;
    }
    let ticks = unsafe { &*(context as *const Ticks) };
    let arrived = unsafe { mach_absolute_time() };
    let (host, flags) = unsafe { ((*output).host, (*output).flags) };
    let host = if flags & HOST_VALID != 0 { host } else { 0 };
    if let Ok(mut ticks) = ticks.lock() {
        ticks.push((arrived, host));
    }
    0
}

pub struct Paced {
    pub arrivals: Vec<f64>,
    pub outputs: Vec<f64>,
    pub callbacks: usize,
}

pub struct Pace {
    link: *mut c_void,
    ticks: *mut Ticks,
    pub rate: f64,
}

impl Pace {
    pub fn start(display: u32) -> Option<Pace> {
        let mut link: *mut c_void = std::ptr::null_mut();
        if unsafe { CVDisplayLinkCreateWithCGDisplay(display, &mut link) } != 0 || link.is_null() {
            return None;
        }
        let period = unsafe { CVDisplayLinkGetNominalOutputVideoRefreshPeriod(link) };
        let rate = if period.value > 0 { period.scale as f64 / period.value as f64 } else { 0.0 };
        let ticks = Box::into_raw(Box::new(Mutex::new(Vec::new())));
        unsafe {
            CVDisplayLinkSetOutputCallback(link, tick, ticks.cast());
            CVDisplayLinkStart(link);
        }
        Some(Pace { link, ticks, rate })
    }

    pub fn stop(self) -> Paced {
        unsafe {
            CVDisplayLinkStop(self.link);
            CVDisplayLinkRelease(self.link);
        }
        let ticks = unsafe { Box::from_raw(self.ticks) }.into_inner().unwrap_or_default();
        let gaps = |values: Vec<u64>| values.windows(2).map(|pair| seconds(pair[1].saturating_sub(pair[0]))).collect();
        let mut hosts: Vec<u64> = ticks.iter().map(|(_, host)| *host).filter(|host| *host != 0).collect();
        hosts.sort_unstable();
        hosts.dedup();
        Paced { arrivals: gaps(ticks.iter().map(|(arrived, _)| *arrived).collect()), outputs: gaps(hosts), callbacks: ticks.len() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(owner: &str, layer: i64, name: &str) -> Seen {
        Seen { owner: owner.into(), pid: 7, layer, number: 1, name: name.into(), bounds: Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 }, on_screen: true }
    }

    const DESKTOP: i64 = -2147483623;
    const SCREEN: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };

    #[test]
    fn the_layer_goes_between_the_picture_and_the_icons() {
        let owner_mac = [window("Finder", -2147483603, ""), window("Dock", -2147483624, ""), window("Window Server", -2147483626, "Desktop")];
        assert_eq!(choose(&owner_mac, &SCREEN, DESKTOP, 99), Some(DESKTOP));
        let runner = [window("Finder", -2147483603, ""), window("Dock", -2147483624, "Wallpaper-A4BD"), window("Wallpaper", -2147483625, "Offscreen Wallpaper Window")];
        assert_eq!(choose(&runner, &SCREEN, DESKTOP, 99), Some(DESKTOP));
    }

    #[test]
    fn without_both_neighbours_the_plain_swap_stays() {
        assert_eq!(choose(&[window("Dock", -2147483624, "")], &SCREEN, DESKTOP, 99), None);
        assert_eq!(choose(&[window("Finder", -2147483603, "")], &SCREEN, DESKTOP, 99), None);
        let elsewhere = Seen { bounds: Rect { x: 1440.0, y: 0.0, w: 1920.0, h: 1080.0 }, ..window("Dock", -2147483624, "") };
        assert_eq!(choose(&[elsewhere, window("Finder", -2147483603, "")], &SCREEN, DESKTOP, 99), None);
        let mine = Seen { pid: 99, ..window("Dock", -2147483624, "") };
        assert_eq!(choose(&[mine, window("Finder", -2147483603, "")], &SCREEN, DESKTOP, 99), None);
    }

    #[test]
    fn the_display_link_time_stamp_has_the_documented_size() {
        assert_eq!(std::mem::size_of::<TimeStamp>(), 80);
        assert_eq!(std::mem::offset_of!(TimeStamp, host), 16);
    }

    #[test]
    fn the_levels_sit_in_the_documented_order() {
        let (desktop, icons) = levels();
        assert!(desktop < icons);
    }
}
