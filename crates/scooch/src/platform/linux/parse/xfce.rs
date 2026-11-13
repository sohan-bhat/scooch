use super::geometry::Insets;
use super::keyfile::Group;
use super::yaml::Yaml;
use scooch_core::Rect;
use std::collections::BTreeMap;

pub const CHANNEL: &str = "xfce4-desktop";
pub const YAML_FILE: &str = "icons.screen0.yaml";
pub const LATEST_RC: &str = "icons.screen.latest.rc";
pub const VERSION_GROUP: &str = "xfdesktop-version-4.10.3+";
pub const MIN_MARGIN: i64 = 8;
pub const DEFAULT_ICON_SIZE: f64 = 48.0;
pub const INSETS: Insets = Insets { left: 2.0, top: 5.0, right: 8.0, bottom: 6.0 };
pub const INSETS_4_20: Insets = Insets { left: 4.0, top: 7.0, right: 11.0, bottom: 18.0 };

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub spacing: f64,
    pub padding: f64,
    pub proportion: f64,
}

pub const STYLE: Style = Style { spacing: 2.0, padding: 6.0, proportion: 1.9 };

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub geometry: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub level: i64,
    pub monitors: Vec<Monitor>,
    pub icons: BTreeMap<String, (i64, i64)>,
}

