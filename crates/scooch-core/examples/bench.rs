#[path = "../tests/support/layouts.rs"]
mod layouts;

use scooch_core::{Options, Scene};
use std::time::Instant;

fn median_ms<T>(runs: usize, mut f: impl FnMut() -> T) -> f64 {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let started = Instant::now();
            std::hint::black_box(f());
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "fox".into());
    let path = layouts::photos().into_iter().find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == name)).expect("photo");
    let photo = scooch_core::open(&path).expect("open");
    let options = Options::default();
    println!("{} {}x{}", name, photo.width(), photo.height());
    let analysis = scooch_core::analyze(&photo);
    println!("analyze          {:7.1} ms", median_ms(15, || scooch_core::analyze(&photo)));
    for (w, h) in [(2880u32, 1800u32), (5120, 2880)] {
        let crowd = Scene { width: w, height: h, icons: layouts::mac(w, h, 100) };
        println!("solve 100 icons {w}x{h} {:7.1} ms", median_ms(15, || scooch_core::solve(&analysis, &crowd, &options, None)));
        let scattered = Scene { width: w, height: h, icons: layouts::scattered(w, h, 100, 7) };
        println!("solve 100 scattered {w}x{h} {:7.1} ms", median_ms(15, || scooch_core::solve(&analysis, &scattered, &options, None)));
        for layout in ["mac", "scattered"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &analysis, w, h) };
            let placement = scooch_core::solve(&analysis, &scene, &options, None);
            println!("render {layout:9} {w}x{h} {:7.1} ms", median_ms(9, || scooch_core::render(&photo, &placement, &scene, &options)));
        }
        let crowd_placement = scooch_core::solve(&analysis, &crowd, &options, None);
        println!("render 100 icons {w}x{h} {:7.1} ms", median_ms(9, || scooch_core::render(&photo, &crowd_placement, &crowd, &options)));
        let bare = Scene { width: w, height: h, icons: Vec::new() };
        let fill = scooch_core::fill(photo.width(), photo.height(), &bare);
        println!("render plain {w}x{h} {:7.1} ms", median_ms(9, || scooch_core::render(&photo, &fill, &bare, &options)));
    }
}
