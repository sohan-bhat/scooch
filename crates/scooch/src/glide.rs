use crate::live::{self, Helper, Hints, Reply, Stills};
use crate::platform::Screen;
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const FORCE: &str = "SCOOCH_GLIDE_ANYWAY";
const SOFTWARE: [&str; 3] = ["basic render driver", "llvmpipe", "swiftshader"];
const STRIKES: u32 = 2;
pub const GLIDE_MS: u32 = 450;
pub const SETTLE_WAIT: Duration = Duration::from_secs(2);
const SETTLE_AFTER_GLIDE: Duration = Duration::from_millis(1500);
const FORGOTTEN: Duration = Duration::from_secs(crate::live::helper::IDLE_EXIT.as_secs() - 5);
pub const CHECK: Duration = Duration::from_millis(50);
pub const PATIENCE: Duration = Duration::from_millis(150);
pub const SHOWN_AFTER_WAKE: Duration = Duration::from_millis(450);
pub const BARE: u32 = 2;
pub const WAKE_EVERY: Duration = Duration::from_secs(10);
pub const PRIME_LIFE: Duration = Duration::from_secs(5);
const NOTHING_NEW: &str = "the new picture is the one on screen or could not be made, so nothing glides";
pub const NORMAL: [&str; 6] = ["hidden", "no room", "too large", "helper cold", "helper budget", "system picture"];

pub type Nudge = Arc<dyn Fn() + Send + Sync>;

const PARK_WARM: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Glided { id: u32, frames: u32, slow: u32, scale: f32, first: u64 },
    Settled(u32),
    Failed(Option<u32>, String),
    Nothing,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Milestones {
    pub launched: Option<Instant>,
    pub hello: Option<Instant>,
    pub device: Option<Instant>,
    pub canvas: Option<Instant>,
    pub ready: Option<Instant>,
    pub primed: Option<(Instant, String)>,
}

pub trait Link: Send {
    fn park(&mut self) -> Result<()>;
    fn parked(&mut self) -> Option<Result<Adapter, String>>;
    fn warm(&mut self, picture: &Path, size: [u32; 2], canvas: Option<&Path>, key: u64, scales: &BTreeMap<String, f32>) -> Result<()>;
    fn ready(&mut self, timeout: Duration) -> Result<Adapter>;
    fn prime(&mut self, screen: &Screen, stills: &Stills) -> Result<()>;
    fn open(&mut self, id: u32, screen: &Screen, stills: &Stills) -> Result<()>;
    fn shown(&mut self, id: u32, timeout: Duration) -> Result<Instant>;
    fn drawn(&mut self, id: u32) -> Option<Split>;
    fn glide(&mut self, id: u32, a: &Path, b: &Path, ms: u32) -> Result<()>;
    fn settle(&mut self, id: u32, still: &Path, ms: Option<u32>) -> Result<()>;
    fn hear(&mut self, timeout: Duration) -> Result<Heard>;
    fn close(&mut self, id: u32);
    fn quit(&mut self);
    fn alive(&mut self) -> bool;
    fn quiet(&self) -> Duration;
    fn milestones(&mut self) -> Milestones;
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
    pub still: Option<String>,
    pub screen_gpu: Option<String>,
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
    fn spawn(&self, nudge: Option<Nudge>) -> Result<Box<dyn Link>>;
}

pub struct Program(pub Option<PathBuf>);

impl Spawn for Program {
    fn spawn(&self, nudge: Option<Nudge>) -> Result<Box<dyn Link>> {
        let program = self.0.as_deref().ok_or_else(|| anyhow!("Scooch could not find its own program"))?;
        let watch = Box::new(move |reply: Option<&Reply>| {
            if let (true, Some(nudge)) = (wakes(reply), nudge.as_ref()) {
                nudge();
            }
        });
        Ok(Box::new(Real::new(Helper::launch_watched(program, watch)?)))
    }
}

fn wakes(reply: Option<&Reply>) -> bool {
    reply.is_none_or(|reply| matches!(reply, Reply::Ready { warm_id: Some(PARK_WARM), .. } | Reply::Failed { id: None, .. }))
}

fn take_failure(failed: &mut Vec<(Option<u32>, String)>, id: u32) -> Option<String> {
    let index = failed.iter().position(|(failed, _)| *failed == Some(id))?;
    Some(failed.remove(index).1)
}

fn take_warm_failure(failed: &mut Vec<(Option<u32>, String)>, warm_id: Option<u32>) -> Option<String> {
    let prefix = warm_id.map(|id| format!("warm {id}: "));
    let index = failed.iter().position(|(id, why)| id.is_none() && if why.starts_with("warm ") { prefix.as_ref().is_some_and(|prefix| why.starts_with(prefix.as_str())) } else { true })?;
    Some(failed.remove(index).1)
}

fn take_heard(failed: &mut Vec<(Option<u32>, String)>) -> Option<(Option<u32>, String)> {
    let index = failed.iter().position(|(id, why)| id.is_some() || !why.starts_with("warm "))?;
    Some(failed.remove(index))
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
    park_id: Option<u32>,
    parked: Option<Adapter>,
    marks: Milestones,
    drawn: Vec<(u32, Split)>,
}

impl Real {
    fn new(helper: Helper) -> Self {
        let marks = Milestones { launched: Some(helper.launched()), ..Milestones::default() };
        Self { helper, shown: Vec::new(), gone: Vec::new(), failed: Vec::new(), heard: Vec::new(), readies: Vec::new(), warm_id: None, park_id: None, parked: None, marks, drawn: Vec::new() }
    }

    fn pump(&mut self, timeout: Duration) -> Result<bool> {
        let started = Instant::now();
        let Some(reply) = self.helper.next(timeout) else {
            if self.helper.exited() || started.elapsed() < timeout / 2 {
                return Err(anyhow!("the helper is gone"));
            }
            return Ok(false);
        };
        let arrived = self.helper.arrived().unwrap_or_else(Instant::now);
        match reply {
            Reply::Shown { id, .. } if self.gone.contains(&id) => {}
            Reply::Shown { id, .. } => self.shown.push((id, arrived)),
            Reply::Closed { .. } => {}
            Reply::Device { .. } => self.marks.device = self.marks.device.or(Some(arrived)),
            Reply::Canvas { warm_id, .. } if warm_id.is_some() && warm_id == self.warm_id => self.marks.canvas = Some(arrived),
            Reply::Canvas { .. } => {}
            Reply::Primed { still, .. } => self.marks.primed = Some((arrived, still)),
            Reply::Drawn { id, window, first, reveal, make, present, found, steps, revealing, still, screen_gpu } => {
                if self.drawn.len() >= 8 {
                    self.drawn.remove(0);
                }
                let part = |ms: u64| u32::try_from(ms).unwrap_or(u32::MAX);
                let steps = steps.into_iter().map(|(name, took)| (name, part(took))).collect();
                let revealing = revealing.into_iter().map(|(name, took)| (name, part(took))).collect();
                self.drawn.push((id, Split { window: part(window), first: part(first), reveal: part(reveal), make: part(make), present: part(present), found, steps, revealing, still, screen_gpu }));
            }
            Reply::Settled { id } => self.heard.push(Heard::Settled(id)),
            Reply::Glided { id, frames, slow_frames, scale, first_frame_ms, .. } => self.heard.push(Heard::Glided { id, frames, slow: slow_frames, scale, first: first_frame_ms }),
            Reply::Failed { id: Some(id), .. } if self.gone.contains(&id) => {}
            Reply::Failed { id, why } => self.failed.push((id, why)),
            Reply::Ready { adapter, kind, warm_id, from, scale, .. } => {
                let after = self.helper.since_launch();
                let adapter = Adapter { name: adapter, kind, from, scale, after };
                if warm_id.is_some() && warm_id == self.park_id && self.parked.is_none() {
                    self.parked = Some(adapter.clone());
                }
                if warm_id.is_some() && warm_id == self.warm_id {
                    self.marks.ready = Some(arrived);
                }
                self.readies.push((warm_id, adapter));
            }
            Reply::Hello { version, .. } if version != live::VERSION => self.failed.push((None, format!("the helper speaks version {version}, not {}", live::VERSION))),
            Reply::Hello { .. } => self.marks.hello = Some(arrived),
        }
        Ok(true)
    }

