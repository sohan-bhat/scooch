#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Options, Placement, Rect, RgbImage, Scene, Scenery, TABLE};
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
    let fixed = Options { max_zoom: 1.0, ..Options::default() };
    let placement = scooch_core::solve(&analysis, &scene, &fixed, None);
    check_placement(&placement, "block");
    assert_ne!(placement.shift, [0.0, 0.0], "{placement:?}");
    let centered = scooch_core::fill(640, 400, &scene);
    assert!(scooch_core::hidden_share(&analysis, &scene, &placement) < scooch_core::hidden_share(&analysis, &scene, &centered));
    let mirrored = scooch_core::solve(&analysis, &scene, &Options { mirror: true, ..fixed }, None);
    check_placement(&mirrored, "block mirrored");
    let gentle = scooch_core::solve(&analysis, &scene, &Options { strength: 0.0, ..fixed }, None);
    assert_eq!(gentle.shift, [0.0, 0.0]);
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
            let (image_aspect, screen_aspect) = (image.width() as f64 / image.height() as f64, w as f64 / h as f64);
            let (cw, ch) = if image_aspect >= screen_aspect { (screen_aspect / image_aspect, 1.0) } else { (1.0, image_aspect / screen_aspect) };
            let line = |span: f64| -> Vec<f32> {
                let start = (1.0 - span) / 2.0;
                let mut out: Vec<f32> = (0..TABLE).map(|k| (start + span * k as f64 / 256.0) as f32).collect();
                out[TABLE - 1] = (start + span) as f32;
                out
            };
            let identity = Placement {
                fx: line(cw),
                fy: line(ch),
                zoom: 1.0,
                mirrored: false,
                crop: [((1.0 - cw) / 2.0) as f32, ((1.0 - ch) / 2.0) as f32, cw as f32, ch as f32],
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
    let odd_options = [options, Options { strength: f32::NAN, max_zoom: f32::INFINITY, mirror: true, pads: f32::NAN, scenery: Scenery::Clouds }, Options { strength: 99.0, max_zoom: -3.0, mirror: true, pads: 7.0, scenery: Scenery::Islands }];
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
    let options = Options { scenery: Scenery::Off, ..Options::default() };
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
    let options = Options { scenery: Scenery::Off, ..Options::default() };
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    let padded = scooch_core::render(&image, &placement, &scene, &options);
    let plain = scooch_core::render(&image, &placement, &scene, &Options { pads: 0.0, ..options });
    assert_ne!(padded.as_raw(), plain.as_raw());
    let far = (0..w / 3).all(|x| (0..h).all(|y| padded.get_pixel(x, y) == plain.get_pixel(x, y)));
    assert!(far, "pads reached pixels far from every icon");
}

fn exif(orientation: u16) -> Vec<u8> {
    let mut out = vec![0x49, 0x49, 0x2A, 0x00, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0];
    out.extend_from_slice(&orientation.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    out
}

fn write_jpeg(path: &std::path::Path, picture: &RgbImage, orientation: Option<u16>) {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95);
    if let Some(value) = orientation {
        encoder.set_exif_metadata(exif(value)).unwrap();
    }
    encoder.write_image(picture.as_raw(), picture.width(), picture.height(), image::ExtendedColorType::Rgb8).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn open_turns_the_picture_by_its_exif_orientation() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let marked = RgbImage::from_fn(64, 32, |x, y| if x < 12 && y < 12 { image::Rgb([230, 30, 30]) } else { image::Rgb([60, 90, 140]) });
    let red = |p: &image::Rgb<u8>| p[0] > 170 && p[1] < 100 && p[2] < 100;
    let plain = dir.join("scooch-orientation-none.jpg");
    write_jpeg(&plain, &marked, None);
    let opened = scooch_core::open(&plain).unwrap();
    assert_eq!(opened.as_raw(), image::open(&plain).unwrap().to_rgb8().as_raw());
    assert_eq!(opened.dimensions(), (64, 32));
    assert!(red(opened.get_pixel(3, 3)));
    let six = dir.join("scooch-orientation-6.jpg");
    write_jpeg(&six, &marked, Some(6));
    let turned = scooch_core::open(&six).unwrap();
    assert_eq!(turned.dimensions(), (32, 64));
    assert!(red(turned.get_pixel(28, 3)) && !red(turned.get_pixel(3, 3)) && !red(turned.get_pixel(3, 60)));
    let three = dir.join("scooch-orientation-3.jpg");
    write_jpeg(&three, &marked, Some(3));
    let flipped = scooch_core::open(&three).unwrap();
    assert_eq!(flipped.dimensions(), (64, 32));
    assert!(red(flipped.get_pixel(60, 28)) && !red(flipped.get_pixel(3, 3)));
}

