use super::parse::desktop::Session;
use super::parse::geometry::intersect;
use super::parse::xfce::{self, Config, STYLE};
use super::parse::{gvariant, keyfile, wakes, yaml};
use super::settings::{self, xfconf, xfconf_list, Xfconf};
use super::{screens, FileWatch, Shell, Signal, Watches};
use crate::platform::Screen;
use anyhow::Result;
use scooch_core::Rect;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Xfce {
    pub desktop: PathBuf,
    pub home: PathBuf,
    pub config: PathBuf,
    pub session: Session,
}

fn flag(property: &str, default: bool) -> bool {
    xfconf(property).and_then(|text| gvariant::boolean(&text)).unwrap_or(default)
}

fn number(property: &str) -> Option<i64> {
    xfconf(property)?.trim().parse().ok()
}

pub fn workarea(session: Session) -> Option<Rect> {
    if session != Session::X11 {
        return None;
    }
    let values = screens::cardinals("_NET_WORKAREA").ok()?;
    let desktop = screens::cardinals("_NET_CURRENT_DESKTOP").ok().and_then(|values| values.first().copied()).unwrap_or(0) as usize;
    xfce::workarea(&values, desktop).or_else(|| xfce::workarea(&values, 0))
}

pub fn workspace(session: Session) -> i64 {
    if flag("/backdrop/single-workspace-mode", false) {
        return number("/backdrop/single-workspace-number").unwrap_or(0);
    }
    if session != Session::X11 {
        return 0;
    }
    screens::cardinals("_NET_CURRENT_DESKTOP").ok().and_then(|values| values.first().copied()).unwrap_or(0) as i64
}

impl Xfce {
    pub fn folder(&self) -> PathBuf {
        self.config.join("xfce4").join("desktop")
    }

    fn shown(&self, id: &str) -> bool {
        if id == "trash:///" {
            return flag("/desktop-icons/file-icons/show-trash", true);
        }
        if !id.starts_with('/') {
            return false;
        }
        let path = Path::new(id);
        if path == self.home {
            return flag("/desktop-icons/file-icons/show-home", true);
        }
        if path == Path::new("/") {
            return flag("/desktop-icons/file-icons/show-filesystem", true);
        }
        path.exists()
    }

    pub fn configs(&self) -> Option<Vec<Config>> {
        let text = std::fs::read_to_string(self.folder().join(xfce::YAML_FILE)).ok()?;
        Some(xfce::configs(&yaml::parse(&text)))
    }

    pub fn rc(&self) -> Option<BTreeMap<String, (i64, i64)>> {
        let text = std::fs::read_to_string(self.folder().join(xfce::LATEST_RC)).ok()?;
        Some(xfce::rc_positions(&keyfile::parse(&text)))
    }

    fn property(&self, screen: &Screen) -> String {
        let prefix = xfce::backdrop_prefix(&screen.id, workspace(self.session));
        format!("{prefix}/last-image")
    }
}

impl Shell for Xfce {
    fn icons(&self, screens: &[Screen]) -> Result<Vec<Rect>> {
        if number("/desktop-icons/style").unwrap_or(2) != 2 {
            return Ok(Vec::new());
        }
        let icon = number("/desktop-icons/icon-size").map_or(xfce::DEFAULT_ICON_SIZE, |size| size as f64);
        let work = workarea(self.session);
        let mut found = Vec::new();
        if let Some(configs) = self.configs() {
            for screen in screens {
                let Some(config) = xfce::config_for(&configs, &screen.id, &screen.frame) else { continue };
                let area = work.and_then(|work| intersect(&work, &screen.frame)).unwrap_or(screen.frame);
                let Some(grid) = xfce::grid(&area, icon, &STYLE) else { continue };
                for (id, (row, col)) in &config.icons {
                    if self.shown(id) {
                        found.extend(grid.rect(*row, *col).map(|rect| xfce::INSETS.apply(rect)));
                    }
                }
            }
            return Ok(found);
        }
        let Some(positions) = self.rc() else { return Ok(found) };
        let primary_only = flag("/desktop-icons/primary", false);
        let area = match (primary_only, work) {
            (true, work) => screens.first().map(|screen| work.and_then(|work| intersect(&work, &screen.frame)).unwrap_or(screen.frame)),
            (false, Some(work)) => Some(work),
            (false, None) => screens.first().map(|screen| screen.frame),
        };
        let Some(grid) = area.and_then(|area| xfce::grid(&area, icon, &STYLE)) else { return Ok(found) };
        for (row, col) in positions.values() {
            found.extend(grid.rect(*row, *col).map(|rect| xfce::INSETS.apply(rect)));
        }
        Ok(found)
    }

    fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>> {
        if let Some(value) = xfconf(&self.property(screen)) {
            return Ok(Some(PathBuf::from(value)).filter(|path| path.is_absolute()));
        }
        let any = xfconf_list().into_iter().find(|property| xfce::workspace_of(property, &screen.id).is_some());
        Ok(any.and_then(|property| xfconf(&property)).map(PathBuf::from).filter(|path| path.is_absolute()))
    }

    fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()> {
        let property = self.property(screen);
        let style = format!("{}/image-style", property.trim_end_matches("/last-image"));
        let wanted = vec![(property.clone(), path.to_string_lossy().into_owned()), (style, "5".to_string())];
        settings::write(&Xfconf, &wanted, &property, |old| old.is_some_and(|old| crate::state::same(Path::new(old), path)))?;
        Ok(())
    }

    fn watches(&self) -> Watches {
        Watches { files: vec![FileWatch { folder: self.folder(), matches: wakes::xfce_file, wallpaper: false }], signals: vec![Signal::Xfconf] }
    }

    fn per_screen(&self) -> bool {
        true
    }

    fn tools(&self) -> &'static [&'static str] {
        &["xfconf-query", "xfdesktop"]
    }
}
