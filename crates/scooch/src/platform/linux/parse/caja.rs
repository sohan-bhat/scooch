use super::geometry::Insets;
use super::keyfile::{find, Group};
use scooch_core::Rect;

pub const POSITION_KEY: &str = "caja-icon-position";
pub const SCALE_KEY: &str = "icon-scale";
pub const ZOOM_KEY: &str = "caja-icon-view-zoom-level";
pub const METADATA_FILE: &str = "desktop-metadata";
pub const DIRECTORY_GROUP: &str = "directory";
pub const STANDARD_ICON: f64 = 48.0;
pub const MAX_TEXT_WIDTH: f64 = 135.0;
pub const LABEL_OFFSET: f64 = 1.0;
pub const TEXT_PADDING_Y: f64 = 1.0;
pub const LINE_SPACING: f64 = 1.36;
pub const DEFAULT_POINTS: f64 = 10.0;
pub const TEXT_PADDING_X: f64 = 4.0;
pub const LINKS: [(&str, &str); 4] = [("computer", "computer-icon-visible"), ("home", "home-icon-visible"), ("trash", "trash-icon-visible"), ("network", "network-icon-visible")];
pub const INSETS: Insets = Insets { left: 0.0, top: -2.0, right: 0.0, bottom: 0.0 };

pub fn zoom_index(name: &str) -> i64 {
    match name.trim().trim_matches('\'') {
        "smallest" => 0,
        "smaller" => 1,
        "small" => 2,
        "large" => 4,
        "larger" => 5,
        "largest" => 6,
        _ => 3,
    }
}

pub fn icon_size(zoom: i64) -> f64 {
    match zoom {
        0 => 16.0,
        1 => 24.0,
        2 => 32.0,
        4 => 72.0,
        5 => 96.0,
        6 => 192.0,
        _ => STANDARD_ICON,
    }
}

pub fn position(value: &str) -> Option<(f64, f64)> {
    let mut parts = value.split(',');
    let x: i64 = parts.next()?.trim().parse().ok()?;
    let y: i64 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((x as f64, y as f64))
}

pub fn line_height(font: Option<&str>) -> f64 {
    let points = font.and_then(|font| font.trim().trim_matches('\'').split_whitespace().last()?.parse::<f64>().ok()).filter(|points| *points > 0.0 && points.is_finite()).unwrap_or(DEFAULT_POINTS);
    (points * 96.0 / 72.0 * LINE_SPACING).ceil()
}

pub fn scale(value: Option<&str>) -> f64 {
    value.and_then(|text| text.trim().parse::<f64>().ok()).filter(|scale| *scale > 0.0 && scale.is_finite()).unwrap_or(1.0)
}

pub fn link(groups: &[Group], name: &str) -> Option<((f64, f64), f64)> {
    let group = find(groups, &[name])?;
    let at = position(group.get(POSITION_KEY)?)?;
    Some((at, scale(group.get(SCALE_KEY))))
}

pub fn zoom(groups: &[Group], default: i64) -> i64 {
    find(groups, &[DIRECTORY_GROUP]).and_then(|group| group.number(ZOOM_KEY)).unwrap_or(default)
}

pub fn margins(workareas: &[u32], screen: (f64, f64)) -> [f64; 4] {
    let (mut left, mut right, mut top, mut bottom) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for area in workareas.chunks(4).filter(|area| area.len() == 4) {
        let (x, y, w, h) = (area[0] as f64, area[1] as f64, area[2] as f64, area[3] as f64);
        if x + w > screen.0 || y + h > screen.1 {
            continue;
        }
        left = left.max(x);
        right = right.max(screen.0 - w - x);
        top = top.max(y);
        bottom = bottom.max(screen.1 - h - y);
    }
    [top, bottom, left, right]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub zoom: i64,
    pub left: f64,
    pub top: f64,
    pub lines: f64,
    pub line: f64,
}

impl Layout {
    pub fn units(&self) -> f64 {
        icon_size(self.zoom) / STANDARD_ICON
    }

