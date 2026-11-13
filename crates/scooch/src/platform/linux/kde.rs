use super::parse::desktop::Session;
use super::parse::kde::{self, Containment, Layout, State};
use super::parse::{gio, keyfile, wakes};
use super::settings::{self, Plasma};
use super::{screens, tool, FileWatch, Shell, Watches};
use crate::platform::Screen;
use anyhow::{anyhow, Result};
use scooch_core::Rect;
use std::path::{Path, PathBuf};

pub const SHELL_NAME: &str = "org.kde.plasmashell";

pub struct Kde {
    pub desktop: PathBuf,
    pub config: PathBuf,
    pub session: Session,
}

pub fn config_folder() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
}

pub fn groups(name: &str) -> Vec<keyfile::Group> {
    std::fs::read_to_string(config_folder().join(name)).map(|text| keyfile::parse(&text)).unwrap_or_default()
}

pub fn evaluate(script: &str) -> Result<String> {
    let connection = super::bus::session()?;
    let reply = connection
        .call_method(Some(SHELL_NAME), "/PlasmaShell", Some("org.kde.PlasmaShell"), "evaluateScript", &(script,))
        .map_err(|_| anyhow!("Plasma did not answer, so check that plasmashell runs."))?;
    Ok(reply.body().deserialize::<String>().unwrap_or_default())
}

pub fn state() -> Result<State> {
    kde::state(&evaluate(&kde::state_script())?).ok_or_else(|| anyhow!("Plasma sent a desktop list Scooch cannot read, so check that plasmashell runs."))
}

pub fn plasma6() -> bool {
    if let Ok(version) = std::env::var("KDE_SESSION_VERSION") {
        return version.trim() != "5";
    }
    tool::run("plasmashell", &["--version"]).map(|output| !String::from_utf8_lossy(&output.stdout).contains(" 5.")).unwrap_or(true)
}

pub fn count(folder: &Path, hidden: bool) -> usize {
    std::fs::read_dir(folder).map(|entries| entries.flatten().filter(|entry| hidden || !entry.file_name().to_string_lossy().starts_with('.')).count()).unwrap_or(0)
}

fn data_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = dirs::data_dir().into_iter().collect();
    let system = std::env::var("XDG_DATA_DIRS").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    folders.extend(std::env::split_paths(&system));
    folders
}

pub fn default_package() -> Option<PathBuf> {
    let package = kde::look_and_feel_package(&groups("kdeglobals"));
    let folders = data_folders();
    let defaults = folders.iter().map(|folder| folder.join("plasma/look-and-feel").join(&package).join("contents/defaults")).find(|path| path.is_file());
    let name = defaults.and_then(|path| std::fs::read_to_string(path).ok()).and_then(|text| kde::look_and_feel_wallpaper(&keyfile::parse(&text)));
    let locate = |name: &str| folders.iter().map(|folder| folder.join("wallpapers").join(name)).find(|path| path.is_dir());
    name.as_deref().and_then(locate).or_else(|| locate("Next"))
}

pub fn image_path(value: &str, screen: &Screen) -> Option<PathBuf> {
    let value = value.trim();
    let path = if value.is_empty() { default_package()? } else { gio::location(value)? };
    if path.is_dir() {
        let images = path.join("contents").join("images");
        let files: Vec<PathBuf> = std::fs::read_dir(&images).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
        return kde::best_image(&files, (screen.frame.w * screen.scale) as f64, (screen.frame.h * screen.scale) as f64);
    }
    Some(path)
}

fn close(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() < 1.0 && (a.y - b.y).abs() < 1.0 && (a.w - b.w).abs() < 1.0 && (a.h - b.h).abs() < 1.0
}

pub fn number_for(state: &State, screen: &Screen) -> Option<i64> {
    state.screens.iter().find(|(_, rect)| close(rect, &screen.frame)).map(|(id, _)| *id)
}

