#[path = "../tests/support/desk.rs"]
mod desk;
#[path = "../tests/support/layouts.rs"]
mod layouts;

use image::{imageops, Rgb, RgbImage};
use scooch_core::{Options, Rect, RgbaImage, Scene, Scenery, Session};
use std::path::PathBuf;
use std::time::Duration;

const STEPS: usize = 20;
const FRAMES: usize = 12;

fn gap(a: &RgbImage, b: &RgbImage) -> (u8, f64) {
    let (mut most, mut sum) = (0u8, 0u64);
    for (p, q) in a.pixels().zip(b.pixels()) {
        let d = (0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap_or(0);
        most = most.max(d);
        sum += d as u64;
    }
    (most, sum as f64 / (a.width() * a.height()) as f64)
}

fn outside(r: &Rect, x: u32, y: u32) -> f32 {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let dx = (r.x - px).max(px - (r.x + r.w)).max(0.0);
    let dy = (r.y - py).max(py - (r.y + r.h)).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

fn layer_gap(a: &RgbaImage, b: &RgbaImage, held: (&Rect, &Rect), cell: f32) -> (u8, u8, f32, f64) {
    let (mut alpha, mut color, mut farthest, mut changed) = (0u8, 0u8, 0.0f32, 0usize);
    for (i, (p, q)) in a.pixels().zip(b.pixels()).enumerate() {
        let da = p[3].abs_diff(q[3]);
        let dc = if p[3] > 0 && q[3] > 0 { (0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap_or(0) } else { 0 };
        if da > 2 || dc > 2 {
            let (x, y) = (i as u32 % a.width(), i as u32 / a.width());
            farthest = farthest.max(outside(held.0, x, y).min(outside(held.1, x, y)) / cell);
            changed += 1;
        }
        alpha = alpha.max(da);
        color = color.max(dc);
    }
    (alpha, color, farthest, 100.0 * changed as f64 / (a.width() * a.height()) as f64)
}

fn whole(before: &[bool], after: &[bool], w: usize, h: usize) -> (usize, usize) {
    let mut seen = vec![false; after.len()];
    let (mut count, mut largest) = (0usize, 0usize);
    for start in 0..after.len() {
        if !after[start] || seen[start] {
            continue;
        }
        let (mut stack, mut size, mut touches) = (vec![start], 0usize, false);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            size += 1;
            touches |= before[i];
            let (x, y) = (i % w, i / w);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                        let n = ny as usize * w + nx as usize;
                        if after[n] && !seen[n] {
                            seen[n] = true;
                            stack.push(n);
                        }
                    }
                }
            }
        }
        if !touches {
            count += 1;
            largest = largest.max(size);
        }
    }
    (count, largest)
}

