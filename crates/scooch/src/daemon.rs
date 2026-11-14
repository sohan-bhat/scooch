use crate::platform::{Desktop, Format, Screen, Snapshot};
use crate::state::{display_of, file_safe, parse, same, write_atomic, Entry, Lock, State, Store, Tally};
use crate::glide::{Glider, Plan, Program};
use crate::{cache, poke, scene};
use anyhow::{Context, Result};
use scooch_core::{Analysis, Options, Placement, RgbImage, Scene};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const QUIET: Duration = Duration::from_millis(250);
const REAP_EVERY: Duration = Duration::from_secs(2);
pub const REST: Duration = Duration::from_secs(30);
pub const LIMIT: Duration = Duration::from_secs(2);
pub const QUALITY: u8 = 95;

#[derive(Default)]
struct Pending {
    first: Option<Instant>,
    last: Option<Instant>,
}

#[derive(Default)]
pub struct Signal {
    pending: Mutex<Pending>,
    ready: Condvar,
}

impl Signal {
    pub fn wake(&self) {
        let now = Instant::now();
        let mut pending = self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        pending.first.get_or_insert(now);
        pending.last = Some(now);
        self.ready.notify_one();
    }

    pub fn first(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while pending.last.is_none() {
            pending = self.ready.wait(pending).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    pub fn take(&self) {
        *self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Pending::default();
    }

    pub fn settle(&self, quiet: Duration, limit: Duration) {
        let mut pending = self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while pending.last.is_none() {
            pending = self.ready.wait(pending).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        while let (Some(first), Some(last)) = (pending.first, pending.last) {
            let due = (last + quiet).min(first + limit);
            let now = Instant::now();
            if now >= due {
                break;
            }
            pending = self.ready.wait_timeout(pending, due - now).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
        }
        *pending = Pending::default();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Unchanged,
    Reapplied,
    Plain,
    Kept,
    Rendered,
    Skipped,
    Interrupted,
}

#[derive(Debug, Default)]
pub struct Report {
    pub outcomes: Vec<(String, Outcome)>,
    pub problem: Option<String>,
    pub retry: bool,
    pub unsaved: bool,
    pub exit: bool,
    pub unload: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Restored {
    Back,
    Plain,
    Stuck(Option<PathBuf>),
}

pub const SPACE_UNKNOWN: &str = "Scooch could not tell which desktop a screen shows, so it left that screen alone.";
pub const ORPHAN: &str = "Scooch found its own picture on a desktop it has no record for, so pick a wallpaper there.";

pub fn put_back(desktop: &dyn Desktop, store: &Store, screen: &Screen, entry: &Entry) -> Result<Restored> {
    let current = desktop.wallpaper(screen)?;
    let showing_ours = current.as_deref().is_some_and(|path| store.owns(path));
    if entry.unrestorable {
        if !showing_ours {
            return Ok(Restored::Plain);
        }
        if !entry.source.is_file() {
            return Ok(Restored::Stuck(current));
        }
        desktop.set_wallpaper(screen, &entry.source)?;
        return Ok(Restored::Plain);
    }
    if !showing_ours && !current.as_deref().is_some_and(|path| same(path, &entry.source)) {
        return Ok(Restored::Back);
    }
    if entry.original.is_file() {
        if !current.as_deref().is_some_and(|path| same(path, &entry.original)) {
            desktop.set_wallpaper(screen, &entry.original)?;
        }
        return Ok(Restored::Back);
    }
    if entry.source.is_file() {
        if !current.as_deref().is_some_and(|path| same(path, &entry.source)) {
            desktop.set_wallpaper(screen, &entry.source)?;
        }
        return Ok(Restored::Plain);
    }
    Ok(Restored::Stuck(current.filter(|path| store.owns(path))))
}

pub fn gone(id: &str, known: Option<&[String]>, screens: &[Screen]) -> bool {
    let Some(known) = known else { return false };
    id.contains('@') && !known.iter().any(|seen| seen == id) && screens.iter().any(|screen| display_of(&screen.id) == display_of(id))
}

pub fn relink(state: &mut State, id: &str, shown: &Path, known: Option<&[String]>, screens: &[Screen]) -> bool {
    if state.screens.contains_key(id) {
        return true;
    }
    let owner = state.screens.iter().find(|(_, entry)| entry.output.as_deref().is_some_and(|output| same(output, shown))).map(|(owner, _)| owner.clone());
    let Some(owner) = owner else { return false };
    let mut entry = state.screens[&owner].clone();
    if !owner.contains('@') || gone(&owner, known, screens) {
        state.screens.remove(&owner);
    } else {
        (entry.absent_since, entry.absent_ticks) = (None, 0);
    }
    state.screens.insert(id.to_string(), entry);
    true
}

pub fn claim_orphans(state: &mut State, desktop: &dyn Desktop, store: &Store, screens: &[Screen], known: Option<&[String]>) -> usize {
    let mut orphans = 0;
    for screen in screens {
        if state.screens.contains_key(&screen.id) {
            continue;
        }
        let Ok(Some(shown)) = desktop.wallpaper(screen) else { continue };
        if !store.owns(&shown) || relink(state, &screen.id, &shown, known, screens) {
            continue;
        }
        orphans += 1;
        keep(state, shown);
    }
    orphans
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settled {
    pub tally: Tally,
    pub standing: bool,
}

pub fn settle_screens(state: &mut State, desktop: &dyn Desktop, store: &Store, screens: &[Screen], known: Option<&[String]>, drop_gone: bool) -> Settled {
    let orphans = claim_orphans(state, desktop, store, screens, known);
    let mut tally = Tally::default();
    let mut stuck = 0;
    let ids: Vec<String> = state.screens.keys().cloned().collect();
    for id in ids {
        let entry = state.screens[&id].clone();
        if untouched(store, &id, &entry) {
            state.screens.remove(&id);
            tally.back += 1;
            continue;
        }
        let Some(screen) = screens.iter().find(|screen| screen.id == id) else {
            if drop_gone && gone(&id, known, screens) {
                state.screens.remove(&id);
                sweep(desktop, store, state, &id, entry.output.as_slice());
            } else {
                tally.pending += 1;
            }
            continue;
        };
        let outcome = match put_back(desktop, store, screen, &entry) {
            Ok(outcome) => outcome,
            Err(_) => {
                tally.pending += 1;
                continue;
            }
        };
        state.screens.remove(&id);
        match outcome {
            Restored::Stuck(showing) => {
                stuck += 1;
                if let Some(showing) = showing {
                    remember(&mut state.stuck, showing);
                }
            }
            Restored::Plain => tally.plain += 1,
            Restored::Back => tally.back += 1,
        }
        sweep(desktop, store, state, &id, entry.output.as_slice());
    }
    match desktop.in_use(&store.out()) {
        Ok(Some(used)) => {
            (tally.stuck, tally.strays) = standing(state, store, &used);
            Settled { tally, standing: true }
        }
        _ => {
            (tally.stuck, tally.strays) = (stuck, orphans);
            Settled { tally, standing: false }
        }
    }
}

pub fn untouched(store: &Store, id: &str, entry: &Entry) -> bool {
    entry.output.is_none() && store.outputs_of(id).is_empty()
}

pub fn absorb(tally: &mut Tally, settled: &Settled) {
    tally.back += settled.tally.back;
    tally.plain += settled.tally.plain;
    if settled.standing {
        (tally.stuck, tally.strays) = (settled.tally.stuck, settled.tally.strays);
    } else {
        tally.stuck += settled.tally.stuck;
        tally.strays = tally.strays.max(settled.tally.strays);
    }
}

pub fn standing(state: &State, store: &Store, used: &[PathBuf]) -> (usize, usize) {
    let mut seen: Vec<&PathBuf> = Vec::new();
    let (mut stuck, mut strays) = (0, 0);
    for path in used {
        if !store.owns(path) || seen.iter().any(|earlier| same(earlier, path)) {
            continue;
        }
        seen.push(path);
        if state.screens.iter().any(|(id, entry)| named_for(id, path) || entry.output.as_deref().is_some_and(|output| same(output, path))) {
            continue;
        }
        if state.stuck.iter().any(|left| same(left, path)) {
            stuck += 1;
        } else {
            strays += 1;
        }
    }
    (stuck, strays)
}

pub fn named_for(id: &str, path: &Path) -> bool {
    path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(&format!("{}-", file_safe(id))))
}

pub fn discard(desktop: &dyn Desktop, store: &Store, doomed: &[PathBuf], owned: &[PathBuf]) {
    let doomed: Vec<&PathBuf> = doomed.iter().filter(|path| store.owns(path) && path.is_file()).collect();
    if doomed.is_empty() {
        return;
    }
    let used = desktop.in_use(&store.out()).ok().flatten();
    for path in doomed {
        let spared = match &used {
            Some(used) => used.iter().any(|shown| same(shown, path)),
            None => !owned.iter().any(|mine| same(mine, path)),
        };
        if !spared {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn protected(state: &State, path: &Path) -> bool {
    state.kept.iter().chain(&state.stuck).any(|kept| same(kept, path)) || state.screens.values().any(|entry| entry.output.as_deref().is_some_and(|output| same(output, path)))
}

pub fn sweep(desktop: &dyn Desktop, store: &Store, state: &State, id: &str, extra: &[PathBuf]) {
    let mut doomed = store.outputs_of(id);
    for path in extra {
        if !doomed.iter().any(|listed| same(listed, path)) {
            doomed.push(path.clone());
        }
    }
    doomed.retain(|path| !protected(state, path));
    discard(desktop, store, &doomed, &doomed);
}

pub fn forget(desktop: &dyn Desktop, store: &Store, state: &mut State) {
    let doomed: Vec<PathBuf> = store.pictures().into_iter().filter(|path| !state.screens.values().any(|entry| entry.output.as_deref().is_some_and(|output| same(output, path)))).collect();
    discard(desktop, store, &doomed, &[]);
    let _ = std::fs::remove_dir(store.out());
    let _ = std::fs::remove_dir_all(store.cache());
    state.kept.retain(|path| path.exists());
    state.stuck.retain(|path| path.exists());
}

fn remember(list: &mut Vec<PathBuf>, path: PathBuf) {
    list.retain(|listed| listed.exists());
    if !list.iter().any(|listed| same(listed, &path)) {
        list.push(path);
    }
}

pub fn keep(state: &mut State, path: PathBuf) {
    remember(&mut state.kept, path);
}

pub const ABSENT_FOR: u64 = 24 * 60 * 60;
pub const ABSENT_TICKS: u32 = 5;

pub fn absence(entry: &mut Entry, present: bool, now: u64) -> bool {
    if present {
        (entry.absent_since, entry.absent_ticks) = (None, 0);
        return false;
    }
    let since = *entry.absent_since.get_or_insert(now);
    entry.absent_ticks = entry.absent_ticks.saturating_add(1);
    now.saturating_sub(since) > ABSENT_FOR && entry.absent_ticks >= ABSENT_TICKS
}

pub fn prune(state: &mut State, desktop: &dyn Desktop, store: &Store, known: Option<&[String]>, screens: &[Screen], now: u64) -> usize {
    let Some(listed) = known else { return 0 };
    let ids: Vec<String> = state.screens.keys().cloned().collect();
    let mut pruned = 0;
    for id in ids {
        let present = listed.contains(&id) || screens.iter().any(|screen| screen.id == id);
        if !present && !gone(&id, known, screens) {
            continue;
        }
        let entry = state.screens.get_mut(&id).expect("the id was just listed");
        if absence(entry, present, now) {
            let output = entry.output.clone();
            state.screens.remove(&id);
            sweep(desktop, store, state, &id, output.as_slice());
            pruned += 1;
        }
    }
    pruned
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_secs()).unwrap_or(0)
}

pub struct Daemon {
    pub desktop: Arc<dyn Desktop>,
    pub store: Store,
    pub grace: Duration,
    recent: Mutex<HashMap<String, Instant>>,
    ledger: Mutex<Ledger>,
    pub glider: Arc<Mutex<Glider>>,
    reaping: Arc<std::sync::atomic::AtomicBool>,
    motion: std::sync::atomic::AtomicU8,
    sessions: Arc<Mutex<HashMap<String, (u64, scooch_core::Session)>>>,
    used: Arc<Mutex<Instant>>,
    seen: Mutex<Option<u64>>,
}

#[derive(Default)]
struct Ledger {
    expected: Option<Vec<u8>>,
    external: bool,
}

pub fn adoptable(entry: &Entry, current: &Path, ours: bool) -> bool {
    !ours && current.is_file() && !same(current, &entry.source)
}

pub const NOT_A_FILE: &str = "Scooch only moves picture files, so pick a photo as your wallpaper to use it again.";

pub fn trace() -> bool {
    static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRACE.get_or_init(|| std::env::var_os("SCOOCH_TRACE").is_some_and(|value| !value.is_empty() && value != "0"))
}

thread_local! {
    static LAPS: std::cell::RefCell<Vec<(&'static str, u128)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn lap(name: &'static str, since: &mut Instant) {
    if trace() {
        LAPS.with(|laps| laps.borrow_mut().push((name, since.elapsed().as_millis())));
        *since = Instant::now();
    }
}

fn laps() -> String {
    LAPS.with(|laps| laps.borrow_mut().drain(..).map(|(name, millis)| format!("{name} {millis}")).collect::<Vec<_>>().join(", "))
}

pub fn stamp() -> String {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}", since.as_secs(), since.subsec_millis())
}

impl Daemon {
    pub fn new(desktop: Arc<dyn Desktop>, store: Store) -> Self {
        let grace = desktop.read_lag();
        let glider = Arc::new(Mutex::new(Glider::new(Box::new(Program(crate::glide::program())))));
        Self { desktop, store, grace, recent: Mutex::new(HashMap::new()), ledger: Mutex::new(Ledger::default()), glider, reaping: Arc::new(std::sync::atomic::AtomicBool::new(false)), motion: std::sync::atomic::AtomicU8::new(0), sessions: Arc::new(Mutex::new(HashMap::new())), used: Arc::new(Mutex::new(Instant::now())), seen: Mutex::new(None) }
    }

    pub fn quick_look(&self) -> bool {
        if !self.desktop.dropped() {
            return false;
        }
        std::thread::sleep(QUICK);
        let Ok(snapshot) = self.desktop.snapshot() else { return false };
        let now = icon_print(&snapshot);
        let before = *self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let moved = before.is_some_and(|before| before != now);
        if trace() {
            eprintln!("{} quick look after {} ms, icons {now:016x} moved {moved}", stamp(), QUICK.as_millis());
        }
        moved
    }

    fn glide_block(&self, state: &State) -> Option<&'static str> {
        if !self.desktop.live() {
            Some("not live")
        } else if !state.glide {
            Some("setting off")
        } else if self.read_motion() {
            Some("reduce motion")
        } else {
            None
        }
    }

    fn read_motion(&self) -> bool {
        let reduce = self.desktop.reduce_motion();
        let seen = if reduce { 2 } else { 1 };
        if self.motion.swap(seen, std::sync::atomic::Ordering::SeqCst) != seen {
            eprintln!("{} glide reduce motion {}", stamp(), if reduce { "on" } else { "off" });
        }
        reduce
    }

    fn glide_wanted(&self, state: &State) -> bool {
        state.on && self.glide_block(state).is_none()
    }

    pub fn warm(&self) {
        if !self.desktop.live() {
            return;
        }
        let state = self.store.load();
        if !self.glide_wanted(&state) {
            return;
        }
        let Ok(screens) = self.desktop.screens() else { return };
        let Ok(mut glider) = self.glider.try_lock() else { return };
        glider.reap();
        for screen in &screens {
            let Some(entry) = state.screens.get(&screen.id) else { continue };
            let Some(output) = entry.output.as_ref().filter(|output| output.is_file()) else { continue };
            let Ok(key) = tables_key(&entry.source, &state.options) else { continue };
            if !self.store.tables_for(output, key).is_file() {
                continue;
            }
            let scene = scene::scene(screen, &[]);
            glider.warm(&entry.source, [scene.width, scene.height]);
            break;
        }
        drop(glider);
        self.reap_later();
    }

    fn session_still(&self, screen: &str, key: u64, (image, analysis, placement, scene, options): (&RgbImage, &Analysis, &Placement, &Scene, &Options)) -> (Arc<scooch_core::Bundle>, Arc<RgbImage>) {
        let mut sessions = self.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let slot = sessions.entry(screen.to_string()).or_insert_with(|| (key, scooch_core::Session::new()));
        if slot.0 != key {
            slot.1.forget();
            slot.0 = key;
        }
        let made = slot.1.still_shared(image, analysis, placement, scene, options);
        drop(sessions);
        *self.used.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
        made
    }

    fn forget_session(&self, screen: &str) {
        if let Some((_, mut session)) = self.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(screen) {
            session.forget();
        }
    }

    pub fn held_bytes(&self) -> usize {
        self.sessions.lock().map(|sessions| sessions.values().map(|(_, session)| session.bytes()).sum()).unwrap_or(0)
    }

    fn reap_later(&self) {
        let running = self.glider.lock().map(|glider| glider.running()).unwrap_or(false);
        let holding = self.sessions.lock().map(|sessions| !sessions.is_empty()).unwrap_or(false);
        if !(running || holding) || self.reaping.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let (glider, reaping, sessions, used) = (self.glider.clone(), self.reaping.clone(), self.sessions.clone(), self.used.clone());
        let spawned = std::thread::Builder::new().name("scooch-reap".into()).spawn(move || loop {
            std::thread::sleep(REAP_EVERY);
            let idle = used.lock().map(|used| used.elapsed() >= REST).unwrap_or(true);
            let holding = {
                let mut sessions = sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if idle && !sessions.is_empty() {
                    let held: usize = sessions.values().map(|(_, session)| session.bytes()).sum();
                    sessions.clear();
                    relieve();
                    if trace() {
                        eprintln!("{} sessions let go of {} MB after {} s at rest", stamp(), held >> 20, REST.as_secs());
                    }
                }
                !sessions.is_empty()
            };
            let mut glider = glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            glider.reap();
            if !glider.running() && !holding {
                reaping.store(false, std::sync::atomic::Ordering::SeqCst);
                return;
            }
        });
        if spawned.is_err() {
            self.reaping.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn tidy_tables(&self, state: &State) {
        let Ok(found) = std::fs::read_dir(self.store.tables()) else { return };
        let kept: Vec<PathBuf> = state.screens.values().filter_map(|entry| Some(self.store.tables_for(entry.output.as_ref()?, tables_key(&entry.source, &state.options).ok()?))).collect();
        for file in found.flatten() {
            let path = file.path();
            if !kept.contains(&path) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn settling(&self, screen: &str) -> bool {
        self.recent.lock().map(|recent| recent.get(screen).is_some_and(|at| at.elapsed() < self.grace)).unwrap_or(false)
    }

    pub fn tick(&self) -> Report {
        let before = self.store.read();
        *self.ledger.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Ledger { expected: before.clone(), external: false };
        let mut state = parse(before.as_deref());
        let mut report = Report::default();
        if !state.on && !state.restoring {
            report.exit = true;
            return report;
        }
        if state.restoring {
            self.restore(&mut state, &mut report);
        } else {
            let mut clock = Instant::now();
            let glider = self.glider.clone();
            let blocked = self.glide_block(&state);
            let mut gliding = blocked.is_none().then(|| glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
            if let Some(glider) = gliding.as_deref_mut() {
                self.open_early(&state, glider);
                lap("early", &mut clock);
            }
            let snapshot = self.desktop.snapshot();
            lap("snapshot", &mut clock);
            if let Ok(snapshot) = &snapshot {
                let print = icon_print(snapshot);
                *self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(print);
                if trace() {
                    eprintln!("{} snapshot icons {print:016x}", stamp());
                }
            }
            match snapshot {
                Ok(snapshot) => self.pass(&mut state, &snapshot, &mut report, (gliding.as_deref_mut(), blocked)),
                Err(error) => report.problem = Some(error.to_string()),
            }
            if let Some(glider) = gliding.as_deref_mut() {
                glider.close_early();
            }
            drop(gliding);
            self.reap_later();
        }
        state.problem = report.problem.clone();
        let now = self.store.read();
        let changed = {
            let ledger = self.ledger.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            ledger.external || now != ledger.expected
        };
        if changed {
            state = merge(&state, &parse(before.as_deref()), &parse(now.as_deref()));
            report.retry = true;
        }
        if state.restoring && !state.on && state.screens.is_empty() {
            state.restoring = false;
            state.picture = None;
            forget(self.desktop.as_ref(), &self.store, &mut state);
            report.unload = true;
        }
        if state == parse(now.as_deref()) {
            return report;
        }
        if let Err(error) = self.store.save(&state) {
            report.problem.get_or_insert(error.to_string());
            report.unsaved = true;
        }
        report
    }

    fn restore(&self, state: &mut State, report: &mut Report) {
        let screens = match self.desktop.screens() {
            Ok(screens) => screens,
            Err(error) => {
                report.problem = Some(error.to_string());
                return;
            }
        };
        let known = self.desktop.known().ok().flatten();
        prune(state, self.desktop.as_ref(), &self.store, known.as_deref(), &screens, unix_now());
        let settled = settle_screens(state, self.desktop.as_ref(), &self.store, &screens, known.as_deref(), false);
        absorb(&mut state.tally, &settled);
        if settled.tally.strays > 0 {
            report.problem = Some(ORPHAN.to_string());
        }
    }

    fn open_early(&self, state: &State, glider: &mut Glider) {
        let Ok(screens) = self.desktop.screens() else { return };
        for screen in &screens {
            let Some(entry) = state.screens.get(&screen.id) else { continue };
            let Some(output) = entry.output.as_ref().filter(|output| output.is_file()) else { continue };
            let Ok(key) = tables_key(&entry.source, &state.options) else { continue };
            if !self.store.tables_for(output, key).is_file() || !self.desktop.wallpaper(screen).ok().flatten().is_some_and(|shown| same(&shown, output)) {
                continue;
            }
            let scene = scene::scene(screen, &[]);
            glider.open_early(screen, &entry.source, [scene.width, scene.height], output);
        }
    }

    fn pass(&self, state: &mut State, snapshot: &Snapshot, report: &mut Report, (mut gliding, blocked): (Option<&mut Glider>, Option<&'static str>)) {
        let options = state.options;
        let known = self.desktop.known().ok().flatten();
        prune(state, self.desktop.as_ref(), &self.store, known.as_deref(), &snapshot.screens, unix_now());
        for screen in &snapshot.screens {
            if unplaced(state, &screen.id) {
                report.outcomes.push((screen.id.clone(), Outcome::Skipped));
                report.problem.get_or_insert(SPACE_UNKNOWN.to_string());
                continue;
            }
            let previous = state.screens.get(&screen.id).and_then(|entry| entry.output.clone());
            let outcome = self.screen(state, screen, snapshot, &options, known.as_deref(), (gliding.as_deref_mut(), blocked));
            if state.screens.get(&screen.id).and_then(|entry| entry.output.clone()) != previous {
                sweep(self.desktop.as_ref(), &self.store, state, &screen.id, previous.as_slice());
            }
            match outcome {
                Ok(outcome) => {
                    report.retry |= outcome == Outcome::Interrupted;
                    report.outcomes.push((screen.id.clone(), outcome));
                }
                Err(error) => {
                    report.outcomes.push((screen.id.clone(), Outcome::Skipped));
                    report.problem.get_or_insert(error.to_string());
                }
            }
        }
        if let Some(glider) = gliding {
            glider.close_early();
            if glider.in_flight() {
                let watched: Vec<(String, PathBuf)> = glider.screens().into_iter().filter_map(|id| state.screens.get(&id).map(|entry| (id, entry.source.clone()))).collect();
                let mirror = options.mirror;
                let interrupted = || {
                    let now = self.store.load();
                    if !now.on || now.restoring || !now.glide || now.options.mirror != mirror || watched.iter().any(|(id, source)| now.screens.get(id).is_none_or(|entry| entry.source != *source)) {
                        return true;
                    }
                    let screens = self.desktop.screens().unwrap_or_default();
                    watched.iter().any(|(id, _)| !screens.iter().any(|screen| screen.id == *id))
                };
                glider.land(self.desktop.hold(), &interrupted);
            }
        }
        self.tidy_tables(state);
    }

    fn screen(&self, state: &mut State, screen: &Screen, snapshot: &Snapshot, options: &Options, known: Option<&[String]>, (mut glider, blocked): (Option<&mut Glider>, Option<&'static str>)) -> Result<Outcome> {
        let current = self.desktop.wallpaper(screen)?;
        let ours = current.as_deref().is_some_and(|path| self.store.owns(path));
        let usable = current.clone().filter(|path| path.is_file());
        let settling = self.settling(&screen.id);
        if !state.screens.contains_key(&screen.id) {
            if settling {
                return Ok(Outcome::Skipped);
            }
            if let (true, Some(shown)) = (ours, current.as_deref()) {
                if !relink(state, &screen.id, shown, known, &snapshot.screens) {
                    keep(state, shown.to_path_buf());
                    anyhow::bail!(ORPHAN);
                }
            }
        }
        if !state.screens.contains_key(&screen.id) {
            let entry = match state.picture.clone().filter(|picture| picture.is_file()) {
                Some(picture) => {
                    let mut entry = Entry::new(&picture);
                    match self.desktop.restorable(screen)?.filter(|path| !self.store.owns(path)) {
                        Some(original) => entry.original = original,
                        None => entry.unrestorable = true,
                    }
                    entry.chosen = true;
                    entry
                }
                None => Entry::new(usable.as_deref().ok_or_else(|| anyhow::anyhow!(NOT_A_FILE))?),
            };
            state.screens.insert(screen.id.clone(), entry);
        }
        let entry = state.screens.get_mut(&screen.id).context("the screen entry vanished")?;
        let chosen = entry.chosen && entry.source.is_file();
        if let (false, false, Some(picture)) = (settling, chosen, usable.as_deref()) {
            if adoptable(entry, picture, ours) {
                (entry.source, entry.original, entry.placement, entry.chosen, entry.unrestorable) = (picture.to_path_buf(), picture.to_path_buf(), None, false, false);
            }
        }
        if usable.is_none() && !ours && !settling && !chosen {
            anyhow::bail!(NOT_A_FILE);
        }
        let scene = scene::scene(screen, &snapshot.icons);
        entry.icons = scene.icons.len();
        let fingerprint = scene::fingerprint(&entry.source, &scene, options);
        let showing = current.as_deref().is_some_and(|path| same(path, entry.shown()));
        if entry.fingerprint.as_deref() == Some(fingerprint.as_str()) && entry.shown().is_file() {
            if let Some(glider) = glider.as_deref_mut() {
                glider.close_early_for(&screen.id);
            }
            if showing || settling {
                return Ok(Outcome::Unchanged);
            }
            let shown = entry.shown().to_path_buf();
            self.show(screen, &shown)?;
            return Ok(Outcome::Reapplied);
        }
        let outcome = self.place(entry, screen, &scene, options, (current.as_deref().filter(|_| !settling), current.as_deref()), (glider, blocked))?;
        if outcome != Outcome::Interrupted {
            entry.fingerprint = Some(fingerprint);
            entry.chosen = false;
        }
        Ok(outcome)
    }

    fn place(&self, entry: &mut Entry, screen: &Screen, scene: &Scene, options: &Options, (current, seen): (Option<&Path>, Option<&Path>), (mut glider, blocked): (Option<&mut Glider>, Option<&'static str>)) -> Result<Outcome> {
        let source = entry.source.clone();
        let mut image: Option<RgbImage> = None;
        let mut clock = Instant::now();
        let key = cache::key(&source)?;
        let analysis = cache::analysis(&self.store, key, || {
            let decoded = self.pixels(&source, key)?;
            lap("decode", &mut clock);
            let made = scooch_core::analyze(&decoded);
            image = Some(decoded);
            Ok(made)
        })?;
        lap("analysis", &mut clock);
        let wanted = crate::state::scenery_on(options) && !scene.icons.is_empty();
        let previous = entry.placement.as_ref().filter(|_| wanted || !entry.painted);
        let placement = scooch_core::solve(&analysis, scene, options, previous);
        lap("solve", &mut clock);
        entry.flip_helps = flip_check(&analysis, scene, options, &placement);
        let pads = scooch_core::pads_active(&analysis, &placement, scene, options);
        let output = entry.output.clone().filter(|path| path.is_file());
        let showing_output = matches!((current, &output), (Some(current), Some(output)) if same(current, output));
        if scene::is_identity(&placement) && !pads {
            if let Some(glider) = glider.as_deref_mut() {
                glider.close_early_for(&screen.id);
            }
            drop(analysis);
            drop(image);
            let copied = showing_output && !entry.painted && entry.placement.as_ref().is_some_and(scene::is_identity);
            entry.placement = Some(placement);
            if current.is_some_and(|path| same(path, &source)) {
                entry.output = None;
            } else if !copied {
                crate::glide::skipped(blocked.unwrap_or("plain picture"), "");
                if !self.plain(entry, screen, current, seen)? {
                    return Ok(Outcome::Interrupted);
                }
            }
            entry.painted = false;
            return Ok(Outcome::Plain);
        }
        if options.pads <= 0.0 && !wanted && !entry.painted && showing_output && entry.placement.as_ref() == Some(&placement) {
            if let Some(glider) = glider.as_deref_mut() {
                glider.close_early_for(&screen.id);
            }
            return Ok(Outcome::Kept);
        }
        let painted = wanted && scooch_core::scenery_active(&analysis, &placement, scene, options);
        let decoded = match image.take() {
            Some(decoded) => decoded,
            None => self.pixels(&source, key)?,
        };
        lap("decode", &mut clock);
        let format = self.desktop.format();
        let mut flight = None;
        let mut tables = None;
        let picture = if let Some(glider) = glider.as_deref_mut() {
            let tables_key = tables_key(&source, options)?;
            let (_, next) = self.fresh_name(&screen.id, entry.counter, format.extension(), current);
            let made = self.store.tables_for(&next, tables_key);
            let (bundle, picture) = self.session_still(&screen.id, key, (&decoded, &analysis, &placement, scene, options));
            lap("render", &mut clock);
            let written = std::fs::create_dir_all(self.store.tables()).map_err(anyhow::Error::from).and_then(|_| write_atomic(&made, &bundle.tables_to_bytes()));
            drop(bundle);
            lap("tables", &mut clock);
            if written.is_ok() {
                let from = output.as_ref().filter(|_| showing_output).map(|output| self.store.tables_for(output, tables_key)).filter(|from| from.is_file());
                match (from, output.as_deref().filter(|_| showing_output)) {
                    (Some(from), Some(still)) => {
                        flight = glider.begin(&Plan { screen, picture: &source, size: [scene.width, scene.height], still, from: &from, to: &made });
                        lap("open", &mut clock);
                    }
                    (_, None) => crate::glide::skipped("output not shown", ""),
                    (None, _) => crate::glide::skipped("no tables", ""),
                }
                tables = Some(made);
            } else {
                crate::glide::skipped("tables not written", "");
            }
            picture
        } else {
            crate::glide::skipped(blocked.unwrap_or("not live"), "");
            let held = entry.placement.as_ref() == Some(&placement);
            let picture = if wanted || held {
                self.session_still(&screen.id, key, (&decoded, &analysis, &placement, scene, options)).1
            } else {
                self.forget_session(&screen.id);
                Arc::new(scooch_core::render_with(&decoded, &analysis, &placement, scene, options))
            };
            lap("render", &mut clock);
            picture
        };
        let undo = |glider: Option<&mut Glider>, flight: Option<u32>, tables: &Option<PathBuf>| {
            if let Some(glider) = glider {
                if let Some(id) = flight {
                    glider.abort(id);
                }
                glider.close_early_for(&screen.id);
            }
            if let Some(tables) = tables {
                let _ = std::fs::remove_file(tables);
            }
        };
        drop(decoded);
        drop(analysis);
        let bytes = match encode(&picture, format, self.desktop.profile().as_deref(), self.desktop.live()) {
            Ok(bytes) => bytes,
            Err(error) => {
                undo(glider.as_deref_mut(), flight, &tables);
                return Err(error);
            }
        };
        drop(picture);
        lap("encode", &mut clock);
        if let (true, Some(output)) = (showing_output, &output) {
            if std::fs::read(output).is_ok_and(|old| old == bytes) {
                undo(glider.as_deref_mut(), flight, &tables);
                entry.placement = Some(placement);
                return Ok(Outcome::Kept);
            }
        }
        if let (Some(glider), None) = (glider.as_deref_mut(), flight) {
            glider.close_early_for(&screen.id);
        }
        let was = std::mem::replace(&mut entry.painted, painted);
        let published = self.publish(entry, screen, current, seen, format.extension(), |path| write_atomic(path, &bytes));
        if !matches!(published, Ok(true)) {
            entry.painted = was;
            undo(glider.as_deref_mut(), flight, &tables);
        }
        if !published? {
            return Ok(Outcome::Interrupted);
        }
        let set = Instant::now();
        entry.placement = Some(placement);
        lap("set", &mut clock);
        if let (Some(made), Some(output), Ok(tables_key)) = (&tables, &entry.output, tables_key(&source, options)) {
            let wanted = self.store.tables_for(output, tables_key);
            if *made != wanted {
                let _ = std::fs::rename(made, &wanted);
            }
        }
        if let (Some(glider), Some(id), Some(output)) = (glider, flight, entry.output.as_ref()) {
            glider.settle(id, output, set);
        }
        Ok(Outcome::Rendered)
    }

    fn plain(&self, entry: &mut Entry, screen: &Screen, current: Option<&Path>, seen: Option<&Path>) -> Result<bool> {
        let source = entry.source.clone();
        let extension = source.extension().map_or("jpg".to_string(), |extension| extension.to_string_lossy().to_ascii_lowercase());
        self.publish(entry, screen, current, seen, &extension, |path| {
            let bytes = std::fs::read(&source).with_context(|| format!("Scooch could not read {}, so check that the picture is still there.", source.display()))?;
            write_atomic(path, &bytes)
        })
    }

    fn publish(&self, entry: &mut Entry, screen: &Screen, current: Option<&Path>, seen: Option<&Path>, extension: &str, write: impl FnOnce(&Path) -> Result<()>) -> Result<bool> {
        let (counter, path) = self.fresh_name(&screen.id, entry.counter, extension, current);
        std::fs::create_dir_all(self.store.out()).with_context(|| format!("Scooch could not make {}, so check the disk.", self.store.out().display()))?;
        write(&path)?;
        let made = [path.clone()];
        if self.picked_meanwhile(screen, seen)? || !self.note(&screen.id, &Entry { output: Some(path.clone()), counter, ..entry.clone() })? {
            discard(self.desktop.as_ref(), &self.store, &made, &made);
            return Ok(false);
        }
        if let Err(error) = self.show(screen, &path) {
            let _ = self.note(&screen.id, entry);
            discard(self.desktop.as_ref(), &self.store, &made, &made);
            return Err(error);
        }
        entry.counter = counter;
        entry.output = Some(path);
        Ok(true)
    }

    fn note(&self, id: &str, entry: &Entry) -> Result<bool> {
        let mut ledger = self.ledger.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let fresh = self.store.read();
        if fresh != ledger.expected {
            ledger.external = true;
        }
        let mut state = parse(fresh.as_deref());
        if !state.on {
            return Ok(false);
        }
        match state.screens.get_mut(id) {
            Some(theirs) => {
                theirs.output.clone_from(&entry.output);
                theirs.painted = entry.painted;
                theirs.counter = theirs.counter.max(entry.counter);
            }
            None => {
                state.screens.insert(id.to_string(), entry.clone());
            }
        }
        ledger.expected = Some(self.store.write(&state)?);
        Ok(true)
    }

    fn picked_meanwhile(&self, screen: &Screen, seen: Option<&Path>) -> Result<bool> {
        if !self.store.load().on {
            return Ok(true);
        }
        let now = self.desktop.wallpaper(screen)?;
        Ok(now.is_some_and(|now| now.is_file() && !self.store.owns(&now) && !seen.is_some_and(|seen| same(&now, seen))))
    }

    fn fresh_name(&self, screen: &str, counter: u64, extension: &str, current: Option<&Path>) -> (u64, PathBuf) {
        let mut next = counter + 1;
        loop {
            let path = self.store.output(screen, next, extension);
            if !path.exists() && !current.is_some_and(|current| same(current, &path)) {
                return (next, path);
            }
            next += 1;
        }
    }

    fn pixels(&self, source: &Path, key: u64) -> Result<RgbImage> {
        if let Some(image) = cache::pixels(&self.store, key) {
            return Ok(image);
        }
        let image = self.decode(source)?;
        cache::keep_pixels(&self.store, key, &image);
        Ok(image)
    }

    fn decode(&self, source: &Path) -> Result<RgbImage> {
        self.desktop.decode(source).with_context(|| format!("Scooch could not read {} as a picture, so pick another wallpaper.", source.display()))
    }

    fn show(&self, screen: &Screen, path: &Path) -> Result<()> {
        self.desktop.set_wallpaper(screen, path)?;
        if let Ok(mut recent) = self.recent.lock() {
            recent.insert(screen.id.clone(), Instant::now());
        }
        if trace() {
            eprintln!("{} set {}", stamp(), path.display());
        }
        Ok(())
    }
}

pub fn unplaced(state: &State, id: &str) -> bool {
    !id.contains('@') && state.screens.keys().any(|known| known.contains('@') && display_of(known) == id)
}

pub fn merge(mine: &State, before: &State, fresh: &State) -> State {
    let mut merged = fresh.clone();
    merged.problem = mine.problem.clone();
    if fresh.tally == before.tally {
        merged.tally = mine.tally;
    }
    for kept in &mine.kept {
        if !merged.kept.contains(kept) {
            merged.kept.push(kept.clone());
        }
    }
    for left in &mine.stuck {
        if !merged.stuck.contains(left) {
            merged.stuck.push(left.clone());
        }
    }
    for (id, ours) in &mine.screens {
        let untouched = |theirs: &Entry| before.screens.get(id).is_some_and(|was| was.source == theirs.source && was.original == theirs.original && was.chosen == theirs.chosen);
        match merged.screens.get_mut(id) {
            Some(theirs) if untouched(theirs) => *theirs = ours.clone(),
            Some(theirs) => {
                if ours.output.is_some() {
                    theirs.output.clone_from(&ours.output);
                    theirs.painted = ours.painted;
                }
                theirs.icons = ours.icons;
                theirs.counter = theirs.counter.max(ours.counter);
            }
            None if before.screens.contains_key(id) => {}
            None => {
                merged.screens.insert(id.clone(), ours.clone());
            }
        }
    }
    for (id, was) in &before.screens {
        if !mine.screens.contains_key(id) && fresh.screens.get(id) == Some(was) {
            merged.screens.remove(id);
        }
    }
    merged
}

pub const QUICK: Duration = Duration::from_millis(60);

pub fn icon_print(snapshot: &Snapshot) -> u64 {
    let mut rects: Vec<[i64; 4]> = snapshot.icons.iter().map(|icon| [icon.x.round() as i64, icon.y.round() as i64, icon.w.round() as i64, icon.h.round() as i64]).collect();
    rects.sort_unstable();
    cache::digest(&rects.iter().flat_map(|rect| rect.iter().flat_map(|value| value.to_le_bytes())).collect::<Vec<u8>>())
}

pub fn tables_key(source: &Path, options: &Options) -> Result<u64> {
    let key = cache::key(source)?;
    Ok(cache::digest(format!("{key:016x}|{}", options.mirror).as_bytes()))
}

pub const FLIP_WORTH: f32 = 0.05;

pub fn worth_flipping(hidden: f32, mirror: bool) -> bool {
    !mirror && hidden > FLIP_WORTH
}

pub fn flip_helps(hidden: f32, flipped: f32) -> bool {
    flipped < hidden / 2.0
}

fn flip_check(analysis: &Analysis, scene: &Scene, options: &Options, placement: &Placement) -> bool {
    let hidden = scooch_core::hidden_share(analysis, scene, placement);
    if !worth_flipping(hidden, options.mirror) {
        return false;
    }
    let flipped = scooch_core::solve(analysis, scene, &Options { mirror: true, ..*options }, None);
    flip_helps(hidden, scooch_core::hidden_share(analysis, scene, &flipped))
}

pub fn encode(picture: &RgbImage, format: Format, profile: Option<&[u8]>, full_color: bool) -> Result<Vec<u8>> {
    Ok(match format {
        Format::Jpeg => jpeg(picture, profile, full_color)?,
        Format::Png => scooch_core::encode_png(picture)?,
    })
}

fn jpeg(picture: &RgbImage, profile: Option<&[u8]>, full_color: bool) -> Result<Vec<u8>> {
    let (Ok(width), Ok(height)) = (u16::try_from(picture.width()), u16::try_from(picture.height())) else {
        return Ok(scooch_core::encode_jpeg(picture, QUALITY)?);
    };
    let mut out = Vec::with_capacity(picture.as_raw().len() / 6);
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, QUALITY);
    encoder.set_sampling_factor(if full_color { jpeg_encoder::SamplingFactor::F_1_1 } else { jpeg_encoder::SamplingFactor::F_2_2 });
    if let Some(profile) = profile {
        encoder.add_icc_profile(profile)?;
    }
    encoder.encode(picture.as_raw(), width, height, jpeg_encoder::ColorType::Rgb)?;
    Ok(out)
}

#[cfg(target_os = "macos")]
fn relieve() {
    extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn relieve() {
    unsafe { libc::malloc_trim(0) };
}

#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
fn relieve() {}

pub const STOP_CAP: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct Stop {
    requested: std::sync::atomic::AtomicBool,
    busy: Mutex<bool>,
    idle: Condvar,
}

impl Stop {
    pub fn request(&self) {
        let _busy = self.busy.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        self.requested.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn requested(&self) -> bool {
        self.requested.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn begin(&self) -> bool {
        let mut busy = self.busy.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.requested() {
            return false;
        }
        *busy = true;
        true
    }

    pub fn end(&self) {
        *self.busy.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
        self.idle.notify_all();
    }

    pub fn wait_idle(&self, cap: Duration) -> bool {
        let busy = self.busy.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_busy, waited) = self.idle.wait_timeout_while(busy, cap, |busy| *busy).unwrap_or_else(|poisoned| poisoned.into_inner());
        !waited.timed_out()
    }
}

#[derive(Debug, PartialEq)]
pub enum Next {
    Go,
    Quit,
}

pub fn after(report: &Report) -> Next {
    if report.unsaved {
        Next::Quit
    } else {
        Next::Go
    }
}

pub fn retire(store: &Store, stop: &Stop) {
    stop.request();
    if store.recorded_pid() == Some(std::process::id()) {
        let _ = std::fs::remove_file(store.pid_file());
    }
}

pub fn release(desktop: &dyn Desktop, store: &Store, report: &Report, stop: &Stop) {
    let asked = stop.requested();
    retire(store, stop);
    if report.unload || (report.exit && !asked) {
        if let Err(error) = desktop.autostart(None) {
            eprintln!("{error}");
        }
    }
}

fn finish(store: &Store, stop: &Stop) -> ! {
    retire(store, stop);
    if stop.wait_idle(STOP_CAP) {
        let _ = store.mark(None);
    }
    std::process::exit(0)
}

pub const LOCK_PATIENCE: Duration = Duration::from_secs(7);
const LOCK_POLL: Duration = Duration::from_millis(100);
pub const ALREADY: &str = "Scooch is already running.";

pub fn hold(store: &Store, patience: Duration, poll: Duration) -> Result<Option<Lock>> {
    let started = Instant::now();
    loop {
        if let Some(lock) = store.lock()? {
            return Ok(Some(lock));
        }
        if started.elapsed() >= patience {
            return Ok(None);
        }
        std::thread::sleep(poll);
    }
}

pub fn gave_up(state: &State) -> Result<()> {
    if state.on || state.restoring {
        anyhow::bail!(ALREADY);
    }
    println!("{ALREADY}");
    Ok(())
}

pub fn prime(desktop: &dyn Desktop, signal: &Arc<Signal>) {
    let consent = signal.clone();
    desktop.watching(Box::new(move || consent.wake()));
}

pub fn watch(desktop: Arc<dyn Desktop>, store: Store) -> Result<()> {
    let signal = Arc::new(Signal::default());
    let stop = Arc::new(Stop::default());
    let poked = signal.clone();
    let (stopper, stopping, stopped) = (stop.clone(), signal.clone(), store.clone());
    poke::listen(
        move || poked.wake(),
        move || {
            retire(&stopped, &stopper);
            stopping.wake();
            finish(&stopped, &stopper);
        },
    )?;
    let Some(_lock) = hold(&store, LOCK_PATIENCE, LOCK_POLL)? else {
        return gave_up(&store.load());
    };
    if let Err(error) = store.mark(Some(crate::state::PROGRAM)) {
        eprintln!("{error}");
    }
    prime(desktop.as_ref(), &signal);
    let daemon = Daemon::new(desktop.clone(), store.clone());
    let worker = signal.clone();
    let tracing = trace();
    std::thread::Builder::new().name("scooch-tick".into()).spawn(move || {
        let mut said: Option<String> = None;
        loop {
            worker.first();
            daemon.warm();
            if daemon.quick_look() {
                worker.take();
            } else {
                worker.settle(QUIET, LIMIT);
            }
            if !stop.begin() {
                return;
            }
            let started = Instant::now();
            let report = daemon.tick();
            relieve();
            stop.end();
            if tracing {
                eprintln!("{} tick {} ms {:?} {}, sessions hold {} MB", stamp(), started.elapsed().as_millis(), report.outcomes, laps(), daemon.held_bytes() >> 20);
            }
            if report.problem != said {
                if let Some(problem) = &report.problem {
                    eprintln!("{problem}");
                }
                said = report.problem.clone();
            }
            if after(&report) == Next::Quit {
                eprintln!("Scooch could not save its settings, so it stopped until the disk has room.");
                finish(&daemon.store, &stop);
            }
            if report.unload || report.exit {
                release(daemon.desktop.as_ref(), &daemon.store, &report, &stop);
                finish(&daemon.store, &stop);
            }
            if report.retry {
                worker.wake();
            }
        }
    })?;
    signal.wake();
    let result = desktop.run(Box::new(move || signal.wake()));
    let _ = store.mark(None);
    result
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::platform::Wake;
    use crate::state::scratch;
    use scooch_core::Rect;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct Fake {
        pub screens: Mutex<Vec<Screen>>,
        pub known: Mutex<Option<Vec<String>>>,
        pub blind: Mutex<bool>,
        pub unloads: AtomicUsize,
        pub icons: Mutex<Vec<Rect>>,
        pub shown: Mutex<HashMap<String, PathBuf>>,
        pub sets: AtomicUsize,
        pub decodes: AtomicUsize,
        pub snapshots: AtomicUsize,
        pub hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        pub moving: Mutex<Option<PathBuf>>,
        pub lost: Mutex<Vec<PathBuf>>,
        pub gallery: Mutex<Vec<PathBuf>>,
        pub broken: Mutex<Option<String>>,
        pub mid_render: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        pub refuse: Mutex<Vec<String>>,
        pub unknowable: Mutex<bool>,
        pub crash_after_set: Mutex<bool>,
        pub lag: Mutex<usize>,
        pub stale: Mutex<HashMap<String, (PathBuf, usize)>>,
        pub watched: Mutex<Option<Wake>>,
        pub live: Mutex<bool>,
        pub reduce: Mutex<bool>,
        pub drop_seen: Mutex<bool>,
        pub events: crate::glide::tests::Events,
    }

    impl Fake {
        pub fn new(screens: Vec<Screen>, picture: &Path) -> Arc<Self> {
            let shown = screens.iter().map(|screen| (screen.id.clone(), picture.to_path_buf())).collect();
            Arc::new(Self { screens: Mutex::new(screens), known: Mutex::new(None), blind: Mutex::new(false), unloads: AtomicUsize::new(0), icons: Mutex::new(Vec::new()), shown: Mutex::new(shown), sets: AtomicUsize::new(0), decodes: AtomicUsize::new(0), snapshots: AtomicUsize::new(0), hook: Mutex::new(None), moving: Mutex::new(None), lost: Mutex::new(Vec::new()), gallery: Mutex::new(Vec::new()), broken: Mutex::new(None), mid_render: Mutex::new(None), refuse: Mutex::new(Vec::new()), unknowable: Mutex::new(false), crash_after_set: Mutex::new(false), lag: Mutex::new(0), stale: Mutex::new(HashMap::new()), watched: Mutex::new(None), live: Mutex::new(false), reduce: Mutex::new(false), drop_seen: Mutex::new(false), events: Arc::new(Mutex::new(Vec::new())) })
        }

        pub fn connect(&self, screens: Vec<Screen>) {
            *self.screens.lock().unwrap() = screens;
        }

        pub fn showing(&self, screen: &str) -> PathBuf {
            self.shown.lock().unwrap()[screen].clone()
        }

        pub fn show(&self, screen: &str, picture: &Path) {
            self.shown.lock().unwrap().insert(screen.into(), picture.to_path_buf());
        }

        fn look(&self, screen: &str) -> (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) {
            let reported = self.shown.lock().unwrap().get(screen).cloned();
            let recorded = match self.stale.lock().unwrap().get_mut(screen) {
                Some((old, left)) if *left > 0 => {
                    *left -= 1;
                    Some(old.clone())
                }
                _ => reported.clone(),
            };
            (recorded.clone(), reported, recorded)
        }

        pub fn place(&self, icons: Vec<Rect>) {
            *self.icons.lock().unwrap() = icons;
        }

        pub fn during(&self, work: impl FnOnce() + Send + 'static) {
            *self.hook.lock().unwrap() = Some(Box::new(work));
        }
    }

    impl Desktop for Fake {
        fn name(&self) -> String {
            "fake".into()
        }

        fn snapshot(&self) -> Result<Snapshot> {
            self.snapshots.fetch_add(1, Ordering::SeqCst);
            self.events.lock().unwrap().push(("snapshot".into(), Instant::now()));
            if let Some(reason) = self.broken.lock().unwrap().clone() {
                anyhow::bail!(reason);
            }
            Ok(Snapshot { screens: self.screens()?, icons: self.icons.lock().unwrap().clone() })
        }

        fn screens(&self) -> Result<Vec<Screen>> {
            let work = self.hook.lock().unwrap().take();
            if let Some(work) = work {
                work();
            }
            if *self.blind.lock().unwrap() {
                anyhow::bail!("No screen was found, so connect one and try again.");
            }
            Ok(self.screens.lock().unwrap().clone())
        }

        fn known(&self) -> Result<Option<Vec<String>>> {
            Ok(self.known.lock().unwrap().clone())
        }

        fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>> {
            let ours = |path: &Path| path.parent().is_some_and(|folder| folder.ends_with("out"));
            let shown = match crate::platform::agreed(|| Ok(self.look(&screen.id)), ours)? {
                crate::platform::Seen::Agreed(recorded) => recorded,
                crate::platform::Seen::Ours(path) => Some(path),
            };
            let special = shown.as_ref().is_some_and(|path| self.moving.lock().unwrap().as_ref() == Some(path) || self.lost.lock().unwrap().contains(path));
            Ok(shown.filter(|_| !special))
        }

        fn restorable(&self, screen: &Screen) -> Result<Option<PathBuf>> {
            let shown = self.shown.lock().unwrap().get(&screen.id).cloned();
            if shown.as_ref().is_some_and(|path| self.lost.lock().unwrap().contains(path)) {
                return Ok(None);
            }
            Ok(shown)
        }

        fn gallery(&self) -> Vec<PathBuf> {
            self.gallery.lock().unwrap().clone()
        }

        fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()> {
            if self.refuse.lock().unwrap().contains(&screen.id) {
                anyhow::bail!("The desktop would not take the picture.");
            }
            self.sets.fetch_add(1, Ordering::SeqCst);
            self.events.lock().unwrap().push((format!("set {}", path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()), Instant::now()));
            let lag = *self.lag.lock().unwrap();
            if let Some(old) = self.shown.lock().unwrap().get(&screen.id).cloned().filter(|_| lag > 0) {
                self.stale.lock().unwrap().insert(screen.id.clone(), (old, lag));
            }
            self.show(&screen.id, path);
            if std::mem::take(&mut *self.crash_after_set.lock().unwrap()) {
                panic!("the process was killed right after the set");
            }
            Ok(())
        }

        fn watching(&self, wake: Wake) {
            *self.watched.lock().unwrap() = Some(wake);
        }

        fn in_use(&self, folder: &Path) -> Result<Option<Vec<PathBuf>>> {
            if *self.unknowable.lock().unwrap() {
                return Ok(None);
            }
            Ok(Some(self.shown.lock().unwrap().values().filter(|path| path.starts_with(folder)).cloned().collect()))
        }

        fn run(&self, _wake: Wake) -> Result<()> {
            Ok(())
        }

        fn autostart(&self, exe: Option<&Path>) -> Result<()> {
            if exe.is_none() {
                self.unloads.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        }

        fn check(&self) -> Vec<(String, Result<String>)> {
            Vec::new()
        }

        fn format(&self) -> Format {
            Format::Jpeg
        }

        fn live(&self) -> bool {
            *self.live.lock().unwrap()
        }

        fn reduce_motion(&self) -> bool {
            *self.reduce.lock().unwrap()
        }

        fn hold(&self) -> Duration {
            Duration::from_millis(30)
        }

        fn dropped(&self) -> bool {
            std::mem::take(&mut *self.drop_seen.lock().unwrap())
        }

        fn decode(&self, path: &Path) -> Result<RgbImage> {
            self.decodes.fetch_add(1, Ordering::SeqCst);
            let work = self.mid_render.lock().unwrap().take();
            if let Some(work) = work {
                work();
            }
            Ok(scooch_core::open(path)?)
        }
    }

    pub fn screen(id: &str, x: f32, scale: f32) -> Screen {
        Screen { id: id.into(), frame: Rect { x, y: 0.0, w: 160.0, h: 100.0 }, scale }
    }

    pub fn picture(folder: &Path, name: &str) -> PathBuf {
        let path = folder.join(name);
        std::fs::write(&path, scooch_core::encode_png(&scene::sample(480, 100)).unwrap()).unwrap();
        path
    }

    pub fn rig(name: &str, screens: Vec<Screen>) -> (Daemon, Arc<Fake>, PathBuf) {
        let folder = scratch(name);
        let original = picture(&folder, "original.png");
        let fake = Fake::new(screens, &original);
        let mut daemon = Daemon::new(fake.clone(), Store::at(folder.join("data")));
        daemon.grace = Duration::ZERO;
        let mut state = State { on: true, ..State::default() };
        state.options.pads = 0.0;
        daemon.store.save(&state).unwrap();
        (daemon, fake, original)
    }

    fn subject() -> Rect {
        Rect { x: 60.0, y: 30.0, w: 40.0, h: 40.0 }
    }

    fn corner() -> Rect {
        Rect { x: 0.0, y: 0.0, w: 20.0, h: 20.0 }
    }

    fn outcome(report: &Report) -> Outcome {
        assert!(report.problem.is_none(), "{:?}", report.problem);
        report.outcomes[0].1
    }

    #[test]
    fn no_icons_show_the_picture_itself() {
        let (daemon, fake, original) = rig("plain", vec![screen("main", 0.0, 1.0)]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.source.as_path(), entry.original.as_path(), entry.output.as_deref()), (original.as_path(), original.as_path(), None));
        assert!(entry.fingerprint.is_some());
        assert_eq!(fake.showing("main"), original);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_icon_over_the_subject_renders_a_new_picture() {
        let (daemon, fake, _) = rig("render", vec![screen("main", 0.0, 2.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        let output = daemon.store.output("main", 1, "jpg");
        assert_eq!(entry.output.as_deref(), Some(output.as_path()));
        assert_eq!((entry.counter, entry.icons), (1, 1));
        assert_eq!(fake.showing("main"), output);
        let written = scooch_core::open(&output).unwrap();
        assert_eq!(written.dimensions(), (320, 200));
    }

    #[test]
    fn nothing_changed_means_nothing_happens() {
        let (daemon, fake, _) = rig("idle", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let saved = daemon.store.read();
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 1);
        assert_eq!(fake.decodes.load(Ordering::SeqCst), 1);
        assert_eq!(daemon.store.read(), saved);
    }

    #[test]
    fn a_moved_icon_draws_again_and_drops_the_old_output() {
        let (daemon, fake, _) = rig("moved", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        fake.place(Vec::new());
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert!(!daemon.store.output("main", 1, "jpg").exists());
        assert_eq!(fake.showing("main"), daemon.store.output("main", 2, "png"));
        assert_eq!(std::fs::read(fake.showing("main")).unwrap(), std::fs::read(&daemon.store.load().screens["main"].source).unwrap());
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        fake.place(vec![subject(), corner()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.icons, entry.counter), (2, 3));
        let files: Vec<_> = std::fs::read_dir(daemon.store.out()).unwrap().flatten().map(|file| file.path()).collect();
        assert_eq!(files, vec![daemon.store.output("main", 3, "jpg")]);
    }

    #[test]
    fn icons_on_calm_ground_keep_the_plain_picture_even_with_pads_on() {
        let (daemon, fake, original) = rig("calm", vec![screen("main", 0.0, 1.0)]);
        let mut state = daemon.store.load();
        state.options.pads = 0.7;
        daemon.store.save(&state).unwrap();
        fake.place(vec![corner()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert_eq!(fake.showing("main"), original);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 0);
        assert_eq!(fake.decodes.load(Ordering::SeqCst), 1);
    }

    fn stripes(folder: &Path) -> PathBuf {
        let path = folder.join("stripes.png");
        let image = RgbImage::from_fn(320, 200, |x, _| if (x / 2) % 2 == 0 { [240, 240, 240].into() } else { [15, 15, 15].into() });
        std::fs::write(&path, scooch_core::encode_png(&image).unwrap()).unwrap();
        path
    }

    fn identity_tick_decodes(pads: f32) -> (Outcome, Outcome, usize, usize, bool) {
        let folder = scratch("identity");
        let busy = stripes(&folder);
        let fake = Fake::new(vec![screen("main", 0.0, 1.0)], &busy);
        let mut daemon = Daemon::new(fake.clone(), Store::at(folder.join("data")));
        daemon.grace = Duration::ZERO;
        let mut state = State { on: true, ..State::default() };
        state.options.pads = pads;
        daemon.store.save(&state).unwrap();
        fake.place(vec![corner()]);
        let first = outcome(&daemon.tick());
        let cold = fake.decodes.load(Ordering::SeqCst);
        let _ = std::fs::remove_file(cache::pixel_file(&daemon.store, cache::key(&busy).unwrap()));
        fake.place(vec![Rect { x: 120.0, ..corner() }]);
        let second = outcome(&daemon.tick());
        let warm = fake.decodes.load(Ordering::SeqCst) - cold;
        let entry = daemon.store.load().screens["main"].clone();
        let identity = entry.placement.as_ref().is_some_and(scene::is_identity);
        (first, second, cold, warm, identity)
    }

    #[test]
    fn an_identity_tick_with_pads_on_renders_once_from_one_decode() {
        let (first, second, cold, warm, identity) = identity_tick_decodes(0.7);
        assert!(identity, "the engine should keep busy stripes centered");
        assert_eq!((first, second), (Outcome::Rendered, Outcome::Rendered));
        assert_eq!((cold, warm), (1, 1));
    }

    #[test]
    fn an_identity_tick_with_pads_off_shows_the_picture_without_decoding() {
        let (first, second, cold, warm, identity) = identity_tick_decodes(0.0);
        assert!(identity);
        assert_eq!((first, second), (Outcome::Plain, Outcome::Plain));
        assert_eq!((cold, warm), (1, 0));
    }

    #[test]
    fn a_moved_icon_reuses_the_decoded_picture() {
        let (daemon, fake, original) = rig("pixels", vec![screen("main", 0.0, 1.0)]);
        let mut state = daemon.store.load();
        state.options.pads = 0.7;
        daemon.store.save(&state).unwrap();
        fake.place(vec![subject()]);
        daemon.tick();
        assert!(cache::pixel_file(&daemon.store, cache::key(&original).unwrap()).exists());
        fake.place(vec![subject(), corner()]);
        assert!(matches!(outcome(&daemon.tick()), Outcome::Rendered | Outcome::Kept));
        assert_eq!(fake.decodes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_new_wallpaper_from_the_user_becomes_source_and_original() {
        let (daemon, fake, original) = rig("adopt", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let chosen = picture(original.parent().unwrap(), "chosen.png");
        fake.show("main", &chosen);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!(entry.source, chosen);
        assert_eq!(entry.original, chosen);
        assert_eq!(fake.showing("main"), daemon.store.output("main", 2, "jpg"));
    }

    #[test]
    fn a_space_showing_the_old_picture_gets_the_output_without_drawing() {
        let (daemon, fake, original) = rig("space", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        fake.show("main", &original);
        assert_eq!(outcome(&daemon.tick()), Outcome::Reapplied);
        assert_eq!(fake.decodes.load(Ordering::SeqCst), 1);
        assert_eq!(fake.showing("main"), daemon.store.output("main", 1, "jpg"));
        assert_eq!(daemon.store.load().screens["main"].source, original);
    }

    #[test]
    fn a_picture_chosen_with_use_is_not_undone_by_adoption() {
        let (daemon, fake, original) = rig("use", vec![screen("main", 0.0, 1.0)]);
        daemon.tick();
        let wanted = picture(original.parent().unwrap(), "wanted.png");
        let mut state = daemon.store.load();
        let entry = state.screens.get_mut("main").unwrap();
        (entry.source, entry.fingerprint, entry.placement, entry.chosen) = (wanted.clone(), None, None, true);
        daemon.store.save(&state).unwrap();
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.source.clone(), entry.original.clone(), entry.chosen), (wanted.clone(), original.clone(), false));
        assert!(daemon.store.owns(&fake.showing("main")));
        assert_eq!(std::fs::read(fake.showing("main")).unwrap(), std::fs::read(&wanted).unwrap());
        let later = picture(original.parent().unwrap(), "later.png");
        fake.show("main", &later);
        daemon.tick();
        assert_eq!(daemon.store.load().screens["main"].source, later);
    }

    #[test]
    fn a_picture_chosen_over_a_moving_default_is_shown() {
        let (daemon, fake, original) = rig("overmoving", vec![screen("main", 0.0, 1.0)]);
        *fake.moving.lock().unwrap() = Some(original.clone());
        daemon.tick();
        assert_eq!(daemon.store.load().problem.as_deref(), Some(NOT_A_FILE));
        let wanted = picture(original.parent().unwrap(), "wanted.png");
        let mut state = daemon.store.load();
        let mut entry = Entry::new(&wanted);
        (entry.original, entry.chosen) = (original.clone(), true);
        state.screens.insert("main".into(), entry);
        daemon.store.save(&state).unwrap();
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(daemon.store.owns(&fake.showing("main")));
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.source, entry.original, entry.chosen), (wanted, original, false));
    }

    #[test]
    fn a_stale_read_right_after_a_set_is_not_taken_for_a_change() {
        let (mut daemon, fake, original) = rig("stale", vec![screen("main", 0.0, 1.0)]);
        daemon.grace = Duration::from_secs(30);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let chosen = picture(original.parent().unwrap(), "chosen.png");
        fake.show("main", &chosen);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!(daemon.store.load().screens["main"].source, original);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 1);
        fake.show("main", &original);
        fake.place(Vec::new());
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        let entry = daemon.store.load().screens["main"].clone();
        assert!(entry.output.as_deref().is_some_and(|output| output.is_file() && daemon.store.owns(output)));
        assert_eq!(fake.showing("main"), entry.output.unwrap());
    }

    #[test]
    fn a_lost_output_is_drawn_again() {
        let (daemon, fake, _) = rig("lost", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        std::fs::remove_file(daemon.store.output("main", 1, "jpg")).unwrap();
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(fake.showing("main"), daemon.store.output("main", 2, "jpg"));
    }

    #[test]
    fn two_screens_with_mixed_scales_get_their_own_icons_and_outputs() {
        let (daemon, fake, _) = rig("two", vec![screen("left", 0.0, 2.0), screen("right", 160.0, 1.0)]);
        let right = Rect { x: 160.0 + 60.0, ..subject() };
        fake.place(vec![subject(), right, Rect { x: 300.0, y: 80.0, w: 10.0, h: 10.0 }]);
        let report = daemon.tick();
        assert_eq!(report.outcomes.len(), 2);
        let state = daemon.store.load();
        assert_eq!((state.screens["left"].icons, state.screens["right"].icons), (1, 2));
        assert_eq!(fake.showing("left"), daemon.store.output("left", 1, "jpg"));
        assert_eq!(fake.showing("right"), daemon.store.output("right", 1, "jpg"));
        assert_eq!(scooch_core::open(&fake.showing("left")).unwrap().dimensions(), (320, 200));
        assert_eq!(scooch_core::open(&fake.showing("right")).unwrap().dimensions(), (160, 100));
    }

    #[test]
    fn a_wallpaper_that_is_not_a_file_is_left_alone() {
        let (daemon, fake, original) = rig("folder", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        fake.show("main", original.parent().unwrap());
        assert_eq!(daemon.tick().outcomes[0].1, Outcome::Skipped);
        assert!(daemon.store.load().screens.is_empty());
        fake.show("main", &original);
        daemon.tick();
        fake.show("main", original.parent().unwrap());
        let report = daemon.tick();
        assert_eq!(report.problem.as_deref(), Some(NOT_A_FILE));
        assert_eq!(fake.sets.load(Ordering::SeqCst), 1);
        assert_eq!(fake.showing("main"), original.parent().unwrap());
    }

    #[test]
    fn an_unreadable_picture_is_reported_and_kept_out_of_the_way() {
        let (daemon, fake, original) = rig("unreadable", vec![screen("main", 0.0, 1.0)]);
        let broken = original.parent().unwrap().join("broken.png");
        std::fs::write(&broken, b"not a picture").unwrap();
        fake.show("main", &broken);
        let report = daemon.tick();
        assert!(report.problem.as_deref().is_some_and(|problem| problem.ends_with('.') && !problem.contains(": ")));
        assert_eq!(daemon.store.load().problem, report.problem);
        fake.show("main", &original);
        let mut state = daemon.store.load();
        state.screens.clear();
        daemon.store.save(&state).unwrap();
        assert!(daemon.tick().problem.is_none());
        assert_eq!(daemon.store.load().problem, None);
    }

    #[test]
    fn a_state_edited_during_a_tick_is_merged_not_overwritten() {
        let (daemon, fake, _) = rig("race", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        let store = daemon.store.clone();
        fake.during(move || {
            let mut edited = store.load();
            edited.options.strength = 0.5;
            store.save(&edited).unwrap();
        });
        assert!(daemon.tick().retry);
        let state = daemon.store.load();
        assert_eq!(state.options.strength, 0.5);
        assert_eq!(state.screens["main"].output.as_ref(), Some(&fake.showing("main")));
        assert!(!daemon.tick().retry);
        assert_eq!(daemon.store.load().screens["main"].output.as_ref(), Some(&fake.showing("main")));
    }

    #[test]
    fn a_picture_switched_during_a_tick_survives_the_merge() {
        let mut mine = State { on: true, ..State::default() };
        mine.screens.insert("main".into(), Entry { output: Some("/d/out/main-4.jpg".into()), counter: 4, fingerprint: Some("f".into()), icons: 2, ..Entry::new(Path::new("/p/old.jpg")) });
        let mut before = State { on: true, ..State::default() };
        before.screens.insert("main".into(), Entry { counter: 3, ..Entry::new(Path::new("/p/old.jpg")) });
        let mut fresh = before.clone();
        fresh.screens.get_mut("main").unwrap().source = "/p/new.jpg".into();
        let merged = merge(&mine, &before, &fresh);
        let entry = &merged.screens["main"];
        assert_eq!(entry.source, PathBuf::from("/p/new.jpg"));
        assert_eq!(entry.fingerprint, None);
        assert_eq!((entry.counter, entry.output.clone()), (4, Some(PathBuf::from("/d/out/main-4.jpg"))));
        let adopted = State { screens: [("main".to_string(), Entry::new(Path::new("/p/picked.jpg")))].into(), ..mine.clone() };
        let options = State { options: Options { strength: 0.5, ..Options::default() }, ..before.clone() };
        let merged = merge(&adopted, &before, &options);
        assert_eq!(merged.screens["main"].source, PathBuf::from("/p/picked.jpg"));
        assert_eq!(merged.options.strength, 0.5);
    }

    #[test]
    fn a_tick_never_writes_over_an_unreadable_state_file() {
        let (daemon, fake, _) = rig("unreadable-state", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        std::fs::write(daemon.store.state_file(), b"{ broken").unwrap();
        daemon.tick();
        assert_eq!(std::fs::read(daemon.store.aside_file()).unwrap(), b"{ broken");
    }

    #[test]
    fn adoption_ignores_outputs_and_fresh_sources() {
        let folder = scratch("adoptable");
        let picture = folder.join("a.jpg");
        std::fs::write(&picture, b"x").unwrap();
        let mut entry = Entry::new(&folder.join("b.jpg"));
        assert!(adoptable(&entry, &picture, false));
        assert!(!adoptable(&entry, &picture, true));
        assert!(!adoptable(&entry, &folder, false));
        entry.source = picture.clone();
        assert!(!adoptable(&entry, &picture, false));
    }

    fn luma_sampling(bytes: &[u8]) -> Option<u8> {
        let start = bytes.windows(2).position(|pair| pair == [0xFF, 0xC0])?;
        bytes.get(start + 11).copied()
    }

    #[test]
    fn stills_that_may_glide_keep_full_color_detail() {
        let picture = scene::sample(64, 40);
        assert_eq!(luma_sampling(&encode(&picture, Format::Jpeg, None, false).unwrap()), Some(0x22));
        assert_eq!(luma_sampling(&encode(&picture, Format::Jpeg, None, true).unwrap()), Some(0x11));
    }

    #[test]
    fn jpeg_output_decodes_to_the_same_size_and_close_colors() {
        let picture = scene::sample(301, 157);
        let bytes = encode(&picture, Format::Jpeg, None, false).unwrap();
        assert!(!bytes.windows(12).any(|window| window == b"ICC_PROFILE\0"));
        let marked = encode(&picture, Format::Jpeg, Some(&[7u8; 300]), false).unwrap();
        assert!(marked.windows(12).any(|window| window == b"ICC_PROFILE\0"));
        let folder = scratch("jpeg");
        std::fs::write(folder.join("a.jpg"), &bytes).unwrap();
        let back = scooch_core::open(&folder.join("a.jpg")).unwrap();
        assert_eq!(back.dimensions(), (301, 157));
        let error: f64 = picture.as_raw().iter().zip(back.as_raw()).map(|(a, b)| (*a as f64 - *b as f64).abs()).sum::<f64>() / picture.as_raw().len() as f64;
        assert!(error < 3.0, "{error}");
    }

    #[test]
    fn flipping_is_tried_only_when_icons_hide_enough() {
        assert!(!worth_flipping(0.05, false));
        assert!(worth_flipping(0.051, false));
        assert!(!worth_flipping(0.4, true));
        assert!(flip_helps(0.4, 0.19));
        assert!(!flip_helps(0.4, 0.2));
        assert!(!flip_helps(0.4, 0.35));
    }

    #[test]
    fn a_tick_records_whether_flipping_would_help() {
        let folder = scratch("edge");
        let edge = folder.join("edge.png");
        let image = RgbImage::from_fn(160, 100, |x, y| {
            let (dx, dy) = (x as f32 - 25.0, y as f32 - 50.0);
            if dx * dx + dy * dy < 625.0 { [250, 240, 220].into() } else { [12, 18, 30].into() }
        });
        std::fs::write(&edge, scooch_core::encode_png(&image).unwrap()).unwrap();
        let fake = Fake::new(vec![screen("main", 0.0, 1.0)], &edge);
        let mut edgy = Daemon::new(fake.clone(), Store::at(folder.join("data")));
        edgy.grace = Duration::ZERO;
        edgy.store.save(&State { on: true, ..State::default() }).unwrap();
        fake.place(vec![Rect { x: 0.0, y: 5.0, w: 60.0, h: 90.0 }]);
        edgy.tick();
        assert!(edgy.store.load().screens["main"].flip_helps);
        let (daemon, fake, _) = rig("fliphelp", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![Rect { x: 0.0, y: 0.0, w: 160.0, h: 100.0 }]);
        daemon.tick();
        assert!(!daemon.store.load().screens["main"].flip_helps);
        let mut state = daemon.store.load();
        state.options.mirror = true;
        daemon.store.save(&state).unwrap();
        fake.place(vec![subject()]);
        daemon.tick();
        assert!(!daemon.store.load().screens["main"].flip_helps);
    }

    #[test]
    fn a_desktop_that_cannot_be_read_keeps_the_picture_and_the_fingerprint() {
        let (daemon, fake, _) = rig("finderoff", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let before = daemon.store.load().screens["main"].clone();
        let showing = fake.showing("main");
        *fake.broken.lock().unwrap() = Some("Finder is not running, so Scooch keeps the picture as it is until Finder starts.".into());
        fake.place(Vec::new());
        let report = daemon.tick();
        assert!(report.problem.as_deref().is_some_and(|problem| problem.starts_with("Finder is not running")));
        assert_eq!(daemon.store.load().screens["main"].fingerprint, before.fingerprint);
        assert_eq!(fake.showing("main"), showing);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_pick_made_during_a_render_is_never_overwritten() {
        let (daemon, fake, original) = rig("midpick", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        let chosen = picture(original.parent().unwrap(), "chosen.png");
        let (picker, picked) = (fake.clone(), chosen.clone());
        *fake.mid_render.lock().unwrap() = Some(Box::new(move || picker.show("main", &picked)));
        let report = daemon.tick();
        assert!(report.retry);
        assert_eq!(report.outcomes[0].1, Outcome::Interrupted);
        assert_eq!(fake.showing("main"), chosen);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 0);
        assert!(std::fs::read_dir(daemon.store.out()).map_or(0, |files| files.count()) == 0);
        assert_eq!(daemon.store.load().screens["main"].fingerprint, None);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.source, entry.original), (chosen.clone(), chosen));
    }

    #[test]
    fn the_settling_grace_follows_the_adapter() {
        let folder = scratch("grace");
        let fake = Fake::new(vec![screen("main", 0.0, 1.0)], &folder.join("x.png"));
        assert_eq!(Daemon::new(fake, Store::at(&folder)).grace, Duration::from_secs(2));
    }

    #[test]
    fn two_spaces_keep_their_own_pictures_and_files() {
        let (daemon, fake, original) = rig("spaces", vec![screen("D@S1", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let other = picture(original.parent().unwrap(), "other.png");
        fake.connect(vec![screen("D@S2", 0.0, 1.0)]);
        fake.show("D@S2", &other);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let state = daemon.store.load();
        assert_eq!(state.screens["D@S1"].source, original);
        assert_eq!(state.screens["D@S2"].source, other);
        assert!(daemon.store.output("D@S1", 1, "jpg").exists());
        assert!(daemon.store.output("D@S2", 1, "jpg").exists());
        assert_ne!(daemon.store.output("D@S1", 1, "jpg"), daemon.store.output("D@S2", 1, "jpg"));
    }

    #[test]
    fn a_space_first_seen_after_use_gets_the_chosen_picture() {
        let (daemon, fake, original) = rig("unpainted", vec![screen("D@S1", 0.0, 1.0)]);
        let wanted = picture(original.parent().unwrap(), "wanted.png");
        let mut state = daemon.store.load();
        let mut entry = Entry::new(&wanted);
        (entry.original, entry.chosen) = (original.clone(), true);
        state.screens.insert("D@S1".into(), entry);
        state.picture = Some(wanted.clone());
        daemon.store.save(&state).unwrap();
        fake.place(vec![subject()]);
        daemon.tick();
        let other = picture(original.parent().unwrap(), "other.png");
        fake.connect(vec![screen("D@S2", 0.0, 1.0)]);
        fake.show("D@S2", &other);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let state = daemon.store.load();
        assert_eq!((state.screens["D@S2"].source.clone(), state.screens["D@S2"].original.clone()), (wanted.clone(), other));
        assert_eq!(state.screens["D@S1"].source, wanted);
        assert!(daemon.store.owns(&fake.showing("D@S2")));
    }

    #[test]
    fn a_screen_without_its_space_is_left_alone_once_spaces_are_known() {
        let (daemon, fake, _) = rig("bare", vec![screen("D@S1", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        fake.connect(vec![screen("D", 0.0, 1.0)]);
        let report = daemon.tick();
        assert_eq!(report.outcomes[0].1, Outcome::Skipped);
        assert_eq!(report.problem.as_deref(), Some(SPACE_UNKNOWN));
        assert!(!daemon.store.load().screens.contains_key("D"));
    }

    #[test]
    fn sweeping_one_space_leaves_the_other_spaces_files() {
        let store = Store::at(scratch("sweepspaces"));
        std::fs::create_dir_all(store.out()).unwrap();
        let (one, two, bare) = (store.output("D@S1", 3, "jpg"), store.output("D@S2", 3, "jpg"), store.output("D", 3, "jpg"));
        for path in [&one, &two, &bare] {
            std::fs::write(path, b"x").unwrap();
        }
        let fake = Fake::new(vec![screen("D@S1", 0.0, 1.0)], &one);
        fake.show("D@S1", Path::new("/p/elsewhere.jpg"));
        let state = State::default();
        sweep(fake.as_ref(), &store, &state, "D@S1", &[]);
        assert!(!one.exists() && two.exists() && bare.exists());
        sweep(fake.as_ref(), &store, &state, "D", &[]);
        assert!(two.exists() && !bare.exists());
    }

    #[test]
    fn a_daemon_with_nothing_to_do_exits() {
        let (daemon, _, _) = rig("exit", vec![screen("main", 0.0, 1.0)]);
        daemon.store.save(&State::default()).unwrap();
        let report = daemon.tick();
        assert!(report.exit && !report.unload);
    }

    #[test]
    fn adoption_makes_a_screen_restorable_again() {
        let (daemon, fake, original) = rig("readopt", vec![screen("main", 0.0, 1.0)]);
        let mut state = daemon.store.load();
        let mut entry = Entry::new(&original);
        (entry.unrestorable, entry.fingerprint) = (true, Some("x".into()));
        state.screens.insert("main".into(), entry);
        daemon.store.save(&state).unwrap();
        let mine = picture(original.parent().unwrap(), "mine.png");
        fake.show("main", &mine);
        daemon.tick();
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.source, entry.unrestorable), (mine, false));
    }

    #[test]
    fn merge_does_not_bring_back_an_entry_a_command_removed() {
        let mut before = State { on: true, ..State::default() };
        before.screens.insert("A".into(), Entry::new(Path::new("/p/a.jpg")));
        before.screens.insert("B".into(), Entry::new(Path::new("/p/b.jpg")));
        let mut mine = before.clone();
        mine.screens.get_mut("A").unwrap().counter = 5;
        mine.screens.remove("B");
        let mut fresh = before.clone();
        fresh.screens.remove("A");
        fresh.on = false;
        let merged = merge(&mine, &before, &fresh);
        assert!(!merged.screens.contains_key("A") && !merged.screens.contains_key("B"));
        assert!(!merged.on);
    }

    #[test]
    fn a_bare_id_is_never_a_gone_space() {
        let screens = [screen("D@S1", 0.0, 1.0)];
        let known = ["D@S1".to_string()];
        assert!(!gone("D", Some(&known), &screens));
        assert!(gone("D@S2", Some(&known), &screens));
        assert!(!gone("D@S2", None, &screens));
        assert!(!gone("E@S2", Some(&known), &screens));
    }

    #[test]
    fn a_tick_moves_an_entry_made_under_a_bare_id_to_its_space() {
        let (daemon, fake, original) = rig("relink", vec![screen("D@S1", 0.0, 1.0)]);
        std::fs::create_dir_all(daemon.store.out()).unwrap();
        let output = daemon.store.output("D", 1, "jpg");
        std::fs::write(&output, b"x").unwrap();
        let mut state = daemon.store.load();
        state.screens.insert("D".into(), Entry { output: Some(output.clone()), counter: 1, ..Entry::new(&original) });
        daemon.store.save(&state).unwrap();
        fake.show("D@S1", &output);
        let report = daemon.tick();
        assert!(report.problem.is_none(), "{:?}", report.problem);
        let state = daemon.store.load();
        assert!(!state.screens.contains_key("D"));
        assert_eq!(state.screens["D@S1"].original, original);
    }

    #[test]
    fn a_tick_leaves_its_own_picture_alone_where_it_has_no_record() {
        let (daemon, fake, _) = rig("stray", vec![screen("D@S1", 0.0, 1.0)]);
        std::fs::create_dir_all(daemon.store.out()).unwrap();
        let stray = daemon.store.output("D", 3, "jpg");
        std::fs::write(&stray, b"x").unwrap();
        fake.show("D@S1", &stray);
        let report = daemon.tick();
        assert_eq!(report.problem.as_deref(), Some(ORPHAN));
        assert_eq!(fake.showing("D@S1"), stray);
        let state = daemon.store.load();
        assert!(stray.exists() && state.screens.is_empty());
        assert_eq!(state.kept, vec![stray.clone()]);
        assert_eq!(state.problem.as_deref(), Some(ORPHAN));
    }

    #[test]
    fn use_during_the_tick_that_first_sees_a_space_keeps_the_entry_use_made() {
        let (daemon, fake, original) = rig("usenewspace", vec![screen("D@S1", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let wanted = picture(original.parent().unwrap(), "wanted.png");
        let other = picture(original.parent().unwrap(), "other.png");
        fake.connect(vec![screen("D@S1", 0.0, 1.0), screen("D@S2", 0.0, 1.0)]);
        fake.show("D@S2", &other);
        let (store, desktop, chosen) = (daemon.store.clone(), fake.clone(), wanted.clone());
        fake.during(move || {
            let mut state = store.load();
            crate::commands::choose_picture(&mut state, desktop.as_ref(), &store, &chosen).unwrap();
            store.save(&state).unwrap();
        });
        daemon.tick();
        let state = daemon.store.load();
        let entry = &state.screens["D@S2"];
        assert_eq!((entry.source.clone(), entry.original.clone(), entry.chosen), (wanted.clone(), other.clone(), true));
        assert_eq!(state.screens["D@S1"].source, wanted);
        daemon.tick();
        let state = daemon.store.load();
        assert_eq!((state.screens["D@S2"].source.clone(), state.screens["D@S2"].original.clone()), (wanted, other));
    }

    #[test]
    fn merge_keeps_what_a_command_wrote_for_an_id_the_tick_never_saw() {
        let before = State { on: true, ..State::default() };
        let mut mine = before.clone();
        mine.screens.insert("D@S2".into(), Entry { output: Some("/d/out/D_S2-1.jpg".into()), counter: 1, icons: 3, fingerprint: Some("f".into()), ..Entry::new(Path::new("/p/old.jpg")) });
        let mut fresh = before.clone();
        fresh.screens.insert("D@S2".into(), Entry { chosen: true, original: "/p/old.jpg".into(), ..Entry::new(Path::new("/p/new.jpg")) });
        let merged = merge(&mine, &before, &fresh);
        let entry = &merged.screens["D@S2"];
        assert_eq!((entry.source.clone(), entry.original.clone(), entry.chosen), (PathBuf::from("/p/new.jpg"), PathBuf::from("/p/old.jpg"), true));
        assert_eq!((entry.output.clone(), entry.counter, entry.icons, entry.fingerprint.clone()), (Some(PathBuf::from("/d/out/D_S2-1.jpg")), 1, 3, None));
    }

    #[test]
    fn a_space_is_forgotten_only_after_a_day_and_five_ticks() {
        let start = 1_000_000;
        let mut slow = Entry::new(Path::new("/p/a.jpg"));
        for tick in 0..4 {
            assert!(!absence(&mut slow, false, start + tick * ABSENT_FOR));
        }
        assert!(absence(&mut slow, false, start + 4 * ABSENT_FOR));
        let mut busy = Entry::new(Path::new("/p/a.jpg"));
        for tick in 0..20 {
            assert!(!absence(&mut busy, false, start + tick));
        }
        assert!(!absence(&mut busy, false, start + ABSENT_FOR));
        assert!(absence(&mut busy, false, start + ABSENT_FOR + 1));
        let mut back = Entry::new(Path::new("/p/a.jpg"));
        for tick in 0..6 {
            absence(&mut back, false, start + tick);
        }
        assert!(!absence(&mut back, true, start + 2 * ABSENT_FOR));
        assert_eq!((back.absent_since, back.absent_ticks), (None, 0));
        assert!(!absence(&mut back, false, start + 3 * ABSENT_FOR));
        assert_eq!((back.absent_since, back.absent_ticks), (Some(start + 3 * ABSENT_FOR), 1));
    }

    #[test]
    fn a_showing_space_is_never_marked_absent() {
        let store = Store::at(scratch("showingpresent"));
        let fake = Fake::new(vec![screen("D@S1", 0.0, 1.0)], Path::new("/p/a.jpg"));
        let mut state = State::default();
        state.screens.insert("D@S1".into(), Entry::new(Path::new("/p/a.jpg")));
        for tick in 0..10 {
            assert_eq!(prune(&mut state, fake.as_ref(), &store, Some(&[]), &[screen("D@S1", 0.0, 1.0)], tick * ABSENT_FOR), 0);
        }
        assert_eq!((state.screens["D@S1"].absent_since, state.screens["D@S1"].absent_ticks), (None, 0));
    }

    #[test]
    fn a_tick_marks_a_missing_space_and_forgets_it_only_when_long_gone() {
        let (daemon, fake, original) = rig("absent", vec![screen("D@S1", 0.0, 1.0)]);
        std::fs::create_dir_all(daemon.store.out()).unwrap();
        let output = daemon.store.output("D@S2", 1, "jpg");
        std::fs::write(&output, b"x").unwrap();
        let mut state = daemon.store.load();
        state.screens.insert("D@S2".into(), Entry { output: Some(output.clone()), ..Entry::new(&original) });
        daemon.store.save(&state).unwrap();
        *fake.known.lock().unwrap() = Some(vec!["D@S1".into()]);
        daemon.tick();
        let entry = daemon.store.load().screens["D@S2"].clone();
        assert!(entry.absent_since.is_some() && entry.absent_ticks == 1 && output.exists());
        *fake.known.lock().unwrap() = Some(vec!["D@S1".into(), "D@S2".into()]);
        daemon.tick();
        let entry = daemon.store.load().screens["D@S2"].clone();
        assert_eq!((entry.absent_since, entry.absent_ticks), (None, 0));
        *fake.known.lock().unwrap() = None;
        daemon.tick();
        assert_eq!(daemon.store.load().screens["D@S2"].absent_ticks, 0);
        let mut state = daemon.store.load();
        let entry = state.screens.get_mut("D@S2").unwrap();
        (entry.absent_since, entry.absent_ticks) = (Some(1), 4);
        daemon.store.save(&state).unwrap();
        *fake.known.lock().unwrap() = Some(vec!["D@S1".into()]);
        daemon.tick();
        assert!(!daemon.store.load().screens.contains_key("D@S2") && !output.exists());
    }

    #[test]
    fn without_an_answer_only_the_cleaned_entrys_own_files_go() {
        let store = Store::at(scratch("unknowable"));
        std::fs::create_dir_all(store.out()).unwrap();
        let (mine, older, theirs) = (store.output("A", 1, "jpg"), store.output("A", 2, "jpg"), store.output("B", 1, "jpg"));
        for path in [&mine, &older, &theirs] {
            std::fs::write(path, b"x").unwrap();
        }
        let fake = Fake::new(vec![screen("A", 0.0, 1.0)], Path::new("/p/a.jpg"));
        *fake.unknowable.lock().unwrap() = true;
        discard(fake.as_ref(), &store, &[mine.clone(), theirs.clone()], std::slice::from_ref(&mine));
        assert!(!mine.exists() && theirs.exists());
        *fake.unknowable.lock().unwrap() = false;
        fake.show("B", &theirs);
        discard(fake.as_ref(), &store, &[older.clone(), theirs.clone()], &[]);
        assert!(!older.exists() && theirs.exists());
    }

    #[test]
    fn a_failed_set_takes_the_new_path_back_out_of_the_record() {
        let (daemon, fake, original) = rig("failedset", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        fake.refuse.lock().unwrap().push("main".into());
        assert!(daemon.tick().problem.is_some());
        assert_eq!(daemon.store.load().screens["main"].output, None);
        assert!(daemon.store.pictures().is_empty());
        assert_eq!(fake.showing("main"), original);
    }

    #[test]
    fn the_record_is_saved_before_the_set_and_a_tick_still_merges_commands() {
        let (daemon, fake, original) = rig("savefirst", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        let (store, flipped) = (daemon.store.clone(), Arc::new(Mutex::new(None)));
        let seen = flipped.clone();
        *fake.mid_render.lock().unwrap() = Some(Box::new(move || {
            let mut state = store.load();
            state.options.mirror = true;
            store.save(&state).unwrap();
            *seen.lock().unwrap() = Some(());
        }));
        let report = daemon.tick();
        assert!(report.retry && flipped.lock().unwrap().is_some());
        let state = daemon.store.load();
        assert!(state.options.mirror);
        assert_eq!(state.screens["main"].output.as_deref(), Some(fake.showing("main").as_path()));
        assert_eq!(state.screens["main"].original, original);
    }

    #[test]
    fn two_spaces_sharing_one_file_keep_it_while_either_shows_it() {
        let (daemon, fake, _) = rig("sharing", vec![screen("D@S1", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let shared = fake.showing("D@S1");
        assert!(daemon.store.owns(&shared));
        *fake.known.lock().unwrap() = Some(vec!["D@S1".into(), "D@S2".into()]);
        fake.connect(vec![screen("D@S2", 0.0, 1.0)]);
        fake.show("D@S2", &shared);
        let report = daemon.tick();
        assert!(report.problem.is_none(), "{:?}", report.problem);
        let state = daemon.store.load();
        assert_eq!(state.screens.keys().cloned().collect::<Vec<_>>(), vec!["D@S1".to_string(), "D@S2".to_string()]);
        fake.place(vec![corner()]);
        daemon.tick();
        assert_ne!(fake.showing("D@S2"), shared);
        assert!(shared.exists());
        assert_eq!(daemon.store.load().screens["D@S1"].output.as_deref(), Some(shared.as_path()));
        fake.connect(vec![screen("D@S1", 0.0, 1.0)]);
        daemon.tick();
        assert_ne!(fake.showing("D@S1"), shared);
        assert!(!shared.exists());
    }

    #[test]
    fn an_entry_moves_only_when_its_owner_is_bare_or_gone() {
        let shown = Path::new("/d/out/D_S1-1.jpg");
        let state = || {
            let mut state = State::default();
            state.screens.insert("D@S1".into(), Entry { output: Some(shown.to_path_buf()), ..Entry::new(Path::new("/p/a.jpg")) });
            state
        };
        let screens = [screen("D@S2", 0.0, 1.0)];
        let mut live = state();
        assert!(relink(&mut live, "D@S2", shown, Some(&["D@S1".to_string(), "D@S2".to_string()]), &screens));
        assert!(live.screens.contains_key("D@S1") && live.screens.contains_key("D@S2"));
        let mut unsure = state();
        assert!(relink(&mut unsure, "D@S2", shown, None, &screens));
        assert!(unsure.screens.contains_key("D@S1"));
        let mut closed = state();
        assert!(relink(&mut closed, "D@S2", shown, Some(&["D@S2".to_string()]), &screens));
        assert!(!closed.screens.contains_key("D@S1") && closed.screens.contains_key("D@S2"));
    }

    #[test]
    fn a_tick_right_after_a_render_never_adopts_the_old_picture() {
        let (daemon, fake, original) = rig("lagtick", vec![screen("main", 0.0, 1.0)]);
        let chosen = picture(original.parent().unwrap(), "chosen.png");
        let mut state = daemon.store.load();
        crate::commands::choose_picture(&mut state, fake.as_ref(), &daemon.store, &chosen).unwrap();
        daemon.store.save(&state).unwrap();
        *fake.lag.lock().unwrap() = 3;
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let render = fake.showing("main");
        fake.place(vec![corner()]);
        daemon.tick();
        let state = daemon.store.load();
        assert_eq!((state.screens["main"].source.clone(), state.screens["main"].original.clone()), (chosen, original));
        assert_ne!(fake.showing("main"), render);
    }

    #[test]
    fn a_daemon_with_nothing_to_watch_takes_its_login_item_away() {
        let (daemon, fake, _) = rig("release", vec![screen("main", 0.0, 1.0)]);
        let unloads = || fake.unloads.load(Ordering::SeqCst);
        release(fake.as_ref(), &daemon.store, &Report { exit: true, ..Report::default() }, &Stop::default());
        assert_eq!(unloads(), 1);
        let asked = Stop::default();
        asked.request();
        release(fake.as_ref(), &daemon.store, &Report { exit: true, ..Report::default() }, &asked);
        assert_eq!(unloads(), 1);
        release(fake.as_ref(), &daemon.store, &Report { unload: true, ..Report::default() }, &asked);
        assert_eq!(unloads(), 2);
        release(fake.as_ref(), &daemon.store, &Report::default(), &Stop::default());
        assert_eq!(unloads(), 2);
    }

    #[test]
    fn a_new_daemon_waits_for_the_old_one_to_let_go() {
        let store = Store::at(scratch("handover"));
        let old = store.lock().unwrap().unwrap();
        assert!(hold(&store, Duration::from_millis(200), Duration::from_millis(20)).unwrap().is_none());
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(old);
        });
        assert!(hold(&store, Duration::from_secs(5), Duration::from_millis(20)).unwrap().is_some());
        letting_go.join().unwrap();
        assert_eq!(LOCK_PATIENCE, Duration::from_secs(7));
        assert_eq!(gave_up(&State { on: true, ..State::default() }).unwrap_err().to_string(), ALREADY);
        assert!(gave_up(&State { restoring: true, ..State::default() }).is_err());
        assert!(gave_up(&State::default()).is_ok());
    }

    #[test]
    fn an_answer_to_a_consent_question_wakes_the_daemon() {
        let fake = Fake::new(vec![screen("main", 0.0, 1.0)], Path::new("/p/a.jpg"));
        let signal = Arc::new(Signal::default());
        prime(fake.as_ref(), &signal);
        let wake = fake.watched.lock().unwrap().take().expect("the daemon handed the adapter a wake before ticking");
        wake();
        let (sender, receiver) = std::sync::mpsc::channel();
        let waiting = signal.clone();
        std::thread::spawn(move || {
            waiting.settle(Duration::ZERO, Duration::ZERO);
            let _ = sender.send(());
        });
        assert!(receiver.recv_timeout(Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn a_stop_waits_for_the_running_tick() {
        let stop = Arc::new(Stop::default());
        assert!(stop.begin());
        let ticking = stop.clone();
        let tick = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            ticking.end();
        });
        let started = Instant::now();
        stop.request();
        assert!(stop.wait_idle(Duration::from_secs(5)));
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert!(!stop.begin());
        tick.join().unwrap();
    }

    #[test]
    fn a_daemon_asked_to_stop_is_never_found_as_the_running_one() {
        let store = Store::at(scratch("retire"));
        let held = store.lock().unwrap();
        assert!(held.is_some());
        assert_eq!(store.watcher(), Some(std::process::id()));
        let stop = Stop::default();
        retire(&store, &stop);
        assert!(stop.requested());
        assert_eq!(store.watcher(), None);
    }

    #[test]
    fn a_stop_gives_up_waiting_at_the_cap() {
        let stop = Stop::default();
        assert!(stop.begin());
        stop.request();
        assert!(!stop.wait_idle(Duration::from_millis(100)));
    }

    #[cfg(unix)]
    #[test]
    fn a_state_that_cannot_be_saved_ends_the_daemon_quietly() {
        use std::os::unix::fs::PermissionsExt;
        let (daemon, fake, _) = rig("readonly", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        std::fs::set_permissions(&daemon.store.root, std::fs::Permissions::from_mode(0o555)).unwrap();
        let report = daemon.tick();
        std::fs::set_permissions(&daemon.store.root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(report.unsaved);
        assert_eq!(after(&report), Next::Quit);
        assert_eq!(after(&Report::default()), Next::Go);
    }

    #[test]
    fn settle_turns_a_burst_into_one_tick() {
        let signal = Signal::default();
        for _ in 0..50 {
            signal.wake();
        }
        let started = Instant::now();
        signal.settle(Duration::from_millis(40), Duration::from_secs(5));
        assert!(started.elapsed() >= Duration::from_millis(30));
        assert!(signal.pending.lock().unwrap().last.is_none());
    }

    #[test]
    fn settle_waits_for_quiet() {
        let signal = Arc::new(Signal::default());
        let feeder = signal.clone();
        let started = Instant::now();
        let feeding = std::thread::spawn(move || {
            for _ in 0..5 {
                feeder.wake();
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        signal.settle(Duration::from_millis(120), Duration::from_secs(5));
        feeding.join().unwrap();
        assert!(started.elapsed() >= Duration::from_millis(120 + 4 * 30 - 10), "{:?}", started.elapsed());
    }

    #[test]
    fn settle_stops_waiting_at_the_limit() {
        let signal = Arc::new(Signal::default());
        let feeder = signal.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = done.clone();
        let feeding = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                feeder.wake();
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let started = Instant::now();
        signal.settle(Duration::from_millis(100), Duration::from_millis(300));
        let waited = started.elapsed();
        done.store(true, Ordering::SeqCst);
        feeding.join().unwrap();
        assert!(waited >= Duration::from_millis(290) && waited < Duration::from_millis(600), "{waited:?}");
    }

    #[test]
    fn settle_sleeps_until_the_first_wake() {
        let signal = Arc::new(Signal::default());
        let waiter = signal.clone();
        let started = Instant::now();
        let waiting = std::thread::spawn(move || {
            waiter.settle(Duration::from_millis(20), Duration::from_secs(1));
            started.elapsed()
        });
        std::thread::sleep(Duration::from_millis(150));
        signal.wake();
        let waited = waiting.join().unwrap();
        assert!(waited >= Duration::from_millis(170), "{waited:?}");
    }

    fn scenery_rig(name: &str, on: bool) -> (Daemon, Arc<Fake>, PathBuf) {
        let (daemon, fake, original) = rig(name, vec![screen("main", 0.0, 2.0)]);
        let mut state = daemon.store.load();
        state.options.scenery = if on { scooch_core::Scenery::Auto } else { scooch_core::Scenery::Off };
        daemon.store.save(&state).unwrap();
        (daemon, fake, original)
    }

    fn set_scenery(daemon: &Daemon, on: bool) {
        let mut state = daemon.store.load();
        state.options.scenery = if on { scooch_core::Scenery::Auto } else { scooch_core::Scenery::Off };
        daemon.store.save(&state).unwrap();
    }

    fn calm() -> Rect {
        Rect { x: 4.0, y: 4.0, w: 20.0, h: 20.0 }
    }

    fn calm_moved() -> Rect {
        Rect { x: 4.0, y: 70.0, w: 20.0, h: 20.0 }
    }

    fn differs_near(a: &RgbImage, b: &RgbImage, icon: &Rect, scale: f32) -> usize {
        let margin = 12.0;
        let x0 = ((icon.x - margin) * scale).max(0.0) as u32;
        let y0 = ((icon.y - margin) * scale).max(0.0) as u32;
        let x1 = (((icon.x + icon.w + margin) * scale) as u32).min(a.width());
        let y1 = (((icon.y + icon.h + margin) * scale) as u32).min(a.height());
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|(x, y)| a.get_pixel(*x, *y).0.iter().zip(b.get_pixel(*x, *y).0.iter()).any(|(p, q)| p.abs_diff(*q) > 6)).count()
    }

    #[test]
    fn with_scenery_on_a_moved_icon_on_calm_ground_renders_again() {
        let (daemon, fake, _) = scenery_rig("scenery-move", true);
        fake.place(vec![subject(), calm()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let first = daemon.store.load().screens["main"].clone();
        let before = scooch_core::open(first.output.as_ref().unwrap()).unwrap();
        fake.place(vec![subject(), calm_moved()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let second = daemon.store.load().screens["main"].clone();
        assert_eq!(second.placement, first.placement);
        assert_ne!(second.output, first.output);
        assert!(second.painted);
        let after = scooch_core::open(second.output.as_ref().unwrap()).unwrap();
        assert!(differs_near(&before, &after, &calm(), 2.0) > 0);
        assert!(differs_near(&before, &after, &calm_moved(), 2.0) > 0);
    }

    #[test]
    fn with_scenery_off_the_same_move_is_kept() {
        let (daemon, fake, _) = scenery_rig("scenery-kept", false);
        fake.place(vec![subject(), calm()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let first = daemon.store.load().screens["main"].clone();
        fake.place(vec![subject(), calm_moved()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Kept);
        let second = daemon.store.load().screens["main"].clone();
        assert_eq!((second.output, second.placement, second.painted), (first.output, first.placement, false));
    }

    #[test]
    fn switching_scenery_renders_again_both_ways_and_off_matches_today() {
        let (today, today_fake, _) = scenery_rig("scenery-today", false);
        today_fake.place(vec![subject(), calm()]);
        today.tick();
        let expected = std::fs::read(today_fake.showing("main")).unwrap();
        let (daemon, fake, _) = scenery_rig("scenery-switch", false);
        fake.place(vec![subject(), calm()]);
        daemon.tick();
        assert_eq!(std::fs::read(fake.showing("main")).unwrap(), expected);
        set_scenery(&daemon, true);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let painted = std::fs::read(fake.showing("main")).unwrap();
        assert_ne!(painted, expected);
        assert!(daemon.store.load().screens["main"].painted);
        set_scenery(&daemon, false);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(std::fs::read(fake.showing("main")).unwrap(), expected);
        assert!(!daemon.store.load().screens["main"].painted);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
    }

    #[test]
    fn switching_scenery_off_on_calm_ground_shows_the_picture_itself_again() {
        let (daemon, fake, original) = scenery_rig("scenery-plain", true);
        fake.place(vec![calm()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(daemon.store.load().screens["main"].painted);
        set_scenery(&daemon, false);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert_eq!(std::fs::read(fake.showing("main")).unwrap(), std::fs::read(&original).unwrap());
        assert!(!daemon.store.load().screens["main"].painted);
    }

    #[test]
    fn scenery_with_no_icons_keeps_the_picture_itself() {
        let (daemon, fake, original) = scenery_rig("scenery-empty", true);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert_eq!(fake.showing("main"), original);
        assert_eq!(fake.decodes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_restore_after_scenery_brings_back_the_users_own_picture() {
        let (daemon, fake, original) = scenery_rig("scenery-off", true);
        fake.place(vec![subject(), calm()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(daemon.store.load().screens["main"].painted);
        fake.connect(Vec::new());
        assert!(matches!(crate::commands::wind_down(fake.as_ref(), &daemon.store).unwrap(), crate::commands::Off::Pending(_)));
        fake.connect(vec![screen("main", 0.0, 2.0)]);
        assert!(daemon.tick().unload);
        assert_eq!(fake.showing("main"), original);
    }

    type Rigged = (Daemon, Arc<Fake>, Arc<Mutex<crate::glide::tests::Script>>, Arc<Mutex<usize>>);
    type Breaker = fn(&mut crate::glide::tests::Script);

    fn glide_rig(name: &str, live: bool) -> Rigged {
        glide_rig_waits(name, live).0
    }

    fn glide_rig_waits(name: &str, live: bool) -> (Rigged, crate::glide::tests::Waits) {
        let (mut daemon, fake, _) = rig(name, vec![screen("main", 0.0, 2.0)]);
        *fake.live.lock().unwrap() = live;
        let spawner = crate::glide::tests::FakeSpawner::new(fake.events.clone());
        let (script, spawned, waits) = (spawner.script.clone(), spawner.spawned.clone(), spawner.waits.clone());
        daemon.glider = Arc::new(Mutex::new(Glider::new(Box::new(spawner))));
        ((daemon, fake, script, spawned), waits)
    }

    fn inside_budget(waits: &[(&'static str, Duration)]) -> bool {
        let shared: Vec<Duration> = waits.iter().filter(|(what, _)| *what != "spawn").map(|(_, wait)| *wait).collect();
        waits.iter().all(|(_, wait)| *wait <= crate::glide::BUDGET) && shared.windows(2).all(|pair| pair[1] <= pair[0])
    }

    fn held(script: &Mutex<crate::glide::tests::Script>) -> Arc<std::sync::atomic::AtomicBool> {
        let held = Arc::new(std::sync::atomic::AtomicBool::new(true));
        script.lock().unwrap().held = Some(held.clone());
        held
    }

    fn moved_subject() -> Rect {
        Rect { x: 76.0, y: 36.0, w: 30.0, h: 30.0 }
    }

    fn first_and_moved(daemon: &Daemon, fake: &Fake) -> (Outcome, Outcome) {
        fake.place(vec![subject()]);
        let first = outcome(&daemon.tick());
        fake.place(vec![moved_subject()]);
        let second = outcome(&daemon.tick());
        (first, second)
    }

    fn events(fake: &Fake) -> Vec<String> {
        fake.events.lock().unwrap().iter().map(|(event, _)| event.clone()).collect()
    }

    fn when(fake: &Fake, start: &str) -> Instant {
        fake.events.lock().unwrap().iter().rev().find(|(event, _)| event.starts_with(start)).map(|(_, at)| *at).unwrap_or_else(|| panic!("no {start}"))
    }

    fn at(events: &[String], start: &str) -> usize {
        events.iter().position(|event| event.starts_with(start)).unwrap_or_else(|| panic!("no {start} in {events:?}"))
    }

    fn today(name: &str) -> Vec<u8> {
        let (daemon, fake, _, _) = glide_rig(name, true);
        *fake.reduce.lock().unwrap() = true;
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        std::fs::read(fake.showing("main")).unwrap()
    }

    #[test]
    fn a_glide_opens_before_the_set_and_settles_and_closes_after_it() {
        let (daemon, fake, _, spawned) = glide_rig("glide-order", true);
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        let events = events(&fake);
        let (open, set, settle, settled, close) = (at(&events, "open"), events.iter().rposition(|event| event.starts_with("set ")).unwrap(), at(&events, "settle "), at(&events, "settled"), at(&events, "close"));
        assert!(at(&events, "glide") > open && open < set && set < settle && settle < settled && settled < close, "{events:?}");
        let first = daemon.store.output("main", 1, "jpg");
        let second = fake.showing("main");
        assert!(events[open].ends_with(&first.file_name().unwrap().to_string_lossy().into_owned()), "{events:?}");
        assert!(events[settle].ends_with(&second.file_name().unwrap().to_string_lossy().into_owned()), "{events:?}");
        assert!(events.contains(&"glide 1 true true".to_string()), "{events:?}");
        assert!(when(&fake, "close") >= when(&fake, "set ") + fake.hold(), "the layer closed before the hold after the set");
        assert_eq!(*spawned.lock().unwrap(), 1);
        assert_eq!(std::fs::read(&second).unwrap(), today("glide-order-today"));
        let tables: Vec<_> = std::fs::read_dir(daemon.store.tables()).unwrap().flatten().map(|file| file.path()).collect();
        assert_eq!(tables, vec![daemon.store.tables_for(&second, tables_key(&daemon.store.load().screens["main"].source, &Options::default()).unwrap())]);
    }

    #[test]
    fn two_screens_glide_in_one_helper_and_each_holds_from_its_own_set() {
        let (mut daemon, fake, _) = rig("glide-two", vec![screen("left", 0.0, 2.0), screen("right", 160.0, 2.0)]);
        *fake.live.lock().unwrap() = true;
        let spawner = crate::glide::tests::FakeSpawner::new(fake.events.clone());
        let spawned = spawner.spawned.clone();
        daemon.glider = Arc::new(Mutex::new(Glider::new(Box::new(spawner))));
        let right = |rect: Rect| Rect { x: rect.x + 160.0, ..rect };
        fake.place(vec![subject(), right(subject())]);
        daemon.tick();
        fake.place(vec![moved_subject(), right(moved_subject())]);
        let report = daemon.tick();
        assert_eq!(report.outcomes.iter().map(|(_, outcome)| *outcome).collect::<Vec<_>>(), vec![Outcome::Rendered, Outcome::Rendered]);
        let stamped = fake.events.lock().unwrap().clone();
        let names: Vec<&str> = stamped.iter().map(|(event, _)| event.as_str()).collect();
        assert_eq!(*spawned.lock().unwrap(), 1, "{names:?}");
        let sets: Vec<Instant> = stamped.iter().filter(|(event, _)| event.starts_with("set ")).map(|(_, at)| *at).collect();
        for (id, set) in [1u32, 2].into_iter().zip(sets[sets.len() - 2..].iter().copied()) {
            let close = stamped.iter().find(|(event, _)| *event == format!("close {id}")).map(|(_, at)| *at).unwrap_or_else(|| panic!("{names:?}"));
            let settled = stamped.iter().position(|(event, _)| *event == format!("settled {id}")).unwrap();
            assert!(settled < stamped.iter().position(|(event, _)| *event == format!("close {id}")).unwrap(), "{names:?}");
            assert!(close >= set + fake.hold(), "{id} {names:?}");
        }
    }

    #[test]
    fn the_layer_opens_before_the_snapshot_and_an_unused_one_closes_with_nothing_set() {
        let (daemon, fake, _, _) = glide_rig("glide-early", true);
        fake.place(vec![subject()]);
        daemon.tick();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        let open = at(&events, "open");
        assert!(open < events.iter().rposition(|event| event == "snapshot").unwrap() && open < at(&events, "glide"), "{events:?}");
        assert!(daemon.glider.lock().unwrap().said.iter().any(|line| line.ends_with("opened early")));
        fake.events.lock().unwrap().clear();
        let sets = fake.sets.load(Ordering::SeqCst);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        let events: Vec<String> = fake.events.lock().unwrap().iter().map(|(event, _)| event.clone()).collect();
        assert!(at(&events, "open") < at(&events, "close") && !events.iter().any(|event| event.starts_with("glide ") || event.starts_with("set ")), "{events:?}");
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
        assert!(daemon.held_bytes() > 0);
    }

    #[test]
    fn a_quick_look_after_a_drop_goes_on_only_when_the_icons_moved() {
        let (daemon, fake, _) = rig("quick", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        assert!(!daemon.quick_look());
        *fake.drop_seen.lock().unwrap() = true;
        assert!(!daemon.quick_look());
        fake.place(vec![corner()]);
        assert!(!daemon.quick_look());
        *fake.drop_seen.lock().unwrap() = true;
        assert!(daemon.quick_look());
        let signal = Signal::default();
        signal.wake();
        signal.take();
        signal.wake();
        let started = Instant::now();
        signal.first();
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    fn two_screen_rig(name: &str) -> (Daemon, Arc<Fake>, Arc<Mutex<crate::glide::tests::Script>>) {
        let (mut daemon, fake, _) = rig(name, vec![screen("left", 0.0, 2.0), screen("right", 160.0, 2.0)]);
        *fake.live.lock().unwrap() = true;
        let spawner = crate::glide::tests::FakeSpawner::new(fake.events.clone());
        let script = spawner.script.clone();
        daemon.glider = Arc::new(Mutex::new(Glider::new(Box::new(spawner))));
        (daemon, fake, script)
    }

    fn right(rect: Rect) -> Rect {
        Rect { x: rect.x + 160.0, ..rect }
    }

    #[test]
    fn a_fault_on_one_screen_holds_the_layers_already_over_a_new_wallpaper() {
        let (daemon, fake, script) = two_screen_rig("glide-fault-hold");
        fake.place(vec![subject(), right(subject())]);
        daemon.tick();
        fake.events.lock().unwrap().clear();
        script.lock().unwrap().refuse_after = Some(2);
        fake.place(vec![moved_subject(), right(moved_subject())]);
        let report = daemon.tick();
        assert_eq!(report.outcomes.iter().map(|(_, outcome)| *outcome).collect::<Vec<_>>(), vec![Outcome::Rendered, Outcome::Rendered]);
        let stamped = fake.events.lock().unwrap().clone();
        let names: Vec<&str> = stamped.iter().map(|(event, _)| event.as_str()).collect();
        let when = |name: &str| stamped.iter().find(|(event, _)| event.starts_with(name)).map(|(_, at)| *at).unwrap_or_else(|| panic!("no {name} in {names:?}"));
        let first_set = stamped.iter().find(|(event, _)| event.starts_with("set ")).map(|(_, at)| *at).unwrap();
        assert!(names.iter().position(|event| event.starts_with("refused 2")) < names.iter().position(|event| *event == "close 1"), "{names:?}");
        assert!(when("close 1") >= first_set + fake.hold(), "{names:?}");
        let second_set = stamped.iter().filter(|(event, _)| event.starts_with("set ")).nth(1).map(|(_, at)| *at).unwrap();
        assert!(when("close 2") <= second_set, "{names:?}");
        assert!(!daemon.glider.lock().unwrap().running(), "{names:?}");
    }

    #[test]
    fn an_early_layer_closes_before_its_screen_takes_the_plain_path() {
        let (daemon, fake, _, _) = glide_rig("glide-early-plain", true);
        fake.place(vec![subject()]);
        daemon.tick();
        fake.place(vec![moved_subject()]);
        daemon.tick();
        fake.events.lock().unwrap().clear();
        fake.place(vec![corner()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        let events = events(&fake);
        let open = at(&events, "open");
        let close = events.iter().position(|event| *event == events[open].replacen("open", "close", 1).split(' ').take(2).collect::<Vec<_>>().join(" ")).unwrap_or_else(|| panic!("{events:?}"));
        let set = events.iter().position(|event| event.starts_with("set ")).unwrap_or_else(|| panic!("{events:?}"));
        assert!(open < close && close < set && !events.iter().any(|event| event.starts_with("glide ")), "{events:?}");
    }

    #[test]
    fn a_screen_too_large_for_the_gpu_is_skipped_until_it_changes() {
        let (daemon, fake, script, spawned) = glide_rig("glide-huge", true);
        fake.place(vec![subject()]);
        daemon.tick();
        script.lock().unwrap().too_large = true;
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(daemon.glider.lock().unwrap().running());
        fake.events.lock().unwrap().clear();
        fake.place(vec![Rect { x: 40.0, ..moved_subject() }]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        assert!(!events.iter().any(|event| event.starts_with("open") || event.starts_with("huge")), "{events:?}");
        assert_eq!(*spawned.lock().unwrap(), 1);
        let said = daemon.glider.lock().unwrap().said.clone();
        assert_eq!(said.iter().filter(|line| line.contains("larger than the GPU can draw")).count(), 1, "{said:?}");
    }

    #[test]
    fn a_still_saved_with_less_color_detail_swaps_plainly() {
        let (daemon, fake, _, _) = glide_rig("glide-old-still", true);
        fake.place(vec![subject()]);
        daemon.tick();
        let still = fake.shown.lock().unwrap().get("main").cloned().unwrap();
        assert!(crate::glide::exact(&still));
        std::fs::write(&still, encode(&scene::sample(64, 40), Format::Jpeg, None, false).unwrap()).unwrap();
        assert!(!crate::glide::exact(&still));
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let seen = events(&fake);
        assert!(seen.iter().any(|event| event.starts_with("set ")) && !seen.iter().any(|event| event.starts_with("open") || event.starts_with("glide ")), "{seen:?}");
        let now = fake.shown.lock().unwrap().get("main").cloned().unwrap();
        assert!(crate::glide::exact(&now));
        fake.place(vec![Rect { x: 40.0, ..moved_subject() }]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(events(&fake).iter().any(|event| event.starts_with("glide ")), "{:?}", events(&fake));
    }

    #[test]
    fn a_png_or_unreadable_still_counts_as_exact() {
        let folder = crate::state::scratch("exact-still");
        let png = folder.join("still.png");
        std::fs::write(&png, encode(&scene::sample(16, 16), Format::Png, None, false).unwrap()).unwrap();
        assert!(crate::glide::exact(&png));
        assert!(crate::glide::exact(&folder.join("missing.jpg")));
        let full = folder.join("full.jpg");
        std::fs::write(&full, encode(&scene::sample(16, 16), Format::Jpeg, Some(&[7u8; 300]), true).unwrap()).unwrap();
        assert!(crate::glide::exact(&full));
    }

    #[test]
    fn every_failure_takes_todays_path_with_the_same_picture() {
        let expected = today("glide-fail-today");
        let cases: [(&str, Breaker); 5] = [
            ("spawn", |script| script.spawn_fails = true),
            ("ready", |script| script.ready_fails = true),
            ("shown", |script| script.shown_fails = true),
            ("settle", |script| script.settle_never = true),
            ("dies", |script| script.dies_after_glide = true),
        ];
        for (name, break_it) in cases {
            let ((daemon, fake, script, _), waits) = glide_rig_waits(&format!("glide-fail-{name}"), true);
            break_it(&mut script.lock().unwrap());
            assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered), "{name}");
            assert!(inside_budget(&waits.lock().unwrap()), "{name} {:?}", waits.lock().unwrap());
            assert_eq!(std::fs::read(fake.showing("main")).unwrap(), expected, "{name}");
            let events = events(&fake);
            let opened = events.iter().filter(|event| event.starts_with("open")).count();
            let closed = events.iter().filter(|event| event.starts_with("close")).count();
            if name != "dies" {
                assert_eq!(opened, closed, "{name} {events:?}");
            }
            assert!(!daemon.glider.lock().unwrap().in_flight(), "{name}");
            assert_eq!(daemon.glider.lock().unwrap().running(), name == "shown", "{name}");
        }
    }

    #[test]
    fn a_failed_open_waits_inside_the_budget_and_then_sets_the_still() {
        let ((daemon, fake, script, _), waits) = glide_rig_waits("glide-budget", true);
        script.lock().unwrap().shown_fails = true;
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        let waits = waits.lock().unwrap().clone();
        assert!(waits.iter().any(|(what, _)| *what == "shown") && inside_budget(&waits), "{waits:?}");
        let events = events(&fake);
        let hidden = at(&events, "hidden");
        let set = events.iter().rposition(|event| event.starts_with("set ")).unwrap();
        assert!(at(&events, "open") < hidden && hidden < at(&events, "close") && at(&events, "close") < set, "{events:?}");
        assert!(daemon.glider.lock().unwrap().said.iter().any(|line| line.starts_with("skipped, hidden")));
    }

    #[test]
    fn no_helper_starts_when_the_glide_cannot_apply() {
        let (daemon, fake, _, spawned) = glide_rig("glide-reduce", true);
        *fake.reduce.lock().unwrap() = true;
        first_and_moved(&daemon, &fake);
        assert_eq!(*spawned.lock().unwrap(), 0);
        let (daemon, fake, _, spawned) = glide_rig("glide-setting", true);
        let mut state = daemon.store.load();
        state.glide = false;
        daemon.store.save(&state).unwrap();
        first_and_moved(&daemon, &fake);
        assert_eq!(*spawned.lock().unwrap(), 0);
        assert!(!daemon.store.tables().exists());
        let (daemon, fake, _, spawned) = glide_rig("glide-no-tables", true);
        fake.place(vec![subject()]);
        daemon.tick();
        std::fs::remove_dir_all(daemon.store.tables()).unwrap();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(*spawned.lock().unwrap(), 0);
        let (daemon, fake, _, spawned) = glide_rig("glide-not-shown", true);
        fake.place(vec![subject()]);
        daemon.tick();
        let other = daemon.store.out().join("main-90.jpg");
        std::fs::copy(fake.showing("main"), &other).unwrap();
        fake.show("main", &other);
        fake.place(vec![moved_subject()]);
        daemon.tick();
        assert_eq!(*spawned.lock().unwrap(), 0);
        let (daemon, fake, _, spawned) = glide_rig("glide-not-live", false);
        first_and_moved(&daemon, &fake);
        assert_eq!(*spawned.lock().unwrap(), 0);
        assert!(!daemon.store.tables().exists());
    }

    #[test]
    fn a_tick_returns_only_after_its_glide_settles_and_the_next_glides_from_there() {
        let (daemon, fake, script, _) = glide_rig("glide-second", true);
        fake.place(vec![subject()]);
        daemon.tick();
        let gate = held(&script);
        fake.place(vec![moved_subject()]);
        let daemon = Arc::new(daemon);
        let ticking = daemon.clone();
        let first = std::thread::spawn(move || outcome(&ticking.tick()));
        while !events(&fake).iter().any(|event| event.starts_with("settle ")) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!first.is_finished());
        gate.store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(first.join().unwrap(), Outcome::Rendered);
        let settled = at(&events(&fake), "settled 1");
        let middle = fake.showing("main");
        fake.place(vec![Rect { x: 40.0, ..moved_subject() }]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        let second_open = at(&events, "open 2");
        assert!(settled < at(&events, "close 1") && at(&events, "close 1") < second_open, "{events:?}");
        assert!(events[second_open].ends_with(&middle.file_name().unwrap().to_string_lossy().into_owned()), "{events:?}");
    }

    #[test]
    fn turning_scooch_off_during_a_glide_closes_it_at_once() {
        let (daemon, fake, script, _) = glide_rig("glide-off", true);
        fake.place(vec![subject()]);
        daemon.tick();
        let _gate = held(&script);
        fake.place(vec![moved_subject()]);
        let store = daemon.store.clone();
        let daemon = Arc::new(daemon);
        let ticking = daemon.clone();
        let first = std::thread::spawn(move || ticking.tick());
        while !events(&fake).iter().any(|event| event.starts_with("settle ")) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut state = store.load();
        state.on = false;
        store.save(&state).unwrap();
        first.join().unwrap();
        let events = events(&fake);
        assert!(events.iter().any(|event| event.starts_with("close")) && !events.iter().any(|event| event.starts_with("settled")), "{events:?}");
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.contains("interrupted")) && !said.iter().any(|line| line.contains("did not settle")), "{said:?}");
    }

    fn opens(fake: &Fake) -> usize {
        events(fake).iter().filter(|event| event.starts_with("glide ")).count()
    }

    #[test]
    fn a_software_adapter_keeps_todays_path_unless_forced() {
        let (daemon, fake, script, spawned) = glide_rig("glide-software", true);
        script.lock().unwrap().adapter = "Microsoft Basic Render Driver".into();
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        assert_eq!(opens(&fake), 0);
        assert!(!daemon.glider.lock().unwrap().running());
        fake.place(vec![Rect { x: 40.0, ..moved_subject() }]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!((*spawned.lock().unwrap(), opens(&fake)), (1, 0));
        let (daemon, fake, script, _) = glide_rig("glide-software-forced", true);
        script.lock().unwrap().adapter = "llvmpipe (LLVM 17.0.6, 256 bits)".into();
        daemon.glider.lock().unwrap().forced = true;
        first_and_moved(&daemon, &fake);
        assert_eq!(opens(&fake), 1);
        for (kind, glides) in [("cpu", 0), ("virtual", 1), ("integrated", 1)] {
            let (daemon, fake, script, _) = glide_rig(&format!("glide-kind-{kind}"), true);
            script.lock().unwrap().adapter = "Apple Paravirtual device".into();
            script.lock().unwrap().kind = kind.into();
            first_and_moved(&daemon, &fake);
            assert_eq!(opens(&fake), glides, "{kind}");
        }
    }

    #[test]
    fn two_stuttering_glides_in_a_row_turn_the_glide_off() {
        let (daemon, fake, script, _) = glide_rig("glide-stutter", true);
        fake.place(vec![subject()]);
        daemon.tick();
        let mut spot = 0;
        let mut glide = |glided: (u32, u32)| {
            script.lock().unwrap().glided = Some(glided);
            for _ in 0..40 {
                spot += 1;
                fake.place(vec![Rect { x: 30.0 + (spot * 13 % 70) as f32, y: 20.0 + (spot * 7 % 40) as f32, w: 40.0, h: 40.0 }]);
                if outcome(&daemon.tick()) == Outcome::Rendered {
                    return;
                }
            }
            panic!("no layout rendered again");
        };
        glide((10, 6));
        glide((10, 2));
        glide((10, 6));
        assert!(daemon.glider.lock().unwrap().usable());
        glide((10, 7));
        assert!(!daemon.glider.lock().unwrap().usable());
        assert_eq!(opens(&fake), 4);
        glide((10, 0));
        assert_eq!(opens(&fake), 4);
        let said = daemon.glider.lock().unwrap().said.clone();
        assert_eq!(said.iter().filter(|line| line.contains("stuttered")).count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_poke_from_another_process_wakes_the_worker() {
        let signal = Arc::new(Signal::default());
        let poked = signal.clone();
        poke::listen(move || poked.wake(), || {}).unwrap();
        assert!(poke::send(std::process::id()));
        let started = Instant::now();
        signal.settle(Duration::from_millis(10), Duration::from_secs(1));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
