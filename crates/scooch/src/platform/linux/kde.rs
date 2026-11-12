use super::parse::kde::{self, Containment, Layout, Panel};
use super::parse::{gio, keyfile, wakes};
use super::settings::{self, Plasma};
use super::{bus, tool, FileWatch, Shell, Watches};
use crate::platform::Screen;
use anyhow::{anyhow, Result};
use scooch_core::Rect;
use std::path::{Path, PathBuf};

pub const SHELL_NAME: &str = "org.kde.plasmashell";

pub struct Kde {
    pub desktop: PathBuf,
    pub config: PathBuf,
}

pub fn config_folder() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
}

pub fn groups(name: &str) -> Vec<keyfile::Group> {
    std::fs::read_to_string(config_folder().join(name)).map(|text| keyfile::parse(&text)).unwrap_or_default()
}

pub fn activity() -> Option<String> {
    bus::call::<_, String>("org.kde.ActivityManager", "/ActivityManager/Activities", "org.kde.ActivityManager.Activities", "CurrentActivity", &()).ok().filter(|id| !id.is_empty())
}

pub fn containment(screen: i64) -> Result<Option<Containment>> {
    let all = kde::containments(&groups(kde::APPLETS));
    let current = activity();
    Ok(kde::for_screen(&all, screen, current.as_deref()).cloned())
}

pub fn evaluate(script: &str) -> Result<String> {
    let connection = bus::session()?;
    let reply = connection
        .call_method(Some(SHELL_NAME), "/PlasmaShell", Some("org.kde.PlasmaShell"), "evaluateScript", &(script,))
        .map_err(|_| anyhow!("Plasma did not take the change, so check that plasmashell runs."))?;
    Ok(reply.body().deserialize::<String>().unwrap_or_default())
}

pub fn plasma6() -> bool {
    if let Ok(version) = std::env::var("KDE_SESSION_VERSION") {
        return version.trim() != "5";
    }
    tool::run("plasmashell", &["--version"]).map(|output| !String::from_utf8_lossy(&output.stdout).contains(" 5.")).unwrap_or(true)
}

pub fn panels() -> Vec<Panel> {
    evaluate(kde::PANELS_SCRIPT).map(|text| kde::panels(&text)).unwrap_or_default()
}

pub fn numbers(screens: &[Screen], primary: Option<usize>) -> Vec<i64> {
    let mut order: Vec<usize> = (0..screens.len()).collect();
    order.sort_by(|a, b| {
        let first = |index: &usize| Some(*index) != primary;
        first(a).cmp(&first(b)).then(screens[*a].frame.x.total_cmp(&screens[*b].frame.x)).then(screens[*a].frame.y.total_cmp(&screens[*b].frame.y))
    });
    let mut numbers = vec![0; screens.len()];
    for (number, index) in order.into_iter().enumerate() {
        numbers[index] = number as i64;
    }
    numbers
}

fn primary_index(screens: &[Screen]) -> Option<usize> {
    let session = super::parse::desktop::session(std::env::var("WAYLAND_DISPLAY").ok().as_deref());
    let found = super::screens::read(session).ok()?;
    let id = found.iter().find(|found| found.primary)?.screen.id.clone();
    screens.iter().position(|screen| screen.id == id)
}

pub fn number_of(screen: &Screen) -> i64 {
    let session = super::parse::desktop::session(std::env::var("WAYLAND_DISPLAY").ok().as_deref());
    let Ok(found) = super::screens::read(session) else { return 0 };
    let all: Vec<Screen> = found.iter().map(|found| found.screen.clone()).collect();
    let primary = found.iter().position(|found| found.primary);
    let numbers = numbers(&all, primary);
    all.iter().position(|candidate| candidate.id == screen.id).map_or(0, |index| numbers[index])
}

pub fn count(folder: &Path, hidden: bool) -> usize {
    std::fs::read_dir(folder).map(|entries| entries.flatten().filter(|entry| hidden || !entry.file_name().to_string_lossy().starts_with('.')).count()).unwrap_or(0)
}

fn folder_of(containment: &Containment, desktop: &Path) -> Option<PathBuf> {
    let url = containment.general.get("url").map(String::as_str).unwrap_or("desktop:/");
    if url == "desktop:/" || url == "desktop:" {
        return Some(desktop.to_path_buf());
    }
    gio::location(url)
}

pub fn image_path(value: &str, screen: &Screen) -> Option<PathBuf> {
    let path = gio::location(value.trim())?;
    if path.is_dir() {
        let images = path.join("contents").join("images");
        let files: Vec<PathBuf> = std::fs::read_dir(&images).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
        return kde::best_image(&files, (screen.frame.w * screen.scale) as f64, (screen.frame.h * screen.scale) as f64);
    }
    Some(path)
}

impl Kde {
    pub fn icons_on(&self, screen: &Screen, number: i64, panels: &[Panel], plasma6: bool) -> Result<Vec<Rect>> {
        let Some(containment) = containment(number)? else { return Ok(Vec::new()) };
        if !containment.folder() {
            return Ok(Vec::new());
        }
        let Some(folder) = folder_of(&containment, &self.desktop) else { return Ok(Vec::new()) };
        let hidden = containment.general.get("showHiddenFiles").is_some_and(|value| value == "true");
        let total = count(&folder, hidden);
        let area = kde::available(&screen.frame, number, panels);
        let layout = Layout::from(&containment, plasma6);
        let per_stripe = layout.per_stripe(area.w as f64, area.h as f64).max(1);
        let manual = containment.setting("sortMode") == Some(-1);
        let saved = containment.general.get("positions").filter(|_| manual).and_then(|raw| kde::positions(raw, &kde::resolution(&screen.frame)));
        let present = |url: &str| kde::item_path(url, &folder).is_some_and(|path| path.exists());
        let cells = kde::occupied(saved.as_ref(), &present, total, per_stripe, plasma6);
        Ok(cells.into_iter().map(|index| kde::INSETS.apply(layout.rect(index, &area))).collect())
    }
}

impl Shell for Kde {
    fn icons(&self, screens: &[Screen]) -> Result<Vec<Rect>> {
        let plasma6 = plasma6();
        let panels = panels();
        let numbers = numbers(screens, primary_index(screens));
        let mut found = Vec::new();
        for (screen, number) in screens.iter().zip(numbers) {
            found.extend(self.icons_on(screen, number, &panels, plasma6)?);
        }
        Ok(found)
    }

    fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>> {
        let Some(containment) = containment(number_of(screen))? else { return Ok(None) };
        if containment.wallpaper_plugin.as_deref().is_some_and(|plugin| plugin != kde::IMAGE_PLUGIN) {
            return Ok(None);
        }
        Ok(containment.image.as_deref().and_then(|value| image_path(value, screen)))
    }

    fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()> {
        let key = settings::plasma_key(number_of(screen));
        let wanted = vec![(key.clone(), gio::path_uri(path))];
        settings::write(&Plasma, &wanted, &key, |old| old.and_then(|value| image_path(value, screen)).is_some_and(|old| crate::state::same(&old, path)))?;
        Ok(())
    }

    fn watches(&self) -> Watches {
        Watches { files: vec![FileWatch { folder: self.config.clone(), matches: wakes::kde_file, wallpaper: true }], signals: Vec::new() }
    }

    fn per_screen(&self) -> bool {
        true
    }

    fn tools(&self) -> &'static [&'static str] {
        &["plasmashell"]
    }
}
