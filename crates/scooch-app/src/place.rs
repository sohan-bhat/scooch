use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DAEMON: &str = if cfg!(windows) { "scooch.exe" } else { "scooch" };

pub fn own_folder() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("The app could not find its own program file.")?;
    Ok(exe.parent().map(Path::to_path_buf).unwrap_or_default())
}

pub fn bundled() -> Result<PathBuf> {
    let folder = own_folder()?;
    if cfg!(target_os = "macos") {
        if let Some(contents) = folder.parent() {
            return Ok(contents.join("Helpers").join("Scooch.app").join("Contents").join("MacOS").join(DAEMON));
        }
    }
    Ok(folder.join(DAEMON))
}

pub fn program() -> Result<PathBuf> {
    let inside = bundled()?;
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() {
        return copy_place(&inside);
    }
    Ok(inside)
}

pub fn ready() -> Result<PathBuf> {
    let inside = bundled()?;
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_some() {
        return copied_out(&inside);
    }
    Ok(inside)
}

#[cfg(target_os = "linux")]
fn copy_place(inside: &Path) -> Result<PathBuf> {
    let home = dirs::home_dir().context("The app could not find your home folder.")?;
    let usual = home.join(".local").join("bin").join(DAEMON);
    let free = match std::fs::symlink_metadata(&usual) {
        Err(_) => true,
        Ok(meta) if meta.file_type().is_symlink() => true,
        Ok(meta) if meta.file_type().is_file() => std::fs::read(&usual).ok() == std::fs::read(inside).ok() || crate::takeover::answers_as_scooch(&usual),
        Ok(_) => false,
    };
    Ok(if free { usual } else { scooch::facade::folder().join("bin").join(DAEMON) })
}

#[cfg(target_os = "linux")]
static COPYING: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(target_os = "linux")]
fn copied_out(inside: &Path) -> Result<PathBuf> {
    let _copying = COPYING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let target = copy_place(inside)?;
    let fresh = std::fs::read(inside).context("The app could not read the scooch program inside it.")?;
    let regular = std::fs::symlink_metadata(&target).is_ok_and(|meta| meta.file_type().is_file());
    if regular && std::fs::read(&target).ok().as_deref() == Some(fresh.as_slice()) {
        return Ok(target);
    }
    let folder = target.parent().context("The copy of scooch has no folder.")?;
    std::fs::create_dir_all(folder)?;
    let staged = folder.join(format!(".scooch.new.{}", std::process::id()));
    std::fs::write(&staged, &fresh)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    if let Err(error) = std::fs::rename(&staged, &target) {
        let _ = std::fs::remove_file(&staged);
        return Err(error.into());
    }
    Ok(target)
}

pub fn bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|path| path.extension().is_some_and(|kind| kind == "app")).map(Path::to_path_buf)
}

pub fn misplaced() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    bundle().is_some_and(|bundle| {
        let text = bundle.to_string_lossy();
        text.contains("/AppTranslocation/") || text.starts_with("/Volumes/")
    })
}

pub fn writable(folder: &Path) -> bool {
    if std::fs::create_dir_all(folder).is_err() {
        return false;
    }
    let probe = folder.join(format!(".scooch.write.{}", std::process::id()));
    let made = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    made
}

pub fn move_to_applications() -> Result<PathBuf> {
    let Some(bundle) = bundle() else { bail!("the app is not inside an app bundle") };
    let name = bundle.file_name().map(|name| name.to_os_string()).unwrap_or_else(|| "Scooch.app".into());
    let mut candidates = vec![PathBuf::from("/Applications")];
    candidates.extend(dirs::home_dir().map(|home| home.join("Applications")));
    for folder in candidates.iter().filter(|folder| writable(folder)) {
        match place_in(&bundle, folder, Path::new(&name)) {
            Ok(target) => return Ok(target),
            Err(error) => eprintln!("scooch-app move into {} failed: {error:#}", folder.display()),
        }
    }
    bail!("no Applications folder took the copy")
}

