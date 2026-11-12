#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Options, Placement, Rect, RgbImage, Scene, TABLE};
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
                let image = scooch_core::open(&path).expect("photo opens");
                let analysis = scooch_core::analyze(&image);
                Photo { name: path.file_stem().unwrap().to_string_lossy().into(), image, analysis }
            })
            .collect()
    })
}

fn small(image: &RgbImage, long: u32) -> RgbImage {
    let scale = long as f32 / image.width().max(image.height()) as f32;
    image::imageops::resize(image, (image.width() as f32 * scale).round() as u32, (image.height() as f32 * scale).round() as u32, image::imageops::FilterType::Triangle)
}

fn check_table(values: &[f32], falling: bool, label: &str) {
    assert_eq!(values.len(), TABLE, "{label}");
    assert!(values.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)), "{label}");
    for pair in values.windows(2) {
        if falling {
            assert!(pair[1] < pair[0], "{label} not falling");
        } else {
            assert!(pair[1] > pair[0], "{label} not rising");
        }
    }
    let span = (values[TABLE - 1] - values[0]).abs() as f64;
    let steps: Vec<f64> = values.windows(2).map(|p| (p[1] as f64 - p[0] as f64).abs() * (TABLE - 1) as f64 / span).collect();
    let jump = steps.windows(2).map(|p| (p[1] - p[0]).abs()).fold(0.0f64, f64::max);
    assert!(jump < 0.1, "{label} slope jumps by {jump}");
}

fn check_placement(placement: &Placement, label: &str) {
    check_table(&placement.fx, placement.mirrored, &format!("{label} fx"));
    check_table(&placement.fy, false, &format!("{label} fy"));
}

#[test]
fn every_photo_and_layout_keeps_tables_valid_and_never_hides_more() {
    let options = Options::default();
    let mut warped = 0;
    for photo in photos() {
        for (w, h) in layouts::SCREENS {
            for layout in layouts::NAMES {
                let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &photo.analysis, w, h) };
                let label = format!("{} {w}x{h} {layout}", photo.name);
                let placement = scooch_core::solve(&photo.analysis, &scene, &options, None);
                check_placement(&placement, &label);
                let centered = scooch_core::fill(photo.image.width(), photo.image.height(), &scene);
                let after = scooch_core::hidden_share(&photo.analysis, &scene, &placement);
                let before = scooch_core::hidden_share(&photo.analysis, &scene, &centered);
                assert!(after <= before, "{label} hides {after} > {before}");
                if placement.shift != [0.0, 0.0] {
                    warped += 1;
                }
            }
        }
    }
    println!("warped placements: {warped}");
}

#[test]
fn a_warped_placement_has_valid_tables() {
    let image = RgbImage::from_fn(640, 400, |x, y| {
        let inside = (260..380).contains(&x) && (140..260).contains(&y);
        if inside {
            image::Rgb([230, 60, 40])
        } else {
            image::Rgb([40 + (y / 8) as u8, 90, 160])
        }
    });
    let analysis = scooch_core::analyze(&image);
    let icons: Vec<Rect> = (0..4).map(|i| Rect { x: 300.0 + 40.0 * (i % 2) as f32, y: 140.0 + 60.0 * (i / 2) as f32, w: 36.0, h: 56.0 }).collect();
    let scene = Scene { width: 640, height: 400, icons };
    let placement = scooch_core::solve(&analysis, &scene, &Options::default(), None);
    check_placement(&placement, "block");
    assert_ne!(placement.shift, [0.0, 0.0], "{placement:?}");
    let mirrored = scooch_core::solve(&analysis, &scene, &Options { mirror: true, ..Options::default() }, None);
    check_placement(&mirrored, "block mirrored");
}

#[test]
fn no_icons_gives_the_centered_crop_and_the_plain_fill() {
    let options = Options::default();
    for photo in photos().iter().take(3) {
        let image = small(&photo.image, 900);
        let analysis = scooch_core::analyze(&image);
        for (w, h) in [(640u32, 400u32), (360, 640)] {
            let scene = Scene { width: w, height: h, icons: Vec::new() };
            let solved = scooch_core::solve(&analysis, &scene, &options, None);
            let (cw, ch) = if image.width() as f32 / image.height() as f32 >= w as f32 / h as f32 {
                ((w as f32 / h as f32) / (image.width() as f32 / image.height() as f32), 1.0)
            } else {
                (1.0, (image.width() as f32 / image.height() as f32) / (w as f32 / h as f32))
            };
            let identity = Placement {
                fx: (0..TABLE).map(|k| (1.0 - cw) / 2.0 + cw * k as f32 / 256.0).collect(),
                fy: (0..TABLE).map(|k| (1.0 - ch) / 2.0 + ch * k as f32 / 256.0).collect(),
                zoom: 1.0,
                mirrored: false,
                crop: [(1.0 - cw) / 2.0, (1.0 - ch) / 2.0, cw, ch],
                shift: [0.0, 0.0],
            };
            assert_eq!(solved.zoom, 1.0);
            assert!(!solved.mirrored);
            assert_eq!(solved.shift, [0.0, 0.0]);
            for k in 0..TABLE {
                assert!((solved.fx[k] - identity.fx[k]).abs() < 1e-6 && (solved.fy[k] - identity.fy[k]).abs() < 1e-6);
            }
            let a = scooch_core::render(&image, &solved, &scene, &options);
            let b = scooch_core::render(&image, &identity, &scene, &options);
            assert_eq!(a.as_raw(), b.as_raw(), "{} {w}x{h}", photo.name);
        }
    }
}

