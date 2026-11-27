#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appimage;
#[cfg(target_os = "linux")]
mod children;
mod life;
mod lock;
mod pictures;
mod place;
#[cfg(target_os = "macos")]
mod power;
#[cfg(target_os = "linux")]
mod session;
#[cfg(unix)]
mod takeover;
mod tray;
#[cfg(feature = "updater")]
mod update;
mod watch;

use scooch::facade::{self, Icons, Problem, Started, Switch, View};
use scooch::facade::Tally;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Runtime, Theme, WebviewWindow, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_window_state::StateFlags;

const AUTOMATION: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation";
const STARTUP: &str = "ms-settings:startupapps";
const FINDER: &str = "/System/Library/CoreServices/Finder.app";
const ISSUES: &str = "https://github.com/sohan-bhat/scooch/issues/new?title=Support%20for%20";
const KINDS: [&str; 10] = ["jpg", "jpeg", "png", "heic", "heif", "webp", "tif", "tiff", "bmp", "avif"];

struct Kept(Mutex<Option<watch::Watch>>);

struct Icon(Mutex<Option<lock::Held>>);

#[cfg(any(target_os = "macos", target_os = "linux", test))]
const IDENTIFIER: &str = "me.sohanbhat.scooch.window";

fn let_go<R: Runtime>(app: &AppHandle<R>) {
    if let Some(icon) = app.try_state::<Icon>() {
        icon.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    }
}

#[cfg(target_os = "macos")]
fn plugin_usable() -> bool {
    use std::os::unix::fs::MetadataExt;
    let socket = format!("/tmp/{}_si.sock", IDENTIFIER.replace(['.', '-'], "_"));
    std::fs::symlink_metadata(socket).map_or(true, |meta| meta.uid() == unsafe { libc::getuid() })
}

#[cfg(not(target_os = "macos"))]
fn plugin_usable() -> bool {
    true
}

#[cfg(target_os = "linux")]
fn handed(arguments: &[String]) -> bool {
    use glib::ToVariant;
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return false };
    let name = format!("{IDENTIFIER}.SingleInstance");
    let path = format!("/{}", name.replace('.', "/").replace('-', "_"));
    let folder = std::env::current_dir().unwrap_or_default().to_string_lossy().into_owned();
    let asked = glib::Variant::tuple_from_iter([arguments.to_variant(), folder.to_variant()]);
    bus.call_sync(Some(&name), &path, "org.SingleInstance.DBus", "ExecuteCallback", Some(&asked), None, gio::DBusCallFlags::NO_AUTO_START, 1000, gio::Cancellable::NONE).is_ok()
}

#[cfg(target_os = "linux")]
fn entered(folder: &std::path::Path, arguments: &[String]) -> life::Entry<lock::Held> {
    if let Some(held) = lock::take(folder) {
        return life::Entry::Holds(held);
    }
    let patience = if session::watcher_now() { life::ANSWER } else { std::time::Duration::ZERO };
    life::enter(patience, life::PAUSE, || lock::take(folder), || handed(arguments))
}

const SAVES: [(&str, &str); 3] = [("so try again in a moment.", "held"), ("so check that the disk has room.", "full"), ("so check that its folder can be written.", "unwritable")];

fn cause(error: &anyhow::Error) -> Option<&'static str> {
    error.chain().find_map(|part| {
        let text = part.to_string();
        text.starts_with("Scooch could not").then(|| SAVES.iter().find(|(ending, _)| text.ends_with(ending)).map(|(_, code)| *code)).flatten()
    })
}

fn said<T>(code: &'static str, result: anyhow::Result<T>) -> Result<T, String> {
    result.map_err(|error| {
        eprintln!("scooch-app {code} failed: {error:#}");
        cause(&error).unwrap_or(code).to_string()
    })
}

fn adopted<T>(result: anyhow::Result<T>) -> anyhow::Result<T> {
    #[cfg(target_os = "linux")]
    children::reap_daemons();
    result
}

