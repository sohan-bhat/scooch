use super::sys::{self, Ref, StreamContext};
use super::Wake;
use anyhow::{bail, Context, Result};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDidChangeScreenParametersNotification, NSEvent, NSEventMask, NSRunningApplication, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification, NSWorkspaceApplicationKey, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidMountNotification, NSWorkspaceDidUnmountNotification, NSWorkspaceDidWakeNotification,
};
use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSNotificationName, NSString};
use std::ffi::{c_char, c_void, CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, NonNull};
use std::sync::Arc;

type Shared = Arc<dyn Fn() + Send + Sync>;

const STREAM_LATENCY: f64 = 0.05;
const FINDER: &str = "com.apple.finder";
const LAYOUT: &str = ".DS_Store";
const INDEX: &str = "Library/Application Support/com.apple.wallpaper/Store/Index.plist";

struct Folder {
    wake: Shared,
    desktop: PathBuf,
}

struct File {
    wake: Shared,
    path: CString,
}

struct Armed {
    file: &'static File,
    source: Ref,
    fd: i32,
}

pub fn run(wake: Wake) -> Result<()> {
    let marker = MainThreadMarker::new().context("Scooch must watch from its main thread, so start it with scooch watch.")?;
    let home = super::real_home().context("Scooch could not find your home folder.")?;
    let wake: Shared = Arc::from(wake);
    let traced = crate::daemon::trace();
    let tag = |name: &'static str| -> Shared {
        let wake = wake.clone();
        if traced {
            Arc::new(move || {
                eprintln!("{} wake {name}", crate::daemon::stamp());
                wake()
            })
        } else {
            wake
        }
    };
    let application = NSApplication::sharedApplication(marker);
    application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    desktop_files(tag("desktop"), &home.join("Desktop"))?;
    mouse(tag("drag"));
    let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
    unsafe {
        observe(&workspace, NSWorkspaceActiveSpaceDidChangeNotification, tag("space"));
        observe(&workspace, NSWorkspaceDidWakeNotification, tag("wake"));
        observe(&workspace, NSWorkspaceDidMountNotification, tag("mount"));
        observe(&workspace, NSWorkspaceDidUnmountNotification, tag("unmount"));
        on_finder_launch(&workspace, tag("finder"));
        observe(&NSNotificationCenter::defaultCenter(), NSApplicationDidChangeScreenParametersNotification, tag("screens"));
    }
    wallpaper_file(tag("wallpaper"), &home.join(INDEX));
    application.run();
    Ok(())
}

pub fn desktop_change(path: &Path, flags: u32, desktop: &Path) -> bool {
    if path.parent() != Some(desktop) {
        return false;
    }
    let Some(name) = path.file_name() else { return false };
    if name == LAYOUT {
        return true;
    }
    name.as_bytes().first() != Some(&b'.') && flags & (sys::ITEM_CREATED | sys::ITEM_REMOVED | sys::ITEM_RENAMED) != 0
}

pub fn finder_launched(bundle: Option<&str>) -> bool {
    bundle == Some(FINDER)
}

fn desktop_files(wake: Shared, desktop: &Path) -> Result<()> {
    let desktop = desktop.canonicalize().unwrap_or_else(|_| desktop.to_path_buf());
    let watched = [NSString::from_str(&desktop.to_string_lossy())];
    let list = NSArray::from_retained_slice(&watched);
    let info = Box::into_raw(Box::new(Folder { wake, desktop }));
    let context = StreamContext { version: 0, info: info.cast(), retain: null(), release: null(), describe: null() };
    let flags = sys::STREAM_FILE_EVENTS | sys::STREAM_NO_DEFER | sys::STREAM_IGNORE_SELF;
    let stream = unsafe { sys::FSEventStreamCreate(null(), changed, &context, &*list as *const NSArray<NSString> as Ref, sys::STREAM_SINCE_NOW, STREAM_LATENCY, flags) };
    if stream.is_null() {
        bail!("macOS would not report Desktop changes, so restart Scooch with scooch.");
    }
    unsafe {
        sys::FSEventStreamSetDispatchQueue(stream, sys::main_queue());
        if sys::FSEventStreamStart(stream) == 0 {
            bail!("macOS would not start Desktop change reports, so restart Scooch with scooch.");
        }
    }
    Ok(())
}

extern "C" fn changed(_stream: Ref, info: *mut c_void, count: usize, paths: *const *const c_char, flags: *const u32, _ids: *const u64) {
    let folder = unsafe { &*(info as *const Folder) };
    let hit = (0..count).any(|index| {
        let (path, flags) = unsafe { (CStr::from_ptr(*paths.add(index)), *flags.add(index)) };
        desktop_change(Path::new(std::ffi::OsStr::from_bytes(path.to_bytes())), flags, &folder.desktop)
    });
    if hit {
        (folder.wake)();
    }
}

