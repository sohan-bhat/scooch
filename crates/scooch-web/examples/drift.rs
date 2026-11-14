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

#[derive(Default)]
struct Gap {
    worst: f64,
    sum: f64,
    count: u64,
    differ: u64,
}

impl Gap {
    fn add(&mut self, a: f64, b: f64) {
        let d = (a - b).abs();
        self.worst = self.worst.max(d);
        self.sum += d;
        self.count += 1;
        self.differ += (a.to_bits() != b.to_bits()) as u64;
    }

    fn line(&self, name: &str) -> String {
        format!("{name}: worst {:.6} mean {:.8} differing {:.4}% of {}", self.worst, self.sum / self.count.max(1) as f64, 100.0 * self.differ as f64 / self.count.max(1) as f64, self.count)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, dir) = (args[0].as_str(), PathBuf::from(&args[1]));
    let (w, h) = (640u32, 480u32);
    let (mut field, mut bytes, mut still) = (Gap::default(), Gap::default(), Gap::default());
    for name in ["balloon-sky", "fox", "astronaut"] {
        let path = layouts::photos().into_iter().find(|p| p.file_stem().unwrap().to_string_lossy() == name).expect("photo");
        let photo = scooch_core::open(&path).expect("open");
        let canvas = scooch_core::canvas(&photo, 1280, 960);
        let analysis = if mode == "write" { scooch_core::analyze(&photo).to_bytes() } else { std::fs::read(dir.join(format!("{name}.analysis"))).expect("analysis") };
        if mode == "write" {
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(dir.join(format!("{name}.analysis")), &analysis).expect("analysis");
        }
        for layout in ["mac", "scattered"] {
            let mut icons: Vec<[f32; 4]> = match layout {
                "mac" => layouts::mac(w, h, 12),
                _ => layouts::scattered(w, h, 12, 5),
            }
            .iter()
            .map(|r| [r.x, r.y, r.w, r.h])
            .collect();
            let mut core = Core::new(&canvas.pixels, canvas.width, canvas.height, &analysis).expect("core");
            core.screen(w, h);
            let mut dice = Dice(name.len() as u64 * 31 + layout.len() as u64);
            let mut held: Option<usize> = None;
            let mut tables = Vec::new();
            let mut stills = Vec::new();
            for n in 0..20 {
                let flat: Vec<f32> = icons.iter().flatten().copied().collect();
                let started = Instant::now();
                let drop = n == 0 || held.is_none() || dice.next() < 0.35;
                let answer = if drop { core.drop_icons(&flat) } else { core.drag(&flat, Duration::from_secs(1_000_000), &|| started.elapsed()) };
                let mut bundle = (*answer.bundle).clone();
                if bundle.canvas.is_empty() {
                    bundle.canvas = canvas.pixels.clone();
                }
                tables.push(bundle.clone());
                stills.push(scooch_core::frame(&bundle, &bundle, 0.0));
                if drop {
                    held = Some((dice.next() * icons.len() as f32) as usize % icons.len());
                }
                if let Some(k) = held {
                    let r = &mut icons[k];
                    r[0] = (r[0] + (dice.next() * 2.0 - 1.0) * 1.5 * r[3]).clamp(0.0, w as f32 - r[2]);
                    r[1] = (r[1] + (dice.next() * 2.0 - 1.0) * 1.5 * r[3]).clamp(0.0, h as f32 - r[3]);
                }
            }
            let file = dir.join(format!("{name}-{layout}"));
            let flat_tables: Vec<u8> = tables.iter().flat_map(|b| [b.field.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>(), b.look.clone(), b.pad.clone(), b.paints.clone(), b.ramps.clone()].concat()).collect();
            let flat_stills: Vec<u8> = stills.iter().flat_map(|s| s.as_raw().clone()).collect();
            if mode == "write" {
                std::fs::write(file.with_extension("tables"), &flat_tables).expect("tables");
                std::fs::write(file.with_extension("stills"), &flat_stills).expect("stills");
                continue;
            }
            let (old_tables, old_stills) = (std::fs::read(file.with_extension("tables")).expect("tables"), std::fs::read(file.with_extension("stills")).expect("stills"));
            let per = flat_tables.len() / 20;
            let cells = tables[0].field.len();
            for n in 0..20 {
                let (a, b) = (&flat_tables[n * per..(n + 1) * per], &old_tables[n * per..(n + 1) * per]);
                for c in 0..cells {
                    let read = |t: &[u8]| f32::from_le_bytes([t[c * 4], t[c * 4 + 1], t[c * 4 + 2], t[c * 4 + 3]]) as f64;
                    field.add(read(a), read(b));
                }
                for (p, q) in a[cells * 4..].iter().zip(&b[cells * 4..]) {
                    bytes.add(*p as f64, *q as f64);
                }
            }
            for (p, q) in flat_stills.iter().zip(&old_stills) {
                still.add(*p as f64, *q as f64);
            }
        }
    }
    if mode != "write" {
        println!("{}", field.line("field, in icon units"));
        println!("{}", bytes.line("look, pad, paints, ramps, in byte levels"));
        println!("{}", still.line("still drawn from the tables, in byte levels"));
    }
}