fn taken_over(program: &std::path::Path) {
    #[cfg(unix)]
    for done in takeover::take_over(program, cfg!(target_os = "macos") || std::env::var_os("APPIMAGE").is_some()) {
        eprintln!("scooch-app {done}");
    }
    #[cfg(not(unix))]
    let _ = program;
}

fn ready() -> anyhow::Result<PathBuf> {
    let program = place::ready()?;
    taken_over(&program);
    Ok(program)
}

async fn off_main<T: Send + 'static>(code: &'static str, work: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    #[cfg(target_os = "linux")]
    appimage::seal();
    said(code, tauri::async_runtime::spawn_blocking(work).await.map_err(|error| anyhow::anyhow!(error.to_string())).and_then(|done| done))
}

fn data_folder<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("scooch-app"))
}

fn current(folder: &std::path::Path) -> View {
    let mut view = facade::view();
    if place::misplaced() {
        view.problem = Some(Problem::Move);
    }
    if view.on {
        pictures::select(folder, None);
    } else {
        pictures::shown(&mut view, pictures::selected(folder).as_deref());
    }
    view
}

#[tauri::command]
async fn view<R: Runtime>(app: AppHandle<R>) -> Result<View, String> {
    let folder = data_folder(&app);
    off_main("view", move || Ok(current(&folder))).await
}

fn switch_on(folder: &std::path::Path) -> anyhow::Result<Started> {
    let program = ready()?;
    match pictures::selected(folder).filter(|_| !facade::is_on()) {
        Some(selection) => {
            adopted(facade::use_picture(&program, &selection))?;
            pictures::select(folder, None);
            Ok(Started::Go)
        }
        None => adopted(facade::turn_on(&program)),
    }
}

fn switch_off() -> anyhow::Result<Tally> {
    adopted(facade::turn_off(&place::program()?))
}

pub fn restart_daemon() -> anyhow::Result<()> {
    adopted(facade::turn_on(&ready()?)).map(drop)
}

#[tauri::command]
async fn turn_on<R: Runtime>(app: AppHandle<R>) -> Result<Started, String> {
    let folder = data_folder(&app);
    off_main("turn-on", move || switch_on(&folder)).await
}

#[tauri::command]
async fn turn_off() -> Result<Tally, String> {
    off_main("turn-off", switch_off).await
}

#[tauri::command]
async fn set(switch: Switch, on: bool) -> Result<(), String> {
    off_main("set", move || facade::set(switch, on)).await
}

#[tauri::command]
async fn use_picture<R: Runtime>(app: AppHandle<R>, path: PathBuf) -> Result<(), String> {
    let folder = data_folder(&app);
    off_main("use", move || {
        if facade::is_on() {
            adopted(facade::use_picture(&ready()?, &path))?;
            pictures::select(&folder, None);
        } else {
            pictures::select(&folder, Some(&facade::select_picture(&path)?));
        }
        pictures::remember(&folder, &path);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn pictures<R: Runtime>(app: AppHandle<R>) -> Result<Vec<String>, String> {
    let folder = data_folder(&app);
    off_main("pictures", move || Ok(pictures::listed(&folder))).await
}

#[tauri::command]
async fn pick<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    let chosen = app.dialog().file().add_filter("Pictures", &KINDS).blocking_pick_file();
    Ok(chosen.and_then(|file| file.into_path().ok()).map(|path| path.to_string_lossy().into_owned()))
}

#[tauri::command]
async fn first_open<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    let marker = data_folder(&app).join("opened");
    if marker.exists() || place::misplaced() {
        return Ok(false);
    }
    let _ = std::fs::create_dir_all(data_folder(&app));
    let _ = pictures::save(&marker, facade::PROGRAM.as_bytes());
    Ok(true)
}

#[cfg(target_os = "linux")]
fn open_link<R: Runtime>(_: &AppHandle<R>, url: String) -> anyhow::Result<()> {
    let mut last = None;
    for program in [&["xdg-open"][..], &["gio", "open"][..]] {
        let mut command = std::process::Command::new(program[0]);
        command.args(&program[1..]).arg(&url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        match appimage::apply(&mut command).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
                return Ok(());
            }
            Err(error) => last = Some(error),
        }
    }
    Err(last.map(anyhow::Error::from).unwrap_or_else(|| anyhow::anyhow!("no opener")))
}

