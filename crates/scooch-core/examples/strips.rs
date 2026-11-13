#[path = "../tests/support/desk.rs"]
mod desk;
#[path = "../tests/support/layouts.rs"]
mod layouts;

use scooch_core::{Options, Rect, Scene, Scenery};
use std::path::PathBuf;

const SCREEN: (u32, u32) = (1280, 800);
const FRAMES: usize = 9;

fn icon(x: f32, y: f32) -> Rect {
    Rect { x, y, w: 76.0, h: 94.0 }
}

fn pair(name: &str) -> (&'static str, Vec<Rect>, Vec<Rect>) {
    match name {
        "cross" => {
            let fixed = vec![icon(980.0, 120.0), icon(1060.0, 120.0), icon(150.0, 600.0)];
            let mut a = fixed.clone();
            a.push(icon(260.0, 330.0));
            let mut b = fixed;
            b.push(icon(700.0, 360.0));
            ("two-swans", a, b)
        }
        "split" => {
            let a = vec![icon(300.0, 380.0), icon(380.0, 380.0), icon(460.0, 380.0), icon(1100.0, 640.0)];
            let mut b = a.clone();
            b[2] = icon(700.0, 420.0);
            ("two-swans", a, b)
        }
        _ => {
            let column: Vec<Rect> = (0..6).map(|k| icon(1194.0, 20.0 + 100.0 * k as f32)).collect();
            let mut a = column.clone();
            a.push(icon(760.0, 160.0));
            let mut b = column;
            b.push(icon(1104.0, 120.0));
            ("balloon-sky", a, b)
        }
    }
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "bundle".into()));
    std::fs::create_dir_all(&out).expect("out folder");
    let (w, h) = SCREEN;
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let mut tiles = Vec::new();
    for name in ["cross", "split", "join"] {
        let (photo_name, first, second) = pair(name);
        let path = layouts::photos().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == photo_name)).expect("photo");
        let photo = scooch_core::open(&path).expect("open");
        let analysis = scooch_core::analyze(&photo);
        let (sa, sb) = (Scene { width: w, height: h, icons: first }, Scene { width: w, height: h, icons: second.clone() });
        let pa = scooch_core::solve(&analysis, &sa, &options, None);
        let pb = scooch_core::solve(&analysis, &sb, &options, Some(&pa));
        let (a, b) = (scooch_core::bundle(&photo, &analysis, &pa, &sa, &options), scooch_core::bundle(&photo, &analysis, &pb, &sb, &options));
        println!("{name}: {photo_name} compatible {} moved picture {}", scooch_core::compatible(&a, &b), pa != pb);
        for k in 0..FRAMES {
            let s = k as f32 / (FRAMES - 1) as f32;
            let mut picture = scooch_core::frame(&a, &b, s);
            desk::draw_icons(&mut picture, &second);
            tiles.push(desk::fit(&picture, 640, 400));
        }
    }
    desk::grid(&tiles, FRAMES, 8).save(out.join("strips.png")).expect("save strips");
}
