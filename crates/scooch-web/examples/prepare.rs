#[path = "../../scooch-core/tests/support/layouts.rs"]
mod layouts;

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (name, out) = (args.first().map_or("balloon-sky", String::as_str), PathBuf::from(args.get(1).map_or("demo/assets", String::as_str)));
    let path = layouts::photos().into_iter().find(|p| p.file_stem().unwrap().to_string_lossy() == name).expect("photo");
    let photo = scooch_core::open(&path).expect("open");
    let analysis = scooch_core::analyze(&photo);
    let canvas = scooch_core::canvas(&photo, 1920, 1080);
    std::fs::create_dir_all(&out).expect("folder");
    std::fs::write(out.join("picture.rgb"), &canvas.pixels).expect("picture");
    std::fs::write(out.join("analysis.bin"), analysis.to_bytes()).expect("analysis");
    std::fs::write(out.join("picture.json"), format!("{{\"width\":{},\"height\":{}}}", canvas.width, canvas.height)).expect("meta");
    println!("{name}: canvas {} x {}", canvas.width, canvas.height);
}
