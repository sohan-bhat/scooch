use scooch_core::Rect;
use std::path::{Path, PathBuf};

pub const SPAN: &str = "span";
pub const TWIN: &str = "scoochw.exe";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    pub inset: f32,
    pub above: f32,
    pub gap: f32,
    pub below: f32,
    pub sides: f32,
}

pub const SPACING: Spacing = Spacing { inset: 4.0, above: 4.0, gap: 6.0, below: 4.0, sides: 8.0 };

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub width: f32,
    pub icon: f32,
    pub line: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Label {
    pub width: f32,
    pub height: f32,
}

impl Cell {
    pub fn wrap(&self, spacing: &Spacing) -> f32 {
        (self.width - spacing.inset).max(1.0)
    }
}

pub fn lines(cell: &Cell, label: Label) -> f32 {
    (label.height / cell.line.max(1.0)).round().clamp(1.0, 2.0)
}

pub fn item_rect(at: (f32, f32), cell: &Cell, label: Label, spacing: &Spacing) -> Rect {
    let width = cell.icon.max(label.width.min(cell.wrap(spacing))) + spacing.sides;
    let left = at.0 + cell.icon / 2.0 - width / 2.0;
    let top = at.1 - spacing.above;
    let height = spacing.above + cell.icon + spacing.gap + lines(cell, label) * cell.line + spacing.below;
    Rect { x: left, y: top, w: width, h: height }
}

pub fn union(rects: &[Rect]) -> Option<Rect> {
    let first = rects.first()?;
    let (mut left, mut top, mut right, mut bottom) = (first.x, first.y, first.x + first.w, first.y + first.h);
    for rect in &rects[1..] {
        left = left.min(rect.x);
        top = top.min(rect.y);
        right = right.max(rect.x + rect.w);
        bottom = bottom.max(rect.y + rect.h);
    }
    Some(Rect { x: left, y: top, w: right - left, h: bottom - top })
}

pub fn plain(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else { return path.to_path_buf() };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

pub fn command_line(program: &Path) -> String {
    format!("\"{}\" watch", plain(program).display())
}

pub fn twin(exe: &Path) -> PathBuf {
    exe.with_file_name(TWIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> Cell {
        Cell { width: 75.0, icon: 48.0, line: 15.0 }
    }

    #[test]
    fn a_one_line_label_sits_under_the_icon() {
        let rect = item_rect((100.0, 200.0), &cell(), Label { width: 30.0, height: 15.0 }, &SPACING);
        assert_eq!(rect, Rect { x: 100.0 + 24.0 - 28.0, y: 196.0, w: 56.0, h: 4.0 + 48.0 + 6.0 + 15.0 + 4.0 });
    }

    #[test]
    fn a_wide_label_is_capped_at_the_wrap_width() {
        let rect = item_rect((0.0, 0.0), &cell(), Label { width: 140.0, height: 30.0 }, &SPACING);
        assert_eq!(rect.w, 71.0 + 8.0);
        assert_eq!(rect.x, 24.0 - 39.5);
        assert_eq!(rect.h, 4.0 + 48.0 + 6.0 + 30.0 + 4.0);
    }

    #[test]
    fn labels_count_at_most_two_lines() {
        assert_eq!(lines(&cell(), Label { width: 10.0, height: 60.0 }), 2.0);
        assert_eq!(lines(&cell(), Label { width: 10.0, height: 0.0 }), 1.0);
        assert_eq!(lines(&cell(), Label { width: 10.0, height: 30.0 }), 2.0);
    }

    #[test]
    fn the_rect_follows_every_constant() {
        let wide = Spacing { sides: 12.0, above: 6.0, ..SPACING };
        let base = item_rect((10.0, 10.0), &cell(), Label { width: 20.0, height: 15.0 }, &SPACING);
        let moved = item_rect((10.0, 10.0), &cell(), Label { width: 20.0, height: 15.0 }, &wide);
        assert_eq!(moved.w - base.w, 4.0);
        assert_eq!(base.y - moved.y, 2.0);
        assert_eq!(moved.h - base.h, 2.0);
    }

    #[test]
    fn union_covers_every_monitor() {
        let left = Rect { x: -1920.0, y: 120.0, w: 1920.0, h: 1080.0 };
        let main = Rect { x: 0.0, y: 0.0, w: 2560.0, h: 1440.0 };
        assert_eq!(union(&[main, left]), Some(Rect { x: -1920.0, y: 0.0, w: 4480.0, h: 1440.0 }));
        assert_eq!(union(&[]), None);
    }

    #[test]
    fn verbatim_prefixes_are_stripped() {
        assert_eq!(plain(Path::new(r"\\?\C:\Users\a\Pictures\lake.jpg")), PathBuf::from(r"C:\Users\a\Pictures\lake.jpg"));
        assert_eq!(plain(Path::new(r"\\?\UNC\server\share\lake.jpg")), PathBuf::from(r"\\server\share\lake.jpg"));
        assert_eq!(plain(Path::new(r"C:\plain.jpg")), PathBuf::from(r"C:\plain.jpg"));
    }

    #[test]
    fn the_login_command_quotes_the_program() {
        assert_eq!(command_line(Path::new(r"\\?\C:\Program Files\Scooch\scoochw.exe")), r#""C:\Program Files\Scooch\scoochw.exe" watch"#);
        assert_eq!(command_line(Path::new(r"C:\tools\scooch.exe")), r#""C:\tools\scooch.exe" watch"#);
    }

    #[test]
    fn the_twin_sits_next_to_the_command() {
        assert_eq!(twin(Path::new("bin/scooch.exe")), PathBuf::from("bin/scoochw.exe"));
    }
}
