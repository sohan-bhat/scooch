#[path = "../tests/support/layouts.rs"]
mod layouts;

use image::{imageops, Rgb, RgbImage};
use scooch_core::{Options, Rect, Scene, Scenery};
use std::path::PathBuf;

fn marks(still: &mut RgbImage, icons: &[Rect]) {
    let (w, h) = still.dimensions();
    for r in icons {
        let s = r.w.min(r.h);
        let (ix, iy, isz) = (r.x + 0.2 * s, r.y + 0.08 * s, 0.6 * s);
        for y in iy as u32..(iy + isz) as u32 {
            for x in ix as u32..(ix + isz) as u32 {
                if x < w && y < h {
                    let p = still.get_pixel_mut(x, y);
                    *p = Rgb([p[0] / 4 + 180, p[1] / 4 + 180, p[2] / 4 + 186]);
                }
            }
        }
        let (lx, ly, lw, lh) = (r.x + 0.12 * s, r.y + 0.76 * s, 0.76 * s, (0.08 * s).max(3.0));
        for (dx, dy, v) in [(2.0, 2.0, 0u8), (0.0, 0.0, 255u8)] {
            for y in (ly + dy) as u32..(ly + dy + lh) as u32 {
                for x in (lx + dx) as u32..(lx + dx + lw) as u32 {
                    if x < w && y < h {
                        let p = still.get_pixel_mut(x, y);
                        *p = if v == 0 { Rgb([p[0] / 2, p[1] / 2, p[2] / 2]) } else { Rgb([255, 255, 255]) };
                    }
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let study = PathBuf::from(args.get(1).expect("usage: beside <study folder> <out folder> [scene]"));
    let out = PathBuf::from(args.get(2).expect("usage: beside <study folder> <out folder> [scene]"));
    std::fs::create_dir_all(&out).expect("out");
    let (w, h) = (2880u32, 1800u32);
    let column: Vec<Rect> = (0..16).map(|k| Rect { x: w as f32 - 120.0, y: 20.0 + 110.0 * k as f32, w: 100.0, h: 100.0 }).collect();
    let mut state = 2027u32;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1u32 << 24) as f32
    };
    let scattered: Vec<Rect> = (0..12).map(|_| Rect { x: 60.0 + next() * (w as f32 - 220.0), y: 60.0 + next() * (h as f32 - 220.0), w: 100.0, h: 100.0 }).collect();
    let site: Vec<Rect> = [(980.0, 90.0), (150.0, 710.0), (130.0, 540.0), (280.0, 600.0), (1030.0, 250.0), (1110.0, 140.0)].iter().map(|(x, y)| Rect { x: *x, y: *y, w: 150.0, h: 150.0 }).collect();
    let scenes = [
        ("balloon-scattered", "balloon-sky", w, h, &scattered),
        ("balloon-column", "balloon-sky", w, h, &column),
        ("moon-column", "moon-sky", w, h, &column),
        ("swans-column", "two-swans", w, h, &column),
        ("swans-scattered", "two-swans", w, h, &scattered),
        ("main-a", "balloon-sky", 1280, 960, &site),
    ];
    let only = args.get(3);
    for (name, photo, sw, sh, icons) in scenes {
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let path = layouts::photos().into_iter().find(|p| p.file_stem().unwrap() == photo).expect("photo");
        let image = scooch_core::open(&path).expect("open");
        let analysis = scooch_core::analyze(&image);
        let scene = Scene { width: sw, height: sh, icons: icons.clone() };
        let options = Options { scenery: Scenery::Auto, pads: 0.0, ..Options::default() };
        let placement = scooch_core::solve(&analysis, &scene, &options, None);
        let mut product = scooch_core::render_with(&image, &analysis, &placement, &scene, &options);
        marks(&mut product, icons);
        product.save(out.join(format!("{name}-product.png"))).expect("product");
        let reference = image::open(study.join(format!("{name}.png"))).expect("study still").to_rgb8();
        let (mut sum, mut largest, mut far) = (0u64, 0u8, 0usize);
        let mut gaps: Vec<u8> = Vec::with_capacity((sw * sh) as usize);
        for (p, q) in product.pixels().zip(reference.pixels()) {
            let d = (0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap_or(0);
            sum += d as u64;
            largest = largest.max(d);
            far += (d > 32) as usize;
            gaps.push(d);
        }
        gaps.sort_unstable();
        let pixels = gaps.len().max(1);
        println!(
            "{name}: mean {:.2}, p99 {}, p99.9 {}, largest {}, over 32 {:.2}%",
            sum as f64 / pixels as f64,
            gaps[pixels * 99 / 100],
            gaps[pixels * 999 / 1000],
            largest,
            100.0 * far as f64 / pixels as f64
        );
        let half = 1200u32;
        let scale = half as f32 / sw as f32;
        let hh = (sh as f32 * scale) as u32;
        let mut sheet = RgbImage::from_pixel(half * 2 + 12, hh, Rgb([0, 0, 0]));
        imageops::replace(&mut sheet, &imageops::resize(&reference, half, hh, imageops::FilterType::Triangle), 0, 0);
        imageops::replace(&mut sheet, &imageops::resize(&product, half, hh, imageops::FilterType::Triangle), half as i64 + 12, 0);
        sheet.save(out.join(format!("{name}-beside.png"))).expect("sheet");
    }
}
