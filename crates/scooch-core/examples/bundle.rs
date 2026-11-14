use image::{ImageBuffer, Rgba};
use scooch_core::{Bundle, Options, Scene, Scenery};
use std::path::{Path, PathBuf};

fn half_bits(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    if v == 0.0 {
        sign
    } else if exponent >= 31 {
        sign | 0x7bff
    } else if exponent <= 0 {
        sign | ((v.abs() / 2.0f32.powi(-24)).round() as u16)
    } else {
        sign | ((exponent as u16) << 10) | ((mantissa >> 13) as u16)
    }
}

fn rgba(width: u32, height: u32, bytes: &[u8], path: &Path) {
    ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, bytes.to_vec()).expect("size").save(path).expect("save png");
}

fn write(bundle: &Bundle, out: &Path, prefix: &str) -> serde_json::Value {
    let long = bundle.width.max(bundle.height) as usize;
    let mut map = vec![0f32; long * 2];
    for (k, row) in [&bundle.xmap, &bundle.ymap].iter().enumerate() {
        for i in 0..long {
            map[k * long + i] = row[i.min(row.len() - 1)] as f32;
        }
    }
    std::fs::write(out.join(format!("{prefix}map.bin")), map.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).expect("map");
    std::fs::write(out.join(format!("{prefix}field.bin")), bundle.field.iter().flat_map(|v| half_bits(*v).to_le_bytes()).collect::<Vec<u8>>()).expect("field");
    rgba(bundle.grid_width, bundle.grid_height, &bundle.look, &out.join(format!("{prefix}look.png")));
    rgba(bundle.grid_width, bundle.grid_height, &bundle.pad, &out.join(format!("{prefix}pad.png")));
    rgba(bundle.paint_width, bundle.paint_height * 8, &bundle.paints, &out.join(format!("{prefix}paints.png")));
    rgba(scooch_core::RAMP as u32, 2, &bundle.ramps, &out.join(format!("{prefix}ramps.png")));
    serde_json::json!({
        "map": { "file": format!("{prefix}map.bin"), "format": "f32le", "width": long, "height": 2 },
        "field": { "file": format!("{prefix}field.bin"), "format": "f16le", "width": bundle.grid_width, "height": bundle.grid_height },
        "look": { "file": format!("{prefix}look.png"), "width": bundle.grid_width, "height": bundle.grid_height },
        "pad": { "file": format!("{prefix}pad.png"), "width": bundle.grid_width, "height": bundle.grid_height },
        "paints": { "file": format!("{prefix}paints.png"), "width": bundle.paint_width, "height": bundle.paint_height, "layers": 8 },
        "ramps": { "file": format!("{prefix}ramps.png"), "width": scooch_core::RAMP, "height": 2 },
        "constants": bundle.constants,
        "mirrored": bundle.mirrored(),
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 && args.len() != 4 {
        eprintln!("usage: bundle <photo> <layout.json> [<second layout.json>] <out folder>");
        std::process::exit(2);
    }
    let photo = scooch_core::open(Path::new(&args[0])).expect("photo");
    let analysis = scooch_core::analyze(&photo);
    let out = PathBuf::from(&args[args.len() - 1]);
    std::fs::create_dir_all(&out).expect("out folder");
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let read = |path: &str| -> Scene { serde_json::from_str(&std::fs::read_to_string(path).expect("layout")).expect("layout json") };
    let first = read(&args[1]);
    let placement = scooch_core::solve(&analysis, &first, &options, None);
    let a = scooch_core::bundle(&photo, &analysis, &placement, &first, &options);
    let canvas = scooch_core::RgbImage::from_raw(a.canvas_width, a.canvas_height, a.canvas.clone()).expect("canvas");
    std::fs::write(out.join("canvas.jpg"), scooch_core::encode_jpeg(&canvas, 92).expect("jpeg")).expect("canvas");
    std::fs::write(out.join("analysis.bin"), analysis.to_bytes()).expect("analysis");
    let mut states = vec![serde_json::json!({ "name": "a", "tables": write(&a, &out, "a.") })];
    if args.len() == 4 {
        let second = read(&args[2]);
        let next = scooch_core::solve(&analysis, &second, &options, Some(&placement));
        let b = scooch_core::bundle(&photo, &analysis, &next, &second, &options);
        assert!(scooch_core::compatible(&a, &b), "the two bundles do not share a canvas");
        states.push(serde_json::json!({ "name": "b", "tables": write(&b, &out, "b.") }));
    }
    let manifest = serde_json::json!({
        "width": a.width,
        "height": a.height,
        "canvas": { "file": "canvas.jpg", "width": a.canvas_width, "height": a.canvas_height },
        "analysis": { "file": "analysis.bin", "format": "scooch analysis v1" },
        "grid_cell": 4,
        "paint_cell": 16,
        "states": states,
    });
    std::fs::write(out.join("bundle.json"), serde_json::to_string_pretty(&manifest).expect("json")).expect("manifest");
    println!("wrote {}", out.display());
}