    fn drain(&mut self) {
        while matches!(self.pump(Duration::ZERO), Ok(true)) {}
    }

    fn failure(&mut self, id: u32) -> Option<String> {
        take_failure(&mut self.failed, id)
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
    fn park(&mut self) -> Result<()> {
        self.park_id = Some(self.helper.ask_park()?);
        Ok(())
    }

    fn parked(&mut self) -> Option<Result<Adapter, String>> {
        self.drain();
        if let Some(adapter) = self.parked.clone() {
            return Some(Ok(adapter));
        }
        let prefix = format!("warm {}: ", self.park_id?);
        let index = self.failed.iter().position(|(id, why)| id.is_none() && why.starts_with(&prefix))?;
        Some(Err(self.failed.remove(index).1))
    }

    fn warm(&mut self, picture: &Path, size: [u32; 2], canvas: Option<&Path>, key: u64, scales: &BTreeMap<String, f32>) -> Result<()> {
        let park = self.park_id.map(|id| format!("warm {id}: "));
        self.failed.retain(|(id, why)| id.is_some() || !why.starts_with("warm ") || park.as_ref().is_some_and(|park| why.starts_with(park.as_str())));
        self.warm_id = Some(self.helper.ask_warm_keyed(Hints::default(), (Some(picture), Some(key)), Some(size), canvas, scales.clone())?);
        self.marks.canvas = None;
        self.marks.ready = None;
        Ok(())
    }

    fn ready(&mut self, timeout: Duration) -> Result<Adapter> {
        let wanted = self.warm_id;
        self.drain();
        self.wait(timeout, |me| {
            if let Some(index) = me.readies.iter().position(|(answered, _)| *answered == wanted) {
                me.readies.drain(..index);
                return Some(Ok(me.readies[0].1.clone()));
            }
            take_warm_failure(&mut me.failed, wanted).map(|why| Err(anyhow!("{why}")))
        })
    }

    fn prime(&mut self, screen: &Screen, stills: &Stills) -> Result<()> {
        self.marks.primed = None;
        self.helper.ask_window(screen, stills)
    }

    fn open(&mut self, id: u32, screen: &Screen, stills: &Stills) -> Result<()> {
        self.helper.ask_open(id, screen, stills)
    }

    fn shown(&mut self, id: u32, timeout: Duration) -> Result<Instant> {
        self.drain();
        self.wait(timeout, |me| {
            if let Some(index) = me.shown.iter().position(|(shown, _)| *shown == id) {
                return Some(Ok(me.shown.remove(index).1));
            }
            me.failure(id).map(|why| Err(anyhow!("{why}")))
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

    fn settle(&mut self, id: u32, still: &Path, ms: Option<u32>) -> Result<()> {
        self.helper.ask_settle(id, still, ms)
    }

    fn hear(&mut self, timeout: Duration) -> Result<Heard> {
        if !self.heard.is_empty() {
            return Ok(self.heard.remove(0));
        }
        if let Some((id, why)) = take_heard(&mut self.failed) {
            return Ok(Heard::Failed(id, why));
        }
        self.pump(timeout)?;
        if !self.heard.is_empty() {
            return Ok(self.heard.remove(0));
        }
        Ok(take_heard(&mut self.failed).map_or(Heard::Nothing, |(id, why)| Heard::Failed(id, why)))
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

    fn quiet(&self) -> Duration {
        self.helper.since_sent()
    }

    fn milestones(&mut self) -> Milestones {
        self.drain();
        self.marks.clone()
    }
}

pub struct Warmup {
    pub picture: PathBuf,
    pub size: [u32; 2],
    pub canvas: Option<PathBuf>,
    pub key: u64,
}

pub struct Prime<'a> {
    pub screen: &'a Screen,
    pub warm: &'a Warmup,
    pub stills: Stills,
    pub drag: bool,
}

pub struct Plan<'a> {
    pub screen: &'a Screen,
    pub picture: &'a Path,
    pub canvas: Option<&'a Path>,
    pub key: u64,
    pub size: [u32; 2],
    pub stills: Stills,
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
    trail: Move,
}

struct Ready {
    id: u32,
    screen: String,
    size: [u32; 2],
    started: Instant,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Move {
    pub screen: String,
    pub drop: Option<Instant>,
    pub begun: Option<Instant>,
    pub marks: Milestones,
    pub parked: bool,
    pub dragged: Option<bool>,
    pub primed_sent: Option<Instant>,
    pub tables: Option<Instant>,
    pub opened: Option<Instant>,
    pub shown: Option<Instant>,
    pub glide: Option<Instant>,
    pub first: Option<Instant>,
    pub set: Option<Instant>,
    pub waited: Option<Duration>,
    pub allowed: Option<Duration>,
    pub drawn: Option<Split>,
    pub helper_gpu: Option<String>,
    pub verdict: Option<(String, String)>,
}

fn signed(at: Instant, origin: Instant) -> i128 {
    if at >= origin {
        at.duration_since(origin).as_millis() as i128
    } else {
        -(origin.duration_since(at).as_millis() as i128)
    }
}

impl Move {
    pub fn new(screen: &str, drop: Option<Instant>) -> Move {
        Move { screen: screen.to_string(), drop, begun: Some(Instant::now()), ..Move::default() }
    }

    pub fn skip(&mut self, reason: &str, detail: &str) {
        if self.verdict.is_none() {
            self.verdict = Some((format!("skipped, {reason}"), detail.to_string()));
        }
    }

    pub fn slid(&mut self, detail: &str) {
        self.verdict = Some(("slid".to_string(), detail.to_string()));
    }

    pub fn also(&mut self, text: &str) {
        match self.verdict.as_mut() {
            Some((_, detail)) if detail.is_empty() => *detail = text.to_string(),
            Some((_, detail)) => *detail = format!("{detail}, {text}"),
            None => self.verdict = Some(("skipped, helper failed".to_string(), text.to_string())),
        }
    }

    #[cfg(test)]
    pub fn reason(&self) -> Option<&str> {
        self.verdict.as_ref().and_then(|(verdict, _)| verdict.strip_prefix("skipped, "))
    }

    pub fn line(&self) -> String {
        let origin = self.drop.or(self.begun).unwrap_or_else(Instant::now);
        let at = |name: &str, when: Option<Instant>| match when {
            Some(when) => format!("{name} {} ms", signed(when, origin)),
            None => format!("{name} none"),
        };
        let (verdict, detail) = self.verdict.clone().unwrap_or_else(|| ("skipped, untold".to_string(), String::new()));
        let mut parts = vec![if detail.is_empty() { verdict } else { format!("{verdict}, {detail}") }];
        parts.push(format!("on {}", self.screen));
        parts.push(format!("times from the {}", if self.drop.is_some() { "drop" } else { "tick" }));
        parts.push(if self.parked { "helper parked".to_string() } else { "helper not parked".to_string() });
        parts.push(at("hello", self.marks.hello));
        parts.push(at("device", self.marks.device));
        parts.push(match self.dragged {
            Some(true) => at("primed at the drag start", self.primed_sent),
            Some(false) => at("primed at the wake", self.primed_sent),
            None => "primed none".to_string(),
        });
        parts.push(at("canvas", self.marks.canvas));
        let still = self.drawn.as_ref().and_then(|split| split.still.clone()).or_else(|| self.marks.primed.as_ref().map(|(_, still)| still.clone()));
        parts.push(match (self.marks.primed.as_ref(), still) {
            (Some((when, _)), Some(still)) => format!("still {} ms {still}", signed(*when, origin)),
            (None, Some(still)) => format!("still at the open {still}"),
            (_, None) => "still none".to_string(),
        });
        parts.push(at("ready", self.marks.ready));
        parts.push(at("tables", self.tables));
        parts.push(at("opened", self.opened));
        parts.push(at("shown", self.shown));
        parts.push(at("glide sent", self.glide));
        parts.push(at("first moving frame", self.first));
        parts.push(at("set", self.set));
        if let (Some(waited), Some(allowed)) = (self.waited, self.allowed) {
            parts.push(format!("waited {} ms of {} ms", waited.as_millis(), allowed.as_millis()));
        }
        parts.push(format!("screen on {}", self.drawn.as_ref().and_then(|split| split.screen_gpu.clone()).unwrap_or_else(|| "unknown".to_string())));
        parts.push(format!("drawn on {}", self.helper_gpu.clone().unwrap_or_else(|| "unknown".to_string())));
        format!("glide {}", parts.join(", "))
    }

    pub fn write(&self) -> String {
        let line = self.line();
        eprintln!("{} {line}", crate::daemon::stamp());
        crate::daemon::trim_log();
        line.strip_prefix("glide ").unwrap_or(&line).to_string()
    }
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

pub fn hold_until(started: Instant, woke: Option<Instant>) -> Instant {
    woke.map_or(started + PATIENCE, |woke| (started + PATIENCE).max(woke + SHOWN_AFTER_WAKE))
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
    } else if ["has not parked", "is not running", "was given up"].iter().any(|cold| text.contains(cold)) {
        "helper cold"
    } else if text.contains(NOTHING_NEW) {
        "nothing new"
    } else if text.contains("did not answer within") || text.contains("was not shown within") || text.contains("was not loaded within") {
        "helper budget"
    } else if text.contains("did not settle") {
        "settle budget"
    } else if text.contains("too large") {
        "too large"
    } else {
        "helper failed"
    }
}

pub fn normal(reason: &str) -> bool {
    NORMAL.contains(&reason) || reason == "nothing new"
}

pub struct Glider {
    pub forced: bool,
    software: bool,
    strikes: u32,
    stalled: bool,
    spawner: Box<dyn Spawn>,
    link: Option<Box<dyn Link>>,
    parked: Option<Adapter>,
    replaced: bool,
    given_up: bool,
    doomed: bool,
    warmed: Option<(PathBuf, [u32; 2], u64)>,
    next: u32,
    flights: Vec<Flight>,
    early: Vec<Early>,
    primes: Vec<(String, PathBuf, Instant, bool)>,
    huge: std::collections::HashMap<String, [u32; 2]>,
    adapter: Option<String>,
    scales: BTreeMap<String, f32>,
    learned: Vec<(String, f32)>,
    prepared: Option<Ready>,
    moved_now: bool,
    bare_prime: bool,
    thrown: u32,
    last_wake_prime: Option<Instant>,
    woke: Option<Instant>,
    nudge: Option<Nudge>,
    pub said: Vec<String>,
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

fn tell(said: &mut Vec<String>, line: String) {
    eprintln!("{} glide {line}", crate::daemon::stamp());
    keep(said, line);
}

impl Glider {
    pub fn new(spawner: Box<dyn Spawn>) -> Self {
        let forced = std::env::var_os(FORCE).is_some_and(|value| !value.is_empty() && value != "0");
        Self {
            forced,
            software: false,
            strikes: 0,
            stalled: false,
            spawner,
            link: None,
            parked: None,
            replaced: false,
            given_up: false,
            doomed: false,
            warmed: None,
            next: 0,
            flights: Vec::new(),
            early: Vec::new(),
            primes: Vec::new(),
            huge: std::collections::HashMap::new(),
            adapter: None,
            scales: BTreeMap::new(),
            learned: Vec::new(),
            prepared: None,
            moved_now: false,
            bare_prime: false,
            thrown: 0,
            last_wake_prime: None,
            woke: None,
            nudge: None,
            said: Vec::new(),
        }
    }

    pub fn nudge_with(&mut self, nudge: Nudge) {
        self.nudge = Some(nudge);
    }

    #[cfg(test)]
    pub fn reset_for_test(&mut self) {
        self.drop_link();
    }

    #[cfg(test)]
    pub fn forget_wake_for_test(&mut self) {
        self.last_wake_prime = None;
        self.primes.clear();
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
                tell(&mut self.said, format!("Scooch turned the glide off until it starts again because {STRIKES} glides in a row stuttered."));
            }
        } else {
            self.strikes = 0;
        }
    }

    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.link.is_some()
    }

    #[cfg(test)]
    pub fn parked(&self) -> bool {
        self.parked.is_some()
    }

    fn drop_link(&mut self) {
        self.prepared = None;
        self.link = None;
        self.parked = None;
        self.warmed = None;
        self.primes.clear();
    }

    pub fn woke(&mut self, at: Instant) {
        self.woke = Some(at);
    }

    fn scales_for(&self, size: [u32; 2]) -> BTreeMap<String, f32> {
        let suffix = format!("|{}x{}", size[0], size[1]);
        self.scales.iter().filter_map(|(key, scale)| key.strip_suffix(&suffix).map(|adapter| (adapter.to_string(), *scale))).collect()
    }

    pub fn keep_parked(&mut self) {
        if !self.usable() || self.given_up {
            if self.link.is_some() && self.flights.is_empty() {
                let given_up = self.given_up;
                self.stop();
                self.given_up = given_up;
            }
            return;
        }
        if self.link.as_mut().is_some_and(|link| !link.alive()) {
            self.reap();
            if self.given_up {
                return;
            }
        }
        if self.link.is_some() {
            self.check_park();
            return;
        }
        let started = Instant::now();
        let launched = self.spawner.spawn(self.nudge.clone()).and_then(|mut link| {
            link.park()?;
            Ok(link)
        });
        match launched {
            Ok(link) => {
                self.link = Some(link);
                note(&mut self.said, format!("helper launched to park in {} ms", started.elapsed().as_millis()));
            }
            Err(error) => {
                tell(&mut self.said, format!("the helper could not start, {error:#}"));
                if self.replaced {
                    self.given_up = true;
                } else {
                    self.replaced = true;
                }
            }
        }
    }

    fn check_park(&mut self) {
        if self.parked.is_some() {
            return;
        }
        let adapter = match self.link.as_mut().and_then(|link| link.parked()) {
            None => return,
            Some(Ok(adapter)) => adapter,
            Some(Err(why)) => {
                tell(&mut self.said, format!("the helper could not make its GPU device, {why}, so Scooch keeps the plain swap until the glide is turned off and on or Scooch starts again"));
                self.stop();
                self.given_up = true;
                return;
            }
        };
        if !self.forced && software(&adapter) {
            self.software = true;
            tell(&mut self.said, format!("the only GPU here is the software renderer {} of kind {}, so Scooch keeps the plain swap and parks no helper", adapter.name, adapter.kind));
            self.stop();
            return;
        }
        tell(&mut self.said, format!("helper parked on {} of kind {} {} ms after its launch", adapter.name, adapter.kind, adapter.after.map_or("unknown".to_string(), |after| after.as_millis().to_string())));
        self.adapter = Some(adapter.name.clone());
        self.parked = Some(adapter);
    }

    pub fn stop(&mut self) {
        if let Some(mut link) = self.link.take() {
            link.quit();
            let _ = std::thread::Builder::new().name("scooch-quit".into()).spawn(move || drop(link));
        }
        self.drop_link();
        self.flights.clear();
        self.early.clear();
        self.replaced = false;
        self.given_up = false;
        self.doomed = false;
    }

    pub fn reap(&mut self) {
        let Some(link) = self.link.as_mut() else { return };
        if link.alive() {
            return;
        }
        let parked = self.parked.is_some();
        self.drop_link();
        self.flights.clear();
        self.early.clear();
        if self.replaced {
            self.given_up = true;
            tell(&mut self.said, "the helper stopped again before any glide since the last one, so Scooch keeps the plain swap until the glide is turned off and on or Scooch starts again".into());
            return;
        }
        self.replaced = true;
        tell(&mut self.said, format!("the helper stopped {}, so a fresh one parks", if parked { "while parked" } else { "before it parked" }));
    }

    pub fn after_snapshot(&mut self, moved: bool) {
        self.moved_now = moved;
        if moved {
            self.thrown = 0;
        } else if self.bare_prime {
            self.thrown += 1;
        }
        self.bare_prime = false;
    }

    pub fn prime(&mut self, prime: &Prime) -> bool {
        self.check_park();
        if !self.usable() || self.parked.is_none() || self.too_large(&prime.screen.id, prime.warm.size) || !prime.stills.still.as_deref().is_some_and(exact) {
            return false;
        }
        let still = prime.stills.still.clone().unwrap_or_default();
        let alive = self.primes.iter().any(|(screen, kept, at, _)| *screen == prime.screen.id && *kept == still && at.elapsed() < PRIME_LIFE);
        if alive {
            return false;
        }
        if !prime.drag && (self.thrown >= BARE || self.last_wake_prime.is_some_and(|at| at.elapsed() < WAKE_EVERY)) {
            return false;
        }
        if self.send_warm(&prime.warm.picture, prime.warm.size, prime.warm.canvas.as_deref(), prime.warm.key).is_err() {
            return false;
        }
        let Some(link) = self.link.as_mut() else { return false };
        if link.prime(prime.screen, &prime.stills).is_err() {
            return false;
        }
        let now = Instant::now();
        self.primes.retain(|(screen, _, _, _)| *screen != prime.screen.id);
        self.primes.push((prime.screen.id.clone(), still, now, prime.drag));
        if !prime.drag {
            self.last_wake_prime = Some(now);
            self.bare_prime = true;
        }
        note(&mut self.said, format!("primed {} at the {}", prime.screen.id, if prime.drag { "drag start" } else { "wake" }));
        true
    }

    pub fn know(&mut self, scales: &BTreeMap<String, f32>) {
        self.scales.clone_from(scales);
    }

    pub fn learned(&mut self) -> Vec<(String, f32)> {
        std::mem::take(&mut self.learned)
    }

    fn send_warm(&mut self, picture: &Path, size: [u32; 2], canvas: Option<&Path>, key: u64) -> Result<()> {
        let wanted = (picture.to_path_buf(), size, key);
        let kept = self.link.as_ref().is_some_and(|link| link.quiet() < FORGOTTEN);
        if self.warmed.as_ref() == Some(&wanted) && kept {
            return Ok(());
        }
        let scales = self.scales_for(size);
        self.link.as_mut().ok_or_else(|| anyhow!("the helper is not running"))?.warm(picture, size, canvas, key, &scales)?;
        note(&mut self.said, format!("warm {} at {}x{} from {}", picture.display(), size[0], size[1], canvas.map_or("the picture".to_string(), |canvas| canvas.display().to_string())));
        self.warmed = Some(wanted);
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
            tell(&mut self.said, format!("Scooch keeps the plain swap on screen {screen} at {}x{} because it is larger than the GPU can draw, until the screen changes or Scooch starts again.", size[0], size[1]));
        }
    }

    pub fn open_early(&mut self, screen: &Screen, (picture, key): (&Path, u64), size: [u32; 2], stills: &Stills, canvas: Option<&Path>) {
        let Some(still) = stills.still.clone() else { return };
        self.check_park();
        if !self.usable() || self.parked.is_none() || self.too_large(&screen.id, size) || self.early.iter().any(|early| early.screen == screen.id) || !exact(&still) {
            return;
        }
        let id = self.fresh_id();
        let result = (|| -> Result<()> {
            self.send_warm(picture, size, canvas, key)?;
            self.link.as_mut().ok_or_else(|| anyhow!("the helper is not running"))?.open(id, screen, stills)
        })();
        match result {
            Ok(()) => {
                note(&mut self.said, format!("open after snapshot {id} on {}", still.display()));
                self.early.push(Early { id, screen: screen.id.clone(), size, still, at: Instant::now() });
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

    pub fn refused(&mut self, plan: &Plan) -> Option<(&'static str, &'static str)> {
        if !self.usable() {
            Some((if self.software { "software adapter" } else { "strikes" }, ""))
        } else if self.too_large(&plan.screen.id, plan.size) {
            Some(("too large", ""))
        } else if !plan.stills.still.as_deref().is_some_and(exact) {
            Some(("old still", "the picture on screen was saved with less color detail than the layer shows, so this move swaps it plainly"))
        } else if self.given_up {
            Some(("helper cold", "the helper was given up after it stopped twice, so this move swaps plainly"))
        } else if self.link.is_none() {
            Some(("helper cold", "the helper is not running yet, so this move swaps plainly"))
        } else if self.parked.is_none() {
            self.check_park();
            self.parked.is_none().then_some(("helper cold", "the helper has not parked yet, so this move swaps plainly"))
        } else {
            None
        }
    }

    pub fn prepare(&mut self, plan: &Plan, trail: &mut Move) -> Option<u32> {
        if let Some((reason, detail)) = self.refused(plan) {
            self.close_early_for(&plan.screen.id);
            trail.skip(reason, detail);
            return None;
        }
        let started = Instant::now();
        let early = self.early.iter().position(|early| early.screen == plan.screen.id).map(|index| self.early.remove(index));
        let reused = early.as_ref().filter(|early| plan.stills.still.as_ref() == Some(&early.still)).map(|early| early.id);
        if let (Some(stale), None) = (early.as_ref(), reused) {
            if let Some(link) = self.link.as_mut() {
                link.close(stale.id);
            }
        }
        let id = reused.unwrap_or_else(|| self.fresh_id());
        if let Some(early) = early.as_ref().filter(|_| reused.is_some()) {
            trail.opened = Some(early.at);
        }
        let result = self.decide(plan, id, started, trail);
        self.fill(trail, id);
        trail.waited = Some(started.elapsed());
        match result {
            Ok(()) => {
                self.prepared = Some(Ready { id, screen: plan.screen.id.clone(), size: plan.size, started });
                Some(id)
            }
            Err(error) => {
                self.remember_size(&plan.screen.id, plan.size, &error);
                let opened = reused.is_some() || trail.opened.is_some();
                self.give_up(opened.then_some(id), &error);
                trail.skip(reason_of(&error), &format!("{error:#}"));
                None
            }
        }
    }

    fn fill(&mut self, trail: &mut Move, id: u32) {
        trail.parked = self.parked.is_some();
        trail.helper_gpu = self.parked.as_ref().map(|adapter| adapter.name.clone());
        if let Some((_, _, at, drag)) = self.primes.iter().find(|(screen, _, _, _)| *screen == trail.screen) {
            trail.primed_sent = Some(*at);
            trail.dragged = Some(*drag);
        }
        if let Some(link) = self.link.as_mut() {
            trail.marks = link.milestones();
            if trail.drawn.is_none() {
                trail.drawn = link.drawn(id);
            }
        }
    }

    fn decide(&mut self, plan: &Plan, id: u32, started: Instant, trail: &mut Move) -> Result<()> {
        let deadline = hold_until(started, self.woke);
        trail.allowed = Some(deadline.saturating_duration_since(started));
        self.send_warm(plan.picture, plan.size, plan.canvas, plan.key)?;
        let link = self.link.as_mut().ok_or_else(|| anyhow!("the helper is not running"))?;
        if trail.opened.is_none() {
            link.open(id, plan.screen, &plan.stills)?;
            trail.opened = Some(Instant::now());
        }
        link.ready(Self::left(deadline)).map_err(|error| if format!("{error:#}").contains("did not answer within") { anyhow!("the picture was not loaded within {} ms of the plain swap being ready, so this move swaps plainly", deadline.saturating_duration_since(started).as_millis()) } else { error })?;
        let shown = link.shown(id, Self::left(deadline)).map_err(|error| if format!("{error:#}").contains("did not answer within") { anyhow!("the layer was not shown within {} ms of the plain swap being ready, so this move swaps plainly", deadline.saturating_duration_since(started).as_millis()) } else { error })?;
        trail.shown = Some(shown);
        trail.drawn = link.drawn(id);
        Ok(())
    }

    pub fn go(&mut self, id: u32, from: &Path, to: &Path, trail: Move) -> Result<(), Box<Move>> {
        let mut trail = trail;
        let Some(prepared) = self.prepared.take().filter(|prepared| prepared.id == id) else { return Err(Box::new(trail)) };
        let result = match self.link.as_mut() {
            Some(link) => link.glide(id, from, to, GLIDE_MS),
            None => Err(anyhow!("the helper is gone")),
        };
        match result {
            Ok(()) => {
                trail.glide = Some(Instant::now());
                trail.slid("");
                note(&mut self.said, format!("open {id} shown and gliding after {} ms", prepared.started.elapsed().as_millis()));
                self.flights.push(Flight { id, screen: prepared.screen, size: prepared.size, settled: false, heard: false, set: None, trail });
                Ok(())
            }
            Err(error) => {
                self.give_up(Some(id), &error);
                trail.skip(reason_of(&error), &format!("{error:#}"));
                Err(Box::new(trail))
            }
        }
    }

    pub fn cross(&mut self, plan: &Plan, trail: &mut Move) -> Option<u32> {
        if let Some((reason, detail)) = self.refused(plan) {
            trail.skip(reason, &format!("{detail}, the picture changed"));
            return None;
        }
        let started = Instant::now();
        let id = self.fresh_id();
        let deadline = hold_until(started, self.woke);
        trail.allowed = Some(deadline.saturating_duration_since(started));
        let result = (|| -> Result<Instant> {
            let link = self.link.as_mut().ok_or_else(|| anyhow!("the helper is not running"))?;
            link.open(id, plan.screen, &plan.stills)?;
            trail.opened = Some(Instant::now());
            link.shown(id, Self::left(deadline)).map_err(|error| if format!("{error:#}").contains("did not answer within") { anyhow!("the layer was not shown within {} ms of the plain swap being ready, so the new picture swaps plainly", deadline.saturating_duration_since(started).as_millis()) } else { error })
        })();
        trail.waited = Some(started.elapsed());
        self.fill(trail, id);
        match result {
            Ok(shown) => {
                trail.shown = Some(shown);
                trail.slid("crossfaded, the picture changed");
                self.flights.push(Flight { id, screen: plan.screen.id.clone(), size: plan.size, settled: false, heard: false, set: None, trail: trail.clone() });
                Some(id)
            }
            Err(error) => {
                self.give_up(Some(id), &error);
                trail.skip(reason_of(&error), &format!("{error:#}, the picture changed"));
                None
            }
        }
    }

    pub fn settle(&mut self, id: u32, still: &Path, set: Instant, ms: Option<u32>) {
        if let Some(flight) = self.flights.iter_mut().find(|flight| flight.id == id) {
            flight.set = Some(set);
            flight.trail.set = Some(set);
        }
        let result = match self.link.as_mut() {
            Some(link) => link.settle(id, still, ms),
            None => Err(anyhow!("the helper is gone")),
        };
        match result {
            Ok(()) => note(&mut self.said, format!("settle {id} on {}", still.display())),
            Err(error) => self.give_up(Some(id), &error),
        }
    }

    pub fn settle_back(&mut self, id: u32, still: &Path, why: &str) {
        if let Some(flight) = self.flights.iter_mut().find(|flight| flight.id == id) {
            flight.trail.slid(&format!("settled back on the picture on screen because {why}"));
        }
        let result = match self.link.as_mut() {
            Some(link) => link.settle(id, still, None),
            None => Err(anyhow!("the helper is gone")),
        };
        match result {
            Ok(()) => keep(&mut self.said, format!("settle {id} back on {} because {why}", still.display())),
            Err(error) => self.give_up(Some(id), &error),
        }
    }

    pub fn abort(&mut self, id: u32, why: &str) {
        if self.prepared.as_ref().is_some_and(|prepared| prepared.id == id) {
            self.prepared = None;
        }
        if let Some(link) = self.link.as_mut() {
            link.close(id);
        }
        if let Some(index) = self.flights.iter().position(|flight| flight.id == id) {
            let flight = self.flights.remove(index);
            let mut trail = flight.trail;
            trail.also(&format!("then closed early, {why}"));
            let line = trail.write();
            keep(&mut self.said, line);
        }
        note(&mut self.said, format!("closed {id} early"));
    }

    pub fn write(&mut self, trail: &Move) {
        let line = trail.write();
        keep(&mut self.said, line);
    }

    pub fn in_flight(&self) -> bool {
        !self.flights.is_empty()
    }

    pub fn screens(&self) -> Vec<String> {
        self.flights.iter().map(|flight| flight.screen.clone()).collect()
    }

    fn give_up(&mut self, opened: Option<u32>, error: &anyhow::Error) {
        let reason = reason_of(error);
        if let Some(link) = self.link.as_mut() {
            if let Some(id) = opened.filter(|id| !self.flights.iter().any(|flight| flight.id == *id)) {
                link.close(id);
            }
        }
        keep(&mut self.said, format!("skipped, {reason}, {error:#}"));
        if !normal(reason) {
            self.fault(reason);
        }
    }

    fn fault(&mut self, reason: &str) {
        if !self.link.as_mut().is_some_and(|link| link.alive()) {
            self.reap();
            return;
        }
        if reason != "helper failed" && reason != "settle budget" {
            return;
        }
        let early = std::mem::take(&mut self.early);
        if let Some(link) = self.link.as_mut() {
            for unused in early {
                link.close(unused.id);
            }
        }
        if self.flights.iter().any(|flight| flight.set.is_some()) {
            self.doomed = true;
            return;
        }
        for flight in std::mem::take(&mut self.flights) {
            if let Some(link) = self.link.as_mut() {
                link.close(flight.id);
            }
            let mut trail = flight.trail;
            trail.also("then the helper stopped answering");
            let line = trail.write();
            keep(&mut self.said, line);
        }
        self.replace(reason);
    }

    fn replace(&mut self, reason: &str) {
        let replaced = self.replaced;
        self.stop();
        if replaced {
            self.given_up = true;
            tell(&mut self.said, format!("the helper stopped answering again, {reason}, so Scooch keeps the plain swap until the glide is turned off and on or Scooch starts again"));
        } else {
            self.replaced = true;
            tell(&mut self.said, format!("the helper stopped answering, {reason}, so a fresh one parks"));
            self.keep_parked();
        }
    }

    pub fn land(&mut self, hold: Duration, interrupted: &dyn Fn() -> bool) {
        if self.flights.is_empty() {
            return;
        }
        let started = Instant::now();
        let mut deadline = started + SETTLE_WAIT;
        while self.flights.iter().any(|flight| !flight.settled) {
            if interrupted() {
                let ids: Vec<u32> = self.flights.iter().map(|flight| flight.id).collect();
                for id in ids {
                    self.abort(id, "the picture or the setting changed during the glide");
                }
                note(&mut self.said, "interrupted, closed early".into());
                return;
            }
            if Instant::now() >= deadline {
                for flight in self.flights.iter_mut().filter(|flight| !flight.settled) {
                    flight.settled = true;
                    flight.trail.also(&format!("then the helper did not settle within {} ms", SETTLE_WAIT.as_millis()));
                }
                self.give_up(None, &anyhow!("the helper did not settle within {} ms", SETTLE_WAIT.as_millis()));
                break;
            }
            let Some(link) = self.link.as_mut() else {
                for flight in std::mem::take(&mut self.flights) {
                    let mut trail = flight.trail;
                    trail.also("then the helper was gone");
                    let line = trail.write();
                    keep(&mut self.said, line);
                }
                self.reap();
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
                    deadline = deadline.max(Instant::now() + SETTLE_AFTER_GLIDE);
                    if let Some(flight) = self.flights.iter_mut().find(|flight| flight.id == id) {
                        flight.trail.first = flight.trail.glide.map(|sent| sent + Duration::from_millis(first));
                        let size = flight.size;
                        note(&mut self.said, format!("glided {id} with {frames} frames, {slow} slow, at scale {scale}, first frame {first} ms after the glide was sent"));
                        self.judge(frames, slow);
                        self.replaced = false;
                        if let Some(adapter) = self.adapter.clone() {
                            self.learned.push((scale_key(&adapter, size), scale));
                        }
                    }
                }
                Ok(Heard::Failed(Some(id), why)) if self.flights.iter().any(|flight| flight.id == id) => {
                    if let Some(flight) = self.flights.iter_mut().find(|flight| flight.id == id) {
                        flight.settled = true;
                        if flight.trail.first.is_none() {
                            flight.trail.verdict = None;
                            flight.trail.skip("helper failed", &format!("the helper failed before the first moving frame, {why}"));
                        } else {
                            flight.trail.also(&format!("then the helper failed, {why}"));
                        }
                    }
                    keep(&mut self.said, format!("skipped, helper failed, {why}"));
                }
                Ok(Heard::Failed(None, why)) => self.give_up(None, &anyhow!(why)),
                Ok(_) => {}
                Err(error) => {
                    for flight in self.flights.iter_mut() {
                        flight.settled = true;
                        flight.trail.also(&format!("then {error:#}"));
                    }
                    self.give_up(None, &error);
                }
            }
        }
        let words = if !self.flights.is_empty() && self.flights.iter().all(|flight| flight.heard) { "settled after" } else { "gave up the settle after" };
        note(&mut self.said, format!("{words} {} ms", started.elapsed().as_millis()));
        let mut due: Vec<(Instant, u32, Move)> = self.flights.drain(..).map(|flight| (flight.set.map_or_else(Instant::now, |set| set + hold), flight.id, flight.trail)).collect();
        due.sort_by_key(|(when, id, _)| (*when, *id));
        for (when, id, trail) in due {
            while Instant::now() < when && !interrupted() {
                std::thread::sleep(CHECK.min(Self::left(when)));
            }
            if let Some(link) = self.link.as_mut() {
                link.close(id);
            }
            note(&mut self.said, format!("closed {id} {} ms after its wallpaper was set", hold.as_millis()));
            let line = trail.write();
            keep(&mut self.said, line);
        }
        if self.link.as_mut().is_some_and(|link| !link.alive()) {
            self.reap();
        } else if std::mem::take(&mut self.doomed) {
            self.replace("the settle failed");
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
    fn the_deadline_is_150_ms_after_the_tables_or_450_ms_after_the_wake_whichever_is_later() {
        let woke = Instant::now();
        assert_eq!(hold_until(after(woke, 200), Some(woke)), after(woke, 450));
        assert_eq!(hold_until(after(woke, 400), Some(woke)), after(woke, 550));
        assert_eq!(hold_until(after(woke, 200), None), after(woke, 350));
    }

    fn lone_screen() -> Screen {
        Screen { id: "main".into(), frame: scooch_core::Rect { x: 0.0, y: 0.0, w: 160.0, h: 100.0 }, scale: 1.0 }
    }

    fn plan(screen: &Screen) -> Plan<'_> {
        Plan { screen, picture: Path::new("p.jpg"), canvas: None, key: 1, size: [160, 100], stills: Stills::of(Path::new("s.png")) }
    }

    pub fn parked(spawner: FakeSpawner) -> Glider {
        let mut glider = Glider::new(Box::new(spawner));
        glider.keep_parked();
        let started = Instant::now();
        while !glider.parked() && started.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(2));
            glider.keep_parked();
        }
        glider
    }

    fn said(log: &Events) -> Vec<String> {
        log.lock().unwrap().iter().map(|(what, _)| what.clone()).collect()
    }

    #[test]
    fn a_parked_helper_shown_inside_the_deadline_glides_and_the_line_names_every_phase() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        spawner.script.lock().unwrap().shown_after = Duration::from_millis(30);
        let mut glider = parked(spawner);
        let screen = lone_screen();
        let mut trail = Move::new("main", Some(Instant::now()));
        let id = glider.prepare(&plan(&screen), &mut trail);
        assert!(id.is_some(), "{:?} {:?}", glider.said, trail.verdict);
        let line = trail.line();
        for part in ["hello ", "device ", "canvas ", "still ", "ready ", "opened ", "shown ", "first moving frame ", "screen on Fake screen GPU", "drawn on Fake GPU", "helper parked"] {
            assert!(line.contains(part), "{part} missing from {line}");
        }
    }

    #[test]
    fn a_helper_that_has_not_parked_swaps_plainly_at_once_and_waits_for_nothing() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        let waits = spawner.waits.clone();
        spawner.script.lock().unwrap().slow_hello = Duration::from_secs(5);
        let mut glider = Glider::new(Box::new(spawner));
        glider.keep_parked();
        let screen = lone_screen();
        let mut trail = Move::new("main", Some(Instant::now()));
        let started = Instant::now();
        assert!(glider.prepare(&plan(&screen), &mut trail).is_none());
        assert!(started.elapsed() < Duration::from_millis(400), "{:?}", started.elapsed());
        assert_eq!(trail.reason(), Some("helper cold"), "{:?}", trail.verdict);
        assert!(waits.lock().unwrap().iter().all(|(what, _)| *what == "spawn"), "{:?}", waits.lock().unwrap());
        let mut none = Glider::new(Box::new(FakeSpawner::new(Arc::new(Mutex::new(Vec::new())))));
        let mut trail = Move::new("main", None);
        assert!(none.prepare(&plan(&screen), &mut trail).is_none());
        assert!(trail.verdict.as_ref().is_some_and(|(_, detail)| detail.contains("not running")), "{:?}", trail.verdict);
    }

