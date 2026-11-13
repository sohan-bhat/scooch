use super::geometry::Insets;
use super::keyfile::{find, Group};
use scooch_core::Rect;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const FOLDER: &str = "org.kde.plasma.folder";
pub const APPLETS: &str = "plasma-org.kde.plasma.desktop-appletsrc";
pub const SHELL: &str = "plasmashellrc";
pub const IMAGE_PLUGIN: &str = "org.kde.image";
pub const INSETS: Insets = Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
pub const PLASMA6_SIZES: [f64; 10] = [22.0, 32.0, 48.0, 64.0, 96.0, 128.0, 160.0, 192.0, 224.0, 256.0];
pub const PLASMA5_SIZES: [f64; 7] = [22.0, 32.0, 48.0, 64.0, 96.0, 128.0, 256.0];
pub const SMALL_ICON: f64 = 16.0;
pub const GRID_UNIT: f64 = 18.0;
pub const SMALL_SPACING: f64 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Containment {
    pub id: String,
    pub screen: i64,
    pub activity: Option<String>,
    pub plugin: String,
    pub wallpaper_plugin: Option<String>,
    pub general: BTreeMap<String, String>,
    pub image: Option<String>,
}

impl Containment {
    pub fn setting(&self, key: &str) -> Option<i64> {
        self.general.get(key)?.trim().parse().ok()
    }

    pub fn folder(&self) -> bool {
        self.plugin == FOLDER
    }
}

pub fn containments(groups: &[Group]) -> Vec<Containment> {
    let mut found = Vec::new();
    for group in groups {
        if group.name.len() != 2 || group.name[0] != "Containments" {
            continue;
        }
        let Some(plugin) = group.get("plugin") else { continue };
        let desktop = group.number("formfactor").unwrap_or(0) == 0 && group.number("location").unwrap_or(0) == 0;
        if !desktop {
            continue;
        }
        let Some(screen) = group.number("lastScreen") else { continue };
        let id = group.name[1].clone();
        let general = find(groups, &["Containments", &id, "General"]).map(|general| general.entries.clone()).unwrap_or_default();
        let image = find(groups, &["Containments", &id, "Wallpaper", IMAGE_PLUGIN, "General"]).and_then(|wallpaper| wallpaper.get("Image")).map(str::to_string);
        found.push(Containment {
            id,
            screen,
            activity: group.get("activityId").filter(|value| !value.is_empty()).map(str::to_string),
            plugin: plugin.to_string(),
            wallpaper_plugin: group.get("wallpaperplugin").map(str::to_string),
            general,
            image,
        });
    }
    found
}

pub fn for_screen<'a>(all: &'a [Containment], screen: i64, activity: Option<&str>) -> Option<&'a Containment> {
    let list: Vec<&Containment> = all.iter().filter(|containment| containment.screen == screen).collect();
    if let Some(activity) = activity {
        if let Some(found) = list.iter().find(|containment| containment.activity.as_deref() == Some(activity)) {
            return Some(found);
        }
    }
    list.into_iter().next()
}

pub fn connectors(groups: &[Group]) -> BTreeMap<i64, String> {
    find(groups, &["ScreenConnectors"]).map(|group| group.entries.iter().filter_map(|(key, value)| Some((key.parse().ok()?, value.clone()))).collect()).unwrap_or_default()
}

