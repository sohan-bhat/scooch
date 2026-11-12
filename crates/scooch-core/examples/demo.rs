use scooch_core::{Analysis, Options, Placement, Rect, RgbImage, Scene};
use std::path::{Path, PathBuf};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 960;
const ICON: f32 = 150.0;
const GRID: u32 = 40;
const BEFORE_AT_LEAST: f32 = 0.10;
const CLEAN: f32 = 0.02;
const COVERS: f32 = 0.08;
const VISIBLE_MOVE: f32 = 0.06;
const SHAPE_SLACK: f32 = 0.02;
const START: [(&str, f32, f32); 6] = [("trip", 40.0, 40.0), ("taxes", 40.0, 210.0), ("notes", 40.0, 380.0), ("resume", 40.0, 550.0), ("shot", 210.0, 210.0), ("recipes", 210.0, 380.0)];
const DRAGGED: &str = "shot";

type Layout = Vec<(&'static str, f32, f32)>;

fn scene(layout: &Layout) -> Scene {
    Scene { width: WIDTH, height: HEIGHT, icons: layout.iter().map(|(_, x, y)| Rect { x: *x, y: *y, w: ICON, h: ICON }).collect() }
}

fn through(values: &[f32], source: f32) -> f32 {
    let falling = values[values.len() - 1] < values[0];
    for (k, pair) in values.windows(2).enumerate() {
        let (a, b) = if falling { (pair[1], pair[0]) } else { (pair[0], pair[1]) };
        if source >= a && source <= b && b > a {
            let t = (source - a) / (b - a);
            let t = if falling { 1.0 - t } else { t };
            return (k as f32 + t) / (values.len() - 1) as f32;
        }
    }
    if (source < values[0]) != falling {
        0.0
    } else {
        1.0
    }
}

fn solid(analysis: &Analysis) -> Option<Rect> {
    let (w, h) = (analysis.width as usize, analysis.height as usize);
    let cells: Vec<(usize, usize)> = (0..w * h).filter(|i| analysis.saliency[*i] >= 0.5).map(|i| (i % w, i / w)).collect();
    let x0 = cells.iter().map(|c| c.0).min()?;
    let x1 = cells.iter().map(|c| c.0).max()? + 1;
    let y0 = cells.iter().map(|c| c.1).min()?;
    let y1 = cells.iter().map(|c| c.1).max()? + 1;
    Some(Rect { x: x0 as f32 / w as f32, y: y0 as f32 / h as f32, w: (x1 - x0) as f32 / w as f32, h: (y1 - y0) as f32 / h as f32 })
}

struct Seen {
    center: (f32, f32),
    whole: bool,
    shape: f32,
}

fn seen(subject: &Rect, placement: &Placement) -> Seen {
    let [x0, y0, cw, ch] = placement.crop;
    let whole = subject.x >= x0 && subject.x + subject.w <= x0 + cw && subject.y >= y0 && subject.y + subject.h <= y0 + ch;
    let (a, b) = (through(&placement.fx, subject.x), through(&placement.fx, subject.x + subject.w));
    let (c, d) = (through(&placement.fy, subject.y), through(&placement.fy, subject.y + subject.h));
    let shape = ((b - a).abs() / (subject.w / cw)) / ((d - c).abs() / (subject.h / ch));
    Seen { center: ((a + b) / 2.0, (c + d) / 2.0), whole, shape }
}

fn calm_shape(look: &Seen) -> bool {
    (look.shape - 1.0).abs() <= SHAPE_SLACK
}

fn overlaps(a: (f32, f32), b: (f32, f32)) -> bool {
    a.0 < b.0 + ICON && b.0 < a.0 + ICON && a.1 < b.1 + ICON && b.1 < a.1 + ICON
}

struct Found {
    layout: Layout,
    placement: Placement,
    hidden: f32,
    covered: f32,
    moved: f32,
    reach: f32,
}

fn frame_json(name: &str, hidden: f32, placement: &Placement) -> String {
    format!("    \"{name}\": {{ \"hidden\": {hidden:.4}, \"zoom\": {:.3}, \"shift\": [{:.4}, {:.4}], \"mirrored\": {} }}", placement.zoom, placement.shift[0], placement.shift[1], placement.mirrored)
}

fn layout_json(layout: &Layout) -> String {
    let parts: Vec<String> = layout.iter().map(|(id, x, y)| format!("\"{id}\": [{x}, {y}]")).collect();
    format!("{{ {} }}", parts.join(", "))
}

fn save(dir: &Path, name: &str, picture: &RgbImage) {
    std::fs::write(dir.join(name), scooch_core::encode_png(picture).expect("encode")).expect("write frame");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let photo_path = PathBuf::from(args.next().expect("usage: demo <photo> [out dir] [repo path]"));
    let out = args.next().map(PathBuf::from);
    let repo_path = args.next().unwrap_or_else(|| photo_path.display().to_string());
    let photo = scooch_core::open(&photo_path).expect("open photo");
    let analysis = scooch_core::analyze(&photo);
    let options = Options::default();
    println!("{} {}x{} aspect {:.2}", photo_path.display(), photo.width(), photo.height(), photo.width() as f32 / photo.height() as f32);
    let Some(subject) = solid(&analysis) else {
        println!("fails: the photo has no solid subject");
        return;
    };
    println!("subject x {:.3}..{:.3} ({:.0}% of width) y {:.3}..{:.3}", subject.x, subject.x + subject.w, subject.w * 100.0, subject.y, subject.y + subject.h);
    let mut problems = Vec::new();
    if (photo.width() as f32) < 16.0 / 9.0 * photo.height() as f32 - 1.0 {
        problems.push("narrower than 16 to 9".to_string());
    }
    if subject.w > 0.30 {
        problems.push(format!("subject is {:.0}% of the width, over 30%", subject.w * 100.0));
    }
    let start: Layout = START.to_vec();
    let start_scene = scene(&start);
    let fill = scooch_core::fill(photo.width(), photo.height(), &start_scene);
    let before = scooch_core::hidden_share(&analysis, &start_scene, &fill);
    let fixed = scooch_core::solve(&analysis, &start_scene, &options, None);
    let fixed_hidden = scooch_core::hidden_share(&analysis, &start_scene, &fixed);
    let (look_before, look_fixed) = (seen(&subject, &fill), seen(&subject, &fixed));
    println!("before hidden {before:.3} whole {} | fixed hidden {fixed_hidden:.3} zoom {:.2} shift {:?} whole {} shape {:.3}", look_before.whole, fixed.zoom, fixed.shift, look_fixed.whole, look_fixed.shape);
    if before < BEFORE_AT_LEAST {
        problems.push(format!("before hides {before:.3}, under {BEFORE_AT_LEAST}"));
    }
    if fixed_hidden > CLEAN {
        problems.push(format!("fixed still hides {fixed_hidden:.3}"));
    }
    if !look_before.whole || !look_fixed.whole {
        problems.push("subject cut by a screen edge in before or fixed".to_string());
    }
    if !calm_shape(&look_fixed) {
        problems.push(format!("fixed changes the subject shape to {:.3}", look_fixed.shape));
    }
    let others: Vec<(f32, f32)> = start.iter().filter(|(id, _, _)| *id != DRAGGED).map(|(_, x, y)| (*x, *y)).collect();
    let mut tried = 0;
    let mut covering = 0;
    let mut found: Vec<Found> = Vec::new();
    let mut misses = [0usize; 4];
    for y in (0..=HEIGHT - ICON as u32).step_by(GRID as usize) {
        for x in (0..=WIDTH - ICON as u32).step_by(GRID as usize) {
            let drop = (x as f32, y as f32);
            if others.iter().any(|o| overlaps(*o, drop)) {
                continue;
            }
            tried += 1;
            let layout: Layout = start.iter().map(|(id, ox, oy)| if *id == DRAGGED { (*id, drop.0, drop.1) } else { (*id, *ox, *oy) }).collect();
            let moved_scene = scene(&layout);
            let covered = scooch_core::hidden_share(&analysis, &moved_scene, &fixed);
            if covered < COVERS {
                continue;
            }
            covering += 1;
            let placement = scooch_core::solve(&analysis, &moved_scene, &options, Some(&fixed));
            let hidden = scooch_core::hidden_share(&analysis, &moved_scene, &placement);
            let look = seen(&subject, &placement);
            let moved = (look.center.0 - look_fixed.center.0).abs().max((look.center.1 - look_fixed.center.1).abs());
            let fails = [hidden > CLEAN, moved < VISIBLE_MOVE, !look.whole, !calm_shape(&look)];
            for (count, failed) in misses.iter_mut().zip(fails) {
                *count += failed as usize;
            }
            if fails.iter().any(|f| *f) {
                continue;
            }
            let middle = (look_fixed.center.0 * WIDTH as f32, look_fixed.center.1 * HEIGHT as f32);
            let reach = ((drop.0 + ICON / 2.0 - middle.0).powi(2) + (drop.1 + ICON / 2.0 - middle.1).powi(2)).sqrt();
            found.push(Found { layout, placement, hidden, covered, moved, reach });
        }
    }
    println!("drops tried {tried}, covering the subject {covering}, passing {}", found.len());
    println!("covering drops that fail: still hidden {}, move under 6% {}, subject cut {}, shape changed {}", misses[0], misses[1], misses[2], misses[3]);
    found.sort_by(|a, b| a.placement.zoom.total_cmp(&b.placement.zoom).then(b.moved.total_cmp(&a.moved)).then(a.reach.total_cmp(&b.reach)));
    let Some(best) = found.first() else {
        problems.push(if covering == 0 { "no drop position covers the subject by 0.08".to_string() } else { "no covering drop gives a clean, whole, visible move".to_string() });
        println!("fails: {}", problems.join("; "));
        return;
    };
    let drop = best.layout.iter().find(|(id, _, _)| *id == DRAGGED).map(|(_, x, y)| (*x, *y)).unwrap_or_default();
    println!(
        "best drop {DRAGGED} at {:?}: covered {:.3} | moved hidden {:.3} zoom {:.2} shift {:?} move {:.1}% shape {:.3}",
        drop,
        best.covered,
        best.hidden,
        best.placement.zoom,
        best.placement.shift,
        best.moved * 100.0,
        seen(&subject, &best.placement).shape
    );
    if problems.is_empty() {
        println!("passes");
    } else {
        println!("fails: {}", problems.join("; "));
    }
    let Some(dir) = out else { return };
    std::fs::create_dir_all(&dir).expect("out dir");
    let plain = Options { pads: 0.0, ..options };
    save(&dir, "before.png", &scooch_core::render_with(&photo, &analysis, &fill, &start_scene, &plain));
    save(&dir, "fixed.png", &scooch_core::render_with(&photo, &analysis, &fixed, &start_scene, &options));
    save(&dir, "moved.png", &scooch_core::render_with(&photo, &analysis, &best.placement, &scene(&best.layout), &options));
    let json = format!(
        "{{\n  \"photo\": \"{repo_path}\",\n  \"screen\": [{WIDTH}, {HEIGHT}],\n  \"icon\": [{ICON}, {ICON}],\n  \"layouts\": {{\n    \"start\": {},\n    \"moved\": {}\n  }},\n  \"frames\": {{\n{},\n{},\n{}\n  }}\n}}\n",
        layout_json(&start),
        layout_json(&best.layout),
        frame_json("before", before, &fill),
        frame_json("fixed", fixed_hidden, &fixed),
        frame_json("moved", best.hidden, &best.placement)
    );
    std::fs::write(dir.join("demo.json"), json).expect("write json");
}