fn mouse(wake: Shared) {
    let traced = crate::daemon::trace();
    let on_up = RcBlock::new(move |_: NonNull<NSEvent>| {
        let front = finder_in_front();
        if traced {
            eprintln!("{} mouse up, Finder in front {front}", crate::daemon::stamp());
        }
        if front {
            wake();
        }
    });
    std::mem::forget(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseUp, &on_up));
}

unsafe fn on_finder_launch(center: &NSNotificationCenter, wake: Shared) {
    let block = RcBlock::new(move |note: NonNull<NSNotification>| {
        let note = unsafe { note.as_ref() };
        let application: Option<Retained<AnyObject>> = note.userInfo().and_then(|info| unsafe { msg_send![&*info, objectForKey: NSWorkspaceApplicationKey] });
        let bundle = application.as_deref().and_then(|object| object.downcast_ref::<NSRunningApplication>()).and_then(|application| application.bundleIdentifier()).map(|name| name.to_string());
        if finder_launched(bundle.as_deref()) {
            wake();
        }
    });
    std::mem::forget(unsafe { center.addObserverForName_object_queue_usingBlock(Some(NSWorkspaceDidLaunchApplicationNotification), None, None, &block) });
}

fn finder_in_front() -> bool {
    NSWorkspace::sharedWorkspace().frontmostApplication().and_then(|application| application.bundleIdentifier()).is_some_and(|name| name.to_string() == FINDER)
}

unsafe fn observe(center: &NSNotificationCenter, name: &NSNotificationName, wake: Shared) {
    let block = RcBlock::new(move |_: NonNull<NSNotification>| wake());
    std::mem::forget(unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) });
}

fn wallpaper_file(wake: Shared, path: &Path) {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else { return };
    let file: &'static File = Box::leak(Box::new(File { wake, path }));
    arm(file);
}

fn arm(file: &'static File) -> bool {
    let fd = unsafe { libc::open(file.path.as_ptr(), libc::O_EVTONLY) };
    if fd < 0 {
        return false;
    }
    let mask = sys::NOTE_WRITE | sys::NOTE_EXTEND | sys::NOTE_DELETE | sys::NOTE_RENAME;
    let source = unsafe { sys::dispatch_source_create(&sys::_dispatch_source_type_vnode as *const c_void, fd as usize, mask, sys::main_queue()) };
    if source.is_null() {
        unsafe { libc::close(fd) };
        return false;
    }
    let armed = Box::into_raw(Box::new(Armed { file, source, fd }));
    unsafe {
        sys::dispatch_set_context(source, armed.cast());
        sys::dispatch_source_set_event_handler_f(source, touched);
        sys::dispatch_source_set_cancel_handler_f(source, disarmed);
        sys::dispatch_resume(source);
    }
    true
}

extern "C" fn touched(context: *mut c_void) {
    let armed = unsafe { &*(context as *const Armed) };
    let flags = unsafe { sys::dispatch_source_get_data(armed.source) };
    (armed.file.wake)();
    if flags & (sys::NOTE_DELETE | sys::NOTE_RENAME) != 0 {
        unsafe { sys::dispatch_source_cancel(armed.source) };
        arm(armed.file);
    }
}

extern "C" fn disarmed(context: *mut c_void) {
    let armed = unsafe { Box::from_raw(context as *mut Armed) };
    unsafe {
        libc::close(armed.fd);
        sys::dispatch_release(armed.source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODIFIED: u32 = 0x1000;
    const INODE: u32 = 0x400;
    const IS_FILE: u32 = 0x10000;

    fn desktop() -> PathBuf {
        PathBuf::from("/Users/someone/Desktop")
    }

    #[test]
    fn files_coming_and_going_on_the_desktop_wake() {
        for flag in [sys::ITEM_CREATED, sys::ITEM_REMOVED, sys::ITEM_RENAMED] {
            assert!(desktop_change(&desktop().join("notes.txt"), flag | IS_FILE, &desktop()));
        }
    }

    #[test]
    fn a_file_being_written_does_not_wake() {
        assert!(!desktop_change(&desktop().join("download.zip"), MODIFIED | INODE | IS_FILE, &desktop()));
    }

    #[test]
    fn hidden_files_and_nested_files_do_not_wake() {
        assert!(!desktop_change(&desktop().join(".localized"), sys::ITEM_CREATED | IS_FILE, &desktop()));
        assert!(!desktop_change(&desktop().join("project/build.o"), sys::ITEM_CREATED | IS_FILE, &desktop()));
        assert!(!desktop_change(&desktop().join("project/.DS_Store"), MODIFIED | IS_FILE, &desktop()));
    }

    #[test]
    fn a_rearranged_desktop_wakes_through_its_layout_file() {
        assert!(desktop_change(&desktop().join(".DS_Store"), MODIFIED | INODE | IS_FILE, &desktop()));
        assert!(desktop_change(&desktop().join(".DS_Store"), sys::ITEM_RENAMED | IS_FILE, &desktop()));
    }

    #[test]
    fn only_finder_launching_wakes() {
        assert!(finder_launched(Some("com.apple.finder")));
        assert!(!finder_launched(Some("com.apple.Safari")));
        assert!(!finder_launched(None));
    }
}
