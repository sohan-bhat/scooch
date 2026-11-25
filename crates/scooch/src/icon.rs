use crate::state::{self, State, Store};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

pub const BACKGROUND: &str = "--background";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Macos,
    Windows,
    Linux,
}

pub fn layout() -> Layout {
    if cfg!(target_os = "macos") {
        Layout::Macos
    } else if cfg!(windows) {
        Layout::Windows
    } else {
        Layout::Linux
    }
}

pub fn wanted(state: &State, custom_home: bool) -> bool {
    state.on && !custom_home
}

fn named(path: &Path, name: &str) -> bool {
    path.file_name().is_some_and(|found| found == name)
}

fn bundle(path: &Path) -> bool {
    path.extension().is_some_and(|kind| kind == "app")
}

pub fn places(program: &Path, origin: Option<&Path>, layout: Layout) -> Vec<PathBuf> {
    let Some(folder) = program.parent() else { return Vec::new() };
    match layout {
        Layout::Macos => {
            let chain: Vec<&Path> = folder.ancestors().take(6).collect();
            match chain.as_slice() {
                [macos, contents, inner, helpers, outer_contents, outer]
                    if named(macos, "MacOS") && named(contents, "Contents") && bundle(inner) && named(helpers, "Helpers") && named(outer_contents, "Contents") && bundle(outer) =>
                {
                    vec![outer_contents.join("MacOS").join("scooch-app")]
                }
                _ => Vec::new(),
            }
        }
        Layout::Windows => vec![folder.join("scooch-app.exe")],
        Layout::Linux => {
            let mut found = vec![folder.join("scooch-app")];
            found.extend(origin.map(Path::to_path_buf));
            found
        }
    }
}

pub fn found(program: &Path, origin: Option<&Path>, layout: Layout) -> Option<PathBuf> {
    places(program, origin, layout).into_iter().find(|place| place.is_file())
}

static STARTED: AtomicBool = AtomicBool::new(false);

fn once(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::SeqCst)
}

pub fn after_first_tick(store: &Store) {
    if once(&STARTED) {
        show(&store.load(), &store.log_file());
    }
}

pub fn show(state: &State, log: &Path) {
    if !wanted(state, std::env::var_os("SCOOCH_HOME").is_some_and(|value| !value.is_empty())) {
        return;
    }
    let began = std::time::Instant::now();
    let Ok(exe) = std::env::current_exe() else { return };
    let program = state::canonical(&exe).unwrap_or(exe);
    let Some(app) = found(&program, state.origin.as_deref(), layout()) else { return };
    let started = launch(&app);
    let took = began.elapsed().as_secs_f64() * 1000.0;
    let line = match started {
        Ok(()) => format!("{} icon started {} in {took:.1} ms", crate::daemon::stamp(), app.display()),
        Err(error) => format!("{} icon not started from {} in {took:.1} ms, {error}", crate::daemon::stamp(), app.display()),
    };
    say(&line, log);
}

fn say(line: &str, log: &Path) {
    eprintln!("{line}");
    #[cfg(target_os = "linux")]
    if !stderr_is(log) {
        append(log, line);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = log;
}

#[cfg(target_os = "linux")]
fn stderr_is(log: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata("/proc/self/fd/2"), std::fs::metadata(log)) {
        (Ok(stderr), Ok(file)) => stderr.dev() == file.dev() && stderr.ino() == file.ino(),
        _ => false,
    }
}

#[cfg(any(target_os = "linux", all(unix, test)))]
fn append(log: &Path, line: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(unix)]
pub fn launch(app: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new(app);
    command.arg(BACKGROUND).current_dir("/").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(target_os = "macos")]
    for name in ["XPC_SERVICE_NAME", "XPC_FLAGS", "__CFBundleIdentifier"] {
        command.env_remove(name);
    }
    #[cfg(target_os = "linux")]
    crate::platform::linux::children::clean(&mut command);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            match libc::fork() {
                -1 => Err(std::io::Error::last_os_error()),
                0 => Ok(()),
                _ => libc::_exit(0),
            }
        });
    }
    command.spawn()?.wait().map(drop)
}