    pub fn rect(&self, at: (f64, f64), scale: f64) -> Rect {
        let units = self.units();
        let icon = icon_size(self.zoom) * scale;
        let left = self.left + at.0 * units;
        let top = self.top + at.1 * units;
        let wide = icon.max(MAX_TEXT_WIDTH * units + 2.0 * TEXT_PADDING_X);
        let tall = icon + LABEL_OFFSET + self.lines * self.line + 2.0 * TEXT_PADDING_Y;
        let center = left + icon / 2.0;
        Rect { x: (center - wide / 2.0) as f32, y: top as f32, w: wide as f32, h: tall as f32 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::parse::keyfile;

    #[test]
    fn a_standard_icon_has_a_label_box_under_it() {
        let layout = Layout { zoom: 3, left: 0.0, top: 32.0, lines: 3.0, line: line_height(Some("'Ubuntu 11'")) };
        let rect = layout.rect((10.0, 10.0), 1.0);
        assert_eq!(rect, Rect { x: 10.0 + 24.0 - 71.5, y: 42.0, w: 143.0, h: 48.0 + 1.0 + 60.0 + 2.0 });
    }

    #[test]
    fn zoom_scales_positions_and_sizes() {
        let layout = Layout { zoom: 4, left: 0.0, top: 0.0, lines: 2.0, line: 20.0 };
        assert_eq!(layout.units(), 1.5);
        let rect = layout.rect((100.0, 40.0), 1.0);
        assert_eq!((rect.y, rect.w), (60.0, 210.5));
        assert_eq!(rect.x, 150.0 + 36.0 - 105.25);
        let big = Layout { zoom: 3, left: 0.0, top: 0.0, lines: 1.0, line: 20.0 }.rect((0.0, 0.0), 4.0);
        assert_eq!(big.w, 192.0);
    }

    #[test]
    fn panels_set_the_margins() {
        assert_eq!(margins(&[0, 25, 1920, 1055], (1920.0, 1080.0)), [25.0, 0.0, 0.0, 0.0]);
        assert_eq!(margins(&[0, 25, 1920, 1055, 0, 0, 3840, 2160], (1920.0, 1080.0)), [25.0, 0.0, 0.0, 0.0]);
        assert_eq!(margins(&[48, 0, 1872, 1080], (1920.0, 1080.0)), [0.0, 0.0, 48.0, 0.0]);
    }

    #[test]
    fn links_and_zoom_come_from_the_metadata_file() {
        let groups = keyfile::parse("[directory]\ncaja-icon-view-zoom-level=4\n\n[home]\ncaja-icon-position=10,120\nicon-scale=1.5\n\n[trash]\ncaja-icon-position=\n");
        assert_eq!(zoom(&groups, 3), 4);
        assert_eq!(zoom(&[], 3), 3);
        assert_eq!(link(&groups, "home"), Some(((10.0, 120.0), 1.5)));
        assert_eq!(link(&groups, "trash"), None);
        assert_eq!(link(&groups, "computer"), None);
    }

    #[test]
    fn values_parse_like_caja() {
        assert_eq!(position(" 10 , -4 "), Some((10.0, -4.0)));
        assert_eq!(position("1.5,2"), None);
        assert_eq!(scale(Some("2.0")), 2.0);
        assert_eq!(scale(Some("nan")), 1.0);
        assert_eq!(scale(None), 1.0);
        assert_eq!(zoom_index("'large'"), 4);
        assert_eq!(zoom_index("standard"), 3);
        assert_eq!(icon_size(4), 72.0);
    }

    #[test]
    fn the_label_line_follows_the_font_size() {
        assert_eq!(line_height(Some("'Ubuntu 11'")), 20.0);
        assert_eq!(line_height(Some("Sans Bold 14")), 26.0);
        assert_eq!(line_height(Some("''")), 19.0);
        assert_eq!(line_height(None), 19.0);
    }
}
