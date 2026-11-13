use scooch_core::Rect;
use std::path::{Path, PathBuf};

pub const SPAN: &str = "span";
pub const TWIN: &str = "scoochw.exe";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    pub wrap: f32,
    pub cap: f32,
    pub above: f32,
    pub gap: f32,
    pub below: f32,
    pub sides: f32,
}

pub const SPACING: Spacing = Spacing { wrap: 4.0, cap: 10.0, above: -3.0, gap: 6.0, below: 4.0, sides: 6.0 };

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
        (self.width - spacing.wrap).max(1.0)
    }

    pub fn cap(&self, spacing: &Spacing) -> f32 {
        (self.width - spacing.cap).max(1.0)
    }
}

pub fn lines(cell: &Cell, label: Label) -> f32 {
    (label.height / cell.line.max(1.0)).round().clamp(1.0, 2.0)
}

pub fn scaled(size: f32, dpi: u32) -> f32 {
    if dpi == 0 {
        return size;
    }
    (size * dpi as f32 / 96.0).round()
}

pub fn item_rect(at: (f32, f32), cell: &Cell, label: Label, spacing: &Spacing) -> Rect {
    let width = cell.icon.max(label.width.min(cell.cap(spacing))) + spacing.sides;
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

fn folded(path: &Path) -> String {
    plain(path).to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

pub fn inside(path: &Path, folder: &Path) -> bool {
    let (path, folder) = (folded(path), folded(folder));
    !folder.is_empty() && path.len() > folder.len() + 1 && path.starts_with(&folder) && path.as_bytes()[folder.len()] == b'\\'
}

pub fn picture_extension(path: &Path, bytes: &[u8]) -> String {
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()).filter(|extension| !extension.is_empty() && extension.chars().all(|letter| letter.is_ascii_alphanumeric())) {
        return extension.to_ascii_lowercase();
    }
    let kind = match bytes {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [b'B', b'M', ..] => "bmp",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => "heic",
        _ => "jpg",
    };
    kind.into()
}

pub fn picked_name(path: &Path, bytes: &[u8], digest: u64) -> String {
    format!("{digest:016x}.{}", picture_extension(path, bytes))
}

pub fn startup_disabled(flag: &[u8]) -> bool {
    flag.first().is_some_and(|first| first & 1 == 1)
}

pub fn refuse(own: bool, shell: Option<bool>) -> bool {
    own && shell != Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> Cell {
        Cell { width: 78.0, icon: 48.0, line: 15.0 }
    }

    fn contains(outer: &Rect, inner: &Rect) -> bool {
        outer.x <= inner.x && outer.y <= inner.y && outer.x + outer.w >= inner.x + inner.w && outer.y + outer.h >= inner.y + inner.h
    }

    #[test]
    fn a_short_label_sits_under_the_icon_box() {
        let rect = item_rect((100.0, 200.0), &cell(), Label { width: 30.0, height: 15.0 }, &SPACING);
        assert_eq!(rect, Rect { x: 100.0 + 24.0 - 27.0, y: 203.0, w: 54.0, h: -3.0 + 48.0 + 6.0 + 15.0 + 4.0 });
    }

    #[test]
    fn a_long_word_is_capped_on_one_line() {
        let rect = item_rect((0.0, 0.0), &cell(), Label { width: 140.0, height: 15.0 }, &SPACING);
        assert_eq!((cell().wrap(&SPACING), cell().cap(&SPACING)), (74.0, 68.0));
        assert_eq!(rect.w, 68.0 + 6.0);
        assert_eq!(rect.x, 24.0 - 37.0);
        assert_eq!(rect.y + rect.h, 48.0 + 6.0 + 15.0 + 4.0);
    }

    #[test]
    fn a_wrapped_label_is_as_wide_as_its_widest_line() {
        let rect = item_rect((0.0, 0.0), &cell(), Label { width: 72.0, height: 45.0 }, &SPACING);
        assert_eq!(rect.w, 68.0 + 6.0);
        assert_eq!(rect.y + rect.h, 48.0 + 6.0 + 30.0 + 4.0);
        let narrow = item_rect((0.0, 0.0), &cell(), Label { width: 52.0, height: 30.0 }, &SPACING);
        assert_eq!(narrow.w, 58.0);
    }

    #[test]
    fn icon_sizes_follow_the_desktop_dpi() {
        assert_eq!(scaled(48.0, 96), 48.0);
        assert_eq!(scaled(48.0, 144), 72.0);
        assert_eq!(scaled(48.0, 120), 60.0);
        assert_eq!(scaled(32.0, 0), 32.0);
    }

    fn judged(rect: &Rect, drawn: &Rect, at: (f32, f32), icon: f32) -> [f32; 4] {
        let left = drawn.x.min(at.0);
        let right = (drawn.x + drawn.w).max(at.0 + icon);
        [left - rect.x, drawn.y - rect.y, rect.x + rect.w - right, rect.y + rect.h - drawn.y - drawn.h]
    }

    fn check(cell: Cell, at: (f32, f32), rows: &[(Label, Rect, f32)], limit: f32) {
        let icon_box = Rect { x: at.0, y: at.1 + 3.0, w: cell.icon, h: cell.icon };
        for (label, drawn, bounds_height) in rows {
            let rect = item_rect(at, &cell, *label, &SPACING);
            let bounds = Rect { x: at.0, y: at.1 + 3.0, w: cell.icon, h: *bounds_height };
            assert!(contains(&rect, drawn) && contains(&rect, &icon_box) && contains(&rect, &bounds), "{rect:?} {drawn:?}");
            let edges = judged(&rect, drawn, at, cell.icon);
            assert!(edges.iter().all(|edge| (0.0..=limit).contains(edge)), "{rect:?} {drawn:?} {edges:?}");
        }
    }

    #[test]
    fn the_rect_holds_what_explorer_drew_on_windows_11() {
        let rows = [
            (Label { width: 85.0, height: 15.0 }, Rect { x: 502.0, y: 263.0, w: 69.0, h: 64.0 }, 70.0),
            (Label { width: 72.0, height: 45.0 }, Rect { x: 501.0, y: 263.0, w: 71.0, h: 79.0 }, 85.0),
            (Label { width: 110.0, height: 15.0 }, Rect { x: 502.0, y: 267.0, w: 69.0, h: 60.0 }, 70.0),
            (Label { width: 11.0, height: 15.0 }, Rect { x: 519.0, y: 263.0, w: 34.0, h: 61.0 }, 70.0),
            (Label { width: 60.0, height: 15.0 }, Rect { x: 507.0, y: 263.0, w: 59.0, h: 61.0 }, 70.0),
        ];
        check(cell(), (512.0, 256.0), &rows, 10.0);
    }

    #[test]
    fn the_rect_holds_what_explorer_drew_on_server_2022() {
        let rows = [
            (Label { width: 142.0, height: 15.0 }, Rect { x: 501.0, y: 264.0, w: 70.0, h: 63.0 }, 70.0),
            (Label { width: 72.0, height: 45.0 }, Rect { x: 500.0, y: 264.0, w: 72.0, h: 78.0 }, 85.0),
            (Label { width: 110.0, height: 15.0 }, Rect { x: 501.0, y: 266.0, w: 70.0, h: 61.0 }, 70.0),
            (Label { width: 11.0, height: 15.0 }, Rect { x: 520.0, y: 264.0, w: 32.0, h: 60.0 }, 70.0),
            (Label { width: 60.0, height: 15.0 }, Rect { x: 506.0, y: 264.0, w: 60.0, h: 60.0 }, 70.0),
        ];
        check(cell(), (512.0, 256.0), &rows, 10.0);
    }

    #[test]
    fn the_rect_holds_what_explorer_drew_at_150_percent() {
        let large = Cell { width: 112.0, icon: scaled(48.0, 144), line: 25.0 };
        let rows = [
            (Label { width: 211.0, height: 25.0 }, Rect { x: 944.0, y: 369.0, w: 103.0, h: 95.0 }, 104.0),
            (Label { width: 89.0, height: 100.0 }, Rect { x: 952.0, y: 369.0, w: 87.0, h: 120.0 }, 129.0),
            (Label { width: 165.0, height: 25.0 }, Rect { x: 950.0, y: 372.0, w: 92.0, h: 92.0 }, 104.0),
            (Label { width: 16.0, height: 25.0 }, Rect { x: 972.0, y: 369.0, w: 48.0, h: 91.0 }, 104.0),
            (Label { width: 90.0, height: 25.0 }, Rect { x: 951.0, y: 369.0, w: 90.0, h: 91.0 }, 104.0),
        ];
        check(large, (960.0, 360.0), &rows, 15.0);
    }

    #[test]
    fn labels_count_at_most_two_lines() {
        assert_eq!(lines(&cell(), Label { width: 10.0, height: 60.0 }), 2.0);
        assert_eq!(lines(&cell(), Label { width: 10.0, height: 0.0 }), 1.0);
        assert_eq!(lines(&cell(), Label { width: 10.0, height: 30.0 }), 2.0);
    }

    #[test]
    fn the_rect_follows_every_constant() {
        let wide = Spacing { sides: SPACING.sides + 4.0, above: SPACING.above + 2.0, ..SPACING };
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
    fn the_themes_folder_is_found_in_any_letter_case() {
        let themes = Path::new(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Themes");
        assert!(inside(Path::new(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Themes\TranscodedWallpaper"), themes));
        assert!(inside(Path::new(r"c:\users\A\appdata\roaming\microsoft\windows\themes\CachedFiles\CachedImage_1920_1080_POS4.jpg"), themes));
        assert!(inside(Path::new(r"\\?\C:\Users\a\AppData\Roaming\Microsoft\Windows\Themes\TranscodedWallpaper"), themes));
        assert!(inside(Path::new(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Themes\x.jpg"), Path::new(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Themes\")));
        assert!(inside(Path::new("C:/Users/a/AppData/Roaming/Microsoft/Windows/Themes/x.jpg"), themes));
    }

    #[test]
    fn other_folders_are_not_the_themes_folder() {
        let themes = Path::new(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Themes");
        assert!(!inside(Path::new(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\ThemesOld\x.jpg"), themes));
        assert!(!inside(Path::new(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Themes"), themes));
        assert!(!inside(Path::new(r"C:\Users\a\AppData\Roaming\Microsoft\Windows\Themes\"), themes));
        assert!(!inside(Path::new(r"C:\Users\a\Pictures\lake.jpg"), themes));
        assert!(!inside(Path::new(r"C:\Users\a\Pictures\lake.jpg"), Path::new("")));
    }

    #[test]
    fn picked_copies_are_named_by_their_bytes() {
        assert_eq!(picked_name(Path::new(r"C:\Themes\TranscodedWallpaper"), &[0xFF, 0xD8, 0xFF, 0xE0], 0xAB), "00000000000000ab.jpg");
        assert_eq!(picked_name(Path::new(r"C:\Themes\TranscodedWallpaper"), &[0x89, b'P', b'N', b'G', 13, 10], 1), "0000000000000001.png");
        assert_eq!(picked_name(Path::new(r"C:\Themes\CachedImage.JPG"), &[0x89, b'P', b'N', b'G'], 2), "0000000000000002.jpg");
        assert_eq!(picture_extension(Path::new("a"), &[0, 0, 0, 24, b'f', b't', b'y', b'p', b'h']), "heic");
        assert_eq!(picture_extension(Path::new("a"), b"BM"), "bmp");
    }

    #[test]
    fn an_odd_first_byte_means_turned_off_in_startup_apps() {
        assert!(startup_disabled(&[3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]));
        assert!(startup_disabled(&[7]));
        assert!(!startup_disabled(&[2, 0, 0, 0]));
        assert!(!startup_disabled(&[6]));
        assert!(!startup_disabled(&[]));
    }

    #[test]
    fn administrator_runs_are_refused_unless_explorer_matches() {
        assert!(refuse(true, Some(false)));
        assert!(refuse(true, None));
        assert!(!refuse(true, Some(true)));
        assert!(!refuse(false, Some(false)));
        assert!(!refuse(false, None));
    }

    #[test]
    fn the_twin_sits_next_to_the_command() {
        assert_eq!(twin(Path::new("bin/scooch.exe")), PathBuf::from("bin/scoochw.exe"));
    }
}
