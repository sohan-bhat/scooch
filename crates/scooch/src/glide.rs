use crate::live::{self, Helper, Hints, Reply};
use crate::platform::Screen;
use crate::state::Start;
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const FORCE: &str = "SCOOCH_GLIDE_ANYWAY";
const SOFTWARE: [&str; 3] = ["basic render driver", "llvmpipe", "swiftshader"];
const STRIKES: u32 = 2;
const LATE: u32 = 3;
pub const GLIDE_MS: u32 = 450;
pub const SETTLE_WAIT: Duration = Duration::from_secs(2);
pub const CHECK: Duration = Duration::from_millis(50);
pub const PATIENCE: Duration = Duration::from_millis(150);
pub const HELLO_GRACE: Duration = Duration::from_millis(100);
pub const SHOWN_AFTER_WAKE: Duration = Duration::from_millis(450);
pub const BARE: u32 = 2;
pub const EARLY_EVERY: Duration = Duration::from_secs(10);
pub const EARLY_LIFE: Duration = Duration::from_millis(1500);
pub const EARLY_CAP: Duration = Duration::from_secs(30);
pub const PRESS_POLL: Duration = Duration::from_millis(50);
const SAMPLES: usize = 3;
const NOTHING_NEW: &str = "the new picture is the one on screen or could not be made, so nothing glides";

#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Glided { id: u32, frames: u32, slow: u32, scale: f32, first: u64 },
    Settled(u32),
    Failed(Option<u32>, String),
    Nothing,
}

pub trait Link: Send {
    fn device(&mut self) -> Result<()>;
    fn warm(&mut self, picture: &Path, size: [u32; 2], canvas: Option<&Path>, scales: &BTreeMap<String, f32>) -> Result<()>;
    fn ready(&mut self, timeout: Duration) -> Result<Adapter>;
    fn answered(&mut self, id: u32) -> bool;
    fn window(&mut self, screen: &Screen) -> Result<()>;
    fn open(&mut self, id: u32, screen: &Screen, still: &Path) -> Result<()>;
    fn shown(&mut self, id: u32, timeout: Duration) -> Result<Instant>;
    fn drawn(&mut self, id: u32) -> Option<Split>;
    fn glide(&mut self, id: u32, a: &Path, b: &Path, ms: u32) -> Result<()>;
    fn settle(&mut self, id: u32, still: &Path) -> Result<()>;
    fn hear(&mut self, timeout: Duration) -> Result<Heard>;
    fn close(&mut self, id: u32);
    fn quit(&mut self);
    fn alive(&mut self) -> bool;
    fn broke(&mut self) -> bool;
    fn greeted(&mut self) -> Option<Duration>;
    fn device_ready(&mut self) -> Option<Duration>;
    fn canvas_ready(&mut self) -> Option<Duration>;
    fn ready_after(&mut self) -> Option<Adapter>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Split {
    pub window: u32,
    pub first: u32,
    pub reveal: u32,
    pub make: u32,
    pub present: u32,
    pub found: bool,
    pub steps: Vec<(String, u32)>,
    pub revealing: Vec<(String, u32)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Adapter {
    pub name: String,
    pub kind: String,
    pub from: Option<String>,
    pub scale: Option<f32>,
    pub after: Option<Duration>,
}

pub trait Spawn: Send + Sync {
    fn spawn(&self) -> Result<Box<dyn Link>>;
}

pub struct Program(pub Option<PathBuf>);

impl Spawn for Program {
    fn spawn(&self) -> Result<Box<dyn Link>> {
        let program = self.0.as_deref().ok_or_else(|| anyhow!("Scooch could not find its own program"))?;
        Ok(Box::new(Real::new(Helper::launch(program)?)))
    }
}

pub fn program() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    if cfg!(windows) {
        return Some(me.with_file_name("scoochw.exe"));
    }
    Some(me)
}

struct Real {
    helper: Helper,
    shown: Vec<(u32, Instant)>,
    gone: Vec<u32>,
    failed: Vec<(Option<u32>, String)>,
    heard: Vec<Heard>,
    readies: Vec<(Option<u32>, Adapter)>,
    warm_id: Option<u32>,
    answered: Option<Adapter>,
    hello: Option<Duration>,
    drawn: Vec<(u32, Split)>,
    device_after: Option<Duration>,
    canvas_after: Option<Duration>,
}

impl Real {
    fn new(helper: Helper) -> Self {
        Self { helper, shown: Vec::new(), gone: Vec::new(), failed: Vec::new(), heard: Vec::new(), readies: Vec::new(), warm_id: None, answered: None, hello: None, drawn: Vec::new(), device_after: None, canvas_after: None }
    }

    fn pump(&mut self, timeout: Duration) -> Result<bool> {
        let started = Instant::now();
        let Some(reply) = self.helper.next(timeout) else {
            if self.helper.exited() || started.elapsed() < timeout / 2 {
                return Err(anyhow!("the helper is gone"));
            }
            return Ok(false);
        };
        match reply {
            Reply::Shown { id, .. } if self.gone.contains(&id) => {}
            Reply::Shown { id, .. } => self.shown.push((id, self.helper.arrived().unwrap_or_else(Instant::now))),
            Reply::Closed { .. } => {}
            Reply::Device { .. } => self.device_after = self.device_after.or(self.helper.since_launch()),
            Reply::Canvas { warm_id, .. } if warm_id.is_some() && warm_id == self.warm_id => self.canvas_after = self.canvas_after.or(self.helper.since_launch()),
            Reply::Canvas { .. } => {}
            Reply::Drawn { id, window, first, reveal, make, present, found, steps, revealing } => {
                if self.drawn.len() >= 8 {
                    self.drawn.remove(0);
                }
                let part = |ms: u64| u32::try_from(ms).unwrap_or(u32::MAX);
                let steps = steps.into_iter().map(|(name, took)| (name, part(took))).collect();
                let revealing = revealing.into_iter().map(|(name, took)| (name, part(took))).collect();
                self.drawn.push((id, Split { window: part(window), first: part(first), reveal: part(reveal), make: part(make), present: part(present), found, steps, revealing }));
            }
            Reply::Settled { id } => self.heard.push(Heard::Settled(id)),
            Reply::Glided { id, frames, slow_frames, scale, first_frame_ms, .. } => self.heard.push(Heard::Glided { id, frames, slow: slow_frames, scale, first: first_frame_ms }),
            Reply::Failed { id: Some(id), .. } if self.gone.contains(&id) => {}
            Reply::Failed { id, why } => self.failed.push((id, why)),
            Reply::Ready { adapter, kind, warm_id, from, scale, .. } => {
                let after = self.helper.since_launch();
                let adapter = Adapter { name: adapter, kind, from, scale, after };
                if warm_id.is_some() && warm_id == self.warm_id && self.answered.is_none() {
                    self.answered = Some(adapter.clone());
                }
                self.readies.push((warm_id, adapter));
            }
            Reply::Hello { version, .. } if version != live::VERSION => self.failed.push((None, format!("the helper speaks version {version}, not {}", live::VERSION))),
            Reply::Hello { .. } => self.hello = self.helper.since_launch(),
        }
        Ok(true)
    }

    fn failure(&mut self, id: Option<u32>) -> Option<String> {
        let index = self.failed.iter().position(|(failed, _)| failed.is_none() || *failed == id)?;
        Some(self.failed.remove(index).1)
    }

    fn wait<T>(&mut self, timeout: Duration, mut done: impl FnMut(&mut Self) -> Option<Result<T>>) -> Result<T> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(result) = done(self) {
                return result;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(anyhow!("the helper did not answer within {} ms", timeout.as_millis()));
            }
            self.pump(left)?;
        }
    }
}

impl Link for Real {
    fn device(&mut self) -> Result<()> {
        self.helper.ask_warm_with(Hints::default(), None, None, None, BTreeMap::new())?;
        Ok(())
    }

    fn device_ready(&mut self) -> Option<Duration> {
        while self.device_after.is_none() && matches!(self.pump(Duration::ZERO), Ok(true)) {}
        self.device_after
    }

    fn canvas_ready(&mut self) -> Option<Duration> {
        while self.canvas_after.is_none() && matches!(self.pump(Duration::ZERO), Ok(true)) {}
        self.canvas_after
    }

    fn warm(&mut self, picture: &Path, size: [u32; 2], canvas: Option<&Path>, scales: &BTreeMap<String, f32>) -> Result<()> {
        self.warm_id = Some(self.helper.ask_warm_with(Hints::default(), Some(picture), Some(size), canvas, scales.clone())?);
        self.answered = None;
        self.canvas_after = None;
        Ok(())
    }

