use super::parse::caja::{self, Layout, LINKS};
use super::parse::desktop::Session;
use super::parse::{gio, gvariant, keyfile, wakes};
use super::settings::{self, gsetting, Gsettings};
use super::{gnome, gvfs, screens, FileWatch, Shell, Signal, Watches};
use crate::platform::Screen;
use anyhow::Result;
use scooch_core::Rect;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const BACKGROUND: &str = "org.mate.background";
pub const DESKTOP: &str = "org.mate.caja.desktop";

pub struct Mate {
    pub desktop: PathBuf,
    pub home: PathBuf,
    pub config: PathBuf,
    pub session: Session,
}

fn flag(schema: &str, key: &str, default: bool) -> bool {
    gsetting(schema, key).and_then(|text| gvariant::boolean(&text)).unwrap_or(default)
}

fn link_default(name: &str) -> bool {
    name != "network"
}

impl Mate {
    pub fn metadata(&self) -> Vec<keyfile::Group> {
        std::fs::read_to_string(self.config.join("caja").join(caja::METADATA_FILE)).map(|text| keyfile::parse(&text)).unwrap_or_default()
    }

    pub fn layout(&self, groups: &[keyfile::Group], screens: &[Screen]) -> Layout {
        let default = gsetting("org.mate.caja.icon-view", "default-zoom-level").map_or(3, |text| caja::zoom_index(&text));
        let lines = gsetting(DESKTOP, "text-ellipsis-limit").and_then(|text| text.trim().parse::<f64>().ok()).filter(|lines| *lines > 0.0).unwrap_or(3.0);
        let font = gsetting(DESKTOP, "font").filter(|font| !font.trim().trim_matches('\'').is_empty()).or_else(|| gsetting("org.mate.interface", "font-name"));
        let size = screens.iter().fold((0.0f64, 0.0f64), |(w, h), screen| (w.max((screen.frame.x + screen.frame.w) as f64), h.max((screen.frame.y + screen.frame.h) as f64)));
        let factor = self.x11_scale().max(1.0) as u32;
        let work: Vec<u32> = if self.session == Session::X11 { screens::cardinals("_NET_WORKAREA").unwrap_or_default().into_iter().map(|value| value / factor).collect() } else { Vec::new() };
        let [top, _, left, _] = caja::margins(&work, size);
        Layout { zoom: caja::zoom(groups, default), left, top, lines, line: caja::line_height(font.as_deref()) }
    }

    pub fn items(&self, groups: &[keyfile::Group]) -> Result<Vec<((f64, f64), f64)>> {
        let mut items = Vec::new();
        for (name, key) in LINKS {
            if flag(DESKTOP, key, link_default(name)) {
                items.extend(caja::link(groups, name));
            }
        }
        let show_hidden = flag("org.mate.caja.preferences", "show-hidden-files", false);
        let files: Vec<OsString> = gnome::entries(&self.desktop, show_hidden).into_iter().map(PathBuf::into_os_string).collect();
        let found = gvfs::metadata(&files)?;
        let by_path: BTreeMap<OsString, &gio::Info> = found.iter().filter_map(|info| Some((gio::uri_path(&info.uri)?.into_os_string(), info))).collect();
        for file in &files {
            let Some(info) = by_path.get(file) else { continue };
            let Some(at) = info.attributes.get(&format!("metadata::{}", caja::POSITION_KEY)).and_then(|value| caja::position(value)) else { continue };
            let scale = caja::scale(info.attributes.get(&format!("metadata::{}", caja::SCALE_KEY)).map(String::as_str));
            items.push((at, scale));
        }
        Ok(items)
    }
}

impl Shell for Mate {
    fn icons(&self, screens: &[Screen]) -> Result<Vec<Rect>> {
        if !flag(BACKGROUND, "show-desktop-icons", true) {
            return Ok(Vec::new());
        }
        let groups = self.metadata();
        let layout = self.layout(&groups, screens);
        Ok(self.items(&groups)?.into_iter().map(|(at, scale)| caja::INSETS.apply(layout.rect(at, scale))).collect())
    }

    fn wallpaper(&self, _screen: &Screen) -> Result<Option<PathBuf>> {
        Ok(gsetting(BACKGROUND, "picture-filename").and_then(|text| gvariant::string(&text)).and_then(|text| gio::location(&text)))
    }

    fn set_wallpaper(&self, _screen: &Screen, path: &Path) -> Result<()> {
        let wanted = vec![(format!("{BACKGROUND} picture-filename"), gvariant::quote(&path.to_string_lossy())), (format!("{BACKGROUND} picture-options"), "'zoom'".to_string())];
        let pictures = vec![format!("{BACKGROUND} picture-filename")];
        settings::write(&Gsettings, "mate", &wanted, &pictures, path, |text| gvariant::string(text).and_then(|value| gio::location(&value)))?;
        Ok(())
    }

    fn watches(&self) -> Watches {
        Watches {
            files: vec![FileWatch { folder: self.config.join("caja"), matches: wakes::desktop_metadata, wallpaper: false }],
            signals: vec![Signal::Metadata(wakes::desktop_in_home(&self.desktop, &self.home)), Signal::Dconf(&wakes::MATE_KEYS)],
            pointer: false,
        }
    }

    fn per_screen(&self) -> bool {
        false
    }

    fn x11_scale(&self) -> f32 {
        if self.session == Session::X11 {
            super::tool::gtk_scale("org.mate.interface", "window-scaling-factor")
        } else {
            1.0
        }
    }

    fn tools(&self) -> &'static [&'static str] {
        &["gio", "gsettings", "caja"]
    }
}
