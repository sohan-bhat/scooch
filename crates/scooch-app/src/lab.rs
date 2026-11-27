use crate::life;
use std::ffi::{c_int, c_void};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};

#[link(name = "webkit2gtk-4.1")]
extern "C" {
    fn webkit_web_view_get_context(view: *mut c_void) -> *mut c_void;
    fn webkit_web_context_get_cache_model(context: *mut c_void) -> c_int;
    fn webkit_web_context_set_cache_model(context: *mut c_void, model: c_int);
}

fn mode() -> &'static str {
    static MODE: OnceLock<String> = OnceLock::new();
    MODE.get_or_init(|| std::env::var("SCOOCH_LAB").unwrap_or_default())
}

fn has(flag: &str) -> bool {
    mode().split(',').any(|part| part == flag)
}

pub fn started() {
    eprintln!("scooch-app lab mode {}", if mode().is_empty() { "none" } else { mode() });
    if has("mmap") {
        let done = unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, 131072) };
        eprintln!("scooch-app lab mmap threshold fixed at 128 kB, mallopt returned {done}");
    }
    if has("quiet") {
        std::env::set_var("WEBKIT_DISABLE_MEMORY_PRESSURE_MONITOR", "1");
        eprintln!("scooch-app lab asked WebKit for no memory pressure monitor");
    }
}

fn resident() -> u64 {
    let pages = std::fs::read_to_string("/proc/self/statm").ok().and_then(|text| text.split_whitespace().nth(1).and_then(|pages| pages.parse::<u64>().ok()));
    pages.unwrap_or(0) * 4
}

fn trim_later() {
    std::thread::spawn(|| {
        for (wait, at) in [(5u64, 5u64), (55, 60)] {
            std::thread::sleep(Duration::from_secs(wait));
            let before = resident();
            let freed = unsafe { libc::malloc_trim(0) };
            eprintln!("scooch-app lab trim {at} s after the close returned {freed}, resident {before} kB before and {} kB after", resident());
        }
    });
}

pub fn opened<R: Runtime>(window: &WebviewWindow<R>) {
    if !has("cache") {
        return;
    }
    let asked = window.with_webview(|webview| {
        use glib::object::ObjectType;
        let view = webview.inner();
        unsafe {
            let context = webkit_web_view_get_context(view.as_ptr() as *mut c_void);
            let before = webkit_web_context_get_cache_model(context);
            webkit_web_context_set_cache_model(context, 0);
            eprintln!("scooch-app lab cache model was {before} and is {}", webkit_web_context_get_cache_model(context));
        }
    });
    if let Err(error) = asked {
        eprintln!("scooch-app lab cache model not set, {error}");
    }
}

fn items(bus: &gio::DBusConnection) -> Vec<String> {
    let asked = glib::Variant::tuple_from_iter([glib::Variant::from(crate::session::WATCHER), glib::Variant::from("RegisteredStatusNotifierItems")]);
    let answer = bus.call_sync(Some(crate::session::WATCHER), "/StatusNotifierWatcher", "org.freedesktop.DBus.Properties", "Get", Some(&asked), glib::VariantTy::new("(v)").ok(), gio::DBusCallFlags::NONE, 1000, gio::Cancellable::NONE);
    answer.ok().and_then(|answer| answer.child_value(0).as_variant()).and_then(|inner| inner.get::<Vec<String>>()).unwrap_or_default()
}

fn owner(bus: &gio::DBusConnection, name: &str) -> Option<u32> {
    let asked = glib::Variant::tuple_from_iter([glib::Variant::from(name)]);
    let answer = bus.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", Some(&asked), glib::VariantTy::new("(u)").ok(), gio::DBusCallFlags::NONE, 1000, gio::Cancellable::NONE);
    answer.ok().and_then(|answer| answer.get::<(u32,)>()).map(|(pid,)| pid)
}

fn another(bus: &gio::DBusConnection, before: &[String]) -> bool {
    items(bus).iter().filter(|entry| !before.contains(entry)).any(|entry| {
        let name = entry.split('/').next().unwrap_or(entry);
        owner(bus, name).is_some_and(|pid| pid != std::process::id() && std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|comm| comm.trim() == "scooch-app"))
    })
}

fn overlap<R: Runtime>(app: &AppHandle<R>, path: PathBuf) {
    let before = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).map(|bus| items(&bus)).unwrap_or_default();
    tauri_plugin_single_instance::destroy(app);
    crate::let_go(app);
    let started = Instant::now();
    if let Err(error) = life::relaunch(&path) {
        eprintln!("scooch-app lab fresh-overlap could not start the next process, {error}");
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok();
        let mut seen = false;
        while started.elapsed() < Duration::from_secs(10) {
            if bus.as_ref().is_some_and(|bus| another(bus, &before)) {
                seen = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        eprintln!("scooch-app lab fresh-overlap saw the next icon {seen} after {} ms", started.elapsed().as_millis());
        life::end(&app, life::Exit::Replaced);
    });
}

pub fn ends_at_close<R: Runtime>(app: &AppHandle<R>) -> bool {
    let program = app.try_state::<life::Program>().map(|program| program.path.clone());
    if has("trim") {
        trim_later();
    }
    if has("fresh-gap") {
        let started = program.is_some_and(|path| life::relaunch(&path).is_ok());
        eprintln!("scooch-app lab fresh-gap started the next process {started}");
        return started;
    }
    if has("fresh-overlap") {
        if let Some(path) = program {
            overlap(app, path);
        }
    }
    false
}
