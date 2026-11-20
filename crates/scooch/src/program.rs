use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seen {
    Renamed,
    Deleted,
    Revoked,
    Written,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    Off(PathBuf),
    Repoint(PathBuf),
    Nothing(&'static str),
}

pub struct After<'a> {
    pub now: Option<&'a Path>,
    pub back: bool,
}

pub fn translocated(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "AppTranslocation")
}

pub fn watched(program: &Path, custom_home: bool, read_only: bool) -> Option<PathBuf> {
    if custom_home || read_only || translocated(program) {
        return None;
    }
    let outermost = program.ancestors().filter(|folder| folder.extension().is_some_and(|kind| kind == "app")).last();
    Some(outermost.unwrap_or(program).to_path_buf())
}

fn plain(path: &Path) -> PathBuf {
    match path.strip_prefix("/System/Volumes/Data") {
        Ok(rest) => Path::new("/").join(rest),
        Err(_) => path.to_path_buf(),
    }
}

pub fn trashed(path: &Path, home: &Path) -> bool {
    let path = plain(path);
    let user = home.join(".Trash");
    if path.starts_with(&user) && path != user {
        return true;
    }
    let parts: Vec<Component> = path.components().collect();
    let named = |part: &Component, name: &str| part.as_os_str() == name;
    match parts.as_slice() {
        [Component::RootDir, trash, _, _, ..] if named(trash, ".Trashes") => true,
        [Component::RootDir, volumes, _, trash, _, _, ..] => named(volumes, "Volumes") && named(trash, ".Trashes"),
        _ => false,
    }
}

pub fn decide(original: &Path, seen: Seen, after: &After, home: &Path) -> Decision {
    match seen {
        Seen::Deleted => Decision::Nothing("was deleted"),
        Seen::Revoked => Decision::Nothing("lost its disk"),
        Seen::Written => Decision::Nothing("changed in place"),
        Seen::Renamed if after.back => Decision::Nothing("moved but a program is back at its old place"),
        Seen::Renamed => match after.now {
            None => Decision::Nothing("moved and could not be found after the wait"),
            Some(now) if trashed(now, home) => Decision::Off(now.to_path_buf()),
            Some(now) if plain(now) == plain(original) => Decision::Nothing("moved back to its old place"),
            Some(now) => Decision::Repoint(now.to_path_buf()),
        },
    }
}

pub fn moved_program(program: &Path, from: &Path, to: &Path) -> PathBuf {
    match program.strip_prefix(from) {
        Ok(inside) if !inside.as_os_str().is_empty() => to.join(inside),
        _ => to.to_path_buf(),
    }
}

pub fn said(decision: &Decision, at: &Path) -> String {
    match decision {
        Decision::Off(now) => format!("program moved to the Trash at {}, so Scooch turns off and puts the wallpaper back", now.display()),
        Decision::Repoint(now) => format!("program moved from {} to {}, so the login item now starts it there", at.display(), now.display()),
        Decision::Nothing(why) => format!("program at {} {why}, so nothing changes", at.display()),
    }
}

