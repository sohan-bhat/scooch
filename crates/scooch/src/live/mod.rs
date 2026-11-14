pub mod canvas;
pub mod helper;

use crate::platform::Screen;
use anyhow::{anyhow, bail, Context, Result};
use scooch_core::Rect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const HELPER: &str = "--live-helper";
pub const VERSION: u32 = 2;
pub const SLOW_HELLO: &str = "SCOOCH_HELPER_SLOW_HELLO_MS";
const QUIT_WAIT: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    Hello { version: u32 },
    Warm {
        #[serde(default)]
        hints: Hints,
        #[serde(default)]
        picture: Option<PathBuf>,
        #[serde(default)]
        size: Option<[u32; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warm_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        canvas: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        scales: BTreeMap<String, f32>,
    },
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
    Ready {
        adapter: String,
        ms: u64,
        #[serde(default)]
        uploads: u32,
        #[serde(default, rename = "device")]
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warm_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<f32>,
    },
    Shown { id: u32, ms: u64 },
    Glided { id: u32, frames: u32, slow_frames: u32, first_frame_ms: u64, #[serde(default)] uploads: u32, #[serde(default = "one")] scale: f32 },
    Settled { id: u32 },
    Closed { id: u32 },
    Failed { #[serde(default)] id: Option<u32>, why: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub adapter: String,
    pub ms: u64,
    pub uploads: u32,
    pub kind: String,
    pub warm_id: Option<u32>,
    pub from: Option<String>,
    pub scale: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glided {
    pub frames: u32,
    pub slow_frames: u32,
    pub first_frame_ms: u64,
    pub uploads: u32,
    pub scale: f32,
}

pub fn line<T: Serialize>(message: &T) -> Result<String> {
    Ok(serde_json::to_string(message).context("a message to the helper could not be written as JSON")? + "\n")
}

pub fn warm_failed(warm_id: Option<u32>, why: &str) -> String {
    match warm_id {
        Some(warm_id) => format!("warm {warm_id}: {why}"),
        None => why.to_string(),
    }
}

pub struct Helper {
    child: Child,
    input: Option<ChildStdin>,
    replies: mpsc::Receiver<(Instant, Reply)>,
    #[cfg(windows)]
    guard: Option<job::Guard>,
    pub pid: u32,
    warms: u32,
    launched: Instant,
    arrived: Option<Instant>,
}

impl Helper {
    pub fn spawn(program: &Path, timeout: Duration) -> Result<Helper> {
        let mut helper = Self::launch(program)?;
        helper.expect(timeout, |reply| match reply {
            Reply::Hello { version, .. } if *version == VERSION => Some(Ok(())),
            Reply::Hello { version, .. } => Some(Err(anyhow!("the helper speaks version {version}, not {VERSION}"))),
            Reply::Failed { id: None, why } => Some(Err(anyhow!("{why}"))),
            _ => None,
        })?;
        Ok(helper)
    }

    pub fn launch(program: &Path) -> Result<Helper> {
        let launched = Instant::now();
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
                if sender.send((Instant::now(), reply)).is_err() {
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
            warms: 0,
            launched,
            arrived: None,
        };
        helper.send(&Command::Hello { version: VERSION })?;
        Ok(helper)
    }

    pub fn since_launch(&self) -> Option<Duration> {
        self.arrived.map(|arrived| arrived.saturating_duration_since(self.launched))
    }

    pub fn send(&mut self, command: &Command) -> Result<()> {
        let text = line(command)?;
        let input = self.input.as_mut().context("the pipe to the helper is closed")?;
        input.write_all(text.as_bytes())?;
        input.flush()?;
        Ok(())
    }

    pub fn expect<T>(&mut self, timeout: Duration, mut pick: impl FnMut(&Reply) -> Option<Result<T>>) -> Result<T> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.replies.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok((arrived, reply)) => {
                    self.arrived = Some(arrived);
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
        let (arrived, reply) = self.replies.recv_timeout(timeout).ok()?;
        self.arrived = Some(arrived);
        Some(reply)
    }

    fn failure(reply: &Reply, wanted: Option<u32>) -> Option<anyhow::Error> {
        match reply {
            Reply::Failed { id, why } if id.is_none() || *id == wanted => Some(anyhow!("{why}")),
            _ => None,
        }
    }

    pub fn ask_warm(&mut self, hints: Hints, picture: Option<&Path>, size: Option<[u32; 2]>) -> Result<u32> {
        self.ask_warm_with(hints, picture, size, None, BTreeMap::new())
    }

    pub fn ask_warm_with(&mut self, hints: Hints, picture: Option<&Path>, size: Option<[u32; 2]>, canvas: Option<&Path>, scales: BTreeMap<String, f32>) -> Result<u32> {
        self.warms = self.warms.wrapping_add(1).max(1);
        let warm_id = self.warms;
        self.send(&Command::Warm { hints, picture: picture.map(Path::to_path_buf), size, warm_id: Some(warm_id), canvas: canvas.map(Path::to_path_buf), scales })?;
        Ok(warm_id)
    }

    pub fn ready_for(&mut self, warm_id: u32, timeout: Duration) -> Result<Ready> {
        let mine = format!("warm {warm_id}: ");
        self.expect(timeout, |reply| match reply {
            Reply::Ready { adapter, ms, uploads, kind, warm_id: Some(answered), from, scale } if *answered == warm_id => Some(Ok(Ready { adapter: adapter.clone(), ms: *ms, uploads: *uploads, kind: kind.clone(), warm_id: Some(warm_id), from: from.clone(), scale: *scale })),
            Reply::Failed { id: None, why } if why.starts_with(&mine) || !why.starts_with("warm ") => Some(Err(anyhow!("{why}"))),
            _ => None,
        })
    }

    pub fn warm(&mut self, hints: Hints, picture: Option<&Path>, size: Option<[u32; 2]>, timeout: Duration) -> Result<Ready> {
        let warm_id = self.ask_warm(hints, picture, size)?;
        self.ready_for(warm_id, timeout)
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
            Reply::Glided { id: glided, frames, slow_frames, first_frame_ms, uploads, scale } if *glided == id => Some(Ok(Glided { frames: *frames, slow_frames: *slow_frames, first_frame_ms: *first_frame_ms, uploads: *uploads, scale: *scale })),
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
            Command::Warm { hints: Hints { present: Some("bare".into()), scale: Some(0.5) }, picture: Some(PathBuf::from("p.jpg")), size: Some([1024, 768]), warm_id: Some(4), canvas: Some(PathBuf::from("main.canvas")), scales: BTreeMap::from([("gpu".to_string(), 0.75)]) },
            Command::Open { id: 3, screen: "one".into(), rect: Rect { x: 0.0, y: 0.0, w: 1024.0, h: 768.0 }, scale: 1.5, still: Some(PathBuf::from("a.png")) },
            Command::Glide { id: 3, a: PathBuf::from("a.tables"), b: PathBuf::from("b.tables"), ms: 450 },
            Command::Settle { id: 3, still: PathBuf::from("b.png"), ms: None },
            Command::Close { id: 3 },
            Command::Quit,
        ];
        for command in commands {
            assert_eq!(serde_json::from_str::<Command>(line(&command).unwrap().trim()).unwrap(), command);
        }
        let replies = [
            Reply::Hello { version: VERSION, pid: 9 },
            Reply::Ready { adapter: "gpu".into(), ms: 12, uploads: 1, kind: "integrated".into(), warm_id: Some(4), from: Some("cache".into()), scale: Some(0.6) },
            Reply::Shown { id: 3, ms: 40 },
            Reply::Glided { id: 3, frames: 24, slow_frames: 1, first_frame_ms: 30, uploads: 1, scale: 0.75 },
            Reply::Settled { id: 3 },
            Reply::Closed { id: 3 },
            Reply::Failed { id: None, why: "no".into() },
        ];
        for reply in replies {
            assert_eq!(serde_json::from_str::<Reply>(line(&reply).unwrap().trim()).unwrap(), reply);
        }
    }

    #[test]
    fn the_ready_answer_names_the_adapter_kind_as_device() {
        let text = line(&Reply::Ready { adapter: "warp".into(), ms: 1, uploads: 0, kind: "cpu".into(), warm_id: Some(7), from: None, scale: None }).unwrap();
        assert!(text.contains(r#""kind":"ready""#) && text.contains(r#""device":"cpu""#), "{text}");
    }

    #[test]
    fn a_path_that_cannot_be_json_is_an_error_at_once() {
        #[cfg(unix)]
        let bad: PathBuf = {
            use std::os::unix::ffi::OsStrExt;
            PathBuf::from(std::ffi::OsStr::from_bytes(b"still-\xff.png"))
        };
        #[cfg(windows)]
        let bad: PathBuf = {
            use std::os::windows::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_wide(&[0x73, 0xD800, 0x2E, 0x70]))
        };
        let command = Command::Settle { id: 1, still: bad, ms: None };
        assert!(line(&command).is_err());
        assert!(line(&Command::Quit).is_ok_and(|text| text.ends_with('\n')));
    }

    #[test]
    fn the_ready_answer_carries_the_warm_it_answers() {
        let text = line(&Reply::Ready { adapter: "gpu".into(), ms: 1, uploads: 1, kind: "integrated".into(), warm_id: Some(9), from: None, scale: None }).unwrap();
        assert!(text.contains(r#""warm_id":9"#), "{text}");
        let warm = line(&Command::Warm { hints: Hints::default(), picture: None, size: None, warm_id: Some(3), canvas: None, scales: BTreeMap::new() }).unwrap();
        assert!(warm.contains(r#""warm_id":3"#), "{warm}");
        assert_eq!(warm_failed(Some(3), "no device"), "warm 3: no device");
    }

    #[test]
    fn a_bare_open_line_takes_the_defaults() {
        let read: Command = serde_json::from_str(r#"{"kind":"open","id":1,"screen":"s","rect":{"x":0,"y":0,"w":10,"h":10}}"#).unwrap();
        assert_eq!(read, Command::Open { id: 1, screen: "s".into(), rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, scale: 1.0, still: None });
        let warm: Command = serde_json::from_str(r#"{"kind":"warm"}"#).unwrap();
        assert_eq!(warm, Command::Warm { hints: Hints::default(), picture: None, size: None, warm_id: None, canvas: None, scales: BTreeMap::new() });
        let settle: Command = serde_json::from_str(r#"{"kind":"settle","id":2,"still":"b.png"}"#).unwrap();
        assert_eq!(settle, Command::Settle { id: 2, still: PathBuf::from("b.png"), ms: None });
    }
}
