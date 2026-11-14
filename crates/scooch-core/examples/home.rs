#[path = "../tests/support/layouts.rs"]
mod layouts;

use image::{imageops, Rgb, RgbImage};
use scooch_core::{Analysis, Options, Placement, Scene, Scenery};
use std::path::PathBuf;

const SHAPES: [(u32, u32); 5] = [(1280, 960), (2880, 1800), (2560, 1080), (1080, 1920), (1600, 1600)];
const ROW: u32 = 220;

fn at(table: &[f32], u: f64) -> f64 {
    let f = u.clamp(0.0, 1.0) * (table.len() - 1) as f64;
    let k = (f as usize).min(table.len() - 2);
    table[k] as f64 + (table[k + 1] as f64 - table[k] as f64) * (f - k as f64)
}

fn inside(analysis: &Analysis, placement: &Placement) -> f64 {
    let (w, h) = (analysis.width as usize, analysis.height as usize);
    let span = |t: &[f32]| (t[0].min(t[t.len() - 1]) as f64, t[0].max(t[t.len() - 1]) as f64);
    let ((x0, x1), (y0, y1)) = (span(&placement.fx), span(&placement.fy));
    let (mut kept, mut total) = (0.0, 0.0);
    for (i, s) in analysis.saliency.iter().enumerate() {
        if *s < 0.5 {
            continue;
        }
        let (cx, cy, cw, ch) = ((i % w) as f64 / w as f64, (i / w) as f64 / h as f64, 1.0 / w as f64, 1.0 / h as f64);
        kept += *s as f64 * (((cx + cw).min(x1) - cx.max(x0)).max(0.0) / cw) * (((cy + ch).min(y1) - cy.max(y0)).max(0.0) / ch);
        total += *s as f64;
    }
    if total > 0.0 { kept / total } else { 1.0 }
}

fn tile(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene) -> RgbImage {
    let still = scooch_core::render_with(image, analysis, placement, scene, &Options { pads: 0.0, ..Options::default() });
    let tw = (ROW as f64 * scene.width as f64 / scene.height as f64).round() as u32;
    let mut out = imageops::resize(&still, tw, ROW, imageops::FilterType::Triangle);
    let (aw, ah) = (analysis.width as f64, analysis.height as f64);
    let solid = |x: u32, y: u32| {
        let (u, v) = ((x as f64 + 0.5) / tw as f64, (y as f64 + 0.5) / ROW as f64);
        let (sx, sy) = (at(&placement.fx, u), at(&placement.fy, v));
        let (cx, cy) = ((sx * aw) as i64, (sy * ah) as i64);
        cx >= 0 && cy >= 0 && (cx as f64) < aw && (cy as f64) < ah && analysis.saliency[cy as usize * analysis.width as usize + cx as usize] >= 0.5
    };
    let mask: Vec<bool> = (0..ROW).flat_map(|y| (0..tw).map(move |x| (x, y))).map(|(x, y)| solid(x, y)).collect();
    for y in 0..ROW {
        for x in 0..tw {
            let here = mask[(y * tw + x) as usize];
            let edge = here && [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)].iter().any(|&(a, b)| a >= tw || b >= ROW || !mask[(b * tw + a) as usize]);
            if edge {
                out.put_pixel(x, y, Rgb([255, 40, 40]));
            }
        }
    }
    for r in &scene.icons {
        let k = ROW as f32 / scene.height as f32;
        let (x0, y0, x1, y1) = ((r.x * k) as u32, (r.y * k) as u32, ((r.x + r.w) * k) as u32, ((r.y + r.h) * k) as u32);
        for x in x0..x1.min(tw) {
            for y in [y0, y1.saturating_sub(1)] {
                if y < ROW {
                    out.put_pixel(x, y, Rgb([255, 255, 255]));
                }
            }
        }
        for y in y0..y1.min(ROW) {
            for x in [x0, x1.saturating_sub(1)] {
                if x < tw {
                    out.put_pixel(x, y, Rgb([255, 255, 255]));
                }
            }
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, dir) = (args[0].as_str(), PathBuf::from(&args[1]));
    std::fs::create_dir_all(&dir).expect("dir");
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let photos: Vec<_> = layouts::photos().into_iter().map(|path| {
        let image = scooch_core::open(&path).expect("open");
        let analysis = scooch_core::analyze(&image);
        (path.file_stem().unwrap().to_string_lossy().to_string(), image, analysis)
    }).collect();
    let (mut changed, mut cases, mut worse) = (0, 0, 0);
    for (w, h) in SHAPES {
        let mut rows = Vec::new();
        for (name, image, analysis) in &photos {
            let scene = Scene { width: w, height: h, icons: layouts::mac(w, h, 6) };
            let file = dir.join(format!("{name}-{w}x{h}.json"));
            let now = scooch_core::solve(analysis, &scene, &options, None);
            if mode == "save" {
                std::fs::write(&file, serde_json::to_string(&now).unwrap()).expect("save");
                continue;
            }
            let was: Placement = serde_json::from_str(&std::fs::read_to_string(&file).expect("saved")).unwrap();
            let (before, after) = (inside(analysis, &was), inside(analysis, &now));
            cases += 1;
            changed += (was != now) as usize;
            worse += (after < before - 1e-9) as usize;
            println!("{name:16} {w}x{h}: solid subject inside {:5.1}% before, {:5.1}% after{}", 100.0 * before, 100.0 * after, if was != now { ", changed" } else { "" });
            rows.push((tile(image, analysis, &was, &scene), tile(image, analysis, &now, &scene)));
        }
        if mode == "save" {
            continue;
        }
        let tw = rows[0].0.width();
        let mut sheet = RgbImage::from_pixel(tw * 2 + 30, rows.len() as u32 * (ROW + 10) + 10, Rgb([24, 24, 24]));
        for (k, (a, b)) in rows.iter().enumerate() {
            let y = 10 + k as u32 * (ROW + 10);
            imageops::overlay(&mut sheet, a, 10, y as i64);
            imageops::overlay(&mut sheet, b, (tw + 20) as i64, y as i64);
        }
        sheet.save(dir.join(format!("sheet-{w}x{h}.png"))).expect("sheet");
    }
    if mode != "save" {
        println!("{changed} of {cases} cases changed, {worse} show less of the subject than before");
    }
}
