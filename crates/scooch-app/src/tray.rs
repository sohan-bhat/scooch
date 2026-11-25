use crate::life::{self, Exit, Step};
use scooch::facade::{self, Started};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

pub const BACKGROUND: &str = "--background";
pub const OPEN: &str = "open";
pub const ON: &str = "on";
pub const QUIT: &str = "quit";
const SAID: &str = "said";
const COPY: &str = include_str!("../ui/src/copy/en.json");

pub struct Tray<R: Runtime> {
    _icon: TrayIcon<R>,
    menu: Menu<R>,
    on: CheckMenuItem<R>,
    said: MenuItem<R>,
    failed: Mutex<Option<bool>>,
}

pub fn background(arguments: &[String]) -> bool {
    arguments.iter().skip(1).any(|argument| argument == BACKGROUND)
}

pub fn starts(background: bool, on: bool, tray: bool) -> bool {
    !background || (on && tray)
}

#[cfg(any(target_os = "linux", test))]
pub fn offered(library: bool, watcher: impl FnOnce() -> bool) -> bool {
    library && watcher()
}

pub fn keeps_running(on: bool, window: bool, tray: bool) -> bool {
    window || (on && tray)
}

static BUSY: AtomicUsize = AtomicUsize::new(0);

pub fn follow_ends(on: bool, window: bool, busy: bool) -> bool {
    !busy && !keeps_running(on, window, true)
}

fn copy() -> &'static Value {
    static PARSED: OnceLock<Value> = OnceLock::new();
    PARSED.get_or_init(|| serde_json::from_str(COPY).unwrap_or(Value::Null))
}

pub fn words(path: &str) -> String {
    path.split('.').fold(copy(), |value, key| &value[key]).as_str().unwrap_or_default().to_string()
}

pub fn failure(code: &str) -> String {
    let said = copy()["failure"][code].as_str();
    said.or_else(|| copy()["failure"]["other"].as_str()).unwrap_or_default().to_string()
}

fn image() -> tauri::Result<Image<'static>> {
    #[cfg(target_os = "macos")]
    let bytes = include_bytes!("../icons/tray-template.png").as_slice();
    #[cfg(windows)]
    let bytes = include_bytes!("../icons/32x32.png").as_slice();
    #[cfg(not(any(target_os = "macos", windows)))]
    let bytes = include_bytes!("../icons/64x64.png").as_slice();
    Image::from_bytes(bytes)
}

pub fn made<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<Tray<R>>().is_some()
}

pub fn make<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN, words("tray.open"), true, None::<&str>)?;
    let on = CheckMenuItem::with_id(app, ON, words("tray.on"), true, facade::is_on(), None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, words("tray.quit"), true, None::<&str>)?;
    let said = MenuItem::with_id(app, SAID, "", false, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &on, &PredefinedMenuItem::separator(app)?, &quit])?;
    let builder = TrayIconBuilder::with_id("scooch").icon(image()?).tooltip(words("name")).menu(&menu);
    #[cfg(target_os = "macos")]
    let builder = builder.icon_as_template(true).show_menu_on_left_click(true);
    #[cfg(windows)]
    let builder = builder.show_menu_on_left_click(false);
    let builder = builder.on_tray_icon_event(|tray, event| {
        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
        if let TrayIconEvent::Click { button, button_state, .. } = event {
            eprintln!("scooch-app tray {button:?} {button_state:?}");
            if cfg!(windows) && button == MouseButton::Left && button_state == MouseButtonState::Up {
                crate::show_window(tray.app_handle());
            }
        }
    });
    let icon = builder.build(app)?;
    app.manage(Tray { _icon: icon, menu, on, said, failed: Mutex::new(None) });
    eprintln!("scooch-app tray made");
    Ok(())
}

fn tell<R: Runtime>(app: &AppHandle<R>, failed: Option<(String, bool)>) {
    let Some(tray) = app.try_state::<Tray<R>>() else { return };
    let mut held = tray.failed.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if held.is_some() {
        let _ = tray.menu.remove(&tray.said);
    }
    *held = None;
    if let Some((text, on)) = failed {
        let _ = tray.said.set_text(text);
        if tray.menu.insert(&tray.said, 0).is_ok() {
            *held = Some(on);
        }
    }
}

pub fn follow<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.try_state::<Tray<R>>() else { return };
    let on = facade::is_on();
    let _ = tray.on.set_checked(on);
    let stale = tray.failed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some_and(|was| was != on);
    if stale {
        tell(app, None);
    }
    let busy = BUSY.load(Ordering::SeqCst) > 0;
    let window = app.get_webview_window("main").is_some();
    if follow_ends(on, window, busy) {
        eprintln!("scooch-app tray ends, Scooch is off and no window is open");
        life::end(app, Exit::Off);
        return;
    }
    if busy {
        return;
    }
    let Some(program) = app.try_state::<life::Program>() else { return };
    match life::step(program.change(), on, window) {
        Step::Handover => {
            handover(app, &program.path);
        }
        Step::End => {
            eprintln!("scooch-app its program file is gone, so the icon ends");
            life::end(app, Exit::Gone);
        }
        Step::Stay | Step::AtClose => {}
    }
}

