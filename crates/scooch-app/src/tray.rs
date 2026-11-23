use scooch::facade::{self, Started};
use serde_json::Value;
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
    let builder = builder.show_menu_on_left_click(false).on_tray_icon_event(|tray, event| {
        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            crate::show_window(tray.app_handle());
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
    if !keeps_running(on, app.get_webview_window("main").is_some(), true) {
        eprintln!("scooch-app tray ends, Scooch is off and no window is open");
        app.exit(0);
    }
}

pub fn pressed<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        OPEN => crate::show_window(app),
        ON => {
            let wanted = app.try_state::<Tray<R>>().map(|tray| tray.on.is_checked().unwrap_or(!facade::is_on()));
            let app = app.clone();
            std::thread::spawn(move || switch(&app, wanted.unwrap_or(!facade::is_on())));
        }
        QUIT => {
            let app = app.clone();
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
        Err(code) => tell(app, Some((failure(&code), was))),
    }
    follow(app);
}

pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "linux")]
    crate::appimage::seal();
    if facade::is_on() {
        if let Err(code) = crate::said("turn-off", crate::switch_off()) {
            tell(app, Some((failure(&code), true)));
            follow(app);
            return;
        }
    }
    eprintln!("scooch-app quit from the menu");
    app.exit(0);
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
