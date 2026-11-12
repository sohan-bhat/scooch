use anyhow::Result;
use scooch_core::{Rect, RgbImage};
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub id: String,
    pub frame: Rect,
    pub scale: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub screens: Vec<Screen>,
    pub icons: Vec<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    Jpeg,
    Png,
}

pub trait Desktop: Send + Sync {
    fn name(&self) -> String;
    fn snapshot(&self) -> Result<Snapshot>;
    fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>>;
    fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()>;
    fn run(&self, wake: Box<dyn Fn() + Send + Sync>) -> Result<()>;
    fn autostart(&self, exe: Option<&Path>) -> Result<()>;
    fn check(&self) -> Vec<(String, Result<String>)>;
    fn format(&self) -> Format {
        Format::Jpeg
    }
    fn decode(&self, path: &Path) -> Result<RgbImage> {
        Ok(image_open(path)?)
    }
}

pub fn image_open(path: &Path) -> Result<RgbImage> {
    Ok(scooch_core::open(path)?)
}

pub fn current() -> Result<Box<dyn Desktop>> {
    #[cfg(target_os = "macos")]
    return macos::desktop();
    #[cfg(target_os = "windows")]
    return windows::desktop();
    #[cfg(target_os = "linux")]
    return linux::desktop();
    #[allow(unreachable_code)]
    Err(anyhow::anyhow!("Scooch does not support this system yet"))
}
