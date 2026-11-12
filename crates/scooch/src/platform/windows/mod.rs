pub mod layout;
pub mod wakes;

#[cfg(target_os = "windows")]
pub mod com;
#[cfg(target_os = "windows")]
pub mod picture;
#[cfg(target_os = "windows")]
pub mod shell;
#[cfg(target_os = "windows")]
pub mod startup;
#[cfg(target_os = "windows")]
pub mod watch;

#[cfg(target_os = "windows")]
pub use system::{desktop, set_recently, Win};

#[cfg(target_os = "windows")]
use super::Wake;

#[cfg(target_os = "windows")]
mod system {
    use super::{com, picture, shell, startup, watch, Wake};
    use crate::platform::{Desktop, Format, Screen, Snapshot};
    use anyhow::{anyhow, Result};
    use scooch_core::RgbImage;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static LAST_SET: AtomicU64 = AtomicU64::new(0);
    const OWN_ECHO: Duration = Duration::from_secs(1);

    fn now_millis() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
    }

    pub fn set_recently() -> bool {
        now_millis().saturating_sub(LAST_SET.load(Ordering::SeqCst)) < OWN_ECHO.as_millis() as u64
    }

    pub struct Win;

    pub fn desktop() -> Result<Box<dyn Desktop>> {
        com::aware();
        Ok(Box::new(Win))
    }

    impl Desktop for Win {
        fn name(&self) -> String {
            match startup::build() {
                Some(build) if build >= 22000 => format!("Windows 11 build {build}"),
                Some(build) => format!("Windows 10 build {build}"),
                None => "Windows".into(),
            }
        }

        fn snapshot(&self) -> Result<Snapshot> {
            let screens = picture::screens()?;
            let icons = shell::icons()?;
            Ok(Snapshot { screens, icons })
        }

        fn screens(&self) -> Result<Vec<Screen>> {
            picture::screens()
        }

        fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>> {
            picture::wallpaper(screen)
        }

        fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()> {
            LAST_SET.store(now_millis(), Ordering::SeqCst);
            let result = picture::set_wallpaper(screen, path);
            LAST_SET.store(now_millis(), Ordering::SeqCst);
            result
        }

        fn run(&self, wake: Wake) -> Result<()> {
            watch::run(wake)
        }

        fn autostart(&self, exe: Option<&Path>) -> Result<()> {
            match exe {
                Some(exe) => startup::install(exe),
                None => startup::remove(),
            }
        }

        fn check(&self) -> Vec<(String, Result<String>)> {
            vec![
                ("Explorer".into(), check_explorer()),
                ("Desktop".into(), check_desktop()),
                ("Icons".into(), self.check_icons()),
                ("Wallpaper".into(), self.check_wallpaper()),
                ("Login item".into(), check_login()),
            ]
        }

        fn format(&self) -> Format {
            Format::Jpeg
        }

        fn decode(&self, path: &Path) -> Result<RgbImage> {
            picture::decode(path)
        }
    }

    impl Win {
        fn check_icons(&self) -> Result<String> {
            let snapshot = self.snapshot()?;
            let screens = snapshot.screens.len();
            Ok(format!("{} on {}", snapshot.icons.len(), crate::commands::plural(screens, "screen")))
        }

        fn check_wallpaper(&self) -> Result<String> {
            let mut found = Vec::new();
            for screen in self.screens()? {
                let path = self.wallpaper(&screen)?.ok_or_else(|| anyhow!("a solid color or a slideshow, so pick one picture in Settings"))?;
                if !path.is_file() {
                    return Err(anyhow!("{} is not a picture file", path.display()));
                }
                found.push(path.display().to_string());
            }
            Ok(found.join(" and "))
        }
    }

    fn check_explorer() -> Result<String> {
        if shell::explorer() {
            Ok("running".into())
        } else {
            Err(anyhow!("not running, so desktop icons are not shown"))
        }
    }

    fn check_desktop() -> Result<String> {
        let view = shell::view().map_err(|_| anyhow!("not found, so restart Explorer from Task Manager"))?;
        Ok(if shell::visible(&view) { "found".into() } else { "found with icons hidden".into() })
    }

    fn check_login() -> Result<String> {
        Ok(match startup::entry() {
            Some(_) => "installed".into(),
            None => "not installed".into(),
        })
    }
}