pub fn containment<'a>(state: &'a State, screen: &Screen) -> Option<&'a Containment> {
    let number = number_for(state, screen)?;
    state.desktops.iter().find(|desktop| desktop.screen == number)
}

fn folder_of(containment: &Containment, desktop: &Path) -> Option<PathBuf> {
    let url = containment.general.get("url").map(String::as_str).filter(|url| !url.is_empty()).unwrap_or("desktop:/");
    if url == "desktop:/" || url == "desktop:" {
        return Some(desktop.to_path_buf());
    }
    gio::location(url)
}

impl Kde {
    pub fn icons_on(&self, state: &State, screen: &Screen, plasma6: bool) -> Vec<Rect> {
        let (Some(number), Some(containment)) = (number_for(state, screen), containment(state, screen)) else { return Vec::new() };
        if !containment.folder() {
            return Vec::new();
        }
        let Some(folder) = folder_of(containment, &self.desktop) else { return Vec::new() };
        let hidden = containment.general.get("showHiddenFiles").is_some_and(|value| value == "true");
        let total = count(&folder, hidden);
        let area = kde::available(&screen.frame, number, &state.panels);
        let layout = Layout::from(containment, plasma6);
        let per_stripe = layout.per_stripe(area.w as f64, area.h as f64).max(1);
        let manual = containment.setting("sortMode") == Some(-1);
        let saved = containment.general.get("positions").filter(|_| manual).and_then(|raw| kde::positions(raw, &kde::resolution(&screen.frame)));
        let present = |url: &str| kde::item_path(url, &folder).is_some_and(|path| path.exists());
        kde::occupied(saved.as_ref(), &present, total, per_stripe, plasma6).into_iter().map(|index| kde::INSETS.apply(layout.rect(index, &area))).collect()
    }
}

impl Shell for Kde {
    fn icons(&self, screens: &[Screen]) -> Result<Vec<Rect>> {
        let state = state()?;
        let plasma6 = plasma6();
        Ok(screens.iter().flat_map(|screen| self.icons_on(&state, screen, plasma6)).collect())
    }

    fn adjust(&self, _session: Session, found: &mut [screens::Found]) {
        let Ok(state) = state() else { return };
        let plasma: Vec<Rect> = state.screens.iter().map(|(_, rect)| *rect).collect();
        let frames: Vec<Rect> = found.iter().map(|found| found.screen.frame).collect();
        for (index, matched) in kde::match_screens(&plasma, &frames).into_iter().enumerate() {
            let Some(target) = matched else { continue };
            let rect = plasma[index];
            let screen = &mut found[target].screen;
            if rect.w > 0.0 {
                screen.scale *= screen.frame.w / rect.w;
            }
            screen.frame = rect;
        }
    }

    fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>> {
        let state = state()?;
        let Some(containment) = containment(&state, screen) else { return Ok(None) };
        if containment.wallpaper_plugin.as_deref().is_some_and(|plugin| plugin != kde::IMAGE_PLUGIN) {
            return Ok(None);
        }
        Ok(image_path(containment.image.as_deref().unwrap_or(""), screen))
    }

    fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()> {
        let state = state()?;
        let id = containment(&state, screen).and_then(|containment| containment.id.parse::<i64>().ok()).ok_or_else(|| anyhow!("Plasma has no desktop on this screen, so Scooch cannot set its picture."))?;
        let key = kde::record_key(id);
        let wanted = vec![(key.clone(), gio::path_uri(path))];
        let pictures = vec![key];
        settings::write(&Plasma, "kde", &wanted, &pictures, path, |value| image_path(value, screen))?;
        Ok(())
    }

    fn watches(&self) -> Watches {
        Watches { files: vec![FileWatch { folder: self.config.clone(), matches: wakes::kde_file, wallpaper: true }], signals: Vec::new(), pointer: self.session == Session::X11 }
    }

    fn per_screen(&self) -> bool {
        true
    }

    fn tools(&self) -> &'static [&'static str] {
        &["plasmashell"]
    }
}
