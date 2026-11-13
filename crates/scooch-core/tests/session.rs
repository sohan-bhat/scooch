#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Options, Placement, Rect, RgbImage, Scene, Scenery, Session};
use std::sync::OnceLock;

struct Photo {
    image: RgbImage,
    analysis: Analysis,
}

fn photos() -> &'static [Photo] {
    static CELL: OnceLock<Vec<Photo>> = OnceLock::new();
    CELL.get_or_init(|| {
        layouts::photos()
            .into_iter()
            .step_by(3)
            .take(5)
            .map(|path| {
                let full = scooch_core::open(&path).expect("photo");
                let scale = 900.0 / full.width().max(full.height()) as f32;
                let image = image::imageops::resize(&full, (full.width() as f32 * scale) as u32, (full.height() as f32 * scale) as u32, image::imageops::FilterType::Triangle);
                let analysis = scooch_core::analyze(&image);
                Photo { image, analysis }
            })
            .collect()
    })
}

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn pick(&mut self, n: usize) -> usize {
        ((self.next() * n as f32) as usize).min(n.saturating_sub(1))
    }
}

struct Walk {
    photo: usize,
    scene: Scene,
    options: Options,
    placement: Placement,
}

fn nudge(dice: &mut Dice, r: &mut Rect, w: u32, h: u32) {
    let u = r.h;
    r.x = (r.x + (dice.next() * 2.0 - 1.0) * 3.0 * u).clamp(0.0, w as f32 - r.w);
    r.y = (r.y + (dice.next() * 2.0 - 1.0) * 3.0 * u).clamp(0.0, h as f32 - r.h);
}

fn step(dice: &mut Dice, walk: &mut Walk) -> &'static str {
    let (w, h) = (walk.scene.width, walk.scene.height);
    let icons = &mut walk.scene.icons;
    let kind = dice.pick(10);
    match kind {
        0..=3 if !icons.is_empty() => {
            let k = dice.pick(icons.len());
            nudge(dice, &mut icons[k], w, h);
            "move"
        }
        4 => {
            let size = icons.first().map_or(Rect { x: 0.0, y: 0.0, w: 40.0, h: 50.0 }, |r| *r);
            let at = dice.pick(icons.len() + 1);
            icons.insert(at, Rect { x: dice.next() * (w as f32 - size.w), y: dice.next() * (h as f32 - size.h), ..size });
            "add"
        }
        5 if icons.len() > 1 => {
            let k = dice.pick(icons.len());
            icons.remove(k);
            "remove"
        }
        6 if !icons.is_empty() => {
            for _ in 0..2 + dice.pick(3) {
                let k = dice.pick(icons.len());
                nudge(dice, &mut icons[k], w, h);
            }
            "several"
        }
        7 => {
            let photo = &photos()[walk.photo];
            walk.placement = if dice.next() < 0.5 { scooch_core::fill(photo.analysis.source_width, photo.analysis.source_height, &walk.scene) } else { scooch_core::solve(&photo.analysis, &walk.scene, &walk.options, None) };
            "placement"
        }
        8 => {
            walk.options = match dice.pick(4) {
                0 => Options { scenery: if walk.options.scenery == Scenery::Off { Scenery::Auto } else { Scenery::Off }, ..walk.options },
                1 => Options { scenery: Scenery::Islands, ..walk.options },
                2 => Options { pads: if walk.options.pads > 0.0 { 0.0 } else { 0.7 }, ..walk.options },
                _ => Options { scenery: Scenery::Clouds, ..walk.options },
            };
            "option"
        }
        _ => {
            walk.photo = (walk.photo + 1 + dice.pick(photos().len() - 1)) % photos().len();
            "picture"
        }
    }
}

fn walk(seed: u64, steps: usize, photo: usize, layout: &str, scenery: Scenery, (w, h): (u32, u32)) {
    let mut dice = Dice(seed);
    let first = &photos()[photo];
    let options = Options { scenery, ..Options::default() };
    let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &first.analysis, w, h) };
    let placement = scooch_core::solve(&first.analysis, &scene, &options, None);
    let mut walk = Walk { photo, scene, options, placement };
    let mut session = Session::new();
    let mut most = 0;
    for n in 0..=steps {
        let what = if n == 0 { "first" } else { step(&mut dice, &mut walk) };
        let photo = &photos()[walk.photo];
        let (bundle, still) = session.still(&photo.image, &photo.analysis, &walk.placement, &walk.scene, &walk.options);
        let (want_bundle, want_still) = scooch_core::still(&photo.image, &photo.analysis, &walk.placement, &walk.scene, &walk.options);
        assert!(still.as_raw() == want_still.as_raw(), "seed {seed} step {n} {what}: image differs");
        assert!(bundle == want_bundle, "seed {seed} step {n} {what}: bundle differs");
        most = most.max(session.bytes());
    }
    assert!(most < 24 * (w * h) as usize + 8 * 1024 * 1024, "{most}");
}