pub fn handover<R: Runtime>(app: &AppHandle<R>, program: &std::path::Path) -> bool {
    match life::relaunch(program) {
        Ok(()) => {
            eprintln!("scooch-app its program file was replaced, so it starts {} and ends", program.display());
            life::end(app, Exit::Replaced);
            true
        }
        Err(error) => {
            eprintln!("scooch-app could not start the new {}, {error}", program.display());
            false
        }
    }
}

pub fn pressed<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        OPEN => crate::show_window(app),
        ON => {
            let wanted = app.try_state::<Tray<R>>().map(|tray| tray.on.is_checked().unwrap_or(!facade::is_on()));
            let app = app.clone();
            BUSY.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || switch(&app, wanted.unwrap_or(!facade::is_on())));
        }
        QUIT => {
            let app = app.clone();
            BUSY.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || quit(&app));
        }
        _ => {}
    }
}

fn switch<R: Runtime>(app: &AppHandle<R>, wanted: bool) {
    #[cfg(target_os = "linux")]
    crate::appimage::seal();
    let was = facade::is_on();
    if let Some(tray) = app.try_state::<Tray<R>>() {
        let _ = tray.on.set_enabled(false);
    }
    let result = if wanted == was {
        Ok(None)
    } else if wanted {
        crate::said("turn-on", crate::switch_on(&crate::data_folder(app))).map(Some)
    } else {
        crate::said("turn-off", crate::switch_off().map(drop)).map(|_| None)
    };
    if let Some(tray) = app.try_state::<Tray<R>>() {
        let _ = tray.on.set_enabled(true);
    }
    match result {
        Ok(started) => {
            tell(app, None);
            if started == Some(Started::Picture) {
                crate::show_window(app);
            }
        }
        Err(code) if !wanted => tell(app, Some((failed_off(&code), facade::is_on()))),
        Err(code) => tell(app, Some((failure(&code), was))),
    }
    BUSY.fetch_sub(1, Ordering::SeqCst);
    follow(app);
}

fn failed_off(code: &str) -> String {
    if !facade::is_on() {
        return failure(code);
    }
    match crate::restart_daemon() {
        Ok(()) => failure(code),
        Err(error) => {
            eprintln!("scooch-app the daemon did not start again after the failed turn off, {error:#}");
            words("problem.stopped.line")
        }
    }
}

pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "linux")]
    crate::appimage::seal();
    if facade::is_on() {
        if let Err(code) = crate::said("turn-off", crate::switch_off()) {
            tell(app, Some((failed_off(&code), facade::is_on())));
            BUSY.fetch_sub(1, Ordering::SeqCst);
            follow(app);
            return;
        }
    }
    eprintln!("scooch-app quit from the menu");
    life::end(app, Exit::Quit);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(list: &[&str]) -> Vec<String> {
        list.iter().map(|each| each.to_string()).collect()
    }

    #[test]
    fn only_the_hidden_argument_after_the_program_marks_a_background_start() {
        assert!(background(&arguments(&["/Applications/Scooch.app/Contents/MacOS/scooch-app", "--background"])));
        assert!(!background(&arguments(&["/Applications/Scooch.app/Contents/MacOS/scooch-app"])));
        assert!(!background(&arguments(&["--background"])));
        assert!(!background(&arguments(&["scooch-app", "--backgrounds"])));
    }

    #[test]
    fn a_background_start_needs_scooch_on_and_a_tray() {
        assert!(starts(true, true, true));
        assert!(!starts(true, false, true));
        assert!(!starts(true, true, false));
        assert!(starts(false, false, false));
    }

    #[test]
    fn without_the_library_the_session_is_never_asked_for_a_tray() {
        assert!(!offered(false, || panic!("the session was asked")));
        assert!(offered(true, || true));
        assert!(!offered(true, || false));
    }

    #[test]
    fn the_process_stays_for_a_window_or_for_an_icon_of_something_on() {
        assert!(keeps_running(true, false, true));
        assert!(!keeps_running(false, false, true));
        assert!(!keeps_running(true, false, false));
        assert!(keeps_running(false, true, true));
        assert!(keeps_running(false, true, false));
    }

    #[test]
    fn the_watcher_ends_nothing_while_a_press_on_the_icon_runs() {
        assert!(follow_ends(false, false, false));
        assert!(!follow_ends(false, false, true));
        assert!(!follow_ends(true, false, false));
        assert!(!follow_ends(false, true, false));
    }

    #[test]
    fn the_stopped_sentence_is_in_the_copy() {
        assert_eq!(words("problem.stopped.line"), "Scooch stopped running.");
    }

    #[test]
    fn the_menu_uses_the_window_copy() {
        assert_eq!(words("tray.open"), "Open Scooch");
        assert_eq!(words("tray.on"), "On");
        assert_eq!(words("tray.quit"), "Quit Scooch");
        assert_eq!(failure("turn-off"), "Scooch did not turn off. Try again.");
        assert_eq!(failure("held"), "Another program had Scooch's settings open. Try again in a moment.");
        assert_eq!(failure("no-such-code"), "Something went wrong. Try again.");
    }

    #[test]
    fn menu_words_keep_the_copy_rules() {
        for text in [words("tray.open"), words("tray.on"), words("tray.quit"), failure("turn-off"), failure("turn-on")] {
            assert!(!text.is_empty());
            assert!(!text.contains(['-', '\u{2013}', '\u{2014}', ':', ';', '(', ')', '?']), "{text}");
        }
    }
}
