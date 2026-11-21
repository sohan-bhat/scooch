use super::on_main;
use super::sys::{self, Ref};
use anyhow::{anyhow, bail, Context, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, Message};
use block2::RcBlock;
use objc2_app_kit::{NSFont, NSFontAttributeName, NSFontWeightSemibold, NSWorkspace, NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidLaunchApplicationNotification, NSWorkspaceDidWakeNotification};
use objc2_core_foundation::{CGRect, CGSize};
use objc2_foundation::{ns_string, NSAppleEventDescriptor, NSAppleEventSendOptions, NSDictionary, NSNotification, NSNumber, NSString};
use scooch_core::Rect;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const NOT_ALLOWED: i32 = -1743;
const WOULD_ASK: i32 = -1744;
const NOT_RUNNING: i32 = -600;
const NO_SUCH_OBJECT: i32 = -1728;
const TIMED_OUT: i32 = -1712;
const BUILD: i32 = 1;
const TIMEOUT: f64 = 2.0;
const PATIENCE: Duration = Duration::from_millis(500);

pub const DENIED: &str = "Scooch cannot see your desktop icons, so turn on Finder for scooch in System Settings, Privacy and Security, Automation.";
pub const ASKING: &str = "macOS is asking whether scooch may control Finder, so choose Allow in that window.";

type Answered = Arc<dyn Fn() + Send + Sync>;
type Check = Arc<dyn Fn(bool) -> Access + Send + Sync>;
pub type Nudge = Arc<dyn Fn() + Send + Sync>;
pub type Listen = Arc<dyn Fn(Nudge) -> Box<dyn FnOnce()> + Send + Sync>;

static ANSWERED: OnceLock<Answered> = OnceLock::new();
static ASKING_HERE: Asking = Asking::new();
static ALLOWED: AtomicBool = AtomicBool::new(false);

pub struct Asking {
    waiting: AtomicBool,
    asked: AtomicBool,
    open: AtomicBool,
    again: AtomicBool,
    count: Mutex<u64>,
    nudged: Condvar,
}

impl Asking {
    pub const fn new() -> Self {
        Self { waiting: AtomicBool::new(false), asked: AtomicBool::new(false), open: AtomicBool::new(false), again: AtomicBool::new(false), count: Mutex::new(0), nudged: Condvar::new() }
    }

    pub fn waiting(&self) -> bool {
        self.waiting.load(Ordering::SeqCst)
    }

    pub fn poke(&self) {
        if self.waiting() && !self.open.load(Ordering::SeqCst) {
            self.again.store(true, Ordering::SeqCst);
            self.nudge();
        }
    }

    pub fn nudge(&self) {
        if self.waiting() {
            *self.count.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
            self.nudged.notify_all();
        }
    }

