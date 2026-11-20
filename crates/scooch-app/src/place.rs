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
    if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some() {
        return copied_out(&inside);
    }
    Ok(inside)
}

fn copied_out(inside: &Path) -> Result<PathBuf> {
    let home = dirs::home_dir().context("The app could not find your home folder.")?;
    let folder = home.join(".local").join("bin");
    let target = folder.join(DAEMON);
    let fresh = std::fs::read(inside).context("The app could not read the scooch program inside it.")?;
    if std::fs::read(&target).ok().as_deref() == Some(fresh.as_slice()) {
        return Ok(target);
    }
    std::fs::create_dir_all(&folder)?;
    let staged = folder.join(".scooch.new");
    std::fs::write(&staged, &fresh)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&staged, &target)?;
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
