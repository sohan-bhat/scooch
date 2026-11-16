use scooch_core::{Bundle, Constants, RgbImage};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn half(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((h >> 10) & 0x1f) as i32;
    let mantissa = (h & 0x3ff) as f32;
    sign * if exponent == 0 { mantissa * 2.0f32.powi(-24) } else { (1.0 + mantissa / 1024.0) * 2.0f32.powi(exponent - 15) }
}

fn rgba(path: &Path) -> (u32, u32, Vec<u8>) {
    let image = image::open(path).unwrap_or_else(|error| panic!("{}: {error}", path.display())).to_rgba8();
    (image.width(), image.height(), image.into_raw())
}

fn file(folder: &Path, table: &Value) -> PathBuf {
    folder.join(table["file"].as_str().expect("table file"))
}

fn number(c: &Value, key: &str) -> f32 {
    c[key].as_f64().unwrap_or_else(|| panic!("constant {key}")) as f32
}

fn pair(c: &Value, key: &str) -> [f32; 2] {
    [c[key][0].as_f64().unwrap_or_else(|| panic!("constant {key}")) as f32, c[key][1].as_f64().unwrap_or_else(|| panic!("constant {key}")) as f32]
}

fn steepest(map: &[f64], size: u32) -> f64 {
    map.windows(2).map(|p| (p[1] - p[0]).abs()).fold(0.0f64, f64::max) * size as f64
}

fn state(folder: &Path, info: &Value, tables: &Value, canvas: &RgbImage) -> Bundle {
    let width = info["width"].as_u64().expect("width") as u32;
    let height = info["height"].as_u64().expect("height") as u32;
    let long = width.max(height) as usize;
    let map: Vec<f32> = std::fs::read(file(folder, &tables["map"])).expect("map").chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let xmap: Vec<f64> = map[..width as usize].iter().map(|v| *v as f64).collect();
    let ymap: Vec<f64> = map[long..long + height as usize].iter().map(|v| *v as f64).collect();
    let field: Vec<f32> = std::fs::read(file(folder, &tables["field"])).expect("field").chunks_exact(2).map(|c| half(u16::from_le_bytes([c[0], c[1]]))).collect();
    let (grid_width, grid_height, look) = rgba(&file(folder, &tables["look"]));
    let (_, _, pad) = rgba(&file(folder, &tables["pad"]));
    let (paint_width, stacked, paints) = rgba(&file(folder, &tables["paints"]));
    let (_, _, ramps) = rgba(&file(folder, &tables["ramps"]));
    assert_eq!(field.len(), (grid_width * grid_height) as usize, "field size");
    let c = &tables["constants"];
    let constants = Constants {
        unit: number(c, "unit"),
        ramp_low: number(c, "ramp_low"),
        ramp_high: number(c, "ramp_high"),
        reach: number(c, "reach"),
        shadow: pair(c, "shadow"),
        rim: pair(c, "rim"),
        belly: pair(c, "belly"),
        grain: number(c, "grain"),
        grain_scale: number(c, "grain_scale"),
        grain_floor: number(c, "grain_floor"),
        core_lift: number(c, "core_lift"),
        core_drop: number(c, "core_drop"),
        volume_lit: number(c, "volume_lit"),
        volume_dark: number(c, "volume_dark"),
    };
    let (canvas_width, canvas_height) = canvas.dimensions();
    let ratio = steepest(&xmap, canvas_width).max(steepest(&ymap, canvas_height));
    Bundle {
        width,
        height,
        canvas_width,
        canvas_height,
        canvas: canvas.as_raw().clone(),
        xmap,
        ymap,
        ratio,
        grid_width,
        grid_height,
        field,
        look,
        pad,
        paint_width,
        paint_height: stacked / 8,
        paints,
        ramps,
        constants,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: scooch-reference <bundle folder> <canvas image> <out folder> [s ...]");
        std::process::exit(2);
    }
    let folder = PathBuf::from(&args[0]);
    let canvas = image::open(&args[1]).unwrap_or_else(|error| panic!("{}: {error}", args[1])).to_rgb8();
    let canvas = RgbImage::from_raw(canvas.width(), canvas.height(), canvas.into_raw()).expect("canvas");
    let out = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&out).expect("out folder");
    let info: Value = serde_json::from_str(&std::fs::read_to_string(folder.join("bundle.json")).expect("bundle.json")).expect("bundle json");
    let states: Vec<Bundle> = info["states"].as_array().expect("states").iter().map(|s| state(&folder, &info, &s["tables"], &canvas)).collect();
    let mut written = Vec::new();
    for (k, bundle) in states.iter().enumerate() {
        let name = format!("{}.png", info["states"][k]["name"].as_str().unwrap_or("state"));
        scooch_core::frame(bundle, bundle, 0.0).save(out.join(&name)).expect("save");
        written.push(name);
    }
    if states.len() > 1 {
        let picks: Vec<f32> = if args.len() > 3 { args[3..].iter().map(|s| s.parse().expect("s")).collect() } else { vec![0.5] };
        for s in picks {
            let name = format!("{}-{}-{s}.png", info["states"][0]["name"].as_str().unwrap_or("a"), info["states"][1]["name"].as_str().unwrap_or("b"));
            scooch_core::frame(&states[0], &states[1], s).save(out.join(&name)).expect("save");
            written.push(name);
        }
    }
    let header: Vec<String> = states[0].tables_to_bytes()[..8].iter().map(|b| format!("{b:02X}")).collect();
    println!("{}", serde_json::json!({ "canvas": [canvas.width(), canvas.height()], "frames": written, "header": header.join(" ") }));
}
