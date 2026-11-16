#[path = "../tests/support/desk.rs"]
mod desk;
#[path = "../tests/support/layouts.rs"]
mod layouts;

use image::{imageops, RgbImage};
use scooch_core::{Options, Rect, Scene, Scenery};
use std::path::PathBuf;

fn linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn ground(picture: &RgbImage, layer: &scooch_core::RgbaImage, r: &Rect) -> Option<f32> {
    let (mut sum, mut count) = (0.0f32, 0.0f32);
    let (x0, x1) = (r.x.max(0.0) as u32, ((r.x + r.w) as u32).min(picture.width()));
    let (y0, y1) = ((r.y + r.h * 2.0 / 3.0).max(0.0) as u32, ((r.y + r.h) as u32).min(picture.height()));
    for y in y0..y1 {
        for x in x0..x1 {
            if layer.get_pixel(x, y).0[3] < 230 {
                continue;
            }
            let p = picture.get_pixel(x, y).0;
            sum += 0.2126 * linear(p[0]) + 0.7152 * linear(p[1]) + 0.0722 * linear(p[2]);
            count += 1.0;
        }
    }
    (count > 0.0).then(|| sum / count)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = PathBuf::from(args.get(1).map_or("labels", String::as_str));
    let tag = args.get(2).map_or("now", String::as_str);
    std::fs::create_dir_all(&out).expect("out");
    let (w, h) = (960u32, 600u32);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let mut worst: [(f32, String); 2] = [(0.0, String::new()), (0.0, String::new())];
    let mut cases: Vec<(f32, String, RgbImage, Rect)> = Vec::new();
    for path in layouts::photos() {
        let full = scooch_core::open(&path).expect("photo");
        let scale = 1200.0 / full.width().max(full.height()) as f32;
        let picture = if scale < 1.0 { imageops::resize(&full, (full.width() as f32 * scale) as u32, (full.height() as f32 * scale) as u32, imageops::FilterType::Triangle) } else { full };
        let analysis = scooch_core::analyze(&picture);
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let mut session = scooch_core::Session::new();
        for layout in ["mac", "windows", "scattered", "cluster"] {
            let icons = layouts::layout(layout, &analysis, w, h);
            let scene = Scene { width: w, height: h, icons: icons.clone() };
            let options = session.options(&picture, &analysis, &scene, &options);
            let placement = scooch_core::solve(&analysis, &scene, &options, None);
            let layers = scooch_core::render_layers(&picture, &analysis, &placement, &scene, &options);
            let bundle = scooch_core::bundle(&picture, &analysis, &placement, &scene, &options);
            let mut shown = layers.picture.clone();
            scooch_core::composite(&mut shown, &layers.scenery);
            for (k, r) in icons.iter().enumerate() {
                let Some(y) = ground(&shown, &layers.scenery, r) else { continue };
                let (cx, cy) = (((r.x + r.w / 2.0) / 4.0) as u32, ((r.y + r.h * 5.0 / 6.0) / 4.0) as u32);
                let cell = (cy.min(bundle.grid_height - 1) * bundle.grid_width + cx.min(bundle.grid_width - 1)) as usize;
                let cloud = bundle.look[cell * 4] >= 128;
                let which = cloud as usize;
                let label = format!("{name} {layout} icon {k}");
                if y > worst[which].0 {
                    worst[which] = (y, label.clone());
                }
                if cloud {
                    let mut drawn = shown.clone();
                    desk::draw_icons(&mut drawn, &icons);
                    cases.push((y, label, drawn, *r));
                }
            }
        }
    }
    for (kind, (y, label)) in ["sand", "clouds"].iter().zip(&worst) {
        println!("{kind}: worst label ground luminance {y:.3} (lightness {:.3}), white text contrast {:.2} to 1, at {label}", y.cbrt(), 1.05 / (y + 0.05));
    }
    cases.sort_by(|a, b| b.0.total_cmp(&a.0));
    let named: Vec<&String> = args.iter().skip(3).collect();
    let chosen: Vec<&(f32, String, RgbImage, Rect)> = if named.is_empty() { cases.iter().take(4).collect() } else { cases.iter().filter(|c| named.iter().any(|n| c.1 == **n)).collect() };
    for (n, (y, label, drawn, r)) in chosen.into_iter().enumerate() {
        println!("crop {n}: {label} luminance {y:.3}");
        let side = (3.2 * r.h) as u32;
        let x = ((r.x + r.w / 2.0) as i64 - side as i64 / 2).clamp(0, (w - side) as i64) as u32;
        let y = ((r.y + r.h / 2.0) as i64 - side as i64 / 2).clamp(0, (h - side) as i64) as u32;
        imageops::crop_imm(drawn, x, y, side, side).to_image().save(out.join(format!("crop{n}-{}-{tag}.png", label.replace(' ', "-")))).expect("crop");
    }
}
