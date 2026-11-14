use crate::live::{self, Helper, Hints, Reply};
use crate::platform::Screen;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const FORCE: &str = "SCOOCH_GLIDE_ANYWAY";
pub const BUDGET: Duration = Duration::from_millis(400);
const SOFTWARE: [&str; 3] = ["basic render driver", "llvmpipe", "swiftshader"];
const STRIKES: u32 = 2;
const LATE: u32 = 3;
pub const GLIDE_MS: u32 = 450;
pub const SETTLE_WAIT: Duration = Duration::from_secs(2);
pub const CHECK: Duration = Duration::from_millis(50);
const SPAWN_WAIT: Duration = Duration::from_millis(350);
const CLOSE_WAIT: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Glided { id: u32, frames: u32, slow: u32, scale: f32 },
    Settled(u32),
    Failed(Option<u32>, String),
    Nothing,
}

pub trait Link: Send {
    fn warm(&mut self, picture: &Path, size: [u32; 2]) -> Result<()>;
    fn ready(&mut self, timeout: Duration) -> Result<Adapter>;
    fn open(&mut self, id: u32, screen: &Screen, still: &Path) -> Result<()>;
    fn shown(&mut self, id: u32, timeout: Duration) -> Result<()>;
    fn glide(&mut self, id: u32, a: &Path, b: &Path, ms: u32) -> Result<()>;
    fn settle(&mut self, id: u32, still: &Path) -> Result<()>;
    fn hear(&mut self, timeout: Duration) -> Result<Heard>;
    fn close(&mut self, id: u32);
    fn alive(&mut self) -> bool;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Adapter {
    pub name: String,
    pub kind: String,
}

pub trait Spawn: Send + Sync {
    fn spawn(&self, timeout: Duration) -> Result<Box<dyn Link>>;
}

pub struct Program(pub Option<PathBuf>);

impl Spawn for Program {
    fn spawn(&self, timeout: Duration) -> Result<Box<dyn Link>> {
        let program = self.0.as_deref().ok_or_else(|| anyhow!("Scooch could not find its own program"))?;
        Ok(Box::new(Real::new(Helper::spawn(program, timeout)?)))
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
    shown: Vec<u32>,
    closed: Vec<u32>,
    failed: Vec<(Option<u32>, String)>,
    heard: Vec<Heard>,
    readies: Vec<(Option<u32>, Adapter)>,
    warm_id: Option<u32>,
}

impl Real {
    fn new(helper: Helper) -> Self {
        Self { helper, shown: Vec::new(), closed: Vec::new(), failed: Vec::new(), heard: Vec::new(), readies: Vec::new(), warm_id: None }
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
            Reply::Shown { id, .. } => self.shown.push(id),
            Reply::Closed { id } => self.closed.push(id),
            Reply::Settled { id } => self.heard.push(Heard::Settled(id)),
            Reply::Glided { id, frames, slow_frames, scale, .. } => self.heard.push(Heard::Glided { id, frames, slow: slow_frames, scale }),
            Reply::Failed { id, why } => self.failed.push((id, why)),
            Reply::Ready { adapter, kind, warm_id, .. } => self.readies.push((warm_id, Adapter { name: adapter, kind })),
            Reply::Hello { .. } => {}
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
    fn warm(&mut self, picture: &Path, size: [u32; 2]) -> Result<()> {
        self.warm_id = Some(self.helper.ask_warm(Hints::default(), Some(picture), Some(size))?);
        Ok(())
    }

    fn ready(&mut self, timeout: Duration) -> Result<Adapter> {
        let wanted = self.warm_id;
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

    fn open(&mut self, id: u32, screen: &Screen, still: &Path) -> Result<()> {
        self.helper.ask_open(id, screen, still)
    }

    fn shown(&mut self, id: u32, timeout: Duration) -> Result<()> {
        self.wait(timeout, |me| {
            if let Some(index) = me.shown.iter().position(|shown| *shown == id) {
                me.shown.remove(index);
                return Some(Ok(()));
            }
            me.failure(Some(id)).map(|why| Err(anyhow!("{why}")))
        })
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
        if self.helper.send(&live::Command::Close { id }).is_err() {
            return;
        }
        let _ = self.wait(CLOSE_WAIT, |me| {
            if let Some(index) = me.closed.iter().position(|closed| *closed == id) {
                me.closed.remove(index);
                return Some(Ok(()));
            }
            let index = me.failed.iter().position(|(failed, _)| *failed == Some(id))?;
            me.failed.remove(index);
            Some(Ok(()))
        });
    }

    fn alive(&mut self) -> bool {
        !self.helper.exited()
    }
}

pub struct Plan<'a> {
    pub screen: &'a Screen,
    pub picture: &'a Path,
    pub size: [u32; 2],
    pub still: &'a Path,
    pub from: &'a Path,
    pub to: &'a Path,
}

struct Early {
    id: u32,
    screen: String,
    size: [u32; 2],
    still: PathBuf,
}

struct Flight {
    id: u32,
    screen: String,
    settled: bool,
    set: Option<Instant>,
}

pub fn software(adapter: &Adapter) -> bool {
    if !adapter.kind.is_empty() {
        return adapter.kind == "cpu";
    }
    let name = adapter.name.to_lowercase();
    SOFTWARE.iter().any(|known| name.contains(known))
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

fn reason_of(error: &anyhow::Error) -> &'static str {
    let text = format!("{error:#}");
    if text.contains("software renderer") {
        "software adapter"
    } else if text.contains("never reported the live layer as visible") {
        "hidden"
    } else if text.contains("no wallpaper window below") {
        "no room"
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
    ready: bool,
    next: u32,
    late: u32,
    flights: Vec<Flight>,
    early: Vec<Early>,
    doomed: bool,
    huge: std::collections::HashMap<String, [u32; 2]>,
    pub said: Vec<String>,
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
        Self { forced, software: false, strikes: 0, stalled: false, spawner, link: None, warmed: None, ready: false, next: 0, late: 0, flights: Vec::new(), early: Vec::new(), doomed: false, huge: std::collections::HashMap::new(), said: Vec::new() }
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
            self.drop_link();
        }
    }