#[test]
fn two_runs_give_identical_bytes() {
    let photo = &photos()[0];
    let image = small(&photo.image, 1200);
    let options = Options { mirror: true, ..Options::default() };
    let (w, h) = (960, 600);
    for layout in ["mac", "scattered", "cluster"] {
        let analysis = scooch_core::analyze(&image);
        let again = scooch_core::analyze(&image);
        assert_eq!(analysis, again);
        let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &analysis, w, h) };
        let first = scooch_core::solve(&analysis, &scene, &options, None);
        let second = scooch_core::solve(&again, &scene, &options, None);
        assert_eq!(first, second);
        let a = scooch_core::render(&image, &first, &scene, &options);
        let b = scooch_core::render(&image, &second, &scene, &options);
        assert_eq!(scooch_core::encode_png(&a).unwrap(), scooch_core::encode_png(&b).unwrap());
    }
}

#[test]
fn a_small_nudge_keeps_the_previous_placement() {
    let options = Options::default();
    let mut moved_any = false;
    for photo in photos() {
        for (w, h) in [(2880u32, 1800u32), (1920, 1080)] {
            for layout in ["mac", "windows", "scattered", "cluster"] {
                let icons = layouts::layout(layout, &photo.analysis, w, h);
                let scene = Scene { width: w, height: h, icons: icons.clone() };
                let first = scooch_core::solve(&photo.analysis, &scene, &options, None);
                for (i, (dx, dy)) in [(4.0f32, 0.0f32), (0.0, 4.0), (-4.0, 0.0), (0.0, -4.0)].into_iter().enumerate() {
                    let mut nudged = icons.clone();
                    let k = (i * 5) % nudged.len();
                    nudged[k].x += dx;
                    nudged[k].y += dy;
                    let next = Scene { width: w, height: h, icons: nudged };
                    let kept = scooch_core::solve(&photo.analysis, &next, &options, Some(&first));
                    assert_eq!(kept, first, "{} {w}x{h} {layout} nudge {i}", photo.name);
                    if scooch_core::solve(&photo.analysis, &next, &options, None) != first {
                        moved_any = true;
                    }
                }
            }
        }
    }
    println!("a fresh solve would have moved: {moved_any}");
}

#[test]
fn a_big_change_still_moves_the_picture() {
    let photo = photos().iter().find(|p| p.name == "dragonfly").unwrap_or(&photos()[0]);
    let options = Options::default();
    let (w, h) = (2880, 1800);
    let calm = Scene { width: w, height: h, icons: layouts::mac(w, h, 16) };
    let first = scooch_core::solve(&photo.analysis, &calm, &options, None);
    let crowded = Scene { width: w, height: h, icons: layouts::cluster(&photo.analysis, w, h) };
    let fresh = scooch_core::solve(&photo.analysis, &crowded, &options, None);
    let next = scooch_core::solve(&photo.analysis, &crowded, &options, Some(&first));
    assert_eq!(next, fresh);
    assert_ne!(next, first);
}