#[test]
fn random_walks_match_the_reference_still() {
    let mut seed = 7;
    for photo in 0..photos().len() {
        for layout in ["mac", "scattered", "cluster"] {
            for scenery in [Scenery::Auto, Scenery::Off] {
                walk(seed, 40, photo, layout, scenery, (640, 400));
                seed += 1;
            }
        }
    }
}

#[test]
#[ignore]
fn a_long_walk_matches_the_reference_still() {
    walk(2027, 2000, 0, "scattered", Scenery::Auto, (1440, 900));
}

#[test]
fn single_moves_change_only_what_the_session_redraws() {
    let (w, h) = (960, 600);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    for photo in photos() {
        for layout in ["mac", "scattered", "cluster"] {
            let icons = layouts::layout(layout, &photo.analysis, w, h);
            let scene = Scene { width: w, height: h, icons: icons.clone() };
            let placement = scooch_core::solve(&photo.analysis, &scene, &options, None);
            let before = scooch_core::still(&photo.image, &photo.analysis, &placement, &scene, &options).1;
            for k in [0, icons.len() / 3, 2 * icons.len() / 3] {
                let mut session = Session::new();
                session.still(&photo.image, &photo.analysis, &placement, &scene, &options);
                let mut moved = icons.clone();
                let r = &mut moved[k];
                r.x = if r.x + 2.0 * r.w < w as f32 { r.x + r.w } else { r.x - r.w };
                let next = Scene { icons: moved, ..scene.clone() };
                let (_, shown) = session.still(&photo.image, &photo.analysis, &placement, &next, &options);
                let after = scooch_core::still(&photo.image, &photo.analysis, &placement, &next, &options).1;
                assert!(shown.as_raw() == after.as_raw());
                for (i, (p, q)) in before.pixels().zip(after.pixels()).enumerate() {
                    if p != q {
                        let (x, y) = (i as u32 % w, i as u32 / w);
                        assert!(session.redrawn(x, y), "{layout} icon {k}: pixel {x},{y} changed outside the redrawn cells");
                    }
                }
            }
        }
    }
}

#[test]
fn a_session_at_2880_holds_under_eighty_megabytes() {
    let path = &layouts::photos()[0];
    let image = scooch_core::open(path).expect("photo");
    let analysis = scooch_core::analyze(&image);
    let (w, h) = (2880, 1800);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let mut scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &analysis, w, h) };
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    let mut session = Session::new();
    for k in 0..3 {
        session.still(&image, &analysis, &placement, &scene, &options);
        assert!(session.bytes() < 80 * 1024 * 1024, "{}", session.bytes());
        scene.icons[k].x += scene.icons[k].w;
    }
}

#[test]
fn a_session_after_forget_or_an_empty_call_starts_clean() {
    let photo = &photos()[1];
    let (w, h) = (640, 400);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &photo.analysis, w, h) };
    let placement = scooch_core::solve(&photo.analysis, &scene, &options, None);
    let want = scooch_core::still(&photo.image, &photo.analysis, &placement, &scene, &options);
    let mut session = Session::new();
    assert_eq!(session.bytes(), 0);
    let first = session.still(&photo.image, &photo.analysis, &placement, &scene, &options);
    assert!(session.bytes() > 0);
    session.forget();
    assert_eq!(session.bytes(), 0);
    let again = session.still(&photo.image, &photo.analysis, &placement, &scene, &options);
    let empty = Scene { width: 0, height: 0, icons: Vec::new() };
    let nothing = session.still(&photo.image, &photo.analysis, &placement, &empty, &options);
    assert_eq!((nothing.0.width, nothing.1.width()), (0, 0));
    let blank = session.still(&RgbImage::new(0, 0), &photo.analysis, &placement, &scene, &options);
    assert_eq!(blank.1.width(), w);
    let last = session.still(&photo.image, &photo.analysis, &placement, &scene, &options);
    for (bundle, still) in [first, again, last] {
        assert!(bundle == want.0 && still.as_raw() == want.1.as_raw());
    }
}
