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
    fn ready(&mut self, timeout: Duration) -> Result<String>;
    fn open(&mut self, id: u32, screen: &Screen, still: &Path) -> Result<()>;
    fn shown(&mut self, id: u32, timeout: Duration) -> Result<()>;
    fn glide(&mut self, id: u32, a: &Path, b: &Path, ms: u32) -> Result<()>;
    fn settle(&mut self, id: u32, still: &Path) -> Result<()>;
    fn hear(&mut self, timeout: Duration) -> Result<Heard>;
    fn close(&mut self, id: u32);
    fn alive(&mut self) -> bool;
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

    fn ready(&mut self, timeout: Duration) -> Result<String> {
        self.0.expect(timeout, |reply| match reply {
            Reply::Ready { adapter, .. } => Some(Ok(adapter.clone())),
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
}

pub fn software(adapter: &str) -> bool {
    let adapter = adapter.to_lowercase();
    SOFTWARE.iter().any(|name| adapter.contains(name))
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
                eprintln!("{line}");
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
                    return Err(anyhow!("the only GPU here is the software renderer {adapter}"));
                }
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
                self.flights.push(Flight { id, screen: plan.screen.id.clone(), settled: false });
                Some(id)
            }
            Err(error) => {
                self.give_up(opened.then_some(id), &error);
                None
            }
        }
    }

    pub fn settle(&mut self, id: u32, still: &Path) {
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
        let line = format!("Scooch swapped the picture without the glide because {error:#}.");
        eprintln!("{line}");
        keep(&mut self.said, line);
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
                    let line = format!("Scooch swapped the picture without the glide because {why}.");
                    eprintln!("{line}");
                    keep(&mut self.said, line);
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
        let until = Instant::now() + hold;
        while Instant::now() < until && !interrupted() {
            std::thread::sleep(CHECK.min(Self::left(until)));
        }
        let ids: Vec<u32> = self.flights.drain(..).map(|flight| flight.id).collect();
        if let Some(link) = self.link.as_mut() {
            for id in &ids {
                link.close(*id);
            }
        }
        note(&mut self.said, format!("closed {ids:?} after holding {} ms", hold.as_millis()));
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct Script {
        pub adapter: String,
        pub glided: Option<(u32, u32)>,
        pub spawn_fails: bool,
        pub ready_fails: bool,
        pub shown_fails: bool,
        pub settle_never: bool,
        pub dies_after_glide: bool,
        pub settle_after: Duration,
    }

    pub struct FakeHelper {
        log: Arc<Mutex<Vec<String>>>,
        script: Arc<Mutex<Script>>,
        settles: Vec<(u32, Instant)>,
        glides: Vec<u32>,
        dead: bool,
    }

    pub struct FakeSpawner {
        pub log: Arc<Mutex<Vec<String>>>,
        pub script: Arc<Mutex<Script>>,
        pub spawned: Arc<Mutex<usize>>,
    }

    impl FakeSpawner {
        pub fn new(log: Arc<Mutex<Vec<String>>>) -> Self {
            Self { log, script: Arc::new(Mutex::new(Script::default())), spawned: Arc::new(Mutex::new(0)) }
        }
    }

    impl Spawn for FakeSpawner {
        fn spawn(&self, _timeout: Duration) -> Result<Box<dyn Link>> {
            if self.script.lock().unwrap().spawn_fails {
                return Err(anyhow!("the test helper could not start"));
            }
            *self.spawned.lock().unwrap() += 1;
            self.log.lock().unwrap().push("spawn".into());
            Ok(Box::new(FakeHelper { log: self.log.clone(), script: self.script.clone(), settles: Vec::new(), glides: Vec::new(), dead: false }))
        }
    }

    impl FakeHelper {
        fn say(&self, what: String) {
            self.log.lock().unwrap().push(what);
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

        fn ready(&mut self, _timeout: Duration) -> Result<String> {
            self.living()?;
            if self.script().ready_fails {
                return Err(anyhow!("no device"));
            }
            let adapter = self.script().adapter;
            Ok(if adapter.is_empty() { "Fake GPU".into() } else { adapter })
        }

        fn open(&mut self, id: u32, _screen: &Screen, still: &Path) -> Result<()> {
            self.living()?;
            self.say(format!("open {id} {}", still.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
            Ok(())
        }

        fn shown(&mut self, id: u32, timeout: Duration) -> Result<()> {
            self.living()?;
            if self.script().shown_fails {
                std::thread::sleep(timeout);
                return Err(anyhow!("macOS never reported the live layer as visible"));
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
            if self.script().settle_never {
                std::thread::sleep(timeout);
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
