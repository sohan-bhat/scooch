#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Bundle, Options, Rect, RgbImage, Scene, Scenery, Session, TablesError};
use std::time::{Duration, Instant};
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

const END_MEAN: f64 = 0.01;
const END_WORST: u8 = 5;

#[test]
fn a_glide_starts_and_ends_on_the_stills() {
    let (w, h) = (960, 600);
    let options = on();
    for photo in photos().iter().step_by(2) {
        let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
        let next = moved(&scene, 3, 2.0);
        let (a, b) = (state(photo, &scene, &options), state(photo, &next, &options));
        let (first, last) = (scooch_core::frame(&a, &a, 0.0), scooch_core::frame(&b, &b, 0.0));
        for (s, still) in [(1e-6f32, &first), (1.0 - 1e-6, &last)] {
            let (mean, worst) = difference(&scooch_core::frame(&a, &b, s), still);
            println!("{} near {s}: mean {mean:.4} worst {worst}", photo.name);
            assert!(mean <= END_MEAN && worst <= END_WORST, "{} s {s} mean {mean:.4} worst {worst}", photo.name);
        }
        for (s, still) in [(0.0f32, &first), (1.0, &last)] {
            let (mean, worst) = difference(&scooch_core::frame_through(&a, &b, s), still);
            println!("{} through {s}: mean {mean:.4} worst {worst}", photo.name);
            assert!(mean == 0.0 && worst == 0, "{} through s {s} mean {mean:.4} worst {worst}", photo.name);
        }
    }
}

const STEP_MEAN: f64 = 0.28;

#[test]
fn a_glide_moves_in_small_even_steps() {
    let (w, h) = (640, 400);
    let options = on();
    for name in ["balloon-sky", "lone-canoe", "moon-sky"] {
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
        let (there, here) = (scooch_core::frame_layers(&bundle, &bundle, 0.0), scooch_core::frame_layers(&back, &back, 0.0));
        let (_, picture) = difference(&here.picture, &there.picture);
        let (_, worst) = difference(&scooch_core::frame(&back, &back, 0.0), &scooch_core::frame(&bundle, &bundle, 0.0));
        println!("{} round trip picture worst {picture} frame worst {worst}", photo.name);
        assert!(picture <= 1 && worst <= 2, "{} picture worst {picture} frame worst {worst}", photo.name);
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

#[test]
fn scenery_active_never_misses_a_painted_pixel() {
    let (w, h) = (480, 300);
    let (mut agree, mut total) = (0, 0);
    for photo in photos() {
        for layout in ["mac", "scattered", "cluster"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &photo.analysis, w, h) };
            for scenery in [Scenery::Auto, Scenery::Islands, Scenery::Clouds, Scenery::Off] {
                let options = Options { scenery, ..Options::default() };
                let placement = scooch_core::solve(&photo.analysis, &scene, &options, None);
                let painted = scooch_core::render_layers(&photo.image, &photo.analysis, &placement, &scene, &options).scenery.pixels().any(|p| p[3] > 0);
                let said = scooch_core::scenery_active(&photo.analysis, &placement, &scene, &options);
                assert!(said || !painted, "{} {layout} {scenery:?} painted but said off", photo.name);
                assert!(scenery != Scenery::Off || !said, "{} {layout} said on with scenery off", photo.name);
                agree += (said == painted) as usize;
                total += 1;
            }
        }
    }
    eprintln!("scenery_active agrees with the render on {agree} of {total}");
    let photo = &photos()[0];
    let none = Scene { width: w, height: h, icons: Vec::new() };
    assert!(!scooch_core::scenery_active(&photo.analysis, &scooch_core::fill(w, h, &none), &none, &on()));
    let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
    let busy = Analysis { saliency: vec![1.0; photo.analysis.saliency.len()], flat: false, ..photo.analysis.clone() };
    let placement = scooch_core::solve(&busy, &scene, &on(), None);
    assert!(!scooch_core::scenery_active(&busy, &placement, &scene, &on()));
    assert!(scooch_core::render_layers(&photo.image, &busy, &placement, &scene, &on()).scenery.pixels().all(|p| p[3] == 0));
}

