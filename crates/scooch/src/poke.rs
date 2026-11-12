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

#[cfg(not(unix))]
pub fn listen(_wake: impl Fn() + Send + 'static) -> Result<()> {
    Ok(())
}

#[cfg(not(unix))]
pub fn send(_process: u32) -> bool {
    false
}

#[cfg(not(unix))]
pub fn stop(_process: u32) -> bool {
    false
}
