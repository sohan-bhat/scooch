mod finder;
mod launchd;
mod picture;
mod store;
mod sys;
mod watch;

use super::{Desktop, Format, Screen, Snapshot, Wake};
use anyhow::{anyhow, Result};
use objc2::rc::autoreleasepool;
use objc2::MainThreadMarker;
use objc2_app_kit::NSRunningApplication;
use objc2_foundation::{ns_string, NSProcessInfo};
use scooch_core::RgbImage;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static RUNNING: AtomicBool = AtomicBool::new(false);
const CONSENT_HINT: &str = "If macOS asks to let scooch control Finder, choose Allow.";
static LAST_SET: AtomicU64 = AtomicU64::new(0);
const OWN_ECHO: Duration = Duration::from_secs(1);

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

pub fn set_recently() -> bool {
    now_millis().saturating_sub(LAST_SET.load(Ordering::SeqCst)) < OWN_ECHO.as_millis() as u64
}

pub fn on_main<T: Send>(work: impl FnOnce(MainThreadMarker) -> T + Send) -> T {
    if let Some(marker) = MainThreadMarker::new() {
        return work(marker);
    }
    if !RUNNING.load(Ordering::SeqCst) {
        return work(unsafe { MainThreadMarker::new_unchecked() });
    }
    let mut work = Some(work);
    let mut result = None;
    let mut call = || result = work.take().map(|work| autoreleasepool(|_| work(unsafe { MainThreadMarker::new_unchecked() })));
    unsafe { sys::dispatch_sync_f(sys::main_queue(), &mut call as *mut _ as *mut c_void, trampoline(&call)) };
    result.expect("the main thread ran the work")
}

fn trampoline<F: FnMut()>(_: &F) -> sys::Work {
    extern "C" fn call<F: FnMut()>(context: *mut c_void) {
        unsafe { (*(context as *mut F))() }
    }
    call::<F>
}

struct Mac;

pub fn desktop() -> Result<Box<dyn Desktop>> {
    Ok(Box::new(Mac))
}

impl Desktop for Mac {
    fn name(&self) -> String {
        let version = NSProcessInfo::processInfo().operatingSystemVersion();
        format!("macOS {}.{}.{}", version.majorVersion, version.minorVersion, version.patchVersion)
    }

    fn snapshot(&self) -> Result<Snapshot> {
        let screens = self.screens()?;
        let icons = autoreleasepool(|_| finder::icons(&finder::view()))?;
        Ok(Snapshot { screens, icons })
    }

    fn screens(&self) -> Result<Vec<Screen>> {
        on_main(picture::screens)
    }

    fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>> {
        let id = screen.id.clone();
        on_main(move |marker| picture::wallpaper(marker, &id))
    }

    fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()> {
        let id = screen.id.clone();
        let path = path.to_path_buf();
        let result = on_main(move |marker| picture::set_wallpaper(marker, &id, &path));
        LAST_SET.store(now_millis(), Ordering::SeqCst);
        result
    }

    fn run(&self, wake: Wake) -> Result<()> {
        RUNNING.store(true, Ordering::SeqCst);
        let result = watch::run(wake);
        RUNNING.store(false, Ordering::SeqCst);
        result
    }

    fn autostart(&self, exe: Option<&Path>) -> Result<()> {
        match exe {
            Some(exe) => launchd::install(exe, &crate::state::folder()),
            None => launchd::remove(),
        }
    }

    fn check(&self) -> Vec<(String, Result<String>)> {
        autoreleasepool(|_| {
            vec![
                ("Finder".into(), check_finder()),
                ("Automation".into(), check_access()),
                ("Icons".into(), self.check_icons()),
                ("Wallpaper".into(), self.check_wallpaper()),
                ("Login item".into(), check_login()),
            ]
        })
    }

    fn format(&self) -> Format {
        Format::Jpeg
    }

    fn decode(&self, path: &Path) -> Result<RgbImage> {
        autoreleasepool(|_| picture::decode(path))
    }

    fn consent_hint(&self) -> Option<&'static str> {
        Some(CONSENT_HINT)
    }

    fn restorable(&self, screen: &Screen) -> Result<Option<PathBuf>> {
        let id = screen.id.clone();
        on_main(move |marker| picture::restorable(marker, &id))
    }
}

impl Mac {
    fn check_icons(&self) -> Result<String> {
        if finder::access(false) != finder::Access::Allowed {
            return Err(anyhow!("not read until Automation is allowed"));
        }
        let view = finder::view();
        if view.hidden {
            return Ok("hidden, so the picture stays where it is".into());
        }
        let snapshot = self.snapshot()?;
        let count = snapshot.icons.len();
        let screens = snapshot.screens.len();
        let mut line = format!("{count} on {screens} {}", if screens == 1 { "screen" } else { "screens" });
        if view.stacked() {
            line.push_str(", stacks on");
        } else if view.sorted() {
            line.push_str(&format!(", sorted by {}", view.arrange));
        }
        Ok(line)
    }

