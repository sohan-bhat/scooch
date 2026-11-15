#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Options, Placement, Rect, RgbImage, Scene, Scenery, TABLE};

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
            "flat-water" => scooch_core::ISLAND_WATER.map(|c| c as f32 + 3.0 * grain),
            "lagoon" => {
                let float = ((u - 0.3) / 0.05).powi(2) + ((v - 0.55) / 0.08).powi(2) < 1.0;
                if float { [230.0, 120.0, 60.0] } else { mix([40.0, 170.0, 170.0], [30.0, 150.0, 160.0], fbm(u * 5.0, v * 3.0, 105)) }.map(|c| c + 2.0 * grain)
            }
            "deep-sea" => if v < 0.3 { mix([200.0, 208.0, 214.0], [218.0, 222.0, 226.0], v / 0.3) } else { mix([0.0, 72.0, 104.0], [0.0, 84.0, 120.0], fbm(u * 6.0, v * 4.0, 101)) }.map(|c| c + 3.0 * grain),
            "pool" => {
                let deck = v < 0.1 || v > 0.9;
                if deck { [200.0, 195.0, 185.0] } else { mix([55.0, 160.0, 200.0], [65.0, 170.0, 210.0], fbm(u * 8.0, v * 5.0, 103)) }.map(|c| c + 2.0 * grain)
            }
            "conifer" => mix([16.0, 58.0, 38.0], [42.0, 108.0, 74.0], fbm(u * 90.0, v * 30.0, 23)).map(|c| c + 14.0 * grain),
            "teal-green-wall" => [30.0, 90.0, 60.0].map(|c| c + 3.0 * grain),
            "light-teal-wall" => [140.0, 210.0, 210.0].map(|c| c + 2.0 * grain),
            "lawn-shade" => mix([20.0, 48.0, 22.0], [55.0, 92.0, 40.0], fbm(u * 60.0, v * 40.0, 21)).map(|c| c + 16.0 * grain),
            "blurred-foliage" | "blurred-teal" => {
                let subject = ((u - 0.5) / 0.16).powi(2) + ((v - 0.55) / 0.3).powi(2) < 1.0;
                let (a, b) = if name == "blurred-foliage" { ([40.0, 100.0, 50.0], [100.0, 155.0, 80.0]) } else { ([25.0, 105.0, 85.0], [75.0, 150.0, 105.0]) };
                if subject { [170.0, 90.0, 40.0] } else { mix(a, b, fbm(u * 3.0, v * 2.0, 25)) }.map(|c| c + 2.0 * grain)
            }
            "deep-sky" => mix([20.0, 45.0, 110.0], [85.0, 145.0, 220.0], v).map(|c| c + 2.0 * grain),
            "dark-blue" => [25.0, 45.0, 105.0].map(|c| c + 2.0 * grain),
            "thin-sky" | "sky-band" => {
                let band = if name == "thin-sky" { 0.139 } else { 0.184 };
                if v < band { [100.0, 160.0, 225.0] } else { mix([52.0, 62.0, 44.0], [72.0, 80.0, 58.0], fbm(u * 4.0, v * 3.0, 91)) }.map(|c| c + 2.0 * grain)
            }
            "sea-under-sky" => {
                let sky = mix([91.0, 151.0, 211.0], [106.0, 164.0, 218.0], (v / 0.3).min(1.0));
                let a = mix(sky, [138.0, 148.0, 155.0], ((v - 0.3) / 0.12).clamp(0.0, 1.0));
                mix(a, [19.0, 83.0, 114.0], ((v - 0.5) / 0.12).clamp(0.0, 1.0)).map(|c| c + 2.0 * grain)
            }
            "lake-under-haze" => {
                let sky = mix([91.0, 151.0, 211.0], [106.0, 164.0, 218.0], (v / 0.35).min(1.0));
                let a = mix(sky, [207.0, 217.0, 225.0], ((v - 0.35) / 0.05).clamp(0.0, 1.0));
                mix(a, [61.0, 106.0, 127.0], ((v - 0.55) / 0.2).clamp(0.0, 1.0)).map(|c| c + 2.0 * grain)
            }
            "wall-window" => mix([55.0, 85.0, 140.0], [100.0, 160.0, 225.0], ((u - 0.45) / 0.1).clamp(0.0, 1.0)).map(|c| c + 2.0 * grain),
            _ => [0.0; 3],
        };
        image::Rgb(c.map(|v| v.round().clamp(0.0, 255.0) as u8))
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Expect {
    Nothing,
    Keep,
    Pin,
}
use Expect::{Keep, Nothing, Pin};

