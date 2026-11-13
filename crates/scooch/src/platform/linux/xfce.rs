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
use std::sync::OnceLock;

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

pub fn workarea(session: Session, current: bool) -> Option<Rect> {
    if session != Session::X11 {
        return None;
    }
    let values = screens::cardinals("_NET_WORKAREA").ok()?;
    let desktop = if current { screens::cardinals("_NET_CURRENT_DESKTOP").ok().and_then(|values| values.first().copied()).unwrap_or(0) as usize } else { 0 };
    xfce::workarea(&values, desktop).or_else(|| xfce::workarea(&values, 0))
}

pub fn version() -> Option<(u32, u32)> {
    static VERSION: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *VERSION.get_or_init(|| super::tool::run("xfdesktop", &["--version"]).ok().and_then(|output| xfce::version(&String::from_utf8_lossy(&output.stdout))))
}

pub fn scale() -> i64 {
    super::tool::run("xfconf-query", &["-c", "xsettings", "-p", "/Gdk/WindowScalingFactor"])
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8_lossy(&output.stdout).trim().parse::<i64>().ok())
        .filter(|factor| *factor > 0)
        .unwrap_or(1)
}

fn shrink(rect: Rect, factor: f32) -> Rect {
    Rect { x: (rect.x / factor).trunc(), y: (rect.y / factor).trunc(), w: (rect.w / factor).trunc(), h: (rect.h / factor).trunc() }
}

pub fn workspace(session: Session) -> i64 {
    if flag("/backdrop/single-workspace-mode", xfce::single_workspace_default(version())) {
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
        let factor = if self.session == Session::X11 { scale() } else { 1 };
        let work = workarea(self.session, true).map(|work| shrink(work, factor as f32));
        let mut found = Vec::new();
        if let Some(configs) = self.configs() {
            let named: Vec<(String, Rect)> = screens.iter().map(|screen| (screen.id.clone(), screen.frame)).collect();
            let assigned = xfce::assign(&configs, &named);
            let grids: Vec<Option<xfce::Grid>> = screens.iter().map(|screen| xfce::grid(&work.and_then(|work| intersect(&work, &screen.frame)).unwrap_or(screen.frame), icon, &STYLE)).collect();
            for (id, (screen, (row, col))) in xfce::placements(&configs, &assigned) {
                if self.shown(&id) {
                    found.extend(grids[screen].and_then(|grid| grid.rect(row, col)).map(|rect| xfce::INSETS_4_20.apply(rect)));
                }
            }
            return Ok(found);
        }
        let Some(positions) = self.rc() else { return Ok(found) };
        let primary_only = flag("/desktop-icons/primary", false);
        let single = workarea(self.session, false).map(|work| xfce::inset_area(&work, factor));
        let area = match (primary_only, single) {
            (true, _) => screens.first().map(|screen| work.and_then(|work| intersect(&work, &screen.frame)).unwrap_or(screen.frame)),
            (false, Some(single)) => Some(single),
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
        let pictures = vec![property];
        settings::write(&Xfconf, "xfce", &wanted, &pictures, path, |text| Some(PathBuf::from(text)).filter(|path| path.is_absolute()))?;
        Ok(())
    }

    fn watches(&self) -> Watches {
        Watches { files: vec![FileWatch { folder: self.folder(), matches: wakes::xfce_file, wallpaper: false }], signals: vec![Signal::Xfconf], pointer: false }
    }

    fn per_screen(&self) -> bool {
        true
    }

    fn x11_scale(&self) -> f32 {
        scale() as f32
    }

    fn tools(&self) -> &'static [&'static str] {
        &["xfconf-query", "xfdesktop"]
    }
}