    #[test]
    fn a_late_parked_helper_misses_at_the_deadline_and_is_kept() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        let (script, spawned) = (spawner.script.clone(), spawner.spawned.clone());
        script.lock().unwrap().shown_after = Duration::from_millis(2000);
        let mut glider = parked(spawner);
        let screen = lone_screen();
        for _ in 0..4 {
            let mut trail = Move::new("main", None);
            let started = Instant::now();
            assert!(glider.prepare(&plan(&screen), &mut trail).is_none());
            let took = started.elapsed();
            assert!(took >= PATIENCE - Duration::from_millis(10) && took < PATIENCE + Duration::from_millis(900), "{took:?}");
            assert_eq!(trail.reason(), Some("helper budget"), "{:?}", trail.verdict);
        }
        assert!(glider.parked() && glider.running(), "a late helper was dropped");
        assert_eq!(*spawned.lock().unwrap(), 1, "a late helper was replaced");
        script.lock().unwrap().shown_after = Duration::from_millis(20);
        let mut trail = Move::new("main", None);
        assert!(glider.prepare(&plan(&screen), &mut trail).is_some(), "{:?}", trail.verdict);
    }

    #[test]
    fn a_helper_that_dies_is_replaced_once_and_a_second_death_gives_the_helper_up() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        let (script, spawned, log) = (spawner.script.clone(), spawner.spawned.clone(), spawner.log.clone());
        let mut glider = parked(spawner);
        script.lock().unwrap().dies = true;
        glider.keep_parked();
        assert_eq!(*spawned.lock().unwrap(), 2, "{:?}", said(&log));
        glider.keep_parked();
        glider.keep_parked();
        assert_eq!(*spawned.lock().unwrap(), 2, "a second death was replaced");
        let mut trail = Move::new("main", None);
        assert!(glider.prepare(&plan(&lone_screen()), &mut trail).is_none());
        assert_eq!(trail.reason(), Some("helper cold"));
        glider.stop();
        script.lock().unwrap().dies = false;
        glider.keep_parked();
        assert_eq!(*spawned.lock().unwrap(), 3, "turning the glide off and on did not start a helper");
    }

    #[test]
    fn a_software_adapter_parks_nothing_and_skips_unless_forced() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        spawner.script.lock().unwrap().kind = "cpu".into();
        let log = spawner.log.clone();
        let mut glider = parked(spawner);
        assert!(!glider.running() && !glider.usable(), "{:?}", glider.said);
        assert!(said(&log).iter().any(|what| what == "quit"), "{:?}", said(&log));
        let mut trail = Move::new("main", None);
        assert!(glider.prepare(&plan(&lone_screen()), &mut trail).is_none());
        assert_eq!(trail.reason(), Some("software adapter"));
    }

    #[test]
    fn a_drag_always_primes_and_a_bare_wake_primes_once_in_ten_seconds_and_not_after_two_bare_wakes() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        let log = spawner.log.clone();
        let mut glider = parked(spawner);
        let screen = lone_screen();
        let warm = Warmup { picture: PathBuf::from("p.jpg"), size: [160, 100], canvas: None, key: 1 };
        let wake = |still: &str| Prime { screen: &screen, warm: &warm, stills: Stills::of(Path::new(still)), drag: false };
        assert!(glider.prime(&wake("a.png")));
        assert!(!glider.prime(&wake("a.png")), "a live prime was sent again");
        glider.after_snapshot(false);
        assert!(!glider.prime(&wake("b.png")), "a second wake inside ten seconds primed");
        assert!(glider.prime(&Prime { drag: true, ..wake("b.png") }), "a drag did not prime");
        glider.last_wake_prime = None;
        assert!(glider.prime(&wake("c.png")));
        glider.after_snapshot(false);
        glider.last_wake_prime = None;
        assert!(!glider.prime(&wake("d.png")), "a wake primed after two bare wakes");
        glider.after_snapshot(true);
        assert!(glider.prime(&wake("d.png")), "a move did not lift the bare wake rule");
        let primes = said(&log).iter().filter(|what| what.starts_with("prime ")).count();
        let warms = said(&log).iter().filter(|what| what.starts_with("warm ")).count();
        assert_eq!((primes, warms), (4, 1), "{:?}", said(&log));
    }

    #[test]
    fn a_picture_rewritten_at_the_same_path_is_warmed_again() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        let log = spawner.log.clone();
        let mut glider = parked(spawner);
        let screen = lone_screen();
        let before = Warmup { picture: PathBuf::from("p.jpg"), size: [160, 100], canvas: None, key: 1 };
        let after = Warmup { key: 2, ..Warmup { picture: PathBuf::from("p.jpg"), size: [160, 100], canvas: None, key: 1 } };
        assert!(glider.prime(&Prime { screen: &screen, warm: &before, stills: Stills::of(Path::new("a.png")), drag: true }));
        assert!(glider.prime(&Prime { screen: &screen, warm: &after, stills: Stills::of(Path::new("b.png")), drag: true }));
        let warms = said(&log).iter().filter(|what| what.starts_with("warm ")).count();
        assert_eq!(warms, 2, "a picture rewritten with a new key was not warmed again {:?}", said(&log));
    }

    #[test]
    fn a_helper_quiet_long_enough_to_let_its_picture_go_is_warmed_again() {
        let spawner = FakeSpawner::new(Arc::new(Mutex::new(Vec::new())));
        let (log, script) = (spawner.log.clone(), spawner.script.clone());
        let mut glider = parked(spawner);
        let screen = lone_screen();
        let warm = Warmup { picture: PathBuf::from("p.jpg"), size: [160, 100], canvas: None, key: 1 };
        let drag = |still: &str| Prime { screen: &screen, warm: &warm, stills: Stills::of(Path::new(still)), drag: true };
        let warms = |log: &Events| said(log).iter().filter(|what| what.starts_with("warm ")).count();
        assert!(glider.prime(&drag("a.png")));
        script.lock().unwrap().quiet = FORGOTTEN - Duration::from_secs(1);
        assert!(glider.prime(&drag("b.png")));
        assert_eq!(warms(&log), 1, "a helper that still holds its picture was warmed again {:?}", said(&log));
        script.lock().unwrap().quiet = crate::live::helper::IDLE_EXIT;
        assert!(glider.prime(&drag("c.png")));
        assert_eq!(warms(&log), 2, "a helper that let its picture go was not warmed again {:?}", said(&log));
    }

    #[test]
    fn the_line_counts_times_from_the_drop_and_says_before_and_after() {
        let drop = Instant::now() + Duration::from_millis(300);
        let mut trail = Move::new("left", Some(drop));
        trail.marks.canvas = Some(drop - Duration::from_millis(120));
        trail.shown = Some(drop + Duration::from_millis(40));
        trail.skip("helper budget", "the layer was not shown within 150 ms of the plain swap being ready, so this move swaps plainly");
        let line = trail.line();
        assert!(line.starts_with("glide skipped, helper budget, the layer was not shown"), "{line}");
        assert!(line.contains("canvas -120 ms") && line.contains("shown 40 ms") && line.contains("glide sent none") && line.contains("helper not parked"), "{line}");
        let mut slid = Move::new("left", None);
        slid.slid("");
        assert!(slid.line().starts_with("glide slid, on left, times from the tick"), "{}", slid.line());
        assert!(!line.contains(['\u{2014}', '\u{2013}', ';']), "{line}");
    }

    #[test]
    fn a_failure_goes_only_to_its_own_layer_or_its_own_warm() {
        let warm = |id: u32, why: &str| (None, format!("warm {id}: {why}"));
        let mut failed = vec![warm(3, "the picture is too large for this GPU"), (Some(7), "layer 7 failed".to_string())];
        assert_eq!(take_failure(&mut failed, 8), None, "a leftover warm failure was blamed on layer 8");
        assert_eq!(take_failure(&mut failed, 7).as_deref(), Some("layer 7 failed"));
        assert_eq!(take_heard(&mut failed), None, "a glide heard a warm failure");
        assert_eq!(take_warm_failure(&mut failed, Some(4)), None, "warm 4 took the failure of warm 3");
        assert_eq!(take_warm_failure(&mut failed, Some(3)).as_deref(), Some("warm 3: the picture is too large for this GPU"));
        let mut broken = vec![(None, "unreadable line from the helper".to_string())];
        assert!(take_heard(&mut broken).is_some(), "a failure of the whole helper was not heard");
    }

    #[test]
    fn the_daemon_is_woken_when_its_helper_parks_fails_or_exits_and_for_nothing_else() {
        let ready = |warm_id: Option<u32>| Reply::Ready { adapter: "gpu".into(), ms: 1, uploads: 0, kind: "integrated".into(), warm_id, from: None, scale: None };
        assert!(wakes(None));
        assert!(wakes(Some(&ready(Some(PARK_WARM)))));
        assert!(wakes(Some(&Reply::Failed { id: None, why: "warm 1: no device".into() })));
        assert!(!wakes(Some(&ready(Some(2)))));
        assert!(!wakes(Some(&Reply::Shown { id: 1, ms: 3 })));
        assert!(!wakes(Some(&Reply::Failed { id: Some(4), why: "layer 4 is not open".into() })));
    }

    #[test]
    fn every_move_writes_its_own_line_and_repeats_are_never_folded() {
        let mut glider = Glider::new(Box::new(FakeSpawner::new(Arc::new(Mutex::new(Vec::new())))));
        for _ in 0..3 {
            let mut trail = Move::new("main", None);
            trail.skip("helper cold", "the helper is not running yet, so this move swaps plainly");
            glider.write(&trail);
        }
        assert_eq!(glider.said.iter().filter(|line| line.starts_with("skipped, helper cold, the helper is not running yet")).count(), 3, "{:?}", glider.said);
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
        pub dies: bool,
        pub settle_after: Duration,
        pub glided_after: Duration,
        pub quiet: Duration,
        pub glide_fails: bool,
        pub slow_hello: Duration,
        pub shown_after: Duration,
        pub ready_after: Duration,
    }

    pub type Waits = Arc<Mutex<Vec<(&'static str, Duration)>>>;
    pub type Events = Arc<Mutex<Vec<(String, Instant)>>>;

    pub struct FakeHelper {
        born: Instant,
        log: Events,
        waits: Waits,
        script: Arc<Mutex<Script>>,
        settles: Vec<(u32, Instant)>,
        glides: Vec<(u32, Instant)>,
        opened: Vec<(u32, Instant)>,
        warmed: Option<Instant>,
        parking: bool,
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
        fn spawn(&self, _nudge: Option<Nudge>) -> Result<Box<dyn Link>> {
            self.waits.lock().unwrap().push(("spawn", Duration::ZERO));
            if self.script.lock().unwrap().spawn_fails {
                return Err(anyhow!("the test helper could not start"));
            }
            *self.spawned.lock().unwrap() += 1;
            self.log.lock().unwrap().push(("spawn".into(), Instant::now()));
            Ok(Box::new(FakeHelper { born: Instant::now(), log: self.log.clone(), waits: self.waits.clone(), script: self.script.clone(), settles: Vec::new(), glides: Vec::new(), opened: Vec::new(), warmed: None, parking: false, dead: false }))
        }
    }

    impl FakeHelper {
        fn say(&self, what: String) {
            self.log.lock().unwrap().push((what, Instant::now()));
        }

        fn script(&self) -> Script {
            self.script.lock().unwrap().clone()
        }

        fn living(&mut self) -> Result<()> {
            if self.script().dies {
                self.dead = true;
            }
            if self.dead {
                return Err(anyhow!("the helper is gone"));
            }
            Ok(())
        }

        fn until(&self, at: Instant, timeout: Duration) -> Result<()> {
            let left = at.saturating_duration_since(Instant::now());
            if left > timeout {
                std::thread::sleep(timeout);
                return Err(anyhow!("the helper did not answer within {} ms", timeout.as_millis()));
            }
            std::thread::sleep(left);
            Ok(())
        }

        fn adapter(&self) -> Adapter {
            let script = self.script();
            Adapter { name: if script.adapter.is_empty() { "Fake GPU".into() } else { script.adapter }, kind: script.kind, from: Some("cache".into()), scale: Some(1.0), after: Some(script.slow_hello) }
        }
    }

    impl Link for FakeHelper {
        fn park(&mut self) -> Result<()> {
            self.living()?;
            self.parking = true;
            self.say("park".into());
            Ok(())
        }

        fn parked(&mut self) -> Option<Result<Adapter, String>> {
            if self.living().is_err() || !self.parking || self.born.elapsed() < self.script().slow_hello {
                return None;
            }
            if self.script().ready_fails {
                return Some(Err("warm 1: no device".into()));
            }
            Some(Ok(self.adapter()))
        }

        fn warm(&mut self, _picture: &Path, size: [u32; 2], canvas: Option<&Path>, _key: u64, scales: &BTreeMap<String, f32>) -> Result<()> {
            self.living()?;
            self.warmed = Some(Instant::now());
            self.say(format!("warm {}x{}{}{}", size[0], size[1], if canvas.is_some() { " canvas" } else { "" }, scales.iter().map(|(adapter, scale)| format!(" {adapter}={scale}")).collect::<String>()));
            Ok(())
        }

        fn ready(&mut self, timeout: Duration) -> Result<Adapter> {
            self.living()?;
            self.waits.lock().unwrap().push(("ready", timeout));
            let at = self.warmed.unwrap_or(self.born) + self.script().ready_after;
            self.until(at, timeout)?;
            if self.script().ready_fails {
                return Err(anyhow!("no device"));
            }
            Ok(self.adapter())
        }

        fn prime(&mut self, screen: &Screen, stills: &Stills) -> Result<()> {
            self.living()?;
            self.say(format!("prime {} {}", screen.id, stills.still.as_deref().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
            Ok(())
        }

        fn open(&mut self, id: u32, _screen: &Screen, stills: &Stills) -> Result<()> {
            self.living()?;
            self.opened.retain(|(opened, _)| *opened != id);
            self.opened.push((id, Instant::now()));
            self.say(format!("open {id} {}", stills.still.as_deref().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
            Ok(())
        }

        fn drawn(&mut self, _id: u32) -> Option<Split> {
            Some(Split { window: 1, first: 1, reveal: 1, still: Some("raw".into()), screen_gpu: Some("Fake screen GPU".into()), ..Split::default() })
        }

        fn shown(&mut self, id: u32, timeout: Duration) -> Result<Instant> {
            self.living()?;
            self.waits.lock().unwrap().push(("shown", timeout));
            let opened = self.opened.iter().find(|(opened, _)| *opened == id).map(|(_, at)| *at).unwrap_or(self.born);
            self.until(opened + self.script().shown_after, timeout)?;
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
            let due = Instant::now() + self.script().glided_after;
            self.glides.push((id, due));
            if self.script().dies_after_glide {
                self.dead = true;
            }
            Ok(())
        }

        fn settle(&mut self, id: u32, still: &Path, ms: Option<u32>) -> Result<()> {
            self.living()?;
            self.say(format!("settle {id} {}{}", still.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(), ms.map(|ms| format!(" over {ms} ms")).unwrap_or_default()));
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
            if script.glide_fails && !self.glides.is_empty() {
                let (id, _) = self.glides.remove(0);
                return Ok(Heard::Failed(Some(id), "glide needs a warm with a picture first".into()));
            }
            if let (Some((frames, slow)), Some(index)) = (script.glided, self.glides.iter().position(|(_, due)| *due <= Instant::now())) {
                let (id, _) = self.glides.remove(index);
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

        fn quiet(&self) -> Duration {
            self.script().quiet
        }

        fn alive(&mut self) -> bool {
            if self.script().dies {
                self.dead = true;
            }
            !self.dead
        }

        fn milestones(&mut self) -> Milestones {
            let parked = self.born + self.script().slow_hello;
            let ready = self.warmed.map(|warmed| warmed + self.script().ready_after);
            Milestones { launched: Some(self.born), hello: Some(self.born), device: Some(parked), canvas: ready, ready, primed: None }
        }
    }
}
