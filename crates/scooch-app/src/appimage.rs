#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

type Env = BTreeMap<OsString, OsString>;
type Changes = Vec<(OsString, Option<OsString>)>;

const RUNTIME: [&str; 4] = ["APPIMAGE", "APPDIR", "OWD", "ARGV0"];

fn inside() -> Option<(PathBuf, PathBuf)> {
    let appimage = std::env::var_os("APPIMAGE")?;
    let appdir = std::env::var_os("APPDIR")?;
    Some((PathBuf::from(appimage), PathBuf::from(appdir)))
}

fn within(part: &Path, appdir: &Path) -> bool {
    part.starts_with(appdir)
}

pub fn scrubbed(value: &OsStr, appdir: &Path) -> Option<OsString> {
    let parts: Vec<PathBuf> = std::env::split_paths(value).collect();
    if !parts.iter().any(|part| within(part, appdir)) {
        return Some(value.to_os_string());
    }
    let kept: Vec<PathBuf> = parts.into_iter().filter(|part| !part.as_os_str().is_empty() && !within(part, appdir)).collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

pub fn cleaned(env: Env, appdir: &Path) -> Env {
    env.into_iter()
        .filter(|(name, _)| !RUNTIME.iter().any(|runtime| name == runtime))
        .filter_map(|(name, value)| scrubbed(&value, appdir).map(|value| (name, value)))
        .collect()
}

pub fn diff(now: &Env, wanted: &Env) -> Changes {
    let mut changes: Changes = now
        .iter()
        .filter_map(|(name, value)| match wanted.get(name) {
            None => Some((name.clone(), None)),
            Some(other) if other != value => Some((name.clone(), Some(other.clone()))),
            Some(_) => None,
        })
        .collect();
    changes.extend(wanted.iter().filter(|(name, _)| !now.contains_key(*name)).map(|(name, value)| (name.clone(), Some(value.clone()))));
    changes
}

#[cfg(unix)]
pub fn parse(raw: &[u8]) -> Env {
    use std::os::unix::ffi::OsStrExt;
    raw.split(|byte| *byte == 0)
        .filter_map(|item| {
            let at = item.iter().position(|byte| *byte == b'=')?;
            (at > 0).then(|| (OsStr::from_bytes(&item[..at]).to_os_string(), OsStr::from_bytes(&item[at + 1..]).to_os_string()))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn launched(appimage: &Path, appdir: &Path) -> Option<Env> {
    use std::os::unix::ffi::OsStrExt;
    let me = std::process::id();
    let parent = std::os::unix::process::parent_id();
    let real = appdir.canonicalize().ok();
    let names_mount = |cmdline: &[u8]| {
        cmdline.split(|byte| *byte == 0).any(|arg| {
            let arg = Path::new(OsStr::from_bytes(arg));
            arg == appdir || real.as_deref().is_some_and(|real| arg == real)
        })
    };
    let mut fallback = None;
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else { continue };
        if pid == me || std::fs::read_link(entry.path().join("exe")).ok().as_deref() != Some(appimage) {
            continue;
        }
        let Ok(raw) = std::fs::read(entry.path().join("environ")) else { continue };
        let env = parse(&raw);
        if env.get(OsStr::new("APPDIR")).is_some_and(|value| Path::new(value) == appdir) {
            continue;
        }
        let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        if pid == parent || names_mount(&cmdline) {
            return Some(env);
        }
        if fallback.is_none() {
            fallback = Some(env);
        }
    }
    fallback
}

#[cfg(not(target_os = "linux"))]
fn launched(_: &Path, _: &Path) -> Option<Env> {
    None
}

pub fn changes() -> Changes {
    static CHANGES: OnceLock<Changes> = OnceLock::new();
    CHANGES
        .get_or_init(|| {
            let Some((appimage, appdir)) = inside() else { return Vec::new() };
            let now: Env = std::env::vars_os().collect();
            let start = launched(&appimage, &appdir);
            eprintln!("scooch-app children get {}", if start.is_some() { "the environment the AppImage started with" } else { "this environment without the AppImage's parts" });
            diff(&now, &cleaned(start.unwrap_or_else(|| now.clone()), &appdir))
        })
        .clone()
}

pub fn start_folder(now: Option<&Path>, owd: Option<&Path>, home: Option<&Path>, appdir: &Path) -> Option<PathBuf> {
    if now.is_some_and(|now| !within(now, appdir)) {
        return None;
    }
    [owd, home, Some(Path::new("/"))].into_iter().flatten().find(|folder| folder.is_absolute() && folder.is_dir() && !within(folder, appdir)).map(Path::to_path_buf)
}

fn child_folder() -> Option<PathBuf> {
    static FOLDER: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let (_, appdir) = inside()?;
            let owd = std::env::var_os("OWD").map(PathBuf::from);
            start_folder(std::env::current_dir().ok().as_deref(), owd.as_deref(), dirs::home_dir().as_deref(), &appdir)
        })
        .clone()
}

pub fn apply(command: &mut std::process::Command) -> &mut std::process::Command {
    if let Some(folder) = child_folder() {
        command.current_dir(folder);
    }
    for (name, value) in changes() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
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
    use std::path::Path;

    fn env(pairs: &[(&str, &str)]) -> Env {
        pairs.iter().map(|(name, value)| (OsString::from(name), OsString::from(value))).collect()
    }

    #[cfg(unix)]
    #[test]
    fn only_the_appimage_parts_are_dropped() {
        let appdir = Path::new("/tmp/.mount_ScoochAb12");
        assert_eq!(scrubbed(OsStr::new("/tmp/.mount_ScoochAb12/usr/lib:/usr/lib/x86_64-linux-gnu"), appdir).as_deref(), Some(OsStr::new("/usr/lib/x86_64-linux-gnu")));
        assert_eq!(scrubbed(OsStr::new("/tmp/.mount_ScoochAb12//usr/share/glib-2.0/schemas"), appdir), None);
        assert_eq!(scrubbed(OsStr::new("/usr/bin::/bin:"), appdir).as_deref(), Some(OsStr::new("/usr/bin::/bin:")));
        assert_eq!(scrubbed(OsStr::new("unix:path=/run/user/1000/bus"), appdir).as_deref(), Some(OsStr::new("unix:path=/run/user/1000/bus")));
    }

    #[cfg(unix)]
    #[test]
    fn the_rule_drops_every_mount_value_and_the_runtime_names() {
        let appdir = Path::new("/tmp/.mount_ScoochAb12");
        let after = env(&[
            ("APPIMAGE", "/home/a/Scooch.AppImage"),
            ("APPDIR", "/tmp/.mount_ScoochAb12"),
            ("OWD", "/home/a"),
            ("ARGV0", "./Scooch.AppImage"),
            ("GTK_PATH", "/tmp/.mount_ScoochAb12//usr/lib/gtk-3.0"),
            ("XDG_DATA_DIRS", "/tmp/.mount_ScoochAb12/usr/share:/usr/share:/usr/local/share"),
            ("PATH", "/tmp/.mount_ScoochAb12/usr/bin:/usr/bin"),
            ("HOME", "/home/a"),
        ]);
        assert_eq!(cleaned(after, appdir), env(&[("HOME", "/home/a"), ("PATH", "/usr/bin"), ("XDG_DATA_DIRS", "/usr/share:/usr/local/share")]));
    }

    #[test]
    fn children_get_the_start_environment_back() {
        let appdir = Path::new("/tmp/.mount_ScoochAb12");
        let start = env(&[("HOME", "/home/a"), ("GDK_BACKEND", "wayland"), ("XDG_DATA_DIRS", "/usr/local/share:/usr/share")]);
        let now = env(&[
            ("HOME", "/home/a"),
            ("APPDIR", "/tmp/.mount_ScoochAb12"),
            ("GDK_BACKEND", "x11"),
            ("GTK_THEME", ""),
            ("XDG_DATA_DIRS", "/tmp/.mount_ScoochAb12/usr/share:/usr/share:/usr/local/share:/usr/share"),
        ]);
        let changes = diff(&now, &cleaned(start, appdir));
        let mut command = std::process::Command::new("/usr/bin/env");
        for (name, value) in &changes {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        let given: Env = command.get_envs().filter_map(|(name, value)| value.map(|value| (name.to_os_string(), value.to_os_string()))).collect();
        assert_eq!(given, env(&[("GDK_BACKEND", "wayland"), ("XDG_DATA_DIRS", "/usr/local/share:/usr/share")]));
        let removed: Vec<_> = command.get_envs().filter(|(_, value)| value.is_none()).map(|(name, _)| name.to_os_string()).collect();
        assert_eq!(removed, vec![OsString::from("APPDIR"), OsString::from("GTK_THEME")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_child_works_in_the_start_folder_and_never_inside_the_mount() {
        let root = std::env::temp_dir().join(format!("scooch-start-{}", std::process::id()));
        let (appdir, owd, home) = (root.join(".mount_Scooch"), root.join("Downloads"), root.join("home"));
        for folder in [&appdir, &owd, &home] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let inside = appdir.join("usr");
        assert_eq!(start_folder(Some(&inside), Some(&owd), Some(&home), &appdir), Some(owd.clone()));
        assert_eq!(start_folder(Some(&inside), Some(&appdir), Some(&home), &appdir), Some(home.clone()));
        assert_eq!(start_folder(None, Some(&root.join("gone")), None, &appdir), Some(PathBuf::from("/")));
        assert_eq!(start_folder(Some(&owd), Some(&home), Some(&home), &appdir), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn an_environ_file_reads_as_names_and_values() {
        assert_eq!(parse(b"HOME=/home/a\0EMPTY=\0=odd\0X=a=b\0"), env(&[("HOME", "/home/a"), ("EMPTY", ""), ("X", "a=b")]));
    }
}