fn place_in(bundle: &Path, folder: &Path, name: &Path) -> Result<PathBuf> {
    let target = folder.join(name);
    let staged = folder.join(format!(".Scooch.app.new.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staged);
    let placed = (|| {
        let copied = Command::new("/usr/bin/ditto").arg(bundle).arg(&staged).status()?;
        if !copied.success() {
            bail!("ditto ended with {copied}");
        }
        let _ = Command::new("/usr/bin/xattr").arg("-dr").arg("com.apple.quarantine").arg(&staged).status();
        let aside = folder.join(format!(".Scooch.app.old.{}", std::process::id()));
        let replacing = std::fs::symlink_metadata(&target).is_ok();
        if replacing {
            if !same_owner(&target, &staged) {
                bail!("{} belongs to another account", target.display());
            }
            let _ = std::fs::remove_dir_all(&aside);
            std::fs::rename(&target, &aside)?;
        }
        if let Err(error) = std::fs::rename(&staged, &target) {
            if replacing {
                let _ = std::fs::rename(&aside, &target);
            }
            return Err(error.into());
        }
        if replacing {
            if let Err(error) = std::fs::remove_dir_all(&aside) {
                eprintln!("scooch-app the old app set aside at {} stays, {error}", aside.display());
            }
        }
        Ok(target.clone())
    })();
    if placed.is_err() {
        let _ = std::fs::remove_dir_all(&staged);
    }
    placed
}

#[cfg(unix)]
fn same_owner(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    matches!((std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)), (Ok(a), Ok(b)) if a.uid() == b.uid())
}

#[cfg(not(unix))]
fn same_owner(_a: &Path, _b: &Path) -> bool {
    true
}

pub fn open_app(target: &Path) -> Result<()> {
    Command::new("/usr/bin/open").arg("-n").arg(target).spawn()?;
    Ok(())
}

pub fn open_normally() -> Result<()> {
    let exe = std::env::current_exe()?;
    Command::new("explorer.exe").arg(exe).spawn()?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_locked_folder_falls_back_to_the_next() {
        let root = std::env::temp_dir().join(format!("scooch-apps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let locked = root.join("Applications");
        let home = root.join("home").join("Applications");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let chosen = [locked.clone(), home.clone()].into_iter().find(|folder| writable(folder));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(chosen, Some(home));
    }

    #[test]
    fn a_failed_place_leaves_no_staged_copy() {
        let root = std::env::temp_dir().join(format!("scooch-place-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("Applications");
        std::fs::create_dir_all(&folder).unwrap();
        let placed = place_in(&root.join("missing.app"), &folder, Path::new("Scooch.app"));
        let left: Vec<_> = std::fs::read_dir(&folder).unwrap().flatten().map(|entry| entry.file_name()).collect();
        let _ = std::fs::remove_dir_all(&root);
        assert!(placed.is_err());
        assert!(left.is_empty(), "{left:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_replace_that_cannot_delete_the_old_app_still_leaves_a_whole_new_one() {
        let root = std::env::temp_dir().join(format!("scooch-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bundle = root.join("New.app");
        std::fs::create_dir_all(bundle.join("Contents")).unwrap();
        std::fs::write(bundle.join("Contents/new"), b"new").unwrap();
        let folder = root.join("Applications");
        let locked = folder.join("Scooch.app/Contents/Locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("old"), b"old").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let placed = place_in(&bundle, &folder, Path::new("Scooch.app"));
        let whole = folder.join("Scooch.app/Contents/new").exists() && !folder.join("Scooch.app/Contents/Locked").exists();
        let staged = std::fs::read_dir(&folder).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".Scooch.app.new"));
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let _ = std::fs::set_permissions(entry.path().join("Contents/Locked"), std::fs::Permissions::from_mode(0o755));
        }
        let _ = std::fs::remove_dir_all(&root);
        assert!(placed.is_ok(), "{placed:?}");
        assert!(whole, "the new app is not whole in place");
        assert!(!staged, "a staged copy stayed");
    }
}
