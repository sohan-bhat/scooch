use super::parse::entry;
use anyhow::{Context, Result};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn path() -> Result<PathBuf> {
    let config = dirs::config_dir().or_else(|| dirs::home_dir().map(|home| home.join(".config"))).context("Scooch could not find your config folder.")?;
    Ok(config.join("autostart").join(entry::NAME))
}

pub fn custom_home() -> bool {
    std::env::var_os("SCOOCH_HOME").is_some_and(|value| !value.is_empty())
}

pub fn installed() -> bool {
    path().is_ok_and(|path| path.is_file())
}

pub fn install(exe: &Path) -> Result<()> {
    if !custom_home() {
        let file = path()?;
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).with_context(|| format!("Scooch could not make {}, so check the folder.", folder.display()))?;
        }
        crate::state::write_atomic(&file, entry::text(exe).as_bytes())?;
    }
    start(exe)
}

pub fn start(exe: &Path) -> Result<()> {
    let mut command = Command::new(exe);
    command.arg("watch").current_dir("/").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn().with_context(|| format!("Scooch could not start {}, so run scooch again.", exe.display()))?;
    Ok(())
}

pub fn remove() -> Result<()> {
    if custom_home() {
        return Ok(());
    }
    let file = path()?;
    match std::fs::remove_file(&file) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error).with_context(|| format!("Scooch could not remove {}, so delete it by hand.", file.display())),
        _ => Ok(()),
    }
}
