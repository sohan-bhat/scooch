#[path = "support/layouts.rs"]
mod layouts;

use scooch_core::{Analysis, Options, Placement, Rect, Scene, FLIP_FLOOR, FLIP_SHARE};
use std::sync::OnceLock;

const SCREENS: [(u32, u32); 2] = [(2880, 1800), (1920, 1080)];
const OLD_MIRRORED: f64 = 0.25;
const OLD_KEEP_FLOOR: f64 = 0.02;
const OLD_KEEP_SHARE: f64 = 0.10;
const SMALL: f32 = 0.01;
const CLEAR: f32 = 0.02;
const REFRAME: f64 = 0.15;

struct Photo {
    name: String,
    analysis: Analysis,
}

fn photos() -> &'static [Photo] {
    static CELL: OnceLock<Vec<Photo>> = OnceLock::new();
    CELL.get_or_init(|| {
        layouts::photos()
            .into_iter()
            .map(|path| Photo { name: path.file_stem().unwrap().to_string_lossy().into(), analysis: scooch_core::analyze(&scooch_core::open(&path).expect("photo opens")) })
            .collect()
    })
}

fn photo(name: &str) -> &'static Analysis {
    &photos().iter().find(|p| p.name == name).expect("photo").analysis
}

fn on() -> Options {
    Options { mirror: true, ..Options::default() }
}

fn bar(cost: f64) -> f64 {
    FLIP_FLOOR + FLIP_SHARE * cost
}

fn right(w: u32, h: u32) -> Vec<Rect> {
    let s = layouts::scale(w, h);
    layouts::mac(w, h, (((h as f32 - 45.0 * s) / (100.0 * s)).floor() as usize).max(1))
}

fn left(w: u32, h: u32) -> Vec<Rect> {
    right(w, h).into_iter().map(|r| Rect { x: w as f32 - r.x - r.w, ..r }).collect()
}

fn on_subject(analysis: &Analysis, w: u32, h: u32) -> Rect {
    let s = layouts::scale(w, h);
    let [x0, y0, cw, ch] = scooch_core::home(analysis, &Scene { width: w, height: h, icons: Vec::new() }).crop;
    let subject = analysis.subject;
    let (cx, cy) = (subject.x + subject.w / 2.0, subject.y + subject.h / 2.0);
    let (iw, ih) = (76.0 * s, 94.0 * s);
    let x = ((cx - x0) / cw * w as f32 - iw / 2.0).clamp(0.0, w as f32 - iw);
    let y = ((cy - y0) / ch * h as f32 - ih / 2.0).clamp(0.0, h as f32 - ih);
    Rect { x, y, w: iw, h: ih }
}

fn moved(analysis: &Analysis, w: u32, h: u32) -> Vec<Rect> {
    let mut icons = right(w, h);
    let k = icons.len() / 2;
    icons[k] = on_subject(analysis, w, h);
    icons
}

fn scene(w: u32, h: u32, icons: Vec<Rect>) -> Scene {
    Scene { width: w, height: h, icons }
}

fn nudged(icons: &[Rect], k: usize, cells: (f32, f32), w: u32, h: u32) -> Vec<Rect> {
    let s = layouts::scale(w, h);
    let mut out = icons.to_vec();
    out[k].x = (out[k].x + cells.0 * 90.0 * s).clamp(0.0, w as f32 - out[k].w);
    out[k].y = (out[k].y + cells.1 * 100.0 * s).clamp(0.0, h as f32 - out[k].h);
    out
}

struct Side {
    cost: f64,
    hidden: f32,
}

fn sides(analysis: &Analysis, scene: &Scene, previous: Option<&Placement>) -> [Option<Side>; 2] {
    scooch_core::flip_sides(analysis, scene, &on(), previous).map(|side| side.map(|(placement, cost)| Side { cost, hidden: scooch_core::hidden_share(analysis, scene, &placement) }))
}

fn old_cost(analysis: &Analysis, scene: &Scene, placement: &Placement) -> f64 {
    scooch_core::placement_cost(analysis, scene, &on(), placement) + if placement.mirrored { OLD_MIRRORED } else { 0.0 }
}

