use super::parse::ding::{self, Grid, Item, Mode, Monitor, Prefs};
use super::parse::{gio, gvariant, wakes};
use super::settings::{self, gsetting, Gsettings};
use super::{bus, gvfs, Shell, Signal, Watches};
use crate::platform::Screen;
use zbus::zvariant;
use anyhow::{bail, Result};
use scooch_core::Rect;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "org.gnome.shell.extensions.ding";
pub const BACKGROUND: &str = "org.gnome.desktop.background";
pub const FORK: &str = "Scooch does not read the Gtk4 desktop icons extension yet, so switch to Desktop Icons NG to use it.";

pub struct Gnome {
    pub desktop: PathBuf,
    pub home: PathBuf,
    pub x11: bool,
}

pub fn geometry() -> Result<Vec<Monitor>> {
    let reply: (bool, zvariant::Signature, Vec<zvariant::OwnedValue>) = bus::call(ding::BUS_NAME, ding::ACTIONS_PATH, "org.gtk.Actions", "Describe", &("desktopGeometry",))?;
    let mut found = Vec::new();
    for value in &reply.2 {
        dicts(value, &mut found);
    }
    Ok(found.iter().filter_map(ding::monitor).collect())
}

fn dicts(value: &zvariant::Value, out: &mut Vec<BTreeMap<String, f64>>) {
    match value {
        zvariant::Value::Value(inner) => dicts(inner, out),
        zvariant::Value::Array(array) => array.inner().iter().for_each(|inner| dicts(inner, out)),
        zvariant::Value::Dict(dict) => {
            let mut map = BTreeMap::new();
            for (key, number) in dict.iter() {
                if let (zvariant::Value::Str(key), zvariant::Value::F64(number)) = (key, number) {
                    map.insert(key.to_string(), *number);
                } else if let (zvariant::Value::Str(key), zvariant::Value::Value(inner)) = (key, number) {
                    if let zvariant::Value::F64(number) = inner.as_ref() {
                        map.insert(key.to_string(), *number);
                    }
                }
            }
            if !map.is_empty() {
                out.push(map);
            }
        }
        _ => {}
    }
}

fn flag(name: &str, default: bool) -> bool {
    gsetting(SCHEMA, name).and_then(|text| gvariant::boolean(&text)).unwrap_or(default)
}

fn text(name: &str) -> Option<String> {
    gsetting(SCHEMA, name).and_then(|text| gvariant::string(&text))
}

pub fn prefs() -> Prefs {
    Prefs {
        size: ding::size(&text("icon-size").unwrap_or_else(|| "standard".into())),
        corner: ding::corner(&text("start-corner").unwrap_or_else(|| "top-left".into())),
        volumes_opposite: flag("add-volumes-opposite", true),
    }
}

pub fn mode(x11: bool) -> Mode {
    let features = gsetting("org.gnome.mutter", "experimental-features").map(|text| gvariant::strings(&text)).unwrap_or_default();
    Mode { x11, premultiplied: features.iter().any(|feature| feature == "scale-monitor-framebuffer") }
}

pub fn running() -> Result<bool> {
    if bus::has_owner(ding::BUS_NAME) {
        return Ok(true);
    }
    if bus::has_owner(ding::GTK4_FORK_BUS_NAME) {
        bail!(FORK);
    }
    Ok(false)
}

pub fn hidden(path: &Path, hidden_list: &[String]) -> bool {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let backup = name.ends_with('~') && path.is_file();
    name.starts_with('.') || backup || hidden_list.contains(&name)
}

pub fn entries(desktop: &Path, show_hidden: bool) -> Vec<PathBuf> {
    let listed: Vec<String> = std::fs::read_to_string(desktop.join(".hidden")).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default();
    let mut found: Vec<PathBuf> = std::fs::read_dir(desktop).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
    found.retain(|path| show_hidden || !hidden(path, &listed));
    found.sort();
    found
}

