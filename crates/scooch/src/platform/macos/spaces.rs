use super::finder::preference;
use super::store::{convert, Plist};
use super::sys::{Owned, Ref};
use objc2::runtime::AnyObject;
use std::ffi::{c_void, CStr};

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub current: Option<String>,
    pub full_screen: bool,
    pub spaces: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Showing {
    Desktop(String),
    FullScreen,
    Unknown,
}

const DESKTOP: f64 = 0.0;

fn desktop_kind(space: &Plist) -> bool {
    space.get("type").and_then(Plist::number).is_none_or(|kind| kind == DESKTOP)
}

pub fn monitors(list: &Plist) -> Option<Vec<Monitor>> {
    let found: Vec<Monitor> = list
        .items()?
        .iter()
        .filter_map(|monitor| {
            let name = monitor.get("Display Identifier")?.text()?.to_string();
            let showing = monitor.get("Current Space");
            let current = showing.and_then(|space| space.get("uuid")).and_then(Plist::text).map(str::to_string);
            let full_screen = showing.is_some_and(|space| !desktop_kind(space));
            let spaces = monitor.get("Spaces").and_then(Plist::items).unwrap_or(&[]).iter().filter(|space| desktop_kind(space)).filter_map(|space| space.get("uuid")?.text().map(str::to_string)).collect();
            Some(Monitor { name, current, full_screen, spaces })
        })
        .collect();
    (!found.is_empty()).then_some(found)
}

pub fn pick<'a>(monitors: &'a [Monitor], display: &str) -> Option<&'a Monitor> {
    let named = |name: &str| monitors.iter().find(|monitor| monitor.name == name && monitor.current.is_some());
    named(display).or_else(|| named("Main"))
}

pub fn showing_from(server: Option<&[Monitor]>, display: &str) -> Showing {
    let Some(monitor) = server.and_then(|monitors| pick(monitors, display)) else { return Showing::Unknown };
    match (&monitor.current, monitor.full_screen) {
        (_, true) => Showing::FullScreen,
        (Some(space), false) => Showing::Desktop(space.clone()),
        (None, false) => Showing::Unknown,
    }
}

pub fn showing(display: &str) -> Showing {
    showing_from(window_server().as_deref(), display)
}

pub fn current(display: &str) -> Option<String> {
    match showing(display) {
        Showing::Desktop(space) => Some(space),
        _ => None,
    }
}

pub fn all_from(monitors: &[Monitor], display: &str) -> Option<Vec<String>> {
    Some(pick(monitors, display)?.spaces.clone())
}

pub fn all(display: &str) -> Option<Vec<String>> {
    window_server().and_then(|monitors| all_from(&monitors, display)).or_else(|| preferences().and_then(|monitors| all_from(&monitors, display)))
}

fn symbol(name: &CStr) -> *mut c_void {
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }
}

pub fn window_server() -> Option<Vec<Monitor>> {
    let connection = symbol(c"CGSMainConnectionID");
    let copy = symbol(c"CGSCopyManagedDisplaySpaces");
    if connection.is_null() || copy.is_null() {
        return None;
    }
    let connection: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(connection) };
    let copy: unsafe extern "C" fn(i32) -> Ref = unsafe { std::mem::transmute(copy) };
    let list = Owned::new(unsafe { copy(connection()) })?;
    monitors(&convert(unsafe { &*(list.0 as *const AnyObject) }))
}

pub fn preferences() -> Option<Vec<Monitor>> {
    let configuration = preference("com.apple.spaces", "SpacesDisplayConfiguration")?;
    monitors(convert(&configuration).get("Management Data")?.get("Monitors")?)
}

pub fn split(id: &str) -> (&str, Option<&str>) {
    match id.split_once('@') {
        Some((display, space)) => (display, Some(space)),
        None => (id, None),
    }
}

