#[path = "../tests/support/layouts.rs"]
mod layouts;

use scooch_core::{Options, Scene, Scenery};
use std::time::Instant;

struct Timing(f64, f64);

impl std::fmt::Display for Timing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "min {:7.1} ms  median {:7.1} ms", self.0, self.1)
    }
}

fn median_ms<T>(runs: usize, mut f: impl FnMut() -> T) -> Timing {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let started = Instant::now();
            std::hint::black_box(f());
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    Timing(times[0], times[times.len() / 2])
}

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "fox".into());
    let path = layouts::photos().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == name)).expect("photo");
    let photo = scooch_core::open(&path).expect("open");
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    println!("{} {}x{}", name, photo.width(), photo.height());
    let analysis = scooch_core::analyze(&photo);
    println!("analyze          {}", median_ms(15, || scooch_core::analyze(&photo)));
    for (w, h) in [(2880u32, 1800u32), (5120, 2880)] {
        let crowd = Scene { width: w, height: h, icons: layouts::mac(w, h, 100) };
        println!("solve 100 icons {w}x{h} {}", median_ms(15, || scooch_core::solve(&analysis, &crowd, &options, None)));
        let scattered = Scene { width: w, height: h, icons: layouts::scattered(w, h, 100, 7) };
        println!("solve 100 scattered {w}x{h} {}", median_ms(15, || scooch_core::solve(&analysis, &scattered, &options, None)));
        for layout in ["mac", "scattered"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &analysis, w, h) };
            let placement = scooch_core::solve(&analysis, &scene, &options, None);
            println!("render {layout:9} {w}x{h} {}", median_ms(9, || scooch_core::render(&photo, &placement, &scene, &options)));
        }
        let crowd_placement = scooch_core::solve(&analysis, &crowd, &options, None);
        println!("render 100 icons {w}x{h} {}", median_ms(9, || scooch_core::render(&photo, &crowd_placement, &crowd, &options)));
        let plain = Options { pads: 0.0, scenery: Scenery::Off, ..options };
        for (layout, icons) in [("mac", layouts::mac(w, h, 30)), ("scattered", layouts::scattered(w, h, 30, 7))] {
            let scene = Scene { width: w, height: h, icons };
            let placement = scooch_core::solve(&analysis, &scene, &options, None);
            let off = median_ms(9, || scooch_core::render_with(&photo, &analysis, &placement, &scene, &plain));
            let on = median_ms(9, || scooch_core::render_with(&photo, &analysis, &placement, &scene, &options));
            println!("scenery 30 {layout:9} {w}x{h} off {off} | on {on} | adds {:.1} ms", on.1 - off.1);
        }
        let bare = Scene { width: w, height: h, icons: Vec::new() };
        let fill = scooch_core::fill(photo.width(), photo.height(), &bare);
        println!("render plain {w}x{h} {}", median_ms(9, || scooch_core::render(&photo, &fill, &bare, &options)));
    }
}
