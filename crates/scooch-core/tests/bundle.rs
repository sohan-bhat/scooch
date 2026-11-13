#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Bundle, Options, Rect, RgbImage, Scene, Scenery, TablesError};
use std::sync::OnceLock;

struct Photo {
    name: String,
    image: RgbImage,
    analysis: Analysis,
}

fn photos() -> &'static [Photo] {
    static CELL: OnceLock<Vec<Photo>> = OnceLock::new();
    CELL.get_or_init(|| {
        layouts::photos()
            .into_iter()
            .map(|path| {
                let full = scooch_core::open(&path).expect("photo");
                let scale = 1200.0 / full.width().max(full.height()) as f32;
                let image = image::imageops::resize(&full, (full.width() as f32 * scale) as u32, (full.height() as f32 * scale) as u32, image::imageops::FilterType::Triangle);
                let analysis = scooch_core::analyze(&image);
                Photo { name: path.file_stem().unwrap().to_string_lossy().to_string(), image, analysis }
            })
            .collect()
    })
}

fn on() -> Options {
    Options { scenery: Scenery::Auto, ..Options::default() }
}

fn state(photo: &Photo, scene: &Scene, options: &Options) -> Bundle {
    let placement = scooch_core::solve(&photo.analysis, scene, options, None);
    scooch_core::bundle(&photo.image, &photo.analysis, &placement, scene, options)
}

fn difference(a: &RgbImage, b: &RgbImage) -> (f64, u8) {
    let (mut sum, mut worst) = (0u64, 0u8);
    for (p, q) in a.as_raw().iter().zip(b.as_raw()) {
        let d = p.abs_diff(*q);
        sum += d as u64;
        worst = worst.max(d);
    }
    (sum as f64 / a.as_raw().len() as f64, worst)
}

fn moved(scene: &Scene, index: usize, by: f32) -> Scene {
    let mut icons = scene.icons.clone();
    icons[index].x += by * icons[index].h;
    Scene { icons, ..scene.clone() }
}

#[test]
fn the_still_is_the_frame_of_its_own_bundle() {
    let (w, h) = (960, 600);
    for photo in photos() {
        for layout in ["mac", "scattered", "cluster"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &photo.analysis, w, h) };
            for options in [on(), Options { pads: 0.0, ..Options::default() }, Options::default()] {
                let placement = scooch_core::solve(&photo.analysis, &scene, &options, None);
                let still = scooch_core::render_with(&photo.image, &photo.analysis, &placement, &scene, &options);
                let bundle = scooch_core::bundle(&photo.image, &photo.analysis, &placement, &scene, &options);
                let frame = scooch_core::frame(&bundle, &bundle, 0.0);
                if options.scenery == Scenery::Off && options.pads > 0.0 {
                    let (mean, _) = difference(&frame, &still);
                    assert!(mean <= 1.0, "{} {layout} pads mean {mean:.3}", photo.name);
                } else {
                    assert_eq!(frame.as_raw(), still.as_raw(), "{} {layout} {:?} pads {}", photo.name, options.scenery, options.pads);
                }
            }
        }
    }
}

#[test]
fn a_glide_starts_and_ends_on_the_stills() {
    let (w, h) = (960, 600);
    let options = on();
    for photo in photos().iter().step_by(2) {
        let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
        let next = moved(&scene, 3, 2.0);
        let (a, b) = (state(photo, &scene, &options), state(photo, &next, &options));
        for (s, still) in [(0.0, scooch_core::frame(&a, &a, 0.0)), (1.0, scooch_core::frame(&b, &b, 0.0))] {
            let (mean, worst) = difference(&scooch_core::frame(&a, &b, s), &still);
            assert!(mean <= 1.0 && worst <= 6, "{} s {s} mean {mean:.3} worst {worst}", photo.name);
        }
    }
}

const STEP_MEAN: f64 = 0.08;

#[test]
fn a_glide_moves_in_small_even_steps() {
    let (w, h) = (640, 400);
    let options = on();
    for name in ["balloon-sky", "two-swans", "moon-sky"] {
        let photo = photos().iter().find(|p| p.name == name).expect("photo");
        let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
        let next = moved(&scene, 6, 2.0);
        let (a, b) = (state(photo, &scene, &options), state(photo, &next, &options));
        let times: Vec<f32> = (0..10).map(|k| 0.05 + 0.09 * k as f32).collect();
        let steps: Vec<f64> = times.iter().map(|s| difference(&scooch_core::frame(&a, &b, *s), &scooch_core::frame(&a, &b, s + 1.0 / 60.0)).0).collect();
        for (s, step) in times.iter().zip(&steps) {
            assert!(*step <= STEP_MEAN, "{name} s {s:.2} step {step:.3}");
        }
        let frames: Vec<RgbImage> = (0..=20).map(|k| scooch_core::frame(&a, &b, k as f32 / 20.0)).collect();
        let bound = STEP_MEAN * 3.0 * 2.0;
        for k in 1..20 {
            let (before, after) = (difference(&frames[k], &frames[k - 1]).0, difference(&frames[k], &frames[k + 1]).0);
            assert!(before.min(after) <= bound, "{name} frame {k} jumps {before:.3} {after:.3}");
        }
    }
}

