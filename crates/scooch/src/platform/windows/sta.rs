use super::com;
use anyhow::{anyhow, Result};
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::OnceLock;
use std::time::Duration;
use ::windows::core::PCWSTR;
use ::windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use ::windows::Win32::System::Threading::{CreateEventW, SetEvent, INFINITE};
use ::windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, QS_ALLINPUT};

pub const PATIENCE: Duration = Duration::from_secs(5);
pub const SLOW: &str = "The desktop icons took over five seconds to read, so Scooch will try again at the next change.";

type Job = Box<dyn FnOnce() + Send>;

struct Worker {
    jobs: Sender<Job>,
    signal: usize,
}

static WORKER: OnceLock<Option<Worker>> = OnceLock::new();

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

fn worker() -> Option<&'static Worker> {
    WORKER.get_or_init(start).as_ref()
}

fn start() -> Option<Worker> {
    let signal = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.ok()?;
    let raw = signal.0 as usize;
    let (jobs, inbox) = mpsc::channel::<Job>();
    std::thread::Builder::new().name("scooch-shell".into()).spawn(move || serve(raw, inbox)).ok()?;
    Some(Worker { jobs, signal: raw })
}

fn serve(signal: usize, inbox: Receiver<Job>) {
    INSIDE.with(|inside| inside.set(true));
    let _ = com::enter(true);
    let handles = [HANDLE(signal as *mut c_void)];
    loop {
        let woke = unsafe { MsgWaitForMultipleObjects(Some(&handles), false, INFINITE, QS_ALLINPUT) };
        if woke == WAIT_OBJECT_0 {
            loop {
                match inbox.try_recv() {
                    Ok(job) => job(),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
        } else if woke.0 == WAIT_OBJECT_0.0 + 1 {
            drain();
        } else {
            return;
        }
    }
}

fn drain() {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
}

pub fn post(job: impl FnOnce() + Send + 'static) -> bool {
    let Some(worker) = worker() else { return false };
    if worker.jobs.send(Box::new(job)).is_err() {
        return false;
    }
    unsafe { SetEvent(HANDLE(worker.signal as *mut c_void)) }.is_ok()
}

pub fn call<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    if INSIDE.with(Cell::get) {
        return work();
    }
    let (reply, answer) = mpsc::sync_channel(1);
    if !post(move || {
        let _ = reply.send(work());
    }) {
        return Err(anyhow!("Scooch could not start its desktop reader, so restart it with scooch."));
    }
    answer.recv_timeout(PATIENCE).map_err(|_| anyhow!(SLOW))?
}
