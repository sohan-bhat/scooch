use anyhow::Result;
use scooch_core::{Rect, RgbImage};
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", test))]
#[doc(hidden)]
pub mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "windows", test))]
#[doc(hidden)]
pub mod windows;

#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub id: String,
    pub frame: Rect,
    pub scale: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub screens: Vec<Screen>,
    pub icons: Vec<Rect>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    Jpeg,
    Png,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
        }
    }
}

pub type Wake = Box<dyn Fn() + Send + Sync>;

pub trait Desktop: Send + Sync {
    fn name(&self) -> String;
    fn snapshot(&self) -> Result<Snapshot>;
    fn screens(&self) -> Result<Vec<Screen>> {
        Ok(self.snapshot()?.screens)
    }
    fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>>;
    fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()>;
    fn run(&self, wake: Wake) -> Result<()>;
    fn autostart(&self, exe: Option<&Path>) -> Result<()>;
    fn check(&self) -> Vec<(String, Result<String>)>;
    fn format(&self) -> Format;
    fn decode(&self, path: &Path) -> Result<RgbImage>;
    fn consent_hint(&self) -> Option<&'static str> {
        None
    }
    fn restorable(&self, screen: &Screen) -> Result<Option<PathBuf>> {
        self.wallpaper(screen)
    }
    fn gallery(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn profile(&self) -> Option<Vec<u8>> {
        None
    }
    fn read_lag(&self) -> std::time::Duration {
        std::time::Duration::from_secs(2)
    }
    fn known(&self) -> Result<Option<Vec<String>>> {
        Ok(None)
    }
    fn in_use(&self, _folder: &Path) -> Result<Option<Vec<PathBuf>>> {
        Ok(None)
    }
}

pub fn folder_and_children(root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![root.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
        children.sort();
        folders.extend(children);
    }
    folders
}

pub fn current() -> Result<Box<dyn Desktop>> {
    #[cfg(target_os = "macos")]
    return macos::desktop();
    #[cfg(target_os = "windows")]
    return windows::desktop();
    #[cfg(target_os = "linux")]
    return linux::desktop();
    #[allow(unreachable_code)]
    Err(anyhow::anyhow!("Scooch does not run on this system yet."))
}
