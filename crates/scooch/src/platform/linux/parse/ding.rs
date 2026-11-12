use super::geometry::{logical, Insets};
use crate::platform::Screen;
use scooch_core::Rect;
use std::collections::BTreeMap;

pub const SPACING: f64 = 2.0;
pub const POSITION_KEY: &str = "metadata::nautilus-icon-position";
pub const BUS_NAME: &str = "com.rastersoft.dingextension";
pub const ACTIONS_PATH: &str = "/com/rastersoft/dingextension/control";
pub const GTK4_FORK_BUS_NAME: &str = "com.desktop.dingextension";
pub const INSETS: Insets = Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub icon: f64,
    pub width: f64,
    pub height: f64,
}

pub fn size(name: &str) -> Size {
    match name {
        "tiny" => Size { icon: 36.0, width: 70.0, height: 80.0 },
        "small" => Size { icon: 48.0, width: 90.0, height: 90.0 },
        "large" => Size { icon: 96.0, width: 130.0, height: 138.0 },
        _ => Size { icon: 64.0, width: 120.0, height: 106.0 },
    }
}

pub fn corner(name: &str) -> (bool, bool) {
    match name {
        "top-right" => (true, false),
        "bottom-left" => (false, true),
        "bottom-right" => (true, true),
        _ => (false, false),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Monitor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub zoom: f64,
    pub margins: [f64; 4],
    pub window: Option<[f64; 4]>,
    pub index: i64,
    pub primary: i64,
}

pub fn monitor(values: &BTreeMap<String, f64>) -> Option<Monitor> {
    let get = |key: &str| values.get(key).copied();
    let zoom = get("scaleFactor").or_else(|| get("zoom")).unwrap_or(1.0).max(1.0);
    let window = match (get("windowMarginTop"), get("windowMarginBottom"), get("windowMarginLeft"), get("windowMarginRight")) {
        (Some(top), Some(bottom), Some(left), Some(right)) => Some([top, bottom, left, right]),
        _ => None,
    };
    Some(Monitor {
        x: get("x")?,
        y: get("y")?,
        width: get("width")?,
        height: get("height")?,
        zoom,
        margins: [get("marginTop").unwrap_or(0.0), get("marginBottom").unwrap_or(0.0), get("marginLeft").unwrap_or(0.0), get("marginRight").unwrap_or(0.0)],
        window,
        index: get("monitorIndex").unwrap_or(0.0) as i64,
        primary: get("primaryMonitor").unwrap_or(0.0) as i64,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mode {
    pub x11: bool,
    pub premultiplied: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub monitor: Monitor,
    pub divisor: f64,
    pub window_x: f64,
    pub window_y: f64,
    pub window_w: f64,
    pub window_h: f64,
    pub margin_x: f64,
    pub margin_y: f64,
    pub width: f64,
    pub height: f64,
    pub cols: i64,
    pub rows: i64,
    pub cell_w: f64,
    pub cell_h: f64,
    pub area: [f64; 4],
    used: Vec<bool>,
}

impl Grid {
    pub fn new(monitor: &Monitor, size: Size, mode: Mode) -> Option<Grid> {
        let [top, bottom, left, right] = monitor.margins;
        let (divisor, window_x, window_y, window_w, window_h, margin_x, margin_y) = match monitor.window {
            Some([w_top, w_bottom, w_left, w_right]) => {
                let divisor = monitor.zoom;
                (
                    divisor,
                    monitor.x + w_left,
                    monitor.y + w_top,
                    ((monitor.width - (w_right + w_left)) / divisor).floor(),
                    ((monitor.height - (w_top + w_bottom)) / divisor).floor(),
                    ((left - w_left) / divisor).floor(),
                    ((top - w_top) / divisor).floor(),
                )
            }
            None => {
                let divisor = if mode.x11 {
                    monitor.zoom.ceil()
                } else if mode.premultiplied {
                    1.0
                } else {
                    monitor.zoom
                };
                (divisor, monitor.x, monitor.y, (monitor.width / divisor).floor(), (monitor.height / divisor).floor(), (left / divisor).floor(), (top / divisor).floor())
            }
        };
        let width = ((monitor.width - left - right) / divisor).floor();
        let height = ((monitor.height - top - bottom) / divisor).floor();
        let cols = (width / (size.width + 4.0 * SPACING)).floor() as i64;
        let rows = (height / (size.height + 4.0 * SPACING)).floor() as i64;
        if cols <= 0 || rows <= 0 {
            return None;
        }
        let area = match monitor.window {
            Some(_) => [window_x, window_y, width, height],
            None => [window_x + margin_x, window_y + margin_y, width, height],
        };
        Some(Grid {
            monitor: monitor.clone(),
            divisor,
            window_x,
            window_y,
            window_w,
            window_h,
            margin_x,
            margin_y,
            width,
            height,
            cols,
            rows,
            cell_w: (width / cols as f64).floor(),
            cell_h: (height / rows as f64).floor(),
            area,
            used: vec![false; (cols * rows) as usize],
        })
    }

    fn origin(&self) -> (f64, f64) {
        (self.window_x + self.margin_x, self.window_y + self.margin_y)
    }

    pub fn local(&self, x: f64, y: f64) -> (f64, f64) {
        let (ox, oy) = self.origin();
        ((x - ox).floor(), (y - oy).floor())
    }

    pub fn stored(&self, col: i64, row: i64) -> (f64, f64) {
        let (ox, oy) = self.origin();
        let lx = (self.width * col as f64 / self.cols as f64).floor() + SPACING;
        let ly = (self.height * row as f64 / self.rows as f64).floor() + SPACING;
        ((ox + lx).floor(), (oy + ly).floor())
    }

    pub fn belongs(&self, x: f64, y: f64) -> bool {
        let [ax, ay, aw, ah] = self.area;
        let (x, y) = (x.floor(), y.floor());
        x >= ax && x < ax + aw && y >= ay && y < ay + ah
    }

    fn free(&self) -> bool {
        self.used.iter().any(|used| !used)
    }

    fn empty(&self, col: i64, row: i64) -> bool {
        !self.used[(row * self.cols + col) as usize]
    }

    pub fn distance(&self, x: f64, y: f64) -> f64 {
        if !self.free() {
            return -1.0;
        }
        if self.belongs(x, y) {
            return 0.0;
        }
        let center_x = self.window_x + self.window_w * self.monitor.zoom / 2.0;
        let center_y = self.window_y + self.window_h * self.monitor.zoom / 2.0;
        (x - center_x).powi(2) + (x - center_y).powi(2)
    }

    fn closest(&self, x: f64, y: f64, assign: bool, corner: (bool, bool)) -> Option<(i64, i64)> {
        let (lx, ly) = self.local(x, y);
        let place_x = ((lx / self.cell_w).floor() as i64).clamp(0, self.cols - 1);
        let place_y = ((ly / self.cell_h).floor() as i64).clamp(0, self.rows - 1);
        if self.empty(place_x, place_y) && !assign {
            return Some((place_x, place_y));
        }
        let mut best = None;
        let mut least = f64::INFINITY;
        for step_col in 0..self.cols {
            let col = if corner.0 { self.cols - step_col - 1 } else { step_col };
            for step_row in 0..self.rows {
                let row = if corner.1 { self.rows - step_row - 1 } else { step_row };
                if !self.empty(col, row) {
                    continue;
                }
                if assign {
                    return Some((col, row));
                }
                let distance = (col as f64 * self.cell_w - lx).hypot(row as f64 * self.cell_h - ly);
                if distance < least {
                    least = distance;
                    best = Some((col, row));
                }
            }
        }
        best
    }

    fn take(&mut self, col: i64, row: i64) {
        self.used[(row * self.cols + col) as usize] = true;
    }

    pub fn cell(&self, col: i64, row: i64) -> [f64; 4] {
        let lx = (self.width * col as f64 / self.cols as f64).floor() + SPACING;
        let ly = (self.height * row as f64 / self.rows as f64).floor() + SPACING;
        let x = self.window_x + (self.margin_x + lx) * self.divisor;
        let y = self.window_y + (self.margin_y + ly) * self.divisor;
        [x, y, (self.cell_w - 2.0 * SPACING) * self.divisor, (self.cell_h - 2.0 * SPACING) * self.divisor]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub saved: Option<(f64, f64)>,
    pub drive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    pub size: Size,
    pub corner: (bool, bool),
    pub volumes_opposite: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub grid: usize,
    pub col: i64,
    pub row: i64,
}

pub fn place(grids: &mut [Grid], items: &[Item], prefs: &Prefs) -> Vec<Option<Placed>> {
    let mut out = vec![None; items.len()];
    let mut outside = Vec::new();
    let mut unassigned = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let Some((x, y)) = item.saved else {
            unassigned.push(index);
            continue;
        };
        match grids.iter().position(|grid| grid.distance(x, y) == 0.0) {
            Some(grid) => out[index] = put(grids, grid, x, y, false, item, prefs),
            None => outside.push(index),
        }
    }
    for index in outside {
        let Some((x, y)) = items[index].saved else { continue };
        let mut nearest: Option<(usize, f64)> = None;
        for (grid, candidate) in grids.iter().enumerate() {
            let distance = candidate.distance(x, y);
            if distance == -1.0 {
                continue;
            }
            if nearest.is_none_or(|(_, least)| distance < least) {
                nearest = Some((grid, distance));
            }
        }
        let Some((grid, _)) = nearest else { break };
        out[index] = put(grids, grid, x, y, false, &items[index], prefs);
    }
    let primary = grids.first().map(|grid| grid.monitor.primary).unwrap_or(0);
    let start = grids.iter().find(|grid| grid.monitor.index == primary).map_or((0.0, 0.0), |grid| (grid.monitor.x, grid.monitor.y));
    for index in unassigned {
        let (x, y) = start;
        let grid = grids.iter().position(|grid| grid.distance(x, y) == 0.0).or_else(|| grids.iter().position(|grid| grid.distance(x, y) != -1.0));
        if let Some(grid) = grid {
            out[index] = put(grids, grid, x, y, true, &items[index], prefs);
        }
    }
    out
}

fn put(grids: &mut [Grid], grid: usize, x: f64, y: f64, assign: bool, item: &Item, prefs: &Prefs) -> Option<Placed> {
    let mut corner = prefs.corner;
    if item.drive && prefs.volumes_opposite {
        corner.0 = !corner.0;
    }
    let (col, row) = grids[grid].closest(x, y, assign, corner)?;
    grids[grid].take(col, row);
    Some(Placed { grid, col, row })
}

pub fn screen_for(monitor: &Monitor, screens: &[Screen]) -> Option<usize> {
    if screens.len() == 1 {
        return Some(0);
    }
    let close = |a: f64, b: f64| (a - b).abs() <= 1.0;
    let mut best: Option<(usize, u8)> = None;
    for (index, screen) in screens.iter().enumerate() {
        for factor in [1.0, screen.scale as f64] {
            let frame = screen.frame;
            let sized = close(frame.w as f64 * factor, monitor.width) && close(frame.h as f64 * factor, monitor.height);
            let placed = close(frame.x as f64 * factor, monitor.x) && close(frame.y as f64 * factor, monitor.y);
            let score = u8::from(sized) * 2 + u8::from(placed);
            if score > 0 && best.is_none_or(|(_, top)| score > top) {
                best = Some((index, score));
            }
        }
    }
    best.map(|(index, _)| index)
}

pub fn rects(grids: &[Grid], placed: &[Option<Placed>], screens: &[Screen], insets: &Insets) -> Vec<Rect> {
    let mut out = Vec::new();
    for spot in placed.iter().flatten() {
        let grid = &grids[spot.grid];
        let Some(screen) = screen_for(&grid.monitor, screens).map(|index| &screens[index]) else { continue };
        let [x, y, w, h] = grid.cell(spot.col, spot.row);
        let monitor = [grid.monitor.x, grid.monitor.y, grid.monitor.width, grid.monitor.height];
        out.push(insets.apply(logical([x, y, w, h], monitor, &screen.frame)));
    }
    out
}

pub fn primary(monitors: &[Monitor], screens: &[Screen]) -> Option<usize> {
    let main = monitors.iter().find(|monitor| monitor.index == monitor.primary)?;
    screen_for(main, screens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fhd() -> Monitor {
        Monitor { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0, zoom: 1.0, margins: [32.0, 0.0, 0.0, 0.0], window: Some([32.0, 0.0, 0.0, 0.0]), index: 0, primary: 0 }
    }

    fn prefs() -> Prefs {
        Prefs { size: size("standard"), corner: corner("top-left"), volumes_opposite: true }
    }

    fn screen(id: &str, x: f32, w: f32, h: f32, scale: f32) -> Screen {
        Screen { id: id.into(), frame: Rect { x, y: 0.0, w, h }, scale }
    }

    #[test]
    fn the_geometry_dictionary_reads_both_forks() {
        let mut values: BTreeMap<String, f64> = [("x", 0.0), ("y", 0.0), ("width", 1920.0), ("height", 1080.0), ("scaleFactor", 1.0), ("marginTop", 32.0), ("monitorIndex", 0.0), ("primaryMonitor", 0.0)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        for key in ["windowMarginTop", "windowMarginBottom", "windowMarginLeft", "windowMarginRight"] {
            values.insert(key.into(), if key == "windowMarginTop" { 32.0 } else { 0.0 });
        }
        assert_eq!(monitor(&values), Some(fhd()));
        values.retain(|key, _| !key.starts_with("window"));
        values.remove("scaleFactor");
        values.insert("zoom".into(), 2.0);
        let older = monitor(&values).unwrap();
        assert_eq!((older.zoom, older.window), (2.0, None));
    }

    #[test]
    fn a_full_hd_screen_with_a_top_bar_has_fifteen_columns_and_nine_rows() {
        let grid = Grid::new(&fhd(), size("standard"), Mode::default()).unwrap();
        assert_eq!((grid.width, grid.height), (1920.0, 1048.0));
        assert_eq!((grid.cols, grid.rows), (15, 9));
        assert_eq!((grid.cell_w, grid.cell_h), (128.0, 116.0));
        assert_eq!(grid.stored(0, 0), (2.0, 34.0));
        assert_eq!(grid.stored(1, 2), (130.0, 32.0 + 232.0 + 2.0));
        assert_eq!(grid.cell(1, 2), [130.0, 266.0, 124.0, 112.0]);
    }

    #[test]
    fn a_stored_point_lands_in_its_own_cell() {
        let mut grids = vec![Grid::new(&fhd(), size("standard"), Mode::default()).unwrap()];
        let items = vec![Item { saved: Some((130.0, 266.0)), drive: false }, Item { saved: Some((2.0, 34.0)), drive: false }];
        let placed = place(&mut grids, &items, &prefs());
        assert_eq!(placed, vec![Some(Placed { grid: 0, col: 1, row: 2 }), Some(Placed { grid: 0, col: 0, row: 0 })]);
    }

    #[test]
    fn two_icons_on_one_cell_push_the_second_to_the_nearest_free_one() {
        let mut grids = vec![Grid::new(&fhd(), size("standard"), Mode::default()).unwrap()];
        let items = vec![Item { saved: Some((130.0, 266.0)), drive: false }, Item { saved: Some((131.0, 267.0)), drive: false }];
        let placed = place(&mut grids, &items, &prefs());
        assert_eq!(placed[0], Some(Placed { grid: 0, col: 1, row: 2 }));
        let second = placed[1].unwrap();
        assert!((second.col - 1).abs() + (second.row - 2).abs() == 1, "{second:?}");
    }

    #[test]
    fn icons_without_a_position_fill_the_first_column_from_the_corner() {
        let mut grids = vec![Grid::new(&fhd(), size("standard"), Mode::default()).unwrap()];
        let items = vec![Item::default(), Item::default(), Item { saved: None, drive: true }];
        let placed = place(&mut grids, &items, &prefs());
        assert_eq!(placed[0], Some(Placed { grid: 0, col: 0, row: 0 }));
        assert_eq!(placed[1], Some(Placed { grid: 0, col: 0, row: 1 }));
        assert_eq!(placed[2], Some(Placed { grid: 0, col: 14, row: 0 }));
    }

    #[test]
    fn the_older_fork_puts_the_bar_inside_the_window() {
        let monitor = Monitor { window: None, ..fhd() };
        let grid = Grid::new(&monitor, size("standard"), Mode::default()).unwrap();
        assert_eq!(grid.area, [0.0, 32.0, 1920.0, 1048.0]);
        assert_eq!(grid.stored(0, 0), (2.0, 34.0));
        let doubled = Monitor { x: 0.0, y: 0.0, width: 3840.0, height: 2160.0, zoom: 2.0, margins: [64.0, 0.0, 0.0, 0.0], window: None, index: 0, primary: 0 };
        let wayland = Grid::new(&doubled, size("standard"), Mode::default()).unwrap();
        assert_eq!((wayland.divisor, wayland.width, wayland.height), (2.0, 1920.0, 1048.0));
        let fractional = Grid::new(&doubled, size("standard"), Mode { x11: false, premultiplied: true }).unwrap();
        assert_eq!((fractional.divisor, fractional.width), (1.0, 3840.0));
    }

    #[test]
    fn stage_pixels_become_logical_pixels_on_a_doubled_screen() {
        let doubled = Monitor { x: 0.0, y: 0.0, width: 3840.0, height: 2160.0, zoom: 2.0, margins: [64.0, 0.0, 0.0, 0.0], window: Some([64.0, 0.0, 0.0, 0.0]), index: 0, primary: 0 };
        let mut grids = vec![Grid::new(&doubled, size("standard"), Mode::default()).unwrap()];
        let placed = place(&mut grids, &[Item::default()], &prefs());
        let found = rects(&grids, &placed, &[screen("DP-1", 0.0, 1920.0, 1080.0, 2.0)], &INSETS);
        assert_eq!(found, vec![Rect { x: 2.0, y: 34.0, w: 124.0, h: 112.0 }]);
    }

    #[test]
    fn monitors_match_screens_by_size_and_place() {
        let screens = [screen("HDMI-1", 0.0, 1920.0, 1080.0, 1.0), screen("eDP-1", 1920.0, 1440.0, 900.0, 2.0)];
        let right = Monitor { x: 1920.0, y: 0.0, width: 2880.0, height: 1800.0, ..fhd() };
        let left = Monitor { x: 0.0, ..fhd() };
        assert_eq!(screen_for(&left, &screens), Some(0));
        assert_eq!(screen_for(&Monitor { x: 3840.0, ..right.clone() }, &screens), Some(1));
        assert_eq!(screen_for(&Monitor { width: 1440.0, height: 900.0, x: 1920.0, ..fhd() }, &screens), Some(1));
        assert_eq!(primary(&[left, Monitor { index: 1, ..right }], &screens), Some(0));
    }

    #[test]
    fn sizes_and_corners_follow_the_settings() {
        assert_eq!(size("small"), Size { icon: 48.0, width: 90.0, height: 90.0 });
        assert_eq!(size("unexpected"), size("standard"));
        assert_eq!(corner("bottom-right"), (true, true));
    }
}