pub fn join(display: &str, space: Option<&str>) -> String {
    match space {
        Some(space) => format!("{display}@{space}"),
        None => display.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::macos::store::parse;

    fn fixture() -> Plist {
        parse(
            b"<?xml version=\"1.0\"?><plist version=\"1.0\"><array>\
              <dict><key>Display Identifier</key><string>Main</string><key>Current Space</key><dict><key>uuid</key><string>S1</string><key>type</key><integer>0</integer></dict>\
              <key>Spaces</key><array><dict><key>uuid</key><string>S1</string><key>type</key><integer>0</integer></dict><dict><key>uuid</key><string>S2</string></dict><dict><key>uuid</key><string>F1</string><key>type</key><integer>4</integer></dict></array></dict>\
              <dict><key>Display Identifier</key><string>WIDE</string><key>Current Space</key><dict><key>uuid</key><string>F2</string><key>type</key><integer>4</integer></dict>\
              <key>Spaces</key><array><dict><key>uuid</key><string>S5</string></dict><dict><key>uuid</key><string>F2</string><key>type</key><integer>4</integer></dict></array></dict>\
              <dict><key>Display Identifier</key><string>SIDE</string><key>Current Space</key><dict><key>uuid</key><string>S9</string></dict>\
              <key>Spaces</key><array><dict><key>uuid</key><string>S9</string></dict></array></dict>\
              <dict><key>Display Identifier</key><string>GONE</string></dict>\
              </array></plist>",
        )
        .unwrap()
    }

    #[test]
    fn monitors_list_their_current_and_every_space() {
        let found = monitors(&fixture()).unwrap();
        assert_eq!(found[0], Monitor { name: "Main".into(), current: Some("S1".into()), full_screen: false, spaces: vec!["S1".into(), "S2".into()] });
        assert_eq!(pick(&found, "SIDE").unwrap().current.as_deref(), Some("S9"));
        assert_eq!(pick(&found, "BUILT-IN").unwrap().name, "Main");
        assert_eq!(pick(&found, "GONE").unwrap().name, "Main");
        assert!(monitors(&Plist::Other).is_none());
    }

    #[test]
    fn full_screen_spaces_show_no_desktop_and_are_not_listed() {
        let found = monitors(&fixture()).unwrap();
        assert_eq!(showing_from(Some(&found), "WIDE"), Showing::FullScreen);
        assert_eq!(all_from(&found, "WIDE"), Some(vec!["S5".to_string()]));
        assert_eq!(all_from(&found, "Main"), Some(vec!["S1".to_string(), "S2".to_string()]));
        assert_eq!(showing_from(Some(&found), "BUILT-IN"), Showing::Desktop("S1".into()));
    }

    #[test]
    fn without_the_window_server_the_showing_space_is_unknown() {
        assert_eq!(showing_from(None, "Main"), Showing::Unknown);
        let found = monitors(&fixture()).unwrap();
        let nameless: Vec<Monitor> = found.iter().cloned().map(|monitor| Monitor { current: None, ..monitor }).collect();
        assert_eq!(showing_from(Some(&nameless), "Main"), Showing::Unknown);
    }

    #[test]
    fn ids_carry_the_space_after_an_at_sign() {
        assert_eq!(join("D1", Some("S1")), "D1@S1");
        assert_eq!(join("D1", None), "D1");
        assert_eq!(split("D1@S1"), ("D1", Some("S1")));
        assert_eq!(split("D1"), ("D1", None));
    }

    #[test]
    fn live_the_window_server_and_the_preferences_agree() {
        if std::env::var_os("SCOOCH_LIVE").is_none_or(|value| value != "1") {
            return;
        }
        let server = window_server().expect("the window server lists Spaces");
        let saved = preferences().expect("the preferences list Spaces");
        let display = server[0].name.clone();
        let now = pick(&server, &display).and_then(|monitor| monitor.current.clone());
        assert!(now.is_some());
        assert_eq!(now, pick(&saved, &display).and_then(|monitor| monitor.current.clone()));
    }
}