#[cfg(not(target_os = "linux"))]
fn open_link<R: Runtime>(app: &AppHandle<R>, url: String) -> anyhow::Result<()> {
    app.opener().open_url(url, None::<&str>).map_err(anyhow::Error::from)
}

#[tauri::command]
async fn fix<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let folder = data_folder(&app);
    let Some(problem) = off_main("view", move || Ok(current(&folder).problem)).await? else { return Ok(()) };
    let opener = app.opener();
    match problem {
        Problem::Automation => said("open", opener.open_url(AUTOMATION, None::<&str>).map_err(anyhow::Error::from)),
        Problem::Finder => said("open", opener.open_path(FINDER, None::<&str>).map_err(anyhow::Error::from)),
        Problem::Startup => said("open", opener.open_url(STARTUP, None::<&str>).map_err(anyhow::Error::from)),
        Problem::Unknown { desktop } => said("open", open_link(&app, format!("{ISSUES}{}", percent_encoding::utf8_percent_encode(&desktop, percent_encoding::NON_ALPHANUMERIC)))),
        Problem::Move => {
            let target = off_main("move", place::move_to_applications).await?;
            tauri_plugin_single_instance::destroy(&app);
            let_go(&app);
            said("move", place::open_app(&target))?;
            life::end(&app, life::Exit::Moving);
            Ok(())
        }
        Problem::Admin => {
            tauri_plugin_single_instance::destroy(&app);
            let_go(&app);
            off_main("admin", place::open_normally).await?;
            life::end(&app, life::Exit::Moving);
            Ok(())
        }
        Problem::Gnome { icons: Icons::Off } => off_main("gnome-icons", gnome_icons).await,
        Problem::Stopped | Problem::Other => off_main("start", || adopted(facade::turn_on(&ready()?)).map(drop)).await,
        Problem::Asking | Problem::Screen | Problem::Wallpaper { .. } | Problem::Gnome { .. } => Ok(()),
    }
}

#[cfg(target_os = "linux")]
fn gnome_icons() -> anyhow::Result<()> {
    facade::show_gnome_icons()
}

#[cfg(not(target_os = "linux"))]
fn gnome_icons() -> anyhow::Result<()> {
    Ok(())
}

fn paint<R: Runtime>(window: &WebviewWindow<R>, theme: Theme) {
    let color = if theme == Theme::Light { Color(239, 231, 214, 255) } else { Color(20, 17, 14, 255) };
    let _ = window.set_background_color(Some(color));
}

fn place_on_pointer<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(pointer) = window.cursor_position() else { return };
    let Ok(monitors) = window.available_monitors() else { return };
    let under = monitors.iter().find(|monitor| {
        let (at, size) = (monitor.position(), monitor.size());
        pointer.x >= at.x as f64 && pointer.y >= at.y as f64 && pointer.x < at.x as f64 + size.width as f64 && pointer.y < at.y as f64 + size.height as f64
    });
    let Some(monitor) = under.or_else(|| monitors.first()) else { return };
    let Ok(outer) = window.outer_size() else { return };
    let (at, size) = (monitor.position(), monitor.size());
    let x = at.x + (size.width.saturating_sub(outer.width) / 2) as i32;
    let y = at.y + (size.height.saturating_sub(outer.height) / 2) as i32;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

static PRESENTING: Mutex<()> = Mutex::new(());
static SHOWN: life::Gate = life::Gate::new();
static DOOR: life::Door = life::Door::new();

