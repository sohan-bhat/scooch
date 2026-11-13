use super::on_main;
use super::sys::{self, Ref};
use anyhow::{anyhow, bail, Context, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, Message};
use objc2_app_kit::{NSFont, NSFontAttributeName, NSFontWeightSemibold};
use objc2_core_foundation::{CGRect, CGSize};
use objc2_foundation::{ns_string, NSAppleEventDescriptor, NSAppleEventSendOptions, NSDictionary, NSNumber, NSString};
use scooch_core::Rect;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

const NOT_ALLOWED: i32 = -1743;
const WOULD_ASK: i32 = -1744;
const NOT_RUNNING: i32 = -600;
const NO_SUCH_OBJECT: i32 = -1728;
const TIMED_OUT: i32 = -1712;
const BUILD: i32 = 1;
const TIMEOUT: f64 = 2.0;

pub const DENIED: &str = "Scooch cannot see your desktop icons, so turn on Finder for scooch in System Settings, Privacy and Security, Automation.";
pub const ASKING: &str = "macOS is asking whether scooch may control Finder, so choose Allow in that window.";

type Answered = Arc<dyn Fn() + Send + Sync>;

static ANSWERED: OnceLock<Answered> = OnceLock::new();
static WAITING: AtomicBool = AtomicBool::new(false);

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

fn fetch(property_code: u32) -> Result<Option<Retained<NSAppleEventDescriptor>>> {
    if WAITING.load(Ordering::SeqCst) {
        bail!("{ASKING}");
    }
    match send(property_code) {
        Err(status @ (WOULD_ASK | TIMED_OUT)) => match consent() {
            Consent::Granted => send(property_code).or_else(failure),
            Consent::AskLater => Err(anyhow!("{ASKING}")),
            _ => failure(status),
        },
        other => other.or_else(failure),
    }
}

fn consent() -> Consent {
    match consent_for(access(false), ANSWERED.get().is_some()) {
        Consent::AskNow if access(true) == Access::Allowed => Consent::Granted,
        Consent::AskNow => Consent::Refused,
        Consent::AskLater => {
            ask_later();
            Consent::AskLater
        }
        other => other,
    }
}

fn ask_later() {
    let Some(answered) = ANSWERED.get().cloned() else { return };
    if !start_asking(&WAITING) {
        return;
    }
    let spawned = std::thread::Builder::new().name("scooch-consent".into()).spawn(move || {
        let answer = objc2::rc::autoreleasepool(|_| access(true));
        WAITING.store(false, Ordering::SeqCst);
        if answer == Access::Allowed {
            answered();
        }
    });
    if spawned.is_err() {
        WAITING.store(false, Ordering::SeqCst);
    }
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

    #[test]
    fn while_macos_asks_the_daemon_sends_finder_nothing() {
        WAITING.store(true, Ordering::SeqCst);
        let answered = fetch(code(b"dpos"));
        WAITING.store(false, Ordering::SeqCst);
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