impl Gnome {
    pub fn locations(&self) -> (Vec<OsString>, Vec<Item>) {
        let mut locations = Vec::new();
        let mut items = Vec::new();
        if flag("show-home", true) {
            locations.push(self.home.clone().into_os_string());
            items.push(Item::default());
        }
        if flag("show-trash", true) {
            locations.push(OsString::from("trash:///"));
            items.push(Item::default());
        }
        let show_hidden = gsetting("org.gtk.Settings.FileChooser", "show-hidden").and_then(|text| gvariant::boolean(&text)).unwrap_or(false);
        for path in entries(&self.desktop, show_hidden) {
            locations.push(path.into_os_string());
            items.push(Item::default());
        }
        (locations, items)
    }

    pub fn saved(&self, locations: &[OsString], items: &mut [Item]) -> Result<Vec<gio::Info>> {
        let found = gvfs::metadata(locations)?;
        let by_location: BTreeMap<OsString, &gio::Info> = found.iter().map(|info| (gio::uri_path(&info.uri).map_or_else(|| OsString::from(&info.uri), PathBuf::into_os_string), info)).collect();
        for (item, location) in items.iter_mut().zip(locations) {
            item.saved = by_location.get(location).and_then(|info| info.attributes.get(ding::POSITION_KEY)).and_then(|value| gio::point(value));
        }
        Ok(found)
    }

    fn in_effect(&self) -> &'static str {
        let dark = gsetting("org.gnome.desktop.interface", "color-scheme").and_then(|text| gvariant::string(&text).or(Some(text))).is_some_and(|scheme| scheme.trim_matches('\'') == "prefer-dark");
        if dark {
            "picture-uri-dark"
        } else {
            "picture-uri"
        }
    }
}

fn picture(uri: &str) -> Option<PathBuf> {
    let path = gio::location(uri)?;
    let xml = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("xml"));
    (!xml).then_some(path)
}

impl Shell for Gnome {
    fn icons(&self, screens: &[Screen]) -> Result<Vec<Rect>> {
        if !running()? {
            return Ok(Vec::new());
        }
        let monitors = geometry()?;
        let prefs = prefs();
        let mode = mode(self.x11);
        let mut grids: Vec<Grid> = monitors.iter().filter_map(|monitor| Grid::new(monitor, prefs.size, mode)).collect();
        let (locations, mut items) = self.locations();
        self.saved(&locations, &mut items)?;
        let placed = ding::place(&mut grids, &items, &prefs);
        Ok(ding::rects(&grids, &placed, screens, &ding::INSETS))
    }

    fn primary(&self, screens: &[Screen]) -> Option<usize> {
        let monitors = geometry().ok()?;
        ding::primary(&monitors, screens)
    }

    fn wallpaper(&self, _screen: &Screen) -> Result<Option<PathBuf>> {
        let key = self.in_effect();
        Ok(gsetting(BACKGROUND, key).and_then(|text| gvariant::string(&text)).and_then(|uri| picture(&uri)))
    }

    fn set_wallpaper(&self, _screen: &Screen, path: &Path) -> Result<()> {
        let value = gvariant::quote(&gio::path_uri(path));
        let wanted = vec![
            (format!("{BACKGROUND} picture-uri"), value.clone()),
            (format!("{BACKGROUND} picture-uri-dark"), value),
            (format!("{BACKGROUND} picture-options"), "'zoom'".to_string()),
        ];
        let in_effect = format!("{BACKGROUND} {}", self.in_effect());
        settings::write(&Gsettings, &wanted, &in_effect, |old| settings::same_path(old, path, |text| gvariant::string(text).and_then(|uri| gio::location(&uri))))?;
        Ok(())
    }

    fn watches(&self) -> Watches {
        let desktop_in_home = wakes::desktop_in_home(&self.desktop, &self.home);
        Watches { files: Vec::new(), signals: vec![Signal::Metadata(desktop_in_home), Signal::Dconf(&wakes::GNOME_KEYS), Signal::Ding] }
    }

    fn per_screen(&self) -> bool {
        false
    }

    fn tools(&self) -> &'static [&'static str] {
        &["gio", "gsettings"]
    }
}
