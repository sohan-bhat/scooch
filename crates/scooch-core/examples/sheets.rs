#[path = "../tests/support/desk.rs"]
mod desk;
#[path = "../tests/support/layouts.rs"]
mod layouts;

use image::RgbImage;
use scooch_core::{Analysis, Options, Scene, Scenery};
use std::path::PathBuf;
use std::time::Instant;

const TILE_W: u32 = 400;
const TILE_H: u32 = 250;
const SCREEN: (u32, u32) = (2880, 1800);
const LAYOUTS: [&str; 4] = ["mac", "windows", "scattered", "cluster"];

fn row(name: &str, photo: &RgbImage, analysis: &Analysis, layout: &str) -> Vec<RgbImage> {
    let (w, h) = SCREEN;
    let icons = layouts::layout(layout, analysis, w, h);
    let scene = Scene { width: w, height: h, icons: icons.clone() };
    let plain = Options { pads: 0.0, scenery: Scenery::Off, ..Options::default() };
    let off = Options { scenery: Scenery::Off, ..Options::default() };
    let on = Options::default();
    let fill = scooch_core::fill(analysis.source_width, analysis.source_height, &scene);
    let before = scooch_core::render_with(photo, analysis, &fill, &scene, &plain);
    let off_placement = scooch_core::solve(analysis, &scene, &off, None);
    let today = scooch_core::render_with(photo, analysis, &off_placement, &scene, &off);
    let started = Instant::now();
    let on_placement = scooch_core::solve(analysis, &scene, &on, None);
    let scenery = scooch_core::render_with(photo, analysis, &on_placement, &scene, &on);
    let spent = started.elapsed().as_secs_f64() * 1000.0;
    let hidden = |p| scooch_core::hidden_share(analysis, &scene, p);
    println!(
        "{name:16} {layout:9} hidden {:.3} | off {:.3} zoom {:.2} | scenery {:.3} zoom {:.2} shift [{:+.3} {:+.3}] {spent:.0} ms",
        hidden(&fill),
        hidden(&off_placement),
        off_placement.zoom,
        hidden(&on_placement),
        on_placement.zoom,
        on_placement.shift[0],
        on_placement.shift[1]
    );
    [before, today, scenery]
        .into_iter()
        .map(|mut picture| {
            desk::draw_icons(&mut picture, &icons);
            desk::fit(&picture, TILE_W, TILE_H)
        })
        .collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "sheets".into()));
    let only: Vec<String> = args.collect();
    std::fs::create_dir_all(&out).expect("out folder");
    for path in layouts::photos() {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if !only.is_empty() && !only.contains(&name) {
            continue;
        }
        let photo = scooch_core::open(&path).expect("photo");
        let analysis = scooch_core::analyze(&photo);
        let tiles: Vec<RgbImage> = LAYOUTS.iter().flat_map(|layout| row(&name, &photo, &analysis, layout)).collect();
        desk::grid(&tiles, 3, 10).save(out.join(format!("{name}.png"))).expect("save");
    }
}
