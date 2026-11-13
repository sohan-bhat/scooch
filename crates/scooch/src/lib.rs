mod cache;
mod commands;
mod daemon;
#[doc(hidden)]
pub mod live;
#[doc(hidden)]
pub mod platform;
#[doc(hidden)]
pub mod poke;
mod scene;
mod state;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "scooch", version, about = "Moves the desktop wallpaper so icons do not cover the subject of the picture.", after_help = "Run scooch with nothing after it to turn it on or see how it is doing.")]
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
        #[arg(long, help = "Render the picture as a plain fill, the way it looks without Scooch")]
        fill: bool,
    },
    #[command(about = "Let Scooch flip the picture when that clears your icons")]
    Flip,
    #[command(about = "Run in the foreground, which is what the login item does")]
    Watch,
    #[command(about = "Check permissions and the desktop")]
    Doctor,
}

pub fn run() -> ExitCode {
    if std::env::args_os().nth(1).is_some_and(|first| first == live::HELPER) {
        return live::helper::main();
    }
    let code = match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    };
    hold_console();
    code
}

fn dispatch() -> Result<()> {
    match Cli::parse().command {
        None => commands::start(),
        Some(Command::Use { image }) => commands::switch(image),
        Some(Command::Off) => commands::off(),
        Some(Command::Status) => commands::status(),
        Some(Command::Render { image, icons, out, json, saliency, fill }) => commands::render(image, icons, out, json, saliency, fill),
        Some(Command::Flip) => commands::flip(),
        Some(Command::Watch) => commands::watch(),
        Some(Command::Doctor) => commands::doctor(),
    }
}

#[cfg(windows)]
fn hold_console() {
    let mut owners = [0u32; 2];
    let count = unsafe { windows::Win32::System::Console::GetConsoleProcessList(&mut owners) };
    if count == 1 {
        println!("Press Enter to close.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

#[cfg(not(windows))]
fn hold_console() {}
