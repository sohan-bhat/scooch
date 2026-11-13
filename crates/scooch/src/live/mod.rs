pub mod helper;

use crate::platform::Screen;
use anyhow::{anyhow, bail, Context, Result};
use scooch_core::Rect;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const HELPER: &str = "--live-helper";
pub const VERSION: u32 = 2;
const QUIT_WAIT: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    Hello { version: u32 },
    Warm { #[serde(default)] hints: Hints, #[serde(default)] picture: Option<PathBuf>, #[serde(default)] size: Option<[u32; 2]> },
    Open { id: u32, screen: String, rect: Rect, #[serde(default = "one")] scale: f32, #[serde(default)] still: Option<PathBuf> },
    Glide { id: u32, a: PathBuf, b: PathBuf, ms: u32 },
    Settle { id: u32, still: PathBuf, #[serde(default)] ms: Option<u32> },
    Close { id: u32 },
    Quit,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Hello { version: u32, pid: u32 },
    Ready { adapter: String, ms: u64, #[serde(default)] uploads: u32 },
    Shown { id: u32, ms: u64 },
    Glided { id: u32, frames: u32, slow_frames: u32, first_frame_ms: u64, #[serde(default)] uploads: u32 },
    Settled { id: u32 },
    Closed { id: u32 },
    Failed { #[serde(default)] id: Option<u32>, why: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub adapter: String,
    pub ms: u64,
    pub uploads: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glided {
    pub frames: u32,
    pub slow_frames: u32,
    pub first_frame_ms: u64,
    pub uploads: u32,
}

pub fn line<T: Serialize>(message: &T) -> String {
    serde_json::to_string(message).unwrap_or_default() + "\n"
}

pub struct Helper {
    child: Child,
    input: Option<ChildStdin>,
    replies: mpsc::Receiver<Reply>,
    #[cfg(windows)]
    guard: Option<job::Guard>,
    pub pid: u32,
}

impl Helper {
    pub fn spawn(program: &Path, timeout: Duration) -> Result<Helper> {
        let mut command = std::process::Command::new(program);
        command.arg(HELPER).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(job::CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().with_context(|| format!("could not start {}", program.display()))?;
        #[cfg(windows)]
        let guard = job::guard(&child).ok();
        let output = child.stdout.take().context("the helper has no output")?;
        let (sender, replies) = mpsc::channel();
        std::thread::Builder::new().name("scooch-live-replies".into()).spawn(move || {
            for text in BufReader::new(output).lines() {
                let Ok(text) = text else { break };
                let reply = serde_json::from_str::<Reply>(&text).unwrap_or_else(|error| Reply::Failed { id: None, why: format!("unreadable line from the helper {error}") });
                if sender.send(reply).is_err() {
                    break;
                }
            }
        })?;
        let pid = child.id();
        let input = child.stdin.take();
        let mut helper = Helper {
            child,
            input,
            replies,
            #[cfg(windows)]
            guard,
            pid,
        };
        helper.send(&Command::Hello { version: VERSION })?;
        helper.expect(timeout, |reply| match reply {
            Reply::Hello { version, .. } if *version == VERSION => Some(Ok(())),
            Reply::Hello { version, .. } => Some(Err(anyhow!("the helper speaks version {version}, not {VERSION}"))),
            Reply::Failed { id: None, why } => Some(Err(anyhow!("{why}"))),
            _ => None,
        })?;
        Ok(helper)
    }

    pub fn send(&mut self, command: &Command) -> Result<()> {
        let input = self.input.as_mut().context("the pipe to the helper is closed")?;
        input.write_all(line(command).as_bytes())?;
        input.flush()?;
        Ok(())
    }

    pub fn expect<T>(&mut self, timeout: Duration, mut pick: impl FnMut(&Reply) -> Option<Result<T>>) -> Result<T> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.replies.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(reply) => {
                    if let Some(result) = pick(&reply) {
                        return result;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => bail!("the helper did not answer within {} ms", timeout.as_millis()),
                Err(mpsc::RecvTimeoutError::Disconnected) => bail!("the helper is gone"),
            }
        }
    }

    pub fn next(&mut self, timeout: Duration) -> Option<Reply> {
        self.replies.recv_timeout(timeout).ok()
    }

    fn failure(reply: &Reply, wanted: Option<u32>) -> Option<anyhow::Error> {
        match reply {
            Reply::Failed { id, why } if id.is_none() || *id == wanted => Some(anyhow!("{why}")),
            _ => None,
        }
    }

    pub fn warm(&mut self, hints: Hints, picture: Option<&Path>, size: Option<[u32; 2]>, timeout: Duration) -> Result<Ready> {
        self.send(&Command::Warm { hints, picture: picture.map(Path::to_path_buf), size })?;
        self.expect(timeout, |reply| match reply {
            Reply::Ready { adapter, ms, uploads } => Some(Ok(Ready { adapter: adapter.clone(), ms: *ms, uploads: *uploads })),
            other => Self::failure(other, None).map(Err),
        })
    }

    pub fn ask_open(&mut self, id: u32, screen: &Screen, still: &Path) -> Result<()> {
        self.send(&Command::Open { id, screen: screen.id.clone(), rect: screen.frame, scale: screen.scale, still: Some(still.to_path_buf()) })
    }

    pub fn shown(&mut self, id: u32, timeout: Duration) -> Result<u64> {
        self.expect(timeout, |reply| match reply {
            Reply::Shown { id: shown, ms } if *shown == id => Some(Ok(*ms)),
            other => Self::failure(other, Some(id)).map(Err),
        })
    }

    pub fn open(&mut self, id: u32, screen: &Screen, still: &Path, timeout: Duration) -> Result<u64> {
        self.ask_open(id, screen, still)?;
        self.shown(id, timeout)
    }

    pub fn ask_glide(&mut self, id: u32, a: &Path, b: &Path, ms: u32) -> Result<()> {
        self.send(&Command::Glide { id, a: a.to_path_buf(), b: b.to_path_buf(), ms })
    }

    pub fn glided(&mut self, id: u32, timeout: Duration) -> Result<Glided> {
        self.expect(timeout, |reply| match reply {
            Reply::Glided { id: glided, frames, slow_frames, first_frame_ms, uploads } if *glided == id => Some(Ok(Glided { frames: *frames, slow_frames: *slow_frames, first_frame_ms: *first_frame_ms, uploads: *uploads })),
            other => Self::failure(other, Some(id)).map(Err),
        })
    }

    pub fn ask_settle(&mut self, id: u32, still: &Path, ms: Option<u32>) -> Result<()> {
        self.send(&Command::Settle { id, still: still.to_path_buf(), ms })
    }

    pub fn settled(&mut self, id: u32, timeout: Duration) -> Result<()> {
        self.expect(timeout, |reply| match reply {
            Reply::Settled { id: settled } if *settled == id => Some(Ok(())),
            other => Self::failure(other, Some(id)).map(Err),
        })
    }

    pub fn close(&mut self, id: u32, timeout: Duration) -> Result<()> {
        self.send(&Command::Close { id })?;
        self.expect(timeout, |reply| match reply {
            Reply::Closed { id: closed } if *closed == id => Some(Ok(())),
            other => Self::failure(other, Some(id)).map(Err),
        })
    }

    pub fn hang_up(&mut self) {
        self.input = None;
    }

    pub fn exited(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }

    pub fn wait_exit(&mut self, limit: Duration) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if self.exited() {
                return Some(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.exited().then(|| started.elapsed())
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        if self.input.is_some() {
            let _ = self.send(&Command::Quit);
        }
        self.input = None;
        if self.wait_exit(QUIT_WAIT).is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        #[cfg(windows)]
        drop(self.guard.take());
    }
}

#[cfg(windows)]
mod job {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};

    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub struct Guard(HANDLE);

    unsafe impl Send for Guard {}

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    pub fn guard(child: &std::process::Child) -> windows::core::Result<Guard> {
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }?;
        let guard = Guard(job);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe { SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const c_void, size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32) }?;
        unsafe { AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())) }?;
        Ok(guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_replies_read_back_the_same() {
        let commands = [
            Command::Hello { version: VERSION },
            Command::Warm { hints: Hints { present: Some("bare".into()) }, picture: Some(PathBuf::from("p.jpg")), size: Some([1024, 768]) },
            Command::Open { id: 3, screen: "one".into(), rect: Rect { x: 0.0, y: 0.0, w: 1024.0, h: 768.0 }, scale: 1.5, still: Some(PathBuf::from("a.png")) },
            Command::Glide { id: 3, a: PathBuf::from("a.tables"), b: PathBuf::from("b.tables"), ms: 450 },
            Command::Settle { id: 3, still: PathBuf::from("b.png"), ms: None },
            Command::Close { id: 3 },
            Command::Quit,
        ];
        for command in commands {
            assert_eq!(serde_json::from_str::<Command>(line(&command).trim()).unwrap(), command);
        }
        let replies = [
            Reply::Hello { version: VERSION, pid: 9 },
            Reply::Ready { adapter: "gpu".into(), ms: 12, uploads: 1 },
            Reply::Shown { id: 3, ms: 40 },
            Reply::Glided { id: 3, frames: 24, slow_frames: 1, first_frame_ms: 30, uploads: 1 },
            Reply::Settled { id: 3 },
            Reply::Closed { id: 3 },
            Reply::Failed { id: None, why: "no".into() },
        ];
        for reply in replies {
            assert_eq!(serde_json::from_str::<Reply>(line(&reply).trim()).unwrap(), reply);
        }
    }

    #[test]
    fn a_bare_open_line_takes_the_defaults() {
        let read: Command = serde_json::from_str(r#"{"kind":"open","id":1,"screen":"s","rect":{"x":0,"y":0,"w":10,"h":10}}"#).unwrap();
        assert_eq!(read, Command::Open { id: 1, screen: "s".into(), rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, scale: 1.0, still: None });
        let warm: Command = serde_json::from_str(r#"{"kind":"warm"}"#).unwrap();
        assert_eq!(warm, Command::Warm { hints: Hints::default(), picture: None, size: None });
        let settle: Command = serde_json::from_str(r#"{"kind":"settle","id":2,"still":"b.png"}"#).unwrap();
        assert_eq!(settle, Command::Settle { id: 2, still: PathBuf::from("b.png"), ms: None });
    }
}
