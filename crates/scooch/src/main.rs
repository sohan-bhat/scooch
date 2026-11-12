mod platform;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use scooch_core::{Options, Rect, Scene};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "scooch", version, about = "Wallpaper that scooches over for your desktop icons.")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Render {
        image: PathBuf,
        #[arg(long)]
        icons: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: Option<PathBuf>,
    },
    Doctor,
}

#[derive(serde::Deserialize)]
struct SceneFile {
    width: u32,
    height: u32,
    icons: Vec<[f32; 4]>,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Some(Command::Render { image, icons, out, json }) => render(image, icons, out, json),
        Some(Command::Doctor) | None => doctor(),
    }
}

fn render(image: PathBuf, icons: PathBuf, out: PathBuf, json: Option<PathBuf>) -> Result<()> {
    let file: SceneFile = serde_json::from_slice(&std::fs::read(&icons).with_context(|| format!("reading {}", icons.display()))?)?;
    let scene = Scene {
        width: file.width,
        height: file.height,
        icons: file.icons.iter().map(|r| Rect { x: r[0], y: r[1], w: r[2], h: r[3] }).collect(),
    };
    let options = Options::default();
    let source = scooch_core::open(&image).with_context(|| format!("opening {}", image.display()))?;
    let analysis = scooch_core::analyze(&source);
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    let picture = scooch_core::render(&source, &placement, &scene, &options);
    let bytes = match out.extension().and_then(|e| e.to_str()) {
        Some("png") => scooch_core::encode_png(&picture)?,
        _ => scooch_core::encode_jpeg(&picture, 95)?,
    };
    std::fs::write(&out, bytes)?;
    if let Some(path) = json {
        std::fs::write(path, serde_json::to_vec(&serde_json::json!({ "scene": scene, "placement": placement, "analysis": analysis }))?)?;
    }
    Ok(())
}

fn doctor() -> Result<()> {
    let desktop = platform::current()?;
    println!("{}", desktop.name());
    for (name, result) in desktop.check() {
        match result {
            Ok(detail) => println!("ok    {name}  {detail}"),
            Err(error) => println!("fail  {name}  {error}"),
        }
    }
    Ok(())
}