    fn ready(&mut self, timeout: Duration) -> Result<Adapter> {
        let wanted = self.warm_id;
        while matches!(self.pump(Duration::ZERO), Ok(true)) {}
        self.wait(timeout, |me| {
            if let Some(index) = me.readies.iter().position(|(answered, _)| *answered == wanted) {
                me.readies.drain(..index);
                return Some(Ok(me.readies.remove(0).1));
            }
            let prefix = wanted.map(|id| format!("warm {id}: "));
            let index = me.failed.iter().position(|(id, why)| id.is_none() && prefix.as_ref().is_none_or(|prefix| why.starts_with(prefix.as_str()) || !why.starts_with("warm ")))?;
            Some(Err(anyhow!("{}", me.failed.remove(index).1)))
        })
    }

    fn answered(&mut self, id: u32) -> bool {
        while matches!(self.pump(Duration::ZERO), Ok(true)) {}
        self.shown.iter().any(|(shown, _)| *shown == id) || self.failed.iter().any(|(failed, _)| failed.is_none() || *failed == Some(id))
    }

    fn window(&mut self, screen: &Screen) -> Result<()> {
        self.helper.ask_window(screen)
    }

    fn open(&mut self, id: u32, screen: &Screen, still: &Path) -> Result<()> {
        self.helper.ask_open(id, screen, still)
    }

    fn shown(&mut self, id: u32, timeout: Duration) -> Result<Instant> {
        while matches!(self.pump(Duration::ZERO), Ok(true)) {}
        self.wait(timeout, |me| {
            if let Some(index) = me.shown.iter().position(|(shown, _)| *shown == id) {
                return Some(Ok(me.shown.remove(index).1));
            }
            me.failure(Some(id)).map(|why| Err(anyhow!("{why}")))
        })
    }

    fn drawn(&mut self, id: u32) -> Option<Split> {
        let found = |me: &Self| me.drawn.iter().rev().find(|(drawn, _)| *drawn == id).map(|(_, split)| split.clone());
        while found(self).is_none() && matches!(self.pump(Duration::ZERO), Ok(true)) {}
        found(self)
    }

    fn glide(&mut self, id: u32, a: &Path, b: &Path, ms: u32) -> Result<()> {
        self.helper.ask_glide(id, a, b, ms)
    }

    fn settle(&mut self, id: u32, still: &Path) -> Result<()> {
        self.helper.ask_settle(id, still, None)
    }

    fn hear(&mut self, timeout: Duration) -> Result<Heard> {
        if !self.heard.is_empty() {
            return Ok(self.heard.remove(0));
        }
        if let Some((id, why)) = (!self.failed.is_empty()).then(|| self.failed.remove(0)) {
            return Ok(Heard::Failed(id, why));
        }
        self.pump(timeout)?;
        if !self.heard.is_empty() {
            return Ok(self.heard.remove(0));
        }
        Ok((!self.failed.is_empty()).then(|| self.failed.remove(0)).map_or(Heard::Nothing, |(id, why)| Heard::Failed(id, why)))
    }

    fn close(&mut self, id: u32) {
        self.shown.retain(|(shown, _)| *shown != id);
        self.failed.retain(|(failed, _)| *failed != Some(id));
        if !self.gone.contains(&id) {
            if self.gone.len() >= 64 {
                self.gone.remove(0);
            }
            self.gone.push(id);
        }
        let _ = self.helper.send(&live::Command::Close { id });
    }

    fn quit(&mut self) {
        let _ = self.helper.send(&live::Command::Quit);
    }

    fn alive(&mut self) -> bool {
        !self.helper.exited()
    }

    fn broke(&mut self) -> bool {
        self.helper.broke()
    }

    fn greeted(&mut self) -> Option<Duration> {
        while self.hello.is_none() && matches!(self.pump(Duration::ZERO), Ok(true)) {}
        self.hello
    }

    fn ready_after(&mut self) -> Option<Adapter> {
        while self.answered.is_none() && matches!(self.pump(Duration::ZERO), Ok(true)) {}
        self.answered.clone()
    }
}

pub struct Warmup {
    pub picture: PathBuf,
    pub size: [u32; 2],
    pub canvas: Option<PathBuf>,
}

pub struct Launch<'a> {
    pub screen: Option<&'a Screen>,
    pub warm: Option<&'a Warmup>,
    pub drag: bool,
}

pub struct Plan<'a> {
    pub screen: &'a Screen,
    pub picture: &'a Path,
    pub canvas: Option<&'a Path>,
    pub size: [u32; 2],
    pub still: &'a Path,
}

struct Early {
    id: u32,
    screen: String,
    size: [u32; 2],
    still: PathBuf,
    at: Instant,
}

struct Flight {
    id: u32,
    screen: String,
    size: [u32; 2],
    settled: bool,
    heard: bool,
    set: Option<Instant>,
}

struct Ready {
    id: u32,
    screen: String,
    size: [u32; 2],
    started: Instant,
    reused: bool,
}

pub fn software(adapter: &Adapter) -> bool {
    if !adapter.kind.is_empty() {
        return adapter.kind == "cpu";
    }
    let name = adapter.name.to_lowercase();
    SOFTWARE.iter().any(|known| name.contains(known))
}

pub fn under(screens: &[Screen], (x, y): (f32, f32)) -> Option<&Screen> {
    screens.iter().find(|screen| x >= screen.frame.x && x < screen.frame.x + screen.frame.w && y >= screen.frame.y && y < screen.frame.y + screen.frame.h)
}

pub fn hold_until(started: Instant, woke: Option<Instant>, predicted: Option<Instant>) -> Option<Instant> {
    let deadline = woke.map_or(started + PATIENCE, |woke| (started + PATIENCE).max(woke + SHOWN_AFTER_WAKE));
    predicted.filter(|predicted| *predicted <= deadline).map(|_| deadline)
}

pub fn skipped(reason: &str, detail: &str) {
    static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if crate::daemon::trace() || *last != reason {
        if detail.is_empty() {
            eprintln!("{} glide skipped, {reason}", crate::daemon::stamp());
        } else {
            eprintln!("{} glide skipped, {reason}, {detail}", crate::daemon::stamp());
        }
        reason.clone_into(&mut last);
    }
}

pub fn exact(still: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::new();
    if std::fs::File::open(still).and_then(|file| file.take(1 << 16).read_to_end(&mut head)).is_err() {
        return true;
    }
    sampling(&head).is_none_or(|factors| factors.iter().all(|factor| *factor == factors[0]))
}

fn sampling(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.get(..2) != Some(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2;
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xFF {
            return None;
        }
        let marker = bytes[at + 1];
        if marker == 0xFF {
            at += 1;
            continue;
        }
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            at += 2;
            continue;
        }
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let count = usize::from(*bytes.get(at + 9)?);
            return (0..count).map(|index| bytes.get(at + 11 + index * 3).copied()).collect();
        }
        at += 2 + usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
    }
    None
}

fn reason_of(error: &anyhow::Error) -> &'static str {
    let text = format!("{error:#}");
    if text.contains("software renderer") {
        "software adapter"
    } else if text.contains("never reported the live layer as visible") || text.contains("cover the whole desktop") {
        "hidden"
    } else if text.contains("no wallpaper window below") {
        "no room"
    } else if ["was not ready within", "had not started", "would be shown", "no cold start is remembered"].iter().any(|cold| text.contains(cold)) {
        "helper cold"
    } else if text.contains(NOTHING_NEW) {
        "nothing new"
    } else if text.contains("did not answer within") {
        "helper budget"
    } else if text.contains("did not settle") {
        "settle budget"
    } else if text.contains("too large") {
        "too large"
    } else {
        "helper failed"
    }
}

pub struct Glider {
    pub forced: bool,
    software: bool,
    strikes: u32,
    stalled: bool,
    spawner: Box<dyn Spawn>,
    link: Option<Box<dyn Link>>,
    warmed: Option<(PathBuf, [u32; 2])>,
    last_warm: Option<(PathBuf, [u32; 2], Option<PathBuf>)>,
    ready: bool,
    next: u32,
    late: u32,
    flights: Vec<Flight>,
    early: Vec<Early>,
    doomed: bool,
    huge: std::collections::HashMap<String, [u32; 2]>,
    adapter: Option<String>,
    scales: BTreeMap<String, f32>,
    learned: Vec<(String, f32)>,
    prepared: Option<Ready>,
    noted: bool,
    revived: bool,
    starts: BTreeMap<String, Start>,
    fresh_starts: Vec<(String, Start)>,
    launched: Option<Instant>,
    ready_at: Option<Instant>,
    early_launch: bool,
    early_generation: u64,
    early_until: Option<Instant>,
    early_window: Option<String>,
    missed: Option<u32>,
    moved_now: bool,
    thrown: u32,
    last_early: Option<Instant>,
    woke: Option<Instant>,
    pub said: Vec<String>,
}