fn crop(image: &RgbImage, around: &Rect, side: u32) -> RgbImage {
    let (w, h) = image.dimensions();
    let x = ((around.x + around.w / 2.0) as i64 - side as i64 / 2).clamp(0, (w - side.min(w)) as i64) as u32;
    let y = ((around.y + around.h / 2.0) as i64 - side as i64 * 5 / 16).clamp(0, (h - (side * 5 / 8).min(h)) as i64) as u32;
    imageops::crop_imm(image, x, y, side.min(w), (side * 5 / 8).min(h)).to_image()
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "drag".into()));
    std::fs::create_dir_all(&out).expect("out");
    let (w, h) = (2880u32, 1800u32);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let column: Vec<Rect> = (0..16).map(|k| Rect { x: w as f32 - 120.0, y: 20.0 + 110.0 * k as f32, w: 100.0, h: 100.0 }).collect();
    let mut clouds = column.clone();
    clouds.push(Rect { x: 2300.0, y: 500.0, w: 100.0, h: 100.0 });
    let islands = layouts::scattered(w, h, 12, 2027).into_iter().map(|r| Rect { w: 100.0, h: 100.0, ..r }).collect::<Vec<Rect>>();
    let cases = [("clouds", "balloon-sky", clouds, 16usize, (14.0f32, 9.0f32)), ("islands", "two-swans", islands, 3usize, (-12.0, 10.0))];
    for (label, photo_name, icons, held, (dx, dy)) in cases {
        let path = layouts::photos().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == photo_name)).expect("photo");
        let image = scooch_core::open(&path).expect("open");
        let analysis = scooch_core::analyze(&image);
        let mut scene = Scene { width: w, height: h, icons: icons.clone() };
        let mut placement = scooch_core::solve(&analysis, &scene, &options, None);
        let mut session = Session::new();
        let (first_bundle, first_still) = session.still_shared(&image, &analysis, &placement, &scene, &options);
        let first_layers = scooch_core::frame_layers(&first_bundle, &first_bundle, 0.0);
        let (mut last_still, mut last_layer) = ((*first_still).clone(), first_layers.scenery);
        let cell = 110.0;
        let mut worst = (0u8, 0u8, 0.0f32, 0.0f64, 0u8);
        let mut shifted = 0;
        let start = scene.icons[held];
        for step in 1..=STEPS {
            let before = scene.icons[held];
            scene.icons[held].x += dx;
            scene.icons[held].y += dy;
            let next = scooch_core::solve_near(&analysis, &scene, &options, &placement, Duration::from_millis(8));
            let moved = next != placement;
            placement = next;
            let (bundle, still) = session.still_shared(&image, &analysis, &placement, &scene, &options);
            let layers = scooch_core::frame_layers(&bundle, &bundle, 0.0);
            let (alpha, color, farthest, share) = layer_gap(&last_layer, &layers.scenery, (&before, &scene.icons[held]), cell);
            let (most, _) = gap(&last_still, &still);
            println!("{label} step {step:2}: scenery alpha {alpha:3} color {color:3} farthest {farthest:.2} cells, changed {share:.2}% | picture largest {most}{}", if moved { " | the wallpaper moved" } else { "" });
            if moved {
                shifted += 1;
            } else {
                worst = (worst.0.max(alpha), worst.1.max(color), worst.2.max(farthest), worst.3.max(share), worst.4.max(most));
            }
            last_still = (*still).clone();
            last_layer = layers.scenery;
        }
        println!("{label} walk worst over the {} steps that kept the wallpaper still: scenery alpha {} color {} farthest {:.2} cells changed {:.2}% picture largest {}", STEPS - shifted, worst.0, worst.1, worst.2, worst.3, worst.4);
        let end = scene.icons[held];
        let (last_bundle, _) = session.still_shared(&image, &analysis, &placement, &scene, &options);
        let (mut first_bundle, mut last_bundle) = ((*first_bundle).clone(), (*last_bundle).clone());
        if let Some(keep) = std::env::var("KEEP").ok().and_then(|v| v.parse::<f32>().ok()) {
            first_bundle.constants.shadow[0] = keep;
            last_bundle.constants.shadow[0] = keep;
        }
        if let Some(gate) = std::env::var("GATE").ok().and_then(|v| v.parse::<f32>().ok()) {
            first_bundle.constants.shadow[1] = gate;
            last_bundle.constants.shadow[1] = gate;
        }
        let middle = Rect { x: (start.x + end.x) / 2.0, y: (start.y + end.y) / 2.0, w: start.w, h: start.h };
        for (mode, dissolve) in [("kind", false), ("dissolve", true)] {
            let mut previous: Option<(RgbImage, Vec<bool>)> = None;
            let (mut most, mut share, mut appeared, mut vanished, mut biggest) = (0u8, 0.0f64, 0usize, 0usize, 0usize);
            let mut tiles = Vec::new();
            for k in 0..FRAMES {
                let s = k as f32 / (FRAMES - 1) as f32;
                let layers = if dissolve { scooch_core::frame_dissolve_layers(&first_bundle, &last_bundle, s) } else { scooch_core::frame_layers(&first_bundle, &last_bundle, s) };
                let mask: Vec<bool> = layers.scenery.pixels().map(|p| p[3] >= 128).collect();
                let mut frame = layers.picture.clone();
                scooch_core::composite(&mut frame, &layers.scenery);
                if let Some((p, before)) = &previous {
                    let (m, _) = gap(p, &frame);
                    most = most.max(m);
                    let far = p.pixels().zip(frame.pixels()).filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 32)).count();
                    share = share.max(100.0 * far as f64 / (w * h) as f64);
                    let (count, size) = whole(before, &mask, w as usize, h as usize);
                    appeared += count;
                    biggest = biggest.max(size);
                    let (count, size) = whole(&mask, before, w as usize, h as usize);
                    vanished += count;
                    biggest = biggest.max(size);
                }
                previous = Some((frame.clone(), mask));
                let shown = Rect { x: start.x + (end.x - start.x) * s, y: start.y + (end.y - start.y) * s, ..start };
                let mut icons_now = scene.icons.clone();
                icons_now[held] = shown;
                desk::draw_icons(&mut frame, &icons_now);
                tiles.push(imageops::resize(&crop(&frame, &middle, 960), 480, 300, imageops::FilterType::Triangle));
            }
            println!("{label} {mode} glide: pixels changed by more than 32 between neighbouring frames at most {share:.2}%, pieces appearing whole {appeared}, vanishing whole {vanished}, largest such piece {biggest} px, largest single pixel change {most}");
            let mut sheet = RgbImage::from_pixel(480 * 6 + 5 * 6, 300 * 2 + 6, Rgb([0, 0, 0]));
            for (k, tile) in tiles.iter().enumerate() {
                imageops::replace(&mut sheet, tile, ((k % 6) * 486) as i64, ((k / 6) * 306) as i64);
            }
            sheet.save(out.join(format!("{label}-{mode}-strip.png"))).expect("strip");
        }
    }
}