#[test]
fn bundles_round_trip_and_stay_small() {
    let photo = &photos()[1];
    let scene = Scene { width: 320, height: 200, icons: vec![Rect { x: 40.0, y: 30.0, w: 38.0, h: 47.0 }, Rect { x: 200.0, y: 100.0, w: 38.0, h: 47.0 }] };
    let bundle = state(photo, &scene, &on());
    let text = serde_json::to_string(&bundle).unwrap();
    let back: Bundle = serde_json::from_str(&text).unwrap();
    assert_eq!(back.xmap, bundle.xmap);
    assert_eq!(back.ymap, bundle.ymap);
    assert_eq!(back.field, bundle.field);
    assert_eq!(back.constants, bundle.constants);
    assert_eq!(back.ratio, bundle.ratio);
    assert_eq!(back, bundle);
    let (w, h) = (2880, 1800);
    let big = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
    let large = state(photo, &big, &on());
    assert!(large.table_bytes() < 6 * 1024 * 1024, "{} bytes", large.table_bytes());
    assert!(large.canvas_width.max(large.canvas_height) <= 2 * w);
}

#[test]
fn only_one_canvas_and_one_direction_can_glide() {
    let (w, h) = (320, 200);
    let scene = Scene { width: w, height: h, icons: vec![Rect { x: 40.0, y: 30.0, w: 38.0, h: 47.0 }] };
    let (first, second) = (&photos()[0], &photos()[1]);
    let a = state(first, &scene, &on());
    assert!(scooch_core::compatible(&a, &state(first, &moved(&scene, 0, 1.0), &on())));
    assert!(!scooch_core::compatible(&a, &state(second, &scene, &on())));
    let mut flipped = scooch_core::fill(first.image.width(), first.image.height(), &scene);
    flipped.fx.reverse();
    flipped.mirrored = true;
    let mirror = scooch_core::bundle(&first.image, &first.analysis, &flipped, &scene, &on());
    assert!(mirror.mirrored() && !scooch_core::compatible(&a, &mirror));
    let crossed = scooch_core::frame(&a, &mirror, 0.5);
    assert_eq!(crossed.dimensions(), (w, h));
}

#[test]
fn tables_travel_without_the_canvas() {
    let (w, h) = (960, 600);
    for photo in photos().iter().step_by(6) {
        let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
        let bundle = state(photo, &scene, &on());
        let bytes = bundle.tables_to_bytes();
        let canvas = scooch_core::canvas(&photo.image, w, h);
        assert_eq!((canvas.width, canvas.height), (bundle.canvas_width, bundle.canvas_height));
        assert_eq!(canvas.pixels, bundle.canvas);
        let back = Bundle::tables_from_bytes(&bytes, canvas.clone()).expect("round trip");
        let (_, worst) = difference(&scooch_core::frame(&back, &back, 0.0), &scooch_core::frame(&bundle, &bundle, 0.0));
        assert!(worst <= 1, "{} worst {worst}", photo.name);
        let mut touched = canvas.clone();
        touched.pixels[17] ^= 1;
        assert_eq!(Bundle::tables_from_bytes(&bytes, touched), Err(TablesError::Canvas));
        let other = photos().iter().find(|p| p.name != photo.name).expect("photo");
        assert!(Bundle::tables_from_bytes(&bytes, scooch_core::canvas(&other.image, w, h)).is_err());
        for cut in [0, 3, 4, 11, 40, 100, bytes.len() / 2, bytes.len() - 9, bytes.len() - 1] {
            assert!(Bundle::tables_from_bytes(&bytes[..cut], canvas.clone()).is_err(), "cut {cut}");
        }
        for at in [0, 5, 9, 30, 70, bytes.len() / 3, bytes.len() - 3] {
            let mut broken = bytes.clone();
            broken[at] ^= 0x41;
            assert!(Bundle::tables_from_bytes(&broken, canvas.clone()).is_err(), "byte {at}");
        }
    }
    let photo = &photos()[2];
    let (w, h) = (2880, 1800);
    let big = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
    assert!(state(photo, &big, &on()).tables_to_bytes().len() < 6 * 1024 * 1024);
}

#[test]
fn one_pass_gives_the_still_and_its_bundle() {
    let (w, h) = (640, 400);
    for photo in photos().iter().step_by(2) {
        for layout in ["mac", "scattered"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &photo.analysis, w, h) };
            for options in [on(), Options::default(), Options { pads: 0.0, ..Options::default() }] {
                let placement = scooch_core::solve(&photo.analysis, &scene, &options, None);
                let (bundle, still) = scooch_core::still(&photo.image, &photo.analysis, &placement, &scene, &options);
                let alone = scooch_core::render_with(&photo.image, &photo.analysis, &placement, &scene, &options);
                assert_eq!(still.as_raw(), alone.as_raw(), "{} {layout} {:?}", photo.name, options.scenery);
                assert!(bundle == scooch_core::bundle(&photo.image, &photo.analysis, &placement, &scene, &options), "{} {layout} {:?}", photo.name, options.scenery);
            }
        }
    }
    let empty = Scene { width: 0, height: 0, icons: Vec::new() };
    let photo = &photos()[0];
    let (bundle, still) = scooch_core::still(&photo.image, &photo.analysis, &scooch_core::fill(1, 1, &empty), &empty, &on());
    assert_eq!((bundle.width, still.width()), (0, 0));
}
