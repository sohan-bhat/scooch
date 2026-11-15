#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Bundle, Options, Placement, Rect, RgbImage, Scene, Scenery, Session};
use std::sync::Arc;
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
    let mut kept: Option<(Arc<Bundle>, Arc<RgbImage>, Bundle, RgbImage)> = None;
    for n in 0..=steps {
        let what = if n == 0 { "first" } else { step(&mut dice, &mut walk) };
        let photo = &photos()[walk.photo];
        let (want_bundle, want_still) = scooch_core::still(&photo.image, &photo.analysis, &walk.placement, &walk.scene, &walk.options);
        let mode = dice.next();
        if mode < 0.3 {
            let bundle = session.tables_shared(&photo.image, &photo.analysis, &walk.placement, &walk.scene, &walk.options);
            assert!(*bundle == want_bundle, "seed {seed} step {n} {what}: tables differ");
            if let Some((bundle, still, was_bundle, was_still)) = &kept {
                assert!(**bundle == *was_bundle && still.as_raw() == was_still.as_raw(), "seed {seed} step {n} {what}: a held result changed");
            }
        } else if mode < 0.65 {
            let (bundle, still) = session.still_shared(&photo.image, &photo.analysis, &walk.placement, &walk.scene, &walk.options);
            assert!(still.as_raw() == want_still.as_raw(), "seed {seed} step {n} {what}: shared image differs");
            assert!(*bundle == want_bundle, "seed {seed} step {n} {what}: shared bundle differs");
            if let Some((bundle, still, was_bundle, was_still)) = &kept {
                assert!(**bundle == *was_bundle && still.as_raw() == was_still.as_raw(), "seed {seed} step {n} {what}: a held result changed");
            }
            kept = (dice.next() < 0.7).then_some((bundle, still, want_bundle, want_still));
        } else {
            let (bundle, still) = session.still(&photo.image, &photo.analysis, &walk.placement, &walk.scene, &walk.options);
            assert!(still.as_raw() == want_still.as_raw(), "seed {seed} step {n} {what}: image differs");
            assert!(bundle == want_bundle, "seed {seed} step {n} {what}: bundle differs");
            if let Some((bundle, still, was_bundle, was_still)) = &kept {
                assert!(**bundle == *was_bundle && still.as_raw() == was_still.as_raw(), "seed {seed} step {n} {what}: a held result changed");
            }
        }
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
    for k in 0..4 {
        if k % 2 == 0 {
            session.still(&image, &analysis, &placement, &scene, &options);
        } else {
            session.still_shared(&image, &analysis, &placement, &scene, &options);
        }
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

#[test]
fn a_picture_that_gets_nothing_builds_no_scenery() {
    let (w, h) = (960, 600);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let path = layouts::photos().into_iter().find(|p| p.to_string_lossy().contains("fox")).expect("fox");
    let image = scooch_core::open(&path).expect("open");
    let analysis = scooch_core::analyze(&image);
    let icons = layouts::layout("scattered", &analysis, w, h);
    let scene = Scene { width: w, height: h, icons: icons.clone() };
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    let mut session = Session::new();
    let (bundle, _) = session.still(&image, &analysis, &placement, &scene, &options);
    assert_eq!(session.sites(), 0);
    assert!(bundle.field.iter().all(|f| *f > bundle.constants.reach), "fox drew scenery");
    let mut moved = icons;
    moved[2].x = (moved[2].x + moved[2].w).min(w as f32 - moved[2].w);
    let next = Scene { icons: moved, ..scene };
    let (_, shown) = session.still(&image, &analysis, &placement, &next, &options);
    assert_eq!(session.sites(), 0);
    assert!(shown.as_raw() == scooch_core::still(&image, &analysis, &placement, &next, &options).1.as_raw());
    let kept = (0..h).step_by(8).flat_map(|y| (0..w).step_by(8).map(move |x| (x, y))).filter(|(x, y)| !session.redrawn(*x, *y)).count();
    assert!(kept * 2 > ((w / 8) * (h / 8)) as usize, "the move redrew the whole still, so it was rebuilt: {kept} cells kept");
}

fn made(name: &str) -> RgbImage {
    let (w, h) = (1600u32, 1000u32);
    RgbImage::from_fn(w, h, |x, y| {
        let mut n = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
        n ^= n >> 13;
        n = n.wrapping_mul(0x5bd1_e995);
        let grain = ((n ^ (n >> 15)) >> 8) as f32 / 16_777_215.0 - 0.5;
        let v = y as f32 / h as f32;
        let c: [f32; 3] = match name {
            "navy" => [10.0 + 10.0 * v, 30.0 + 15.0 * v, 70.0 + 25.0 * v],
            "deep-sky" => [20.0 + 65.0 * v, 45.0 + 100.0 * v, 110.0 + 110.0 * v],
            _ => scooch_core::ISLAND_WATER.map(|c| c as f32),
        };
        image::Rgb(c.map(|c| (c + 2.0 * grain).round().clamp(0.0, 255.0) as u8))
    })
}

fn navy(sx: f32) -> RgbImage {
    let mut image = made("navy");
    let (w, h) = image.dimensions();
    for (x, y, p) in image.enumerate_pixels_mut() {
        let (dx, dy) = (x as f32 / w as f32 - sx, (y as f32 / h as f32 - 0.45) * h as f32 / w as f32);
        if dx * dx + dy * dy < 0.045 * 0.045 {
            *p = image::Rgb([240, 200, 150]);
        }
    }
    image
}

fn picture(name: &str) -> RgbImage {
    match name {
        "two-swans" => scooch_core::open(&layouts::photos().into_iter().find(|p| p.to_string_lossy().contains("two-swans")).expect("photo")).expect("open"),
        _ => made(name),
    }
}

#[test]
fn one_session_works_each_map_out_once() {
    let (w, h) = (960u32, 600u32);
    let image = made("flat-water");
    let analysis = scooch_core::analyze(&image);
    let icons = |size: f32| (0..12).map(|k| Rect { x: 60.0 + 70.0 * k as f32, y: 80.0 + 30.0 * (k % 3) as f32, w: size, h: size }).collect::<Vec<Rect>>();
    let auto = Options { scenery: Scenery::Auto, ..Options::default() };
    let clouds = Options { scenery: Scenery::Clouds, ..Options::default() };
    let mut session = Session::new();
    for (size, options) in [(64.0, auto), (80.0, auto), (80.0, clouds)] {
        let scene = Scene { width: w, height: h, icons: icons(size) };
        assert!(session.belongs(&image, &analysis, &scene, &options));
        assert!(session.bytes() > 0, "the kept map is not counted");
        let kept = session.map_ids().0;
        let placement = scooch_core::solve(&analysis, &scene, &options, None);
        session.still(&image, &analysis, &placement, &scene, &options);
        assert!(kept.is_some() && session.map_ids() == (kept, kept), "icons of {size} with {:?}: the map was worked out again", options.scenery);
    }
}

#[test]
fn a_bare_picture_is_placed_as_with_scenery_off() {
    let (w, h) = (1024u32, 768u32);
    let (auto, off) = (Options { scenery: Scenery::Auto, ..Options::default() }, Options { scenery: Scenery::Off, ..Options::default() });
    let mut bit = 0;
    for sx in [0.15f32, 0.3, 0.7, 0.85] {
    let image = navy(sx);
    let analysis = scooch_core::analyze(&image);
    let mut scenes: Vec<Scene> = ["mac", "windows", "scattered", "cluster"].iter().map(|l| Scene { width: w, height: h, icons: layouts::layout(l, &analysis, w, h) }).collect();
    scenes.extend((1..12).map(|seed| Scene { width: w, height: h, icons: layouts::scattered(w, h, 6 + seed as usize % 10, seed) }));
    for scene in &scenes {
        let (raw, plain) = (scooch_core::solve(&analysis, scene, &auto, None), scooch_core::solve(&analysis, scene, &off, None));
        let mut session = Session::new();
        assert!(!session.belongs(&image, &analysis, scene, &auto));
        assert!(session.solve(&image, &analysis, scene, &auto, None) == plain, "the session placed a bare picture unlike scenery off");
        if raw != plain {
            bit += 1;
            println!("navy with its subject at {sx} at {w}x{h} with {} icons: raw auto zoom {:.3} shift {:?} against off zoom {:.3} shift {:?}", scene.icons.len(), raw.zoom, raw.shift, plain.zoom, plain.shift);
        }
    }
    }
    assert!(bit > 0, "no layout placed the navy picture differently with scenery on, so this test does not bite");
}

#[test]
fn belongs_says_what_the_bundle_carries() {
    let (w, h) = (960u32, 600u32);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    for (name, want) in [("navy", false), ("two-swans", false), ("flat-water", true), ("deep-sky", true)] {
        let image = picture(name);
        let analysis = scooch_core::analyze(&image);
        for layout in ["mac", "scattered"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &analysis, w, h) };
            for call in ["still", "still_shared", "tables_shared"] {
                let mut session = Session::new();
                let belongs = session.belongs(&image, &analysis, &scene, &options);
                assert_eq!(belongs, want, "{name} {layout}");
                let kept = session.map_ids().0;
                let placement = session.solve(&image, &analysis, &scene, &options, None);
                let bundle = match call {
                    "still" => session.still(&image, &analysis, &placement, &scene, &options).0,
                    "still_shared" => (*session.still_shared(&image, &analysis, &placement, &scene, &options).0).clone(),
                    _ => (*session.tables_shared(&image, &analysis, &placement, &scene, &options)).clone(),
                };
                assert_eq!(bundle.bare(), !belongs, "{name} {layout} {call}");
                assert!(kept.is_some() && session.map_ids() == (kept, kept), "{name} {layout} {call}: the map was worked out again");
                let canvas = scooch_core::canvas(&image, w, h);
                let plain = scooch_core::bundle(&image, &analysis, &placement, &scene, &options);
                let back = scooch_core::Bundle::tables_from_bytes(&plain.tables_to_bytes(), canvas).expect("tables");
                assert_eq!(back.bare(), !belongs, "{name} {layout} read back");
            }
        }
    }
}

