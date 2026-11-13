use super::geometry::Insets;
use super::gvariant::{get, put, quote, string, Dict};
use super::keyfile::{find, Group};
use scooch_core::Rect;

pub const POSITION_KEY: &str = "nemo-icon-position";
pub const MONITOR_KEY: &str = "monitor";
pub const ZOOM_KEY: &str = "nemo-icon-view-zoom-level";
pub const HORIZONTAL_KEY: &str = "desktop-horizontal";
pub const ADJUST_KEY: &str = "desktop-grid-adjust";
pub const METADATA_FILE: &str = "desktop-metadata";
pub const BASE_SNAP_X: f64 = 130.0;
pub const BASE_SNAP_Y: f64 = 100.0;
pub const STANDARD_ICON: f64 = 48.0;
pub const STANDARD_ZOOM: i64 = 3;
pub const NON_PRIMARY_MARGIN: f64 = 25.0;
pub const LINKS: [(&str, &str); 4] = [("computer", "computer-icon-visible"), ("home", "home-icon-visible"), ("trash", "trash-icon-visible"), ("network", "network-icon-visible")];
pub const INSETS: Insets = Insets { left: 7.0, top: 7.0, right: 9.0, bottom: 6.0 };

pub fn icon_size(zoom: i64) -> f64 {
    match zoom {
        1 => 24.0,
        2 => 32.0,
        4 => 64.0,
        5 => 96.0,
        _ => STANDARD_ICON,
    }
}

pub fn position(value: &str) -> Option<(i64, i64)> {
    let mut parts = value.split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((x, y))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub primary: bool,
    pub others: bool,
}

