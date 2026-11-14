#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Options, Placement, Rect, RgbImage, Scene, Scenery, Session};
use std::sync::OnceLock;

fn balloon() -> &'static (RgbImage, Analysis) {
    static CELL: OnceLock<(RgbImage, Analysis)> = OnceLock::new();
    CELL.get_or_init(|| {
        let path = layouts::photos().into_iter().find(|p| p.file_stem().unwrap() == "balloon-sky").expect("balloon");
        let image = scooch_core::open(&path).expect("open");
        let analysis = scooch_core::analyze(&image);
        (image, analysis)
    })
}

fn inverse(table: &[f32], x: f64) -> f64 {
    let steps = (table.len() - 1) as f64;
    let rising = table[table.len() - 1] >= table[0];
    let at = |k: usize| if rising { table[k] as f64 } else { -(table[k] as f64) };
    let x = if rising { x } else { -x };
    if x <= at(0) {
        return 0.0;
    }
    if x >= at(table.len() - 1) {
        return 1.0;
    }
    let k = (0..table.len() - 1).find(|k| at(*k + 1) >= x).unwrap_or(table.len() - 2);
    let span = (at(k + 1) - at(k)).max(1e-12);
    let u = (k as f64 + (x - at(k)) / span) / steps;
    if rising { u } else { 1.0 - u }
}

fn solid_box(analysis: &Analysis, placement: &Placement) -> [f64; 4] {
    let (w, h) = (analysis.width as usize, analysis.height as usize);
    let mut out = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for (i, s) in analysis.saliency.iter().enumerate() {
        if *s < 0.5 {
            continue;
        }
        let (x, y) = ((i % w) as f64, (i / w) as f64);
        let (xa, xb) = (inverse(&placement.fx, x / w as f64), inverse(&placement.fx, (x + 1.0) / w as f64));
        let (ya, yb) = (inverse(&placement.fy, y / h as f64), inverse(&placement.fy, (y + 1.0) / h as f64));
        out = [out[0].min(xa.min(xb)), out[1].min(ya.min(yb)), out[2].max(xa.max(xb)), out[3].max(ya.max(yb))];
    }
    out
}

fn solid_inside(analysis: &Analysis, placement: &Placement) -> f64 {
    let (w, h) = (analysis.width as usize, analysis.height as usize);
    let span = |t: &[f32]| (t[0].min(t[t.len() - 1]) as f64, t[0].max(t[t.len() - 1]) as f64);
    let ((x0, x1), (y0, y1)) = (span(&placement.fx), span(&placement.fy));
    let (mut inside, mut total) = (0.0, 0.0);
    for (i, s) in analysis.saliency.iter().enumerate() {
        if *s < 0.5 {
            continue;
        }
        let (cx, cy) = ((i % w) as f64 / w as f64, (i / w) as f64 / h as f64);
        let (cw, ch) = (1.0 / w as f64, 1.0 / h as f64);
        let ox = ((cx + cw).min(x1) - cx.max(x0)).max(0.0) / cw;
        let oy = ((cy + ch).min(y1) - cy.max(y0)).max(0.0) / ch;
        inside += *s as f64 * ox * oy;
        total += *s as f64;
    }
    if total > 0.0 { inside / total } else { 1.0 }
}

fn row(count: usize) -> Vec<Rect> {
    (0..count).map(|k| Rect { x: 25.0 + 170.0 * k as f32, y: 1710.0, w: 150.0, h: 150.0 }).collect()
}

fn column(count: usize) -> Vec<Rect> {
    (0..count).map(|k| Rect { x: 2350.0, y: 35.0 + 170.0 * k as f32, w: 150.0, h: 150.0 }).collect()
}

