use super::parse::gio::{self, Info, BATCH};
use super::tool;
use anyhow::{bail, Result};
use std::ffi::OsString;

pub fn metadata(locations: &[OsString]) -> Result<Vec<Info>> {
    let mut found = Vec::new();
    for chunk in locations.chunks(BATCH) {
        let mut arguments: Vec<OsString> = vec!["info".into(), "-a".into(), "metadata::*".into()];
        arguments.extend(chunk.iter().cloned());
        let output = tool::run_os("gio", &arguments)?;
        found.extend(gio::parse_info(&String::from_utf8_lossy(&output.stdout)));
    }
    Ok(found)
}

pub fn set(location: &OsString, key: &str, value: Option<&str>) -> Result<()> {
    let mut arguments: Vec<OsString> = vec!["set".into()];
    match value {
        Some(value) => arguments.extend([location.clone(), key.into(), value.into()]),
        None => arguments.extend(["-t".into(), "unset".into(), location.clone(), key.into()]),
    }
    let output = tool::run_os("gio", &arguments)?;
    if !output.status.success() {
        bail!("gio could not set {key}, so check that the gvfs metadata service runs.");
    }
    Ok(())
}
