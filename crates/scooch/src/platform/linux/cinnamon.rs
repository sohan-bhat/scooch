use super::parse::desktop::Session;
use super::parse::geometry::subtract;
use super::parse::nemo::{self, Grid, LINKS};
use super::parse::{gio, gvariant, keyfile, wakes};
use super::settings::{self, gsetting, Gsettings};
use super::{bus, gnome, gvfs, screens, FileWatch, Shell, Signal, Watches};
use crate::platform::Screen;
use anyhow::Result;
use scooch_core::Rect;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const BACKGROUND: &str = "org.cinnamon.desktop.background";
pub const DESKTOP: &str = "org.nemo.desktop";

pub struct Cinnamon {
    pub desktop: PathBuf,
    pub home: PathBuf,
    pub config: PathBuf,
    pub session: Session,
}

pub type Item = (Option<(i64, i64)>, i64);

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub index: i64,
    pub canvas: Rect,
    pub primary: bool,
}

fn flag(schema: &str, key: &str, default: bool) -> bool {
    gsetting(schema, key).and_then(|text| gvariant::boolean(&text)).unwrap_or(default)
}

pub fn work_rects() -> Result<Vec<(i64, Rect)>> {
    let indices: Vec<i32> = bus::call("org.Cinnamon", "/org/Cinnamon", "org.Cinnamon", "GetMonitors", &())?;
    let mut found = Vec::new();
    for index in indices {
        let rect: Vec<i32> = bus::call("org.Cinnamon", "/org/Cinnamon", "org.Cinnamon", "GetMonitorWorkRect", &(index,))?;
        if let [x, y, w, h] = rect[..] {
            found.push((i64::from(index), Rect { x: x as f32, y: y as f32, w: w as f32, h: h as f32 }));
        }
    }
    Ok(found)
}

fn inside(rect: &Rect, frame: &Rect) -> bool {
    let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    cx >= frame.x && cx < frame.x + frame.w && cy >= frame.y && cy < frame.y + frame.h
}

impl Cinnamon {
    pub fn metadata(&self) -> Vec<keyfile::Group> {
        std::fs::read_to_string(self.config.join("nemo").join(nemo::METADATA_FILE)).map(|text| keyfile::parse(&text)).unwrap_or_default()
    }

    pub fn windows(&self) -> Vec<Window> {
        let Ok(found) = screens::read(self.session) else { return Vec::new() };
        let primary = found.iter().find(|found| found.primary).map(|found| found.screen.frame);
        if let Ok(rects) = work_rects() {
            if !rects.is_empty() {
                return rects.into_iter().map(|(index, canvas)| Window { index, canvas, primary: primary.is_some_and(|frame| inside(&canvas, &frame)) }).collect();
            }
        }
        let work = if self.session == Session::X11 { screens::cardinals("_NET_WORKAREA").ok().and_then(|values| super::parse::xfce::workarea(&values, 0)) } else { None };
        found
            .iter()
            .enumerate()
            .map(|(index, found)| {
                let frame = found.screen.frame;
                let margins = match (found.primary, work.and_then(|work| super::parse::geometry::intersect(&work, &frame))) {
                    (true, Some(area)) => [(area.y - frame.y) as f64, (frame.y + frame.h - area.y - area.h) as f64, (area.x - frame.x) as f64, (frame.x + frame.w - area.x - area.w) as f64],
                    (true, None) => [0.0; 4],
                    (false, _) => [nemo::NON_PRIMARY_MARGIN; 4],
                };
                Window { index: index as i64, canvas: subtract(&frame, margins), primary: found.primary }
            })
            .collect()
    }

    pub fn items(&self, groups: &[keyfile::Group]) -> Result<Vec<Item>> {
        let mut items = Vec::new();
        for (name, key) in LINKS {
            if flag(DESKTOP, key, false) {
                items.push(nemo::link(groups, name));
            }
        }
        let show_hidden = flag("org.nemo.preferences", "show-hidden-files", false);
        let files: Vec<OsString> = gnome::entries(&self.desktop, show_hidden).into_iter().map(PathBuf::into_os_string).collect();
        let found = gvfs::metadata(&files)?;
        let by_path: BTreeMap<OsString, &gio::Info> = found.iter().filter_map(|info| Some((gio::uri_path(&info.uri)?.into_os_string(), info))).collect();
        for file in &files {
            let info = by_path.get(file);
            let saved = info.and_then(|info| info.attributes.get(&format!("metadata::{}", nemo::POSITION_KEY))).and_then(|value| nemo::position(value));
            let monitor = info.and_then(|info| info.attributes.get(&format!("metadata::{}", nemo::MONITOR_KEY))).and_then(|value| value.trim().parse().ok()).unwrap_or(-1);
            items.push((saved, monitor));
        }
        Ok(items)
    }
}

impl Shell for Cinnamon {
    fn icons(&self, _screens: &[Screen]) -> Result<Vec<Rect>> {
        let layout = nemo::layout(&gsetting(DESKTOP, "desktop-layout").and_then(|text| gvariant::string(&text)).unwrap_or_default());
        let windows = self.windows();
        let shown: Vec<&Window> = windows.iter().filter(|window| if window.primary { layout.primary } else { layout.others }).collect();
        if shown.is_empty() {
            return Ok(Vec::new());
        }
        let indices: Vec<i64> = shown.iter().map(|window| window.index).collect();
        let primary = windows.iter().find(|window| window.primary).map_or(indices[0], |window| window.index);
        let groups = self.metadata();
        let items = self.items(&groups)?;
        let mut found = Vec::new();
        for window in shown {
            let Some(mut grid) = Grid::new(window.canvas, &nemo::settings(&groups, window.index)) else { continue };
            let here: Vec<Option<(i64, i64)>> = items.iter().filter(|(_, monitor)| nemo::monitor_for(*monitor, &indices, primary, layout) == Some(window.index)).map(|(saved, _)| *saved).collect();
            for (col, row) in grid.place(&here) {
                found.push(nemo::INSETS.apply(grid.rect(col, row)));
            }
        }
        Ok(found)
    }

    fn wallpaper(&self, _screen: &Screen) -> Result<Option<PathBuf>> {
        let picture = gsetting(BACKGROUND, "picture-uri").and_then(|text| gvariant::string(&text)).and_then(|uri| gio::location(&uri));
        Ok(picture.filter(|path| !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))))
    }

    fn set_wallpaper(&self, _screen: &Screen, path: &Path) -> Result<()> {
        let wanted = vec![(format!("{BACKGROUND} picture-uri"), gvariant::quote(&gio::path_uri(path))), (format!("{BACKGROUND} picture-options"), "'zoom'".to_string())];
        let in_effect = format!("{BACKGROUND} picture-uri");
        settings::write(&Gsettings, &wanted, &in_effect, |old| settings::same_path(old, path, |text| gvariant::string(text).and_then(|uri| gio::location(&uri))))?;
        Ok(())
    }

    fn watches(&self) -> Watches {
        Watches {
            files: vec![FileWatch { folder: self.config.join("nemo"), matches: wakes::desktop_metadata, wallpaper: false }],
            signals: vec![Signal::Metadata(wakes::desktop_in_home(&self.desktop, &self.home)), Signal::Dconf(&wakes::CINNAMON_KEYS)],
        }
    }

    fn per_screen(&self) -> bool {
        false
    }

    fn tools(&self) -> &'static [&'static str] {
        &["gio", "gsettings", "nemo-desktop"]
    }
}
