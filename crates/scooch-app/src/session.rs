use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const WATCHER: &str = "org.kde.StatusNotifierWatcher";
pub const LIBRARIES: [&str; 2] = ["libayatana-appindicator3.so.1", "libappindicator3.so.1"];
pub const DEADLINE: Duration = Duration::from_secs(30);
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";

pub fn scheme(value: u32) -> Option<tauri::Theme> {
    match value {
        1 => Some(tauri::Theme::Dark),
        2 => Some(tauri::Theme::Light),
        _ => None,
    }
}

fn unwrapped(mut value: glib::Variant) -> Option<u32> {
    while let Some(inner) = value.as_variant() {
        value = inner;
    }
    value.get::<u32>()
}

pub fn portal_theme() -> Option<tauri::Theme> {
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    let asked = glib::Variant::tuple_from_iter([glib::Variant::from(APPEARANCE), glib::Variant::from("color-scheme")]);
    let answer = bus.call_sync(Some(PORTAL), PORTAL_PATH, SETTINGS, "Read", Some(&asked), glib::VariantTy::new("(v)").ok(), gio::DBusCallFlags::NONE, 1000, gio::Cancellable::NONE).ok()?;
    scheme(unwrapped(answer.child_value(0))?)
}

pub fn follow_theme(changed: impl Fn(tauri::Theme) + 'static) {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return };
    bus.signal_subscribe(Some(PORTAL), Some(SETTINGS), Some("SettingChanged"), Some(PORTAL_PATH), Some(APPEARANCE), gio::DBusSignalFlags::NONE, move |_, _, _, _, _, parameters| {
        if parameters.child_value(1).str() != Some("color-scheme") {
            return;
        }
        if let Some(theme) = unwrapped(parameters.child_value(2)).and_then(scheme) {
            changed(theme);
        }
    });
}

pub fn library() -> bool {
    LIBRARIES.iter().any(|name| {
        let Ok(name) = std::ffi::CString::new(*name) else { return false };
        !unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) }.is_null()
    })
}

pub fn watcher_now() -> bool {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return false };
    let asked = bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&glib::Variant::tuple_from_iter([glib::Variant::from(WATCHER)])),
        glib::VariantTy::new("(b)").ok(),
        gio::DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
    );
    asked.ok().and_then(|answer| answer.get::<(bool,)>()).is_some_and(|(owned,)| owned)
}

pub fn watcher_within(deadline: Duration) -> bool {
    let main = glib::MainLoop::new(None, false);
    let seen = Arc::new(AtomicBool::new(false));
    let (found, quit) = (seen.clone(), main.clone());
    let watch = gio::bus_watch_name(
        gio::BusType::Session,
        WATCHER,
        gio::BusNameWatcherFlags::NONE,
        move |_, _, _| {
            found.store(true, Ordering::SeqCst);
            quit.quit();
        },
        |_, _| {},
    );
    let quit = main.clone();
    let timer = glib::timeout_add_local_once(deadline, move || quit.quit());
    main.run();
    gio::bus_unwatch_name(watch);
    let appeared = seen.load(Ordering::SeqCst);
    if appeared {
        timer.remove();
    }
    appeared
}

pub fn tray(background: bool) -> bool {
    let library = library();
    if !library {
        eprintln!("scooch-app no app indicator library, so no tray");
    }
    let found = crate::tray::offered(library, || if background { watcher_within(DEADLINE) } else { watcher_now() });
    if library && !found {
        eprintln!("scooch-app no {WATCHER} on the session bus, so no tray");
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portal_scheme_picks_dark_light_or_leaves_it_to_gtk() {
        assert_eq!(scheme(1), Some(tauri::Theme::Dark));
        assert_eq!(scheme(2), Some(tauri::Theme::Light));
        assert_eq!(scheme(0), None);
        let twice = glib::Variant::from_variant(&glib::Variant::from_variant(&glib::Variant::from(1u32)));
        assert_eq!(unwrapped(twice), Some(1));
    }
}
