use super::parse::kde;
use super::parse::record::{self, Outcome, Settings};
use super::parse::xfce::CHANNEL;
use super::{kde as plasma, tool};
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub struct Gsettings;

fn split(key: &str) -> (&str, &str) {
    key.split_once(' ').unwrap_or((key, ""))
}

impl Settings for Gsettings {
    fn get(&self, key: &str) -> Result<Option<String>> {
        let (schema, name) = split(key);
        let output = tool::run("gsettings", &["get", schema, name])?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_string()))
    }

    fn set(&self, key: &str, value: Option<&str>) -> Result<()> {
        let (schema, name) = split(key);
        let output = match value {
            Some(value) => tool::run("gsettings", &["set", schema, name, value])?,
            None => tool::run("gsettings", &["reset", schema, name])?,
        };
        if !output.status.success() {
            bail!("gsettings could not change {schema} {name}, so check that the schema is installed.");
        }
        Ok(())
    }
}

pub fn gsetting(schema: &str, name: &str) -> Option<String> {
    Gsettings.get(&format!("{schema} {name}")).ok().flatten()
}

pub struct Xfconf;

fn xfconf_type(property: &str) -> &'static str {
    if property.ends_with("/image-style") || property.ends_with("/icon-size") || property.ends_with("/style") || property.ends_with("number") {
        "int"
    } else if property.ends_with("-mode") || property.contains("/show-") || property.ends_with("/primary") {
        "bool"
    } else {
        "string"
    }
}

impl Settings for Xfconf {
    fn get(&self, key: &str) -> Result<Option<String>> {
        let output = tool::run("xfconf-query", &["-c", CHANNEL, "-p", key])?;
        if !output.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(Some(text.strip_suffix('\n').unwrap_or(&text).to_string()))
    }

    fn set(&self, key: &str, value: Option<&str>) -> Result<()> {
        let output = match value {
            Some(value) => tool::run("xfconf-query", &["-c", CHANNEL, "-p", key, "-n", "-t", xfconf_type(key), "-s", value])?,
            None => tool::run("xfconf-query", &["-c", CHANNEL, "-p", key, "-r"])?,
        };
        if !output.status.success() {
            bail!("xfconf-query could not change {key}, so check that xfconfd runs.");
        }
        Ok(())
    }
}

pub fn xfconf(property: &str) -> Option<String> {
    Xfconf.get(property).ok().flatten()
}

pub fn xfconf_list() -> Vec<String> {
    tool::run("xfconf-query", &["-c", CHANNEL, "-l"]).map(|output| String::from_utf8_lossy(&output.stdout).lines().map(str::to_string).collect()).unwrap_or_default()
}

pub struct Plasma;

pub fn plasma_key(screen: i64) -> String {
    format!("screen {screen} Image")
}

fn plasma_screen(key: &str) -> Option<i64> {
    key.strip_prefix("screen ")?.strip_suffix(" Image")?.parse().ok()
}

impl Settings for Plasma {
    fn get(&self, key: &str) -> Result<Option<String>> {
        let screen = plasma_screen(key).unwrap_or(0);
        Ok(plasma::containment(screen)?.and_then(|containment| containment.image))
    }

    fn set(&self, key: &str, value: Option<&str>) -> Result<()> {
        let screen = plasma_screen(key).unwrap_or(0);
        plasma::evaluate(&kde::wallpaper_script(screen, value.unwrap_or("")))?;
        Ok(())
    }
}

pub fn record_file() -> PathBuf {
    record::file(&crate::state::folder())
}

pub fn write(settings: &dyn Settings, wanted: &[(String, String)], in_effect: &str, original: impl Fn(Option<&str>) -> bool) -> Result<Outcome> {
    record::write(settings, &record_file(), wanted, in_effect, original)
}

pub fn same_path(text: Option<&str>, path: &Path, read: impl Fn(&str) -> Option<PathBuf>) -> bool {
    text.and_then(read).is_some_and(|old| crate::state::same(&old, path))
}