#[derive(Default)]
struct Trace {
    hello: Option<Duration>,
    device: Option<Duration>,
    canvas: Option<Duration>,
    ready: Option<Duration>,
    shown: Option<Duration>,
    predicted: Option<Duration>,
    opened: bool,
    drawn: Option<Split>,
}

fn drawn_words(split: Option<&Split>) -> String {
    let known = |value: Option<u32>| value.map_or("unknown".to_string(), |value| value.to_string());
    let listed = |steps: &[(String, u32)]| if steps.is_empty() { "unknown".to_string() } else { steps.iter().map(|(name, took)| format!("{name} {took}")).collect::<Vec<_>>().join(", ") };
    let steps = split.map_or("unknown".to_string(), |split| listed(&split.steps));
    let revealing = split.map_or("unknown".to_string(), |split| listed(&split.revealing));
    format!(
        "the helper made its window in {} ms, drew in {} ms and revealed in {} ms, made it in {} ms and presented it in {} ms, open found it made {}, window steps {steps}, reveal steps {revealing}",
        known(split.map(|split| split.window)),
        known(split.map(|split| split.first)),
        known(split.map(|split| split.reveal)),
        known(split.map(|split| split.make)),
        known(split.map(|split| split.present)),
        split.map_or("unknown".to_string(), |split| split.found.to_string())
    )
}