    fn nudges(&self) -> u64 {
        *self.count.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn wait_past(&self, seen: u64) {
        let mut count = self.count.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while *count == seen {
            count = self.nudged.wait(count).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl Default for Asking {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Consent {
    Granted,
    AskNow,
    AskLater,
    Refused,
}

pub fn consent_for(access: Access, waiting_allowed: bool) -> Consent {
    match (access, waiting_allowed) {
        (Access::Allowed, _) => Consent::Granted,
        (Access::WouldAsk, true) => Consent::AskLater,
        (Access::WouldAsk, false) => Consent::AskNow,
        _ => Consent::Refused,
    }
}

pub fn ask_in_background(answered: impl Fn() + Send + Sync + 'static) {
    let _ = ANSWERED.set(Arc::new(answered));
}

pub fn start_asking(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::SeqCst)
}

const LABEL_GAP: f32 = 5.0;
const LABEL_HALO: f32 = 8.0;
const LABEL_TAIL: f32 = 9.0;
const WRAP_INSET: f32 = 6.0;
const WRAP_SLACK: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Access {
    Allowed,
    WouldAsk,
    Denied,
    FinderOff,
    Unknown(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub icon: f32,
    pub text: f32,
    pub spacing: f32,
    pub bottom: bool,
    pub info: bool,
    pub arrange: String,
    pub group: String,
    pub hidden: bool,
}

impl Default for View {
    fn default() -> Self {
        Self { icon: 64.0, text: 12.0, spacing: 54.0, bottom: true, info: false, arrange: "none".into(), group: "None".into(), hidden: false }
    }
}

impl View {
    pub fn sorted(&self) -> bool {
        !self.arrange.eq_ignore_ascii_case("none")
    }

    pub fn stacked(&self) -> bool {
        !self.group.eq_ignore_ascii_case("none")
    }

    pub fn wrap(&self) -> f32 {
        (self.icon + self.spacing - WRAP_INSET).max(self.icon)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Label {
    pub width: f32,
    pub lines: u32,
    pub line: f32,
}

pub fn item_rect(center: (f32, f32), view: &View, label: Label) -> Rect {
    let half = view.icon / 2.0;
    let lines = label.lines + u32::from(view.info);
    let block = lines as f32 * label.line + LABEL_TAIL;
    let wide = label.width + 2.0 * LABEL_HALO;
    if view.bottom {
        let reach = half.max(wide / 2.0);
        return Rect { x: center.0 - reach, y: center.1 - half, w: 2.0 * reach, h: view.icon + LABEL_GAP + block };
    }
    let tall = view.icon.max(block);
    Rect { x: center.0 - half, y: center.1 - tall / 2.0, w: view.icon + LABEL_GAP + wide, h: tall }
}

pub fn access(ask: bool) -> Access {
    let finder = finder();
    let descriptor: *const sys::AeDesc = unsafe { msg_send![&*finder, aeDesc] };
    let descriptor = descriptor as Ref;
    let wild = code(b"****");
    match unsafe { sys::AEDeterminePermissionToAutomateTarget(descriptor, wild, wild, u8::from(ask)) } {
        0 => Access::Allowed,
        WOULD_ASK => Access::WouldAsk,
        NOT_ALLOWED => Access::Denied,
        NOT_RUNNING => Access::FinderOff,
        other => Access::Unknown(other),
    }
}

pub fn view() -> View {
    let mut view = View::default();
    let finder = "com.apple.finder";
    if let Some(settings) = preference(finder, "DesktopViewSettings") {
        if let Some(group) = text(&settings, "GroupBy") {
            view.group = group;
        }
        if let Some(icons) = child(&settings, "IconViewSettings") {
            view.icon = number(&icons, "iconSize").unwrap_or(view.icon);
            view.text = number(&icons, "textSize").unwrap_or(view.text);
            view.spacing = number(&icons, "gridSpacing").unwrap_or(view.spacing);
            view.bottom = number(&icons, "labelOnBottom").map_or(view.bottom, |value| value != 0.0);
            view.info = number(&icons, "showItemInfo").is_some_and(|value| value != 0.0);
            view.arrange = text(&icons, "arrangeBy").unwrap_or(view.arrange);
        }
    }
    let desktop_off = preference(finder, "CreateDesktop").is_some_and(|value| !truthy(&value));
    let manager = "com.apple.WindowManager";
    let stage = preference(manager, "GloballyEnabled").is_some_and(|value| truthy(&value));
    let key = if stage { "HideDesktop" } else { "StandardHideDesktopIcons" };
    let items_off = preference(manager, key).is_some_and(|value| truthy(&value));
    view.hidden = desktop_off || items_off;
    view
}

pub fn icons(view: &View) -> Result<Vec<Rect>> {
    if view.hidden {
        return Ok(Vec::new());
    }
    for _ in 0..3 {
        let centers = centers()?;
        let names = names()?;
        if centers.len() == names.len() {
            let view = view.clone();
            return Ok(on_main(move |_| rects(&centers, &names, &view)));
        }
    }
    bail!("The desktop kept changing while Scooch read it, so it will try again at the next change.")
}

pub fn named(view: &View) -> Result<Vec<(String, Rect)>> {
    let centers = centers()?;
    let names = names()?;
    if centers.len() != names.len() {
        bail!("The desktop kept changing while Scooch read it, so it will try again at the next change.");
    }
    let view = view.clone();
    Ok(on_main(move |_| {
        let attributes = attributes(&view);
        centers.iter().zip(names).map(|(center, name)| {
            let rect = item_rect(*center, &view, measure(&name, &attributes, view.wrap()));
            (name, rect)
        }).collect()
    }))
}

fn attributes(view: &View) -> Retained<NSDictionary<NSString, AnyObject>> {
    let font = NSFont::systemFontOfSize_weight(view.text as f64, unsafe { NSFontWeightSemibold });
    let font: &AnyObject = &font;
    NSDictionary::from_slices(&[unsafe { NSFontAttributeName }], &[font])
}

fn rects(centers: &[(f32, f32)], names: &[String], view: &View) -> Vec<Rect> {
    let attributes = attributes(view);
    let mut seen = Vec::<(i32, i32)>::new();
    let mut out = Vec::new();
    for (center, name) in centers.iter().zip(names) {
        let spot = (center.0.round() as i32, center.1.round() as i32);
        if seen.contains(&spot) {
            continue;
        }
        seen.push(spot);
        out.push(item_rect(*center, view, measure(name, &attributes, view.wrap())));
    }
    out
}

fn measure(name: &str, attributes: &NSDictionary<NSString, AnyObject>, wrap: f32) -> Label {
    let string = NSString::from_str(name);
    let natural: CGSize = unsafe { msg_send![&*string, sizeWithAttributes: attributes] };
    let line = natural.height as f32 + 1.0;
    let limit = (wrap + WRAP_SLACK) as f64;
    if natural.width <= limit {
        return Label { width: natural.width as f32, lines: 1, line };
    }
    let room = CGSize { width: limit, height: 100_000.0 };
    let wrapped: CGRect = unsafe { msg_send![&*string, boundingRectWithSize: room, options: 1isize, attributes: attributes] };
    let rows = (wrapped.size.height / natural.height.max(1.0)).round();
    let width = if rows > 2.0 { wrap } else { wrapped.size.width as f32 };
    Label { width, lines: 2, line }
}

fn centers() -> Result<Vec<(f32, f32)>> {
    let reply = fetch(code(b"dpos"))?;
    list(reply.as_deref()).iter().map(|item| point(item).context("Finder sent a position Scooch cannot read")).collect()
}

fn names() -> Result<Vec<String>> {
    let reply = fetch(code(b"dnam"))?;
    Ok(list(reply.as_deref()).iter().map(|item| item.stringValue().map(|name| name.to_string()).unwrap_or_default()).collect())
}

pub fn poked() {
    ASKING_HERE.poke();
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum First {
    Send,
    Ask,
    Refuse,
}

fn first_step(allowed: bool, check: impl FnOnce() -> Access) -> First {
    if allowed {
        return First::Send;
    }
    match check() {
        Access::WouldAsk => First::Ask,
        Access::Denied => First::Refuse,
        _ => First::Send,
    }
}

fn granted(property_code: u32, status: i32) -> Result<Option<Retained<NSAppleEventDescriptor>>> {
    match consent() {
        Consent::Granted => {
            ALLOWED.store(true, Ordering::SeqCst);
            send(property_code).or_else(failure)
        }
        Consent::AskLater => Err(anyhow!("{ASKING}")),
        _ => failure(status),
    }
}

fn fetch(property_code: u32) -> Result<Option<Retained<NSAppleEventDescriptor>>> {
    if ASKING_HERE.waiting() {
        ASKING_HERE.nudge();
        bail!("{ASKING}");
    }
    match first_step(ALLOWED.load(Ordering::SeqCst), || access(false)) {
        First::Ask => return granted(property_code, WOULD_ASK),
        First::Refuse => return failure(NOT_ALLOWED),
        First::Send => {}
    }
    match send(property_code) {
        Ok(reply) => {
            ALLOWED.store(true, Ordering::SeqCst);
            Ok(reply)
        }
        Err(TIMED_OUT) if ALLOWED.load(Ordering::SeqCst) => failure(TIMED_OUT),
        Err(status @ (WOULD_ASK | TIMED_OUT)) => {
            ALLOWED.store(false, Ordering::SeqCst);
            granted(property_code, status)
        }
        Err(status) => failure(status),
    }
}

fn consent() -> Consent {
    if let Some(answered) = ANSWERED.get().cloned() {
        return consent_aside(Arc::new(access), answered, &ASKING_HERE, Arc::new(listen_for_answers), PATIENCE);
    }
    match consent_for(access(false), false) {
        Consent::AskNow if access(true) == Access::Allowed => Consent::Granted,
        Consent::AskNow => Consent::Refused,
        other => other,
    }
}

pub fn consent_aside(check: Check, answered: Answered, asking: &'static Asking, listen: Listen, patience: Duration) -> Consent {
    if !start_asking(&asking.waiting) {
        return Consent::AskLater;
    }
    let slot = Arc::new((Mutex::new((None::<Access>, false)), Condvar::new()));
    let told = slot.clone();
    let spawned = std::thread::Builder::new().name("scooch-consent".into()).spawn(move || {
        let first = objc2::rc::autoreleasepool(|_| check(false));
        if first != Access::WouldAsk {
            asking.waiting.store(false, Ordering::SeqCst);
        }
        let late = {
            let mut held = told.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            held.0 = Some(first);
            told.1.notify_all();
            held.1
        };
        if first != Access::WouldAsk {
            if late {
                answered();
            }
            return;
        }
        let ask = || {
            asking.again.store(false, Ordering::SeqCst);
            asking.open.store(true, Ordering::SeqCst);
            said("asks whether scooch may control Finder");
            let answer = objc2::rc::autoreleasepool(|_| check(true));
            asking.open.store(false, Ordering::SeqCst);
            let now = if answer == Access::Allowed { answer } else { objc2::rc::autoreleasepool(|_| check(false)) };
            said(&format!("the ask came back as {answer:?} and a check without asking says {now:?}"));
            now
        };
        let mut seen = asking.nudges();
        let mut now = if asking.asked.swap(true, Ordering::SeqCst) { objc2::rc::autoreleasepool(|_| check(false)) } else { ask() };
        let mut listening: Option<Box<dyn FnOnce()>> = None;
        while now == Access::WouldAsk {
            if listening.is_none() {
                listening = Some(objc2::rc::autoreleasepool(|_| listen(Arc::new(move || asking.nudge()))));
                said("waits for the answer without asking again");
            }
            asking.wait_past(seen);
            seen = asking.nudges();
            now = objc2::rc::autoreleasepool(|_| check(false));
            said(&format!("checked again without asking, {now:?}"));
            if now == Access::WouldAsk && asking.again.swap(false, Ordering::SeqCst) {
                now = ask();
            }
        }
        if let Some(stop) = listening {
            objc2::rc::autoreleasepool(|_| stop());
        }
        asking.waiting.store(false, Ordering::SeqCst);
        if now == Access::Allowed {
            answered();
        }
    });
    if spawned.is_err() {
        asking.waiting.store(false, Ordering::SeqCst);
        return Consent::AskLater;
    }
    let deadline = Instant::now() + patience;
    let mut held = slot.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    while held.0.is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        held = slot.1.wait_timeout(held, left).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
    }
    match held.0 {
        Some(first) => consent_for(first, true),
        None => {
            held.1 = true;
            Consent::AskLater
        }
    }
}

fn said(what: &str) {
    if crate::daemon::trace() {
        eprintln!("{} consent {what}", crate::daemon::stamp());
    }
}

fn listen_for_answers(nudge: Nudge) -> Box<dyn FnOnce()> {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let names = unsafe { [NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidWakeNotification, NSWorkspaceDidLaunchApplicationNotification] };
    let tokens: Vec<_> = names
        .into_iter()
        .map(|name| {
            let nudge = nudge.clone();
            let block = RcBlock::new(move |_: NonNull<NSNotification>| nudge());
            unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
        })
        .collect();
    Box::new(move || {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        for token in tokens {
            unsafe { center.removeObserver(token.as_ref()) };
        }
    })
}

fn send(property_code: u32) -> std::result::Result<Option<Retained<NSAppleEventDescriptor>>, i32> {
    let root = NSAppleEventDescriptor::nullDescriptor();
    let desktop = property(code(b"desk"), &root)?;
    let all = code(b"all ");
    let every: Option<Retained<NSAppleEventDescriptor>> = unsafe {
        msg_send![class!(NSAppleEventDescriptor), descriptorWithDescriptorType: code(b"abso"), bytes: &all as *const u32 as *const c_void, length: 4usize]
    };
    let every = every.ok_or(BUILD)?;
    let items = specifier(code(b"cobj"), code(b"indx"), &every, &desktop)?;
    let target = property(property_code, &items)?;
    let finder = finder();
    let event: Retained<NSAppleEventDescriptor> = unsafe {
        msg_send![class!(NSAppleEventDescriptor), appleEventWithEventClass: code(b"core"), eventID: code(b"getd"), targetDescriptor: &*finder, returnID: -1i16, transactionID: 0i32]
    };
    let _: () = unsafe { msg_send![&*event, setParamDescriptor: &*target, forKeyword: code(b"----")] };
    let options = NSAppleEventSendOptions::WaitForReply | NSAppleEventSendOptions::NeverInteract;
    let reply = event.sendEventWithOptions_timeout_error(options, TIMEOUT).map_err(|error| error.code() as i32)?;
    let status: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![&*reply, paramDescriptorForKeyword: code(b"errn")] };
    if let Some(status) = status {
        if status.int32Value() != 0 {
            return Err(status.int32Value());
        }
    }
    Ok(unsafe { msg_send![&*reply, paramDescriptorForKeyword: code(b"----")] })
}

pub const FINDER_OFF: &str = "Finder is not running, so Scooch keeps the picture as it is until Finder starts.";

fn failure(status: i32) -> Result<Option<Retained<NSAppleEventDescriptor>>> {
    match status {
        NO_SUCH_OBJECT => Ok(None),
        NOT_RUNNING => Err(anyhow!("{FINDER_OFF}")),
        NOT_ALLOWED | WOULD_ASK => Err(anyhow!("{DENIED}")),
        TIMED_OUT => Err(anyhow!("Finder took over two seconds to list the desktop, so Scooch will try again at the next change.")),
        BUILD => Err(anyhow!("Scooch could not build its question for Finder, so run scooch doctor.")),
        other => Err(anyhow!("Finder answered with code {other}, so run scooch doctor to see more.")),
    }
}

fn list(reply: Option<&NSAppleEventDescriptor>) -> Vec<Retained<NSAppleEventDescriptor>> {
    let Some(reply) = reply else { return Vec::new() };
    let kind: u32 = unsafe { msg_send![reply, descriptorType] };
    if kind != code(b"list") {
        return vec![reply.retain()];
    }
    (1..=reply.numberOfItems()).filter_map(|index| reply.descriptorAtIndex(index)).collect()
}

fn point(item: &NSAppleEventDescriptor) -> Option<(f32, f32)> {
    let kind: u32 = unsafe { msg_send![item, descriptorType] };
    if kind == code(b"QDpt") {
        let bytes = item.data().to_vec();
        if bytes.len() < 4 {
            return None;
        }
        let down = i16::from_ne_bytes([bytes[0], bytes[1]]);
        let across = i16::from_ne_bytes([bytes[2], bytes[3]]);
        return Some((across as f32, down as f32));
    }
    if item.numberOfItems() != 2 {
        return None;
    }
    Some((item.descriptorAtIndex(1)?.doubleValue() as f32, item.descriptorAtIndex(2)?.doubleValue() as f32))
}

fn finder() -> Retained<NSAppleEventDescriptor> {
    NSAppleEventDescriptor::descriptorWithBundleIdentifier(ns_string!("com.apple.finder"))
}

fn property(name: u32, from: &NSAppleEventDescriptor) -> std::result::Result<Retained<NSAppleEventDescriptor>, i32> {
    specifier(code(b"prop"), code(b"prop"), &NSAppleEventDescriptor::descriptorWithTypeCode(name), from)
}

fn specifier(want: u32, form: u32, selector: &NSAppleEventDescriptor, from: &NSAppleEventDescriptor) -> std::result::Result<Retained<NSAppleEventDescriptor>, i32> {
    let record = NSAppleEventDescriptor::recordDescriptor();
    put(&record, b"want", &NSAppleEventDescriptor::descriptorWithTypeCode(want));
    put(&record, b"form", &NSAppleEventDescriptor::descriptorWithEnumCode(form));
    put(&record, b"seld", selector);
    put(&record, b"from", from);
    let object: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![&*record, coerceToDescriptorType: code(b"obj ")] };
    object.ok_or(BUILD)
}

fn put(record: &NSAppleEventDescriptor, key: &[u8; 4], value: &NSAppleEventDescriptor) {
    let _: () = unsafe { msg_send![record, setDescriptor: value, forKeyword: code(key)] };
}

const fn code(text: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*text)
}

pub fn preference(application: &str, key: &str) -> Option<Retained<AnyObject>> {
    let application = NSString::from_str(application);
    let key = NSString::from_str(key);
    unsafe {
        sys::CFPreferencesAppSynchronize(as_ref(&application));
        let value = sys::CFPreferencesCopyAppValue(as_ref(&key), as_ref(&application));
        Retained::from_raw(value as *mut AnyObject)
    }
}

fn as_ref(string: &NSString) -> Ref {
    string as *const NSString as Ref
}

fn lookup(dictionary: &AnyObject, key: &str) -> Option<Retained<AnyObject>> {
    let dictionary = dictionary.downcast_ref::<NSDictionary>()?;
    let key = NSString::from_str(key);
    unsafe { msg_send![dictionary, objectForKey: &*key] }
}

fn child(dictionary: &AnyObject, key: &str) -> Option<Retained<AnyObject>> {
    lookup(dictionary, key).filter(|value| value.downcast_ref::<NSDictionary>().is_some())
}

fn number(dictionary: &AnyObject, key: &str) -> Option<f32> {
    lookup(dictionary, key)?.downcast_ref::<NSNumber>().map(|value| value.floatValue())
}

fn text(dictionary: &AnyObject, key: &str) -> Option<String> {
    lookup(dictionary, key)?.downcast_ref::<NSString>().map(|value| value.to_string())
}

fn truthy(value: &AnyObject) -> bool {
    if let Some(number) = value.downcast_ref::<NSNumber>() {
        return number.boolValue();
    }
    value.downcast_ref::<NSString>().is_some_and(|text| {
        let text = text.to_string().to_ascii_lowercase();
        text == "1" || text == "true" || text == "yes"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_asks_for_finder_in_the_background_and_a_command_asks_in_place() {
        assert_eq!(consent_for(Access::Allowed, true), Consent::Granted);
        assert_eq!(consent_for(Access::WouldAsk, true), Consent::AskLater);
        assert_eq!(consent_for(Access::WouldAsk, false), Consent::AskNow);
        assert_eq!(consent_for(Access::Denied, true), Consent::Refused);
        assert_eq!(consent_for(Access::FinderOff, false), Consent::Refused);
        let flag = AtomicBool::new(false);
        assert!(start_asking(&flag));
        assert!(!start_asking(&flag));
        flag.store(false, Ordering::SeqCst);
        assert!(start_asking(&flag));
        assert!(!ASKING.contains(['\u{2014}', '\u{2013}', ';', '(', ')']) && !ASKING.contains(": "));
    }

    fn until(done: impl Fn() -> bool) -> bool {
        let started = Instant::now();
        while !done() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        done()
    }

    fn settle(asking: &Asking) -> bool {
        until(|| !asking.waiting())
    }

    fn fresh() -> &'static Asking {
        Box::leak(Box::new(Asking::new()))
    }

    fn deaf() -> Listen {
        Arc::new(|_| Box::new(|| {}))
    }

    #[derive(Default)]
    struct Heard {
        nudges: Mutex<Vec<Nudge>>,
        added: std::sync::atomic::AtomicUsize,
        removed: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Heard {
        fn listen(self: &Arc<Self>) -> Listen {
            let heard = self.clone();
            Arc::new(move |nudge| {
                heard.added.fetch_add(1, Ordering::SeqCst);
                heard.nudges.lock().unwrap().push(nudge);
                let removed = heard.removed.clone();
                Box::new(move || {
                    removed.fetch_add(1, Ordering::SeqCst);
                })
            })
        }

        fn fire(&self) {
            for nudge in self.nudges.lock().unwrap().iter() {
                nudge();
            }
        }

        fn counts(&self) -> (usize, usize) {
            (self.added.load(Ordering::SeqCst), self.removed.load(Ordering::SeqCst))
        }
    }

    struct Tcc {
        state: Mutex<Access>,
        asks: std::sync::atomic::AtomicUsize,
        joins: bool,
        closed: Mutex<Option<Access>>,
        held: AtomicBool,
        checks: std::sync::atomic::AtomicUsize,
        checking_held: AtomicBool,
    }

    impl Tcc {
        fn new(state: Access, joins: bool) -> Arc<Self> {
            Arc::new(Self { state: Mutex::new(state), asks: std::sync::atomic::AtomicUsize::new(0), joins, closed: Mutex::new(None), held: AtomicBool::new(false), checks: std::sync::atomic::AtomicUsize::new(0), checking_held: AtomicBool::new(false) })
        }

        fn closed_unanswered(returns: Access) -> Arc<Self> {
            let tcc = Self::new(Access::WouldAsk, false);
            tcc.close_with(Some(returns));
            tcc
        }

        fn checks(&self) -> usize {
            self.checks.load(Ordering::SeqCst)
        }

        fn check(self: &Arc<Self>) -> Check {
            let tcc = self.clone();
            Arc::new(move |ask| {
                if !ask {
                    tcc.checks.fetch_add(1, Ordering::SeqCst);
                    while tcc.checking_held.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                if ask {
                    tcc.asks.fetch_add(1, Ordering::SeqCst);
                    while tcc.held.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    if let Some(returns) = *tcc.closed.lock().unwrap() {
                        return returns;
                    }
                    while tcc.joins && *tcc.state.lock().unwrap() == Access::WouldAsk {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                *tcc.state.lock().unwrap()
            })
        }

        fn hold(&self, held: bool) {
            self.held.store(held, Ordering::SeqCst);
        }

        fn close_with(&self, returns: Option<Access>) {
            *self.closed.lock().unwrap() = returns;
        }

        fn answer(&self, access: Access) {
            *self.state.lock().unwrap() = access;
        }

        fn asks(&self) -> usize {
            self.asks.load(Ordering::SeqCst)
        }
    }

    fn woke_into() -> (Arc<AtomicBool>, Answered) {
        let woke = Arc::new(AtomicBool::new(false));
        let told = woke.clone();
        (woke, Arc::new(move || told.store(true, Ordering::SeqCst)))
    }

    #[test]
    fn a_daemon_whose_ask_cannot_join_the_open_window_keeps_a_waiter_that_hears_allow() {
        let (tcc, heard, asking) = (Tcc::new(Access::WouldAsk, false), Arc::new(Heard::default()), fresh());
        let (woke, answered) = woke_into();
        assert_eq!(consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_millis(200)), Consent::AskLater);
        assert!(until(|| heard.counts() == (1, 0)), "{:?}", heard.counts());
        assert!(asking.waiting() && tcc.asks() == 1 && !woke.load(Ordering::SeqCst));
        tcc.answer(Access::Allowed);
        heard.fire();
        assert!(until(|| woke.load(Ordering::SeqCst)), "Allow in the earlier window did not wake the daemon");
        assert!(settle(asking) && heard.counts() == (1, 1) && tcc.asks() == 1, "{:?} asks {}", heard.counts(), tcc.asks());
    }

    #[test]
    fn a_window_closed_without_a_choice_leaves_a_waiter_whatever_the_ask_returned() {
        for returned in [Access::Denied, Access::WouldAsk, Access::Unknown(-128)] {
            let (tcc, heard, asking) = (Tcc::closed_unanswered(returned), Arc::new(Heard::default()), fresh());
            let (woke, answered) = woke_into();
            assert_eq!(consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_millis(100)), Consent::AskLater);
            assert!(until(|| heard.counts() == (1, 0)), "{returned:?} {:?}", heard.counts());
            assert!(asking.waiting() && tcc.asks() == 1 && !woke.load(Ordering::SeqCst), "{returned:?}");
            tcc.answer(Access::Allowed);
            heard.fire();
            assert!(until(|| woke.load(Ordering::SeqCst)) && settle(asking) && heard.counts() == (1, 1) && tcc.asks() == 1, "{returned:?}");
        }
    }

    #[test]
    fn the_first_read_checks_without_asking_until_the_daemon_knows_it_is_allowed() {
        let calls = std::cell::Cell::new(0);
        let counted = |access: Access| {
            let calls = &calls;
            move || {
                calls.set(calls.get() + 1);
                access
            }
        };
        assert_eq!(first_step(false, counted(Access::Allowed)), First::Send);
        assert_eq!(first_step(false, counted(Access::WouldAsk)), First::Ask);
        assert_eq!(first_step(false, counted(Access::Denied)), First::Refuse);
        assert_eq!(first_step(false, counted(Access::FinderOff)), First::Send);
        assert_eq!(first_step(false, counted(Access::Unknown(-1712))), First::Send);
        assert_eq!(calls.get(), 5);
        assert_eq!(first_step(true, counted(Access::WouldAsk)), First::Send);
        assert_eq!(calls.get(), 5);
    }

    #[test]
    fn nudges_never_ask_again_and_a_poke_asks_once_more_but_never_over_an_open_ask() {
        let (tcc, heard, asking) = (Tcc::closed_unanswered(Access::WouldAsk), Arc::new(Heard::default()), fresh());
        let (woke, answered) = woke_into();
        assert_eq!(consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_millis(100)), Consent::AskLater);
        assert!(until(|| heard.counts().0 == 1));
        for _ in 0..3 {
            asking.nudge();
            heard.fire();
        }
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(tcc.asks(), 1, "a nudge asked again");
        tcc.hold(true);
        asking.poke();
        assert!(until(|| tcc.asks() == 2), "a poke did not ask once more");
        asking.poke();
        asking.poke();
        tcc.hold(false);
        std::thread::sleep(Duration::from_millis(150));
        heard.fire();
        std::thread::sleep(Duration::from_millis(150));
        assert!(tcc.asks() == 2 && asking.waiting(), "asks {} over an open ask", tcc.asks());
        tcc.close_with(None);
        tcc.answer(Access::Allowed);
        heard.fire();
        assert!(until(|| woke.load(Ordering::SeqCst)) && settle(asking) && tcc.asks() == 2);
    }

    #[test]
    fn a_poke_before_the_first_ask_is_used_up_by_that_ask() {
        let (tcc, heard, asking) = (Tcc::closed_unanswered(Access::WouldAsk), Arc::new(Heard::default()), fresh());
        let (_, answered) = woke_into();
        tcc.checking_held.store(true, Ordering::SeqCst);
        assert_eq!(consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_millis(50)), Consent::AskLater);
        assert!(until(|| tcc.checks() == 1) && asking.waiting() && tcc.asks() == 0);
        asking.poke();
        tcc.checking_held.store(false, Ordering::SeqCst);
        assert!(until(|| heard.counts().0 == 1));
        for _ in 0..2 {
            let before = tcc.checks();
            heard.fire();
            assert!(until(|| tcc.checks() > before));
        }
        assert_eq!(tcc.asks(), 1, "a poke from before the first ask asked again with no person acting since");
        asking.poke();
        assert!(until(|| tcc.asks() == 2));
    }

    #[test]
    fn allow_in_the_window_a_poke_brought_back_wakes_the_daemon() {
        let (tcc, heard, asking) = (Tcc::closed_unanswered(Access::Denied), Arc::new(Heard::default()), fresh());
        let (woke, answered) = woke_into();
        consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_millis(100));
        assert!(until(|| heard.counts().0 == 1) && asking.waiting());
        tcc.hold(true);
        tcc.close_with(None);
        asking.poke();
        assert!(until(|| tcc.asks() == 2));
        tcc.answer(Access::Allowed);
        tcc.hold(false);
        assert!(until(|| woke.load(Ordering::SeqCst)) && settle(asking), "Allow in the second window did not wake the daemon");
        assert_eq!((tcc.asks(), heard.counts()), (2, (1, 1)));
    }

    #[test]
    fn a_persisted_dont_allow_never_reaches_a_poke() {
        let (tcc, heard, asking) = (Tcc::new(Access::Denied, false), Arc::new(Heard::default()), fresh());
        let (woke, answered) = woke_into();
        assert_eq!(consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_secs(5)), Consent::Refused);
        assert!(settle(asking));
        asking.poke();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!((tcc.asks(), heard.counts(), woke.load(Ordering::SeqCst)), (0, (0, 0), false));
    }

    #[test]
    fn an_ask_that_joins_the_open_window_wakes_at_its_answer_and_never_listens() {
        let (tcc, heard, asking) = (Tcc::new(Access::WouldAsk, true), Arc::new(Heard::default()), fresh());
        let (woke, answered) = woke_into();
        assert_eq!(consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_millis(100)), Consent::AskLater);
        std::thread::sleep(Duration::from_millis(150));
        assert!(asking.waiting() && tcc.asks() == 1);
        tcc.answer(Access::Allowed);
        assert!(until(|| woke.load(Ordering::SeqCst)) && settle(asking));
        assert_eq!((heard.counts(), tcc.asks()), ((0, 0), 1));
    }

    #[test]
    fn the_person_is_asked_once_however_often_the_daemon_ticks_and_dont_allow_ends_it() {
        let (tcc, heard, asking) = (Tcc::new(Access::WouldAsk, false), Arc::new(Heard::default()), fresh());
        let (woke, answered) = woke_into();
        assert_eq!(consent_aside(tcc.check(), answered.clone(), asking, heard.listen(), Duration::from_millis(100)), Consent::AskLater);
        assert!(until(|| heard.counts().0 == 1));
        for _ in 0..5 {
            asking.nudge();
            assert_eq!(consent_aside(tcc.check(), answered.clone(), asking, heard.listen(), Duration::from_millis(10)), Consent::AskLater);
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(asking.waiting() && tcc.asks() == 1 && heard.counts() == (1, 0), "asks {} {:?}", tcc.asks(), heard.counts());
        tcc.answer(Access::Denied);
        heard.fire();
        assert!(settle(asking) && heard.counts() == (1, 1));
        assert_eq!(consent_aside(tcc.check(), answered.clone(), asking, heard.listen(), Duration::from_secs(5)), Consent::Refused);
        let next = fresh();
        assert_eq!(consent_aside(tcc.check(), answered, next, heard.listen(), Duration::from_secs(5)), Consent::Refused);
        assert!(settle(next));
        assert_eq!((tcc.asks(), heard.counts(), woke.load(Ordering::SeqCst)), (1, (1, 1), false));
    }

    #[test]
    fn a_waiter_that_ended_without_an_answer_never_asks_again_in_the_same_daemon() {
        let (tcc, heard, asking) = (Tcc::new(Access::WouldAsk, false), Arc::new(Heard::default()), fresh());
        let (woke, answered) = woke_into();
        consent_aside(tcc.check(), answered.clone(), asking, heard.listen(), Duration::from_millis(100));
        assert!(until(|| heard.counts().0 == 1));
        tcc.answer(Access::FinderOff);
        heard.fire();
        assert!(settle(asking));
        tcc.answer(Access::WouldAsk);
        assert_eq!(consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_secs(5)), Consent::AskLater);
        assert!(until(|| heard.counts().0 == 2) && asking.waiting());
        assert_eq!((tcc.asks(), woke.load(Ordering::SeqCst)), (1, false));
        tcc.answer(Access::Allowed);
        heard.fire();
        assert!(until(|| woke.load(Ordering::SeqCst)) && settle(asking) && heard.counts() == (2, 2));
    }

    #[test]
    fn a_decided_permission_never_listens() {
        for decided in [Access::Allowed, Access::Denied] {
            let (tcc, heard, asking) = (Tcc::new(decided, false), Arc::new(Heard::default()), fresh());
            let (_, answered) = woke_into();
            assert_eq!(consent_aside(tcc.check(), answered, asking, heard.listen(), Duration::from_secs(5)), consent_for(decided, true));
            assert!(settle(asking));
            assert_eq!((heard.counts(), tcc.asks()), ((0, 0), 0), "{decided:?}");
        }
    }

    #[test]
    fn a_consent_check_that_never_answers_leaves_the_tick_free() {
        let asking = fresh();
        let woke = Arc::new(AtomicBool::new(false));
        let told = woke.clone();
        let check: Check = Arc::new(|_| {
            std::thread::sleep(Duration::from_secs(3));
            Access::Allowed
        });
        let started = Instant::now();
        assert_eq!(consent_aside(check, Arc::new(move || told.store(true, Ordering::SeqCst)), asking, deaf(), Duration::from_millis(100)), Consent::AskLater);
        assert!(started.elapsed() < Duration::from_secs(1), "the check held the tick for {:?}", started.elapsed());
        assert!(asking.waiting());
        assert!(until(|| woke.load(Ordering::SeqCst)) && !asking.waiting(), "the late answer did not wake the daemon");
    }

    #[test]
    fn a_second_tick_during_a_slow_check_starts_no_second_check() {
        let asking = fresh();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = calls.clone();
        let check: Check = Arc::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(600));
            Access::Allowed
        });
        let quiet: Answered = Arc::new(|| {});
        assert_eq!(consent_aside(check.clone(), quiet.clone(), asking, deaf(), Duration::from_millis(50)), Consent::AskLater);
        assert_eq!(consent_aside(check, quiet, asking, deaf(), Duration::from_millis(50)), Consent::AskLater);
        assert!(settle(asking));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_late_refusal_wakes_the_daemon() {
        let asking = fresh();
        let woke = Arc::new(AtomicBool::new(false));
        let told = woke.clone();
        let check: Check = Arc::new(|_| {
            std::thread::sleep(Duration::from_millis(300));
            Access::Denied
        });
        assert_eq!(consent_aside(check, Arc::new(move || told.store(true, Ordering::SeqCst)), asking, deaf(), Duration::from_millis(50)), Consent::AskLater);
        assert!(until(|| woke.load(Ordering::SeqCst)) && !asking.waiting(), "a late refusal did not wake the daemon");
    }

