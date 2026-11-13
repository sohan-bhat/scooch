mod finder;
mod launchd;
mod picture;
mod spaces;
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static RUNNING: AtomicBool = AtomicBool::new(false);
const GALLERY: [&str; 2] = ["/System/Library/Desktop Pictures", "/Library/Desktop Pictures"];
const CONSENT_HINT: &str = "If macOS asks to let scooch control Finder, choose Allow.";
pub fn real_home() -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;
    let mut record: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buffer = vec![0 as libc::c_char; 4096];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let status = unsafe { libc::getpwuid_r(libc::getuid(), &mut record, buffer.as_mut_ptr(), buffer.len(), &mut found) };
    if status != 0 || found.is_null() || record.pw_dir.is_null() {
        return None;
    }
    let home = unsafe { CStr::from_ptr(record.pw_dir) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(home.to_bytes()))).filter(|path| path.is_absolute())
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
        on_main(move |marker| picture::set_wallpaper(marker, &id, &path))
    }

    fn read_lag(&self) -> Duration {
        Duration::ZERO
    }

    fn known(&self) -> Result<Option<Vec<String>>> {
        let mut found = Vec::new();
        for screen in self.screens()? {
            let (display, _) = spaces::split(&screen.id);
            let Some(every) = spaces::all(display) else { return Ok(None) };
            found.extend(every.iter().map(|space| spaces::join(display, Some(space))));
        }
        Ok(Some(found))
    }

    fn in_use(&self, folder: &Path) -> Result<Option<Vec<PathBuf>>> {
        Ok(store::in_use(folder))
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

    fn gallery(&self) -> Vec<PathBuf> {
        GALLERY.iter().map(PathBuf::from).collect()
    }

    fn profile(&self) -> Option<Vec<u8>> {
        picture::profile()
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
    fn the_real_home_ignores_the_home_variable() {
        let home = real_home().unwrap();
        assert!(home.is_absolute() && home.is_dir());
        assert!(home.join("Library").is_dir());
    }

    #[test]
    fn live_the_gallery_has_system_pictures() {
        if !live() {
            return;
        }
        let found = crate::commands::listing(&[], &Mac.gallery());
        assert!(found.iter().any(|path| path.extension().is_some_and(|kind| kind == "heic")), "{found:?}");
        assert!(found.iter().all(|path| path.extension().is_none_or(|kind| kind != "madesktop")));
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
            let (display, space) = spaces::split(&screen.id);
            assert_eq!(display.len(), 36, "{}", screen.id);
            assert!(space.is_some_and(|space| space.len() == 36), "{}", screen.id);
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

}