fn old_solve(analysis: &Analysis, scene: &Scene, previous: Option<&Placement>) -> Placement {
    let [plain, flipped] = scooch_core::flip_sides(analysis, scene, &on(), previous);
    let plain = plain.map_or_else(|| scooch_core::solve(analysis, scene, &Options::default(), None), |p| p.0);
    let fresh = match flipped {
        Some((f, _)) if old_cost(analysis, scene, &f) < old_cost(analysis, scene, &plain) => f,
        _ => plain,
    };
    let Some(old) = previous else { return fresh };
    if *old == fresh {
        return fresh;
    }
    let (cost_new, cost_old) = (old_cost(analysis, scene, &fresh), old_cost(analysis, scene, old));
    if cost_new < cost_old - (OLD_KEEP_FLOOR + OLD_KEEP_SHARE * cost_old) {
        fresh
    } else {
        old.clone()
    }
}

struct Case {
    label: String,
    analysis: &'static Analysis,
    scene: Scene,
    current: bool,
    plain: Side,
    flipped: Side,
    old_flips: bool,
    new_flips: bool,
    placed: Placement,
}

impl Case {
    fn stay_other(&self) -> (&Side, &Side) {
        if self.current {
            (&self.flipped, &self.plain)
        } else {
            (&self.plain, &self.flipped)
        }
    }

    fn uncovers(&self) -> f32 {
        let (stay, other) = self.stay_other();
        stay.hidden - other.hidden
    }

    fn saves(&self) -> f64 {
        let (stay, other) = self.stay_other();
        stay.cost - other.cost
    }
}

fn case(label: String, analysis: &'static Analysis, scene: Scene, before: Option<(&Placement, &Placement)>) -> Option<Case> {
    let (old_before, new_before) = before.unzip();
    let current = new_before.is_some_and(|p| p.mirrored);
    let [plain, flipped] = sides(analysis, &scene, new_before);
    let (plain, flipped) = (plain?, flipped?);
    let old_flips = old_solve(analysis, &scene, old_before).mirrored != old_before.is_some_and(|p| p.mirrored);
    let placed = scooch_core::solve(analysis, &scene, &on(), new_before);
    Some(Case { label, analysis, current, plain, flipped, old_flips, new_flips: placed.mirrored != current, placed, scene })
}

fn cases() -> &'static [Case] {
    static CELL: OnceLock<Vec<Case>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut out = Vec::new();
        for photo in photos() {
            let analysis = &photo.analysis;
            for (w, h) in SCREENS {
                let fresh = [("left", left(w, h)), ("right", right(w, h)), ("scattered", layouts::scattered(w, h, 12, 2026)), ("crowd", layouts::scattered(w, h, 60, 7)), ("subject", vec![on_subject(analysis, w, h)])];
                for (layout, icons) in fresh {
                    out.extend(case(format!("{} {w}x{h} {layout}", photo.name), analysis, scene(w, h, icons), None));
                }
                let column = scene(w, h, right(w, h));
                let (old, new) = (old_solve(analysis, &column, None), scooch_core::solve(analysis, &column, &on(), None));
                out.extend(case(format!("{} {w}x{h} move", photo.name), analysis, scene(w, h, moved(analysis, w, h)), Some((&old, &new))));
                let spot = vec![on_subject(analysis, w, h)];
                let first = scene(w, h, spot.clone());
                let (old, new) = (old_solve(analysis, &first, None), scooch_core::solve(analysis, &first, &on(), None));
                out.extend(case(format!("{} {w}x{h} nudge", photo.name), analysis, scene(w, h, nudged(&spot, 0, (1.0, 0.0), w, h)), Some((&old, &new))));
            }
        }
        out
    })
}

