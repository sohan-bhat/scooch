mod finder;
mod launchd;
mod picture;
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

static RUNNING: AtomicBool = AtomicBool::new(false);

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
            let path = self.wallpaper(&screen)?.ok_or_else(|| anyhow!("macOS reports no wallpaper"))?;
            if !path.is_file() {
                return Err(anyhow!("{} is not a picture file", path.display()));
            }
            found.push(path.display().to_string());
        }
        Ok(found.join(" and "))
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
