use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const WATCHER: &str = "org.kde.StatusNotifierWatcher";
pub const LIBRARIES: [&str; 2] = ["libayatana-appindicator3.so.1", "libappindicator3.so.1"];
pub const DEADLINE: Duration = Duration::from_secs(30);

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