    fn drop_link(&mut self) {
        self.link = None;
        self.warmed = None;
        self.ready = false;
    }

    fn connect(&mut self, wait: Duration) -> Result<&mut Box<dyn Link>> {
        if self.link.as_mut().is_some_and(|link| !link.alive()) {
            self.drop_link();
        }
        if self.link.is_none() {
            let started = Instant::now();
            let link = self.spawner.spawn(wait)?;
            note(&mut self.said, format!("helper started in {} ms", started.elapsed().as_millis()));
            self.link = Some(link);
            self.warmed = None;
            self.ready = false;
        }
        self.link.as_mut().ok_or_else(|| anyhow!("no helper"))
    }

    pub fn warm(&mut self, picture: &Path, size: [u32; 2]) {
        if !self.usable() {
            return;
        }
        if let Err(error) = self.send_warm(picture, size, SPAWN_WAIT) {
            self.give_up(None, &error);
        }
    }

    fn send_warm(&mut self, picture: &Path, size: [u32; 2], wait: Duration) -> Result<()> {
        let wanted = (picture.to_path_buf(), size);
        self.connect(wait)?;
        if self.warmed.as_ref() == Some(&wanted) {
            return Ok(());
        }
        self.link.as_mut().ok_or_else(|| anyhow!("no helper"))?.warm(picture, size)?;
        note(&mut self.said, format!("warm {} at {}x{}", picture.display(), size[0], size[1]));
        self.warmed = Some(wanted);
        self.ready = false;
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

    pub fn open_early(&mut self, screen: &Screen, picture: &Path, size: [u32; 2], still: &Path) {
        if !self.usable() || self.doomed || self.too_large(&screen.id, size) || self.early.iter().any(|early| early.screen == screen.id) {
            return;
        }
        let id = self.fresh_id();
        let result = (|| -> Result<()> {
            self.send_warm(picture, size, SPAWN_WAIT)?;
            self.link.as_mut().ok_or_else(|| anyhow!("no helper"))?.open(id, screen, still)
        })();
        match result {
            Ok(()) => {
                note(&mut self.said, format!("early open {id} on {}", still.display()));
                self.early.push(Early { id, screen: screen.id.clone(), size, still: still.to_path_buf() });
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
        let shown = link.shown(unused.id, BUDGET);
        link.close(unused.id);
        note(&mut self.said, format!("closed early open {} with nothing to glide", unused.id));
        if let Err(error) = shown {
            self.remember_size(&unused.screen, unused.size, &error);
        }
    }

    pub fn begin(&mut self, plan: &Plan) -> Option<u32> {
        if !self.usable() {
            skipped(if self.software { "software adapter" } else { "strikes" }, "");
            return None;
        }
        if self.too_large(&plan.screen.id, plan.size) || self.doomed {
            self.close_early_for(&plan.screen.id);
            skipped(if self.doomed { "helper failed" } else { "too large" }, "");
            return None;
        }
        let started = Instant::now();
        let deadline = started + BUDGET;
        let early = self.early.iter().position(|early| early.screen == plan.screen.id).map(|index| self.early.remove(index));
        let reused = early.as_ref().filter(|early| early.still == plan.still).map(|early| early.id);
        if let (Some(stale), None) = (early.as_ref(), reused) {
            if let Some(link) = self.link.as_mut() {
                link.close(stale.id);
            }
        }
        let id = reused.unwrap_or_else(|| self.fresh_id());
        let mut opened = reused.is_some();
        let result = (|| -> Result<()> {
            self.send_warm(plan.picture, plan.size, Self::left(deadline).min(SPAWN_WAIT))?;
            let link = self.link.as_mut().ok_or_else(|| anyhow!("no helper"))?;
            if !self.ready {
                let adapter = link.ready(Self::left(deadline))?;
                if !self.forced && software(&adapter) {
                    self.software = true;
                    return Err(anyhow!("the only GPU here is the software renderer {} of kind {}", adapter.name, adapter.kind));
                }
                note(&mut self.said, format!("ready on {} of kind {}", adapter.name, adapter.kind));
                self.ready = true;
            }
            if reused.is_none() {
                link.open(id, plan.screen, plan.still)?;
                opened = true;
            }
            link.shown(id, Self::left(deadline))?;
            link.glide(id, plan.from, plan.to, GLIDE_MS)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.late = 0;
                note(&mut self.said, format!("open {id} shown and gliding after {} ms{}", started.elapsed().as_millis(), if reused.is_some() { ", opened early" } else { "" }));
                self.flights.push(Flight { id, screen: plan.screen.id.clone(), settled: false, set: None });
                Some(id)
            }
            Err(error) => {
                self.remember_size(&plan.screen.id, plan.size, &error);
                self.give_up(opened.then_some(id), &error);
                None
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

    pub fn abort(&mut self, id: u32) {
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
        let normal = matches!(reason, "hidden" | "no room" | "too large") || (reason == "helper budget" && self.late < LATE);
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
            self.drop_link();
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
            self.drop_link();
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
                    }
                }
                Ok(Heard::Glided { id, frames, slow, scale }) => {
                    if self.flights.iter().any(|flight| flight.id == id) {
                        note(&mut self.said, format!("glided {id} with {frames} frames, {slow} slow, at scale {scale}"));
                        self.judge(frames, slow);
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
        note(&mut self.said, format!("settled after {} ms", started.elapsed().as_millis()));
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
        if self.doomed {
            self.doomed = false;
            self.drop_link();
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

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
    }

    pub type Waits = Arc<Mutex<Vec<(&'static str, Duration)>>>;
    pub type Events = Arc<Mutex<Vec<(String, Instant)>>>;

    pub struct FakeHelper {
        log: Events,
        waits: Waits,
        script: Arc<Mutex<Script>>,
        settles: Vec<(u32, Instant)>,
        glides: Vec<u32>,
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
        fn spawn(&self, timeout: Duration) -> Result<Box<dyn Link>> {
            self.waits.lock().unwrap().push(("spawn", timeout));
            if self.script.lock().unwrap().spawn_fails {
                return Err(anyhow!("the test helper could not start"));
            }
            *self.spawned.lock().unwrap() += 1;
            self.log.lock().unwrap().push(("spawn".into(), Instant::now()));
            Ok(Box::new(FakeHelper { log: self.log.clone(), waits: self.waits.clone(), script: self.script.clone(), settles: Vec::new(), glides: Vec::new(), dead: false }))
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
    }

    impl Link for FakeHelper {
        fn warm(&mut self, _picture: &Path, size: [u32; 2]) -> Result<()> {
            self.living()?;
            self.say(format!("warm {}x{}", size[0], size[1]));
            Ok(())
        }

        fn ready(&mut self, timeout: Duration) -> Result<Adapter> {
            self.living()?;
            self.waits.lock().unwrap().push(("ready", timeout));
            if self.script().ready_fails {
                return Err(anyhow!("no device"));
            }
            let script = self.script();
            Ok(Adapter { name: if script.adapter.is_empty() { "Fake GPU".into() } else { script.adapter }, kind: script.kind })
        }

        fn open(&mut self, id: u32, _screen: &Screen, still: &Path) -> Result<()> {
            self.living()?;
            self.say(format!("open {id} {}", still.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
            Ok(())
        }

        fn shown(&mut self, id: u32, timeout: Duration) -> Result<()> {
            self.living()?;
            self.waits.lock().unwrap().push(("shown", timeout));
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
            Ok(())
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
                return Ok(Heard::Glided { id, frames, slow, scale: 1.0 });
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

        fn alive(&mut self) -> bool {
            !self.dead
        }
    }
}
