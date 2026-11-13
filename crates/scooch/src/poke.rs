use anyhow::Result;

#[cfg(unix)]
mod unix {
    use anyhow::{bail, Result};
    use std::ffi::c_int;
    use std::sync::atomic::{AtomicI32, Ordering};

    static WRITE: AtomicI32 = AtomicI32::new(-1);
    const WAKE: u8 = 1;
    const STOP: u8 = 2;

    extern "C" fn poked(signal: c_int) {
        let fd = WRITE.load(Ordering::Relaxed);
        if fd >= 0 {
            let byte = if signal == libc::SIGTERM { STOP } else { WAKE };
            unsafe { libc::write(fd, &byte as *const u8 as *const libc::c_void, 1) };
        }
    }

    pub fn listen(wake: impl Fn() + Send + 'static, stop: impl Fn() + Send + 'static) -> Result<()> {
        let mut ends = [0 as c_int; 2];
        if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
            bail!("Scooch could not open a pipe, so restart it with scooch.");
        }
        unsafe {
            libc::fcntl(ends[1], libc::F_SETFL, libc::O_NONBLOCK);
            libc::fcntl(ends[0], libc::F_SETFD, libc::FD_CLOEXEC);
            libc::fcntl(ends[1], libc::F_SETFD, libc::FD_CLOEXEC);
        }
        WRITE.store(ends[1], Ordering::SeqCst);
        let handler = poked as extern "C" fn(c_int) as libc::sighandler_t;
        unsafe {
            libc::signal(libc::SIGUSR1, handler);
            libc::signal(libc::SIGTERM, handler);
        }
        std::thread::Builder::new().name("scooch-poke".into()).spawn(move || {
            let mut buffer = [0u8; 64];
            loop {
                let read = unsafe { libc::read(ends[0], buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) };
                if read > 0 {
                    let bytes = &buffer[..read as usize];
                    if bytes.contains(&STOP) {
                        stop();
                    } else {
                        wake();
                    }
                } else if read == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                    break;
                }
            }
        })?;
        Ok(())
    }

    pub fn send(process: u32) -> bool {
        alive(process) && unsafe { libc::kill(process as libc::pid_t, libc::SIGUSR1) } == 0
    }

    pub fn stop(process: u32) -> bool {
        alive(process) && unsafe { libc::kill(process as libc::pid_t, libc::SIGTERM) } == 0
    }

    pub fn alive(process: u32) -> bool {
        if process <= 1 || process > i32::MAX as u32 {
            return false;
        }
        let exists = unsafe { libc::kill(process as libc::pid_t, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        exists && named_scooch(process)
    }

    #[cfg(target_os = "macos")]
    fn named_scooch(process: u32) -> bool {
        use std::os::unix::ffi::OsStrExt;
        let mut buffer = vec![0u8; 4096];
        let length = unsafe { libc::proc_pidpath(process as c_int, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        length > 0 && super::scooch_name(std::path::Path::new(std::ffi::OsStr::from_bytes(&buffer[..length as usize])))
    }

    #[cfg(target_os = "linux")]
    fn named_scooch(process: u32) -> bool {
        std::fs::read_link(format!("/proc/{process}/exe")).is_ok_and(|exe| super::scooch_name(&exe)) || std::fs::read_to_string(format!("/proc/{process}/comm")).is_ok_and(|name| name.trim().starts_with("scooch"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn named_scooch(_process: u32) -> bool {
        true
    }
}

pub fn scooch_name(exe: &std::path::Path) -> bool {
    exe.file_name().is_some_and(|name| name.to_string_lossy().to_ascii_lowercase().starts_with("scooch"))
}

#[cfg(unix)]
pub use unix::{alive, send, stop};

#[cfg(unix)]
pub fn listen(wake: impl Fn() + Send + 'static, stop: impl Fn() + Send + 'static) -> Result<()> {
    unix::listen(wake, stop)
}

#[cfg(windows)]
mod named {
    use anyhow::{anyhow, Result};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{
        CreateEventW, OpenEventW, OpenProcess, QueryFullProcessImageNameW, SetEvent, WaitForMultipleObjects, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    pub const POKE: &str = "poke";
    pub const STOP: &str = "stop";

    struct Events([HANDLE; 2]);

    unsafe impl Send for Events {}

    pub fn name(folder: &Path, kind: &str) -> String {
        format!("Local\\scooch-{:016x}-{kind}", crate::cache::digest(folder.to_string_lossy().as_bytes()))
    }

    fn here(kind: &str) -> HSTRING {
        HSTRING::from(name(&crate::state::Store::open().root, kind))
    }

    fn create(kind: &str) -> Result<HANDLE> {
        unsafe { CreateEventW(None, false, false, &here(kind)) }.map_err(|_| anyhow!("Scooch could not open its wake signal, so restart it with scooch."))
    }

    pub fn listen(wake: impl Fn() + Send + 'static, stop: impl Fn() + Send + 'static) -> Result<()> {
        let events = Events([create(POKE)?, create(STOP)?]);
        std::thread::Builder::new().name("scooch-poke".into()).spawn(move || {
            let events = events;
            loop {
                let woke = unsafe { WaitForMultipleObjects(&events.0, false, INFINITE) };
                if woke == WAIT_OBJECT_0 {
                    wake();
                } else if woke.0 == WAIT_OBJECT_0.0 + 1 {
                    stop();
                } else {
                    break;
                }
            }
        })?;
        Ok(())
    }

    pub fn signal(kind: &str) -> bool {
        let Ok(event) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &here(kind)) }) else { return false };
        let set = unsafe { SetEvent(event) }.is_ok();
        let _ = unsafe { CloseHandle(event) };
        set
    }

    pub fn alive(process: u32) -> bool {
        if process == 0 {
            return false;
        }
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, process) }) else { return false };
        let running = unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT;
        let named = running && image(handle).is_some_and(|path| super::scooch_name(&path));
        let _ = unsafe { CloseHandle(handle) };
        named
    }

    fn image(handle: HANDLE) -> Option<PathBuf> {
        let mut buffer = vec![0u16; 32_768];
        let mut size = buffer.len() as u32;
        unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut size) }.ok()?;
        let length = (size as usize).min(buffer.len());
        Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::Win32::System::Threading::WaitForSingleObject;

        #[test]
        fn names_are_local_and_follow_the_folder() {
            let one = name(Path::new(r"C:\Users\a\AppData\Local\Scooch"), POKE);
            let two = name(Path::new(r"C:\Users\b\AppData\Local\Scooch"), POKE);
            assert!(one.starts_with("Local\\scooch-") && one.ends_with("-poke") && one.len() == 34, "{one}");
            assert_ne!(one, two);
        }

        #[test]
        fn a_signal_reaches_a_named_event() {
            let kind = format!("test-{}", std::process::id());
            let event = create(&kind).unwrap();
            assert!(signal(&kind));
            assert_eq!(unsafe { WaitForSingleObject(event, 1000) }, WAIT_OBJECT_0);
            assert!(!signal("missing-event"));
            let _ = unsafe { CloseHandle(event) };
        }

        #[test]
        fn a_running_scooch_is_alive_and_others_are_not() {
            assert!(alive(std::process::id()));
            assert!(!alive(0));
            assert!(!alive(u32::MAX));
            let mut child = std::process::Command::new("cmd").args(["/C", "ping -n 6 127.0.0.1 > NUL"]).spawn().unwrap();
            assert!(!alive(child.id()));
            let _ = child.kill();
            let _ = child.wait();
            assert!(!alive(child.id()));
        }
    }
}