fn made_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    #[allow(unused_mut)]
    let mut config = app.config().app.windows.iter().find(|window| window.label == "main").cloned().ok_or(tauri::Error::WindowNotFound)?;
    #[cfg(target_os = "linux")]
    if config.theme.is_none() {
        config.theme = session::portal_theme();
    }
    let window = tauri::WebviewWindowBuilder::from_config(app, &config)?.build()?;
    if let Ok(theme) = window.theme() {
        paint(&window, theme);
    }
    place_on_pointer(&window);
    Ok(window)
}

fn present<R: Runtime>(app: &AppHandle<R>) {
    let _presenting = PRESENTING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    loop {
        let closes = DOOR.quiet();
        let window = match app.get_webview_window("main") {
            Some(window) => window,
            None => match made_window(app) {
                Ok(window) => window,
                Err(error) => {
                    eprintln!("scooch-app window failed: {error}");
                    return;
                }
            },
        };
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if DOOR.quiet() == closes {
            break;
        }
    }
    if SHOWN.first() {
        first_window(app);
    }
}

fn first_window<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(feature = "updater")]
    update::fetch(app);
    let handle = app.clone();
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        appimage::seal();
        let _ = facade::set_origin(std::env::var_os("APPIMAGE").map(PathBuf::from).as_deref());
        if !place::misplaced() {
            if let Ok(program) = place::program() {
                if facade::reclaim(&program) {
                    let _ = handle.emit("changed", ());
                }
            }
        }
        if !place::misplaced() && facade::is_on() {
            if let Ok(program) = place::ready() {
                if adopted(facade::repair(&program)).unwrap_or(false) {
                    let _ = handle.emit("changed", ());
                }
                taken_over(&program);
            }
        }
        let view = facade::view();
        eprintln!("scooch-app live {} reduce motion {} problem {}", view.live, view.reduce_motion, serde_json::to_string(&view.problem).unwrap_or_default());
    });
}

#[cfg(feature = "updater")]
fn update_waiting<R: Runtime>(app: &AppHandle<R>) -> bool {
    update::waiting(app)
}

#[cfg(not(feature = "updater"))]
fn update_waiting<R: Runtime>(_app: &AppHandle<R>) -> bool {
    false
}

fn own_change<R: Runtime>(app: &AppHandle<R>) -> life::Change {
    app.try_state::<life::Program>().map_or(life::Change::Same, |program| program.change())
}

fn decided<R: Runtime>(app: &AppHandle<R>) -> life::Close {
    DOOR.settle();
    life::at_close(facade::is_on(), tray::made(app), update_waiting(app), own_change(app), fresh_at_close())
}

fn ended<R: Runtime>(app: &AppHandle<R>) {
    if let Some(kept) = app.try_state::<Kept>() {
        kept.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    }
    let exit = life::reason();
    let icon = tray::made(app);
    eprintln!("scooch-app ends, {exit:?}, holding the icon {icon}");
    #[cfg(feature = "updater")]
    if life::installs(exit) && update::install(app) && icon && facade::is_on() && cfg!(not(windows)) {
        if let Some(program) = app.try_state::<life::Program>() {
            let_go(app);
            let _ = life::relaunch(&program.path);
        }
    }
    #[cfg(target_os = "macos")]
    if life::restarts(exit, icon, facade::is_on(), power::going()) {
        if let Some(program) = app.try_state::<life::Program>() {
            match life::relaunch(&program.path) {
                Ok(()) => eprintln!("scooch-app the system ended it with Scooch on, so it starts the icon again"),
                Err(error) => eprintln!("scooch-app could not start the icon again, {error}"),
            }
        }
    }
}

fn show_window<R: Runtime>(app: &AppHandle<R>) {
    if !DOOR.opens() {
        let program = app.try_state::<life::Program>().map(|program| program.path.clone());
        let started = program.is_some_and(|path| life::reopen(&path).is_ok());
        eprintln!("scooch-app a start reached this process as it ends, started again {started}");
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        present(&app);
        DOOR.opened();
    });
}

