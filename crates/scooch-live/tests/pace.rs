#![cfg(target_os = "macos")]

use objc2::rc::{autoreleasepool, Retained};
use objc2_quartz_core::CAMetalLayer;
use scooch_core::{Options, Rect, Scene, Scenery};
use scooch_live::{Gpu, Look, Target, Took};
use std::path::Path;
use std::time::{Duration, Instant};

const SIZES: [(u32, u32); 2] = [(2880, 1800), (1920, 1080)];
const FRAMES: usize = 300;
const GLIDE: usize = 30;
const LEAD: usize = 5;
const BUDGET: Duration = Duration::from_secs(10);
const SLOW: f64 = 100.0;
const PERIOD: Duration = Duration::from_micros(16_667);

fn icons(width: u32, height: u32, moved: bool) -> Vec<Rect> {
    let unit = height as f32 / 10.6;
    let (w, h) = (unit * 0.88, unit);
    (0..12)
        .map(|index| {
            let (column, row) = ((index / 6) as f32, (index % 6) as f32);
            let x = width as f32 - (column + 1.0) * (w + unit * 0.3);
            let y = row * (h + unit * 0.4) + unit * 0.3;
            if moved && index == 7 {
                Rect { x: width as f32 * 0.52, y: height as f32 * 0.44, w, h }
            } else {
                Rect { x, y, w, h }
            }
        })
        .collect()
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ms(time: Duration) -> f64 {
    time.as_secs_f64() * 1000.0
}

fn spread(values: &[f64]) -> String {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |share: f64| sorted.get(((sorted.len().saturating_sub(1)) as f64 * share).round() as usize).copied().unwrap_or(0.0);
    format!("median {:.1}, 90th {:.1}, most {:.1}", at(0.5), at(0.9), sorted.last().copied().unwrap_or(0.0))
}

fn pace(gpu: &Gpu, (width, height): (u32, u32)) -> Result<String, String> {
    let began = Instant::now();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/photos/balloon-sky.jpg");
    let image = scooch_core::open(&path).map_err(|error| format!("photo {error}"))?;
    let analysis = scooch_core::analyze(&image);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let (before, after) = (Scene { width, height, icons: icons(width, height, false) }, Scene { width, height, icons: icons(width, height, true) });
    let first = scooch_core::solve(&analysis, &before, &options, None);
    let second = scooch_core::solve(&analysis, &after, &options, Some(&first));
    let (a, b) = (scooch_core::bundle(&image, &analysis, &first, &before, &options), scooch_core::bundle(&image, &analysis, &second, &after, &options));
    let picture = gpu.try_picture(&scooch_core::canvas(&image, width, height))?;
    let glide = gpu.glide(&picture, &a, &b)?;
    let shown = gpu.snapshot_at(width, height, &[(Look::Glide(&glide, 0.0), 1.0)], 1.0)?;
    let still = gpu.still(&shown)?;
    let _ = gpu.time(width, height, &[(Look::Glide(&glide, 0.5), 1.0)])?;
    let heaviest = gpu.time(width, height, &[(Look::Glide(&glide, 0.5), 1.0)])?;
    let scale = scooch_live::choose(heaviest, PERIOD);
    let layer = CAMetalLayer::new();
    layer.setOpaque(false);
    let mut screen = gpu.screen(Target::Layer(Retained::as_ptr(&layer) as *mut std::ffi::c_void), width, height)?;
    screen.scale = scale;
    let ready = began.elapsed();
    let started = Instant::now();
    let mut each: Vec<(Took, Duration)> = Vec::new();
    for index in 0..FRAMES {
        if started.elapsed() > BUDGET {
            break;
        }
        let step = index % GLIDE;
        let s = ease(step as f32 / (GLIDE - 1) as f32);
        let layers = if step < LEAD { vec![(Look::Still(&still), 1.0), (Look::Glide(&glide, s), (step + 1) as f32 / LEAD as f32)] } else { vec![(Look::Glide(&glide, s), 1.0)] };
        let framed = Instant::now();
        autoreleasepool(|_| screen.show(&layers))?;
        each.push((screen.took, framed.elapsed()));
    }
    let shown_for = started.elapsed();
    drop(screen);
    let part = |pick: fn(&(Took, Duration)) -> Duration| spread(&each.iter().map(|frame| ms(pick(frame))).collect::<Vec<_>>());
    let slow = each.iter().filter(|(_, whole)| ms(*whole) > SLOW).count();
    Ok(format!(
        "pace {} of kind {} at {width}x{height}, the chooser's scale {scale} from a heaviest frame of {:.1} ms, set up in {:.1} s, {} frames shown in {:.1} s, in ms: drawable {}, draw {}, present {}, of which the turn {}, submit {}, scope pop {}, queue present {}, the whole frame {}, frames over {SLOW} ms {slow}",
        gpu.name,
        gpu.kind,
        ms(heaviest),
        ready.as_secs_f64(),
        each.len(),
        shown_for.as_secs_f64(),
        part(|(took, _)| took.acquire),
        part(|(took, _)| took.draw),
        part(|(took, _)| took.present),
        part(|(took, _)| took.split.turn),
        part(|(took, _)| took.split.submit),
        part(|(took, _)| took.split.pop),
        part(|(took, _)| took.split.queue),
        part(|(_, whole)| *whole),
    ))
}

#[test]
#[ignore = "an offscreen pace measure, run by name with --ignored"]
fn a_glide_on_a_layer_in_no_window_keeps_its_pace() {
    let gpu = match Gpu::new(false) {
        Ok(gpu) => gpu,
        Err(why) => {
            assert!(std::env::var_os("SCOOCH_NEED_ADAPTER").is_none(), "no adapter, and this runner must have one");
            println!("skipped: no adapter {why}");
            return;
        }
    };
    for size in SIZES {
        match pace(&gpu, size) {
            Ok(said) => println!("{said}"),
            Err(why) => panic!("the pace at {}x{} was not measured, {why}", size.0, size.1),
        }
    }
}
