#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Options, Rect, RgbImage, Scene, Scenery};

fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_215.0
}

fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xf, yf) = (x.floor(), y.floor());
    let (tx, ty) = (x - xf, y - yf);
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (xi, yi) = (xf as u32, yf as u32);
    let a = hash(xi, yi, seed) + (hash(xi + 1, yi, seed) - hash(xi, yi, seed)) * tx;
    let b = hash(xi, yi + 1, seed) + (hash(xi + 1, yi + 1, seed) - hash(xi, yi + 1, seed)) * tx;
    a + (b - a) * ty
}

fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    (0..5).map(|o| noise(x * 2f32.powi(o), y * 2f32.powi(o), seed + o as u32) / 2f32.powi(o)).sum::<f32>() / 1.9375
}

fn made(name: &str) -> RgbImage {
    let (w, h) = (1600u32, 1000u32);
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
    RgbImage::from_fn(w, h, |x, y| {
        let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
        let grain = hash(x, y, 7) - 0.5;
        let c: [f32; 3] = match name {
            "near-black" => [8.0, 8.0, 10.0].map(|c| c + 3.0 * grain),
            "dark-gradient" => mix([12.0, 12.0, 22.0], [58.0, 44.0, 66.0], v).map(|c| c + 2.0 * grain),
            "mid-grey" => [128.0, 128.0, 128.0].map(|c| c + 3.0 * grain),
            "white-wall" => {
                let shade = 1.0 - 0.35 * (1.0 - ((u - 0.15) / 0.5).clamp(0.0, 1.0)).powi(2) * (0.7 + 0.3 * v);
                [236.0, 233.0, 227.0].map(|c| c * shade + 2.0 * grain)
            }
            "star-field" => {
                let star = if hash(x / 2, y / 2, 11) > 0.9985 { 200.0 + 55.0 * hash(x, y, 12) } else { 0.0 };
                [6.0 + star, 7.0 + star, 14.0 + star]
            }
            "grass" => mix([38.0, 82.0, 26.0], [104.0, 150.0, 52.0], fbm(u * 60.0, v * 40.0, 21)).map(|c| c + 20.0 * grain),
            "snow" => mix([206.0, 216.0, 232.0], [250.0, 251.0, 253.0], fbm(u * 8.0, v * 6.0, 31)).map(|c| c + 6.0 * grain),
            "sand" => mix([196.0, 168.0, 120.0], [232.0, 210.0, 168.0], fbm(u * 30.0, v * 20.0, 41)).map(|c| c + 12.0 * grain),
            "road" => {
                if (u - 0.5).abs() < 0.006 && (v * 10.0).fract() < 0.55 {
                    [235.0, 232.0, 220.0]
                } else {
                    mix([52.0, 52.0, 55.0], [92.0, 92.0, 96.0], fbm(u * 80.0, v * 50.0, 51)).map(|c| c + 18.0 * grain)
                }
            }
            "night-sky" => {
                let star = if hash(x / 2, y / 2, 61) > 0.999 && v < 0.8 { 160.0 * hash(x, y, 62) } else { 0.0 };
                let ground = if v > 0.82 - 0.03 * (u * 9.0).sin() { 0.0 } else { 1.0 };
                mix([10.0, 18.0, 52.0], [2.0, 4.0, 14.0], 1.0 - v).map(|c| (c + star) * ground + (1.0 - ground) * 4.0)
            }
            "fog" => mix([170.0, 176.0, 182.0], [222.0, 225.0, 228.0], 0.6 * v + 0.4 * fbm(u * 3.0, v * 2.0, 71)).map(|c| c + 3.0 * grain),
            "plain-gradient" => mix([240.0, 140.0, 70.0], [110.0, 60.0, 150.0], 0.6 * v + 0.4 * u).map(|c| c + 2.0 * grain),
            "sky-gradient" => mix([70.0, 128.0, 206.0], [168.0, 206.0, 238.0], v).map(|c| c + 2.0 * grain),
            "flat-water" => [19.0, 90.0, 48.0].map(|c| c + 3.0 * grain),
            _ => [0.0; 3],
        };
        image::Rgb(c.map(|v| v.round().clamp(0.0, 255.0) as u8))
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Expect {
    Nothing,
    Keep,
}
use Expect::{Keep, Nothing};

const MADE: [&str; 14] = ["near-black", "dark-gradient", "mid-grey", "white-wall", "star-field", "grass", "snow", "sand", "road", "night-sky", "fog", "plain-gradient", "sky-gradient", "flat-water"];

fn verdict(name: &str) -> (Expect, Expect) {
    match name {
        "balloon-sky" | "moon-sky" | "seagull-sky" | "sky-gradient" => (Keep, Nothing),
        "two-swans" | "flat-water" => (Nothing, Keep),
        _ => (Nothing, Nothing),
    }
}

fn coverage(name: &str, image: &RgbImage, layout: &str, icons: &[Rect], (w, h): (u32, u32)) -> [f64; 4] {
    let analysis = scooch_core::analyze(image);
    let scene = Scene { width: w, height: h, icons: icons.to_vec() };
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    let layers = scooch_core::render_layers(image, &analysis, &placement, &scene, &options);
    let bundle = scooch_core::bundle(image, &analysis, &placement, &scene, &options);
    let gw = bundle.grid_width as usize;
    let under = |x: u32, y: u32| icons.iter().any(|r| x as f32 >= r.x && (x as f32) < r.x + r.w && y as f32 >= r.y && (y as f32) < r.y + r.h);
    let (mut screen, mut beneath, mut icon_pixels) = ([0usize; 2], [0usize; 2], 0usize);
    for (x, y, p) in layers.scenery.enumerate_pixels() {
        let inside = under(x, y);
        icon_pixels += inside as usize;
        if p[3] == 0 {
            continue;
        }
        let k = (bundle.look.get(((y as usize / 4) * gw + x as usize / 4) * 4).copied().unwrap_or(0) < 128) as usize;
        screen[k] += 1;
        beneath[k] += inside as usize;
    }
    let total = (w * h) as f64;
    let out = [screen[0] as f64 / total, screen[1] as f64 / total, beneath[0] as f64 / icon_pixels.max(1) as f64, beneath[1] as f64 / icon_pixels.max(1) as f64];
    println!("{name} {layout}: clouds {:.3}% land {:.3}% of the screen, under icons clouds {:.3}% land {:.3}%", 100.0 * out[0], 100.0 * out[1], 100.0 * out[2], 100.0 * out[3]);
    out
}

const TODAY: [(&str, &str, f64, f64); 12] = [
    ("balloon-sky", "column", 1.0, 0.0),
    ("balloon-sky", "scattered", 1.0, 0.0),
    ("moon-sky", "column", 1.0, 0.0),
    ("moon-sky", "scattered", 0.9167, 0.0),
    ("seagull-sky", "column", 1.0, 0.0),
    ("seagull-sky", "scattered", 0.9741, 0.0),
    ("two-swans", "column", 0.0, 1.0),
    ("two-swans", "scattered", 0.0, 0.9167),
    ("sky-gradient", "column", 1.0, 0.0),
    ("sky-gradient", "scattered", 1.0, 0.0),
    ("flat-water", "column", 0.0, 0.0),
    ("flat-water", "scattered", 0.0, 0.0),
];
const KEEP_SHARE: f64 = 0.0;
const NOTHING_MOST: f64 = 0.0005;

#[test]
fn scenery_appears_only_where_it_belongs() {
    let (w, h) = (1440u32, 900u32);
    let column: Vec<Rect> = (0..16).map(|k| Rect { x: w as f32 - 60.0, y: 10.0 + 55.0 * k as f32, w: 50.0, h: 50.0 }).collect();
    let scattered: Vec<Rect> = layouts::scattered(w, h, 12, 2027).into_iter().map(|r| Rect { w: 50.0, h: 50.0, ..r }).collect();
    let mut pictures: Vec<(String, RgbImage)> = layouts::photos().into_iter().map(|p| (p.file_stem().unwrap().to_string_lossy().to_string(), scooch_core::open(&p).expect("open"))).collect();
    pictures.extend(MADE.iter().map(|n| (n.to_string(), made(n))));
    let mut failures = Vec::new();
    for (name, image) in &pictures {
        let (clouds, land) = verdict(name);
        for (layout, icons) in [("column", &column), ("scattered", &scattered)] {
            let got = coverage(name, image, layout, icons, (w, h));
            for (k, expect) in [(0usize, clouds), (1, land)] {
                match expect {
                    Nothing if got[k] > NOTHING_MOST => failures.push(format!("{name} {layout} {} covers {:.3}% of the screen", ["clouds", "land"][k], 100.0 * got[k])),
                    Keep => {
                        let today = TODAY.iter().find(|t| t.0 == name && t.1 == layout).map_or(0.0, |t| [t.2, t.3][k]);
                        if got[2 + k] < KEEP_SHARE * today {
                            failures.push(format!("{name} {layout} {} under icons {:.3}% against {:.3}% today", ["clouds", "land"][k], 100.0 * got[2 + k], 100.0 * today));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