const MADE: [&str; 26] = [
    "deep-sky", "dark-blue", "near-black", "dark-gradient", "mid-grey", "white-wall", "star-field", "grass", "snow", "sand", "road", "night-sky", "fog", "plain-gradient", "sky-gradient", "flat-water", "lagoon", "deep-sea", "pool", "conifer",
    "teal-green-wall", "light-teal-wall", "lawn-shade", "blurred-foliage", "blurred-teal", "aerial-teal",
];

fn verdict(name: &str) -> (Expect, Expect) {
    match name {
        "balloon-sky" | "deep-sky" => (Keep, Nothing),
        "moon-sky" | "sky-gradient" => (Pin, Nothing),
        "lone-canoe" | "flat-water" | "lagoon" | "deep-sea" | "pool" => (Nothing, Keep),
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

const TODAY: [(&str, &str, f64, f64); 14] = [
    ("balloon-sky", "column", 1.0, 0.0),
    ("balloon-sky", "scattered", 1.0, 0.0),
    ("lone-canoe", "column", 0.0, 0.7016),
    ("lone-canoe", "scattered", 0.0, 0.624),
    ("flat-water", "column", 0.0, 1.0),
    ("flat-water", "scattered", 0.0, 1.0),
    ("lagoon", "column", 0.0, 1.0),
    ("lagoon", "scattered", 0.0, 1.0),
    ("deep-sea", "column", 0.0, 0.5773),
    ("deep-sea", "scattered", 0.0, 0.3333),
    ("pool", "column", 0.0, 0.6848),
    ("pool", "scattered", 0.0, 0.4578),
    ("deep-sky", "column", 1.0, 0.0),
    ("deep-sky", "scattered", 1.0, 0.0),
];
const PINNED: [(&str, f64); 2] = [("moon-sky", 0.6264), ("sky-gradient", 0.6)];
const PIN_GRID: (usize, usize) = (8, 5);
const KEEP_SHARE: f64 = 0.95;
const PIN_BAND: f64 = 0.5;
const NOTHING_MOST: f64 = 0.0005;

#[test]
fn scenery_appears_only_where_it_belongs() {
    let (w, h) = (1440u32, 900u32);
    let column: Vec<Rect> = (0..16).map(|k| Rect { x: w as f32 - 60.0, y: 10.0 + 55.0 * k as f32, w: 50.0, h: 50.0 }).collect();
    let scattered: Vec<Rect> = layouts::scattered(w, h, 12, 2027).into_iter().map(|r| Rect { w: 50.0, h: 50.0, ..r }).collect();
    let pins = grid(w, h, PIN_GRID.0, PIN_GRID.1);
    let icons = (PIN_GRID.0 * PIN_GRID.1) as f64;
    let mut pictures: Vec<(String, RgbImage)> = layouts::photos().into_iter().map(|p| (p.file_stem().unwrap().to_string_lossy().to_string(), scooch_core::open(&p).expect("open"))).collect();
    pictures.extend(MADE.iter().map(|n| (n.to_string(), made(n))));
    let mut failures = Vec::new();
    for (name, image) in &pictures {
        let (clouds, land) = verdict(name);
        for (layout, icons) in [("column", &column), ("scattered", &scattered)] {
            let got = coverage(name, image, layout, icons, (w, h));
            for (k, expect) in [(0usize, clouds), (1, land)] {
                match expect {
                    Pin if got[k] <= NOTHING_MOST => failures.push(format!("{name} {layout} {} lost all of it", ["clouds", "land"][k])),
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
        for (k, expect) in [(0usize, clouds), (1, land)] {
            if expect == Pin {
                let got = coverage(name, image, "pins", &pins, (w, h));
                let pinned = PINNED.iter().find(|t| t.0 == name).map_or(0.0, |t| t.1);
                if (got[2 + k] - pinned).abs() * icons > PIN_BAND {
                    failures.push(format!("{name} {} under {icons} icons {:.4} against the pinned {pinned:.4}, more than half an icon off", ["clouds", "land"][k], got[2 + k]));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn line(from: f64, to: f64) -> Vec<f32> {
    (0..TABLE).map(|k| (from + (to - from) * k as f64 / (TABLE - 1) as f64) as f32).collect()
}

fn framing(image: &RgbImage, (w, h): (u32, u32), zoom: f64, at: (f64, f64)) -> Placement {
    let (picture, screen) = (image.width() as f64 / image.height() as f64, w as f64 / h as f64);
    let (cw, ch) = if picture >= screen { (screen / picture / zoom, 1.0 / zoom) } else { (1.0 / zoom, picture / screen / zoom) };
    let (x0, y0) = ((1.0 - cw) * at.0, (1.0 - ch) * at.1);
    Placement { fx: line(x0, x0 + cw), fy: line(y0, y0 + ch), zoom: zoom as f32, mirrored: false, crop: [x0 as f32, y0 as f32, cw as f32, ch as f32], shift: [0.0, 0.0] }
}

const FRAMINGS: [(f64, (f64, f64)); 6] = [(1.0, (0.5, 0.5)), (1.0, (0.0, 0.0)), (1.15, (0.3, 0.7)), (1.22, (0.5, 0.5)), (1.3, (0.0, 1.0)), (1.3, (1.0, 0.0))];

fn column(w: u32) -> Vec<Rect> {
    (0..16).map(|k| Rect { x: w as f32 - 60.0, y: 10.0 + 55.0 * k as f32, w: 50.0, h: 50.0 }).collect()
}

fn grid(w: u32, h: u32, across: usize, down: usize) -> Vec<Rect> {
    (0..across * down).map(|k| Rect { x: ((k % across) as f32 + 0.5) * w as f32 / across as f32 - 25.0, y: ((k / across) as f32 + 0.5) * h as f32 / down as f32 - 25.0, w: 50.0, h: 50.0 }).collect()
}

fn scenery_pictures() -> Vec<(String, RgbImage)> {
    let mut out: Vec<(String, RgbImage)> = layouts::photos()
        .into_iter()
        .map(|p| (p.file_stem().unwrap().to_string_lossy().to_string(), p))
        .filter(|(n, _)| ["balloon-sky", "moon-sky", "lone-canoe"].contains(&n.as_str()))
        .map(|(n, p)| (n, scooch_core::open(&p).expect("open")))
        .collect();
    out.extend(["deep-sky", "sky-gradient", "flat-water", "lagoon"].iter().map(|n| (n.to_string(), made(n))));
    out
}

fn screen_matches_the_picture_map(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options, map: &[[f32; 2]], (mw, mh): (u32, u32)) -> u32 {
    let (cells, gw, gh) = scooch_core::regions(image, analysis, placement, scene, options);
    let bundle = scooch_core::bundle(image, analysis, placement, scene, options);
    let scale = (scene.width as f64 / image.width() as f64).max(scene.height as f64 / image.height() as f64);
    let size = [image.width() as f64 * scale, image.height() as f64 * scale];
    assert_eq!((mw, mh), ((size[0].ceil() as u32).div_ceil(4), (size[1].ceil() as u32).div_ceil(4)));
    let center = |m: &[f64], k: usize| (m[(4 * k + 1).min(m.len() - 1)] + m[(4 * k + 2).min(m.len() - 1)]) * 0.5;
    let at = |k: usize, fx: f64, fy: f64| {
        let gx = ((fx * size[0] / 4.0 - 0.5) as f32).clamp(0.0, (mw - 1) as f32);
        let gy = ((fy * size[1] / 4.0 - 0.5) as f32).clamp(0.0, (mh - 1) as f32);
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(mw as usize - 1), (y0 + 1).min(mh as usize - 1));
        let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
        let v = |x: usize, y: usize| map[y * mw as usize + x][k];
        let top = v(x0, y0) + (v(x1, y0) - v(x0, y0)) * tx;
        let bottom = v(x0, y1) + (v(x1, y1) - v(x0, y1)) * tx;
        top + (bottom - top) * ty
    };
    let mut worst = 0u32;
    for j in 0..gh as usize {
        for i in 0..gw as usize {
            let (fx, fy) = (center(&bundle.xmap, i), center(&bundle.ymap, j));
            for (k, got) in cells[j * gw as usize + i].iter().take(2).enumerate() {
                let want = (at(k, fx, fy).clamp(0.0, 1.0) * 255.0).round() as i32;
                worst = worst.max((*got as i32 - want).unsigned_abs());
            }
        }
    }
    worst
}

#[test]
fn belonging_stays_on_the_picture_through_pans_and_zooms() {
    let (w, h) = (1440u32, 900u32);
    let scene = Scene { width: w, height: h, icons: column(w) };
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    for (name, image) in scenery_pictures() {
        let analysis = scooch_core::analyze(&image);
        let mut first: Option<(Vec<[f32; 2]>, u32, u32)> = None;
        for (zoom, at) in FRAMINGS {
            let placement = framing(&image, (w, h), zoom, at);
            let map = scooch_core::belonging_map(&image, &analysis, &placement, &scene, &options);
            assert!(map.0.iter().any(|v| v[1] > 0.5), "{name} has no scenery to keep");
            let worst = screen_matches_the_picture_map(&image, &analysis, &placement, &scene, &options, &map.0, (map.1, map.2));
            println!("{name} zoom {zoom} at {at:?}: screen off the picture map by at most {worst}/255");
            assert!(worst <= 1, "{name} zoom {zoom} at {at:?} the screen map is not the picture map, off by {worst}");
            match &first {
                None => first = Some(map),
                Some(f) => assert!(*f == map, "{name} zoom {zoom} at {at:?} the belonging map moved"),
            }
        }
    }
}

#[test]
fn a_sky_too_small_to_seed_gets_nothing_at_any_zoom() {
    let (w, h) = (1440u32, 900u32);
    let scene = Scene { width: w, height: h, icons: grid(w, h, 8, 5) };
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    for (name, seeds) in [("thin-sky", false), ("sky-band", true)] {
        let image = made(name);
        let analysis = scooch_core::analyze(&image);
        for zoom in [1.0, 1.1, 1.2, 1.3] {
            let placement = framing(&image, (w, h), zoom, (0.5, 0.0));
            let (cells, _, _) = scooch_core::regions(&image, &analysis, &placement, &scene, &options);
            let most = cells.iter().map(|c| c[1]).max().unwrap_or(0);
            println!("{name} zoom {zoom}: most belonging {most}/255");
            assert_eq!(most > 128, seeds, "{name} zoom {zoom} most belonging {most}");
            if !seeds {
                assert_eq!(most, 0, "{name} zoom {zoom}");
            }
        }
    }
}

#[test]
fn moving_an_icon_that_moves_the_picture_keeps_what_belongs() {
    let (w, h) = (1440u32, 900u32);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let mut moved_any = 0;
    for (name, image) in scenery_pictures().into_iter().filter(|(n, _)| ["balloon-sky", "moon-sky", "lone-canoe"].contains(&n.as_str())) {
        let analysis = scooch_core::analyze(&image);
        let icons = layouts::layout("scattered", &analysis, w, h);
        let scene = Scene { width: w, height: h, icons: icons.clone() };
        let placement = scooch_core::solve(&analysis, &scene, &options, None);
        let map = scooch_core::belonging_map(&image, &analysis, &placement, &scene, &options);
        let s = analysis.subject;
        for k in 0..icons.len() {
            let mut moved = icons.clone();
            moved[k].x = ((s.x + 0.5 * s.w) * w as f32 - 0.5 * moved[k].w).clamp(0.0, w as f32 - moved[k].w);
            moved[k].y = ((s.y + 0.5 * s.h) * h as f32 - 0.5 * moved[k].h).clamp(0.0, h as f32 - moved[k].h);
            let next = Scene { width: w, height: h, icons: moved };
            let other = scooch_core::solve(&analysis, &next, &options, None);
            if other == placement {
                continue;
            }
            moved_any += 1;
            println!("{name} icon {k} moves the picture to zoom {:.3} shift {:?}", other.zoom, other.shift);
            assert!(scooch_core::belonging_map(&image, &analysis, &other, &next, &options) == map, "{name} icon {k} moved what belongs");
            break;
        }
    }
    assert!(moved_any >= 3, "too few moves changed the picture: {moved_any}");
}

#[test]
fn the_island_water_colour_gets_islands() {
    let (w, h) = (1440u32, 900u32);
    let image = made("flat-water");
    assert!(image.pixels().all(|p| (0..3).all(|k| p.0[k].abs_diff(scooch_core::ISLAND_WATER[k]) <= 2)));
    let got = coverage("island-water", &image, "pins", &grid(w, h, PIN_GRID.0, PIN_GRID.1), (w, h));
    assert!(got[0] == 0.0 && got[3] > 0.95, "the island water colour drew clouds {:.4} and land under icons {:.4}", got[0], got[3]);
}

#[test]
fn green_water_draws_only_when_islands_are_forced() {
    let (w, h) = (1440u32, 900u32);
    let path = layouts::photos().into_iter().find(|p| p.to_string_lossy().contains("two-swans")).expect("two-swans");
    let image = scooch_core::open(&path).expect("open");
    let analysis = scooch_core::analyze(&image);
    let scene = Scene { width: w, height: h, icons: grid(w, h, PIN_GRID.0, PIN_GRID.1) };
    for (scenery, draws) in [(Scenery::Auto, false), (Scenery::Islands, true)] {
        let options = Options { scenery, ..Options::default() };
        let placement = scooch_core::solve(&analysis, &scene, &options, None);
        let layers = scooch_core::render_layers(&image, &analysis, &placement, &scene, &options);
        let drawn = layers.scenery.pixels().filter(|p| p.0[3] > 0).count();
        println!("two-swans {scenery:?}: {drawn} scenery pixels");
        assert_eq!(drawn > 10_000, draws, "two-swans {scenery:?} drew {drawn}");
        if !draws {
            assert_eq!(drawn, 0, "two-swans {scenery:?}");
        }
    }
}

fn clouds_where(image: &RgbImage, icons: &[Rect], (w, h): (u32, u32), inside: impl Fn(f32, f32) -> bool) -> (usize, usize) {
    let analysis = scooch_core::analyze(image);
    let scene = Scene { width: w, height: h, icons: icons.to_vec() };
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let placement = framing(image, (w, h), 1.0, (0.5, 0.5));
    let layers = scooch_core::render_layers(image, &analysis, &placement, &scene, &options);
    let bundle = scooch_core::bundle(image, &analysis, &placement, &scene, &options);
    let gw = bundle.grid_width as usize;
    let (mut there, mut elsewhere) = (0usize, 0usize);
    for (x, y, p) in layers.scenery.enumerate_pixels() {
        if p[3] == 0 || bundle.look[((y as usize / 4) * gw + x as usize / 4) * 4] < 128 {
            continue;
        }
        if inside(x as f32 / w as f32, y as f32 / h as f32) {
            there += 1;
        } else {
            elsewhere += 1;
        }
    }
    (there, elsewhere)
}

#[test]
fn the_sky_grows_only_up_and_sideways_and_not_far() {
    let (w, h) = (1440u32, 900u32);
    let icons = grid(w, h, 8, 5);
    for (name, below) in [("sea-under-sky", 0.6), ("lake-under-haze", 0.78)] {
        let (water, sky) = clouds_where(&made(name), &icons, (w, h), |_, y| y > below);
        println!("{name}: {water} cloud pixels on the water, {sky} in the sky");
        assert_eq!(water, 0, "{name} clouds on the water");
        assert!(sky > 20_000, "{name} the sky lost its clouds: {sky}");
    }
    let (wall, window) = clouds_where(&made("wall-window"), &icons, (w, h), |x, _| x < 0.45);
    println!("wall-window: {wall} cloud pixels on the wall, {window} in the window");
    assert_eq!(wall, 0, "clouds on the wall");
    assert!(window > 20_000, "the window lost its clouds: {window}");
}