#[cfg(target_os = "linux")]
fn tray_offered(background: bool) -> bool {
    session::tray(background)
}

#[cfg(not(target_os = "linux"))]
fn tray_offered(_background: bool) -> bool {
    true
}

#[cfg(target_os = "linux")]
fn fresh_at_close() -> bool {
    session::watcher_now()
}

#[cfg(not(target_os = "linux"))]
fn fresh_at_close() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn app_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    let menu = tauri::menu::Menu::default(app)?;
    if let Some(tauri::menu::MenuItemKind::Submenu(first)) = menu.items()?.into_iter().next() {
        let count = first.items()?.len();
        if count > 0 {
            first.remove_at(count - 1)?;
        }
        first.append(&tauri::menu::MenuItem::with_id(app, tray::QUIT, tray::words("tray.quit"), true, Some("CmdOrCtrl+Q"))?)?;
    }
    Ok(menu)
}

fn main() {
    #[cfg(target_os = "linux")]
    facade::child_env(appimage::changes());
    let arguments: Vec<String> = std::env::args_os().map(|argument| argument.to_string_lossy().into_owned()).collect();
    let background = tray::background(&arguments);
    let on = facade::is_on();
    if background && !on {
        eprintln!("scooch-app background start with Scooch off ends");
        return;
    }
    let folder = facade::folder();
    #[cfg(target_os = "linux")]
    let entry = if background { None } else { Some(entered(&folder, &arguments)) };
    #[cfg(target_os = "linux")]
    let held = match entry {
        Some(life::Entry::Holds(held)) => Some(held),
        Some(life::Entry::Alone) => None,
        Some(life::Entry::Sent) => {
            eprintln!("scooch-app another process holds the icon and shows the window");
            return;
        }
        None if life::waits(&arguments) => lock::wait(&folder, life::HANDOVER),
        None => lock::take(&folder),
    };
    #[cfg(not(target_os = "linux"))]
    let held = if life::waits(&arguments) { lock::wait(&folder, life::HANDOVER) } else { lock::take(&folder) };
    let start = life::start(background, held.is_some());
    if start == life::Start::End {
        eprintln!("scooch-app background start while another process holds the icon ends");
        return;
    }
    let offered = start == life::Start::Icon && tray_offered(background);
    if !tray::starts(background, on, offered) {
        eprintln!("scooch-app background start without a tray ends");
        return;
    }
    if start == life::Start::Window {
        eprintln!("scooch-app another process holds the icon, so this window comes without one");
    }
    let program = held.as_ref().and_then(|_| life::Program::own());
    let builder = tauri::Builder::default();
    let builder = if !life::answers(held.is_some(), cfg!(target_os = "linux")) {
        builder
    } else if plugin_usable() {
        builder.plugin(tauri_plugin_single_instance::init(|app, arguments, _folder| {
            if !tray::background(&arguments) {
                show_window(app);
            }
        }))
    } else {
        eprintln!("scooch-app the single instance socket belongs to another account, so this start goes without it");
        builder
    };
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().with_state_flags(StateFlags::SIZE | StateFlags::MAXIMIZED).build());
    #[cfg(feature = "updater")]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu);
    builder
        .on_menu_event(|app, event| tray::pressed(app, event.id().as_ref()))
        .register_asynchronous_uri_scheme_protocol(pictures::SCHEME, |_context, request, responder| {
            std::thread::spawn(move || responder.respond(pictures::answer(&request)));
        })
        .on_window_event(|window, event| {
            #[cfg(target_os = "linux")]
            if let WindowEvent::CloseRequested { .. } = event {
                DOOR.closes();
            }
            if let WindowEvent::ThemeChanged(theme) = event {
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
                    paint(&webview, *theme);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![view, turn_on, turn_off, set, use_picture, pictures, pick, first_open, fix])
        .setup(move |app| {
            facade::host();
            app.manage(Icon(Mutex::new(held)));
            if let Some(program) = program {
                app.manage(program);
            }
            #[cfg(target_os = "macos")]
            {
                power::watch();
                if background {
                    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
            let handle = app.handle().clone();
            let watcher = watch::follow(&facade::folder(), move || {
                let _ = handle.emit("changed", ());
                tray::follow(&handle);
            })
            .ok();
            app.manage(Kept(Mutex::new(watcher)));
            #[cfg(target_os = "linux")]
            {
                let handle = app.handle().clone();
                session::follow_theme(move |theme| {
                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.set_theme(Some(theme));
                    }
                });
            }
            if offered {
                if let Err(error) = tray::make(app.handle()) {
                    eprintln!("scooch-app tray failed: {error}");
                }
            }
            if life::ends_without_icon(background, tray::made(app.handle())) {
                eprintln!("scooch-app background start whose icon could not be made ends");
                life::end(app.handle(), life::Exit::NoIcon);
                return Ok(());
            }
            if !background {
                present(app.handle());
            }
            tray::follow(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the Scooch window failed to start")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { code: None, api, .. } => match decided(app) {
                life::Close::Keep => {
                    api.prevent_exit();
                    #[cfg(target_os = "macos")]
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    eprintln!("scooch-app window closed, the icon stays");
                }
                life::Close::Handover => {
                    let program = app.try_state::<life::Program>().map(|program| program.path.clone());
                    if program.is_some_and(|path| life::relaunch(&path).is_ok()) {
                        life::mark(life::Exit::Replaced);
                        eprintln!("scooch-app window closed over a replaced program, which takes over the icon");
                    } else {
                        api.prevent_exit();
                        eprintln!("scooch-app window closed, the replaced program did not start, the icon stays");
                    }
                }
                life::Close::Fresh => {
                    #[cfg(target_os = "linux")]
                    appimage::seal();
                    let program = app.try_state::<life::Program>().map(|program| program.path.clone());
                    match life::leave(&DOOR, || program.is_some_and(|path| life::relaunch(&path).is_ok())) {
                        life::Leave::Gone => {
                            tauri_plugin_single_instance::destroy(app);
                            life::mark(life::Exit::Fresh);
                            eprintln!("scooch-app window closed, a fresh process takes over the icon");
                        }
                        life::Leave::Opening => {
                            api.prevent_exit();
                            eprintln!("scooch-app window closed while another opens, the icon stays");
                        }
                        life::Leave::Failed => {
                            api.prevent_exit();
                            eprintln!("scooch-app window closed, a fresh process did not start, the icon stays");
                        }
                    }
                }
                life::Close::End(exit) => life::mark(exit),
            },
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => show_window(app),
            tauri::RunEvent::Exit => ended(app),
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identifier_is_the_one_in_the_tauri_config() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], IDENTIFIER);
    }

    #[test]
    fn a_failure_reaches_the_window_as_its_code_only() {
        let failed: anyhow::Result<()> = Err(anyhow::anyhow!("Permission denied (os error 13)"));
        assert_eq!(said("move", failed), Err("move".to_string()));
    }

    #[test]
    fn a_failed_save_reaches_the_window_as_its_cause() {
        for (sentence, code) in [
            ("Scooch could not replace C:\\Scooch\\state.json because another program had it open, so try again in a moment.", "held"),
            ("Scooch could not write /data/state.json, so check that the disk has room.", "full"),
            ("Scooch could not write /data/state.json, so check that its folder can be written.", "unwritable"),
        ] {
            let failed: anyhow::Result<()> = Err(anyhow::Error::new(std::io::Error::from_raw_os_error(5)).context(sentence).context("turning off"));
            assert_eq!(said("turn-off", failed), Err(code.to_string()));
        }
        let other: anyhow::Result<()> = Err(anyhow::anyhow!("Scooch could not reach the X display, so check that DISPLAY is set."));
        assert_eq!(said("turn-off", other), Err("turn-off".to_string()));
    }
}
