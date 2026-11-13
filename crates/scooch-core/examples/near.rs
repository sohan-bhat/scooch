#[path = "../tests/support/layouts.rs"]
mod layouts;

use scooch_core::{Options, Scene, Scenery};
use std::time::{Duration, Instant};

fn main() {
    let (w, h) = (2880u32, 1800u32);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let budget = Duration::from_millis(8);
    let (mut agree_kept, mut agree_fresh, mut total) = (0usize, 0usize, 0usize);
    let mut times = Vec::new();
    for path in layouts::photos() {
        let photo = scooch_core::open(&path).expect("photo");
        let analysis = scooch_core::analyze(&photo);
        for layout in ["mac", "scattered", "cluster"] {
            let icons = layouts::layout(layout, &analysis, w, h);
            let scene = Scene { width: w, height: h, icons: icons.clone() };
            let previous = scooch_core::solve(&analysis, &scene, &options, None);
            for k in [0, icons.len() / 3, 2 * icons.len() / 3] {
                let mut moved = icons.clone();
                let r = &mut moved[k];
                r.x = if r.x + 2.0 * r.w < w as f32 { r.x + r.w } else { r.x - r.w };
                let next = Scene { icons: moved, ..scene.clone() };
                let kept = scooch_core::solve(&analysis, &next, &options, Some(&previous));
                let fresh = scooch_core::solve(&analysis, &next, &options, None);
                let started = Instant::now();
                let near = scooch_core::solve_near(&analysis, &next, &options, &previous, budget);
                times.push(started.elapsed().as_secs_f64() * 1000.0);
                total += 1;
                agree_kept += (near == kept) as usize;
                agree_fresh += (near == fresh) as usize;
            }
        }
    }
    times.sort_by(f64::total_cmp);
    println!(
        "moves {total} | same as solve with previous {agree_kept} ({:.0}%) | same as fresh solve {agree_fresh} ({:.0}%) | median {:.1} ms p90 {:.1} ms max {:.1} ms",
        100.0 * agree_kept as f64 / total as f64,
        100.0 * agree_fresh as f64 / total as f64,
        times[times.len() / 2],
        times[times.len() * 9 / 10],
        times[times.len() - 1]
    );
}
