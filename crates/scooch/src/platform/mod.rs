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
pub type DragStart = Box<dyn Fn((f32, f32)) -> bool + Send + Sync>;

pub struct DragHooks {
    pub on_icon: DragStart,
    pub start: DragStart,
}

pub const DRAG_SLOP: f32 = 4.0;
pub const PRESS_CAP: Duration = Duration::from_secs(30);
const FAST_POLLS: Duration = Duration::from_millis(300);

pub fn pressed(at: Option<(f32, f32)>, on_icon: impl Fn((f32, f32)) -> bool) -> Option<(f32, f32)> {
    at.filter(|at| on_icon(*at))
}

pub fn poll_gap(since: Duration) -> Duration {
    if since < FAST_POLLS {
        Duration::from_millis(20)
    } else {
        Duration::from_millis(100)
    }
}

pub fn follow(from: (f32, f32), mut poll: impl FnMut(Duration) -> Option<(bool, Option<(f32, f32)>)>) -> Option<Duration> {
    let mut since = Duration::ZERO;
    while since < PRESS_CAP {
        let gap = poll_gap(since);
        since += gap;
        let (held, at) = poll(gap)?;
        if !held {
            return None;
        }
        if at.is_some_and(|to| (to.0 - from.0).hypot(to.1 - from.1) >= DRAG_SLOP) {
            return Some(since);
        }
    }
    None
}

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
    fn busy(&self) -> Box<dyn std::any::Any> {
        Box::new(())
    }
    fn dropped(&self) -> bool {
        false
    }
    fn reads_at_once(&self) -> bool {
        false
    }
    fn place_saved(&self) -> Option<Instant> {
        None
    }
    fn predicted(&self, _shown: &dyn Fn() -> bool) -> Option<Snapshot> {
        None
    }
    fn put_back_detached(&self, _id: &str, _picture: &Path) -> Result<bool> {
        Ok(false)
    }
    fn leave_login(&self) -> Result<()> {
        leave_login()
    }
    fn pointer(&self) -> Option<(f32, f32)> {
        None
    }
    fn drag_starts(&self, _hooks: DragHooks) {}
    fn desktop_at(&self, _point: (f32, f32)) -> bool {
        false
    }
    fn button_down(&self) -> bool {
        false
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

pub fn login_program() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    return macos::launchd::program();
    #[cfg(target_os = "windows")]
    return windows::startup::named();
    #[cfg(target_os = "linux")]
    return linux::autostart::program();
    #[allow(unreachable_code)]
    None
}

pub fn login_exists() -> bool {
    #[cfg(target_os = "macos")]
    return macos::launchd::installed();
    #[cfg(target_os = "windows")]
    return windows::startup::entry().is_some();
    #[cfg(target_os = "linux")]
    return linux::autostart::installed();
    #[allow(unreachable_code)]
    false
}

pub fn remove_login() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::launchd::remove();
    #[cfg(target_os = "windows")]
    return windows::startup::remove();
    #[cfg(target_os = "linux")]
    return linux::autostart::remove();
    #[allow(unreachable_code)]
    Ok(())
}

pub fn leave_login() -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::launchd::forget();
    #[allow(unreachable_code)]
    remove_login()
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

    type Read = Option<(bool, Option<(f32, f32)>)>;

    fn scripted(steps: Vec<Read>) -> (Option<Duration>, Vec<Duration>) {
        let mut gaps = Vec::new();
        let mut steps = steps.into_iter();
        let found = follow((100.0, 100.0), |gap| {
            gaps.push(gap);
            steps.next().unwrap_or(Some((true, Some((100.0, 100.0)))))
        });
        (found, gaps)
    }

    #[test]
    fn a_click_with_no_move_starts_no_drag() {
        let (found, gaps) = scripted(vec![Some((true, Some((100.0, 100.0)))), Some((true, Some((102.0, 101.0)))), Some((false, Some((140.0, 100.0))))]);
        assert_eq!((found, gaps.len()), (None, 3));
        let (found, gaps) = scripted(vec![Some((true, Some((101.0, 99.0)))), None, Some((true, Some((140.0, 100.0))))]);
        assert_eq!((found, gaps.len()), (None, 2), "a press that ended went on being polled");
    }

    #[test]
    fn a_drag_starts_once_at_the_first_poll_four_points_away() {
        let (found, gaps) = scripted(vec![Some((true, Some((100.0, 100.0)))), Some((true, Some((103.0, 103.0)))), Some((true, Some((150.0, 100.0))))]);
        assert_eq!((found, gaps.len()), (Some(Duration::from_millis(40)), 2));
        assert_eq!(pressed(None, |_| true), None);
    }

    #[test]
    fn a_held_press_is_polled_every_20_ms_then_every_100_ms_until_30_s() {
        let (found, gaps) = scripted(Vec::new());
        assert_eq!(found, None);
        assert_eq!(gaps.iter().filter(|gap| **gap == Duration::from_millis(20)).count(), 15);
        assert_eq!(gaps.iter().filter(|gap| **gap == Duration::from_millis(100)).count(), 297);
        assert_eq!(gaps.iter().sum::<Duration>(), PRESS_CAP);
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
