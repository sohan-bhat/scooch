#[path = "../tests/support/desk.rs"]
mod desk;
#[path = "../tests/support/layouts.rs"]
mod layouts;

use image::{imageops, RgbImage};
use scooch_core::{Options, Rect, Scene, Scenery};
use std::path::PathBuf;

const SCREEN: (u32, u32) = (2880, 1800);
const PICKS: [(&str, &str); 12] = [
    ("moon-sky", "mac"),
    ("seagull-sky", "mac"),
    ("beach-sitter", "mac"),
    ("fox", "mac"),
    ("golden-tree", "mac"),
    ("city-night", "mac"),
    ("balloon-sky", "scattered"),
    ("two-swans", "scattered"),
    ("dragonfly", "scattered"),
    ("frosty-tree", "scattered"),
    ("great-wave", "scattered"),
    ("tree-field", "scattered"),
];

const UNTOUCHED: [(&str, &str); 4] = [("bamboo", "scattered"), ("crown-portrait", "scattered"), ("city-night", "scattered"), ("glass-cubes", "mac")];

enum Spot {
    Edge(usize),
    Loner { top: bool },
}

const CLOSE: [(&str, &str, Spot); 4] = [
    ("beach-sitter", "mac", Spot::Edge(4)),
    ("two-swans", "scattered", Spot::Loner { top: false }),
    ("seagull-sky", "mac", Spot::Edge(1)),
    ("balloon-sky", "scattered", Spot::Loner { top: true }),
];

fn shot(name: &str, layout: &str) -> (RgbImage, Vec<Rect>) {
    let path = layouts::photos().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == name)).expect("photo");
    let photo = scooch_core::open(&path).expect("open");
    let analysis = scooch_core::analyze(&photo);
    let (w, h) = SCREEN;
    let icons = layouts::layout(layout, &analysis, w, h);
    let scene = Scene { width: w, height: h, icons: icons.clone() };
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    let mut picture = scooch_core::render_with(&photo, &analysis, &placement, &scene, &options);
    desk::draw_icons(&mut picture, &icons);
    (picture, icons)
}

fn center(r: &Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn loner(icons: &[Rect], top: bool) -> usize {
    let gap = |i: usize| {
        let (x, y) = center(&icons[i]);
        icons
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, r)| {
                let (u, v) = center(r);
                (u - x).hypot(v - y)
            })
            .fold(f32::MAX, f32::min)
    };
    let edge = |i: usize| {
        let r = &icons[i];
        r.x.min(r.y).min(SCREEN.0 as f32 - r.x - r.w).min(SCREEN.1 as f32 - r.y - r.h)
    };
    (0..icons.len())
        .filter(|&i| edge(i) > 220.0)
        .filter(|&i| !top || center(&icons[i]).1 < SCREEN.1 as f32 * 0.45)
        .max_by(|&a, &b| gap(a).total_cmp(&gap(b)))
        .unwrap_or(0)
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "scenery".into()));
    std::fs::create_dir_all(&out).expect("out folder");
    let mut tiles = Vec::new();
    for (name, layout) in PICKS {
        let (picture, _) = shot(name, layout);
        tiles.push(desk::fit(&picture, 960, 600));
        println!("gallery {name} {layout}");
    }
    desk::grid(&tiles, 3, 12).save(out.join("gallery.png")).expect("save gallery");
    let mut none = Vec::new();
    for (name, layout) in UNTOUCHED {
        let (picture, _) = shot(name, layout);
        none.push(desk::fit(&picture, 960, 600));
        println!("untouched {name} {layout}");
    }
    desk::grid(&none, 2, 12).save(out.join("untouched.png")).expect("save untouched");
    let mut crops = Vec::new();
    for (name, layout, spot) in CLOSE {
        let (picture, icons) = shot(name, layout);
        let index = match spot {
            Spot::Edge(i) => i.min(icons.len() - 1),
            Spot::Loner { top } => loner(&icons, top),
        };
        let (cx, cy) = center(&icons[index]);
        let x = (cx as i64 - 450).clamp(0, picture.width() as i64 - 900) as u32;
        let y = (cy as i64 - 300).clamp(0, picture.height() as i64 - 600) as u32;
        crops.push(imageops::crop_imm(&picture, x, y, 900, 600).to_image());
    }
    desk::grid(&crops, 2, 12).save(out.join("close.png")).expect("save close");
}