pub fn current_activity(groups: &[Group]) -> Option<String> {
    find(groups, &["main"]).and_then(|group| group.get("currentActivity")).filter(|value| !value.is_empty()).map(str::to_string)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Positions {
    pub stripes: i64,
    pub per_stripe: i64,
    pub items: Vec<(String, i64, i64)>,
}

pub fn resolution(frame: &Rect) -> String {
    format!("{}x{}", frame.w.floor() as i64, frame.h.floor() as i64)
}

pub fn positions(raw: &str, resolution: &str) -> Option<Positions> {
    let cleaned = raw.replace("\\,", ",");
    let list: Vec<String> = match serde_json::from_str::<serde_json::Value>(&cleaned) {
        Ok(serde_json::Value::Object(map)) => map.get(resolution)?.as_array()?.iter().filter_map(|value| value.as_str().map(str::to_string)).collect(),
        _ => cleaned.split(',').map(str::to_string).collect(),
    };
    if list.len() < 2 || !(list.len() - 2).is_multiple_of(3) {
        return None;
    }
    let stripes = list[0].trim().parse().ok()?;
    let per_stripe = list[1].trim().parse().ok()?;
    let mut items = Vec::new();
    for chunk in list[2..].chunks(3) {
        let (Ok(stripe), Ok(pos)) = (chunk[1].trim().parse(), chunk[2].trim().parse()) else { continue };
        if !chunk[0].is_empty() {
            items.push((chunk[0].clone(), stripe, pos));
        }
    }
    Some(Positions { stripes, per_stripe, items })
}

pub fn occupied(positions: Option<&Positions>, present: &dyn Fn(&str) -> bool, count: usize, per_stripe: i64, plasma6: bool) -> Vec<i64> {
    let Some(positions) = positions.filter(|positions| !positions.items.is_empty()) else {
        return (0..count as i64).collect();
    };
    let mut taken: BTreeSet<i64> = BTreeSet::new();
    let mut placed: BTreeSet<&str> = BTreeSet::new();
    let fits = |pos: i64| if plasma6 { pos < per_stripe } else { pos <= per_stripe };
    for (name, stripe, pos) in &positions.items {
        if !fits(*pos) || placed.contains(name.as_str()) || !present(name) {
            continue;
        }
        let index = stripe * per_stripe + pos;
        if taken.contains(&index) {
            continue;
        }
        taken.insert(index);
        placed.insert(name);
    }
    let mut left = count.saturating_sub(placed.len());
    let first_free = |taken: &BTreeSet<i64>| -> i64 {
        let last = taken.iter().next_back().copied().unwrap_or(-1);
        (0..=last).find(|index| !taken.contains(index)).unwrap_or(last + 1)
    };
    while left > 0 {
        let index = first_free(&taken);
        taken.insert(index);
        left -= 1;
    }
    taken.into_iter().collect()
}

pub fn item_path(url: &str, desktop: &Path) -> Option<PathBuf> {
    if let Some(rest) = url.strip_prefix("desktop:/") {
        let rest = rest.trim_start_matches('/');
        return Some(desktop.join(super::gio::bytes::to_path(super::gio::decode(rest))));
    }
    super::gio::uri_path(url)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub plasma6: bool,
    pub icon: f64,
    pub label_width: f64,
    pub text_lines: f64,
    pub rows_first: bool,
    pub right_to_left: bool,
    pub grid_unit: f64,
}

impl Layout {
    pub fn from(containment: &Containment, plasma6: bool) -> Layout {
        let index = containment.setting("iconSize").unwrap_or(3).max(0) as usize;
        let sizes: &[f64] = if plasma6 { &PLASMA6_SIZES } else { &PLASMA5_SIZES };
        Layout {
            plasma6,
            icon: sizes.get(index).copied().unwrap_or(64.0),
            label_width: containment.setting("labelWidth").unwrap_or(1) as f64,
            text_lines: containment.setting("textLines").unwrap_or(2) as f64,
            rows_first: containment.setting("arrangement").unwrap_or(0) == 0,
            right_to_left: containment.setting("alignment").unwrap_or(0) != 0,
            grid_unit: GRID_UNIT,
        }
    }

    pub fn small_spacing(&self) -> f64 {
        if self.plasma6 {
            SMALL_SPACING
        } else {
            (self.grid_unit / 4.0).floor().max(2.0)
        }
    }

    pub fn cell(&self, view_width: f64, view_height: f64) -> (f64, f64) {
        let small = self.small_spacing();
        let natural = self.icon + 2.0 * self.grid_unit + 2.0 * small;
        let least_wide = natural.max(SMALL_ICON * (self.label_width * 2.0 + 4.0));
        let tall = if self.plasma6 { self.icon + self.grid_unit * self.text_lines + small * 3.0 } else { self.icon + self.grid_unit * self.text_lines + small * 4.0 };
        (least_wide + extra(least_wide, view_width), tall + extra(tall, view_height))
    }

    pub fn per_stripe(&self, view_width: f64, view_height: f64) -> i64 {
        let (wide, tall) = self.cell(view_width, view_height);
        if self.rows_first {
            (view_width / wide).floor() as i64
        } else {
            (view_height / tall).floor() as i64
        }
    }

    pub fn rect(&self, index: i64, area: &Rect) -> Rect {
        let (view_width, view_height) = (area.w as f64, area.h as f64);
        let (wide, tall) = self.cell(view_width, view_height);
        let per = self.per_stripe(view_width, view_height).max(1);
        let (col, row) = if self.rows_first { (index % per, index / per) } else { (index / per, index % per) };
        let left = if self.right_to_left { view_width - (col as f64 + 1.0) * wide } else { col as f64 * wide };
        Rect { x: (area.x as f64 + left) as f32, y: (area.y as f64 + row as f64 * tall) as f32, w: wide as f32, h: tall as f32 }
    }
}

pub fn extra(cell: f64, container: f64) -> f64 {
    let fit = (container / cell).floor();
    if fit <= 0.0 {
        return 0.0;
    }
    ((container - fit * cell).max(0.0) / fit).floor()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub screen: i64,
    pub location: String,
    pub height: f64,
    pub hiding: String,
}

pub fn panels(json: &str) -> Vec<Panel> {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(json.trim()) else { return Vec::new() };
    list.iter()
        .filter_map(|panel| {
            Some(Panel {
                screen: panel.get("screen")?.as_i64()?,
                location: panel.get("location")?.as_str()?.to_string(),
                height: panel.get("height")?.as_f64()?,
                hiding: panel.get("hiding").and_then(|value| value.as_str()).unwrap_or("none").to_string(),
            })
        })
        .collect()
}

pub fn available(frame: &Rect, screen: i64, panels: &[Panel]) -> Rect {
    let mut edges = [0.0f64; 4];
    for panel in panels.iter().filter(|panel| panel.screen == screen && panel.hiding == "none") {
        let slot = match panel.location.as_str() {
            "top" => 0,
            "bottom" => 1,
            "left" => 2,
            "right" => 3,
            _ => continue,
        };
        edges[slot] = edges[slot].max(panel.height);
    }
    super::geometry::subtract(frame, edges)
}

pub const GENERAL_KEYS: [&str; 9] = ["positions", "sortMode", "iconSize", "labelWidth", "textLines", "arrangement", "alignment", "url", "showHiddenFiles"];

pub fn state_script() -> String {
    let keys = serde_json::to_string(&GENERAL_KEYS).unwrap_or_else(|_| "[]".into());
    format!(
        "var out = {{activity: currentActivity(), screens: [], desktops: [], panels: []}}; \
for (var i = 0; i < screenCount; i++) {{ var g = screenGeometry(i); out.screens.push({{id: i, x: g.x, y: g.y, width: g.width, height: g.height}}); }} \
var keys = {keys}; var all = desktopsForActivity(currentActivity()); \
for (var j = 0; j < all.length; j++) {{ var d = all[j]; var item = {{id: d.id, screen: d.screen, type: d.type, wallpaper: d.wallpaperPlugin, general: {{}}}}; \
d.currentConfigGroup = ['General']; for (var k = 0; k < keys.length; k++) {{ var v = d.readConfig(keys[k], ''); item.general[keys[k]] = (v === undefined || v === null) ? '' : String(v); }} \
d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General']; var image = d.readConfig('Image', ''); item.image = (image === undefined || image === null) ? '' : String(image); d.currentConfigGroup = []; out.desktops.push(item); }} \
var ps = panels(); for (var p = 0; p < ps.length; p++) {{ out.panels.push({{screen: ps[p].screen, location: ps[p].location, height: ps[p].height, hiding: ps[p].hiding}}); }} \
print(JSON.stringify(out));"
    )
}

pub fn read_image_script(id: i64) -> String {
    format!("var d = desktopById({id}); if (d) {{ d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General']; var v = d.readConfig('Image', ''); print((v === undefined || v === null) ? '' : String(v)); }}")
}

pub fn wallpaper_script(id: i64, uri: &str) -> String {
    let text = serde_json::to_string(uri).unwrap_or_else(|_| "\"\"".into());
    format!("var d = desktopById({id}); if (d) {{ d.wallpaperPlugin = 'org.kde.image'; d.currentConfigGroup = ['Wallpaper', 'org.kde.image', 'General']; d.writeConfig('Image', {text}); }}")
}

pub fn record_key(id: i64) -> String {
    format!("containment {id} Image")
}

pub fn record_id(key: &str) -> Option<i64> {
    key.strip_prefix("containment ")?.strip_suffix(" Image")?.parse().ok()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub activity: Option<String>,
    pub screens: Vec<(i64, Rect)>,
    pub desktops: Vec<Containment>,
    pub panels: Vec<Panel>,
}

fn text_of(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items.iter().map(text_of).collect::<Vec<_>>().join(","),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub fn state(json: &str) -> Option<State> {
    let root: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    let screens = root.get("screens")?.as_array()?.iter().filter_map(|screen| {
        let number = |key: &str| screen.get(key).and_then(serde_json::Value::as_f64);
        Some((screen.get("id")?.as_i64()?, Rect { x: number("x")? as f32, y: number("y")? as f32, w: number("width")? as f32, h: number("height")? as f32 }))
    });
    let activity = root.get("activity").and_then(serde_json::Value::as_str).filter(|id| !id.is_empty()).map(str::to_string);
    let desktops = root.get("desktops")?.as_array()?.iter().filter_map(|desktop| {
        let general = desktop.get("general").and_then(serde_json::Value::as_object).map(|map| map.iter().map(|(key, value)| (key.clone(), text_of(value))).collect()).unwrap_or_default();
        Some(Containment {
            id: desktop.get("id")?.as_i64()?.to_string(),
            screen: desktop.get("screen").and_then(serde_json::Value::as_i64).unwrap_or(-1),
            activity: activity.clone(),
            plugin: desktop.get("type").map(text_of).unwrap_or_default(),
            wallpaper_plugin: desktop.get("wallpaper").map(text_of).filter(|plugin| !plugin.is_empty()),
            general,
            image: desktop.get("image").map(text_of),
        })
    });
    let panels = root.get("panels").map(|value| panels(&value.to_string())).unwrap_or_default();
    Some(State { activity: activity.clone(), screens: screens.collect(), desktops: desktops.collect(), panels })
}

fn normalized(rects: &[Rect]) -> Vec<[f32; 4]> {
    let left = rects.iter().map(|rect| rect.x).fold(f32::INFINITY, f32::min);
    let top = rects.iter().map(|rect| rect.y).fold(f32::INFINITY, f32::min);
    let right = rects.iter().map(|rect| rect.x + rect.w).fold(f32::NEG_INFINITY, f32::max);
    let bottom = rects.iter().map(|rect| rect.y + rect.h).fold(f32::NEG_INFINITY, f32::max);
    let (wide, tall) = ((right - left).max(1.0), (bottom - top).max(1.0));
    rects.iter().map(|rect| [(rect.x - left) / wide, (rect.y - top) / tall, rect.w / wide, rect.h / tall]).collect()
}

pub fn match_screens(plasma: &[Rect], frames: &[Rect]) -> Vec<Option<usize>> {
    let (one, two) = (normalized(plasma), normalized(frames));
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for (i, a) in one.iter().enumerate() {
        for (j, b) in two.iter().enumerate() {
            pairs.push((a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum(), i, j));
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = vec![None; plasma.len()];
    let mut taken = vec![false; frames.len()];
    for (_, i, j) in pairs {
        if out[i].is_none() && !taken[j] {
            out[i] = Some(j);
            taken[j] = true;
        }
    }
    out
}

pub fn look_and_feel_wallpaper(defaults: &[Group]) -> Option<String> {
    find(defaults, &["Wallpaper"])?.get("Image").map(str::trim).filter(|name| !name.is_empty()).map(str::to_string)
}

pub fn look_and_feel_package(kdeglobals: &[Group]) -> String {
    find(kdeglobals, &["KDE"]).and_then(|group| group.get("LookAndFeelPackage")).map(str::trim).filter(|name| !name.is_empty()).unwrap_or("org.kde.breeze.desktop").to_string()
}

pub fn distance(size: (f64, f64), desired: (f64, f64)) -> f64 {
    let desired_ratio = if desired.1 > 0.0 { desired.0 / desired.1 } else { 0.0 };
    let candidate_ratio = if size.1 > 0.0 { size.0 / size.1 } else { f64::MAX };
    let delta = size.0 - desired.0;
    let delta = if delta >= 0.0 { delta } else { -delta * 2.0 };
    (candidate_ratio - desired_ratio).abs() * 25000.0 + delta
}

pub fn best_image(files: &[PathBuf], width: f64, height: f64) -> Option<PathBuf> {
    let target = if width > 0.0 && height > 0.0 { (width, height) } else { (1920.0, 1080.0) };
    let mut sorted: Vec<&PathBuf> = files.iter().collect();
    sorted.sort();
    let mut best: Option<(f64, &PathBuf)> = None;
    for file in sorted {
        let Some(stem) = file.file_stem().and_then(|stem| stem.to_str()) else { continue };
        let Some((w, h)) = stem.split_once('x') else { continue };
        let (Ok(w), Ok(h)) = (w.parse::<f64>(), h.parse::<f64>()) else { continue };
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let gap = distance((w, h), target);
        if best.is_none_or(|(least, _)| gap < least) {
            best = Some((gap, file));
        }
    }
    best.map(|(_, file)| file.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::parse::keyfile::parse;

    const APPLETS: &str = "[Containments][1]\nactivityId=a1\nformfactor=0\nlastScreen=0\nlocation=0\nplugin=org.kde.plasma.folder\nwallpaperplugin=org.kde.image\n\n[Containments][1][General]\npositions={\"1920x1080\":[\"1\"\\,\"17\"\\,\"desktop:/b.txt\"\\,\"0\"\\,\"3\"\\,\"desktop:/a.txt\"\\,\"0\"\\,\"0\"]}\nsortMode=-1\n\n[Containments][1][Wallpaper][org.kde.image][General]\nImage=file:///home/ada/Pictures/lake.jpg\n\n[Containments][2]\nformfactor=2\nlastScreen=0\nlocation=4\nplugin=org.kde.panel\n\n[Containments][3]\nactivityId=a2\nformfactor=0\nlastScreen=0\nplugin=org.kde.desktopcontainment\n";

    #[test]
    fn desktop_containments_are_found_and_panels_skipped() {
        let all = containments(&parse(APPLETS));
        assert_eq!(all.len(), 2);
        assert!(all[0].folder() && !all[1].folder());
        assert_eq!(all[0].image.as_deref(), Some("file:///home/ada/Pictures/lake.jpg"));
        assert_eq!(all[0].setting("sortMode"), Some(-1));
        assert_eq!(for_screen(&all, 0, Some("a2")).map(|c| c.id.as_str()), Some("3"));
        assert_eq!(for_screen(&all, 0, None).map(|c| c.id.as_str()), Some("1"));
        assert_eq!(for_screen(&all, 1, None), None);
    }

    #[test]
    fn positions_read_with_escaped_commas() {
        let all = containments(&parse(APPLETS));
        let found = positions(&all[0].general["positions"], "1920x1080").unwrap();
        assert_eq!((found.stripes, found.per_stripe), (1, 17));
        assert_eq!(found.items, vec![("desktop:/b.txt".to_string(), 0, 3), ("desktop:/a.txt".to_string(), 0, 0)]);
        assert_eq!(positions(&all[0].general["positions"], "1280x720"), None);
        assert_eq!(positions("1,8,desktop:/x,0,1", "any").unwrap().items, vec![("desktop:/x".to_string(), 0, 1)]);
    }

    #[test]
    fn saved_cells_are_kept_and_new_items_fill_the_gaps() {
        let saved = Positions { stripes: 1, per_stripe: 17, items: vec![("desktop:/b.txt".into(), 0, 3), ("desktop:/a.txt".into(), 0, 0), ("desktop:/gone.txt".into(), 0, 1)] };
        let present = |name: &str| name != "desktop:/gone.txt";
        assert_eq!(occupied(Some(&saved), &present, 4, 17, true), vec![0, 1, 2, 3]);
        assert_eq!(occupied(Some(&saved), &present, 2, 17, true), vec![0, 3]);
        let far = Positions { stripes: 2, per_stripe: 17, items: vec![("desktop:/a.txt".into(), 1, 5)] };
        assert_eq!(occupied(Some(&far), &|_| true, 2, 17, true), vec![0, 22]);
        assert_eq!(occupied(None, &|_| true, 3, 17, true), vec![0, 1, 2]);
    }

    #[test]
    fn a_position_past_the_stripe_moves_to_a_free_cell() {
        let wide = Positions { stripes: 1, per_stripe: 20, items: vec![("desktop:/a.txt".into(), 0, 17), ("desktop:/b.txt".into(), 0, 18)] };
        assert_eq!(occupied(Some(&wide), &|_| true, 2, 17, true), vec![0, 1]);
        assert_eq!(occupied(Some(&wide), &|_| true, 2, 17, false), vec![0, 17]);
    }

    #[test]
    fn plasma6_cells_follow_the_folder_view_rules() {
        let layout = Layout { plasma6: true, icon: 64.0, label_width: 1.0, text_lines: 2.0, rows_first: true, right_to_left: false, grid_unit: 18.0 };
        assert_eq!(layout.cell(1920.0, 1036.0), (112.0, 115.0));
        assert_eq!(layout.per_stripe(1920.0, 1036.0), 17);
        let area = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1036.0 };
        assert_eq!(layout.rect(18, &area), Rect { x: 112.0, y: 115.0, w: 112.0, h: 115.0 });
        let columns = Layout { rows_first: false, ..layout };
        assert_eq!(columns.per_stripe(1920.0, 1036.0), 9);
        assert_eq!(columns.rect(10, &area), Rect { x: 112.0, y: 115.0, w: 112.0, h: 115.0 });
        let mirrored = Layout { right_to_left: true, ..layout };
        assert_eq!(mirrored.rect(0, &area).x, 1920.0 - 112.0);
    }

    #[test]
    fn plasma5_cells_use_a_quarter_grid_unit_padding() {
        let layout = Layout { plasma6: false, icon: 64.0, label_width: 1.0, text_lines: 2.0, rows_first: true, right_to_left: false, grid_unit: 18.0 };
        assert_eq!(layout.small_spacing(), 4.0);
        let (wide, tall) = layout.cell(1920.0, 1036.0);
        assert_eq!((wide, tall), (112.0, 116.0 + extra(116.0, 1036.0)));
    }

    #[test]
    fn panels_shrink_the_folder_view() {
        let found = panels("[{\"screen\":0,\"location\":\"bottom\",\"height\":44,\"hiding\":\"none\"},{\"screen\":0,\"location\":\"top\",\"height\":30,\"hiding\":\"autohide\"},{\"screen\":1,\"location\":\"left\",\"height\":40,\"hiding\":\"none\"}]");
        assert_eq!(found.len(), 3);
        let frame = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
        assert_eq!(available(&frame, 0, &found), Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1036.0 });
        assert!(panels("not json").is_empty());
    }

    #[test]
    fn items_map_to_files() {
        let desktop = Path::new("/home/ada/Desktop");
        assert_eq!(item_path("desktop:/a%20b.txt", desktop), Some(desktop.join("a b.txt")));
        assert_eq!(item_path("file:///home/ada/Desktop/c.txt", desktop), Some(desktop.join("c.txt")));
        assert_eq!(item_path("trash:/", desktop), None);
    }

    #[test]
    fn the_best_package_image_fits_the_screen() {
        let files: Vec<PathBuf> = ["1280x800.png", "1920x1080.png", "3840x2160.png", "1080x1920.png", "screenshot.png"].iter().map(PathBuf::from).collect();
        assert_eq!(best_image(&files, 1920.0, 1080.0), Some(PathBuf::from("1920x1080.png")));
        assert_eq!(best_image(&files, 2560.0, 1440.0), Some(PathBuf::from("1920x1080.png")));
        assert_eq!(best_image(&files, 3840.0, 2160.0), Some(PathBuf::from("3840x2160.png")));
        assert_eq!(distance((1920.0, 1080.0), (2560.0, 1440.0)), 1280.0);
        assert_eq!(best_image(&files, 1440.0, 900.0), Some(PathBuf::from("1280x800.png")));
        assert_eq!(best_image(&[], 1.0, 1.0), None);
    }

    #[test]
    fn live_state_reads_screens_desktops_and_panels() {
        let json = r#"{"activity":"a1","screens":[{"id":0,"x":1920,"y":0,"width":1920,"height":1080},{"id":1,"x":0,"y":0,"width":1920,"height":1080}],"desktops":[{"id":12,"screen":0,"type":"org.kde.plasma.folder","wallpaper":"org.kde.image","general":{"positions":"{\"1920x1080\":[\"1\"\\,\"17\"]}","sortMode":"-1"},"image":""}],"panels":[{"screen":0,"location":"bottom","height":44,"hiding":"none"}]}"#;
        let found = state(json).unwrap();
        assert_eq!(found.activity.as_deref(), Some("a1"));
        assert_eq!(found.screens[0], (0, Rect { x: 1920.0, y: 0.0, w: 1920.0, h: 1080.0 }));
        let desktop = &found.desktops[0];
        assert_eq!((desktop.id.as_str(), desktop.screen, desktop.folder(), desktop.image.as_deref()), ("12", 0, true, Some("")));
        assert_eq!(desktop.setting("sortMode"), Some(-1));
        assert_eq!(found.panels.len(), 1);
        assert!(state("not json").is_none());
        assert!(state_script().contains("desktopsForActivity(currentActivity())"));
        assert!(read_image_script(12).contains("readConfig('Image', '')"));
        assert_eq!(record_id(&record_key(12)), Some(12));
    }

    #[test]
    fn plasma_screens_match_outputs_by_layout_not_order() {
        let plasma = [Rect { x: 1920.0, y: 0.0, w: 1920.0, h: 1080.0 }, Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 }];
        let outputs = [Rect { x: 0.0, y: 0.0, w: 3840.0, h: 2160.0 }, Rect { x: 3840.0, y: 0.0, w: 3840.0, h: 2160.0 }];
        assert_eq!(match_screens(&plasma, &outputs), vec![Some(1), Some(0)]);
        assert_eq!(match_screens(&plasma[..1], &outputs[..1]), vec![Some(0)]);
    }

    #[test]
    fn the_default_wallpaper_follows_the_look_and_feel() {
        let defaults = parse("[Wallpaper]\nImage=Next\n");
        assert_eq!(look_and_feel_wallpaper(&defaults), Some("Next".into()));
        assert_eq!(look_and_feel_package(&parse("[KDE]\nLookAndFeelPackage=org.kde.breezedark.desktop\n")), "org.kde.breezedark.desktop");
        assert_eq!(look_and_feel_package(&[]), "org.kde.breeze.desktop");
    }

    #[test]
    fn scripts_quote_the_picture() {
        let script = wallpaper_script(7, "file:///p/it's \"here\".jpg");
        assert!(script.contains("desktopById(7)"));
        assert!(script.contains("d.writeConfig('Image', \"file:///p/it's \\\"here\\\".jpg\")"));
        assert_eq!(resolution(&Rect { x: 0.0, y: 0.0, w: 1920.6, h: 1080.0 }), "1920x1080");
    }
}