#[test]
fn the_belonging_stamp_follows_what_the_map_depends_on() {
    let (w, h) = (960u32, 600u32);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let (image, other) = (made("navy"), made("deep-sky"));
    let (analysis, elsewhere) = (scooch_core::analyze(&image), scooch_core::analyze(&other));
    let scene = Scene { width: w, height: h, icons: layouts::layout("scattered", &analysis, w, h) };
    let stamp = scooch_core::belonging_stamp(&analysis, &scene, &options);
    let mut moved = scene.clone();
    moved.icons[2].x += moved.icons[2].w;
    assert_eq!(scooch_core::belonging_stamp(&analysis, &moved, &options), stamp, "an icon move changed the stamp");
    let wider = Scene { width: w + 64, ..scene.clone() };
    let mut taller = scene.clone();
    for r in &mut taller.icons {
        r.h *= 1.25;
    }
    let islands = Options { scenery: Scenery::Islands, ..options };
    for (what, other) in [("screen", scooch_core::belonging_stamp(&analysis, &wider, &options)), ("icon height", scooch_core::belonging_stamp(&analysis, &taller, &options)), ("setting", scooch_core::belonging_stamp(&analysis, &scene, &islands)), ("analysis", scooch_core::belonging_stamp(&elsewhere, &scene, &options))] {
        assert_ne!(other, stamp, "the {what} did not change the stamp");
    }
}