    #[test]
    fn a_quick_consent_answer_decides_at_once() {
        for (first, asked, wanted, wakes) in [(Access::Allowed, Access::Allowed, Consent::Granted, false), (Access::Denied, Access::Allowed, Consent::Refused, false), (Access::WouldAsk, Access::Allowed, Consent::AskLater, true), (Access::WouldAsk, Access::Denied, Consent::AskLater, false)] {
            let asking = fresh();
            let woke = Arc::new(AtomicBool::new(false));
            let told = woke.clone();
            let chose = Arc::new(AtomicBool::new(false));
            let check: Check = Arc::new(move |ask| {
                if ask {
                    chose.store(true, Ordering::SeqCst);
                }
                if chose.load(Ordering::SeqCst) { asked } else { first }
            });
            assert_eq!(consent_aside(check, Arc::new(move || told.store(true, Ordering::SeqCst)), asking, deaf(), Duration::from_secs(5)), wanted, "{first:?} then {asked:?}");
            assert!(settle(asking), "{first:?} then {asked:?} left the flag up");
            assert!(!wakes || until(|| woke.load(Ordering::SeqCst)), "{first:?} then {asked:?} did not wake the daemon");
            assert_eq!(woke.load(Ordering::SeqCst), wakes, "{first:?} then {asked:?}");
        }
    }