#[cfg(windows)]
pub fn launch(app: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let mut command = Command::new(app);
    command.arg(BACKGROUND).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    if let Some(folder) = app.parent() {
        command.current_dir(folder);
    }
    command.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(on: bool, restoring: bool) -> State {
        let mut state = crate::state::Store::at(crate::state::scratch("icon-state")).load();
        state.on = on;
        state.restoring = restoring;
        state
    }

    #[test]
    fn the_icon_starts_once_for_the_life_of_the_daemon() {
        let flag = AtomicBool::new(false);
        assert!(once(&flag));
        assert!(!once(&flag));
        assert!(!once(&flag));
    }

    #[cfg(unix)]
    #[test]
    fn the_icon_line_is_added_to_the_end_of_the_log() {
        let folder = crate::state::scratch("icon-log");
        let log = folder.join("watch.log");
        std::fs::write(&log, "1.000 set out\n").unwrap();
        append(&log, "2.000 icon started /usr/bin/scooch-app in 1.0 ms");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "1.000 set out\n2.000 icon started /usr/bin/scooch-app in 1.0 ms\n");
    }

    #[test]
    fn the_icon_is_wanted_only_while_on_and_without_a_custom_home() {
        assert!(wanted(&state(true, false), false));
        assert!(!wanted(&state(true, false), true));
        assert!(!wanted(&state(false, false), false));
        assert!(!wanted(&state(false, true), false));
    }

    #[test]
    fn on_macos_the_app_is_the_outer_bundle_of_the_helper() {
        let helper = Path::new("/Applications/Tools/Scooch.app/Contents/Helpers/Scooch.app/Contents/MacOS/scooch");
        assert_eq!(places(helper, None, Layout::Macos), vec![PathBuf::from("/Applications/Tools/Scooch.app/Contents/MacOS/scooch-app")]);
        assert!(places(Path::new("/Users/a/.local/bin/scooch"), None, Layout::Macos).is_empty());
        assert!(places(Path::new("/Applications/Scooch.app/Contents/MacOS/scooch"), None, Layout::Macos).is_empty());
        assert!(places(Path::new("/Applications/Scooch.app/Contents/Resources/Scooch.app/Contents/MacOS/scooch"), None, Layout::Macos).is_empty());
        assert!(places(Path::new("/opt/Contents/Helpers/Scooch.app/Contents/MacOS/scooch"), None, Layout::Macos).is_empty());
    }

    #[test]
    fn on_windows_the_app_sits_beside_the_daemon() {
        assert_eq!(places(Path::new("/Programs/Scooch/scoochw.exe"), None, Layout::Windows), vec![PathBuf::from("/Programs/Scooch/scooch-app.exe")]);
        assert_eq!(places(Path::new("/Programs/Scooch/scoochw.exe"), Some(Path::new("/x/Scooch.AppImage")), Layout::Windows), vec![PathBuf::from("/Programs/Scooch/scooch-app.exe")]);
    }

    #[test]
    fn on_linux_the_app_beside_the_daemon_comes_before_the_appimage() {
        assert_eq!(places(Path::new("/usr/bin/scooch"), None, Layout::Linux), vec![PathBuf::from("/usr/bin/scooch-app")]);
        assert_eq!(
            places(Path::new("/home/a/.local/bin/scooch"), Some(Path::new("/home/a/Apps/Scooch.AppImage")), Layout::Linux),
            vec![PathBuf::from("/home/a/.local/bin/scooch-app"), PathBuf::from("/home/a/Apps/Scooch.AppImage")]
        );
    }

    #[test]
    fn only_a_file_at_an_expected_place_is_started() {
        let folder = crate::state::scratch("icon-found");
        let bin = folder.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let program = bin.join("scooch");
        let image = folder.join("Scooch.AppImage");
        assert_eq!(found(&program, Some(&image), Layout::Linux), None);
        std::fs::create_dir_all(&image).unwrap();
        assert_eq!(found(&program, Some(&image), Layout::Linux), None);
        std::fs::remove_dir(&image).unwrap();
        std::fs::write(&image, b"app").unwrap();
        assert_eq!(found(&program, Some(&image), Layout::Linux), Some(image.clone()));
        std::fs::write(bin.join("scooch-app"), b"app").unwrap();
        assert_eq!(found(&program, Some(&image), Layout::Linux), Some(bin.join("scooch-app")));
    }

    #[cfg(unix)]
    #[test]
    fn a_started_app_is_no_child_and_no_group_member_of_the_daemon() {
        use std::os::unix::fs::PermissionsExt;
        let folder = crate::state::scratch("icon-launch");
        let told = folder.join("told");
        let script = folder.join("app");
        std::fs::write(&script, format!("#!/bin/sh\necho \"$1 $PPID $(ps -o pgid= -p $$)\" > '{}.part'\nmv '{}.part' '{}'\n", told.display(), told.display(), told.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        launch(&script).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !told.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let text = std::fs::read_to_string(&told).unwrap();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let own_group = unsafe { libc::getpgrp() };
        assert_eq!(parts[0], BACKGROUND);
        assert_ne!(parts[1].parse::<u32>().unwrap(), std::process::id());
        assert_ne!(parts[2].parse::<i32>().unwrap(), own_group);
    }
}
