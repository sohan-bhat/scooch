#[path = "../../scooch-core/tests/support/layouts.rs"]
mod layouts;

use scooch_web::Core;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn tables(answer: &scooch_web::Answer) -> Vec<u8> {
    let b = &answer.bundle;
    let mut out = Vec::new();
    scooch_web::map(b).iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    scooch_web::field(b).iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    out.extend_from_slice(&b.look);
    out.extend_from_slice(&b.pad);
    out.extend_from_slice(&b.paints);
    out.extend_from_slice(&b.ramps);
    scooch_web::constants(b).iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    out.push(answer.fits as u8);
    out
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("usage: walk <out folder>"));
    let (w, h) = (640u32, 480u32);
    let mut cases = Vec::new();
    for name in ["balloon-sky", "fox", "astronaut"] {
        let path = layouts::photos().into_iter().find(|p| p.file_stem().unwrap().to_string_lossy() == name).expect("photo");
        let photo = scooch_core::open(&path).expect("open");
        let analysis = scooch_core::analyze(&photo);
        let canvas = scooch_core::canvas(&photo, 1280, 960);
        for layout in ["mac", "scattered"] {
            let dir = out.join(format!("{name}-{layout}"));
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(dir.join("picture.rgb"), &canvas.pixels).expect("picture");
            std::fs::write(dir.join("analysis.bin"), analysis.to_bytes()).expect("analysis");
            let mut icons: Vec<[f32; 4]> = match layout {
                "mac" => layouts::mac(w, h, 12),
                _ => layouts::scattered(w, h, 12, 5),
            }
            .iter()
            .map(|r| [r.x, r.y, r.w, r.h])
            .collect();
            let mut core = Core::new(&canvas.pixels, canvas.width, canvas.height, &analysis.to_bytes()).expect("core");
            core.screen(w, h);
            let mut dice = Dice(name.len() as u64 * 31 + layout.len() as u64);
            let mut steps = Vec::new();
            let mut expected = Vec::new();
            let mut held: Option<usize> = None;
            for n in 0..20 {
                let flat: Vec<f32> = icons.iter().flatten().copied().collect();
                let started = Instant::now();
                let (op, answer) = if n == 0 || held.is_none() || dice.next() < 0.35 {
                    ("drop", core.drop_icons(&flat))
                } else {
                    ("drag", core.drag(&flat, Duration::from_secs(1_000_000), &|| started.elapsed()))
                };
                steps.push(format!("{{\"op\":\"{op}\",\"held\":{},\"icons\":[{}]}}", held.unwrap_or(0), flat.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(",")));
                expected.extend(tables(&answer));
                if op == "drop" {
                    held = Some((dice.next() * icons.len() as f32) as usize % icons.len());
                }
                if let Some(k) = held {
                    let r = &mut icons[k];
                    r[0] = (r[0] + (dice.next() * 2.0 - 1.0) * 1.5 * r[3]).clamp(0.0, w as f32 - r[2]);
                    r[1] = (r[1] + (dice.next() * 2.0 - 1.0) * 1.5 * r[3]).clamp(0.0, h as f32 - r[3]);
                }
            }
            let meta = format!("{{\"picture\":[{},{}],\"screen\":[{w},{h}],\"size\":{},\"steps\":[{}]}}", canvas.width, canvas.height, expected.len() / 20, steps.join(","));
            std::fs::write(dir.join("walk.json"), meta).expect("meta");
            std::fs::write(dir.join("expected.bin"), &expected).expect("expected");
            cases.push(format!("\"{name}-{layout}\""));
        }
    }
    std::fs::write(out.join("cases.json"), format!("[{}]", cases.join(","))).expect("cases");
    println!("wrote {}", out.display());
}