    #[test]
    fn while_macos_asks_the_daemon_sends_finder_nothing() {
        ASKING_HERE.waiting.store(true, Ordering::SeqCst);
        let answered = fetch(code(b"dpos"));
        ASKING_HERE.waiting.store(false, Ordering::SeqCst);
        assert_eq!(answered.unwrap_err().to_string(), ASKING);
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1.5
    }

    #[test]
    fn one_line_label_matches_the_measured_desktop() {
        let view = View::default();
        let rect = item_rect((1253.0, 40.0), &view, Label { width: 71.86, lines: 1, line: 16.0 });
        assert!(close(rect.x, 1253.0 - 43.5));
        assert!(close(rect.x + rect.w, 1253.0 + 42.5));
        assert_eq!(rect.y, 8.0);
        assert!(close(rect.y + rect.h, 40.0 + 61.5));
    }

    #[test]
    fn two_line_label_matches_the_measured_desktop() {
        let view = View::default();
        let rect = item_rect((1375.0, 600.0), &view, Label { width: 112.0, lines: 2, line: 16.0 });
        assert!(close(rect.x, 1375.0 - 63.5));
        assert!(close(rect.x + rect.w, 1375.0 + 63.0));
        assert_eq!(rect.y, 568.0);
        assert!(close(rect.y + rect.h, 600.0 + 76.5));
    }