pub fn configs(doc: &Yaml) -> Vec<Config> {
    let Some(list) = doc.get("configs") else { return Vec::new() };
    list.list()
        .iter()
        .map(|config| {
            let monitors = config
                .get("monitors")
                .map(|monitors| {
                    monitors
                        .list()
                        .iter()
                        .map(|monitor| {
                            let geometry = monitor.get("geometry");
                            let number = |key: &str| geometry.and_then(|g| g.get(key)).and_then(Yaml::number).unwrap_or(0) as f32;
                            Monitor {
                                id: monitor.get("id").and_then(Yaml::text).unwrap_or_default().to_string(),
                                name: monitor.get("display_name").and_then(Yaml::text).unwrap_or_default().to_string(),
                                geometry: Rect { x: number("x"), y: number("y"), w: number("width"), h: number("height") },
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let icons = config
                .get("icons")
                .map(|icons| icons.entries().iter().filter_map(|(id, place)| Some((id.clone(), (place.get("row").and_then(Yaml::number)?, place.get("col").and_then(Yaml::number)?)))).collect())
                .unwrap_or_default();
            Config { level: config.get("level").and_then(Yaml::number).unwrap_or(0), monitors, icons }
        })
        .collect()
}

pub fn rc_positions(groups: &[Group]) -> BTreeMap<String, (i64, i64)> {
    groups
        .iter()
        .filter(|group| group.joined() != VERSION_GROUP)
        .filter_map(|group| Some((group.joined(), (group.number("row")?, group.number("col")?))))
        .filter(|(_, (row, col))| *row >= 0 && *col >= 0)
        .collect()
}

pub fn config_for<'a>(configs: &'a [Config], name: &str, frame: &Rect) -> Option<&'a Config> {
    let by_name = configs.iter().find(|config| config.monitors.iter().any(|monitor| !name.is_empty() && monitor.name == name));
    let by_place = || configs.iter().find(|config| config.monitors.iter().any(|monitor| monitor.geometry == *frame));
    let by_size = || configs.iter().find(|config| config.monitors.iter().any(|monitor| monitor.geometry.w == frame.w && monitor.geometry.h == frame.h));
    by_name.or_else(by_place).or_else(by_size)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub origin: (i64, i64),
    pub slot: f64,
    pub rows: i64,
    pub cols: i64,
    pub xspacing: i64,
    pub yspacing: i64,
    pub xmargin: i64,
    pub ymargin: i64,
}

fn c_div(a: i64, b: i64) -> i64 {
    if b == 0 {
        0
    } else {
        a / b
    }
}

pub fn grid(area: &Rect, icon: f64, style: &Style) -> Option<Grid> {
    let slot = style.proportion * icon + style.padding * 2.0;
    let (width, height) = (area.w as i64, area.h as i64);
    let rows = (((height - MIN_MARGIN * 2) as f64 / slot) as i64).max(0);
    let cols = (((width - MIN_MARGIN * 2) as f64 / slot) as i64).max(0);
    if rows <= 0 || cols <= 0 {
        return None;
    }
    let xrest = (width as f64 - cols as f64 * slot) as i64;
    let xspacing = if cols > 1 { c_div(xrest - MIN_MARGIN * 2, cols - 1) } else { 1 };
    let xmargin = c_div(xrest - (cols - 1) * xspacing, 2);
    let yrest = (height as f64 - rows as f64 * slot) as i64;
    let yspacing = if rows > 1 { c_div(yrest - MIN_MARGIN * 2, rows - 1) } else { 1 };
    let ymargin = c_div(yrest - (rows - 1) * yspacing, 2);
    Some(Grid { origin: (area.x as i64, area.y as i64), slot, rows, cols, xspacing, yspacing, xmargin, ymargin })
}

impl Grid {
    pub fn rect(&self, row: i64, col: i64) -> Option<Rect> {
        if row < 0 || col < 0 || row >= self.rows || col >= self.cols {
            return None;
        }
        let x = (self.origin.0 as f64 + self.xmargin as f64 + col as f64 * self.slot + (col * self.xspacing) as f64) as i64;
        let y = (self.origin.1 as f64 + self.ymargin as f64 + row as f64 * self.slot + (row * self.yspacing) as f64) as i64;
        Some(Rect { x: x as f32, y: y as f32, w: self.slot.floor() as f32, h: self.slot.floor() as f32 })
    }
}

pub fn version(text: &str) -> Option<(u32, u32)> {
    text.split(|letter: char| letter.is_whitespace() || letter == ',').find_map(|word| {
        let mut parts = word.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        Some((major, minor))
    })
}

pub fn single_workspace_default(version: Option<(u32, u32)>) -> bool {
    version.is_none_or(|version| version < (4, 19))
}

pub fn inset_area(work: &Rect, scale: i64) -> Rect {
    let scale = scale.max(1);
    let (x, y, w, h) = (work.x as i64 / scale, work.y as i64 / scale, work.w as i64 / scale, work.h as i64 / scale);
    Rect { x: (x + MIN_MARGIN) as f32, y: (y + MIN_MARGIN) as f32, w: (w - 2 * MIN_MARGIN) as f32, h: (h - 2 * MIN_MARGIN) as f32 }
}

pub fn backdrop_prefix(monitor: &str, workspace: i64) -> String {
    let name: String = monitor.chars().filter(|letter| !letter.is_whitespace()).collect();
    format!("/backdrop/screen0/monitor{name}/workspace{workspace}")
}

pub fn workspace_of(property: &str, monitor: &str) -> Option<i64> {
    let name: String = monitor.chars().filter(|letter| !letter.is_whitespace()).collect();
    let rest = property.strip_prefix(&format!("/backdrop/screen0/monitor{name}/workspace"))?;
    rest.strip_suffix("/last-image")?.parse().ok()
}

pub fn workarea(values: &[u32], desktop: usize) -> Option<Rect> {
    let at = desktop * 4;
    let slice = values.get(at..at + 4)?;
    Some(Rect { x: slice[0] as f32, y: slice[1] as f32, w: slice[2] as f32, h: slice[3] as f32 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::parse::{keyfile, yaml};

    #[test]
    fn a_full_hd_work_area_holds_ten_rows_of_forty_eight_pixel_icons() {
        let area = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
        let grid = grid(&area, 48.0, &STYLE).unwrap();
        assert!((grid.slot - 103.2).abs() < 1e-9);
        assert_eq!((grid.rows, grid.cols), (10, 18));
        assert_eq!((grid.xspacing, grid.yspacing), (2, 3));
        assert_eq!((grid.xmargin, grid.ymargin), (14, 10));
        assert_eq!(grid.rect(0, 0), Some(Rect { x: 14.0, y: 10.0, w: 103.0, h: 103.0 }));
        assert_eq!(grid.rect(1, 2), Some(Rect { x: 224.0, y: 116.0, w: 103.0, h: 103.0 }));
        assert_eq!(grid.rect(10, 0), None);
    }

    #[test]
    fn a_panel_moves_the_origin() {
        let area = Rect { x: 0.0, y: 30.0, w: 1920.0, h: 1050.0 };
        let grid = grid(&area, 48.0, &STYLE).unwrap();
        assert_eq!(grid.rows, 10);
        assert_eq!(grid.rect(0, 0).unwrap().y, 30.0 + grid.ymargin as f32);
    }

    #[test]
    fn yaml_configs_give_each_icon_a_row_and_column() {
        let doc = yaml::parse("configs:\n- level: 0\n  monitors:\n  - id: \"x1\"\n    display_name: \"screen\"\n    geometry:\n      x: 0\n      y: 0\n      width: 1920\n      height: 1080\n  icons:\n    \"/home/ada/Desktop/a.txt\":\n      row: 3\n      col: 1\n");
        let found = configs(&doc);
        assert_eq!(found[0].icons["/home/ada/Desktop/a.txt"], (3, 1));
        let frame = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
        assert!(config_for(&found, "screen", &frame).is_some());
        assert!(config_for(&found, "HDMI-1", &frame).is_some());
        assert!(config_for(&found, "HDMI-1", &Rect { w: 1280.0, h: 720.0, ..frame }).is_none());
    }

    #[test]
    fn older_rc_files_list_paths_as_groups() {
        let groups = keyfile::parse("[xfdesktop-version-4.10.3+]\n4.10.3+=true\n\n[/home/ada/Desktop/a.txt]\nrow=0\ncol=1\n\n[/home/ada]\nrow=1\ncol=0\n");
        let found = rc_positions(&groups);
        assert_eq!(found.len(), 2);
        assert_eq!(found["/home/ada"], (1, 0));
    }

    #[test]
    fn backdrop_properties_name_the_monitor_and_workspace() {
        assert_eq!(backdrop_prefix("Virtual-1", 0), "/backdrop/screen0/monitorVirtual-1/workspace0");
        assert_eq!(backdrop_prefix("LG Ultra", 2), "/backdrop/screen0/monitorLGUltra/workspace2");
        assert_eq!(workspace_of("/backdrop/screen0/monitorVirtual-1/workspace3/last-image", "Virtual-1"), Some(3));
        assert_eq!(workspace_of("/backdrop/screen0/monitorVirtual-1/workspace3/image-style", "Virtual-1"), None);
    }

    #[test]
    fn the_version_line_gives_major_and_minor() {
        assert_eq!(version("This is xfdesktop version 4.18.1, running on Xfce 4.18.\nBuilt with GTK+ 3.24.41"), Some((4, 18)));
        assert_eq!(version("This is xfdesktop version 4.20.1, running on Xfce 4.20."), Some((4, 20)));
        assert_eq!(version("nothing here"), None);
        assert!(single_workspace_default(Some((4, 18))));
        assert!(!single_workspace_default(Some((4, 20))));
        assert!(single_workspace_default(None));
    }

    #[test]
    fn xfdesktop_4_18_insets_the_work_area_by_eight_pixels() {
        let work = Rect { x: 0.0, y: 0.0, w: 1060.0, h: 800.0 };
        let inset = inset_area(&work, 1);
        assert_eq!(inset, Rect { x: 8.0, y: 8.0, w: 1044.0, h: 784.0 });
        assert_eq!(grid(&work, 48.0, &STYLE).unwrap().cols, 10);
        assert_eq!(grid(&inset, 48.0, &STYLE).unwrap().cols, 9);
        assert_eq!(inset_area(&Rect { x: 0.0, y: 60.0, w: 3840.0, h: 2100.0 }, 2), Rect { x: 8.0, y: 38.0, w: 1904.0, h: 1034.0 });
    }

    #[test]
    fn the_work_area_is_picked_per_desktop() {
        assert_eq!(workarea(&[0, 30, 1920, 1050, 0, 0, 1920, 1080], 1), Some(Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 }));
        assert_eq!(workarea(&[0, 30, 1920, 1050], 1), None);
    }
}
