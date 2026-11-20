use std::path::{Path, PathBuf};

pub const PLACES: &str = "SCOOCH_LINK_PLACES";

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

#[cfg(any(target_os = "macos", test))]
pub fn translocated(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "AppTranslocation")
}

#[cfg(any(target_os = "macos", test))]
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

#[cfg(any(target_os = "macos", test))]
pub fn trashed(path: &Path, home: &Path) -> bool {
    let path = plain(path);
    let user = home.join(".Trash");
    if path.starts_with(&user) && path != user {
        return true;
    }
    use std::path::Component;
    let parts: Vec<Component> = path.components().collect();
    let named = |part: &Component, name: &str| part.as_os_str() == name;
    match parts.as_slice() {
        [Component::RootDir, trash, _, _, ..] if named(trash, ".Trashes") => true,
        [Component::RootDir, volumes, _, trash, _, _, ..] => named(volumes, "Volumes") && named(trash, ".Trashes"),
        _ => false,
    }
}

#[cfg(any(target_os = "linux", test))]
pub fn trash_roots(data_home: &Path, top: &Path, uid: u32) -> Vec<PathBuf> {
    vec![data_home.join("Trash").join("files"), top.join(".Trash").join(uid.to_string()).join("files"), top.join(format!(".Trash-{uid}")).join("files")]
}

#[cfg(any(target_os = "linux", test))]
pub fn inside_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root) && path != root.as_path())
}

#[cfg(any(target_os = "linux", test))]
pub fn seen_from(unmount: bool, delete_self: bool, move_self: bool, close_write: bool, unlinked: bool) -> Option<Seen> {
    if unmount {
        Some(Seen::Revoked)
    } else if delete_self || unlinked {
        Some(Seen::Deleted)
    } else if move_self {
        Some(Seen::Renamed)
    } else if close_write {
        Some(Seen::Written)
    } else {
        None
    }
}

pub fn decide(original: &Path, seen: Seen, after: &After, is_trash: &dyn Fn(&Path) -> bool) -> Decision {
    match seen {
        Seen::Deleted => Decision::Nothing("was deleted"),
        Seen::Revoked => Decision::Nothing("lost its disk"),
        Seen::Written => Decision::Nothing("changed in place"),
        Seen::Renamed if after.back => Decision::Nothing("moved but a program is back at its old place"),
        Seen::Renamed => match after.now {
            None => Decision::Nothing("moved and could not be found after the wait"),
            Some(now) if is_trash(now) => Decision::Off(now.to_path_buf()),
            Some(now) if plain(now) == plain(original) => Decision::Nothing("moved back to its old place"),
            Some(now) => Decision::Repoint(now.to_path_buf()),
        },
    }
}

#[cfg(any(target_os = "macos", test))]
pub fn moved_program(program: &Path, from: &Path, to: &Path) -> PathBuf {
    match program.strip_prefix(from) {
        Ok(inside) if !inside.as_os_str().is_empty() => to.join(inside),
        _ => to.to_path_buf(),
    }
}

pub fn said(decision: &Decision, at: &Path) -> String {
    match decision {
        Decision::Off(now) => format!("program moved to the Trash at {}, so Scooch turns off and puts the wallpaper back", now.display()),
        Decision::Repoint(now) => format!("program moved from {} to {}, so Scooch follows it there", at.display(), now.display()),
        Decision::Nothing(why) => format!("program at {} {why}, so nothing changes", at.display()),
    }
}

pub fn places(home: &Path, custom: Option<std::ffi::OsString>) -> Vec<PathBuf> {
    match custom.filter(|list| !list.is_empty()) {
        Some(list) => std::env::split_paths(&list).collect(),
        None => vec![home.join(".local").join("bin"), home.join("bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/opt/homebrew/bin")],
    }
}

pub fn links_into(places: &[PathBuf], target: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for place in places {
        let Ok(entries) = std::fs::read_dir(place) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
                continue;
            }
            let Ok(points) = std::fs::read_link(&path) else { continue };
            let points = if points.is_absolute() { points } else { place.join(points) };
            if points.starts_with(target) {
                found.push(path);
            }
        }
    }
    found
}

pub fn remove_links(links: &[PathBuf]) -> Vec<String> {
    links
        .iter()
        .map(|link| match std::fs::remove_file(link) {
            Ok(()) => format!("program link {} removed", link.display()),
            Err(error) => format!("program link {} could not be removed, {error}", link.display()),
        })
        .collect()
}