    #[test]
    fn short_label_never_narrows_the_icon() {
        let view = View::default();
        let rect = item_rect((100.0, 100.0), &view, Label { width: 10.0, lines: 1, line: 16.0 });
        assert_eq!((rect.x, rect.w), (68.0, 64.0));
    }

    #[test]
    fn wrap_width_follows_the_grid() {
        assert_eq!(View::default().wrap(), 112.0);
        let tight = View { spacing: 1.0, ..View::default() };
        assert_eq!(tight.wrap(), 64.0);
    }

    #[test]
    fn label_on_the_right_sits_beside_the_icon() {
        let view = View { bottom: false, ..View::default() };
        let rect = item_rect((200.0, 200.0), &view, Label { width: 80.0, lines: 1, line: 16.0 });
        assert_eq!((rect.x, rect.y, rect.h), (168.0, 168.0, 64.0));
        assert_eq!(rect.w, 64.0 + 5.0 + 96.0);
    }

    #[test]
    fn item_info_adds_a_line() {
        let plain = item_rect((0.0, 0.0), &View::default(), Label { width: 50.0, lines: 1, line: 16.0 });
        let info = item_rect((0.0, 0.0), &View { info: true, ..View::default() }, Label { width: 50.0, lines: 1, line: 16.0 });
        assert_eq!(info.h - plain.h, 16.0);
    }

    #[test]
    fn finder_not_running_is_an_error_and_an_empty_desktop_is_not() {
        assert_eq!(failure(NOT_RUNNING).unwrap_err().to_string(), FINDER_OFF);
        assert!(failure(NO_SUCH_OBJECT).unwrap().is_none());
    }

    #[test]
    fn sorted_and_stacked_read_the_settings() {
        let view = View { arrange: "name".into(), group: "Kind".into(), ..View::default() };
        assert!(view.sorted() && view.stacked());
        assert!(!View::default().sorted() && !View::default().stacked());
    }
}
