use anyhow::Result;
use scooch_core::{Rect, RgbImage};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", test))]
#[doc(hidden)]
pub mod linux;
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub mod macos;
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
    fn watching(&self, _wake: Wake) {}
    fn live(&self) -> bool {
        cfg!(windows)
    }
    fn reduce_motion(&self) -> bool {
        false
    }
    fn hold(&self) -> Duration {
        Duration::from_millis(250)
    }
}

pub const AGREE: Duration = Duration::from_millis(500);
pub const RECHECK: Duration = Duration::from_millis(20);

#[derive(Debug, PartialEq)]
pub enum Seen<T> {
    Agreed(T),
    Ours(PathBuf),
}

pub fn agreed<T>(mut look: impl FnMut() -> Result<(Option<PathBuf>, Option<PathBuf>, T)>, ours: impl Fn(&Path) -> bool) -> Result<Seen<T>> {
    let deadline = Instant::now() + AGREE;
    loop {
        let (recorded, reported, view) = look()?;
        match disagreement(recorded.as_deref(), reported.as_deref(), &ours) {
            None => return Ok(Seen::Agreed(view)),
            Some(preferred) if Instant::now() >= deadline => return Ok(Seen::Ours(preferred)),
            Some(_) => std::thread::sleep(RECHECK),
        }
    }
}

pub fn disagreement(recorded: Option<&Path>, reported: Option<&Path>, ours: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let reported_ours = reported.filter(|path| ours(path));
    let recorded_ours = recorded.filter(|path| ours(path));
    if reported_ours.is_none() && recorded_ours.is_none() {
        return None;
    }
    if let (Some(recorded), Some(reported)) = (recorded, reported) {
        if crate::state::same(recorded, reported) {
            return None;
        }
    }
    reported_ours.or(recorded_ours).map(Path::to_path_buf)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disagreement_matters_only_when_one_side_points_at_scoochs_picture() {
        let ours = |path: &Path| path.starts_with("/d/out");
        let (render, old, newer) = (Path::new("/d/out/D-1.jpg"), Path::new("/p/old.jpg"), Path::new("/p/new.jpg"));
        assert_eq!(disagreement(Some(old), Some(render), ours), Some(render.to_path_buf()));
        assert_eq!(disagreement(Some(render), Some(old), ours), Some(render.to_path_buf()));
        assert_eq!(disagreement(Some(render), None, ours), Some(render.to_path_buf()));
        assert_eq!(disagreement(Some(render), Some(render), ours), None);
        assert_eq!(disagreement(Some(old), Some(newer), ours), None);
        assert_eq!(disagreement(None, None, ours), None);
    }

    #[test]
    fn a_lasting_disagreement_ends_on_scoochs_picture_at_the_cap() {
        let started = Instant::now();
        let seen = agreed(|| Ok((Some(PathBuf::from("/p/old.jpg")), Some(PathBuf::from("/d/out/D-1.jpg")), ())), |path: &Path| path.starts_with("/d/out")).unwrap();
        assert_eq!(seen, Seen::Ours(PathBuf::from("/d/out/D-1.jpg")));
        assert!(started.elapsed() >= AGREE);
        let mut reads = 0;
        let seen = agreed(
            || {
                reads += 1;
                let recorded = if reads < 3 { "/p/old.jpg" } else { "/d/out/D-1.jpg" };
                Ok((Some(PathBuf::from(recorded)), Some(PathBuf::from("/d/out/D-1.jpg")), reads))
            },
            |path: &Path| path.starts_with("/d/out"),
        )
        .unwrap();
        assert_eq!(seen, Seen::Agreed(3));
    }
}