fn ms(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

fn keep_last(list: &mut Vec<u32>, value: u32) {
    list.push(value);
    let extra = list.len().saturating_sub(SAMPLES);
    list.drain(..extra);
}

fn median(list: &[u32]) -> Option<Duration> {
    let mut sorted = list.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    let value = match sorted.len() {
        0 => return None,
        count if count % 2 == 0 => (sorted[middle - 1] + sorted[middle]) / 2,
        _ => sorted[middle],
    };
    Some(Duration::from_millis(value.into()))
}

fn shown_ms(value: Option<Duration>) -> String {
    value.map_or("unknown".to_string(), |value| value.as_millis().to_string())
}

pub fn scale_key(adapter: &str, size: [u32; 2]) -> String {
    format!("{adapter}|{}x{}", size[0], size[1])
}

const SAID: usize = 32;

fn note(said: &mut Vec<String>, line: String) {
    if crate::daemon::trace() {
        eprintln!("{} glide {line}", crate::daemon::stamp());
    }
    keep(said, line);
}

fn keep(said: &mut Vec<String>, line: String) {
    if said.len() >= SAID {
        said.remove(0);
    }
    said.push(line);
}

impl Glider {
    pub fn new(spawner: Box<dyn Spawn>) -> Self {
        let forced = std::env::var_os(FORCE).is_some_and(|value| !value.is_empty() && value != "0");
        Self { forced, software: false, strikes: 0, stalled: false, spawner, link: None, warmed: None, last_warm: None, ready: false, next: 0, late: 0, flights: Vec::new(), early: Vec::new(), doomed: false, huge: std::collections::HashMap::new(), adapter: None, scales: BTreeMap::new(), learned: Vec::new(), prepared: None, noted: false, revived: false, starts: BTreeMap::new(), fresh_starts: Vec::new(), launched: None, ready_at: None, early_launch: false, early_generation: 0, early_until: None, early_window: None, missed: None, moved_now: false, thrown: 0, last_early: None, woke: None, said: Vec::new() }
    }

    #[cfg(test)]
    pub fn reset_for_test(&mut self) {
        self.drop_link();
    }

    pub fn usable(&self) -> bool {
        self.forced || !(self.software || self.stalled)
    }

    fn judge(&mut self, frames: u32, slow: u32) {
        if self.forced {
            return;
        }
        if slow * 2 > frames {
            self.strikes += 1;
            if self.strikes >= STRIKES && !self.stalled {
                self.stalled = true;
                let line = format!("Scooch turned the glide off until it starts again because {STRIKES} glides in a row stuttered.");
                eprintln!("{} glide {line}", crate::daemon::stamp());
                keep(&mut self.said, line);
            }
        } else {
            self.strikes = 0;
        }
    }

    pub fn running(&self) -> bool {
        self.link.is_some()
    }

    pub fn reap(&mut self) {
        if self.flights.is_empty() && self.link.as_mut().is_some_and(|link| !link.alive()) {
            self.revive();
        }
        if !self.noted {
            if let Some(adapter) = self.link.as_mut().and_then(|link| link.ready_after()) {
                note(&mut self.said, format!("helper ready {} ms after launch", shown_ms(adapter.after)));
                self.noted = true;
                if let Some(after) = adapter.after {
                    self.learn(&adapter.name, |start| start.ready = ms(after));
                }
            }
        }
    }

    fn drop_link(&mut self) {
        self.prepared = None;
        self.noted = false;
        self.link = None;
        self.warmed = None;
        self.ready = false;
        self.launched = None;
        self.ready_at = None;
        self.early_launch = false;
        self.early_until = None;
        self.early_window = None;
    }

    pub fn woke(&mut self, at: Instant) {
        self.woke = Some(at);
    }

    fn scales_for(&self, size: [u32; 2]) -> BTreeMap<String, f32> {
        let suffix = format!("|{}x{}", size[0], size[1]);
        self.scales.iter().filter_map(|(key, scale)| key.strip_suffix(&suffix).map(|adapter| (adapter.to_string(), *scale))).collect()
    }

    pub fn wake(&mut self, launch: &Launch) -> Option<u64> {
        let held = !launch.drag && (self.thrown >= BARE || self.last_early.is_some_and(|at| at.elapsed() < EARLY_EVERY));
        if !self.usable() || self.doomed || held {
            return None;
        }
        if self.link.is_some() {
            if self.early_launch {
                self.top_up(launch);
            }
            return None;
        }
        let started = Instant::now();
        self.last_early = Some(started);
        let scales = launch.warm.map(|warm| self.scales_for(warm.size)).unwrap_or_default();
        let launched = self.spawner.spawn().and_then(|mut link| {
            match launch.warm {
                Some(warm) => link.warm(&warm.picture, warm.size, warm.canvas.as_deref(), &scales)?,
                None => link.device()?,
            }
            if let Some(screen) = launch.screen {
                link.window(screen)?;
            }
            Ok(link)
        });
        match launched {
            Ok(link) => {
                self.drop_link();
                self.link = Some(link);
                self.launched = Some(started);
                self.early_launch = true;
                self.early_until = Some(started + EARLY_LIFE);
                self.early_generation += 1;
                if let Some(warm) = launch.warm {
                    self.warmed = Some((warm.picture.clone(), warm.size));
                    self.last_warm = Some((warm.picture.clone(), warm.size, warm.canvas.clone()));
                }
                self.early_window = launch.screen.map(|screen| screen.id.clone());
                let asked = match (launch.warm.is_some(), launch.screen) {
                    (true, Some(screen)) => format!("warm and a window on {}", screen.id),
                    (true, None) => "warm only".to_string(),
                    (false, Some(screen)) => format!("device and a window on {}", screen.id),
                    (false, None) => "device only".to_string(),
                };
                let when = if launch.drag { "the drag start" } else { "the wake" };
                note(&mut self.said, format!("early launch at {when} in {} ms, {asked}", started.elapsed().as_millis()));
                Some(self.early_generation)
            }
            Err(error) => {
                note(&mut self.said, format!("early launch failed, {error:#}"));
                None
            }
        }
    }

    fn top_up(&mut self, launch: &Launch) {
        let wanted = launch.warm.map(|warm| (warm.picture.clone(), warm.size)).filter(|wanted| self.warmed.as_ref() != Some(wanted));
        let screen = launch.screen.filter(|screen| self.early_window.as_deref() != Some(screen.id.as_str()));
        let scales = launch.warm.map(|warm| self.scales_for(warm.size)).unwrap_or_default();
        let Some(link) = self.link.as_mut() else { return };
        let mut sent = Vec::new();
        if let (Some(wanted), Some(warm)) = (wanted, launch.warm) {
            if link.warm(&warm.picture, warm.size, warm.canvas.as_deref(), &scales).is_ok() {
                self.warmed = Some(wanted);
                self.last_warm = Some((warm.picture.clone(), warm.size, warm.canvas.clone()));
                sent.push("a warm".to_string());
            }
        }
        if let Some(screen) = screen {
            if link.window(screen).is_ok() {
                self.early_window = Some(screen.id.clone());
                sent.push(format!("a window on {}", screen.id));
            }
        }
        if !sent.is_empty() {
            let when = if launch.drag { "the drag start" } else { "the wake" };
            note(&mut self.said, format!("early helper given {} at {when}", sent.join(" and ")));
        }
    }

    pub fn extend_early(&mut self, generation: u64, until: Instant) -> bool {
        if !self.early_launch || self.early_generation != generation {
            return false;
        }
        self.early_until = Some(until);
        true
    }

    pub fn after_snapshot(&mut self, moved: bool) {
        self.moved_now = moved;
        if moved {
            self.thrown = 0;
        } else if self.early_launch && self.early_until.is_some_and(|until| Instant::now() >= until) {
            self.thrown += 1;
            self.quit_early("nothing moved");
        }
    }

    pub fn expire_early(&mut self, generation: u64) {
        if self.early_launch && self.early_generation == generation {
            self.thrown += 1;
            self.quit_early("nothing moved within 1.5 s of its launch or of the button going up");
        }
    }

    pub fn retire_unused(&mut self) {
        if self.early_launch && self.moved_now && self.warmed.is_none() && self.flights.is_empty() {
            self.quit_early("no screen could glide");
        }
    }

    fn quit_early(&mut self, why: &str) {
        let lived = self.launched.map(|at| at.elapsed());
        if let Some(mut link) = self.link.take() {
            link.quit();
            let _ = std::thread::Builder::new().name("scooch-quit".into()).spawn(move || drop(link));
        }
        self.drop_link();
        note(&mut self.said, format!("early helper quit, {why}, it lived {} ms", shown_ms(lived)));
    }

    #[cfg(test)]
    pub fn forget_early_for_test(&mut self) {
        self.last_early = None;
    }

    fn learn(&mut self, adapter: &str, change: impl FnOnce(&mut Start)) {
        let start = self.starts.entry(adapter.to_string()).or_default();
        change(start);
        start.at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let saved = start.clone();
        self.fresh_starts.retain(|(known, _)| known != adapter);
        self.fresh_starts.push((adapter.to_string(), saved));
    }

    fn remembered(&self) -> Option<&Start> {
        match &self.adapter {
            Some(adapter) => self.starts.get(adapter),
            None => self.starts.values().max_by_key(|start| start.at),
        }
    }

    pub fn learned_starts(&mut self) -> Vec<(String, Start)> {
        std::mem::take(&mut self.fresh_starts)
    }

    fn connect(&mut self) -> Result<&mut Box<dyn Link>> {
        if self.link.as_mut().is_some_and(|link| !link.alive()) {
            self.drop_link();
        }
        if self.link.is_none() {
            let started = Instant::now();
            let link = self.spawner.spawn()?;
            note(&mut self.said, format!("helper launched in {} ms", started.elapsed().as_millis()));
            self.link = Some(link);
            self.warmed = None;
            self.ready = false;
            self.launched = Some(started);
        }
        self.link.as_mut().ok_or_else(|| anyhow!("no helper"))
    }

    pub fn know(&mut self, scales: &BTreeMap<String, f32>, starts: &BTreeMap<String, Start>) {
        self.scales.clone_from(scales);
        self.starts.clone_from(starts);
        for (adapter, start) in &self.fresh_starts {
            self.starts.insert(adapter.clone(), start.clone());
        }
    }

    pub fn learned(&mut self) -> Vec<(String, f32)> {
        std::mem::take(&mut self.learned)
    }

    fn send_warm(&mut self, picture: &Path, size: [u32; 2], canvas: Option<&Path>) -> Result<()> {
        let wanted = (picture.to_path_buf(), size);
        self.connect()?;
        self.early_launch = false;
        self.early_until = None;
        if self.warmed.as_ref() == Some(&wanted) {
            return Ok(());
        }
        let scales = self.scales_for(size);
        self.link.as_mut().ok_or_else(|| anyhow!("no helper"))?.warm(picture, size, canvas, &scales)?;
        note(&mut self.said, format!("warm {} at {}x{} from {}", picture.display(), size[0], size[1], canvas.map_or("the picture".to_string(), |canvas| canvas.display().to_string())));
        self.last_warm = Some((picture.to_path_buf(), size, canvas.map(Path::to_path_buf)));
        self.warmed = Some(wanted);
        self.ready = false;
        self.early_launch = false;
        Ok(())
    }

    fn left(deadline: Instant) -> Duration {
        deadline.saturating_duration_since(Instant::now())
    }

    fn fresh_id(&mut self) -> u32 {
        self.next = self.next.wrapping_add(1).max(1);
        self.next
    }

    fn too_large(&self, screen: &str, size: [u32; 2]) -> bool {
        self.huge.get(screen) == Some(&size)
    }

    fn remember_size(&mut self, screen: &str, size: [u32; 2], error: &anyhow::Error) {
        if reason_of(error) == "too large" && self.huge.insert(screen.to_string(), size) != Some(size) {
            let line = format!("Scooch keeps the plain swap on screen {screen} at {}x{} because it is larger than the GPU can draw, until the screen changes or Scooch starts again.", size[0], size[1]);
            eprintln!("{} glide {line}", crate::daemon::stamp());
            keep(&mut self.said, line);
        }
    }

    pub fn open_early(&mut self, screen: &Screen, picture: &Path, size: [u32; 2], still: &Path, canvas: Option<&Path>) {
        if !self.usable() || self.doomed || self.too_large(&screen.id, size) || self.early.iter().any(|early| early.screen == screen.id) || !exact(still) {
            return;
        }
        let id = self.fresh_id();
        let result = (|| -> Result<()> {
            self.send_warm(picture, size, canvas)?;
            self.link.as_mut().ok_or_else(|| anyhow!("no helper"))?.open(id, screen, still)
        })();
        match result {
            Ok(()) => {
                note(&mut self.said, format!("open after snapshot {id} on {}", still.display()));
                self.early.push(Early { id, screen: screen.id.clone(), size, still: still.to_path_buf(), at: Instant::now() });
            }
            Err(error) => self.give_up(None, &error),
        }
    }

    pub fn close_early(&mut self) {
        let screens: Vec<String> = self.early.iter().map(|early| early.screen.clone()).collect();
        for screen in screens {
            self.close_early_for(&screen);
        }
    }

    pub fn close_early_for(&mut self, screen: &str) {
        let Some(index) = self.early.iter().position(|early| early.screen == screen) else { return };
        let unused = self.early.remove(index);
        let Some(link) = self.link.as_mut() else { return };
        let shown = link.shown(unused.id, Duration::ZERO).map(|_| ());
        link.close(unused.id);
        note(&mut self.said, format!("closed open after snapshot {} with nothing to glide", unused.id));
        if let Err(error) = shown {
            self.remember_size(&unused.screen, unused.size, &error);
        }
    }

    #[cfg(test)]
    pub fn prepare(&mut self, plan: &Plan) -> Option<u32> {
        self.prepare_while(plan, None, &mut || true)
    }

    pub fn cannot_glide(&mut self, plan: &Plan) -> bool {
        let refused = if !self.usable() {
            Some((if self.software { "software adapter" } else { "strikes" }, ""))
        } else if self.too_large(&plan.screen.id, plan.size) || self.doomed {
            Some((if self.doomed { "helper failed" } else { "too large" }, ""))
        } else if !exact(plan.still) {
            Some(("old still", "the picture on screen was saved with less color detail than the layer shows, so this move swaps it plainly"))
        } else {
            None
        };
        if let Some((reason, detail)) = refused {
            self.close_early_for(&plan.screen.id);
            skipped(reason, detail);
            return true;
        }
        let started = Instant::now();
        if self.send_warm(plan.picture, plan.size, plan.canvas).is_err() || self.ready {
            return false;
        }
        let launched = self.launched.map(|launched| started.saturating_duration_since(launched));
        if launched.is_none_or(|launched| launched < HELLO_GRACE) || self.link.as_mut().is_none_or(|link| link.greeted().is_some()) {
            return false;
        }
        note(&mut self.said, format!("decision at the set, helper launched {} ms before it, after launch hello unknown, device unknown, canvas unknown, ready unknown, shown unknown, predicted shown unknown, waited 0 ms, 0 ms of it on the new picture, plain before the tables, {}", shown_ms(launched), drawn_words(None)));
        self.close_early_for(&plan.screen.id);
        self.give_up(None, &anyhow!("the helper had not started when the plain swap was ready, so this move swaps plainly"));
        true
    }

    pub fn prepare_while(&mut self, plan: &Plan, ahead: Option<Duration>, idle: &mut dyn FnMut() -> bool) -> Option<u32> {
        if !self.usable() {
            self.close_early_for(&plan.screen.id);
            skipped(if self.software { "software adapter" } else { "strikes" }, "");
            return None;
        }
        if self.too_large(&plan.screen.id, plan.size) || self.doomed {
            self.close_early_for(&plan.screen.id);
            skipped(if self.doomed { "helper failed" } else { "too large" }, "");
            return None;
        }
        if !exact(plan.still) {
            self.close_early_for(&plan.screen.id);
            skipped("old still", "the picture on screen was saved with less color detail than the layer shows, so this move swaps it plainly");
            return None;
        }
        let started = Instant::now();
        let early = self.early.iter().position(|early| early.screen == plan.screen.id).map(|index| self.early.remove(index));
        let reused = early.as_ref().filter(|early| early.still == plan.still).map(|early| early.id);
        if let (Some(stale), None) = (early.as_ref(), reused) {
            if let Some(link) = self.link.as_mut() {
                link.close(stale.id);
            }
        }
        let id = reused.unwrap_or_else(|| self.fresh_id());
        let opened_at = early.as_ref().filter(|_| reused.is_some()).map(|early| early.at);
        let fresh = self.link.is_none() || !self.ready;
        let mut trace = Trace::default();
        let mut busy = Duration::ZERO;
        let mut timed = || {
            let began = Instant::now();
            let going = idle();
            busy += began.elapsed();
            going
        };
        let result = self.decide(plan, id, (started, opened_at), &mut trace, (ahead, &mut timed)).map_err(|error| if fresh && format!("{error:#}").contains("did not answer within") { anyhow!("the helper was not ready within {} ms of the plain swap, so this move swaps plainly", started.elapsed().as_millis()) } else { error });
        let opened = reused.is_some() || trace.opened;
        let launched = self.launched.map(|launched| started.saturating_duration_since(launched));
        note(
            &mut self.said,
            format!(
                "decision at the set, helper launched {} ms before it, after launch hello {}, device {}, canvas {}, ready {}, shown {}, predicted shown {}, waited {} ms, {} ms of it on the new picture, {}, {}",
                shown_ms(launched),
                shown_ms(trace.hello),
                shown_ms(trace.device),
                shown_ms(trace.canvas),
                shown_ms(trace.ready),
                shown_ms(trace.shown),
                shown_ms(trace.predicted),
                started.elapsed().as_millis(),
                busy.as_millis(),
                if result.is_ok() { "glide" } else { "plain" },
                drawn_words(trace.drawn.as_ref())
            ),
        );
        match result {
            Ok(()) => {
                self.prepared = Some(Ready { id, screen: plan.screen.id.clone(), size: plan.size, started, reused: reused.is_some() });
                Some(id)
            }
            Err(error) => {
                self.remember_size(&plan.screen.id, plan.size, &error);
                self.give_up(opened.then_some(id), &error);
                None
            }
        }
    }

    fn decide(&mut self, plan: &Plan, id: u32, (started, opened_at): (Instant, Option<Instant>), trace: &mut Trace, (ahead, idle): (Option<Duration>, &mut dyn FnMut() -> bool)) -> Result<()> {
        self.send_warm(plan.picture, plan.size, plan.canvas)?;
        let launched = self.launched;
        let since = |at: Instant| launched.map(|launched| at.saturating_duration_since(launched));
        let remembered = self.remembered().cloned().unwrap_or_default();
        let link = self.link.as_mut().ok_or_else(|| anyhow!("no helper"))?;
        let cold = !self.ready;
        if cold {
            trace.hello = link.greeted();
            if trace.hello.is_none() {
                return Err(anyhow!("the helper had not started when the plain swap was ready, so this move swaps plainly"));
            }
        }
        if cold {
            match link.ready(Duration::ZERO) {
                Ok(adapter) => self.heard_ready(adapter)?,
                Err(error) if format!("{error:#}").contains("did not answer within") => {}
                Err(error) => return Err(error),
            }
        }
        trace.device = self.link.as_mut().and_then(|link| link.device_ready());
        trace.canvas = self.link.as_mut().and_then(|link| link.canvas_ready());
        let waits_for_ready = !self.ready;
        let woke = self.woke;
        let open = opened_at.unwrap_or(started);
        let missed = remembered.missed.or(self.missed).map(|sample| Duration::from_millis(sample.into()));
        let mut guessed = false;
        let mut showing = None;
        let deadline = if waits_for_ready {
            let device = launched.zip(trace.device).map(|(launched, device)| launched + device);
            let canvas = launched.zip(trace.canvas).map(|(launched, canvas)| launched + canvas);
            let show = device.and_then(|device| Some(device.max(open + median(&remembered.windows)?) + median(&remembered.draws)? + median(&remembered.reveals)?));
            let ready = canvas.or_else(|| device.zip(median(&remembered.canvases)).map(|(device, canvas)| device.max(open) + canvas));
            let predicted = match (show, median(&remembered.colds).or(missed)) {
                (Some(show), _) => Some(ready.map_or(show, |ready| show.max(ready))),
                (None, Some(cold)) => Some(open + cold),
                (None, None) => None,
            };
            trace.predicted = predicted.and_then(since);
            showing = predicted;
            let greeted_by_open = launched.zip(trace.hello).is_some_and(|(launched, hello)| launched + hello <= open);
            match (hold_until(started, woke, predicted), predicted) {
                (Some(deadline), _) => deadline,
                (None, None) if greeted_by_open => {
                    guessed = true;
                    hold_until(started, woke, Some(started)).unwrap_or(started + PATIENCE)
                }
                (None, None) => return Err(anyhow!("no cold start is remembered for this adapter yet and the helper said hello after the open, so this move swaps plainly")),
                (None, Some(predicted)) => {
                    let allowed = hold_until(started, woke, Some(started)).map_or(PATIENCE, |deadline| deadline.saturating_duration_since(started));
                    return Err(anyhow!("the helper would be shown {} ms after the plain swap was ready, past the {} ms it may wait, so this move swaps plainly", predicted.saturating_duration_since(started).as_millis(), allowed.as_millis()));
                }
            }
        } else {
            hold_until(started, woke, Some(started)).unwrap_or(started + PATIENCE)
        };
        let worth = |showing: Option<Instant>| ahead.zip(showing).is_some_and(|(ahead, showing)| showing > Instant::now() + ahead);
        if waits_for_ready {
            if worth(showing) && !idle() {
                return Err(anyhow!(NOTHING_NEW));
            }
            let waited = self.link.as_mut().ok_or_else(|| anyhow!("no helper"))?.ready(Self::left(deadline));
            let adapter = match waited {
                Ok(adapter) => adapter,
                Err(error) => return Err(self.missed_wait(guessed, open, deadline, error)),
            };
            self.heard_ready(adapter)?;
        }
        trace.ready = self.ready_at.and_then(since);
        let open_at = Instant::now();
        let link = self.link.as_mut().ok_or_else(|| anyhow!("no helper"))?;
        if opened_at.is_none() {
            link.open(id, plan.screen, plan.still)?;
            trace.opened = true;
        }
        let showing = showing.or_else(|| Some(opened_at.unwrap_or(open_at) + median(&remembered.windows)? + median(&remembered.draws)? + median(&remembered.reveals)?));
        if !link.answered(id) && worth(showing) && !idle() {
            return Err(anyhow!(NOTHING_NEW));
        }
        let shown_at = match link.shown(id, Self::left(deadline)) {
            Ok(shown_at) => shown_at,
            Err(error) => return Err(self.missed_wait(guessed, open, deadline, error)),
        };
        trace.drawn = link.drawn(id);
        trace.shown = since(shown_at);
        let device_at = launched.zip(link.device_ready()).map(|(launched, device)| launched + device);
        let canvas_at = launched.zip(link.canvas_ready()).map(|(launched, canvas)| launched + canvas);
        let opened = opened_at.unwrap_or(open_at);
        let cold = device_at.is_some_and(|device| device > opened);
        let canvas = device_at.zip(canvas_at).map(|(device, canvas)| canvas.saturating_duration_since(device.max(opened)));
        let drawn = trace.drawn.clone();
        if cold {
            self.missed = None;
        }
        if let Some(adapter) = self.adapter.clone() {
            self.learn(&adapter, |start| {
                if cold {
                    keep_last(&mut start.colds, ms(shown_at.saturating_duration_since(opened)));
                    start.missed = None;
                }
                if let Some(canvas) = canvas {
                    keep_last(&mut start.canvases, ms(canvas));
                }
                if let Some(split) = drawn {
                    keep_last(&mut start.windows, split.window);
                    keep_last(&mut start.draws, split.first);
                    keep_last(&mut start.reveals, split.reveal);
                }
            });
        }
        Ok(())
    }

    fn missed_wait(&mut self, guessed: bool, open: Instant, deadline: Instant, error: anyhow::Error) -> anyhow::Error {
        if guessed && format!("{error:#}").contains("did not answer within") {
            let sample = ms(deadline.saturating_duration_since(open) + SHOWN_AFTER_WAKE);
            self.missed = Some(sample);
            if let Some(adapter) = self.adapter.clone() {
                self.learn(&adapter, |start| start.missed = Some(sample));
            }
            note(&mut self.said, format!("no cold start was remembered and the helper missed the wait, so {sample} ms is kept and the next cold drag swaps plainly at once"));
        }
        error
    }

    fn heard_ready(&mut self, adapter: Adapter) -> Result<()> {
        if !self.forced && software(&adapter) {
            self.software = true;
            return Err(anyhow!("the only GPU here is the software renderer {} of kind {}", adapter.name, adapter.kind));
        }
        note(&mut self.said, format!("ready on {} of kind {}, canvas from {}, first scale {}, {} ms after launch", adapter.name, adapter.kind, adapter.from.as_deref().unwrap_or("unknown"), adapter.scale.map_or("unknown".to_string(), |scale| scale.to_string()), shown_ms(adapter.after)));
        self.ready_at = Some(self.launched.zip(adapter.after).map_or_else(Instant::now, |(launched, after)| launched + after));
        if !self.noted {
            self.noted = true;
            if let Some(after) = adapter.after {
                self.learn(&adapter.name, |start| start.ready = ms(after));
            }
        }
        self.adapter = Some(adapter.name);
        self.ready = true;
        Ok(())
    }

    pub fn go(&mut self, id: u32, from: &Path, to: &Path) -> bool {
        let Some(prepared) = self.prepared.take().filter(|prepared| prepared.id == id) else { return false };
        let result = match self.link.as_mut() {
            Some(link) => link.glide(id, from, to, GLIDE_MS),
            None => Err(anyhow!("the helper is gone")),
        };
        match result {
            Ok(()) => {
                self.late = 0;
                self.revived = false;
                note(&mut self.said, format!("open {id} shown and gliding after {} ms{}", prepared.started.elapsed().as_millis(), if prepared.reused { ", opened after the snapshot" } else { "" }));
                self.flights.push(Flight { id, screen: prepared.screen, size: prepared.size, settled: false, heard: false, set: None });
                true
            }
            Err(error) => {
                self.give_up(Some(id), &error);
                false
            }
        }
    }

    pub fn settle(&mut self, id: u32, still: &Path, set: Instant) {
        if let Some(flight) = self.flights.iter_mut().find(|flight| flight.id == id) {
            flight.set = Some(set);
        }
        let result = match self.link.as_mut() {
            Some(link) => link.settle(id, still),
            None => Err(anyhow!("the helper is gone")),
        };
        match result {
            Ok(()) => note(&mut self.said, format!("settle {id} on {}", still.display())),
            Err(error) => self.give_up(Some(id), &error),
        }
    }

    pub fn skip(&mut self, reason: &str, detail: &str) {
        skipped(reason, detail);
        keep(&mut self.said, format!("skipped, {reason}, {detail}"));
    }

    pub fn settle_back(&mut self, id: u32, still: &Path, why: &str) {
        let result = match self.link.as_mut() {
            Some(link) => link.settle(id, still),
            None => Err(anyhow!("the helper is gone")),
        };
        match result {
            Ok(()) => {
                let line = format!("settle {id} back on {} because {why}", still.display());
                eprintln!("{} glide {line}", crate::daemon::stamp());
                keep(&mut self.said, line);
            }
            Err(error) => self.give_up(Some(id), &error),
        }
    }

    pub fn abort(&mut self, id: u32) {
        if self.prepared.as_ref().is_some_and(|prepared| prepared.id == id) {
            self.prepared = None;
        }
        if let Some(link) = self.link.as_mut() {
            link.close(id);
        }
        self.flights.retain(|flight| flight.id != id);
        note(&mut self.said, format!("closed {id} early"));
    }

    pub fn in_flight(&self) -> bool {
        !self.flights.is_empty()
    }

    pub fn screens(&self) -> Vec<String> {
        self.flights.iter().map(|flight| flight.screen.clone()).collect()
    }

    fn give_up(&mut self, opened: Option<u32>, error: &anyhow::Error) {
        let reason = reason_of(error);
        if reason == "helper budget" {
            self.late += 1;
        }
        let normal = matches!(reason, "hidden" | "no room" | "too large" | "helper cold" | "nothing new") || (reason == "helper budget" && self.late < LATE);
        if let Some(link) = self.link.as_mut() {
            if let Some(id) = opened.filter(|id| !self.flights.iter().any(|flight| flight.id == *id)) {
                link.close(id);
            }
        }
        let detail = format!("{error:#}");
        skipped(reason, &detail);
        keep(&mut self.said, format!("skipped, {reason}, {detail}"));
        if !normal {
            self.fault();
        }
    }

    fn fault(&mut self) {
        let alive = self.link.as_mut().is_some_and(|link| link.alive());
        if !alive {
            if !self.flights.is_empty() {
                let line = "the helper is gone, so its layers went with it and none could be held".to_string();
                eprintln!("{} glide {line}", crate::daemon::stamp());
                keep(&mut self.said, line);
            }
            self.flights.clear();
            self.early.clear();
            self.doomed = false;
            self.revive();
            return;
        }
        let early = std::mem::take(&mut self.early);
        let flights = std::mem::take(&mut self.flights);
        if let Some(link) = self.link.as_mut() {
            for unused in early {
                link.close(unused.id);
            }
            for flight in flights {
                if flight.set.is_some() {
                    self.flights.push(Flight { settled: true, ..flight });
                } else {
                    link.close(flight.id);
                }
            }
        }
        self.doomed = true;
        if self.flights.is_empty() {
            self.doomed = false;
            self.revive();
        }
    }

    fn revive(&mut self) {
        let broke = self.warmed.is_some() && !self.revived && self.link.as_mut().is_some_and(|link| link.broke());
        self.drop_link();
        if let (true, Some((picture, size, canvas))) = (broke, self.last_warm.clone()) {
            if self.send_warm(&picture, size, canvas.as_deref()).is_ok() {
                self.revived = true;
                note(&mut self.said, "the helper stopped without leaving cleanly, so a fresh one is warming for the next drag".into());
            }
        }
    }

    pub fn land(&mut self, hold: Duration, interrupted: &dyn Fn() -> bool) {
        if self.flights.is_empty() {
            return;
        }
        let started = Instant::now();
        let deadline = started + SETTLE_WAIT;
        while self.flights.iter().any(|flight| !flight.settled) {
            if interrupted() {
                let ids: Vec<u32> = self.flights.iter().map(|flight| flight.id).collect();
                for id in ids {
                    self.abort(id);
                }
                note(&mut self.said, "interrupted, closed early".into());
                return;
            }
            if Instant::now() >= deadline {
                self.give_up(None, &anyhow!("the helper did not settle within {} ms", SETTLE_WAIT.as_millis()));
                continue;
            }
            let Some(link) = self.link.as_mut() else {
                self.give_up(None, &anyhow!("the helper is gone"));
                return;
            };
            match link.hear(CHECK.min(Self::left(deadline))) {
                Ok(Heard::Settled(id)) => {
                    if let Some(flight) = self.flights.iter_mut().find(|flight| flight.id == id) {
                        flight.settled = true;
                        flight.heard = true;
                    }
                }
                Ok(Heard::Glided { id, frames, slow, scale, first }) => {
                    if let Some(size) = self.flights.iter().find(|flight| flight.id == id).map(|flight| flight.size) {
                        note(&mut self.said, format!("glided {id} with {frames} frames, {slow} slow, at scale {scale}, first frame {first} ms after the glide was sent"));
                        self.judge(frames, slow);
                        if let Some(adapter) = self.adapter.clone() {
                            self.learned.push((scale_key(&adapter, size), scale));
                        }
                    }
                }
                Ok(Heard::Failed(Some(id), why)) if self.flights.iter().any(|flight| flight.id == id) => {
                    if let Some(flight) = self.flights.iter_mut().find(|flight| flight.id == id) {
                        flight.settled = true;
                    }
                    skipped("helper failed", &why);
                    keep(&mut self.said, format!("skipped, helper failed, {why}"));
                }
                Ok(Heard::Failed(None, why)) => self.give_up(None, &anyhow!(why)),
                Ok(_) => {}
                Err(error) => {
                    self.give_up(None, &error);
                    if self.flights.is_empty() {
                        return;
                    }
                }
            }
        }
        let words = if !self.flights.is_empty() && self.flights.iter().all(|flight| flight.heard) { "settled after" } else { "gave up the settle after" };
        note(&mut self.said, format!("{words} {} ms", started.elapsed().as_millis()));
        let mut due: Vec<(Instant, u32)> = self.flights.drain(..).map(|flight| (flight.set.map_or_else(Instant::now, |set| set + hold), flight.id)).collect();
        due.sort();
        for (when, id) in due {
            while Instant::now() < when && !interrupted() {
                std::thread::sleep(CHECK.min(Self::left(when)));
            }
            if let Some(link) = self.link.as_mut() {
                link.close(id);
            }
            note(&mut self.said, format!("closed {id} {} ms after its wallpaper was set", hold.as_millis()));
        }
        if self.doomed || self.link.as_mut().is_some_and(|link| !link.alive()) {
            self.doomed = false;
            self.revive();
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn after(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn a_decision_200_ms_after_the_wake_waits_for_a_helper_shown_at_370_ms() {
        let woke = Instant::now();
        assert_eq!(hold_until(after(woke, 200), Some(woke), Some(after(woke, 370))), Some(after(woke, 450)));
    }

    #[test]
    fn the_same_decision_swaps_plainly_at_once_for_a_helper_shown_at_470_ms() {
        let woke = Instant::now();
        assert_eq!(hold_until(after(woke, 200), Some(woke), Some(after(woke, 470))), None);
    }

    #[test]
    fn a_decision_400_ms_after_the_wake_waits_for_a_helper_shown_at_540_ms() {
        let woke = Instant::now();
        assert_eq!(hold_until(after(woke, 400), Some(woke), Some(after(woke, 540))), Some(after(woke, 550)));
    }

    #[test]
    fn no_prediction_swaps_plainly_at_once() {
        let woke = Instant::now();
        assert_eq!(hold_until(after(woke, 200), Some(woke), None), None);
        assert_eq!(hold_until(after(woke, 200), None, None), None);
    }

    fn lone_screen() -> Screen {
        Screen { id: "main".into(), frame: scooch_core::Rect { x: 0.0, y: 0.0, w: 160.0, h: 100.0 }, scale: 1.0 }
    }

    fn cold_drag(glider: &mut Glider, screen: &Screen) -> Option<u32> {
        glider.open_early(screen, Path::new("p.jpg"), [160, 100], Path::new("s.png"), None);
        glider.prepare(&Plan { screen, picture: Path::new("p.jpg"), canvas: None, size: [160, 100], still: Path::new("s.png") })
    }

    fn ready_waits(waits: &Waits) -> usize {
        waits.lock().unwrap().iter().filter(|(what, wait)| *what == "ready" && !wait.is_zero()).count()
    }

    fn plan(screen: &Screen) -> Plan<'_> {
        Plan { screen, picture: Path::new("p.jpg"), canvas: None, size: [160, 100], still: Path::new("s.png") }
    }

    #[test]
    fn a_helper_launched_20_ms_before_the_question_keeps_its_tables_and_glides_when_the_hello_comes() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        spawner.script.lock().unwrap().slow_hello = Duration::from_millis(60);
        let mut glider = Glider::new(Box::new(spawner));
        let screen = lone_screen();
        glider.open_early(&screen, Path::new("p.jpg"), [160, 100], Path::new("s.png"), None);
        std::thread::sleep(Duration::from_millis(20));
        assert!(!glider.cannot_glide(&plan(&screen)), "{:?}", glider.said);
        std::thread::sleep(Duration::from_millis(60));
        assert!(glider.prepare(&plan(&screen)).is_some(), "{:?}", glider.said);
    }

    #[test]
    fn a_helper_launched_150_ms_before_the_question_with_no_hello_gets_no_tables() {
        let log: Events = Arc::new(Mutex::new(Vec::new()));
        let spawner = FakeSpawner::new(log.clone());
        spawner.script.lock().unwrap().slow_hello = Duration::from_secs(10);
        let mut glider = Glider::new(Box::new(spawner));
        let screen = lone_screen();
        glider.open_early(&screen, Path::new("p.jpg"), [160, 100], Path::new("s.png"), None);
        std::thread::sleep(Duration::from_millis(150));
        assert!(glider.cannot_glide(&plan(&screen)), "{:?}", glider.said);
        assert!(glider.said.iter().any(|line| line.contains("skipped, helper cold, the helper had not started")), "{:?}", glider.said);
        let said: Vec<String> = log.lock().unwrap().iter().map(|(what, _)| what.clone()).collect();
        assert!(said.iter().any(|what| what.starts_with("open ")) && said.iter().any(|what| what.starts_with("close ")), "{said:?}");
    }

    #[test]
    fn a_first_drag_with_no_memory_waits_and_learns() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        let (script, waits) = (spawner.script.clone(), spawner.waits.clone());
        {
            let mut script = script.lock().unwrap();
            script.ready_after = Duration::from_millis(60);
            script.device_after = Some(Duration::from_millis(20));
        }
        let mut glider = Glider::new(Box::new(spawner));
        let screen = lone_screen();
        assert!(cold_drag(&mut glider, &screen).is_some(), "{:?}", glider.said);
        assert_eq!(ready_waits(&waits), 1, "{:?}", waits.lock().unwrap());
        let starts = glider.learned_starts();
        assert!(starts.iter().any(|(name, start)| name == "Fake GPU" && start.colds.len() == 1 && start.missed.is_none()), "{starts:?}");
    }

    #[test]
    fn a_missed_first_wait_is_not_repeated_and_a_helper_ready_first_learns_the_true_time() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        let (script, waits) = (spawner.script.clone(), spawner.waits.clone());
        let mut glider = Glider::new(Box::new(spawner));
        let screen = lone_screen();
        script.lock().unwrap().slow_hello = Duration::from_secs(1);
        let started = Instant::now();
        assert!(cold_drag(&mut glider, &screen).is_none());
        assert!(started.elapsed() < PATIENCE && ready_waits(&waits) == 0, "a helper with its hello a second late held the set {:?}", started.elapsed());
        glider.reset_for_test();
        {
            let mut script = script.lock().unwrap();
            script.slow_hello = Duration::ZERO;
            script.ready_after = Duration::from_secs(2);
        }
        let started = Instant::now();
        assert!(cold_drag(&mut glider, &screen).is_none());
        assert!(started.elapsed() >= PATIENCE - Duration::from_millis(20) && ready_waits(&waits) == 1, "the first drag with no memory did not wait, {:?}", started.elapsed());
        assert!(glider.missed.is_some() && glider.said.iter().any(|line| line.contains("missed the wait")), "{:?}", glider.said);
        glider.reset_for_test();
        let started = Instant::now();
        assert!(cold_drag(&mut glider, &screen).is_none());
        assert!(started.elapsed() < PATIENCE && ready_waits(&waits) == 1, "the missed wait was repeated, {:?}", started.elapsed());
        assert!(glider.said.iter().any(|line| line.contains("would be shown")), "{:?}", glider.said);
        glider.reset_for_test();
        {
            let mut script = script.lock().unwrap();
            script.ready_after = Duration::from_millis(40);
            script.device_after = Some(Duration::from_millis(10));
        }
        glider.open_early(&screen, Path::new("p.jpg"), [160, 100], Path::new("s.png"), None);
        std::thread::sleep(Duration::from_millis(80));
        assert!(glider.prepare(&Plan { screen: &screen, picture: Path::new("p.jpg"), canvas: None, size: [160, 100], still: Path::new("s.png") }).is_some(), "{:?}", glider.said);
        assert!(glider.missed.is_none());
        let starts = glider.learned_starts();
        let learned = starts.iter().find(|(name, _)| name == "Fake GPU").map(|(_, start)| start.clone()).unwrap_or_default();
        assert!(learned.colds.len() == 1 && learned.colds[0] >= 40 && learned.colds[0] < 1000 && learned.missed.is_none(), "{starts:?}");
    }

    #[derive(Clone, Default)]
    pub struct Script {
        pub adapter: String,
        pub kind: String,
        pub held: Option<Arc<std::sync::atomic::AtomicBool>>,
        pub glided: Option<(u32, u32)>,
        pub spawn_fails: bool,
        pub ready_fails: bool,
        pub shown_fails: bool,
        pub refuse_after: Option<u32>,
        pub too_large: bool,
        pub settle_never: bool,
        pub dies_after_glide: bool,
        pub settle_after: Duration,
        pub slow_hello: Duration,
        pub shown_after: Duration,
        pub shown_after_asked: Option<Duration>,
        pub ready_after: Duration,
        pub device_after: Option<Duration>,
        pub canvas_after: Option<Duration>,
    }

    pub type Waits = Arc<Mutex<Vec<(&'static str, Duration)>>>;
    pub type Events = Arc<Mutex<Vec<(String, Instant)>>>;

    pub struct FakeHelper {
        born: Instant,
        log: Events,
        waits: Waits,
        script: Arc<Mutex<Script>>,
        settles: Vec<(u32, Instant)>,
        glides: Vec<u32>,
        asked: Option<(u32, Instant)>,
        dead: bool,
    }

    pub struct FakeSpawner {
        pub log: Events,
        pub script: Arc<Mutex<Script>>,
        pub spawned: Arc<Mutex<usize>>,
        pub waits: Waits,
    }

    impl FakeSpawner {
        pub fn new(log: Events) -> Self {
            Self { log, script: Arc::new(Mutex::new(Script::default())), spawned: Arc::new(Mutex::new(0)), waits: Arc::new(Mutex::new(Vec::new())) }
        }
    }

    impl Spawn for FakeSpawner {
        fn spawn(&self) -> Result<Box<dyn Link>> {
            self.waits.lock().unwrap().push(("spawn", Duration::ZERO));
            if self.script.lock().unwrap().spawn_fails {
                return Err(anyhow!("the test helper could not start"));
            }
            *self.spawned.lock().unwrap() += 1;
            self.log.lock().unwrap().push(("spawn".into(), Instant::now()));
            Ok(Box::new(FakeHelper { born: Instant::now(), log: self.log.clone(), waits: self.waits.clone(), script: self.script.clone(), settles: Vec::new(), glides: Vec::new(), asked: None, dead: false }))
        }
    }

    impl FakeHelper {
        fn say(&self, what: String) {
            self.log.lock().unwrap().push((what, Instant::now()));
        }

        fn script(&self) -> Script {
            self.script.lock().unwrap().clone()
        }

        fn living(&self) -> Result<()> {
            if self.dead {
                return Err(anyhow!("the helper is gone"));
            }
            Ok(())
        }

        fn shown_at(&self, id: u32) -> Duration {
            let script = self.script();
            let shown = match (script.shown_after_asked, self.asked) {
                (Some(after), Some((asked, at))) if asked == id => at.saturating_duration_since(self.born) + after,
                _ => script.shown_after,
            };
            script.slow_hello.max(shown)
        }

        fn awake(&self, id: u32, timeout: Duration) -> Result<()> {
            self.until(self.shown_at(id), timeout)
        }

        fn until(&self, at: Duration, timeout: Duration) -> Result<()> {
            let left = at.saturating_sub(self.born.elapsed());
            if left > timeout {
                std::thread::sleep(timeout);
                return Err(anyhow!("the helper did not answer within {} ms", timeout.as_millis()));
            }
            std::thread::sleep(left);
            Ok(())
        }

        fn ready_at(&self) -> Duration {
            let script = self.script();
            script.slow_hello.max(script.ready_after)
        }

        fn adapter(&self) -> Adapter {
            let script = self.script();
            Adapter { name: if script.adapter.is_empty() { "Fake GPU".into() } else { script.adapter }, kind: script.kind, from: Some("cache".into()), scale: Some(1.0), after: Some(self.ready_at()) }
        }
    }

    impl Link for FakeHelper {
        fn device(&mut self) -> Result<()> {
            self.living()?;
            self.say("device".into());
            Ok(())
        }

        fn warm(&mut self, _picture: &Path, size: [u32; 2], canvas: Option<&Path>, scales: &BTreeMap<String, f32>) -> Result<()> {
            self.living()?;
            self.say(format!("warm {}x{}{}{}", size[0], size[1], if canvas.is_some() { " canvas" } else { "" }, scales.iter().map(|(adapter, scale)| format!(" {adapter}={scale}")).collect::<String>()));
            Ok(())
        }

        fn ready(&mut self, timeout: Duration) -> Result<Adapter> {
            self.living()?;
            if !timeout.is_zero() {
                self.waits.lock().unwrap().push(("ready", timeout));
            }
            self.until(self.ready_at(), timeout)?;
            if self.script().ready_fails {
                return Err(anyhow!("no device"));
            }
            Ok(self.adapter())
        }

        fn answered(&mut self, id: u32) -> bool {
            if self.asked.is_none_or(|(asked, _)| asked != id) {
                self.asked = Some((id, Instant::now()));
            }
            self.dead || self.born.elapsed() >= self.shown_at(id)
        }

        fn window(&mut self, screen: &Screen) -> Result<()> {
            self.living()?;
            self.say(format!("window {}", screen.id));
            Ok(())
        }

        fn open(&mut self, id: u32, _screen: &Screen, still: &Path) -> Result<()> {
            self.living()?;
            self.say(format!("open {id} {}", still.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
            Ok(())
        }

        fn drawn(&mut self, _id: u32) -> Option<Split> {
            Some(Split { window: 1, first: 1, reveal: 1, ..Split::default() })
        }

        fn shown(&mut self, id: u32, timeout: Duration) -> Result<Instant> {
            self.living()?;
            self.waits.lock().unwrap().push(("shown", timeout));
            self.awake(id, timeout)?;
            let script = self.script();
            if script.shown_fails {
                self.say(format!("hidden {id}"));
                return Err(anyhow!("macOS never reported the live layer as visible, so Scooch keeps the plain swap."));
            }
            if script.too_large {
                self.say(format!("huge {id}"));
                return Err(anyhow!("the screen is too large for the GPU"));
            }
            if script.refuse_after.is_some_and(|first| id >= first) {
                self.say(format!("refused {id}"));
                return Err(anyhow!("the layer could not be ordered under the icons"));
            }
            self.say(format!("shown {id}"));
            Ok(Instant::now())
        }

        fn glide(&mut self, id: u32, a: &Path, b: &Path, _ms: u32) -> Result<()> {
            self.living()?;
            self.say(format!("glide {id} {} {}", a.exists(), b.exists()));
            self.glides.push(id);
            if self.script().dies_after_glide {
                self.dead = true;
            }
            Ok(())
        }

        fn settle(&mut self, id: u32, still: &Path) -> Result<()> {
            self.living()?;
            self.say(format!("settle {id} {}", still.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
            self.settles.push((id, Instant::now() + self.script().settle_after));
            Ok(())
        }

        fn hear(&mut self, timeout: Duration) -> Result<Heard> {
            self.living()?;
            let script = self.script();
            if script.settle_never || script.held.as_ref().is_some_and(|held| held.load(std::sync::atomic::Ordering::SeqCst)) {
                std::thread::sleep(timeout.min(Duration::from_millis(5)));
                return Ok(Heard::Nothing);
            }
            if let (Some((frames, slow)), Some(id)) = (self.script().glided, self.glides.pop()) {
                return Ok(Heard::Glided { id, frames, slow, scale: 1.0, first: 16 });
            }
            if let Some(index) = self.settles.iter().position(|(_, at)| *at <= Instant::now()) {
                let (id, _) = self.settles.remove(index);
                self.say(format!("settled {id}"));
                return Ok(Heard::Settled(id));
            }
            std::thread::sleep(timeout.min(Duration::from_millis(5)));
            Ok(Heard::Nothing)
        }

        fn close(&mut self, id: u32) {
            if !self.dead {
                self.say(format!("close {id}"));
            }
        }

        fn quit(&mut self) {
            if !self.dead {
                self.say("quit".into());
            }
        }

        fn alive(&mut self) -> bool {
            !self.dead
        }

        fn broke(&mut self) -> bool {
            self.dead
        }

        fn greeted(&mut self) -> Option<Duration> {
            let hello = self.script().slow_hello;
            (self.born.elapsed() >= hello).then_some(hello)
        }

        fn device_ready(&mut self) -> Option<Duration> {
            self.script().device_after.filter(|after| self.born.elapsed() >= *after)
        }

        fn canvas_ready(&mut self) -> Option<Duration> {
            self.script().canvas_after.filter(|after| self.born.elapsed() >= *after)
        }

        fn ready_after(&mut self) -> Option<Adapter> {
            (self.born.elapsed() >= self.ready_at() && !self.script().ready_fails).then(|| self.adapter())
        }
    }
}