#[test]
fn the_upright_balloon_rests_whole_and_centered() {
    let (_, analysis) = balloon();
    for icons in [row(6), Vec::new()] {
        let scene = Scene { width: 1080, height: 1920, icons };
        for scenery in [Scenery::Auto, Scenery::Off] {
            let options = Options { scenery, ..Options::default() };
            let placement = scooch_core::solve(analysis, &scene, &options, None);
            let b = solid_box(analysis, &placement);
            eprintln!("upright {} icons {scenery:?}: solid box {:.3} {:.3} {:.3} {:.3} of the screen, inside {:.3}", scene.icons.len(), b[0], b[1], b[2] - b[0], b[3] - b[1], solid_inside(analysis, &placement));
            assert!(b[0] >= 0.08 && b[2] <= 0.92 && b[1] >= 0.0 && b[3] <= 1.0, "{b:?}");
            let hidden = scooch_core::hidden_share(analysis, &scene, &placement);
            eprintln!("hidden share {hidden}");
            assert!(hidden < 1e-3, "{hidden}");
            for r in &scene.icons {
                let (x0, x1) = (r.x as f64 / 1080.0, (r.x + r.w) as f64 / 1080.0);
                let (y0, y1) = (r.y as f64 / 1920.0, (r.y + r.h) as f64 / 1920.0);
                assert!(b[2] <= x0 || b[0] >= x1 || b[3] <= y0 || b[1] >= y1, "the solid subject meets an icon");
            }
        }
    }
}

#[test]
fn the_wide_balloon_keeps_its_basket_clear_of_the_edges() {
    let (image, analysis) = balloon();
    let scene = Scene { width: 2560, height: 1080, icons: column(6) };
    for scenery in [Scenery::Auto, Scenery::Off] {
        let placement = scooch_core::solve(analysis, &scene, &Options { scenery, ..Options::default() }, None);
        let still = scooch_core::render_with(image, analysis, &placement, &scene, &Options::default());
        let (mut top, mut bottom) = (u32::MAX, 0u32);
        for (x, y, p) in still.enumerate_pixels() {
            if x < 2300 && (p[2] as i32 - p[0] as i32) < 25 {
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
        eprintln!("wide {scenery:?}: balloon pixels from y {top} to {bottom} of 1080");
        assert!(top as f64 >= 0.04 * 1080.0 && bottom as f64 <= 0.96 * 1080.0, "{top} {bottom}");
    }
}

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[test]
fn home_ignores_the_icons() {
    let (_, analysis) = balloon();
    for (w, h) in [(1280u32, 960u32), (2880, 1800), (2560, 1080), (1080, 1920), (1600, 1600)] {
        let rest = scooch_core::home(analysis, &Scene { width: w, height: h, icons: Vec::new() });
        let mut dice = Dice(w as u64 * 7 + h as u64);
        for _ in 0..10 {
            let icons = (0..1 + (dice.next() * 12.0) as usize).map(|_| Rect { x: dice.next() * (w as f32 - 80.0), y: dice.next() * (h as f32 - 90.0), w: 76.0, h: 90.0 }).collect();
            assert_eq!(scooch_core::home(analysis, &Scene { width: w, height: h, icons }), rest, "{w}x{h}");
        }
        assert_eq!(scooch_core::solve(analysis, &Scene { width: w, height: h, icons: Vec::new() }, &Options::default(), None), rest);
    }
}

#[test]
fn an_upright_walk_keeps_what_home_shows() {
    let (image, analysis) = balloon();
    let (w, h) = (1080u32, 1920u32);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let mut scene = Scene { width: w, height: h, icons: row(6) };
    let rest = scooch_core::home(analysis, &scene);
    let floor = solid_inside(analysis, &rest);
    let mut placement = scooch_core::solve(analysis, &scene, &options, None);
    let mut session = Session::new();
    let mut dice = Dice(1080);
    let (mut held, mut cut, mut steps) = (0usize, 0usize, 0usize);
    for n in 0..20 {
        let drop = n % 4 == 3;
        if !drop {
            if n % 4 == 0 {
                held = (dice.next() * 6.0) as usize % 6;
            }
            let r = &mut scene.icons[held];
            r.x = dice.next() * (w as f32 - r.w);
            r.y = dice.next() * (h as f32 - r.h);
        }
        placement = if drop { scooch_core::solve(analysis, &scene, &options, Some(&placement)) } else { scooch_core::solve_near(analysis, &scene, &options, &placement, std::time::Duration::from_secs(60)) };
        let tables = session.tables_shared(image, analysis, &placement, &scene, &options);
        assert!(*tables == scooch_core::bundle(image, analysis, &placement, &scene, &options), "step {n}");
        let shown = solid_inside(analysis, &placement);
        steps += 1;
        if shown < floor - 1e-9 {
            cut += 1;
            assert!(scooch_core::hidden_share(analysis, &scene, &placement) < scooch_core::hidden_share(analysis, &scene, &rest), "step {n}: the subject was cut without clearing an icon");
        }
    }
    eprintln!("the walk cut the subject on {cut} of {steps} steps, each time to clear an icon off it");
}