#[cfg(windows)]
pub fn listen(wake: impl Fn() + Send + 'static, stop: impl Fn() + Send + 'static) -> Result<()> {
    named::listen(wake, stop)
}

#[cfg(windows)]
pub fn send(_process: u32) -> bool {
    named::signal(named::POKE)
}

#[cfg(windows)]
pub fn stop(_process: u32) -> bool {
    named::signal(named::STOP)
}

#[cfg(windows)]
pub fn alive(process: u32) -> bool {
    named::alive(process)
}

#[cfg(not(any(unix, windows)))]
pub fn listen(_wake: impl Fn() + Send + 'static, _stop: impl Fn() + Send + 'static) -> Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub fn send(_process: u32) -> bool {
    false
}

#[cfg(not(any(unix, windows)))]
pub fn stop(_process: u32) -> bool {
    false
}

#[cfg(not(any(unix, windows)))]
pub fn alive(_process: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scooch_programs_count() {
        assert!(scooch_name(std::path::Path::new("/opt/bin/scooch")));
        assert!(scooch_name(std::path::Path::new("/Applications/Scooch/scoochw.exe")));
        assert!(!scooch_name(std::path::Path::new("/usr/bin/zsh")));
    }

    #[cfg(unix)]
    #[test]
    fn a_running_scooch_is_alive_and_others_are_not() {
        assert!(alive(std::process::id()));
        assert!(!alive(0) && !alive(1));
        assert!(!alive(u32::MAX));
        let mut child = std::process::Command::new("/bin/sleep").arg("5").spawn().unwrap();
        assert!(!alive(child.id()));
        assert!(!send(child.id()));
        let _ = child.kill();
        let _ = child.wait();
    }
}
