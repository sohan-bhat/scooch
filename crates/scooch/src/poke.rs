use anyhow::Result;

#[cfg(unix)]
mod unix {
    use anyhow::{bail, Result};
    use std::ffi::c_int;
    use std::sync::atomic::{AtomicI32, Ordering};

    static WRITE: AtomicI32 = AtomicI32::new(-1);

    extern "C" fn poked(_signal: c_int) {
        let fd = WRITE.load(Ordering::Relaxed);
        if fd >= 0 {
            let byte = 1u8;
            unsafe { libc::write(fd, &byte as *const u8 as *const libc::c_void, 1) };
        }
    }

    pub fn listen(wake: impl Fn() + Send + 'static) -> Result<()> {
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
        unsafe { libc::signal(libc::SIGUSR1, handler) };
        std::thread::Builder::new().name("scooch-poke".into()).spawn(move || {
            let mut buffer = [0u8; 64];
            loop {
                let read = unsafe { libc::read(ends[0], buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) };
                if read > 0 {
                    wake();
                } else if read == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                    break;
                }
            }
        })?;
        Ok(())
    }

    pub fn send(process: u32) -> bool {
        process > 1 && unsafe { libc::kill(process as libc::pid_t, libc::SIGUSR1) } == 0
    }

    pub fn stop(process: u32) -> bool {
        process > 1 && unsafe { libc::kill(process as libc::pid_t, libc::SIGTERM) } == 0
    }
}

#[cfg(unix)]
pub use unix::{send, stop};

#[cfg(unix)]
pub fn listen(wake: impl Fn() + Send + 'static) -> Result<()> {
    unix::listen(wake)
}

#[cfg(windows)]
mod named {
    use anyhow::{anyhow, Result};
    use std::path::Path;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateEventW, OpenEventW, SetEvent, WaitForMultipleObjects, EVENT_MODIFY_STATE, INFINITE};

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

    pub fn listen(wake: impl Fn() + Send + 'static) -> Result<()> {
        let events = Events([create(POKE)?, create(STOP)?]);
        std::thread::Builder::new().name("scooch-poke".into()).spawn(move || {
            let events = events;
            loop {
                let woke = unsafe { WaitForMultipleObjects(&events.0, false, INFINITE) };
                if woke == WAIT_OBJECT_0 {
                    wake();
                } else if woke.0 == WAIT_OBJECT_0.0 + 1 {
                    std::process::exit(0);
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
    }
}

#[cfg(windows)]
pub fn listen(wake: impl Fn() + Send + 'static) -> Result<()> {
    named::listen(wake)
}

#[cfg(windows)]
pub fn send(_process: u32) -> bool {
    named::signal(named::POKE)
}

#[cfg(windows)]
pub fn stop(_process: u32) -> bool {
    named::signal(named::STOP)
}

#[cfg(not(any(unix, windows)))]
pub fn listen(_wake: impl Fn() + Send + 'static) -> Result<()> {
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
