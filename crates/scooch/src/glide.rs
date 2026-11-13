use crate::live::{self, Helper, Hints, Reply};
use crate::platform::Screen;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const FORCE: &str = "SCOOCH_GLIDE_ANYWAY";
pub const BUDGET: Duration = Duration::from_millis(400);
const SOFTWARE: [&str; 3] = ["basic render driver", "llvmpipe", "swiftshader"];
const STRIKES: u32 = 2;
pub const GLIDE_MS: u32 = 450;
pub const SETTLE_WAIT: Duration = Duration::from_secs(2);
pub const CHECK: Duration = Duration::from_millis(50);
const SPAWN_WAIT: Duration = Duration::from_millis(350);
const CLOSE_WAIT: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Glided { id: u32, frames: u32, slow: u32 },
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
        Ok(Box::new(Real(Helper::spawn(program, timeout)?)))
    }
}

pub fn program() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    if cfg!(windows) {
        return Some(me.with_file_name("scoochw.exe"));
    }
    Some(me)
}

struct Real(Helper);

impl Link for Real {
    fn warm(&mut self, picture: &Path, size: [u32; 2]) -> Result<()> {
        self.0.send(&live::Command::Warm { hints: Hints::default(), picture: Some(picture.to_path_buf()), size: Some(size) })
    }

    fn ready(&mut self, timeout: Duration) -> Result<Adapter> {
        self.0.expect(timeout, |reply| match reply {
            Reply::Ready { adapter, kind, .. } => Some(Ok(Adapter { name: adapter.clone(), kind: kind.clone() })),
            Reply::Failed { id: None, why } => Some(Err(anyhow!("{why}"))),
            _ => None,
        })
    }

    fn open(&mut self, id: u32, screen: &Screen, still: &Path) -> Result<()> {
        self.0.ask_open(id, screen, still)
    }

    fn shown(&mut self, id: u32, timeout: Duration) -> Result<()> {
        self.0.shown(id, timeout).map(drop)
    }

    fn glide(&mut self, id: u32, a: &Path, b: &Path, ms: u32) -> Result<()> {
        self.0.ask_glide(id, a, b, ms)
    }

    fn settle(&mut self, id: u32, still: &Path) -> Result<()> {
        self.0.ask_settle(id, still, None)
    }

    fn hear(&mut self, timeout: Duration) -> Result<Heard> {
        let started = Instant::now();
        match self.0.next(timeout) {
            Some(Reply::Settled { id }) => Ok(Heard::Settled(id)),
            Some(Reply::Glided { id, frames, slow_frames, .. }) => Ok(Heard::Glided { id, frames, slow: slow_frames }),
            Some(Reply::Failed { id, why }) => Ok(Heard::Failed(id, why)),
            Some(_) => Ok(Heard::Nothing),
            None if self.0.exited() || started.elapsed() < timeout / 2 => Err(anyhow!("the helper is gone")),
            None => Ok(Heard::Nothing),
        }
    }

    fn close(&mut self, id: u32) {
        let _ = self.0.close(id, CLOSE_WAIT);
    }

    fn alive(&mut self) -> bool {
        !self.0.exited()
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
    flights: Vec<Flight>,
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
        Self { forced, software: false, strikes: 0, stalled: false, spawner, link: None, warmed: None, ready: false, next: 0, flights: Vec::new(), said: Vec::new() }
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

    pub fn begin(&mut self, plan: &Plan) -> Option<u32> {
        if !self.usable() {
            skipped(if self.software { "software adapter" } else { "strikes" }, "");
            return None;
        }
        let started = Instant::now();
        let deadline = started + BUDGET;
        self.next = self.next.wrapping_add(1).max(1);
        let id = self.next;
        let mut opened = false;
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
            link.open(id, plan.screen, plan.still)?;
            opened = true;
            link.shown(id, Self::left(deadline))?;
            link.glide(id, plan.from, plan.to, GLIDE_MS)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                note(&mut self.said, format!("open {id} shown and gliding after {} ms", started.elapsed().as_millis()));
                self.flights.push(Flight { id, screen: plan.screen.id.clone(), settled: false, set: None });
                Some(id)
            }
            Err(error) => {
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
        if let Some(link) = self.link.as_mut() {
            for flight in &self.flights {
                link.close(flight.id);
            }
            if let Some(id) = opened.filter(|id| !self.flights.iter().any(|flight| flight.id == *id)) {
                link.close(id);
            }
        }
        self.flights.clear();
        self.drop_link();
        let reason = reason_of(error);
        let detail = format!("{error:#}");
        skipped(reason, &detail);
        keep(&mut self.said, format!("skipped, {reason}, {detail}"));
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
                return;
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
                Ok(Heard::Glided { id, frames, slow }) => {
                    if self.flights.iter().any(|flight| flight.id == id) {
                        note(&mut self.said, format!("glided {id} with {frames} frames, {slow} slow"));
                        self.judge(frames, slow);
                    }
                }
                Ok(Heard::Failed(Some(id), why)) if self.flights.iter().any(|flight| flight.id == id) => {
                    link.close(id);
                    self.flights.retain(|flight| flight.id != id);
                    skipped("helper failed", &why);
                    keep(&mut self.said, format!("skipped, helper failed, {why}"));
                }
                Ok(Heard::Failed(None, why)) => {
                    self.give_up(None, &anyhow!(why));
                    return;
                }
                Ok(_) => {}
                Err(error) => {
                    self.give_up(None, &error);
                    return;
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
            if self.script().shown_fails {
                self.say(format!("hidden {id}"));
                return Err(anyhow!("macOS never reported the live layer as visible, so Scooch keeps the plain swap."));
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
                return Ok(Heard::Glided { id, frames, slow });
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
