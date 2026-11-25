pub mod canvas;
pub mod still;
pub mod helper;

use crate::platform::Screen;
use anyhow::{anyhow, bail, Context, Result};
use scooch_core::Rect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub const HELPER: &str = "--live-helper";
pub const VERSION: u32 = 6;
pub const SLOW_HELLO: &str = "SCOOCH_HELPER_SLOW_HELLO_MS";
pub const READ_LAG: &str = "SCOOCH_READ_LAG_MS";
pub const SPIN: &str = "SCOOCH_HELPER_SPIN";
const QUIT_WAIT: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reveal {
    #[default]
    Transaction,
    Spin,
}

impl Reveal {
    pub fn from(value: Option<&str>) -> Reveal {
        if value == Some("1") {
            Reveal::Spin
        } else {
            Reveal::Transaction
        }
    }

    pub fn chosen() -> Reveal {
        Reveal::from(std::env::var(SPIN).ok().as_deref())
    }
}

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<u64>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        scales: BTreeMap<String, f32>,
    },
    Park {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warm_id: Option<u32>,
    },
    Window { screen: String, rect: Rect, scale: f32, #[serde(default)] stills: Stills },
    Open { id: u32, screen: String, rect: Rect, #[serde(default = "one")] scale: f32, #[serde(default)] stills: Stills },
    Glide { id: u32, a: PathBuf, b: PathBuf, ms: u32 },
    Settle { id: u32, still: PathBuf, #[serde(default)] ms: Option<u32> },
    Close { id: u32 },
    Unprime { screen: String },
    Quit,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stills {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub still: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tables: Option<PathBuf>,
}

impl Stills {
    pub fn of(still: &Path) -> Stills {
        Stills { still: Some(still.to_path_buf()), raw: None, tables: None }
    }
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
    Drawn {
        id: u32,
        window: u64,
        first: u64,
        reveal: u64,
        make: u64,
        present: u64,
        found: bool,
        steps: Vec<(String, u64)>,
        #[serde(default)]
        revealing: Vec<(String, u64)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        still: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        screen_gpu: Option<String>,
    },
    Primed { screen: String, window: u64, first: u64, still: String },
    Device { ms: u64 },
    Canvas { #[serde(default)] warm_id: Option<u32>, ms: u64 },
    Glided { id: u32, frames: u32, slow_frames: u32, first_frame_ms: u64, #[serde(default)] uploads: u32, #[serde(default = "one")] scale: f32 },
    Settled { id: u32 },
    Drew { id: u32, frames: String },
    Closed { id: u32 },
    Unprimed { screen: String },
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

pub type Watch = Box<dyn Fn(Option<&Reply>) + Send>;

pub const BACKLOG: usize = 32;

struct Pipe {
    lines: mpsc::Sender<String>,
    backlog: Arc<std::sync::atomic::AtomicUsize>,
}

impl Pipe {
    fn new(mut input: ChildStdin) -> Result<Pipe> {
        let (lines, waiting) = mpsc::channel::<String>();
        let backlog = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let written = backlog.clone();
        std::thread::Builder::new().name("scooch-live-commands".into()).spawn(move || {
            for text in waiting {
                if input.write_all(text.as_bytes()).and_then(|()| input.flush()).is_err() {
                    break;
                }
                written.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        })?;
        Ok(Pipe { lines, backlog })
    }

    fn send(&self, text: String) -> Result<()> {
        if self.backlog.load(std::sync::atomic::Ordering::SeqCst) >= BACKLOG {
            bail!("the helper has not read its last {BACKLOG} commands, so it is stopped");
        }
        self.backlog.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.lines.send(text).map_err(|_| anyhow!("the pipe to the helper is closed"))
    }
}

pub struct Helper {
    child: Child,
    input: Option<Pipe>,
    replies: mpsc::Receiver<(Instant, Reply)>,
    #[cfg(windows)]
    guard: Option<job::Guard>,
    pub pid: u32,
    warms: u32,
    launched: Instant,
    arrived: Option<Instant>,
    sent: Instant,
}

impl Helper {
    pub fn spawn(program: &Path, timeout: Duration) -> Result<Helper> {
        Self::spawn_with(program, timeout, &[])
    }

    pub fn spawn_with(program: &Path, timeout: Duration, env: &[(&str, &str)]) -> Result<Helper> {
        let mut helper = Self::launch_with(program, env)?;
        helper.expect(timeout, |reply| match reply {
            Reply::Hello { version, .. } if *version == VERSION => Some(Ok(())),
            Reply::Hello { version, .. } => Some(Err(anyhow!("the helper speaks version {version}, not {VERSION}"))),
            Reply::Failed { id: None, why } => Some(Err(anyhow!("{why}"))),
            _ => None,
        })?;
        Ok(helper)
    }

    pub fn launch(program: &Path) -> Result<Helper> {
        Self::launch_with(program, &[])
    }

    pub fn launch_with(program: &Path, env: &[(&str, &str)]) -> Result<Helper> {
        Self::launch_full(program, env, None)
    }

    pub fn launch_watched(program: &Path, watch: Watch) -> Result<Helper> {
        Self::launch_full(program, &[], Some(watch))
    }

    fn launch_full(program: &Path, env: &[(&str, &str)], watch: Option<Watch>) -> Result<Helper> {
        let launched = Instant::now();
        let mut command = std::process::Command::new(program);
        #[cfg(target_os = "linux")]
        crate::platform::linux::children::clean(&mut command);
        command.arg(HELPER).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).envs(env.iter().copied());
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
                let arrived = Instant::now();
                if let Some(watch) = &watch {
                    watch(Some(&reply));
                }
                if sender.send((arrived, reply)).is_err() {
                    break;
                }
            }
            if let Some(watch) = &watch {
                watch(None);
            }
        })?;
        let pid = child.id();
        let input = child.stdin.take().map(Pipe::new).transpose()?;
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
            sent: launched,
        };
        helper.send(&Command::Hello { version: VERSION })?;
        Ok(helper)
    }

    pub fn launched(&self) -> Instant {
        self.launched
    }

    pub fn arrived(&self) -> Option<Instant> {
        self.arrived
    }

    pub fn since_launch(&self) -> Option<Duration> {
        self.arrived.map(|arrived| arrived.saturating_duration_since(self.launched))
    }

    pub fn send(&mut self, command: &Command) -> Result<()> {
        let text = line(command)?;
        self.input.as_ref().context("the pipe to the helper is closed")?.send(text)?;
        self.sent = Instant::now();
        Ok(())
    }

    pub fn since_sent(&self) -> Duration {
        self.sent.elapsed()
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
        self.ask_warm_keyed(hints, (picture, None), size, canvas, scales)
    }

    pub fn ask_warm_keyed(&mut self, hints: Hints, (picture, key): (Option<&Path>, Option<u64>), size: Option<[u32; 2]>, canvas: Option<&Path>, scales: BTreeMap<String, f32>) -> Result<u32> {
        self.warms = self.warms.wrapping_add(1).max(1);
        let warm_id = self.warms;
        self.send(&Command::Warm { hints, picture: picture.map(Path::to_path_buf), size, warm_id: Some(warm_id), canvas: canvas.map(Path::to_path_buf), key, scales })?;
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

    pub fn ask_unprime(&mut self, screen: &str) -> Result<()> {
        self.send(&Command::Unprime { screen: screen.to_string() })
    }

    pub fn ask_window(&mut self, screen: &Screen, stills: &Stills) -> Result<()> {
        self.send(&Command::Window { screen: screen.id.clone(), rect: screen.frame, scale: screen.scale, stills: stills.clone() })
    }

    pub fn ask_park(&mut self) -> Result<u32> {
        self.warms = self.warms.wrapping_add(1).max(1);
        let warm_id = self.warms;
        self.send(&Command::Park { warm_id: Some(warm_id) })?;
        Ok(warm_id)
    }

    pub fn ask_open(&mut self, id: u32, screen: &Screen, stills: &Stills) -> Result<()> {
        self.send(&Command::Open { id, screen: screen.id.clone(), rect: screen.frame, scale: screen.scale, stills: stills.clone() })
    }

    pub fn shown(&mut self, id: u32, timeout: Duration) -> Result<u64> {
        self.expect(timeout, |reply| match reply {
            Reply::Shown { id: shown, ms } if *shown == id => Some(Ok(*ms)),
            other => Self::failure(other, Some(id)).map(Err),
        })
    }

    pub fn open(&mut self, id: u32, screen: &Screen, still: &Path, timeout: Duration) -> Result<u64> {
        self.ask_open(id, screen, &Stills::of(still))?;
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

    pub fn broke(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some_and(|status| !status.success())
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

    #[cfg(unix)]
    #[test]
    fn commands_to_a_helper_that_stops_reading_fail_instead_of_blocking() {
        use std::os::unix::fs::PermissionsExt;
        let folder = crate::state::scratch("helper-stopped");
        let program = folder.join("never-reads");
        std::fs::write(&program, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut helper = Helper::launch(&program).unwrap();
        let long = PathBuf::from(format!("/{}", "p".repeat(4000)));
        let started = Instant::now();
        let mut refused = None;
        for index in 0..200 {
            if let Err(error) = helper.ask_warm_with(Hints::default(), Some(&long), Some([1, 1]), None, BTreeMap::new()) {
                refused = Some((index, format!("{error:#}")));
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(started.elapsed() < Duration::from_secs(10), "a send to a helper that stopped reading blocked");
        let (index, why) = refused.expect("every send to a helper that stopped reading was taken");
        assert!(why.contains("has not read its last"), "{why}");
        assert!(index > BACKLOG, "{index}");
        drop(helper);
    }

    #[test]
    fn commands_and_replies_read_back_the_same() {
        let commands = [
            Command::Hello { version: VERSION },
            Command::Warm { hints: Hints { present: Some("bare".into()), scale: Some(0.5) }, picture: Some(PathBuf::from("p.jpg")), size: Some([1024, 768]), warm_id: Some(4), canvas: Some(PathBuf::from("main.canvas")), key: Some(7), scales: BTreeMap::from([("gpu".to_string(), 0.75)]) },
            Command::Park { warm_id: Some(2) },
            Command::Window { screen: "one".into(), rect: Rect { x: 0.0, y: 0.0, w: 1024.0, h: 768.0 }, scale: 1.5, stills: Stills { still: Some(PathBuf::from("a.jpg")), raw: Some(PathBuf::from("a.jpg.still")), tables: Some(PathBuf::from("a.tables")) } },
            Command::Open { id: 3, screen: "one".into(), rect: Rect { x: 0.0, y: 0.0, w: 1024.0, h: 768.0 }, scale: 1.5, stills: Stills::of(Path::new("a.png")) },
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
            Reply::Drawn { id: 3, window: 12, first: 20, reveal: 34, make: 80, present: 10, found: true, steps: vec![("screen".into(), 2), ("window".into(), 78)], revealing: vec![("spin".into(), 34), ("unhide".into(), 2)], still: Some("raw".into()), screen_gpu: Some("gpu".into()) },
            Reply::Primed { screen: "one".into(), window: 20, first: 30, still: "kept".into() },
            Reply::Glided { id: 3, frames: 24, slow_frames: 1, first_frame_ms: 30, uploads: 1, scale: 0.75 },
            Reply::Settled { id: 3 },
            Reply::Drew { id: 3, frames: "1795551562.312 move S@1 G0.01@0.20".into() },
            Reply::Closed { id: 3 },
            Reply::Failed { id: None, why: "no".into() },
        ];
        for reply in replies {
            assert_eq!(serde_json::from_str::<Reply>(line(&reply).unwrap().trim()).unwrap(), reply);
        }
    }

    const SPOKEN: [(u32, &str); 2] = [
        (5, "commands hello warm park window open glide settle close unprime quit, replies hello ready shown drawn primed device canvas glided settled closed unprimed failed"),
        (6, "commands hello warm park window open glide settle close unprime quit, replies hello ready shown drawn primed device canvas glided settled drew closed unprimed failed"),
    ];

    fn kind<T: Serialize>(message: &T) -> String {
        serde_json::to_value(message).ok().and_then(|value| value["kind"].as_str().map(str::to_string)).unwrap_or_default()
    }

    fn spoken() -> String {
        let rect = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        let commands = [
            Command::Hello { version: VERSION },
            Command::Warm { hints: Hints::default(), picture: None, size: None, warm_id: None, canvas: None, key: None, scales: BTreeMap::new() },
            Command::Park { warm_id: None },
            Command::Window { screen: "s".into(), rect, scale: 1.0, stills: Stills::default() },
            Command::Open { id: 1, screen: "s".into(), rect, scale: 1.0, stills: Stills::default() },
            Command::Glide { id: 1, a: PathBuf::from("a"), b: PathBuf::from("b"), ms: 1 },
            Command::Settle { id: 1, still: PathBuf::from("s"), ms: None },
            Command::Close { id: 1 },
            Command::Unprime { screen: "s".into() },
            Command::Quit,
        ];
        let replies = [
            Reply::Hello { version: VERSION, pid: 1 },
            Reply::Ready { adapter: "gpu".into(), ms: 1, uploads: 0, kind: "integrated".into(), warm_id: None, from: None, scale: None },
            Reply::Shown { id: 1, ms: 1 },
            Reply::Drawn { id: 1, window: 1, first: 1, reveal: 1, make: 1, present: 1, found: true, steps: Vec::new(), revealing: Vec::new(), still: None, screen_gpu: None },
            Reply::Primed { screen: "s".into(), window: 1, first: 1, still: "raw".into() },
            Reply::Device { ms: 1 },
            Reply::Canvas { warm_id: None, ms: 1 },
            Reply::Glided { id: 1, frames: 1, slow_frames: 0, first_frame_ms: 1, uploads: 0, scale: 1.0 },
            Reply::Settled { id: 1 },
            Reply::Drew { id: 1, frames: String::new() },
            Reply::Closed { id: 1 },
            Reply::Unprimed { screen: "s".into() },
            Reply::Failed { id: None, why: String::new() },
        ];
        for command in &commands {
            match command {
                Command::Hello { .. } | Command::Warm { .. } | Command::Park { .. } | Command::Window { .. } | Command::Open { .. } | Command::Glide { .. } | Command::Settle { .. } | Command::Close { .. } | Command::Unprime { .. } | Command::Quit => {}
            }
        }
        for reply in &replies {
            match reply {
                Reply::Hello { .. } | Reply::Ready { .. } | Reply::Shown { .. } | Reply::Drawn { .. } | Reply::Primed { .. } | Reply::Device { .. } | Reply::Canvas { .. } | Reply::Glided { .. } | Reply::Settled { .. } | Reply::Drew { .. } | Reply::Closed { .. } | Reply::Unprimed { .. } | Reply::Failed { .. } => {}
            }
        }
        format!("commands {}, replies {}", commands.iter().map(kind).collect::<Vec<_>>().join(" "), replies.iter().map(kind).collect::<Vec<_>>().join(" "))
    }

    #[test]
    fn every_change_to_what_the_helper_speaks_raises_the_version() {
        let now = spoken();
        let recorded = SPOKEN.iter().find(|(version, _)| *version == VERSION).map(|(_, kinds)| *kinds);
        assert_eq!(recorded, Some(now.as_str()), "version {VERSION} is recorded as speaking another set of commands and replies, so a daemon and a helper of different builds would not part at the hello");
        for (version, kinds) in SPOKEN.iter().filter(|(version, _)| *version != VERSION) {
            assert!(*version < VERSION, "version {version} is recorded but {VERSION} is the newest");
            assert_ne!(*kinds, now.as_str(), "version {version} speaks the same as {VERSION}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_of_the_version_before_parts_at_the_hello() {
        use std::os::unix::fs::PermissionsExt;
        let folder = crate::state::scratch("helper-older");
        let program = folder.join("older");
        std::fs::write(&program, format!("#!/bin/sh\necho '{{\"kind\":\"hello\",\"version\":{},\"pid\":4}}'\nexec sleep 5\n", VERSION - 1)).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        let refused = Helper::spawn(&program, Duration::from_secs(5)).err().map(|error| format!("{error:#}"));
        assert_eq!(refused, Some(format!("the helper speaks version {}, not {VERSION}", VERSION - 1)));
        assert!(started.elapsed() < Duration::from_secs(4), "the hello was not answered at once");
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
        let warm = line(&Command::Warm { hints: Hints::default(), picture: None, size: None, warm_id: Some(3), canvas: None, key: None, scales: BTreeMap::new() }).unwrap();
        assert!(warm.contains(r#""warm_id":3"#), "{warm}");
        assert_eq!(warm_failed(Some(3), "no device"), "warm 3: no device");
    }

    #[test]
    fn a_bare_open_line_takes_the_defaults() {
        let read: Command = serde_json::from_str(r#"{"kind":"open","id":1,"screen":"s","rect":{"x":0,"y":0,"w":10,"h":10}}"#).unwrap();
        assert_eq!(read, Command::Open { id: 1, screen: "s".into(), rect: Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }, scale: 1.0, stills: Stills::default() });
        let warm: Command = serde_json::from_str(r#"{"kind":"warm"}"#).unwrap();
        assert_eq!(warm, Command::Warm { hints: Hints::default(), picture: None, size: None, warm_id: None, canvas: None, key: None, scales: BTreeMap::new() });
        let settle: Command = serde_json::from_str(r#"{"kind":"settle","id":2,"still":"b.png"}"#).unwrap();
        assert_eq!(settle, Command::Settle { id: 2, still: PathBuf::from("b.png"), ms: None });
    }
}