pub fn layout(value: &str) -> Layout {
    let value = value.trim().trim_matches(|letter| letter == '\'' || letter == '"');
    match value.split_once("::") {
        Some((primary, others)) => Layout { primary: primary == "true", others: others == "true" },
        None => Layout { primary: true, others: false },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub zoom: i64,
    pub horizontal: bool,
    pub adjust: (i64, i64),
}

pub fn settings(groups: &[Group], monitor: i64) -> Settings {
    let name = format!("desktop-monitor-{monitor}");
    let group = find(groups, &[name.as_str()]);
    let zoom = group.and_then(|group| group.number(ZOOM_KEY)).unwrap_or(STANDARD_ZOOM);
    let horizontal = group.and_then(|group| group.get(HORIZONTAL_KEY)).is_some_and(|value| value.trim() == "true");
    let adjust = group.and_then(|group| group.get(ADJUST_KEY)).map_or((100, 100), |value| {
        let mut parts = value.split(';').map(str::trim);
        let horizontal = parts.next().and_then(|part| part.parse().ok()).unwrap_or(100);
        let vertical = parts.next().and_then(|part| part.parse().ok()).unwrap_or(100);
        (horizontal, vertical)
    });
    Settings { zoom, horizontal, adjust }
}

pub fn link(groups: &[Group], name: &str) -> (Option<(i64, i64)>, i64) {
    let group = find(groups, &[name]);
    let saved = group.and_then(|group| group.get(POSITION_KEY)).and_then(position);
    let monitor = group.and_then(|group| group.number(MONITOR_KEY)).unwrap_or(-1);
    (saved, monitor)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub canvas: Rect,
    pub snap_x: i64,
    pub snap_y: i64,
    pub cols: i64,
    pub rows: i64,
    pub left: i64,
    pub top: i64,
    pub horizontal: bool,
    used: Vec<bool>,
}

impl Grid {
    pub fn new(canvas: Rect, settings: &Settings) -> Option<Grid> {
        let scale = icon_size(settings.zoom) / STANDARD_ICON;
        let snap_x = (BASE_SNAP_X * scale * (settings.adjust.0 as f64 / 100.0)) as i64;
        let snap_y = (BASE_SNAP_Y * scale * (settings.adjust.1 as f64 / 100.0)) as i64;
        let (width, height) = (canvas.w as i64, canvas.h as i64);
        if snap_x <= 0 || snap_y <= 0 {
            return None;
        }
        let (cols, rows) = (width / snap_x, height / snap_y);
        if cols == 0 || rows == 0 {
            return None;
        }
        Some(Grid {
            canvas,
            snap_x,
            snap_y,
            cols,
            rows,
            left: (width - cols * snap_x) / 2,
            top: (height - rows * snap_y) / 2,
            horizontal: settings.horizontal,
            used: vec![false; (cols * rows) as usize],
        })
    }

    pub fn cell_of(&self, x: i64, y: i64) -> (i64, i64) {
        (((x - self.left) / self.snap_x).clamp(0, self.cols - 1), ((y - self.top) / self.snap_y).clamp(0, self.rows - 1))
    }

    fn slot(&self, col: i64, row: i64) -> usize {
        (col * self.rows + row) as usize
    }

    fn free(&self, col: i64, row: i64) -> bool {
        (col == self.cols - 1 && row == self.rows - 1) || !self.used[self.slot(col, row)]
    }

    fn mark(&mut self, col: i64, row: i64) {
        let slot = self.slot(col, row);
        self.used[slot] = true;
    }

    fn next(&self, col: i64, row: i64) -> (i64, i64) {
        let (mut next_col, mut next_row) = if self.horizontal { (col + 1, row) } else { (col, row + 1) };
        if self.horizontal && next_col >= self.cols {
            (next_col, next_row) = (0, row + 1);
        }
        if !self.horizontal && next_row >= self.rows {
            (next_col, next_row) = (col + 1, 0);
        }
        (next_col.clamp(0, self.cols - 1), next_row.clamp(0, self.rows - 1))
    }

    pub fn place(&mut self, saved: &[Option<(i64, i64)>]) -> Vec<(i64, i64)> {
        let mut out = vec![(0, 0); saved.len()];
        for (index, spot) in saved.iter().enumerate() {
            if let Some((x, y)) = spot {
                let cell = self.cell_of(*x, *y);
                out[index] = cell;
                if *x > 0 || *y > 0 {
                    self.mark(cell.0, cell.1);
                }
            }
        }
        for (index, spot) in saved.iter().enumerate() {
            if spot.is_some() {
                continue;
            }
            let mut cell = (0, 0);
            while !self.free(cell.0, cell.1) {
                cell = self.next(cell.0, cell.1);
            }
            self.mark(cell.0, cell.1);
            out[index] = cell;
        }
        out
    }

    pub fn rect(&self, col: i64, row: i64) -> Rect {
        Rect {
            x: self.canvas.x + (self.left + col * self.snap_x) as f32,
            y: self.canvas.y + (self.top + row * self.snap_y) as f32,
            w: self.snap_x as f32,
            h: self.snap_y as f32,
        }
    }
}

pub fn monitor_for(file_monitor: i64, shown: &[i64], primary: i64, layout: Layout) -> Option<i64> {
    if layout.primary && !layout.others {
        return shown.contains(&primary).then_some(primary);
    }
    if shown.contains(&file_monitor) {
        return Some(file_monitor);
    }
    if shown.contains(&primary) {
        return Some(primary);
    }
    shown.first().copied()
}

pub fn canvas(window: Rect, margins: [f64; 4]) -> Rect {
    super::geometry::subtract(&window, margins)
}

pub const LIST_KEY: &str = "picture-uri-list";
pub const MODE_KEY: &str = "background-mode";

pub fn entry_for(list: &[Dict], mode: &str, connector: &str) -> Option<usize> {
    if list.is_empty() {
        return None;
    }
    if mode == "independent" {
        if let Some(at) = list.iter().position(|dict| get(dict, "connector").and_then(string).as_deref() == Some(connector)) {
            return Some(at);
        }
        if let Some(at) = list.iter().position(|dict| get(dict, "index").map(|value| value.trim_start_matches("int32").trim()) == Some("0")) {
            return Some(at);
        }
    }
    Some(0)
}

pub fn listed_picture(list: &[Dict], mode: &str, connector: &str) -> Option<String> {
    let at = entry_for(list, mode, connector)?;
    get(&list[at], "picture-uri").and_then(string).filter(|uri| !uri.is_empty())
}

pub fn with_picture(list: &[Dict], mode: &str, connector: &str, uri: &str) -> Vec<Dict> {
    let mut out = list.to_vec();
    match entry_for(&out, mode, connector) {
        Some(at) => {
            put(&mut out[at], "picture-uri", quote(uri));
            put(&mut out[at], "picture-options", quote("zoom"));
            out[at].retain(|(key, _)| key != "slideshow");
        }
        None => out.push(vec![("picture-uri".to_string(), quote(uri)), ("picture-options".to_string(), quote("zoom"))]),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::parse::keyfile;

    fn work() -> Rect {
        Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1040.0 }
    }

    #[test]
    fn a_standard_grid_on_a_full_hd_work_area() {
        let grid = Grid::new(work(), &Settings { zoom: 3, horizontal: false, adjust: (100, 100) }).unwrap();
        assert_eq!((grid.snap_x, grid.snap_y, grid.cols, grid.rows), (130, 100, 14, 10));
        assert_eq!((grid.left, grid.top), (50, 20));
        assert_eq!(grid.rect(1, 2), Rect { x: 180.0, y: 220.0, w: 130.0, h: 100.0 });
    }

    #[test]
    fn zoom_and_adjust_scale_the_snap() {
        let large = Grid::new(work(), &Settings { zoom: 4, horizontal: false, adjust: (100, 100) }).unwrap();
        assert_eq!((large.snap_x, large.snap_y), (173, 133));
        let tight = Grid::new(work(), &Settings { zoom: 3, horizontal: false, adjust: (80, 120) }).unwrap();
        assert_eq!((tight.snap_x, tight.snap_y), (104, 120));
    }

    #[test]
    fn stored_icon_positions_find_their_cell() {
        let grid = Grid::new(work(), &Settings { zoom: 3, horizontal: false, adjust: (100, 100) }).unwrap();
        let x = 50 + 130 + 65 - 24;
        let y = 20 + 200 + 50 - 40;
        assert_eq!(grid.cell_of(x, y), (1, 2));
        assert_eq!(grid.cell_of(-30, -10), (0, 0));
        assert_eq!(grid.cell_of(5000, 5000), (13, 9));
    }

    #[test]
    fn unplaced_icons_fill_free_cells_down_the_first_column() {
        let mut grid = Grid::new(work(), &Settings { zoom: 3, horizontal: false, adjust: (100, 100) }).unwrap();
        let cells = grid.place(&[None, Some((50 + 41, 20 + 10)), None, Some((50 + 130 + 41, 20 + 10))]);
        assert_eq!(cells, vec![(0, 1), (0, 0), (0, 2), (1, 0)]);
        let mut rows = Grid::new(work(), &Settings { zoom: 3, horizontal: true, adjust: (100, 100) }).unwrap();
        assert_eq!(rows.place(&[None, None, Some((91, 30))]), vec![(1, 0), (2, 0), (0, 0)]);
    }

    #[test]
    fn the_metadata_file_holds_links_and_monitor_settings() {
        let groups = keyfile::parse("[desktop-monitor-0]\nnemo-icon-view-zoom-level=4\ndesktop-horizontal=true\ndesktop-grid-adjust=90;110;\n\n[home]\nnemo-icon-position=91,30\nmonitor=0\n\n[trash]\nnemo-icon-position=junk\n");
        assert_eq!(settings(&groups, 0), Settings { zoom: 4, horizontal: true, adjust: (90, 110) });
        assert_eq!(settings(&groups, 1), Settings { zoom: 3, horizontal: false, adjust: (100, 100) });
        assert_eq!(link(&groups, "home"), (Some((91, 30)), 0));
        assert_eq!(link(&groups, "trash"), (None, -1));
        assert_eq!(link(&groups, "computer"), (None, -1));
    }

    #[test]
    fn the_layout_setting_picks_monitors() {
        let primary_only = layout("'true::false'");
        assert_eq!(primary_only, Layout { primary: true, others: false });
        assert_eq!(monitor_for(1, &[0], 0, primary_only), Some(0));
        let all = layout("true::true");
        assert_eq!(monitor_for(1, &[0, 1], 0, all), Some(1));
        assert_eq!(monitor_for(-1, &[0, 1], 0, all), Some(0));
        assert_eq!(monitor_for(5, &[0, 1], 0, all), Some(0));
        assert_eq!(layout("garbage"), Layout { primary: true, others: false });
    }

    #[test]
    fn positions_parse_like_sscanf() {
        assert_eq!(position("91,30"), Some((91, 30)));
        assert_eq!(position(" 91 , 30 "), Some((91, 30)));
        assert_eq!(position("91,30,x"), None);
        assert_eq!(position(""), None);
        assert_eq!(icon_size(5), 96.0);
        assert_eq!(icon_size(0), 48.0);
    }

    #[test]
    fn margins_come_off_the_window() {
        let window = Rect { x: 1920.0, y: 0.0, w: 1280.0, h: 1024.0 };
        assert_eq!(canvas(window, [NON_PRIMARY_MARGIN; 4]), Rect { x: 1945.0, y: 25.0, w: 1230.0, h: 974.0 });
    }

    #[test]
    fn the_background_list_changes_only_the_shown_entry() {
        let list = super::super::gvariant::dicts("[{'connector': <'DP-1'>, 'index': <0>, 'picture-uri': <'file:///a.jpg'>, 'picture-options': <'zoom'>}, {'connector': <'HDMI-1'>, 'index': <1>, 'picture-uri': <'file:///b.jpg'>, 'slideshow': <true>}]");
        assert_eq!(listed_picture(&list, "independent", "HDMI-1").as_deref(), Some("file:///b.jpg"));
        assert_eq!(listed_picture(&list, "independent", "eDP-1").as_deref(), Some("file:///a.jpg"));
        assert_eq!(listed_picture(&list, "mirror", "HDMI-1").as_deref(), Some("file:///a.jpg"));
        let changed = with_picture(&list, "independent", "HDMI-1", "file:///out.jpg");
        assert_eq!(listed_picture(&changed, "independent", "HDMI-1").as_deref(), Some("file:///out.jpg"));
        assert_eq!(listed_picture(&changed, "independent", "DP-1").as_deref(), Some("file:///a.jpg"));
        assert_eq!(get(&changed[1], "slideshow"), None);
        let fresh = with_picture(&[], "mirror", "screen", "file:///out.jpg");
        assert_eq!(super::super::gvariant::write_dicts(&fresh), "[{'picture-uri': <'file:///out.jpg'>, 'picture-options': <'zoom'>}]");
        assert_eq!(listed_picture(&[], "mirror", "screen"), None);
    }
}
