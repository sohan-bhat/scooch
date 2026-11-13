#![cfg(any(windows, target_os = "macos"))]

use scooch_core::{Bundle, Options, Rect, RgbImage, Scene, Scenery};
use scooch_live::{Gpu, Look};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 600;

fn grid(right: bool) -> Vec<Rect> {
    (0..12)
        .map(|i| {
            let (column, row) = ((i / 5) as f32, (i % 5) as f32);
            let x = if right { WIDTH as f32 - (column + 1.0) * 76.0 } else { column * 76.0 + 2.0 };
            Rect { x, y: row * 100.0 + 4.0, w: 72.0, h: 84.0 }
        })
        .collect()
}

fn moved(icons: &[Rect], index: usize, dx: f32, dy: f32) -> Vec<Rect> {
    let mut out = icons.to_vec();
    out[index].x = (out[index].x + dx).clamp(0.0, WIDTH as f32 - out[index].w);
    out[index].y = (out[index].y + dy).clamp(0.0, HEIGHT as f32 - out[index].h);
    out
}

fn compare(gpu: &RgbImage, cpu: &RgbImage) -> (f64, f64, u8) {
    let (mut sum, mut off, mut worst) = (0u64, 0usize, 0u8);
    for (p, q) in gpu.as_raw().as_chunks::<3>().0.iter().zip(cpu.as_raw().as_chunks::<3>().0.iter()) {
        let largest = (0..3).map(|c| p[c].abs_diff(q[c])).max().unwrap_or(0);
        sum += (0..3).map(|c| u64::from(p[c].abs_diff(q[c]))).sum::<u64>();
        worst = worst.max(largest);
        if largest > 8 {
            off += 1;
        }
    }
    let pixels = (gpu.width() * gpu.height()) as f64;
    (sum as f64 / pixels / 3.0, off as f64 / pixels, worst)
}