#[test]
fn odd_inputs_do_not_panic() {
    let options = Options::default();
    let tiny = RgbImage::from_pixel(1, 1, image::Rgb([200, 10, 10]));
    let thin = RgbImage::from_fn(8, 8000, |x, y| image::Rgb([(x * 30) as u8, (y % 251) as u8, 90]));
    let mut weird = vec![
        Rect { x: 10.0, y: 10.0, w: 0.0, h: 40.0 },
        Rect { x: 10.0, y: 10.0, w: -30.0, h: -40.0 },
        Rect { x: f32::NAN, y: 10.0, w: 30.0, h: 40.0 },
        Rect { x: 10.0, y: f32::INFINITY, w: 30.0, h: 40.0 },
        Rect { x: -5000.0, y: -5000.0, w: 30.0, h: 40.0 },
        Rect { x: 1e9, y: 1e9, w: 30.0, h: 40.0 },
        Rect { x: -20.0, y: -20.0, w: 50.0, h: 60.0 },
        Rect { x: 1.0, y: 1.0, w: 1e9, h: 1e9 },
    ];
    weird.extend(layouts::scattered(1600, 1000, 500, 99));
    let screens = [(16u32, 9000u32), (1, 1), (1600, 1000), (0, 0), (5, 0)];
    let odd_options = [options, Options { strength: f32::NAN, max_zoom: f32::INFINITY, mirror: true, pads: f32::NAN }, Options { strength: 99.0, max_zoom: -3.0, mirror: true, pads: 7.0 }];
    for image in [&tiny, &thin] {
        let analysis = scooch_core::analyze(image);
        assert_eq!(analysis.saliency.len(), (analysis.width * analysis.height) as usize);
        let _ = scooch_core::saliency_image(&analysis);
        for (w, h) in screens {
            for opts in &odd_options {
                let scene = Scene { width: w, height: h, icons: weird.clone() };
                let placement = scooch_core::solve(&analysis, &scene, opts, None);
                assert_eq!(placement.fx.len(), TABLE);
                let again = scooch_core::solve(&analysis, &scene, opts, Some(&placement));
                assert_eq!(again.fx.len(), TABLE);
                if w > 0 && h > 0 && (w as u64 * h as u64) <= 2_000_000 {
                    let out = scooch_core::render(image, &placement, &scene, opts);
                    assert_eq!(out.dimensions(), (w, h));
                }
            }
        }
    }
    let empty = RgbImage::new(0, 0);
    let analysis = scooch_core::analyze(&empty);
    let scene = Scene { width: 64, height: 40, icons: weird.clone() };
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    assert_eq!(scooch_core::render(&empty, &placement, &scene, &options).dimensions(), (64, 40));
    let photo = &photos()[0];
    let crowd = Scene { width: 2880, height: 1800, icons: layouts::scattered(2880, 1800, 500, 5) };
    let placement = scooch_core::solve(&photo.analysis, &crowd, &options, None);
    assert_eq!(placement.fx.len(), TABLE);
    let broken = Placement { fx: vec![f32::NAN; 4], fy: vec![], zoom: f32::NAN, mirrored: true, crop: [f32::NAN; 4], shift: [0.0; 2] };
    let fixed = scooch_core::solve(&photo.analysis, &crowd, &options, Some(&broken));
    assert_eq!(fixed, placement);
    let image = small(&photo.image, 400);
    assert_eq!(scooch_core::render(&image, &broken, &Scene { width: 50, height: 30, icons: weird }, &options).dimensions(), (50, 30));
}

#[test]
fn a_plain_gradient_is_flat_and_gets_no_pads() {
    let image = RgbImage::from_fn(1200, 800, |x, y| {
        let t = (x as f32 / 1199.0 + y as f32 / 799.0) / 2.0;
        image::Rgb([(30.0 + 180.0 * t) as u8, (60.0 + 120.0 * t) as u8, (150.0 + 90.0 * t) as u8])
    });
    let analysis = scooch_core::analyze(&image);
    assert!(analysis.flat);
    let (w, h) = (960, 600);
    let scene = Scene { width: w, height: h, icons: layouts::scattered(w, h, 25, 3) };
    let options = Options::default();
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    check_placement(&placement, "gradient");
    let padded = scooch_core::render(&image, &placement, &scene, &options);
    let plain = scooch_core::render(&image, &placement, &scene, &Options { pads: 0.0, ..options });
    assert_eq!(padded.as_raw(), plain.as_raw());
}

#[test]
fn busy_ground_under_icons_gets_pads() {
    let image = RgbImage::from_fn(1200, 800, |x, y| if ((x / 12) + (y / 12)) % 2 == 0 { image::Rgb([235, 235, 225]) } else { image::Rgb([25, 30, 40]) });
    let analysis = scooch_core::analyze(&image);
    let (w, h) = (960, 600);
    let scene = Scene { width: w, height: h, icons: layouts::mac(w, h, 8) };
    let options = Options::default();
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    let padded = scooch_core::render(&image, &placement, &scene, &options);
    let plain = scooch_core::render(&image, &placement, &scene, &Options { pads: 0.0, ..options });
    assert_ne!(padded.as_raw(), plain.as_raw());
    let far = (0..w / 3).all(|x| (0..h).all(|y| padded.get_pixel(x, y) == plain.get_pixel(x, y)));
    assert!(far, "pads reached pixels far from every icon");
}