#[test]
fn the_bar_lets_through_only_flips_that_uncover_the_subject() {
    let word = |flips: bool| if flips { "flips" } else { "stays" };
    for c in cases() {
        println!(
            "{:34} from {:7} plain {:.3} hides {:.3} flipped {:.3} hides {:.3} bar {:.3} today {} now {}",
            c.label,
            if c.current { "flipped" } else { "plain" },
            c.plain.cost,
            c.plain.hidden,
            c.flipped.cost,
            c.flipped.hidden,
            bar(c.stay_other().0.cost),
            word(c.old_flips),
            word(c.new_flips)
        );
    }
    let unearned: Vec<&str> = cases().iter().filter(|c| c.new_flips && c.uncovers() < SMALL && c.saves() < REFRAME).map(|c| c.label.as_str()).collect();
    assert!(unearned.is_empty(), "flips that uncover less than {SMALL} of the subject {unearned:?}");
    let clear: Vec<&Case> = cases().iter().filter(|c| c.uncovers() >= CLEAR && c.saves() > 0.0).collect();
    let kept = clear.iter().filter(|c| c.new_flips).count();
    let missed: Vec<&str> = clear.iter().filter(|c| !c.new_flips).map(|c| c.label.as_str()).collect();
    println!("floor {FLIP_FLOOR} share {FLIP_SHARE} lets through {kept} of {} flips that uncover at least {CLEAR}, missed {missed:?}", clear.len());
    assert!(10 * kept >= 9 * clear.len(), "{kept} of {} clear flips happen, missed {missed:?}", clear.len());
}

#[test]
fn a_flip_short_of_the_bar_is_refused() {
    let analysis = photo("two-swans");
    let (w, h) = (1920, 1080);
    let column = scene(w, h, right(w, h));
    let [Some(plain), Some(flipped)] = sides(analysis, &column, None) else { panic!("both sides") };
    assert!(flipped.cost < plain.cost && flipped.cost >= plain.cost - bar(plain.cost), "plain {} flipped {}", plain.cost, flipped.cost);
    assert!(!scooch_core::solve(analysis, &column, &on(), None).mirrored);
}

#[test]
fn a_flip_past_the_bar_happens() {
    let analysis = photo("glass-cubes");
    let (w, h) = (2880, 1800);
    let column = scene(w, h, right(w, h));
    let [Some(plain), Some(flipped)] = sides(analysis, &column, None) else { panic!("both sides") };
    assert!(flipped.cost < plain.cost - bar(plain.cost), "plain {} flipped {}", plain.cost, flipped.cost);
    assert!(flipped.hidden < plain.hidden / 2.0);
    let placed = scooch_core::solve(analysis, &column, &on(), None);
    assert!(placed.mirrored);
    assert!(scooch_core::hidden_share(analysis, &column, &placed) < plain.hidden / 2.0);
}

#[test]
fn a_flip_back_needs_the_same_bar() {
    let (w, h) = (2880, 1800);
    let balloon = layouts::scattered(w, h, 12, 2026);
    let dragonfly = moved(photo("dragonfly"), 1920, 1080);
    let trips = [("balloon-sky", (w, h), balloon.clone(), nudged(&balloon, 10, (1.0, 0.0), w, h)), ("dragonfly", (1920, 1080), dragonfly.clone(), nudged(&dragonfly, dragonfly.len() / 2, (0.0, 1.0), 1920, 1080))];
    for (name, (w, h), here, there) in trips {
        let analysis = photo(name);
        let (a, b) = (scene(w, h, here), scene(w, h, there));
        let first = scooch_core::solve(analysis, &a, &on(), None);
        assert!(first.mirrored, "{name}");
        let [Some(plain), Some(flipped)] = sides(analysis, &b, Some(&first)) else { panic!("{name} both sides") };
        assert!(plain.cost < flipped.cost && plain.cost >= flipped.cost - bar(flipped.cost), "{name} plain {} flipped {}", plain.cost, flipped.cost);
        let mut placed = first;
        for (step, next) in [&b, &a, &b, &a].into_iter().enumerate() {
            placed = scooch_core::solve(analysis, next, &on(), Some(&placed));
            assert!(placed.mirrored, "{name} step {step} flipped back");
        }
    }
}

#[test]
fn flip_off_never_mirrors() {
    let off = Options::default();
    let mut held = 0;
    for c in cases() {
        assert!(!scooch_core::solve(c.analysis, &c.scene, &off, None).mirrored, "{}", c.label);
        if c.placed.mirrored {
            held += 1;
            assert!(!scooch_core::solve(c.analysis, &c.scene, &off, Some(&c.placed)).mirrored, "{} kept a flip", c.label);
        }
    }
    assert!(held > 0);
}
