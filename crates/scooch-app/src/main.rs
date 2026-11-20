#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pictures;
mod place;
#[cfg(feature = "updater")]
mod update;
mod watch;

use scooch::facade::{self, Problem, Started, Switch, View};
use scooch::facade::Tally;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_window_state::StateFlags;

const AUTOMATION: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation";
const STARTUP: &str = "ms-settings:startupapps";
const FINDER: &str = "/System/Library/CoreServices/Finder.app";
const ISSUES: &str = "https://github.com/sohan-bhat/scooch/issues/new?title=Support%20for%20";
const KINDS: [&str; 10] = ["jpg", "jpeg", "png", "heic", "heif", "webp", "tif", "tiff", "bmp", "avif"];

struct Kept(Mutex<Option<notify::RecommendedWatcher>>);

fn said<T>(result: anyhow::Result<T>) -> Result<T, String> {
    result.map_err(|error| error.to_string())
}

async fn off_main<T: Send + 'static>(work: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Result<T, String> {
    said(tauri::async_runtime::spawn_blocking(work).await.map_err(|error| anyhow::anyhow!(error.to_string())).and_then(|done| done))
}

fn data_folder<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("scooch-app"))
}

#[tauri::command]
async fn view() -> Result<View, String> {
    off_main(|| {
        let mut view = facade::view();
        if place::misplaced() {
            view.problem = Some(Problem::Move);
        }
        Ok(view)
    })
    .await
}

#[tauri::command]
async fn turn_on() -> Result<Started, String> {
    off_main(|| facade::turn_on(&place::program()?)).await
}

#[tauri::command]
async fn turn_off() -> Result<Tally, String> {
    off_main(|| facade::turn_off(&place::program()?)).await
}

#[tauri::command]
async fn set(switch: Switch, on: bool) -> Result<(), String> {
    off_main(move || facade::set(switch, on)).await
}

#[tauri::command]
async fn use_picture<R: Runtime>(app: AppHandle<R>, path: PathBuf) -> Result<(), String> {
    let folder = data_folder(&app);
    off_main(move || {
        facade::use_picture(&place::program()?, &path)?;
        pictures::remember(&folder, &path);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn pictures<R: Runtime>(app: AppHandle<R>) -> Result<Vec<String>, String> {
    let folder = data_folder(&app);
    off_main(move || Ok(pictures::listed(&folder))).await
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
    let _ = std::fs::write(&marker, facade::PROGRAM);
    Ok(true)
}

#[tauri::command]
async fn fix<R: Runtime>(app: AppHandle<R>, problem: Problem) -> Result<(), String> {
    let opener = app.opener();
    match problem {
        Problem::Automation => said(opener.open_url(AUTOMATION, None::<&str>).map_err(anyhow::Error::from)),
        Problem::Finder => said(opener.open_path(FINDER, None::<&str>).map_err(anyhow::Error::from)),
        Problem::Startup => said(opener.open_url(STARTUP, None::<&str>).map_err(anyhow::Error::from)),
        Problem::Unknown { desktop } => said(opener.open_url(format!("{ISSUES}{}", desktop.replace(' ', "%20")), None::<&str>).map_err(anyhow::Error::from)),
        Problem::Move => {
            off_main(place::move_to_applications).await?;
            app.exit(0);
            Ok(())
        }
        Problem::Admin => {
            off_main(place::open_normally).await?;
            app.exit(0);
            Ok(())
        }
        Problem::Gnome => off_main(gnome_icons).await,
        Problem::Stopped | Problem::Other => off_main(|| facade::turn_on(&place::program()?).map(drop)).await,
        Problem::Asking | Problem::Screen | Problem::Wallpaper { .. } => Ok(()),
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

fn main() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().with_state_flags(StateFlags::SIZE | StateFlags::MAXIMIZED).build());
    #[cfg(feature = "updater")]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    builder
        .register_asynchronous_uri_scheme_protocol(pictures::SCHEME, |_context, request, responder| {
            std::thread::spawn(move || responder.respond(pictures::answer(&request)));
        })
        .invoke_handler(tauri::generate_handler![view, turn_on, turn_off, set, use_picture, pictures, pick, first_open, fix])
        .setup(|app| {
            facade::host();
            #[cfg(feature = "updater")]
            update::fetch(app.handle());
            let handle = app.handle().clone();
            let watcher = watch::follow(&facade::folder(), move || {
                let _ = handle.emit("changed", ());
            })
            .ok();
            app.manage(Kept(Mutex::new(watcher)));
            if let Some(window) = app.get_webview_window("main") {
                place_on_pointer(&window);
                let _ = window.show();
                let _ = window.set_focus();
            }
            if !place::misplaced() {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    if let Ok(program) = place::program() {
                        if facade::repair(&program).unwrap_or(false) {
                            let _ = handle.emit("changed", ());
                        }
                    }
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the Scooch window failed to start")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(kept) = app.try_state::<Kept>() {
                    kept.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
                }
                #[cfg(feature = "updater")]
                update::install(app);
            }
        });
}