#[test]
fn render_with_matches_render_byte_for_byte() {
    let options = Options::default();
    for photo in photos().iter().step_by(3) {
        let picture = small(&photo.image, 1200);
        let analysis = scooch_core::analyze(&picture);
        let (w, h) = (960, 600);
        for layout in ["mac", "scattered", "none"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &analysis, w, h) };
            let placement = scooch_core::solve(&analysis, &scene, &options, None);
            let a = scooch_core::render(&picture, &placement, &scene, &options);
            let b = scooch_core::render_with(&picture, &analysis, &placement, &scene, &options);
            assert_eq!(a.as_raw(), b.as_raw(), "{} {layout}", photo.name);
        }
    }
}

#[test]
fn pads_active_never_misses_a_pad() {
    let options = Options { scenery: Scenery::Off, ..Options::default() };
    let bare = Options { pads: 0.0, ..options };
    let (w, h) = (960, 600);
    for photo in photos() {
        for layout in layouts::NAMES {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &photo.analysis, w, h) };
            let placement = scooch_core::solve(&photo.analysis, &scene, &options, None);
            let active = scooch_core::pads_active(&photo.analysis, &placement, &scene, &options);
            assert!(!scooch_core::pads_active(&photo.analysis, &placement, &scene, &bare));
            if !active {
                let padded = scooch_core::render_with(&photo.image, &photo.analysis, &placement, &scene, &options);
                let plain = scooch_core::render_with(&photo.image, &photo.analysis, &placement, &scene, &bare);
                assert_eq!(padded.as_raw(), plain.as_raw(), "{} {layout}", photo.name);
            }
        }
    }
}

struct Shifted {
    label: String,
    before: f32,
    after: f32,
    rigid: usize,
}

const STRIP: f32 = 0.10;
const CALM_STRIP: usize = 10;

fn rigid_strip_columns(analysis: &Analysis, at: f32) -> usize {
    let (w, h) = (analysis.width as usize, analysis.height as usize);
    let columns: Vec<usize> = if at > 0.5 { ((w as f32 * (1.0 - STRIP)) as usize..w).collect() } else { (0..(w as f32 * STRIP).ceil() as usize).collect() };
    columns
        .into_iter()
        .filter(|&c| {
            let mut column: Vec<f32> = (0..h).map(|r| analysis.saliency[r * w + c]).collect();
            column.sort_by(f32::total_cmp);
            let peak = column[((h - 1) as f32 * 0.97).round() as usize];
            let t = ((peak - 0.12) / (0.35 - 0.12)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t) >= 0.5
        })
        .count()
}

fn shifted_set(edge: bool, options: Options) -> Vec<Shifted> {
    let (w, h) = (2880, 1800);
    let mut out = Vec::new();
    for photo in photos() {
        let spots = if edge { layouts::EDGES } else { layouts::SHIFTS };
        for (at, layout) in spots {
            let crop = if edge { layouts::edged(&photo.image, &photo.analysis, at) } else { layouts::shifted(&photo.image, &photo.analysis, at) };
            let Some(crop) = crop else { continue };
            let crop = small(&crop, 1600);
            let analysis = scooch_core::analyze(&crop);
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &analysis, w, h) };
            let placement = scooch_core::solve(&analysis, &scene, &options, None);
            let label = format!("{}@{}", photo.name, (at * 100.0).round());
            check_placement(&placement, &label);
            let centered = scooch_core::fill(crop.width(), crop.height(), &scene);
            let before = scooch_core::hidden_share(&analysis, &scene, &centered);
            let after = scooch_core::hidden_share(&analysis, &scene, &placement);
            let rigid = rigid_strip_columns(&analysis, at);
            println!("{label:20} hidden {before:.3} -> {after:.3} zoom {:.2} mirrored {} shift {:?} rigid strip columns {rigid}", placement.zoom, placement.mirrored, placement.shift);
            out.push(Shifted { label, before, after, rigid });
        }
    }
    out
}

fn touching() -> &'static [Shifted] {
    static CELL: OnceLock<Vec<Shifted>> = OnceLock::new();
    CELL.get_or_init(|| shifted_set(false, Options { mirror: true, scenery: Scenery::Off, ..Options::default() }))
}

