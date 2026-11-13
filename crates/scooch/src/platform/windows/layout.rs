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

pub const SPACING: Spacing = Spacing { inset: 10.0, above: -3.0, gap: 6.0, below: 4.0, sides: 6.0 };

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

    fn spare(outer: &Rect, inner: &Rect) -> f32 {
        [inner.x - outer.x, inner.y - outer.y, outer.x + outer.w - inner.x - inner.w, outer.y + outer.h - inner.y - inner.h].into_iter().fold(0.0, f32::max)
    }

    #[test]
    fn a_short_label_sits_under_the_icon_box() {
        let rect = item_rect((100.0, 200.0), &cell(), Label { width: 30.0, height: 15.0 }, &SPACING);
        assert_eq!(rect, Rect { x: 100.0 + 24.0 - 27.0, y: 203.0, w: 54.0, h: -3.0 + 48.0 + 6.0 + 15.0 + 4.0 });
    }

    #[test]
    fn a_wide_label_is_capped_at_the_wrap_width() {
        let rect = item_rect((0.0, 0.0), &cell(), Label { width: 140.0, height: 30.0 }, &SPACING);
        assert_eq!(cell().wrap(&SPACING), 68.0);
        assert_eq!(rect.w, 68.0 + 6.0);
        assert_eq!(rect.x, 24.0 - 37.0);
        assert_eq!(rect.y + rect.h, 48.0 + 6.0 + 30.0 + 4.0);
    }

    #[test]
    fn the_rect_holds_what_explorer_drew_on_the_runner() {
        let at = (512.0, 256.0);
        let one = item_rect(at, &cell(), Label { width: 80.0, height: 15.0 }, &SPACING);
        let two = item_rect(at, &cell(), Label { width: 68.0, height: 45.0 }, &SPACING);
        let icon_box = Rect { x: 512.0, y: 259.0, w: 48.0, h: 48.0 };
        let drawn_one = Rect { x: 502.0, y: 263.0, w: 69.0, h: 64.0 };
        let drawn_two = Rect { x: 501.0, y: 263.0, w: 71.0, h: 79.0 };
        let bounds_one = Rect { x: 512.0, y: 259.0, w: 48.0, h: 70.0 };
        let bounds_two = Rect { x: 512.0, y: 259.0, w: 48.0, h: 85.0 };
        for (rect, drawn, bounds) in [(one, drawn_one, bounds_one), (two, drawn_two, bounds_two)] {
            assert!(contains(&rect, &drawn) && contains(&rect, &icon_box) && contains(&rect, &bounds), "{rect:?}");
            assert!(spare(&rect, &drawn) < 8.0, "{rect:?} {}", spare(&rect, &drawn));
        }
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
