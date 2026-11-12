#![allow(dead_code)]

use scooch_core::{Analysis, Rect, Scene};

pub const SCREENS: [(u32, u32); 5] = [(2880, 1800), (1920, 1080), (3840, 2160), (5120, 2880), (1080, 1920)];
pub const NAMES: [&str; 5] = ["mac", "windows", "scattered", "cluster", "none"];

pub fn scale(width: u32, height: u32) -> f32 {
    if width.max(height) >= 2560 {
        2.0
    } else {
        1.0
    }
}

pub fn mac(width: u32, height: u32, count: usize) -> Vec<Rect> {
    let s = scale(width, height);
    let (cell_w, cell_h) = (90.0 * s, 100.0 * s);
    let top = 35.0 * s;
    let right = width as f32 - 10.0 * s;
    let per = (((height as f32 - top - 10.0 * s) / cell_h).floor() as usize).max(1);
    (0..count)
        .map(|i| {
            let (col, row) = (i / per, i % per);
            let x = right - (col + 1) as f32 * cell_w;
            let y = top + row as f32 * cell_h;
            Rect { x: x + 7.0 * s, y: y + 3.0 * s, w: 76.0 * s, h: 94.0 * s }
        })
        .collect()
}

pub fn windows(width: u32, height: u32, count: usize) -> Vec<Rect> {
    let s = scale(width, height);
    let (cell_w, cell_h) = (76.0 * s, 100.0 * s);
    let per = (((height as f32 - 48.0 * s) / cell_h).floor() as usize).max(1);
    (0..count)
        .map(|i| {
            let (col, row) = (i / per, i % per);
            Rect { x: col as f32 * cell_w + 2.0 * s, y: row as f32 * cell_h + 4.0 * s, w: 72.0 * s, h: 84.0 * s }
        })
        .collect()
}

pub fn scattered(width: u32, height: u32, count: usize, seed: u32) -> Vec<Rect> {
    let s = scale(width, height);
    let (w, h) = (76.0 * s, 94.0 * s);
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1u32 << 24) as f32
    };
    (0..count).map(|_| Rect { x: next() * (width as f32 - w), y: next() * (height as f32 - h), w, h }).collect()
}

pub fn cluster(analysis: &Analysis, width: u32, height: u32) -> Vec<Rect> {
    let s = scale(width, height);
    let scene = Scene { width, height, icons: Vec::new() };
    let fill = scooch_core::fill(analysis.source_width, analysis.source_height, &scene);
    let [x0, y0, cw, ch] = fill.crop;
    let subject = analysis.subject;
    let (sx, sy) = (subject.x + subject.w / 2.0, subject.y + subject.h / 2.0);
    let (cell_w, cell_h) = (90.0 * s, 100.0 * s);
    let cx = (((sx - x0) / cw) * width as f32).clamp(1.5 * cell_w, width as f32 - 1.5 * cell_w);
    let cy = (((sy - y0) / ch) * height as f32).clamp(1.5 * cell_h, height as f32 - 1.5 * cell_h);
    (0..9)
        .map(|i| {
            let (col, row) = ((i % 3) as f32 - 1.5, (i / 3) as f32 - 1.5);
            Rect { x: cx + col * cell_w + 7.0 * s, y: cy + row * cell_h + 3.0 * s, w: 76.0 * s, h: 94.0 * s }
        })
        .collect()
}

pub fn layout(name: &str, analysis: &Analysis, width: u32, height: u32) -> Vec<Rect> {
    match name {
        "mac" => mac(width, height, 16),
        "windows" => windows(width, height, 12),
        "scattered" => scattered(width, height, 25, 2026),
        "cluster" => cluster(analysis, width, height),
        _ => Vec::new(),
    }
}

pub fn photos() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/photos");
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "jpg" || x == "jpeg" || x == "png")).collect())
        .unwrap_or_default();
    out.sort();
    out
}