#[cfg(any(target_os = "macos", test))]
pub fn follow_links(links: &[PathBuf], from: &Path, to: &Path) -> Vec<String> {
    links
        .iter()
        .map(|link| {
            let followed = std::fs::read_link(link).map(|points| moved_program(&points, from, to)).and_then(|points| {
                let name = link.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                let fresh = link.with_file_name(format!(".{name}.following"));
                let _ = std::fs::remove_file(&fresh);
                std::os::unix::fs::symlink(&points, &fresh)?;
                std::fs::rename(&fresh, link)?;
                Ok(points)
            });
            match followed {
                Ok(points) => format!("program link {} now points at {}", link.display(), points.display()),
                Err(error) => format!("program link {} could not follow, {error}", link.display()),
            }
        })
        .collect()
}

#[cfg(any(target_os = "linux", test))]
pub fn follow_origin(store: &crate::state::Store, now: &Path) -> anyhow::Result<()> {
    let _lock = store.command_lock(crate::state::COMMAND_PATIENCE)?;
    let bytes = store.read().ok_or_else(|| anyhow::anyhow!("the saved state could not be read"))?;
    let mut state: crate::state::State = serde_json::from_slice(&bytes)?;
    state.origin = Some(now.to_path_buf());
    store.save(&state)
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
    let places = places(&home, std::env::var_os(PLACES));
    let act = move |decision: &Decision, at: &Path, program: &Path| {
        if let Decision::Repoint(now) = decision {
            if let Err(error) = crate::platform::macos::launchd::repoint(&moved_program(program, at, &plain(now))) {
                eprintln!("{} program moved from {} to {} but the login item could not follow it, {error:#}", stamp(), at.display(), now.display());
                return;
            }
            for line in follow_links(&links_into(&places, at), at, &plain(now)) {
                eprintln!("{} {line}", stamp());
            }
        }
        eprintln!("{} {}", stamp(), said(decision, at));
        if let Decision::Off(_) = decision {
            for line in remove_links(&links_into(&places, at)) {
                eprintln!("{} {line}", stamp());
            }
            trashed.store(true, std::sync::atomic::Ordering::SeqCst);
            signal.wake();
        }
    };
    let _ = std::thread::Builder::new().name("scooch-program".into()).spawn(move || mac::run(program, target, home, act));
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{decide, moved_program, trashed, After, Decision, Seen};
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
                if crate::daemon::trace() {
                    eprintln!("{} program event flags {flags:#x} on {}", crate::daemon::stamp(), target.display());
                }
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
                let decision = decide(&target, seen, &After { now: now.as_deref(), back: target.exists() }, &|path| trashed(path, &home));
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

#[cfg(target_os = "linux")]
pub mod linux {
    use super::{decide, follow_origin, inside_any, links_into, places, remove_links, said, seen_from, trash_roots, After, Decision, Seen, PLACES};
    use crate::daemon::{stamp, trace, Signal};
    use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask, Watches};
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::Duration;

    const SETTLE: Duration = Duration::from_secs(10);

    struct Armed {
        origin: PathBuf,
        wd: WatchDescriptor,
        fd: Option<std::fs::File>,
        dev: u64,
        ino: u64,
        roots: Vec<PathBuf>,
    }

    pub struct Watcher {
        watches: Mutex<Watches>,
        armed: Mutex<Option<Armed>>,
        wanted: Mutex<Option<PathBuf>>,
        home: PathBuf,
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn read_only(path: &Path) -> bool {
        let Ok(name) = CString::new(path.as_os_str().as_bytes()) else { return false };
        let mut found: libc::statvfs = unsafe { std::mem::zeroed() };
        unsafe { libc::statvfs(name.as_ptr(), &mut found) == 0 && found.f_flag & libc::ST_RDONLY != 0 }
    }

    fn top_of(path: &Path, dev: u64) -> PathBuf {
        let mut top = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        for folder in path.ancestors().skip(1) {
            match std::fs::metadata(folder) {
                Ok(meta) if meta.dev() == dev => top = folder.to_path_buf(),
                _ => break,
            }
        }
        top
    }

    fn locate(dev: u64, ino: u64, folders: &[PathBuf]) -> Option<PathBuf> {
        folders.iter().filter_map(|folder| std::fs::read_dir(folder).ok()).flat_map(|entries| entries.flatten()).map(|entry| entry.path()).find(|path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.dev() == dev && meta.ino() == ino))
    }

    impl Watcher {
        pub fn start(trashed: Arc<AtomicBool>, signal: Arc<Signal>) -> Option<Arc<Watcher>> {
            if std::env::var_os("SCOOCH_HOME").is_some_and(|value| !value.is_empty()) {
                return None;
            }
            let home = dirs::home_dir()?;
            let inotify = Inotify::init().ok()?;
            let watcher = Arc::new(Watcher { watches: Mutex::new(inotify.watches()), armed: Mutex::new(None), wanted: Mutex::new(None), home: home.clone() });
            let places = places(&home, std::env::var_os(PLACES));
            let program = std::env::current_exe().ok().map(|exe| crate::state::canonical(&exe).unwrap_or(exe));
            let act = move |decision: &Decision, at: &Path| {
                if let Decision::Repoint(now) = decision {
                    if let Err(error) = follow_origin(&crate::state::Store::open(), now) {
                        eprintln!("{} program moved from {} to {} but Scooch could not follow it, {error:#}", stamp(), at.display(), now.display());
                        return;
                    }
                }
                eprintln!("{} {}", stamp(), said(decision, at));
                if let Decision::Off(_) = decision {
                    if let Some((copy, others)) = places.split_first().map(|(first, others)| (first.join("scooch"), others)) {
                        if program.as_deref().is_some_and(|program| crate::state::canonical(&copy).is_ok_and(|real| real == program)) {
                            for line in remove_links(&links_into(others, &copy)) {
                                eprintln!("{} {line}", stamp());
                            }
                            match std::fs::remove_file(&copy) {
                                Ok(()) => eprintln!("{} program copy {} removed", stamp(), copy.display()),
                                Err(error) => eprintln!("{} program copy {} could not be removed, {error}", stamp(), copy.display()),
                            }
                        }
                    }
                    trashed.store(true, Ordering::SeqCst);
                    signal.wake();
                }
            };
            let runner = watcher.clone();
            std::thread::Builder::new().name("scooch-program".into()).spawn(move || runner.run(inotify, act)).ok()?;
            Some(watcher)
        }

        pub fn arm(&self, origin: Option<&Path>) {
            let mut wanted = lock(&self.wanted);
            if wanted.as_deref() == origin {
                return;
            }
            *wanted = origin.map(Path::to_path_buf);
            let mut armed = lock(&self.armed);
            if let Some(old) = armed.take() {
                let _ = lock(&self.watches).remove(old.wd);
            }
            let Some(origin) = origin else { return };
            match self.make(origin) {
                Ok(made) => {
                    if trace() {
                        eprintln!("{} program watched at {}", stamp(), origin.display());
                    }
                    *armed = Some(made);
                }
                Err(why) => {
                    if trace() {
                        eprintln!("{} program not watched at {}, {why}", stamp(), origin.display());
                    }
                }
            }
        }

        fn make(&self, origin: &Path) -> Result<Armed, String> {
            let meta = std::fs::metadata(origin).map_err(|error| error.to_string())?;
            if read_only(origin) {
                return Err("it sits on a read only mount".into());
            }
            let wd = lock(&self.watches).add(origin, WatchMask::MOVE_SELF | WatchMask::DELETE_SELF | WatchMask::ATTRIB | WatchMask::CLOSE_WRITE).map_err(|error| error.to_string())?;
            let same = std::fs::metadata(&self.home).is_ok_and(|home| home.dev() == meta.dev());
            let fd = if same { std::fs::OpenOptions::new().read(true).custom_flags(libc::O_PATH).open(origin).ok() } else { None };
            let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|path| path.is_absolute()).unwrap_or_else(|| self.home.join(".local").join("share"));
            let roots = trash_roots(&data, &top_of(origin, meta.dev()), unsafe { libc::getuid() });
            Ok(Armed { origin: origin.to_path_buf(), wd, fd, dev: meta.dev(), ino: meta.ino(), roots })
        }

        fn current(&self, wd: &WatchDescriptor) -> Option<(PathBuf, bool)> {
            let armed = lock(&self.armed);
            let armed = armed.as_ref().filter(|armed| armed.wd == *wd)?;
            let unlinked = armed.fd.as_ref().is_some_and(|fd| fd.metadata().is_ok_and(|meta| meta.nlink() == 0));
            Some((armed.origin.clone(), unlinked))
        }

        fn where_now(&self, wd: &WatchDescriptor) -> (Option<PathBuf>, Vec<PathBuf>) {
            let armed = lock(&self.armed);
            let Some(armed) = armed.as_ref().filter(|armed| armed.wd == *wd) else { return (None, Vec::new()) };
            let now = match &armed.fd {
                Some(fd) => std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())).ok().filter(|path| !path.to_string_lossy().ends_with(" (deleted)")),
                None => {
                    let mut folders = armed.roots.clone();
                    folders.extend(armed.origin.parent().map(Path::to_path_buf));
                    folders.extend(["", "Applications", "Desktop", "Downloads"].iter().map(|name| self.home.join(name)));
                    locate(armed.dev, armed.ino, &folders)
                }
            };
            (now, armed.roots.clone())
        }

        fn moved(&self, wd: &WatchDescriptor, now: &Path) {
            if let Some(armed) = lock(&self.armed).as_mut().filter(|armed| armed.wd == *wd) {
                armed.origin = now.to_path_buf();
            }
            *lock(&self.wanted) = Some(now.to_path_buf());
        }

        fn ended(&self, wd: &WatchDescriptor) {
            let matched = {
                let mut armed = lock(&self.armed);
                let matched = armed.as_ref().is_some_and(|armed| armed.wd == *wd);
                if matched {
                    *armed = None;
                }
                matched
            };
            if matched {
                *lock(&self.wanted) = None;
            }
        }

        fn run(&self, mut inotify: Inotify, act: impl Fn(&Decision, &Path)) {
            let mut buffer = [0u8; 4096];
            loop {
                let events: Vec<(WatchDescriptor, EventMask)> = match inotify.read_events_blocking(&mut buffer) {
                    Ok(events) => events.map(|event| (event.wd.clone(), event.mask)).collect(),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return,
                };
                for (wd, mask) in events {
                    let Some((origin, unlinked)) = self.current(&wd) else { continue };
                    if trace() {
                        eprintln!("{} program event flags {:#x} on {}", stamp(), mask.bits(), origin.display());
                    }
                    if mask.contains(EventMask::IGNORED) {
                        self.ended(&wd);
                        continue;
                    }
                    let seen = seen_from(mask.contains(EventMask::UNMOUNT), mask.contains(EventMask::DELETE_SELF), mask.contains(EventMask::MOVE_SELF), mask.contains(EventMask::CLOSE_WRITE), mask.contains(EventMask::ATTRIB) && unlinked);
                    let Some(seen) = seen else { continue };
                    if seen == Seen::Renamed {
                        std::thread::sleep(SETTLE);
                    }
                    let (now, roots) = self.where_now(&wd);
                    let decision = decide(&origin, seen, &After { now: now.as_deref(), back: origin.exists() }, &|path| inside_any(path, &roots));
                    act(&decision, &origin);
                    match decision {
                        Decision::Repoint(now) => self.moved(&wd, &now),
                        Decision::Off(_) => {
                            self.ended(&wd);
                            return;
                        }
                        Decision::Nothing(_) if matches!(seen, Seen::Deleted | Seen::Revoked) => self.ended(&wd),
                        Decision::Nothing(_) => {}
                    }
                }
            }
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
        let decision = decide(&original(), Seen::Renamed, &After { now: Some(&trash()), back: false }, &|path: &Path| trashed(path, &home()));
        assert_eq!(decision, Decision::Off(trash()));
        assert!(said(&decision, &original()).starts_with("program moved to the Trash at /Users/ada/.Trash/Scooch.app"));
    }

    #[test]
    fn a_program_back_at_the_old_place_before_the_wait_ends_changes_nothing() {
        let replaced = decide(&original(), Seen::Renamed, &After { now: Some(&trash()), back: true }, &|path: &Path| trashed(path, &home()));
        let put_back = decide(&original(), Seen::Renamed, &After { now: Some(&original()), back: true }, &|path: &Path| trashed(path, &home()));
        for decision in [replaced, put_back] {
            assert!(matches!(decision, Decision::Nothing(_)), "{decision:?}");
        }
    }

    #[test]
    fn a_rename_elsewhere_points_the_login_item_at_the_new_place() {
        let moved = PathBuf::from("/Users/ada/Apps/Scooch.app");
        let decision = decide(&original(), Seen::Renamed, &After { now: Some(&moved), back: false }, &|path: &Path| trashed(path, &home()));
        assert_eq!(decision, Decision::Repoint(moved.clone()));
        assert_eq!(moved_program(Path::new("/Applications/Scooch.app/Contents/MacOS/scooch"), &original(), &moved), PathBuf::from("/Users/ada/Apps/Scooch.app/Contents/MacOS/scooch"));
        assert_eq!(moved_program(Path::new("/usr/local/bin/scooch"), Path::new("/usr/local/bin/scooch"), Path::new("/opt/scooch")), PathBuf::from("/opt/scooch"));
        assert!(said(&decision, &original()).contains("from /Applications/Scooch.app to /Users/ada/Apps/Scooch.app"));
    }

    #[test]
    fn a_delete_a_revoke_and_an_edit_in_place_change_nothing() {
        for seen in [Seen::Deleted, Seen::Revoked, Seen::Written] {
            let decision = decide(&original(), seen, &After { now: Some(&trash()), back: false }, &|path: &Path| trashed(path, &home()));
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

    fn linux_roots() -> Vec<PathBuf> {
        trash_roots(Path::new("/home/ada/.local/share"), Path::new("/media/ada/Spare"), 1000)
    }

    fn appimage() -> PathBuf {
        PathBuf::from("/home/ada/Applications/Scooch.AppImage")
    }

    fn in_trash() -> PathBuf {
        PathBuf::from("/home/ada/.local/share/Trash/files/Scooch.AppImage")
    }

    fn link(from: &Path, to: &Path) {
        std::os::unix::fs::symlink(to, from).unwrap();
    }

    fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
        paths.sort();
        paths
    }

    #[test]
    fn a_path_in_the_home_trash_or_a_volume_trash_counts_and_a_folder_named_trash_does_not() {
        let roots = linux_roots();
        for path in ["/home/ada/.local/share/Trash/files/Scooch.AppImage", "/home/ada/.local/share/Trash/files/Scooch.2.AppImage", "/media/ada/Spare/.Trash-1000/files/Scooch.AppImage", "/media/ada/Spare/.Trash/1000/files/Scooch.AppImage"] {
            assert!(inside_any(Path::new(path), &roots), "{path}");
        }
        for path in ["/home/ada/Trash/Scooch.AppImage", "/home/ada/Documents/Trash/files/Scooch.AppImage", "/home/ada/.local/share/Trash/info/Scooch.AppImage.trashinfo", "/home/ada/.local/share/Trash/files", "/media/ada/Spare/.Trash-1001/files/Scooch.AppImage", "/media/ada/Other/.Trash-1000/files/Scooch.AppImage", "/home/ada/Applications/Scooch.AppImage"] {
            assert!(!inside_any(Path::new(path), &roots), "{path}");
        }
    }

    #[test]
    fn on_linux_trashed_with_nothing_back_turns_off_and_something_back_changes_nothing() {
        let roots = linux_roots();
        let is_trash = |path: &Path| inside_any(path, &roots);
        assert_eq!(decide(&appimage(), Seen::Renamed, &After { now: Some(&in_trash()), back: false }, &is_trash), Decision::Off(in_trash()));
        for now in [in_trash(), appimage()] {
            let decision = decide(&appimage(), Seen::Renamed, &After { now: Some(&now), back: true }, &is_trash);
            assert!(matches!(decision, Decision::Nothing(_)), "{decision:?}");
        }
    }

    #[test]
    fn a_rename_on_the_same_file_system_writes_the_new_origin_and_leaves_the_login_item_alone() {
        let roots = linux_roots();
        let moved = PathBuf::from("/home/ada/Apps/Scooch.AppImage");
        let decision = decide(&appimage(), Seen::Renamed, &After { now: Some(&moved), back: false }, &|path: &Path| inside_any(path, &roots));
        assert_eq!(decision, Decision::Repoint(moved.clone()));
        let folder = crate::state::scratch("origin-follows");
        let store = crate::state::Store::at(&folder);
        let before = crate::state::State { on: true, origin: Some(appimage()), ..crate::state::State::default() };
        store.save(&before).unwrap();
        follow_origin(&store, &moved).unwrap();
        let after = store.load();
        assert_eq!(after.origin, Some(moved));
        assert_eq!(crate::state::State { origin: before.origin.clone(), ..after }, before);
        let others: Vec<String> = std::fs::read_dir(&folder).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name != "state.json" && name != "command.lock").collect();
        assert!(others.is_empty(), "{others:?}");
    }

    #[test]
    fn a_delete_a_replace_in_place_and_a_move_across_file_systems_change_nothing() {
        let roots = linux_roots();
        let is_trash = |path: &Path| inside_any(path, &roots);
        assert_eq!(seen_from(false, true, false, false, false), Some(Seen::Deleted));
        assert_eq!(seen_from(false, false, false, false, true), Some(Seen::Deleted));
        assert_eq!(seen_from(false, false, false, true, false), Some(Seen::Written));
        assert_eq!(seen_from(true, false, true, false, false), Some(Seen::Revoked));
        assert_eq!(seen_from(false, false, true, false, false), Some(Seen::Renamed));
        assert_eq!(seen_from(false, false, false, false, false), None);
        for seen in [Seen::Deleted, Seen::Written, Seen::Revoked] {
            let decision = decide(&appimage(), seen, &After { now: Some(&in_trash()), back: false }, &is_trash);
            assert!(matches!(decision, Decision::Nothing(_)), "{seen:?} {decision:?}");
        }
    }

    #[test]
    fn on_the_trash_only_links_into_the_bundle_or_at_the_copy_go() {
        let folder = crate::state::scratch("links-trash");
        let (local, bin, other) = (folder.join("local"), folder.join("bin"), folder.join("usr"));
        for place in [&local, &bin, &other] {
            std::fs::create_dir_all(place).unwrap();
        }
        let bundle = original();
        link(&local.join("scooch"), &bundle.join("Contents/MacOS/scooch"));
        link(&other.join("helper"), &bundle.join("Contents/MacOS/helper"));
        std::fs::write(bin.join("scooch"), b"a program of its own").unwrap();
        link(&other.join("scooch"), Path::new("/opt/elsewhere/scooch"));
        link(&other.join("near"), Path::new("/Applications/Scooch.app.old/Contents/MacOS/scooch"));
        let places = vec![local.clone(), bin.clone(), other.clone()];
        let found = links_into(&places, &bundle);
        assert_eq!(sorted(found.clone()), sorted(vec![local.join("scooch"), other.join("helper")]));
        let said = remove_links(&found);
        assert!(said.iter().all(|line| line.ends_with(" removed")), "{said:?}");
        assert!(std::fs::symlink_metadata(local.join("scooch")).is_err() && std::fs::symlink_metadata(other.join("helper")).is_err());
        assert_eq!(std::fs::read(bin.join("scooch")).unwrap(), b"a program of its own");
        assert_eq!(std::fs::read_link(other.join("scooch")).unwrap(), PathBuf::from("/opt/elsewhere/scooch"));
        assert!(std::fs::symlink_metadata(other.join("near")).is_ok());
        let copy = local.join("scooch");
        std::fs::write(&copy, b"the copy").unwrap();
        link(&bin.join("scooch-link"), &copy);
        link(&other.join("next"), &bin.join("unrelated"));
        assert_eq!(links_into(&[bin.clone(), other.clone()], &copy), vec![bin.join("scooch-link")]);
    }

    #[test]
    fn on_a_move_the_links_follow() {
        let folder = crate::state::scratch("links-move");
        let local = folder.join("local");
        std::fs::create_dir_all(&local).unwrap();
        let moved = PathBuf::from("/Users/ada/Apps/Scooch.app");
        link(&local.join("scooch"), &original().join("Contents/MacOS/scooch"));
        link(&local.join("foreign"), Path::new("/opt/elsewhere/scooch"));
        std::fs::write(local.join("plain"), b"not a link").unwrap();
        let said = follow_links(&links_into(std::slice::from_ref(&local), &original()), &original(), &moved);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("now points at"), "{said:?}");
        assert_eq!(std::fs::read_link(local.join("scooch")).unwrap(), moved.join("Contents/MacOS/scooch"));
        assert_eq!(std::fs::read_link(local.join("foreign")).unwrap(), PathBuf::from("/opt/elsewhere/scooch"));
        assert_eq!(std::fs::read(local.join("plain")).unwrap(), b"not a link");
        let left: Vec<String> = std::fs::read_dir(&local).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.starts_with('.')).collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn the_four_places_are_the_usual_ones_unless_given() {
        let home = Path::new("/home/ada");
        assert_eq!(places(home, None), vec![home.join(".local/bin"), home.join("bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/opt/homebrew/bin")]);
        assert_eq!(places(home, Some("/tmp/a:/tmp/b".into())), vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]);
        assert_eq!(places(home, Some("".into())), places(home, None));
    }
}