fn device() -> Option<MutexGuard<'static, Gpu>> {
    static SHARED: OnceLock<Option<Mutex<Gpu>>> = OnceLock::new();
    let shared = SHARED.get_or_init(|| match Gpu::new(false) {
        Ok(gpu) => Some(Mutex::new(gpu)),
        Err(error) => {
            println!("skipped: no adapter {error}");
            None
        }
    });
    assert!(shared.is_some() || std::env::var_os("SCOOCH_NEED_ADAPTER").is_none(), "no adapter, and this runner must have one");
    shared.as_ref().map(|gpu| gpu.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

fn states(name: &str, pairs: &[(Vec<Rect>, Vec<Rect>)], options: &Options) -> (RgbImage, Vec<(Bundle, Bundle)>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/photos").join(name);
    let full = scooch_core::open(&path).expect("photo");
    let small = scooch_core::canvas(&full, 600, 600);
    let image = RgbImage::from_raw(small.width, small.height, small.pixels).expect("image");
    let analysis = scooch_core::analyze(&image);
    let made = pairs
        .iter()
        .map(|(first, second)| {
            let (one, two) = (Scene { width: WIDTH, height: HEIGHT, icons: first.clone() }, Scene { width: WIDTH, height: HEIGHT, icons: second.clone() });
            let a = scooch_core::solve(&analysis, &one, options, None);
            let b = scooch_core::solve(&analysis, &two, options, Some(&a));
            (scooch_core::bundle(&image, &analysis, &a, &one, options), scooch_core::bundle(&image, &analysis, &b, &two, options))
        })
        .collect();
    (image, made)
}

fn measure(gpu: &Gpu, options: &Options, label: &str, scale: f32) -> (f64, f64) {
    let pairs = [(grid(false), moved(&grid(false), 7, 380.0, 120.0)), (grid(true), moved(&grid(true), 2, -300.0, 260.0))];
    let mut worst = (0.0f64, 0.0f64);
    for name in ["balloon-sky.jpg", "fox.jpg"] {
        let (image, made) = states(name, &pairs, options);
        let picture = gpu.picture(&scooch_core::canvas(&image, WIDTH, HEIGHT));
        for (index, (a, b)) in made.iter().enumerate() {
            let glide = gpu.glide(&picture, a, b).expect("glide");
            for s in [0.0f32, 0.37, 1.0] {
                let drawn = gpu.snapshot_at(WIDTH, HEIGHT, &[(Look::Glide(&glide, s), 1.0)], scale).expect("snapshot");
                let (mean, off, largest) = compare(&drawn, &scooch_core::frame(a, b, s));
                println!("{label} at scale {scale} {name} pair {index} s {s}: mean {mean:.3}/255, off by more than 8 {:.3}%, largest {largest}", off * 100.0);
                worst = (worst.0.max(mean), worst.1.max(off));
            }
        }
    }
    println!("{label} worst mean {:.3}/255, worst share off by more than 8 {:.3}%", worst.0, worst.1 * 100.0);
    worst
}

#[test]
fn the_gpu_glide_matches_the_cpu_frame() {
    let Some(gpu) = device() else { return };
    println!("adapter {}", gpu.name);
    let plain = measure(&gpu, &Options { pads: 0.0, ..Options::default() }, "picture only", 1.0);
    assert!(plain.0 <= 1.5 && plain.1 <= 0.005, "the picture is too far from the CPU frame: mean {:.3}/255, share {:.3}%", plain.0, plain.1 * 100.0);
    let scenic = measure(&gpu, &Options { scenery: Scenery::Auto, ..Options::default() }, "with scenery", 1.0);
    assert!(scenic.0 <= 1.5 && scenic.1 <= 0.005, "the scenery frame is too far from the CPU frame: mean {:.3}/255, share {:.3}%", scenic.0, scenic.1 * 100.0);
}

#[test]
fn smaller_moving_frames_stay_close_to_the_cpu_frame() {
    let Some(gpu) = device() else { return };
    for scale in [0.75f32, 0.6, 0.5] {
        let worst = measure(&gpu, &Options { scenery: Scenery::Auto, ..Options::default() }, "with scenery", scale);
        assert!(worst.0 <= 3.0, "at scale {scale} the frame is too far from the CPU frame: mean {:.3}/255", worst.0);
    }
}

#[test]
fn a_still_lands_exactly_while_the_glide_is_scaled() {
    let Some(gpu) = device() else { return };
    let raw: Vec<u8> = (0..61u32).flat_map(|y| (0..97u32).flat_map(move |x| [(x * 3 % 256) as u8, (y * 11 % 256) as u8, ((x * y) % 256) as u8])).collect();
    let image = RgbImage::from_raw(97, 61, raw).expect("image");
    let still = gpu.still(&image).expect("still");
    for scale in scooch_live::SCALES {
        assert_eq!(gpu.snapshot_at(97, 61, &[(Look::Still(&still), 1.0)], scale).expect("snapshot"), image, "at scale {scale}");
    }
}

#[test]
fn the_chooser_fits_the_frame_in_the_period() {
    let period = Duration::from_micros(16_667);
    assert!(scooch_live::choose(Duration::from_millis(15), period) <= 0.75);
    assert_eq!(scooch_live::choose(Duration::from_millis(5), period), 1.0);
    assert_eq!(scooch_live::choose(Duration::from_millis(80), period), 0.5);
    assert_eq!(scooch_live::step(1.0, 20, 3, Duration::from_millis(9), period), 0.75);
    assert_eq!(scooch_live::step(0.6, 20, 0, Duration::from_millis(5), period), 0.75);
    assert_eq!(scooch_live::step(0.75, 20, 1, Duration::from_millis(9), period), 0.75);
    assert_eq!(scooch_live::step(0.5, 20, 5, Duration::from_millis(30), period), 0.5);
}

#[test]
fn the_chooser_picks_a_scale_on_this_device() {
    let Some(gpu) = device() else { return };
    let image = RgbImage::from_raw(64, 40, vec![100u8; 64 * 40 * 3]).expect("image");
    let picture = gpu.picture(&scooch_core::canvas(&image, 64, 40));
    for (width, height) in [(1024u32, 768u32), (2880, 1800)] {
        let heavy = scooch_live::heavy(width, height, &picture);
        let glide = gpu.glide(&picture, &heavy, &heavy).expect("glide");
        let _ = gpu.time(width, height, &[(Look::Glide(&glide, 0.5), 1.0)]);
        let took = gpu.time(width, height, &[(Look::Glide(&glide, 0.5), 1.0)]).expect("time");
        println!("{} at {width}x{height}: the heaviest frame took {took:?}, the chooser picks {} at 60 Hz", gpu.name, scooch_live::choose(took, Duration::from_micros(16_667)));
    }
}

#[test]
fn a_frame_can_be_timed_without_a_readback() {
    let Some(gpu) = device() else { return };
    let raw = vec![90u8; 64 * 48 * 3];
    let still = gpu.still(&RgbImage::from_raw(64, 48, raw).expect("image")).expect("still");
    let took = gpu.time(64, 48, &[(Look::Still(&still), 1.0)]).expect("time");
    println!("one still frame at 64x48 took {took:?}");
    assert!(took < std::time::Duration::from_secs(10));
}

#[test]
fn a_still_lands_on_the_pixels_exactly() {
    let Some(gpu) = device() else { return };
    let raw: Vec<u8> = (0..61u32).flat_map(|y| (0..97u32).flat_map(move |x| [(x * 7 % 256) as u8, (y * 13 % 256) as u8, ((x + y) * 5 % 256) as u8])).collect();
    let image = RgbImage::from_raw(97, 61, raw).expect("image");
    let still = gpu.still(&image).expect("still");
    let drawn = gpu.snapshot(97, 61, &[(Look::Still(&still), 1.0)]).expect("snapshot");
    assert_eq!(drawn, image);
}

#[test]
fn a_mirrored_pair_cross_fades_like_the_cpu() {
    let Some(gpu) = device() else { return };
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/photos/balloon-sky.jpg");
    let full = scooch_core::open(&path).expect("photo");
    let small = scooch_core::canvas(&full, 600, 600);
    let image = RgbImage::from_raw(small.width, small.height, small.pixels).expect("image");
    let analysis = scooch_core::analyze(&image);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let scene = Scene { width: WIDTH, height: HEIGHT, icons: grid(false) };
    let a = scooch_core::solve(&analysis, &scene, &options, None);
    let mut b = a.clone();
    b.fx.iter_mut().for_each(|value| *value = 1.0 - *value);
    b.mirrored = !a.mirrored;
    let (first, second) = (scooch_core::bundle(&image, &analysis, &a, &scene, &options), scooch_core::bundle(&image, &analysis, &b, &scene, &options));
    if scooch_core::compatible(&first, &second) {
        println!("the mirrored pair counts as compatible, so there is nothing to cross-fade");
        return;
    }
    let picture = gpu.picture(&scooch_core::canvas(&image, WIDTH, HEIGHT));
    let glide = gpu.glide(&picture, &first, &second).expect("glide");
    for s in [0.0f32, 0.37, 1.0] {
        let drawn = gpu.snapshot(WIDTH, HEIGHT, &[(Look::Glide(&glide, s), 1.0)]).expect("snapshot");
        let (mean, off, largest) = compare(&drawn, &scooch_core::frame(&first, &second, s));
        println!("mirrored s {s}: mean {mean:.3}/255, off by more than 8 {:.3}%, largest {largest}", off * 100.0);
        assert!(mean <= 1.5 && off <= 0.005, "the mirrored cross-fade is too far from the CPU frame: mean {mean:.3}/255, share {:.3}%", off * 100.0);
    }
}
