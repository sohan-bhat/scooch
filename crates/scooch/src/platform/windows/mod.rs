pub mod layout;
pub mod wakes;

#[cfg(target_os = "windows")]
pub mod com;
#[cfg(target_os = "windows")]
pub mod layer;
#[cfg(target_os = "windows")]
pub mod picture;
#[cfg(target_os = "windows")]
pub mod shell;
#[cfg(target_os = "windows")]
pub mod sta;
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
    use super::{com, layout, picture, shell, sta, startup, wakes, watch, Wake};
    use crate::platform::{Desktop, Format, Screen, Snapshot};
    use anyhow::{anyhow, Result};
    use scooch_core::RgbImage;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::Instant;

    static CLOCK: OnceLock<Instant> = OnceLock::new();
    static LAST_SET: AtomicU64 = AtomicU64::new(0);

    fn now_millis() -> u64 {
        CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
    }

    pub fn set_recently() -> bool {
        wakes::echo(now_millis(), LAST_SET.load(Ordering::SeqCst))
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
            let icons = sta::call(shell::icons)?;
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
                ("Startup".into(), check_startup()),
                ("Rights".into(), check_rights()),
            ]
        }

        fn format(&self) -> Format {
            Format::Png
        }

        fn decode(&self, path: &Path) -> Result<RgbImage> {
            picture::decode(path)
        }

        fn live(&self) -> bool {
            true
        }

        fn reduce_motion(&self) -> bool {
            animations_off()
        }

        fn hold(&self) -> std::time::Duration {
            std::time::Duration::from_millis(700)
        }

        fn gallery(&self) -> Vec<std::path::PathBuf> {
            std::env::var_os("WINDIR").map(|dir| crate::platform::folder_and_children(&Path::new(&dir).join("Web").join("Wallpaper"))).unwrap_or_default()
        }
    }

    fn animations_off() -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
        let mut enabled = windows::core::BOOL(1);
        let read = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut enabled as *mut windows::core::BOOL as *mut std::ffi::c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
        read.is_ok() && !enabled.as_bool()
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
        let visible = sta::call(|| shell::view().map(|view| shell::visible(&view))).map_err(|_| anyhow!("not found, so restart Explorer from Task Manager"))?;
        Ok(if visible { "found".into() } else { "found with icons hidden".into() })
    }

    fn check_startup() -> Result<String> {
        match startup::approval() {
            Some(flag) if layout::startup_disabled(&flag) => Err(anyhow!("turned off under Startup apps, so run scooch to turn it back on")),
            Some(_) => Ok("allowed under Startup apps".into()),
            None => Ok("no switch set under Startup apps".into()),
        }
    }

    fn check_rights() -> Result<String> {
        if startup::refused() {
            return Err(anyhow!("running as administrator, so run scooch from a normal window"));
        }
        Ok(if startup::own_elevation() { "administrator like Explorer".into() } else { "normal".into() })
    }

    fn check_login() -> Result<String> {
        Ok(match startup::entry() {
            Some(_) => "installed".into(),
            None => "not installed".into(),
        })
    }
}
