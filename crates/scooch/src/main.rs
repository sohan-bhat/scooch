mod cache;
mod commands;
mod daemon;
mod platform;
mod poke;
mod scene;
mod state;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[cfg(target_os = "macos")]
#[used]
#[link_section = "__TEXT,__info_plist"]
static INFO: [u8; include_bytes!("Info.plist").len()] = *include_bytes!("Info.plist");

#[derive(Parser)]
#[command(name = "scooch", version, about = "Wallpaper that scooches over for your desktop icons.", after_help = "Run scooch with nothing after it to turn it on or see how it is doing.")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Switch the picture on every screen")]
    Use { image: Option<PathBuf> },
    #[command(about = "Stop, remove the login item and put your wallpaper back")]
    Off,
    #[command(about = "Show whether Scooch is on and what it shows")]
    Status,
    #[command(about = "Render one picture to a file without touching your wallpaper")]
    Render {
        image: Option<PathBuf>,
        #[arg(long, help = "A JSON file with the screen width, height and icon rectangles, or your real desktop when left out")]
        icons: Option<PathBuf>,
        #[arg(long, help = "Where to write the picture, or a name next to you when left out")]
        out: Option<PathBuf>,
        #[arg(long, help = "Also write the scene, placement and analysis as JSON")]
        json: Option<PathBuf>,
        #[arg(long, help = "Also write the saliency map as a PNG")]
        saliency: Option<PathBuf>,
    },
    #[command(about = "Run in the foreground, which is what the login item does")]
    Watch,
    #[command(about = "Check permissions and the desktop")]
    Doctor,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().command {
        None => commands::start(),
        Some(Command::Use { image }) => commands::switch(image),
        Some(Command::Off) => commands::off(),
        Some(Command::Status) => commands::status(),
        Some(Command::Render { image, icons, out, json, saliency }) => commands::render(image, icons, out, json, saliency),
        Some(Command::Watch) => commands::watch(),
        Some(Command::Doctor) => commands::doctor(),
    }
}