#[cfg(target_os = "macos")]
pub fn watch_self(trashed: std::sync::Arc<std::sync::atomic::AtomicBool>, signal: std::sync::Arc<crate::daemon::Signal>) {
    use crate::daemon::{stamp, trace};
    let Ok(exe) = std::env::current_exe() else { return };
    let program = crate::state::canonical(&exe).unwrap_or(exe);
    let custom = std::env::var_os("SCOOCH_HOME").is_some_and(|value| !value.is_empty());
    let Some(home) = crate::platform::macos::real_home() else { return };
    let Some(target) = watched(&program, custom, mac::read_only(&program)) else {
        if trace() {
            eprintln!("{} program not watched at {}", stamp(), program.display());
        }
        return;
    };
    let act = move |decision: &Decision, at: &Path, program: &Path| {
        eprintln!("{} {}", stamp(), said(decision, at));
        match decision {
            Decision::Off(_) => {
                trashed.store(true, std::sync::atomic::Ordering::SeqCst);
                signal.wake();
            }
            Decision::Repoint(now) => {
                if let Err(error) = crate::platform::macos::launchd::repoint(&moved_program(program, at, &plain(now))) {
                    eprintln!("{} program moved but the login item could not follow it, {error:#}", stamp());
                }
            }
            Decision::Nothing(_) => {}
        }
    };
    let _ = std::thread::Builder::new().name("scooch-program".into()).spawn(move || mac::run(program, target, home, act));
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{decide, moved_program, After, Decision, Seen};
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const SETTLE: Duration = Duration::from_secs(10);

    pub fn read_only(path: &Path) -> bool {
        let Ok(name) = CString::new(path.as_os_str().as_bytes()) else { return false };
        let mut found: libc::statfs = unsafe { std::mem::zeroed() };
        unsafe { libc::statfs(name.as_ptr(), &mut found) == 0 && found.f_flags & libc::MNT_RDONLY as u32 != 0 }
    }

    fn path_of(fd: libc::c_int) -> Option<PathBuf> {
        let mut buffer = vec![0u8; libc::MAXPATHLEN as usize];
        if unsafe { libc::fcntl(fd, libc::F_GETPATH, buffer.as_mut_ptr()) } < 0 {
            return None;
        }
        let end = buffer.iter().position(|byte| *byte == 0)?;
        Some(PathBuf::from(OsStr::from_bytes(&buffer[..end])))
    }

    pub fn run(mut program: PathBuf, mut target: PathBuf, home: PathBuf, act: impl Fn(&Decision, &Path, &Path)) {
        let Ok(name) = CString::new(target.as_os_str().as_bytes()) else { return };
        let fd = unsafe { libc::open(name.as_ptr(), libc::O_EVTONLY) };
        if fd < 0 {
            return;
        }
        let queue = unsafe { libc::kqueue() };
        let mut change: libc::kevent = unsafe { std::mem::zeroed() };
        change.ident = fd as libc::uintptr_t;
        change.filter = libc::EVFILT_VNODE;
        change.flags = libc::EV_ADD | libc::EV_CLEAR;
        change.fflags = libc::NOTE_DELETE | libc::NOTE_RENAME | libc::NOTE_REVOKE | libc::NOTE_WRITE | libc::NOTE_EXTEND;
        if queue >= 0 && unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) } == 0 {
            if crate::daemon::trace() {
                eprintln!("{} program watched at {}", crate::daemon::stamp(), target.display());
            }
            loop {
                let mut event: libc::kevent = unsafe { std::mem::zeroed() };
                let got = unsafe { libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null()) };
                if got < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                if got <= 0 {
                    break;
                }
                let flags = event.fflags;
                let seen = if flags & libc::NOTE_REVOKE != 0 {
                    Seen::Revoked
                } else if flags & libc::NOTE_DELETE != 0 {
                    Seen::Deleted
                } else if flags & libc::NOTE_RENAME != 0 {
                    Seen::Renamed
                } else {
                    Seen::Written
                };
                if seen == Seen::Renamed {
                    std::thread::sleep(SETTLE);
                }
                let now = path_of(fd);
                let decision = decide(&target, seen, &After { now: now.as_deref(), back: target.exists() }, &home);
                act(&decision, &target, &program);
                match decision {
                    Decision::Repoint(now) => {
                        program = moved_program(&program, &target, &now);
                        target = now;
                    }
                    Decision::Off(_) => break,
                    Decision::Nothing(_) if matches!(seen, Seen::Deleted | Seen::Revoked) => break,
                    Decision::Nothing(_) => {}
                }
            }
        }
        unsafe {
            if queue >= 0 {
                libc::close(queue);
            }
            libc::close(fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/ada")
    }

    fn original() -> PathBuf {
        PathBuf::from("/Applications/Scooch.app")
    }

    fn trash() -> PathBuf {
        PathBuf::from("/Users/ada/.Trash/Scooch.app")
    }

    #[test]
    fn only_a_users_trash_or_a_volumes_trashes_counts_as_trashed() {
        let home = home();
        for path in ["/Users/ada/.Trash/Scooch.app", "/System/Volumes/Data/Users/ada/.Trash/Scooch.app", "/Users/ada/.Trash/Scooch 10.42.07.app", "/Volumes/Spare/.Trashes/501/Scooch.app", "/.Trashes/501/Scooch.app"] {
            assert!(trashed(Path::new(path), &home), "{path}");
        }
        for path in ["/Users/ada/Documents/Trash/Scooch.app", "/Users/ada/Documents/.Trash/Scooch.app", "/Users/bo/.Trash/Scooch.app", "/Applications/Scooch.app", "/Users/ada/.Trash", "/Volumes/Spare/Trash/Scooch.app", "/Volumes/Spare/.Trashes", "/Applications/.Trashes/Scooch.app"] {
            assert!(!trashed(Path::new(path), &home), "{path}");
        }
    }

    #[test]
    fn trashed_and_still_trashed_with_nothing_back_turns_off() {
        let decision = decide(&original(), Seen::Renamed, &After { now: Some(&trash()), back: false }, &home());
        assert_eq!(decision, Decision::Off(trash()));
        assert!(said(&decision, &original()).starts_with("program moved to the Trash at /Users/ada/.Trash/Scooch.app"));
    }

    #[test]
    fn a_program_back_at_the_old_place_before_the_wait_ends_changes_nothing() {
        let replaced = decide(&original(), Seen::Renamed, &After { now: Some(&trash()), back: true }, &home());
        let put_back = decide(&original(), Seen::Renamed, &After { now: Some(&original()), back: true }, &home());
        for decision in [replaced, put_back] {
            assert!(matches!(decision, Decision::Nothing(_)), "{decision:?}");
        }
    }

    #[test]
    fn a_rename_elsewhere_points_the_login_item_at_the_new_place() {
        let moved = PathBuf::from("/Users/ada/Apps/Scooch.app");
        let decision = decide(&original(), Seen::Renamed, &After { now: Some(&moved), back: false }, &home());
        assert_eq!(decision, Decision::Repoint(moved.clone()));
        assert_eq!(moved_program(Path::new("/Applications/Scooch.app/Contents/MacOS/scooch"), &original(), &moved), PathBuf::from("/Users/ada/Apps/Scooch.app/Contents/MacOS/scooch"));
        assert_eq!(moved_program(Path::new("/usr/local/bin/scooch"), Path::new("/usr/local/bin/scooch"), Path::new("/opt/scooch")), PathBuf::from("/opt/scooch"));
        assert!(said(&decision, &original()).contains("from /Applications/Scooch.app to /Users/ada/Apps/Scooch.app"));
    }

    #[test]
    fn a_delete_a_revoke_and_an_edit_in_place_change_nothing() {
        for seen in [Seen::Deleted, Seen::Revoked, Seen::Written] {
            let decision = decide(&original(), seen, &After { now: Some(&trash()), back: false }, &home());
            assert!(matches!(decision, Decision::Nothing(_)), "{seen:?} {decision:?}");
        }
    }

    #[test]
    fn no_watch_with_a_custom_home_or_from_a_disk_image_or_a_translocated_path() {
        let program = Path::new("/Applications/Scooch.app/Contents/MacOS/scooch");
        assert_eq!(watched(program, false, false), Some(original()));
        assert_eq!(watched(Path::new("/Applications/Scooch.app/Contents/Helpers/Glide.app/Contents/MacOS/glide"), false, false), Some(original()));
        assert_eq!(watched(Path::new("/Users/ada/.local/bin/scooch"), false, false), Some(PathBuf::from("/Users/ada/.local/bin/scooch")));
        assert_eq!(watched(program, true, false), None);
        assert_eq!(watched(Path::new("/Volumes/Scooch/Scooch.app/Contents/MacOS/scooch"), false, true), None);
        assert_eq!(watched(Path::new("/private/var/folders/x1/T/AppTranslocation/0A1B/d/Scooch.app/Contents/MacOS/scooch"), false, false), None);
    }
}