fn mirror(b: &Bundle) -> Bundle {
    let flip = |v: &[u8], w: usize| v.chunks(w * 4).flat_map(|row| row.chunks(4).rev().flatten().copied().collect::<Vec<u8>>()).collect::<Vec<u8>>();
    let (gw, pw) = (b.grid_width as usize, b.paint_width as usize);
    let mut out = b.clone();
    out.field = b.field.chunks(gw).flat_map(|row| row.iter().rev().copied()).collect();
    out.look = flip(&b.look, gw);
    out.pad = flip(&b.pad, gw);
    out.paints = flip(&b.paints, pw);
    out.xmap = b.xmap.iter().rev().copied().collect();
    out
}

#[test]
fn a_bare_picture_keeps_the_real_unit() {
    let (w, h) = (960, 600);
    let photo = photos().iter().find(|p| p.name == "fox").expect("fox");
    let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
    let bundle = state(photo, &scene, &on());
    assert!(bundle.field.iter().all(|f| *f > bundle.constants.reach), "fox drew scenery");
    let u = scene.icons.iter().map(|r| r.h).fold(0.0f32, f32::max);
    assert_eq!(bundle.constants.unit, u, "a bare state glides with unit {}", bundle.constants.unit);
}

#[test]
fn a_glide_takes_its_unit_from_the_state_with_scenery() {
    let (w, h) = (960, 600);
    let photo = photos().iter().find(|p| p.name == "lone-canoe").expect("lone-canoe");
    let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
    let off = Options { scenery: Scenery::Off, ..Options::default() };
    let placement = scooch_core::solve(&photo.analysis, &scene, &on(), None);
    let (with, without) = (scooch_core::bundle(&photo.image, &photo.analysis, &placement, &scene, &on()), scooch_core::bundle(&photo.image, &photo.analysis, &placement, &scene, &off));
    assert!(!with.bare() && without.bare());
    assert_eq!(without.constants.unit, with.constants.unit, "scenery off carries unit {} beside {}", without.constants.unit, with.constants.unit);
    let empty = Scene { icons: Vec::new(), ..scene.clone() };
    let mut none = scooch_core::bundle(&photo.image, &photo.analysis, &placement, &empty, &on());
    assert!(none.bare());
    for s in [0.25f32, 0.5, 0.75] {
        let (there, back) = (scooch_core::frame(&none, &with, s), scooch_core::frame(&with, &none, s));
        let kept = none.constants.unit;
        none.constants.unit = with.constants.unit;
        assert!(scooch_core::frame(&none, &with, s) == there && scooch_core::frame(&with, &none, s) == back, "s {s}: the unit of a state with no icons changed the glide");
        none.constants.unit = kept;
    }
}

#[test]
fn a_mirrored_pair_glides_as_the_mirror() {
    let (w, h) = (640, 400);
    for name in ["lone-canoe", "balloon-sky"] {
        let photo = photos().iter().find(|p| p.name == name).expect("photo");
        let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
        let (mut a, mut b) = (state(photo, &scene, &on()), state(photo, &moved(&scene, 6, 2.0), &on()));
        a.constants.grain = 0.0;
        b.constants.grain = 0.0;
        let (ma, mb) = (mirror(&a), mirror(&b));
        for s in [0.2f32, 0.5, 0.8] {
            let flipped = image::imageops::flip_horizontal(&scooch_core::frame(&a, &b, s));
            let (mean, worst) = difference(&scooch_core::frame(&ma, &mb, s), &flipped);
            println!("{name} mirrored at {s}: mean {mean:.4} worst {worst}");
            assert!(mean <= 0.01 && worst <= 1, "{name} s {s} mean {mean:.4} worst {worst}");
        }
    }
}