    fn check_wallpaper(&self) -> Result<String> {
        let mut found = Vec::new();
        for screen in self.screens()? {
            let Some(path) = self.wallpaper(&screen)?.filter(|path| path.is_file()) else {
                return Err(anyhow!("{}, so Scooch leaves it alone until you pick a picture file", kind_words(store::describe(&screen.id))));
            };
            found.push(path.display().to_string());
        }
        Ok(found.join(" and "))
    }
}

fn kind_words(kind: Option<store::Kind>) -> &'static str {
    match kind {
        Some(store::Kind::Default) => "the macOS default wallpaper, which moves",
        Some(store::Kind::Picture(_)) => "a picture macOS cannot read",
        Some(store::Kind::Other(_)) => "a dynamic, shuffled or color wallpaper",
        None => "not a picture file",
    }
}

fn check_finder() -> Result<String> {
    let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(ns_string!("com.apple.finder"));
    if running.is_empty() {
        return Err(anyhow!("not running, so desktop icons are not shown"));
    }
    Ok("running".into())
}

fn check_access() -> Result<String> {
    match finder::access(false) {
        finder::Access::Allowed => Ok("allowed for this app".into()),
        finder::Access::WouldAsk => Err(anyhow!("not asked yet, macOS asks the first time Scooch reads the desktop")),
        finder::Access::Denied => Err(anyhow!("turned off, so turn on Finder for scooch in System Settings, Privacy and Security, Automation")),
        finder::Access::FinderOff => Err(anyhow!("unknown while Finder is not running")),
        finder::Access::Unknown(status) => Err(anyhow!("macOS answered with code {status}")),
    }
}

fn check_login() -> Result<String> {
    Ok(match (launchd::installed(), launchd::loaded()) {
        (true, true) => "installed and running".into(),
        (true, false) => "installed, starts at the next login".into(),
        (false, _) => "not installed".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn live() -> bool {
        std::env::var_os("SCOOCH_LIVE").is_some_and(|value| value == "1")
    }

    #[test]
    fn the_consent_hint_names_finder() {
        assert_eq!(Mac.consent_hint(), Some("If macOS asks to let scooch control Finder, choose Allow."));
    }

    #[test]
    fn live_screens_have_stable_ids_and_scales() {
        if !live() {
            return;
        }
        let screens = Mac.screens().unwrap();
        assert!(!screens.is_empty());
        for screen in &screens {
            assert_eq!(screen.id.len(), 36, "{}", screen.id);
            assert!(screen.scale >= 1.0 && screen.frame.w > 0.0 && screen.frame.h > 0.0);
        }
        assert_eq!(screens, Mac.screens().unwrap());
    }

    #[test]
    fn live_wallpaper_decodes_through_imageio() {
        if !live() {
            return;
        }
        let screen = Mac.screens().unwrap().remove(0);
        let shown = Mac.wallpaper(&screen).unwrap();
        assert!(shown.is_some() || store::describe(&screen.id).is_some());
        let path = shown.unwrap_or_else(|| PathBuf::from(store::DEFAULT_DESKTOP));
        assert!(path.is_file(), "{}", path.display());
        let image = Mac.decode(&path).unwrap();
        assert!(image.width() > 0 && image.height() > 0);
    }

    #[test]
    fn live_a_probe_icon_is_found_where_finder_put_it() {
        if !live() {
            return;
        }
        let home = dirs::home_dir().unwrap();
        let name = format!("scooch-probe-live-{}", std::process::id());
        let probe = home.join("Desktop").join(&name);
        std::fs::write(&probe, b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let script = format!("tell application \"Finder\" to set desktop position of item \"{name}\" of desktop to {{333, 444}}");
        let moved = Command::new("/usr/bin/osascript").arg("-e").arg(script).output();
        let snapshot = Mac.snapshot();
        std::fs::remove_file(&probe).unwrap();
        assert!(moved.unwrap().status.success());
        let icons = snapshot.unwrap().icons;
        let found = icons.iter().find(|icon| icon.x < 333.0 && icon.x + icon.w > 333.0 && icon.y < 444.0 && icon.y + icon.h > 444.0).copied();
        let found = found.expect("no icon covers the probe");
        assert!((found.y - (444.0 - 32.0)).abs() < 0.5, "{found:?}");
        assert!((found.x + found.w / 2.0 - 333.0).abs() < 0.5, "{found:?}");
    }

    #[test]
    fn live_the_login_item_installs_and_leaves_no_trace() {
        if !live() {
            return;
        }
        let folder = crate::state::scratch("launchd");
        launchd::install(Path::new("/usr/bin/true"), &folder).unwrap();
        let installed = (launchd::installed(), launchd::loaded());
        launchd::remove().unwrap();
        assert_eq!(installed, (true, true));
        assert!(!launchd::installed() && !launchd::loaded());
    }
}
