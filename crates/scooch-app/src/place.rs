use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DAEMON: &str = if cfg!(windows) { "scooch.exe" } else { "scooch" };

pub fn own_folder() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("The app could not find its own program file.")?;
    Ok(exe.parent().map(Path::to_path_buf).unwrap_or_default())
}

pub fn bundled() -> Result<PathBuf> {
    Ok(own_folder()?.join(DAEMON))
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
fn copied_out(inside: &Path) -> Result<PathBuf> {
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

pub fn move_to_applications() -> Result<()> {
    let Some(bundle) = bundle() else { bail!("The app is not inside an app bundle.") };
    let name = bundle.file_name().map(|name| name.to_os_string()).unwrap_or_else(|| "Scooch.app".into());
    let target = Path::new("/Applications").join(name);
    let staged = Path::new("/Applications").join(".Scooch.app.new");
    let _ = std::fs::remove_dir_all(&staged);
    let copied = Command::new("/usr/bin/ditto").arg(&bundle).arg(&staged).status()?;
    if !copied.success() {
        bail!("The copy to Applications did not finish.");
    }
    if target.exists() {
        std::fs::remove_dir_all(&target)?;
    }
    std::fs::rename(&staged, &target)?;
    Command::new("/usr/bin/open").arg("-n").arg(&target).spawn()?;
    Ok(())
}

pub fn open_normally() -> Result<()> {
    let exe = std::env::current_exe()?;
    Command::new("explorer.exe").arg(exe).spawn()?;
    Ok(())
}