#[test]
fn icon_order_does_not_change_the_scenery() {
    let (w, h) = (960, 600);
    for name in ["balloon-sky", "lone-canoe", "fox"] {
        let photo = photos().iter().find(|p| p.name == name).expect("photo");
        let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
        let placement = scooch_core::solve(&photo.analysis, &scene, &on(), None);
        let mut shuffled = scene.clone();
        shuffled.icons.reverse();
        shuffled.icons.rotate_left(3);
        let a = scooch_core::bundle(&photo.image, &photo.analysis, &placement, &scene, &on());
        let b = scooch_core::bundle(&photo.image, &photo.analysis, &placement, &shuffled, &on());
        assert!(a.field.iter().zip(&b.field).all(|(p, q)| p.to_bits() == q.to_bits()), "{name} field");
        assert_eq!(a.look, b.look, "{name} look");
        assert_eq!(a.paints, b.paints, "{name} paints");
        assert_eq!(a.pad, b.pad, "{name} pad");
    }
}

#[test]
fn a_new_icon_leaves_far_scenery_alone() {
    let (w, h) = (2880, 1800);
    for name in ["balloon-sky", "lone-canoe"] {
        let photo = photos().iter().find(|p| p.name == name).expect("photo");
        let icons: Vec<Rect> = (0..10).map(|k| Rect { x: 40.0 + 130.0 * (k % 2) as f32, y: 60.0 + 170.0 * (k / 2) as f32, w: 100.0, h: 100.0 }).collect();
        let scene = Scene { width: w, height: h, icons };
        let mut more = scene.clone();
        more.icons.insert(0, Rect { x: 2700.0, y: 1600.0, w: 100.0, h: 100.0 });
        let placement = scooch_core::solve(&photo.analysis, &scene, &on(), None);
        let a = scooch_core::bundle(&photo.image, &photo.analysis, &placement, &scene, &on());
        let b = scooch_core::bundle(&photo.image, &photo.analysis, &placement, &more, &on());
        let reach = 1300.0;
        let gw = a.grid_width as usize;
        let (mut far, mut same) = (0usize, 0usize);
        for (i, (p, q)) in a.field.iter().zip(&b.field).enumerate() {
            let (x, y) = ((i % gw) as f32 * 4.0 + 2.0, (i / gw) as f32 * 4.0 + 2.0);
            if x > 2700.0 - reach && y > 1600.0 - reach {
                continue;
            }
            far += 1;
            same += (p.to_bits() == q.to_bits() && a.look[i * 4..i * 4 + 4] == b.look[i * 4..i * 4 + 4]) as usize;
        }
        assert!(a.field.iter().any(|v| *v < 0.0), "{name} has no scenery to compare");
        assert_eq!(same, far, "{name} {} of {far} far cells changed", far - same);
    }
}

#[test]
fn the_lattice_stays_bounded_for_odd_icons() {
    let photo = photos().iter().find(|p| p.name == "balloon-sky").expect("photo");
    let small: Vec<Rect> = (0..120).map(|k| Rect { x: 7600.0 - 40.0 * (k / 30) as f32, y: 20.0 + 36.0 * (k % 30) as f32, w: 16.0, h: 16.0 }).collect();
    let cases = [
        ("a one pixel icon", 1920, 1080, vec![Rect { x: 900.0, y: 500.0, w: 1.0, h: 1.0 }], Duration::from_secs(4)),
        ("a sliver at the edge", 1920, 1080, vec![Rect { x: 1918.0, y: 500.0, w: 100.0, h: 100.0 }], Duration::from_secs(4)),
        ("16 px icons on 7680x4320", 7680, 4320, small, Duration::from_secs(20)),
    ];
    for (label, w, h, icons, limit) in cases {
        let scene = Scene { width: w, height: h, icons };
        let placement = scooch_core::solve(&photo.analysis, &scene, &on(), None);
        let mut session = Session::new();
        let started = Instant::now();
        let _ = session.still(&photo.image, &photo.analysis, &placement, &scene, &on());
        let took = started.elapsed();
        let (sites, bytes) = (session.sites(), session.bytes());
        println!("{label}: {sites} sites, session {:.1} MB, {:.2} s", bytes as f64 / 1048576.0, took.as_secs_f64());
        assert!(sites <= scooch_core::SITES_MOST, "{label} {sites} sites");
        assert!(took <= limit, "{label} took {took:?}");
    }
}
