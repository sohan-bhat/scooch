use std::path::Path;
use std::time::Duration;

const QUIET: Duration = Duration::from_millis(120);

#[cfg(not(windows))]
pub use notify::RecommendedWatcher as Watch;

#[cfg(not(windows))]
pub fn changes(kind: &notify::EventKind) -> bool {
    use notify::event::{AccessKind, AccessMode};
    use notify::EventKind;
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

#[cfg(not(windows))]
pub fn follow(folder: &Path, changed: impl Fn() + Send + 'static) -> std::io::Result<Watch> {
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc;
    let failed = |error: notify::Error| std::io::Error::other(error.to_string());
    let _ = std::fs::create_dir_all(folder);
    let (send, receive) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| changes(&event.kind)) {
            let _ = send.send(());
        }
    })
    .map_err(failed)?;
    watcher.watch(folder, RecursiveMode::NonRecursive).map_err(failed)?;
    std::thread::Builder::new().name("scooch-app-watch".into()).spawn(move || {
        while receive.recv().is_ok() {
            while receive.recv_timeout(QUIET).is_ok() {}
            changed();
        }
    })?;
    Ok(watcher)
}

#[cfg(windows)]
pub struct Watch {
    stop: isize,
}

#[cfg(windows)]
impl Drop for Watch {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::System::Threading::SetEvent(self.stop as windows_sys::Win32::Foundation::HANDLE) };
    }
}

#[cfg(windows)]
pub fn follow(folder: &Path, changed: impl Fn() + Send + 'static) -> std::io::Result<Watch> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
    use windows_sys::Win32::Storage::FileSystem::{FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE};
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};
    let _ = std::fs::create_dir_all(folder);
    let wide: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
    let change = unsafe { FindFirstChangeNotificationW(wide.as_ptr(), 0, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE) };
    if change == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if stop.is_null() {
        unsafe { FindCloseChangeNotification(change) };
        return Err(std::io::Error::last_os_error());
    }
    let (change, stop) = (change as isize, stop as isize);
    std::thread::Builder::new().name("scooch-app-watch".into()).spawn(move || {
        let handles: [HANDLE; 2] = [change as HANDLE, stop as HANDLE];
        'follow: loop {
            if unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) } != WAIT_OBJECT_0 {
                break;
            }
            loop {
                if unsafe { FindNextChangeNotification(handles[0]) } == 0 {
                    break 'follow;
                }
                match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, QUIET.as_millis() as u32) } {
                    WAIT_OBJECT_0 => continue,
                    woke if woke == WAIT_OBJECT_0 + 1 => break 'follow,
                    _ => break,
                }
            }
            changed();
        }
        unsafe {
            FindCloseChangeNotification(handles[0]);
            CloseHandle(handles[1]);
        }
    })?;
    Ok(Watch { stop })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[cfg(not(windows))]
    #[test]
    fn opening_and_reading_are_no_change_and_every_write_is_one() {
        use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, ModifyKind, RemoveKind};
        use notify::EventKind;
        assert!(!changes(&EventKind::Access(AccessKind::Open(AccessMode::Any))));
        assert!(!changes(&EventKind::Access(AccessKind::Read)));
        assert!(!changes(&EventKind::Access(AccessKind::Close(AccessMode::Read))));
        assert!(changes(&EventKind::Access(AccessKind::Close(AccessMode::Write))));
        assert!(changes(&EventKind::Modify(ModifyKind::Data(DataChange::Any))));
        assert!(changes(&EventKind::Create(CreateKind::File)));
        assert!(changes(&EventKind::Remove(RemoveKind::File)));
    }

    #[test]
    fn a_follower_that_reads_the_folder_does_not_wake_itself() {
        let folder = std::env::temp_dir().join(format!("scooch-app-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let state = folder.join("state.json");
        std::fs::write(&state, b"{}").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let woke = Arc::new(AtomicUsize::new(0));
        let (count, read) = (woke.clone(), state.clone());
        let watch = follow(&folder, move || {
            count.fetch_add(1, Ordering::SeqCst);
            let _ = std::fs::read(&read);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::read(&state);
        std::thread::sleep(Duration::from_millis(800));
        let after_read = woke.load(Ordering::SeqCst);
        let staged = folder.join("state.json.new");
        std::fs::write(&staged, b"{\"on\": true}").unwrap();
        std::fs::rename(&staged, &state).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        let after_write = woke.load(Ordering::SeqCst);
        drop(watch);
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(&state, b"{}").unwrap();
        std::thread::sleep(Duration::from_millis(800));
        let after_drop = woke.load(Ordering::SeqCst);
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(after_read, 0);
        assert!((1..=2).contains(&after_write), "{after_write}");
        assert_eq!(after_drop, after_write);
    }
}