fn behind_strip() -> &'static [Shifted] {
    static CELL: OnceLock<Vec<Shifted>> = OnceLock::new();
    CELL.get_or_init(|| shifted_set(true, Options { scenery: Scenery::Off, ..Options::default() }))
}

fn assert_most_clear(cases: &[Shifted]) {
    let collided: Vec<&Shifted> = cases.iter().filter(|c| c.before > 0.02).collect();
    let cleared = collided.iter().filter(|c| c.after < 0.5 * c.before).count();
    let missed: Vec<&str> = collided.iter().filter(|c| c.after >= 0.5 * c.before).map(|c| c.label.as_str()).collect();
    assert!(collided.len() >= 6, "only {} cases collide", collided.len());
    assert!(3 * cleared >= 2 * collided.len(), "{cleared} of {} cleared, missed {missed:?}", collided.len());
}

#[test]
fn shifted_subjects_never_hide_more() {
    for cases in [touching(), behind_strip()] {
        assert!(cases.len() >= 10, "{}", cases.len());
        for case in cases {
            assert!(case.after <= case.before, "{} {} > {}", case.label, case.after, case.before);
        }
    }
}

#[test]
fn subjects_touching_the_edge_clear_with_a_flip() {
    assert_most_clear(touching());
}

#[test]
fn subjects_with_a_calm_strip_behind_slide_clear() {
    let counted: Vec<&Shifted> = behind_strip().iter().filter(|c| c.before > 0.02 && c.rigid <= CALM_STRIP).collect();
    let cleared = counted.iter().filter(|c| c.after < 0.5 * c.before).count();
    let missed: Vec<&str> = counted.iter().filter(|c| c.after >= 0.5 * c.before).map(|c| c.label.as_str()).collect();
    assert!(counted.len() >= 4, "only {} cases collide with a calm strip", counted.len());
    assert!(3 * cleared >= 2 * counted.len(), "{cleared} of {} cleared, missed {missed:?}", counted.len());
}

fn on_screen_distance(px: f32, py: f32, r: &Rect) -> f32 {
    let dx = (r.x - px).max(px - (r.x + r.w)).max(0.0);
    let dy = (r.y - py).max(py - (r.y + r.h)).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

fn mean_lightness(picture: &RgbImage, layer: &scooch_core::RgbaImage, r: &Rect) -> Option<f32> {
    let (mut sum, mut count) = (0.0f32, 0.0f32);
    let (x0, x1) = (r.x.max(0.0) as u32, ((r.x + r.w) as u32).min(picture.width()));
    let (y0, y1) = ((r.y + r.h * 2.0 / 3.0).max(0.0) as u32, ((r.y + r.h) as u32).min(picture.height()));
    for y in y0..y1 {
        for x in x0..x1 {
            if layer.get_pixel(x, y).0[3] < 128 {
                continue;
            }
            let p = picture.get_pixel(x, y).0;
            let lin = p.map(|v| {
                let c = v as f32 / 255.0;
                if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
            });
            sum += (0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]).cbrt();
            count += 1.0;
        }
    }
    (count > 0.0).then(|| sum / count)
}

#[test]
fn scenery_off_adds_nothing_and_layers_rebuild_the_render() {
    let (w, h) = (960, 600);
    for photo in photos().iter().step_by(4) {
        let picture = small(&photo.image, 1200);
        let analysis = scooch_core::analyze(&picture);
        for layout in ["mac", "scattered"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &analysis, w, h) };
            for scenery in [Scenery::Off, Scenery::Auto, Scenery::Islands, Scenery::Clouds] {
                let options = Options { scenery, ..Options::default() };
                let placement = scooch_core::solve(&analysis, &scene, &options, None);
                let whole = scooch_core::render_with(&picture, &analysis, &placement, &scene, &options);
                let layers = scooch_core::render_layers(&picture, &analysis, &placement, &scene, &options);
                let mut rebuilt = layers.picture.clone();
                scooch_core::composite(&mut rebuilt, &layers.scenery);
                assert_eq!(rebuilt.as_raw(), whole.as_raw(), "{} {layout} {scenery:?}", photo.name);
                if scenery == Scenery::Off {
                    assert!(layers.scenery.pixels().all(|p| p.0[3] == 0));
                } else {
                    assert!(layers.scenery.pixels().any(|p| p.0[3] > 0), "{} {layout} {scenery:?} drew nothing", photo.name);
                }
            }
        }
    }
}

