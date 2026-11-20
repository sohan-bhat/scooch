use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const KEYS: [&str; 8] = ["LD_LIBRARY_PATH", "PATH", "PYTHONPATH", "PYTHONHOME", "PERLLIB", "QT_PLUGIN_PATH", "GIO_MODULE_DIR", "GSETTINGS_SCHEMA_DIR"];

static READY: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

fn inside() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")?;
    std::env::var_os("APPDIR").map(PathBuf::from)
}

pub fn scrubbed(value: &std::ffi::OsStr, appdir: &std::path::Path) -> Option<std::ffi::OsString> {
    let kept: Vec<PathBuf> = std::env::split_paths(value).filter(|path| !path.as_os_str().is_empty() && !path.starts_with(appdir)).collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

pub fn settle() {
    let (done, ready) = &READY;
    let mut done = done.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if *done {
        return;
    }
    if let Some(appdir) = inside() {
        for key in KEYS {
            let Some(value) = std::env::var_os(key) else { continue };
            match scrubbed(&value, &appdir) {
                Some(kept) => std::env::set_var(key, kept),
                None => std::env::remove_var(key),
            }
        }
    }
    *done = true;
    ready.notify_all();
}

pub fn wait() {
    if inside().is_none() {
        return;
    }
    let (done, ready) = &READY;
    let done = done.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let waited = ready.wait_timeout_while(done, Duration::from_secs(15), |done| !*done);
    let late = waited.is_ok_and(|(_, timeout)| timeout.timed_out());
    if late {
        settle();
    }
}

#[cfg(target_os = "linux")]
pub fn seal() {
    if inside().is_none() {
        return;
    }
    let Ok(entries) = std::fs::read_dir("/proc/self/fd") else { return };
    for fd in entries.flatten().filter_map(|entry| entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok())).filter(|fd| *fd > 2) {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
            unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn only_the_appimage_parts_are_dropped() {
        let appdir = Path::new("/tmp/.mount_ScoochAb12");
        assert_eq!(scrubbed(OsStr::new("/tmp/.mount_ScoochAb12/usr/lib:/usr/lib/x86_64-linux-gnu"), appdir).as_deref(), Some(OsStr::new("/usr/lib/x86_64-linux-gnu")));
        assert_eq!(scrubbed(OsStr::new("/tmp/.mount_ScoochAb12/usr/share/glib-2.0/schemas"), appdir), None);
        assert_eq!(scrubbed(OsStr::new("/usr/bin:/bin"), appdir).as_deref(), Some(OsStr::new("/usr/bin:/bin")));
    }
}