#[test]
fn scenery_is_deterministic_and_local() {
    let photo = &photos()[0];
    let picture = small(&photo.image, 1200);
    let analysis = scooch_core::analyze(&picture);
    let (w, h) = (1440, 900);
    let icons: Vec<Rect> = [(100.0, 100.0), (180.0, 100.0), (900.0, 500.0), (1200.0, 150.0), (300.0, 650.0)].iter().map(|(x, y)| Rect { x: *x, y: *y, w: 76.0, h: 94.0 }).collect();
    let u = 94.0;
    let scene = Scene { width: w, height: h, icons: icons.clone() };
    let options = Options::default();
    let placement = scooch_core::fill(picture.width(), picture.height(), &scene);
    let first = scooch_core::render_with(&picture, &analysis, &placement, &scene, &options);
    let again = scooch_core::render_with(&picture, &analysis, &placement, &scene, &options);
    assert_eq!(first.as_raw(), again.as_raw());
    let mut moved = icons.clone();
    moved[2].x += 2.0 * u;
    let next = scooch_core::render_with(&picture, &analysis, &placement, &Scene { icons: moved.clone(), ..scene.clone() }, &options);
    let mut changed_near = false;
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let far = on_screen_distance(px, py, &icons[2]) > 3.0 * u && on_screen_distance(px, py, &moved[2]) > 3.0 * u;
            let same = first.get_pixel(x, y) == next.get_pixel(x, y);
            if far {
                assert!(same, "pixel {x},{y} changed far from the moved icon");
            } else if !same {
                changed_near = true;
            }
        }
    }
    assert!(changed_near);
}

#[test]
fn scenery_survives_odd_scenes() {
    let picture = RgbImage::from_fn(300, 200, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 120]));
    let analysis = scooch_core::analyze(&picture);
    let mut weird = vec![
        Rect { x: 10.0, y: 10.0, w: 0.0, h: 40.0 },
        Rect { x: f32::NAN, y: 10.0, w: 30.0, h: 40.0 },
        Rect { x: -5000.0, y: -5000.0, w: 30.0, h: 40.0 },
        Rect { x: 1e9, y: 1e9, w: 30.0, h: 40.0 },
        Rect { x: 10.0, y: 10.0, w: -30.0, h: 40.0 },
    ];
    weird.extend(layouts::scattered(1600, 1000, 500, 4));
    for (w, h) in [(16u32, 9000u32), (1600, 1000), (1, 1)] {
        for scenery in [Scenery::Auto, Scenery::Islands, Scenery::Clouds] {
            let scene = Scene { width: w, height: h, icons: weird.clone() };
            let options = Options { scenery, ..Options::default() };
            let placement = scooch_core::solve(&analysis, &scene, &options, None);
            assert_eq!(scooch_core::render_with(&picture, &analysis, &placement, &scene, &options).dimensions(), (w, h));
            assert_eq!(scooch_core::scenery_field(&scene, w, h).dimensions(), (w, h));
        }
    }
}

#[test]
fn old_option_files_load_with_scenery_on() {
    let old: Options = serde_json::from_str(r#"{"strength":1.0,"max_zoom":1.3,"mirror":false,"pads":0.7}"#).unwrap();
    assert_eq!(old.scenery, Scenery::Auto);
    let named: Options = serde_json::from_str(r#"{"scenery":"clouds"}"#).unwrap();
    assert_eq!(named.scenery, Scenery::Clouds);
}

#[test]
fn labels_stay_readable_on_the_scenery() {
    let (w, h) = (960, 600);
    for photo in photos() {
        let picture = small(&photo.image, 1200);
        let analysis = scooch_core::analyze(&picture);
        for layout in ["mac", "windows", "scattered", "cluster"] {
            {
                let icons = layouts::layout(layout, &analysis, w, h);
                let scene = Scene { width: w, height: h, icons: icons.clone() };
                let options = Options::default();
                let placement = scooch_core::solve(&analysis, &scene, &options, None);
                let layers = scooch_core::render_layers(&picture, &analysis, &placement, &scene, &options);
                let mut shown = layers.picture.clone();
                scooch_core::composite(&mut shown, &layers.scenery);
                for r in &icons {
                    if let Some(l) = mean_lightness(&shown, &layers.scenery, r) {
                        assert!(l <= 0.66, "{} {layout} label ground lightness {l:.3}", photo.name);
                    }
                }
            }
        }
    }
}
