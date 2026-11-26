use crate::platform::{Desktop, DragHooks, Format, Screen, Snapshot};
use crate::state::{display_of, file_safe, parse, same, write_atomic, Entry, Lock, Places, State, Store, Tally};
use crate::glide::{Glider, Move, Plan, Prime, Program, Warmup};
use crate::live::Stills;
use crate::{cache, poke, scene};
use anyhow::{Context, Result};
use scooch_core::{Analysis, Options, Placement, Rect, RgbImage, Scene};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const QUIET: Duration = Duration::from_millis(250);
pub const SAVE_QUIET: Duration = Duration::from_millis(20);
const REAP_EVERY: Duration = Duration::from_secs(2);
pub const REST: Duration = Duration::from_secs(30);
pub const LIMIT: Duration = Duration::from_secs(2);
pub const QUALITY: u8 = 95;

#[derive(Default)]
struct Pending {
    first: Option<Instant>,
    last: Option<Instant>,
    due: Option<Instant>,
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

    pub fn first(&self) -> Instant {
        let mut pending = self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while pending.last.is_none() {
            let now = Instant::now();
            pending = match pending.due {
                Some(due) if due <= now => {
                    (pending.first, pending.last) = (Some(now), Some(now));
                    pending
                }
                Some(due) => self.ready.wait_timeout(pending, due - now).unwrap_or_else(|poisoned| poisoned.into_inner()).0,
                None => self.ready.wait(pending).unwrap_or_else(|poisoned| poisoned.into_inner()),
            };
        }
        pending.first.unwrap_or_else(Instant::now)
    }

    pub fn retry_in(&self, wait: Duration) {
        self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).due = Some(Instant::now() + wait);
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
    pub silent: bool,
    pub unsaved: bool,
    pub exit: bool,
    pub unload: bool,
    #[cfg(any(target_os = "linux", test))]
    pub origin: Option<PathBuf>,
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
            } else if drop_gone && detached(desktop, &id, &entry) {
                state.screens.remove(&id);
                tally.back += 1;
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

pub fn detached(desktop: &dyn Desktop, id: &str, entry: &Entry) -> bool {
    !entry.unrestorable && entry.original.is_file() && desktop.put_back_detached(id, &entry.original).unwrap_or(false)
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

type MapKey = (u64, (u32, u32), u64);
type Parts<'a> = (&'a RgbImage, &'a Analysis, &'a Placement, &'a Scene, &'a Options);

struct Finished {
    bundle: Option<Arc<scooch_core::Bundle>>,
    encoded: Result<Vec<u8>>,
    same: bool,
    picture: Option<Arc<RgbImage>>,
}

#[derive(Clone, Copy)]
enum Hold {
    Taking,
    Since(Instant),
}

pub struct Daemon {
    pub desktop: Arc<dyn Desktop>,
    pub store: Store,
    pub grace: Duration,
    recent: Mutex<HashMap<String, Instant>>,
    last_wake: Mutex<Option<Instant>>,
    canvases: Arc<Mutex<HashSet<PathBuf>>>,
    ledger: Mutex<Ledger>,
    pub glider: Arc<Mutex<Glider>>,
    woke: Mutex<Option<Instant>>,
    reaping: Arc<std::sync::atomic::AtomicBool>,
    motion: std::sync::atomic::AtomicU8,
    sessions: Arc<Mutex<HashMap<String, (u64, scooch_core::Session)>>>,
    used: Arc<Mutex<Instant>>,
    seen: Mutex<Option<u64>>,
    prints: Mutex<HashMap<String, u64>>,
    moved: Mutex<Vec<String>>,
    handed: Mutex<Option<(Instant, Snapshot)>>,
    icons: Mutex<Option<Vec<Rect>>>,
    quick_hold: Mutex<Option<Hold>>,
    mapped: Mutex<HashMap<String, MapKey>>,
    last_screens: Mutex<Vec<Screen>>,
    stills: Mutex<HashMap<String, Duration>>,
    saves: Mutex<Option<Instant>>,
    #[cfg(test)]
    pub marks: Mutex<Option<crate::glide::tests::Events>>,
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spent {
    pub thread: Duration,
    pub process: Duration,
    pub writers: usize,
}

static WRITERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(any(windows, target_os = "macos"))]
struct Writing;

#[cfg(any(windows, target_os = "macos"))]
impl Writing {
    fn begin() -> Writing {
        WRITERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Writing
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl Drop for Writing {
    fn drop(&mut self) {
        WRITERS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(unix)]
fn spent() -> Spent {
    let mut clock = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    let thread = if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut clock) } == 0 { Duration::new(clock.tv_sec.max(0) as u64, clock.tv_nsec.max(0) as u32) } else { Duration::ZERO };
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let seconds = |time: libc::timeval| Duration::new(time.tv_sec.max(0) as u64, (time.tv_usec.max(0) as u32) * 1000);
    let process = if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0 { seconds(usage.ru_utime) + seconds(usage.ru_stime) } else { Duration::ZERO };
    Spent { thread, process, writers: WRITERS.load(std::sync::atomic::Ordering::SeqCst) }
}

#[cfg(not(unix))]
fn spent() -> Spent {
    Spent { writers: WRITERS.load(std::sync::atomic::Ordering::SeqCst), ..Spent::default() }
}

thread_local! {
    static LAPS: std::cell::RefCell<Vec<(&'static str, u128)>> = const { std::cell::RefCell::new(Vec::new()) };
    static LAPPED: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    static WOKE: std::cell::Cell<Option<Spent>> = const { std::cell::Cell::new(None) };
    static SET: std::cell::Cell<Option<Spent>> = const { std::cell::Cell::new(None) };
}

fn lap(name: &'static str) {
    if trace() {
        let now = Instant::now();
        let since = LAPPED.with(|lapped| lapped.replace(Some(now))).unwrap_or(now);
        LAPS.with(|laps| laps.borrow_mut().push((name, now.saturating_duration_since(since).as_millis())));
    }
}

fn lap_set() {
    lap("set");
    if trace() {
        SET.with(|set| set.set(Some(spent())));
    }
}

fn laps_from(at: Instant) {
    if trace() {
        LAPPED.with(|lapped| lapped.set(Some(at)));
    }
}

fn begin_laps(woke: Instant) {
    if trace() {
        laps_from(woke);
        WOKE.with(|start| start.set(Some(spent())));
        SET.with(|set| set.set(None));
    }
}

fn take_laps() -> Vec<(&'static str, u128)> {
    LAPS.with(|laps| laps.borrow_mut().drain(..).collect())
}

fn give_laps(given: Vec<(&'static str, u128)>) {
    LAPS.with(|laps| laps.borrow_mut().extend(given));
}

fn laps() -> String {
    LAPS.with(|laps| laps.borrow_mut().drain(..).map(|(name, millis)| format!("{name} {millis}")).collect::<Vec<_>>().join(", "))
}

pub fn spent_words(woke: Option<Spent>, set: Option<Spent>) -> String {
    let (Some(woke), Some(set)) = (woke, set) else { return String::new() };
    format!(", cpu to the set {} ms on the tick thread and {} ms in the daemon, writers {} at the wake and {} at the set", set.thread.saturating_sub(woke.thread).as_millis(), set.process.saturating_sub(woke.process).as_millis(), woke.writers, set.writers)
}

fn spent_line() -> String {
    spent_words(WOKE.with(std::cell::Cell::get), SET.with(std::cell::Cell::take))
}

#[cfg(any(unix, windows))]
const LOG_MOST: u64 = 1 << 20;
#[cfg(any(unix, windows))]
const LOG_KEPT: u64 = 1 << 18;

static LOG: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[cfg(not(windows))]
pub fn log_to(path: PathBuf) {
    let _ = LOG.set(path);
}

#[cfg(windows)]
static OWN_LOG: std::sync::OnceLock<std::fs::File> = std::sync::OnceLock::new();

#[cfg(windows)]
pub fn log_to(path: PathBuf) {
    use std::io::{Seek, SeekFrom};
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_DISK, FILE_TYPE_PIPE};
    use windows::Win32::System::Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE};
    let _ = LOG.set(path.clone());
    let kept = unsafe { GetStdHandle(STD_ERROR_HANDLE) }.ok().filter(|handle| !handle.is_invalid() && !handle.0.is_null()).is_some_and(|handle| matches!(unsafe { GetFileType(handle) }, FILE_TYPE_DISK | FILE_TYPE_PIPE));
    if kept || OWN_LOG.get().is_some() {
        return;
    }
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&path) else { return };
    let _ = file.seek(SeekFrom::End(0));
    if unsafe { SetStdHandle(STD_ERROR_HANDLE, HANDLE(file.as_raw_handle())) }.is_ok() {
        let _ = OWN_LOG.set(file);
    }
}

#[cfg(unix)]
pub fn trim_log() {
    use std::os::fd::FromRawFd;
    let Some(path) = LOG.get() else { return };
    let stderr = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(2) });
    trim(path, &stderr);
}

#[cfg(windows)]
pub fn trim_log() {
    if let (Some(path), Some(file)) = (LOG.get(), OWN_LOG.get()) {
        trim(path, file);
    }
}

#[cfg(any(unix, windows))]
fn trim(path: &Path, open: &std::fs::File) -> bool {
    use std::io::{Read, Seek, SeekFrom, Write};
    let (Ok(named), Ok(held)) = (std::fs::metadata(path), open.metadata()) else { return false };
    if !same_file(&named, &held) || named.len() <= LOG_MOST {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut tail = Vec::new();
    if file.seek(SeekFrom::Start(named.len() - LOG_KEPT)).is_err() || file.read_to_end(&mut tail).is_err() {
        return false;
    }
    let start = tail.iter().position(|byte| *byte == b'\n').map_or(0, |at| at + 1);
    let mut open = open;
    open.set_len(0).is_ok() && open.seek(SeekFrom::Start(0)).is_ok() && open.write_all(&tail[start..]).is_ok()
}

#[cfg(unix)]
fn same_file(named: &std::fs::Metadata, held: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    named.dev() == held.dev() && named.ino() == held.ino()
}

#[cfg(windows)]
fn same_file(named: &std::fs::Metadata, held: &std::fs::Metadata) -> bool {
    named.len() == held.len()
}

#[cfg(not(any(unix, windows)))]
pub fn trim_log() {}

pub fn stamp() -> String {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}", since.as_secs(), since.subsec_millis())
}

impl Daemon {
    pub fn new(desktop: Arc<dyn Desktop>, store: Store) -> Self {
        let grace = std::env::var(crate::live::READ_LAG).ok().and_then(|ms| ms.parse::<u64>().ok()).map_or_else(|| desktop.read_lag(), |ms| Duration::from_millis(ms.min(60_000)));
        let glider = Arc::new(Mutex::new(Glider::new(Box::new(Program(crate::glide::program())))));
        Self { desktop, store, grace, recent: Mutex::new(HashMap::new()), last_wake: Mutex::new(None), canvases: Arc::new(Mutex::new(HashSet::new())), ledger: Mutex::new(Ledger::default()), glider, woke: Mutex::new(None), reaping: Arc::new(std::sync::atomic::AtomicBool::new(false)), motion: std::sync::atomic::AtomicU8::new(0), sessions: Arc::new(Mutex::new(HashMap::new())), used: Arc::new(Mutex::new(Instant::now())), seen: Mutex::new(None), prints: Mutex::new(HashMap::new()), moved: Mutex::new(Vec::new()), handed: Mutex::new(None), icons: Mutex::new(None), quick_hold: Mutex::new(None), mapped: Mutex::new(HashMap::new()), last_screens: Mutex::new(Vec::new()), stills: Mutex::new(HashMap::new()), saves: Mutex::new(None), #[cfg(test)] marks: Mutex::new(None) }
    }

    #[cfg(test)]
    fn mark(&self, what: &str, screen: &str) {
        if let Some(events) = self.marks.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_ref() {
            events.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push((format!("{what} {screen}"), Instant::now()));
        }
    }

    #[cfg(not(test))]
    fn mark(&self, _what: &str, _screen: &str) {}

    pub fn quick_look(&self) -> bool {
        let started = Instant::now();
        if self.desktop.dropped() {
            return self.look(started, false) || {
                std::thread::sleep(QUICK);
                self.look(started, false)
            };
        }
        let held = self.quick_hold.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some_and(|hold| match hold {
            Hold::Taking => true,
            Hold::Since(at) => at.elapsed() < QUIET,
        });
        let quick = self.desktop.reads_at_once() && !held && self.look(started, true);
        if quick {
            *self.quick_hold.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Hold::Taking);
        }
        quick
    }

    fn look(&self, started: Instant, pure: bool) -> bool {
        let taken = Instant::now();
        let Ok(snapshot) = self.desktop.snapshot() else { return false };
        let now = icon_print(&snapshot);
        let before = *self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let moved = before.is_some_and(|before| before != now) && (!pure || self.icons.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_deref().is_some_and(|known| pure_move(known, &snapshot.icons)));
        if trace() {
            eprintln!("{} quick look after {} ms, icons {now:016x} moved {moved}", stamp(), taken.saturating_duration_since(started).as_millis());
        }
        if moved {
            *self.handed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((taken, snapshot));
        }
        moved
    }

    pub fn gather(&self, worker: &Signal) {
        let now = Instant::now();
        *self.woke.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(now);
        self.glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).woke(now);
        self.wake_early();
        lap("prime");
        if self.predicted(worker) || self.heard_save(worker) {
            lap("look");
            return;
        }
        if self.quick_look() {
            worker.take();
        } else {
            worker.settle(QUIET, LIMIT);
        }
        lap("look");
    }

    fn predicted(&self, worker: &Signal) -> bool {
        let Some(snapshot) = self.desktop.predicted(&|| parse(self.store.read().as_deref()).predict) else { return false };
        if trace() {
            eprintln!("{} predicted drop handed, icons {:016x}", stamp(), icon_print(&snapshot));
        }
        *self.handed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((Instant::now(), snapshot));
        worker.take();
        true
    }

    fn heard_save(&self, worker: &Signal) -> bool {
        let mut handled = self.saves.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(first) = self.desktop.place_saved().filter(|at| handled.is_none_or(|seen| *at > seen)) else { return false };
        let mut last = first;
        loop {
            let due = last + SAVE_QUIET;
            let now = Instant::now();
            if now < due {
                std::thread::sleep(due - now);
                last = self.desktop.place_saved().unwrap_or(last);
                continue;
            }
            let taken = Instant::now();
            if let Ok(snapshot) = self.desktop.snapshot() {
                if trace() {
                    eprintln!("{} save read {} ms after the first call, icons {:016x}", stamp(), taken.saturating_duration_since(first).as_millis(), icon_print(&snapshot));
                }
                *self.handed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((taken, snapshot));
            }
            worker.take();
            let newest = self.desktop.place_saved().unwrap_or(last);
            *handled = Some(newest);
            if newest <= taken {
                return true;
            }
            last = newest;
        }
    }

    pub fn wake_early(&self) -> bool {
        let before = self.last_wake.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).replace(Instant::now());
        let screens = self.last_screens.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        let Some(screen) = self.desktop.pointer().and_then(|point| crate::glide::under(&screens, point).cloned()) else { return false };
        let set = self.recent.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&screen.id).copied();
        if set.is_some_and(|set| before.is_none_or(|before| set > before)) {
            return false;
        }
        self.prime_on(&screen, false)
    }

    pub fn over_icon(&self, point: (f32, f32)) -> bool {
        self.icons.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_deref().is_some_and(|icons| icons.iter().any(|icon| holds(icon, point)))
    }

    pub fn drag_starting(self: &Arc<Self>, point: (f32, f32)) -> bool {
        if !self.over_icon(point) || !self.desktop.desktop_at(point) {
            return false;
        }
        let daemon = self.clone();
        let _ = std::thread::Builder::new().name("scooch-drag".into()).spawn(move || {
            daemon.launch_at_drag(point);
        });
        true
    }

    pub fn launch_at_drag(&self, point: (f32, f32)) -> bool {
        let screens = self.last_screens.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        let Some(screen) = crate::glide::under(&screens, point).cloned() else { return false };
        self.prime_on(&screen, true)
    }

    fn prime_on(&self, screen: &Screen, drag: bool) -> bool {
        let state = parse(self.store.read().as_deref());
        if !state.on || state.restoring || self.glide_block(&state).is_some() {
            return false;
        }
        let Some(entry) = state.screens.get(&screen.id) else { return false };
        let scene = scene::scene(screen, &[]);
        if self.glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cannot(&screen.id, [scene.width, scene.height]).is_some() {
            return false;
        }
        self.mark("prime look", &screen.id);
        let Some(output) = entry.output.as_ref().filter(|output| output.is_file()) else { return false };
        if !self.desktop.wallpaper(screen).ok().flatten().is_some_and(|shown| same(&shown, output)) {
            return false;
        }
        let Ok(key) = tables_key(&entry.source, &state.options) else { return false };
        let canvas = Some(self.store.canvas_for(&screen.id, key)).filter(|canvas| canvas.is_file());
        let warm = Warmup { picture: entry.source.clone(), size: [scene.width, scene.height], canvas, key };
        let stills = self.stills_of(output, key);
        let mut glider = self.glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        glider.prime(&Prime { screen, warm: &warm, stills, drag })
    }

    fn stills_of(&self, output: &Path, key: u64) -> Stills {
        let raw = self.store.still_for(output);
        let tables = self.store.tables_for(output, key);
        Stills { still: Some(output.to_path_buf()), raw: raw.is_file().then_some(raw), tables: tables.is_file().then_some(tables) }
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

    fn moved_screens(&self, snapshot: &Snapshot) -> Vec<String> {
        let mut prints = self.prints.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut moved = Vec::new();
        for screen in &snapshot.screens {
            let now = screen_print(snapshot, screen);
            if prints.insert(screen.id.clone(), now).is_some_and(|before| before != now) {
                moved.push(screen.id.clone());
            }
        }
        moved
    }

    fn with_session<T>(&self, screen: &str, key: u64, work: impl FnOnce(&mut scooch_core::Session) -> T) -> T {
        let mut sessions = self.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let slot = sessions.entry(screen.to_string()).or_insert_with(|| (key, scooch_core::Session::new()));
        if slot.0 != key {
            slot.1.forget();
            slot.0 = key;
        }
        let made = work(&mut slot.1);
        drop(sessions);
        *self.used.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
        made
    }

    fn session_still(&self, screen: &str, key: u64, (image, analysis, placement, scene, options): Parts) -> (Arc<scooch_core::Bundle>, Arc<RgbImage>) {
        self.with_session(screen, key, |session| session.still_shared(image, analysis, placement, scene, options))
    }

    fn session_tables(&self, screen: &str, key: u64, (image, analysis, placement, scene, options): Parts) -> Arc<scooch_core::Bundle> {
        self.with_session(screen, key, |session| session.tables_shared(image, analysis, placement, scene, options))
    }

    fn bare(&self, entry: &mut Entry, screen: &str, (source, key): (&Path, u64), (image, analysis, scene, options): (&mut Option<RgbImage>, &Analysis, &Scene, &Options)) -> Result<bool> {
        if !crate::state::scenery_on(options) || scene.icons.is_empty() {
            return Ok(false);
        }
        let stamp = scooch_core::belonging_stamp(analysis, scene, options);
        if let Some(known) = entry.belongs.filter(|known| known.key == key && known.stamp == stamp) {
            return Ok(!known.answer);
        }
        let decoded = match image.take() {
            Some(decoded) => decoded,
            None => {
                let decoded = self.pixels(source, key)?;
                lap("decode");
                decoded
            }
        };
        let answer = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let slot = sessions.entry(screen.to_string()).or_insert_with(|| (key, scooch_core::Session::new()));
            if slot.0 != key {
                slot.1.forget();
                slot.0 = key;
            }
            slot.1.options(&decoded, analysis, scene, options).scenery == options.scenery
        };
        *self.used.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
        lap("belongs");
        *image = Some(decoded);
        entry.belongs = Some(crate::state::Belongs { key, stamp, answer });
        Ok(!answer)
    }

    fn adopt_kept(&self, screen: &str, key: u64) -> bool {
        let mut sessions = self.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let slot = sessions.entry(screen.to_string()).or_insert_with(|| (key, scooch_core::Session::new()));
        if slot.0 != key {
            slot.1.forget();
            slot.0 = key;
        }
        if slot.1.map().is_some() {
            return false;
        }
        let mut mapped = self.mapped.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match cache::kept_map(&self.store, screen, key) {
            Some(map) => {
                mapped.insert(screen.to_string(), map.key());
                slot.1.adopt(map);
            }
            None => {
                mapped.remove(screen);
            }
        }
        true
    }

    fn keep_maps(&self) {
        let held: Vec<(String, u64, scooch_core::Map)> = self.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().filter_map(|(screen, (key, session))| Some((screen.clone(), *key, session.map()?))).collect();
        let mut mapped = self.mapped.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for (screen, key, map) in held {
            if mapped.get(&screen) == Some(&map.key()) && cache::map_file(&self.store, &screen, key).is_file() {
                continue;
            }
            match cache::keep_map(&self.store, &screen, key, &map) {
                Ok(()) => {
                    mapped.insert(screen, map.key());
                }
                Err(error) => eprintln!("{} map not kept for {screen}, {error:#}", stamp()),
            }
        }
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
        let holding = self.sessions.lock().map(|sessions| !sessions.is_empty()).unwrap_or(false);
        if !holding || self.reaping.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let (reaping, sessions, used) = (self.reaping.clone(), self.sessions.clone(), self.used.clone());
        let spawned = std::thread::Builder::new().name("scooch-reap".into()).spawn(move || loop {
            std::thread::sleep(REAP_EVERY);
            let idle = used.lock().map(|used| used.elapsed() >= REST).unwrap_or(true);
            let mut sessions = sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if idle && !sessions.is_empty() {
                let held: usize = sessions.values().map(|(_, session)| session.bytes()).sum();
                sessions.clear();
                relieve();
                if trace() {
                    eprintln!("{} sessions let go of {} MB after {} s at rest", stamp(), held >> 20, REST.as_secs());
                }
            }
            if sessions.is_empty() {
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
        let mut kept: Vec<PathBuf> = state.screens.values().filter_map(|entry| Some(self.store.tables_for(entry.output.as_ref()?, tables_key(&entry.source, &state.options).ok()?))).collect();
        kept.extend(state.screens.values().filter_map(|entry| Some(self.store.still_for(entry.output.as_ref()?))));
        kept.extend(state.screens.iter().filter_map(|(id, entry)| Some(self.store.canvas_for(id, tables_key(&entry.source, &state.options).ok()?))));
        for file in found.flatten() {
            let path = file.path();
            let writing = file.file_name().to_string_lossy().starts_with('.') && file.metadata().ok().and_then(|meta| meta.modified().ok()).and_then(|at| at.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(60));
            if !kept.contains(&path) && !writing {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn settling(&self, screen: &str) -> bool {
        self.recent.lock().map(|recent| recent.get(screen).is_some_and(|at| at.elapsed() < self.grace)).unwrap_or(false)
    }

    pub fn tick(&self) -> Report {
        let report = self.tick_once();
        let mut hold = self.quick_hold.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if matches!(*hold, Some(Hold::Taking)) {
            *hold = Some(Hold::Since(Instant::now()));
        }
        report
    }

    fn tick_once(&self) -> Report {
        let handed = self.handed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        let before = self.store.read();
        *self.ledger.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Ledger { expected: before.clone(), external: false };
        let mut state = parse(before.as_deref());
        let mut report = Report::default();
        #[cfg(any(target_os = "linux", test))]
        report.origin.clone_from(&state.origin);
        if !state.on && !state.restoring {
            report.exit = true;
            return report;
        }
        if state.restoring {
            self.glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).stop();
            self.restore(&mut state, &mut report);
        } else {
            lap("state");
            let glider = self.glider.clone();
            let blocked = self.glide_block(&state);
            lap("block");
            let mut gliding = blocked.is_none().then(|| glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
            if let Some(glider) = gliding.as_deref_mut() {
                glider.know(&state.scales);
            }
            let snapshot = match handed.filter(|(taken, _)| taken.elapsed() < QUIET) {
                Some((_, snapshot)) => Ok(snapshot),
                None => self.desktop.snapshot(),
            };
            lap("snapshot");
            let mut moved = false;
            if let Ok(snapshot) = &snapshot {
                let print = icon_print(snapshot);
                *self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(print);
                *self.icons.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(snapshot.icons.clone());
                self.last_screens.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone_from(&snapshot.screens);
                if trace() {
                    eprintln!("{} snapshot icons {print:016x}", stamp());
                }
                let screens = self.moved_screens(snapshot);
                moved = !screens.is_empty();
                *self.moved.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = screens;
            }
            match gliding.as_deref_mut() {
                Some(glider) => glider.after_snapshot(moved),
                None => glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).after_snapshot(moved),
            }
            match snapshot {
                Ok(snapshot) => self.pass(&mut state, &snapshot, &mut report, (gliding.as_deref_mut(), blocked)),
                Err(error) => {
                    report.silent = silent(&error);
                    report.problem = Some(error.to_string());
                }
            }
            {
                let mut held = gliding.take().unwrap_or_else(|| glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
                held.close_early();
                for (key, scale) in held.learned() {
                    state.scales.insert(key, scale);
                }
                if blocked.is_some() {
                    held.stop();
                } else {
                    held.keep_parked();
                }
            }
            self.keep_maps();
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
            #[cfg(target_os = "linux")]
            if crate::commands::copy_goes(&state) {
                let own = std::env::current_exe().ok().map(|exe| crate::state::canonical(&exe).unwrap_or(exe));
                let proven: Vec<PathBuf> = own.into_iter().chain(crate::platform::login_program()).collect();
                for line in crate::program::remove_copies(&crate::program::copies(&self.store), &proven, &crate::program::link_places()) {
                    eprintln!("{} {line}", stamp());
                }
            }
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

    fn open_after_snapshot(&self, entry: &Entry, screen: &Screen, options: &Options, current: Option<&Path>, glider: &mut Glider) {
        if !self.moved.lock().map(|moved| moved.contains(&screen.id)).unwrap_or(false) {
            return;
        }
        let scene = scene::scene(screen, &[]);
        if glider.cannot(&screen.id, [scene.width, scene.height]).is_some() {
            return;
        }
        self.mark("read tables", &screen.id);
        let Some(output) = entry.output.as_ref().filter(|output| output.is_file()) else { return };
        let Ok(key) = tables_key(&entry.source, options) else { return };
        let tables = self.store.tables_for(output, key);
        if !tables.is_file() || !current.is_some_and(|shown| same(shown, output)) || stale_tables(&tables).is_some() {
            return;
        }
        let canvas = Some(self.store.canvas_for(&screen.id, key)).filter(|canvas| canvas.is_file());
        glider.open_early(screen, (&entry.source, key), [scene.width, scene.height], &self.stills_of(output, key), canvas.as_deref());
    }

    fn pass(&self, state: &mut State, snapshot: &Snapshot, report: &mut Report, (mut gliding, blocked): (Option<&mut Glider>, Option<&'static str>)) {
        let options = state.options;
        let known = self.desktop.known().ok().flatten();
        prune(state, self.desktop.as_ref(), &self.store, known.as_deref(), &snapshot.screens, unix_now());
        lap("screens");
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
                    report.silent |= silent(&error);
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
            if entry.placed.is_none() && entry.placement.is_some() {
                entry.placed = Some(solved_with(options));
            }
            if entry.places.is_none() {
                entry.places = Some(Places { width: scene.width, height: scene.height, icons: scene.icons.clone() });
            }
            if let Some(glider) = glider.as_deref_mut() {
                glider.close_early_for(&screen.id);
            }
            if showing && glider.is_some() && !settling {
                self.ready_files(entry, screen, &scene, options);
                lap("ready");
            }
            if showing || settling {
                return Ok(Outcome::Unchanged);
            }
            if let Some(glider) = glider.as_deref_mut() {
                glider.clear(&screen.id);
            }
            let shown = entry.shown().to_path_buf();
            self.show(screen, &shown)?;
            return Ok(Outcome::Reapplied);
        }
        lap("wallpaper");
        if let (Some(glider), false) = (glider.as_deref_mut(), settling) {
            self.open_after_snapshot(entry, screen, options, current.as_deref(), glider);
            lap("early");
        }
        let outcome = self.place(entry, screen, &scene, options, (current.as_deref().filter(|_| !settling), current.as_deref()), (glider, blocked))?;
        if outcome != Outcome::Interrupted {
            entry.fingerprint = Some(fingerprint);
            entry.chosen = false;
            entry.places = Some(Places { width: scene.width, height: scene.height, icons: scene.icons.clone() });
        }
        Ok(outcome)
    }

    fn place(&self, entry: &mut Entry, screen: &Screen, scene: &Scene, options: &Options, (current, seen): (Option<&Path>, Option<&Path>), (mut glider, blocked): (Option<&mut Glider>, Option<&'static str>)) -> Result<Outcome> {
        let source = entry.source.clone();
        let mut trail = Move::new(&screen.id, *self.woke.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        let changed = entry.placement.is_none();
        let mut image: Option<RgbImage> = None;
        let key = cache::key(&source)?;
        let analysis = cache::analysis(&self.store, key, || {
            let decoded = self.pixels(&source, key)?;
            lap("decode");
            let made = scooch_core::analyze(&decoded);
            image = Some(decoded);
            Ok(made)
        })?;
        lap("analysis");
        let placed = solved_with(options);
        let bare_off;
        let options = if self.bare(entry, &screen.id, (&source, key), (&mut image, &analysis, scene, options))? {
            bare_off = Options { scenery: scooch_core::Scenery::Off, ..*options };
            &bare_off
        } else {
            options
        };
        let wanted = crate::state::scenery_on(options) && !scene.icons.is_empty();
        let previous = entry.placement.as_ref().filter(|_| wanted || !entry.painted).filter(|_| entry.placed.as_deref().is_none_or(|before| before == placed));
        let placement = scooch_core::solve(&analysis, scene, options, previous);
        lap("solve");
        entry.flip_helps = flip_check(&analysis, scene, options, &placement);
        lap("flip");
        let pads = scooch_core::pads_active(&analysis, &placement, scene, options);
        let output = entry.output.clone().filter(|path| path.is_file());
        let showing_output = matches!((current, &output), (Some(current), Some(output)) if same(current, output));
        if scene::is_identity(&placement) && !pads {
            if let Some(glider) = glider.as_deref_mut() {
                glider.clear(&screen.id);
            }
            drop(analysis);
            drop(image);
            let copied = showing_output && !entry.painted && entry.placement.as_ref().is_some_and(scene::is_identity);
            entry.placement = Some(placement);
            entry.placed = Some(placed);
            let system = current.is_some_and(|path| same(path, &source));
            if system || copied {
                if system {
                    entry.output = None;
                }
                trail.skip("kept", "the picture already fits the icons as the system shows it, so it stays");
                self.tell(glider.as_deref_mut(), &trail);
            } else {
                trail.skip(blocked.unwrap_or("plain picture"), "the picture fits the icons as the system shows it, so it is put back plainly");
                if !self.plain(entry, screen, current, seen)? {
                    return Ok(Outcome::Interrupted);
                }
                trail.set = Some(Instant::now());
                self.tell(glider.as_deref_mut(), &trail);
            }
            entry.painted = false;
            entry.drawn = None;
            return Ok(Outcome::Plain);
        }
        let drawn = scene::fingerprint(&source, scene, options);
        let ours = showing_output || (output.is_some() && current.is_none() && self.settling(&screen.id));
        if !wanted && !entry.painted && ours && entry.placement.as_ref() == Some(&placement) && (options.pads <= 0.0 || entry.drawn.as_deref() == Some(drawn.as_str())) {
            if let Some(glider) = glider.as_deref_mut() {
                glider.clear(&screen.id);
            }
            entry.placed = Some(placed);
            trail.skip("kept", "the placement already fits the icons, so the picture stays");
            self.tell(glider.as_deref_mut(), &trail);
            return Ok(Outcome::Kept);
        }
        let painted = wanted && scooch_core::scenery_active(&analysis, &placement, scene, options);
        let decoded = match image.take() {
            Some(decoded) => decoded,
            None => self.pixels(&source, key)?,
        };
        lap("decode");
        let format = self.desktop.format();
        let shown = output.clone().filter(|_| showing_output);
        let parts: Parts = (&decoded, &analysis, &placement, scene, options);
        let finish = || {
            let started = Instant::now();
            self.mark("drawing", &screen.id);
            let (bundle, picture) = self.session_still(&screen.id, key, parts);
            self.mark("drawn", &screen.id);
            lap("render");
            let encoded = encode(&picture, format, self.desktop.profile().as_deref(), self.desktop.live());
            let lossless = format == Format::Png;
            lap("encode");
            let same = encoded.as_ref().is_ok_and(|bytes| shown.as_deref().is_some_and(|output| std::fs::read(output).is_ok_and(|old| old == *bytes)));
            lap("same");
            self.stills.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(screen.id.clone(), started.elapsed());
            Finished { bundle: Some(bundle), encoded, same, picture: lossless.then_some(picture) }
        };
        let beside = |wait: &mut dyn FnMut()| -> Finished {
            std::thread::scope(|scope| {
                let drawing = scope.spawn(|| {
                    laps_from(Instant::now());
                    let done = finish();
                    (done, take_laps())
                });
                wait();
                let (done, laps) = drawing.join().unwrap_or_else(|_| (Finished { bundle: None, encoded: Err(anyhow::anyhow!("the still could not be drawn")), same: false, picture: None }, Vec::new()));
                give_laps(laps);
                laps_from(Instant::now());
                done
            })
        };
        let tables_key = if glider.is_some() { tables_key(&source, options).ok() } else { None };
        let plain = glider.as_deref_mut().and_then(|glider| match shown {
            None if current.is_some_and(|current| !self.store.owns(current)) => Some(("system picture", "the system's own picture is on screen, so this first set swaps plainly".to_string())),
            None => Some(("output not shown", "the picture on screen is not the one Scooch set last".to_string())),
            Some(_) => glider.cannot(&screen.id, [scene.width, scene.height]).map(|(reason, detail)| (reason, if changed { format!("{detail}, the picture changed") } else { detail.to_string() })),
        });
        lap("cold");
        let mut flight = None;
        let mut crossing = false;
        let mut made = None;
        let mut finished = None;
        if let (Some(glider), Some((reason, detail))) = (glider.as_deref_mut(), plain.as_ref()) {
            trail.skip(reason, detail);
            glider.clear(&screen.id);
            lap("clear");
        }
        if let (Some(glider), None) = (glider.as_deref_mut(), plain.as_ref()) {
            if self.adopt_kept(&screen.id, key) {
                lap("map");
            }
            self.mark("read tables", &screen.id);
            let from = tables_key.zip(shown.as_deref()).map(|(tables_key, output)| self.store.tables_for(output, tables_key)).filter(|from| from.is_file());
            let on_screen = from.as_deref().and_then(|from| std::fs::read(from).ok());
            let stale = from.as_deref().zip(on_screen.as_deref()).and_then(|(from, bytes)| stale_bytes(from, bytes));
            lap("read");
            let canvas = tables_key.map(|tables_key| self.store.canvas_for(&screen.id, tables_key)).filter(|canvas| canvas.is_file());
            let size = [scene.width, scene.height];
            match (tables_key, from, shown.as_deref()) {
                (Some(_), Some(_), Some(_)) if stale.is_some() => trail.skip("old tables", stale.as_deref().unwrap_or_default()),
                (Some(tables_key), Some(from), Some(still)) => {
                    let plan = Plan { screen, picture: &source, canvas: canvas.as_deref(), key: tables_key, size, stills: self.stills_of(still, tables_key) };
                    let refused = glider.refused(&plan);
                    lap("ask");
                    if let Some((reason, detail)) = refused {
                        glider.close_early_for(&screen.id);
                        trail.skip(reason, detail);
                    } else {
                        let tables = self.session_tables(&screen.id, key, parts);
                        self.mark("tables", &screen.id);
                        lap("bundle");
                        trail.tables = Some(Instant::now());
                        let fresh = tables.tables_to_bytes();
                        if on_screen.as_deref() == Some(fresh.as_slice()) {
                            trail.skip("nothing moves", "the new tables are the tables on screen");
                        } else {
                            finished = Some(beside(&mut || {
                                let Some(id) = glider.prepare(&plan, &mut trail) else { return };
                                let (_, next) = self.fresh_name(&screen.id, entry.counter, format.extension(), current);
                                let target = self.store.tables_for(&next, tables_key);
                                match std::fs::create_dir_all(self.store.tables()).map_err(anyhow::Error::from).and_then(|_| crate::state::write_unsynced(&target, &fresh)) {
                                    Ok(()) => {
                                        match glider.go(id, &from, &target, std::mem::take(&mut trail)) {
                                            Ok(()) => flight = Some(id),
                                            Err(back) => trail = *back,
                                        }
                                        made = Some(target);
                                    }
                                    Err(error) => {
                                        glider.abort(id, "the tables could not be written");
                                        trail.skip("tables not written", &format!("{} {error:#}", target.display()));
                                    }
                                }
                            }));
                            lap("open");
                        }
                    }
                }
                (Some(tables_key), None, Some(still)) if changed => {
                    let plan = Plan { screen, picture: &source, canvas: canvas.as_deref(), key: tables_key, size, stills: Stills { tables: None, ..self.stills_of(still, tables_key) } };
                    finished = Some(beside(&mut || {
                        if let Some(id) = glider.cross(&plan, &mut trail) {
                            flight = Some(id);
                            crossing = true;
                        }
                    }));
                    lap("open");
                }
                _ => trail.skip("no tables", "the picture on screen has no tables"),
            }
            if flight.is_none() {
                glider.clear(&screen.id);
                lap("clear");
            }
        }
        let Finished { bundle, encoded, same, picture } = match finished {
            Some(done) => done,
            None if glider.is_some() && plain.is_none() => finish(),
            None => {
                trail.skip(blocked.unwrap_or("not live"), "");
                let held = entry.placement.as_ref() == Some(&placement);
                let (bundle, picture) = if wanted || held {
                    if self.adopt_kept(&screen.id, key) {
                        lap("map");
                    }
                    self.mark("drawing", &screen.id);
                    let (bundle, still) = self.session_still(&screen.id, key, parts);
                    self.mark("drawn", &screen.id);
                    (Some(bundle), still)
                } else {
                    self.forget_session(&screen.id);
                    self.mark("rendering", &screen.id);
                    (None, Arc::new(scooch_core::render_with(&decoded, &analysis, &placement, scene, options)))
                };
                lap("render");
                let encoded = encode(&picture, format, self.desktop.profile().as_deref(), self.desktop.live());
                let lossless = format == Format::Png;
                lap("encode");
                let same = encoded.as_ref().is_ok_and(|bytes| shown.as_deref().is_some_and(|output| std::fs::read(output).is_ok_and(|old| old == *bytes)));
                lap("same");
                Finished { bundle, encoded, same, picture: lossless.then_some(picture) }
            }
        };
        let back = |glider: &mut Glider, why: &str| {
            if let (Some(id), Some(old)) = (flight, shown.as_deref()) {
                glider.settle_back(id, old, why);
            }
        };
        let bytes = match encoded {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(glider) = glider.as_deref_mut() {
                    back(glider, &format!("the new picture could not be encoded, {error:#}"));
                    glider.clear(&screen.id);
                }
                if flight.is_none() {
                    trail.skip("helper failed", &format!("the new picture could not be encoded, {error:#}"));
                    self.tell(glider.as_deref_mut(), &trail);
                }
                return Err(error);
            }
        };
        if same {
            if let Some(glider) = glider.as_deref_mut() {
                back(glider, "the new picture is the picture on screen");
                glider.close_early_for(&screen.id);
            }
            if flight.is_none() {
                match trail.verdict {
                    Some(_) => trail.also("the new picture is the picture on screen"),
                    None => trail.skip("nothing new", "the new picture is the picture on screen"),
                }
                self.tell(glider.as_deref_mut(), &trail);
            }
            entry.placement = Some(placement);
            entry.placed = Some(placed);
            entry.drawn = Some(drawn);
            entry.painted = painted;
            return Ok(Outcome::Kept);
        }
        let was = std::mem::replace(&mut entry.painted, painted);
        let published = self.publish(entry, screen, current, seen, format.extension(), |path| write_atomic(path, &bytes));
        if !matches!(published, Ok(true)) {
            entry.painted = was;
            let settling = published.is_err() && flight.is_some() && shown.is_some();
            if let (Some(glider), Some(id)) = (glider.as_deref_mut(), flight) {
                match &published {
                    Err(error) if settling => back(glider, &format!("the new picture could not be set, {error:#}")),
                    _ => glider.abort(id, "the wallpaper changed or Scooch was turned off before the set"),
                }
            }
            if let Some(made) = made.as_ref().filter(|_| !settling) {
                let _ = std::fs::remove_file(made);
            }
            if flight.is_none() {
                trail.also("the set did not happen");
                self.tell(glider.as_deref_mut(), &trail);
            }
        }
        if !published? {
            return Ok(Outcome::Interrupted);
        }
        let set = Instant::now();
        trail.set = Some(set);
        let bundle = bundle.or_else(|| {
            tables_key?;
            let tables = self.session_tables(&screen.id, key, parts);
            self.mark("tables", &screen.id);
            Some(tables)
        });
        drop(decoded);
        drop(analysis);
        entry.placement = Some(placement);
        entry.placed = Some(placed);
        entry.drawn = Some(drawn);
        if let (Some(bundle), Some(output), Some(tables_key)) = (bundle, entry.output.clone(), tables_key) {
            let wanted = self.store.tables_for(&output, tables_key);
            let written = match &made {
                Some(made) if *made != wanted => crate::state::replace(made, &wanted).map_err(anyhow::Error::from),
                Some(_) => Ok(()),
                None => std::fs::create_dir_all(self.store.tables()).map_err(anyhow::Error::from).and_then(|_| crate::state::write_unsynced(&wanted, &bundle.tables_to_bytes())),
            };
            if let (Some(glider), Some(id)) = (glider.as_deref_mut(), flight) {
                glider.settle(id, &output, set, crossing.then_some(crate::glide::GLIDE_MS));
            }
            self.keep_still(&output, picture);
            self.keep_canvas(&screen.id, tables_key, bundle);
            match written {
                Ok(()) => lap("later"),
                Err(error) => eprintln!("{} tables not written to {}, {error:#}", stamp(), wanted.display()),
            }
        }
        if flight.is_none() {
            self.tell(glider, &trail);
        }
        Ok(Outcome::Rendered)
    }

    fn tell(&self, glider: Option<&mut Glider>, trail: &Move) {
        match glider {
            Some(glider) => glider.write(trail),
            None => {
                trail.write();
            }
        }
    }

    fn ready_files(&self, entry: &Entry, screen: &Screen, scene: &Scene, options: &Options) {
        let (Some(output), Some(placement)) = (entry.output.as_ref().filter(|output| output.is_file()), entry.placement.as_ref()) else { return };
        let Ok(key) = tables_key(&entry.source, options) else { return };
        let tables = self.store.tables_for(output, key);
        if !self.store.still_for(output).is_file() {
            self.keep_still(output, None);
        }
        let tables_ready = tables.is_file() && stale_tables(&tables).is_none();
        if tables_ready && self.canvas_ready(&screen.id, key) {
            return;
        }
        let Ok(pixel_key) = cache::key(&entry.source) else { return };
        let Ok(decoded) = self.pixels(&entry.source, pixel_key) else { return };
        let Ok(analysis) = cache::analysis(&self.store, pixel_key, || Ok(scooch_core::analyze(&decoded))) else { return };
        let bare_off = Options { scenery: scooch_core::Scenery::Off, ..*options };
        let options = if entry.belongs.is_some_and(|known| known.key == pixel_key && !known.answer) { &bare_off } else { options };
        let bundle = self.session_tables(&screen.id, pixel_key, (&decoded, &analysis, placement, scene, options));
        if std::fs::create_dir_all(self.store.tables()).is_ok() && (tables_ready || crate::state::write_unsynced(&tables, &bundle.tables_to_bytes()).is_ok()) {
            self.keep_canvas(&screen.id, key, bundle);
        }
    }

    fn canvas_ready(&self, screen: &str, key: u64) -> bool {
        let canvas = self.store.canvas_for(screen, key);
        cfg!(not(any(windows, target_os = "macos"))) || canvas.is_file() || self.canvases.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).contains(&canvas)
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn keep_canvas(&self, _screen: &str, _key: u64, _bundle: Arc<scooch_core::Bundle>) {}

    #[cfg(any(windows, target_os = "macos"))]
    fn keep_canvas(&self, screen: &str, key: u64, bundle: Arc<scooch_core::Bundle>) {
        let path = self.store.canvas_for(screen, key);
        if path.is_file() || !self.canvases.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(path.clone()) {
            return;
        }
        let writing = self.canvases.clone();
        let busy = Writing::begin();
        let spawned = std::thread::Builder::new().name("scooch-canvas".into()).spawn(move || {
            let _busy = busy;
            let print = crate::live::canvas::fingerprint(bundle.canvas_width, bundle.canvas_height, &bundle.canvas);
            let canvas = scooch_core::Canvas { width: bundle.canvas_width, height: bundle.canvas_height, pixels: bundle.canvas.clone() };
            drop(bundle);
            let _ = crate::live::canvas::write(&path, canvas.width, canvas.height, &scooch_live::levels(&canvas), print);
            writing.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&path);
        });
        if spawned.is_err() {
            self.canvases.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&self.store.canvas_for(screen, key));
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn keep_still(&self, _output: &Path, _picture: Option<Arc<RgbImage>>) {}

    #[cfg(any(windows, target_os = "macos"))]
    fn keep_still(&self, output: &Path, picture: Option<Arc<RgbImage>>) {
        let (target, source, desktop) = (self.store.still_for(output), output.to_path_buf(), self.desktop.clone());
        let busy = Writing::begin();
        let _ = std::thread::Builder::new().name("scooch-still".into()).spawn(move || {
            let _busy = busy;
            let image = match picture {
                Some(picture) => picture,
                None => match desktop.decode(&source) {
                    Ok(image) => Arc::new(image),
                    Err(_) => return,
                },
            };
            if crate::live::still::write_rgb(&target, &image).is_err() {
                let _ = std::fs::remove_file(&target);
            }
        });
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
        let fixed = self.desktop.reloads_picture();
        let published = self.put(entry, screen, (current, seen), extension, fixed, write);
        if fixed && !matches!(published, Ok(true)) {
            (entry.fingerprint, entry.placement) = (None, None);
        }
        published
    }

    fn put(&self, entry: &mut Entry, screen: &Screen, (current, seen): (Option<&Path>, Option<&Path>), extension: &str, fixed: bool, write: impl FnOnce(&Path) -> Result<()>) -> Result<bool> {
        let (counter, path) = self.fresh_name(&screen.id, entry.counter, extension, current);
        std::fs::create_dir_all(self.store.out()).with_context(|| format!("Scooch could not make {}, so check the disk.", self.store.out().display()))?;
        let made = [if fixed { self.store.staged(&screen.id) } else { path.clone() }];
        write(&made[0])?;
        lap("write");
        if self.picked_meanwhile(screen, seen)? || !self.note(&screen.id, &Entry { output: Some(path.clone()), counter, ..entry.clone() })? {
            discard(self.desktop.as_ref(), &self.store, &made, &made);
            return Ok(false);
        }
        lap("check");
        let placed = if fixed { crate::state::replace(&made[0], &path).map_err(|error| crate::state::unwritten(&path, error, true)) } else { Ok(()) };
        if let Err(error) = placed.and_then(|()| self.show(screen, &path)) {
            let _ = self.note(&screen.id, entry);
            discard(self.desktop.as_ref(), &self.store, &made, &made);
            return Err(error);
        }
        lap_set();
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
                if self.desktop.reloads_picture() {
                    (theirs.fingerprint, theirs.placement) = (None, None);
                }
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
        if self.desktop.reloads_picture() {
            return (counter, self.store.fixed(screen, self.desktop.format().extension()));
        }
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
    for (key, scale) in &mine.scales {
        merged.scales.insert(key.clone(), *scale);
    }
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
                theirs.places.clone_from(&ours.places);
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

pub fn holds(rect: &Rect, (x, y): (f32, f32)) -> bool {
    x >= rect.x && x < rect.x + rect.w && y >= rect.y && y < rect.y + rect.h
}

pub fn pure_move(before: &[Rect], after: &[Rect]) -> bool {
    let sizes = |icons: &[Rect]| {
        let mut sizes: Vec<[i64; 2]> = icons.iter().map(|icon| [icon.w.round() as i64, icon.h.round() as i64]).collect();
        sizes.sort_unstable();
        sizes
    };
    before.len() == after.len() && sizes(before) == sizes(after)
}

pub fn icon_print(snapshot: &Snapshot) -> u64 {
    print_of(snapshot.icons.iter())
}

pub fn screen_print(snapshot: &Snapshot, screen: &Screen) -> u64 {
    let frame = &screen.frame;
    print_of(snapshot.icons.iter().filter(|icon| {
        let (x, y) = (icon.x + icon.w / 2.0, icon.y + icon.h / 2.0);
        x >= frame.x && x < frame.x + frame.w && y >= frame.y && y < frame.y + frame.h
    }))
}

fn print_of<'a>(icons: impl Iterator<Item = &'a Rect>) -> u64 {
    let mut rects: Vec<[i64; 4]> = icons.map(|icon| [icon.x.round() as i64, icon.y.round() as i64, icon.w.round() as i64, icon.h.round() as i64]).collect();
    rects.sort_unstable();
    cache::digest(&rects.iter().flat_map(|rect| rect.iter().flat_map(|value| value.to_le_bytes())).collect::<Vec<u8>>())
}

fn stale_tables(path: &Path) -> Option<String> {
    stale_bytes(path, &std::fs::read(path).ok()?)
}

fn stale_bytes(path: &Path, bytes: &[u8]) -> Option<String> {
    let empty = scooch_core::Canvas { width: 0, height: 0, pixels: Vec::new() };
    match scooch_core::Bundle::tables_from_bytes(bytes, empty) {
        Err(error @ (scooch_core::TablesError::Version | scooch_core::TablesError::Magic | scooch_core::TablesError::Checksum | scooch_core::TablesError::Short)) => Some(format!("{} {error}", path.display())),
        _ => None,
    }
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

pub fn solved_with(options: &Options) -> String {
    format!("{}/{}/{}/{:?}", options.strength, options.max_zoom, options.mirror, options.scenery)
}

pub fn flip_hint(hidden: f32, flipped: &Placement, flipped_hidden: f32) -> bool {
    flipped.mirrored && flip_helps(hidden, flipped_hidden)
}

fn flip_check(analysis: &Analysis, scene: &Scene, options: &Options, placement: &Placement) -> bool {
    let hidden = scooch_core::hidden_share(analysis, scene, placement);
    if !worth_flipping(hidden, options.mirror) {
        return false;
    }
    let flipped = scooch_core::solve(analysis, scene, &Options { mirror: true, ..*options }, None);
    flip_hint(hidden, &flipped, scooch_core::hidden_share(analysis, scene, &flipped))
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

pub fn off_here(daemon: &Daemon) {
    let _lock = daemon.store.command_lock(crate::state::COMMAND_PATIENCE).ok();
    match crate::commands::wind_down(daemon.desktop.as_ref(), &daemon.store) {
        Ok(off) => eprintln!("{} program in the Trash, Scooch turned off with {off:?}", stamp()),
        Err(error) => eprintln!("{} program in the Trash, but Scooch could not turn off, {error:#}", stamp()),
    }
    match daemon.desktop.leave_login() {
        Ok(()) => eprintln!("{} program in the Trash, the login item removed", stamp()),
        Err(error) => eprintln!("{} program in the Trash, but the login item could not be removed, {error:#}", stamp()),
    }
}

pub fn park_left(store: &Store, login: bool) -> bool {
    let mut state = parse(store.read().as_deref());
    if !state.restoring || state.on || login || !store.state_file().exists() {
        return false;
    }
    state.restoring = false;
    store.save(&state).is_ok()
}

fn ended(store: &Store) {
    if crate::platform::login_exists() || std::env::var_os("SCOOCH_HOME").is_some_and(|value| !value.is_empty()) {
        return;
    }
    if park_left(store, false) {
        eprintln!("{} Scooch ended before every screen was back and no login item will start it again, so the rest stay parked", stamp());
    }
}

fn finish(store: &Store, stop: &Stop) -> ! {
    retire(store, stop);
    if stop.wait_idle(STOP_CAP) {
        ended(store);
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

pub const RETRY_WAITS: [Duration; 3] = [Duration::from_secs(5), Duration::from_secs(30), Duration::from_secs(120)];

pub fn retry_wait(tries: usize) -> Duration {
    RETRY_WAITS[tries.min(RETRY_WAITS.len() - 1)]
}

pub fn rearm(worker: &Signal, report: &Report, tries: usize) -> usize {
    if !report.silent {
        return 0;
    }
    worker.retry_in(retry_wait(tries));
    tries + 1
}

#[cfg(any(target_os = "linux", test))]
fn silent(error: &anyhow::Error) -> bool {
    crate::platform::linux::silent(error)
}

#[cfg(not(any(target_os = "linux", test)))]
fn silent(_error: &anyhow::Error) -> bool {
    false
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
        move || {
            #[cfg(target_os = "macos")]
            crate::platform::macos::finder::poked();
            poked.wake()
        },
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
    log_to(store.log_file());
    trim_log();
    let daemon = Arc::new(Daemon::new(desktop.clone(), store.clone()));
    let nudged = signal.clone();
    daemon.glider.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).nudge_with(Arc::new(move || nudged.wake()));
    let (looker, starter) = (daemon.clone(), daemon.clone());
    desktop.drag_starts(DragHooks { on_icon: Box::new(move |point| looker.over_icon(point)), start: Box::new(move |point| starter.drag_starting(point)) });
    let worker = signal.clone();
    let tracing = trace();
    let trashed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(target_os = "macos")]
    crate::program::watch_self(trashed.clone(), signal.clone());
    #[cfg(target_os = "linux")]
    let origin = crate::program::linux::Watcher::start(trashed.clone(), signal.clone());
    std::thread::Builder::new().name("scooch-tick".into()).spawn(move || {
        let mut said: Option<String> = None;
        let mut tries = 0;
        loop {
            begin_laps(worker.first());
            let busy = daemon.desktop.busy();
            lap("wake");
            daemon.gather(&worker);
            if !stop.begin() {
                return;
            }
            if trashed.swap(false, std::sync::atomic::Ordering::SeqCst) {
                off_here(&daemon);
            }
            let started = Instant::now();
            let report = daemon.tick();
            #[cfg(target_os = "linux")]
            if let Some(origin) = &origin {
                origin.arm(report.origin.as_deref());
            }
            drop(busy);
            relieve();
            stop.end();
            if tracing {
                eprintln!("{} tick {} ms {:?} {}, sessions hold {} MB{}", stamp(), started.elapsed().as_millis(), report.outcomes, laps(), daemon.held_bytes() >> 20, spent_line());
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
            tries = rearm(&worker, &report, tries);
            crate::icon::after_first_tick(&daemon.store);
        }
    })?;
    signal.wake();
    let result = desktop.run(Box::new(move || signal.wake()));
    ended(&store);
    let _ = store.mark(None);
    result
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::platform::linux::parse::drops::{hand, through, Guess, Hold, Press, Tracker, Verdict};
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
        pub after_read: Mutex<Option<Vec<Rect>>>,
        pub lost: Mutex<Vec<PathBuf>>,
        pub gallery: Mutex<Vec<PathBuf>>,
        pub broken: Mutex<Option<String>>,
        pub mid_render: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        pub on_set: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        pub refuse: Mutex<Vec<String>>,
        pub unknowable: Mutex<bool>,
        pub crash_after_set: Mutex<bool>,
        pub lag: Mutex<usize>,
        pub stale: Mutex<HashMap<String, (PathBuf, usize)>>,
        pub watched: Mutex<Option<Wake>>,
        pub live: Mutex<bool>,
        pub reduce: Mutex<bool>,
        pub drop_seen: Mutex<bool>,
        pub at_once: Mutex<bool>,
        pub pointer: Mutex<Option<(f32, f32)>>,
        pub button: Mutex<bool>,
        pub covered: Mutex<bool>,
        pub bad_profile: Mutex<bool>,
        pub slow_profile: Mutex<Duration>,
        pub saves: Mutex<Vec<Instant>>,
        pub predicted: Mutex<Option<Snapshot>>,
        pub leaves: AtomicUsize,
        pub installs: AtomicUsize,
        pub detached: Mutex<Vec<String>>,
        pub offered: Mutex<Option<Hold>>,
        pub hold: Mutex<Option<Hold>>,
        pub events: crate::glide::tests::Events,
        pub silent: Mutex<usize>,
        pub reloads: Mutex<bool>,
        pub writes: AtomicUsize,
        pub on_read: Mutex<std::collections::VecDeque<Turn>>,
        pub silent_set: Mutex<usize>,
    }

    pub type Turn = Box<dyn FnOnce() -> Result<()> + Send>;

    impl Fake {
        pub fn new(screens: Vec<Screen>, picture: &Path) -> Arc<Self> {
            let shown = screens.iter().map(|screen| (screen.id.clone(), picture.to_path_buf())).collect();
            Arc::new(Self { screens: Mutex::new(screens), known: Mutex::new(None), blind: Mutex::new(false), unloads: AtomicUsize::new(0), icons: Mutex::new(Vec::new()), shown: Mutex::new(shown), sets: AtomicUsize::new(0), decodes: AtomicUsize::new(0), snapshots: AtomicUsize::new(0), hook: Mutex::new(None), moving: Mutex::new(None), after_read: Mutex::new(None), lost: Mutex::new(Vec::new()), gallery: Mutex::new(Vec::new()), broken: Mutex::new(None), mid_render: Mutex::new(None), on_set: Mutex::new(None), refuse: Mutex::new(Vec::new()), unknowable: Mutex::new(false), crash_after_set: Mutex::new(false), lag: Mutex::new(0), stale: Mutex::new(HashMap::new()), watched: Mutex::new(None), live: Mutex::new(false), reduce: Mutex::new(false), drop_seen: Mutex::new(false), at_once: Mutex::new(false), pointer: Mutex::new(None), button: Mutex::new(false), covered: Mutex::new(false), bad_profile: Mutex::new(false), slow_profile: Mutex::new(Duration::ZERO), saves: Mutex::new(Vec::new()), predicted: Mutex::new(None), leaves: AtomicUsize::new(0), installs: AtomicUsize::new(0), detached: Mutex::new(Vec::new()), offered: Mutex::new(None), hold: Mutex::new(None), events: Arc::new(Mutex::new(Vec::new())), silent: Mutex::new(0), reloads: Mutex::new(false), writes: AtomicUsize::new(0), on_read: Mutex::new(std::collections::VecDeque::new()), silent_set: Mutex::new(0) })
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
            let mut left = self.silent.lock().unwrap();
            if *left > 0 {
                *left -= 1;
                return Err(anyhow::Error::new(crate::platform::linux::Silent("The Wayland display did not answer within 10 seconds, so check that the desktop still runs.".into())));
            }
            drop(left);
            let screens = self.screens()?;
            let icons = through(&mut self.hold.lock().unwrap(), self.icons.lock().unwrap().clone(), Instant::now());
            if let Some(later) = self.after_read.lock().unwrap().take() {
                *self.icons.lock().unwrap() = later;
            }
            Ok(Snapshot { screens, icons })
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
            let turn = self.on_read.lock().unwrap().pop_front();
            if let Some(turn) = turn {
                turn()?;
            }
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
            let work = self.on_set.lock().unwrap().take();
            if let Some(work) = work {
                work();
            }
            let mut quiet = self.silent_set.lock().unwrap();
            if *quiet > 0 {
                *quiet -= 1;
                return Err(anyhow::Error::new(crate::platform::linux::Silent("gsettings did not finish within 10 seconds, so check that the desktop still runs.".into())));
            }
            drop(quiet);
            if self.shown.lock().unwrap().get(&screen.id).is_none_or(|shown| shown != path) {
                self.writes.fetch_add(1, Ordering::SeqCst);
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
            match exe {
                None => self.unloads.fetch_add(1, Ordering::SeqCst),
                Some(_) => self.installs.fetch_add(1, Ordering::SeqCst),
            };
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

        fn reloads_picture(&self) -> bool {
            *self.reloads.lock().unwrap()
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

        fn reads_at_once(&self) -> bool {
            *self.at_once.lock().unwrap()
        }

        fn leave_login(&self) -> Result<()> {
            self.leaves.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn put_back_detached(&self, id: &str, picture: &Path) -> Result<bool> {
            if !self.detached.lock().unwrap().iter().any(|listed| listed == id) {
                return Ok(false);
            }
            self.shown.lock().unwrap().insert(id.to_string(), picture.to_path_buf());
            Ok(true)
        }

        fn predicted(&self, shown: &dyn Fn() -> bool) -> Option<Snapshot> {
            let offered = self.predicted.lock().unwrap().take().map(|snapshot| (snapshot, self.offered.lock().unwrap().take()));
            hand(offered, &mut self.hold.lock().unwrap(), shown)
        }

        fn place_saved(&self) -> Option<Instant> {
            let now = Instant::now();
            self.saves.lock().unwrap().iter().filter(|at| **at <= now).max().copied()
        }

        fn pointer(&self) -> Option<(f32, f32)> {
            *self.pointer.lock().unwrap()
        }

        fn desktop_at(&self, _point: (f32, f32)) -> bool {
            !*self.covered.lock().unwrap()
        }

        fn button_down(&self) -> bool {
            *self.button.lock().unwrap()
        }

        fn profile(&self) -> Option<Vec<u8>> {
            std::thread::sleep(*self.slow_profile.lock().unwrap());
            self.bad_profile.lock().unwrap().then(|| vec![0; 17 << 20])
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

    fn compact(daemon: &Daemon) -> Vec<u8> {
        let bytes = serde_json::to_vec(&daemon.store.load()).unwrap();
        std::fs::write(daemon.store.state_file(), &bytes).unwrap();
        bytes
    }

    #[test]
    fn a_placed_screen_keeps_the_icons_of_that_tick_and_the_scene_size() {
        let (daemon, fake, _) = rig("places", vec![screen("main", 0.0, 2.0)]);
        fake.place(vec![subject()]);
        let reads = fake.snapshots.load(Ordering::SeqCst);
        let before = compact(&daemon);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), reads + 1);
        assert_ne!(daemon.store.read(), Some(before));
        let places = daemon.store.load().screens["main"].places.clone();
        assert_eq!(places, Some(Places { width: 320, height: 200, icons: vec![Rect { x: 120.0, y: 60.0, w: 80.0, h: 80.0 }] }));
        fake.place(vec![corner()]);
        daemon.tick();
        assert_eq!(daemon.store.load().screens["main"].places.clone().map(|places| places.icons), Some(vec![Rect { x: 0.0, y: 0.0, w: 40.0, h: 40.0 }]));
    }

    #[test]
    fn a_tick_that_changes_nothing_writes_nothing_and_reads_the_desktop_once() {
        let (daemon, fake, _) = rig("places-idle", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let placed = daemon.store.load().screens["main"].places.clone();
        assert!(placed.is_some());
        fake.place(vec![Rect { x: 60.2, ..subject() }]);
        let reads = fake.snapshots.load(Ordering::SeqCst);
        let before = compact(&daemon);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), reads + 1);
        assert_eq!(daemon.store.read(), Some(before));
        assert_eq!(daemon.store.load().screens["main"].places, placed);
    }

    #[test]
    fn an_old_state_file_without_places_loads_and_the_next_placement_adds_them() {
        let (daemon, fake, _) = rig("places-old", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let mut old: serde_json::Value = serde_json::from_slice(&daemon.store.read().unwrap()).unwrap();
        old["screens"]["main"].as_object_mut().unwrap().remove("places");
        std::fs::write(daemon.store.state_file(), serde_json::to_vec_pretty(&old).unwrap()).unwrap();
        let loaded = daemon.store.load();
        let entry = &loaded.screens["main"];
        assert_eq!((entry.places.clone(), entry.counter, entry.icons), (None, 1, 1));
        assert!(!daemon.store.aside_file().exists());
        fake.place(vec![corner()]);
        daemon.tick();
        let text = String::from_utf8(daemon.store.read().unwrap()).unwrap();
        assert!(text.contains("\"places\""), "{text}");
        let mut newer: serde_json::Value = serde_json::from_str(&text).unwrap();
        newer["screens"]["main"]["later"] = serde_json::json!({ "from": "a newer Scooch" });
        let read: State = serde_json::from_value(newer).unwrap();
        assert_eq!(read.screens["main"].places.clone().map(|places| places.icons), Some(vec![corner()]));
    }

    #[test]
    fn an_entry_without_places_gets_them_on_the_first_unchanged_tick_and_saves_once() {
        let (daemon, fake, _) = rig("places-upgrade", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let mut old: serde_json::Value = serde_json::from_slice(&daemon.store.read().unwrap()).unwrap();
        old["screens"]["main"].as_object_mut().unwrap().remove("places");
        let older = serde_json::to_vec(&old).unwrap();
        std::fs::write(daemon.store.state_file(), &older).unwrap();
        let reads = fake.snapshots.load(Ordering::SeqCst);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), reads + 1);
        assert_ne!(daemon.store.read(), Some(older));
        assert_eq!(daemon.store.load().screens["main"].places.clone().map(|places| (places.width, places.height, places.icons)), Some((160, 100, vec![subject()])));
        fake.place(vec![Rect { x: 60.2, ..subject() }]);
        let before = compact(&daemon);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), reads + 2);
        assert_eq!(daemon.store.read(), Some(before));
    }

    #[test]
    fn an_idle_pool_thread_of_the_blocking_crate_ends() {
        struct Gone(std::sync::mpsc::Sender<()>);
        impl Drop for Gone {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        thread_local! {
            static GONE: std::cell::RefCell<Option<Gone>> = const { std::cell::RefCell::new(None) };
        }
        let (sender, ended) = std::sync::mpsc::channel();
        blocking::unblock(move || GONE.with(|gone| *gone.borrow_mut() = Some(Gone(sender)))).detach();
        assert!(ended.recv_timeout(Duration::from_secs(5)).is_ok(), "a pool thread of the blocking crate stayed after its work, so it would wake twice a second at rest");
    }

    #[test]
    fn the_tick_reads_origin_from_the_state_it_loads_with_no_new_read_or_save() {
        let (daemon, fake, _) = rig("origin-read", vec![screen("main", 0.0, 1.0)]);
        let mut state = daemon.store.load();
        state.origin = Some(PathBuf::from("/home/ada/Applications/Scooch.AppImage"));
        daemon.store.save(&state).unwrap();
        fake.place(vec![subject()]);
        let reads = fake.snapshots.load(Ordering::SeqCst);
        let report = daemon.tick();
        assert_eq!(outcome(&report), Outcome::Rendered);
        assert_eq!(report.origin, state.origin);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), reads + 1);
        let before = compact(&daemon);
        let report = daemon.tick();
        assert_eq!(outcome(&report), Outcome::Unchanged);
        assert_eq!(report.origin, state.origin);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), reads + 2);
        assert_eq!(daemon.store.read(), Some(before));
        assert_eq!(daemon.store.load().origin, state.origin);
    }

    #[test]
    fn two_screens_each_keep_their_own_icons() {
        let (daemon, fake, _) = rig("places-two", vec![screen("main", 0.0, 1.0), screen("side", 160.0, 1.0)]);
        fake.place(vec![subject(), Rect { x: 230.0, y: 10.0, w: 30.0, h: 30.0 }]);
        daemon.tick();
        let state = daemon.store.load();
        let of = |id: &str| state.screens[id].places.clone().map(|places| (places.width, places.height, places.icons));
        assert_eq!(of("main"), Some((160, 100, vec![subject()])));
        assert_eq!(of("side"), Some((160, 100, vec![Rect { x: 70.0, y: 10.0, w: 30.0, h: 30.0 }])));
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

    fn flip_case(name: &str) -> (Daemon, Arc<Fake>) {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/flip-case");
        let folder = scratch(name);
        let picture = folder.join("picture.png");
        std::fs::copy(data.join("picture.png"), &picture).unwrap();
        let laid: Scene = serde_json::from_slice(&std::fs::read(data.join("scene.json")).unwrap()).unwrap();
        let fake = Fake::new(vec![Screen { id: "main".into(), frame: Rect { x: 0.0, y: 0.0, w: laid.width as f32, h: laid.height as f32 }, scale: 1.0 }], &picture);
        let mut daemon = Daemon::new(fake.clone(), Store::at(folder.join("data")));
        daemon.grace = Duration::ZERO;
        daemon.store.save(&State { on: true, options: Options { scenery: scooch_core::Scenery::Auto, ..Options::default() }, ..State::default() }).unwrap();
        fake.place(laid.icons);
        (daemon, fake)
    }

    fn switch(daemon: &Daemon, change: impl FnOnce(&mut Options)) {
        let mut state = daemon.store.load();
        change(&mut state.options);
        daemon.store.save(&state).unwrap();
    }

    fn main_entry(daemon: &Daemon) -> Entry {
        daemon.store.load().screens["main"].clone()
    }

    #[test]
    fn where_the_flip_hint_shows_turning_flip_on_mirrors_the_picture() {
        let (daemon, _fake) = flip_case("flip-case-on");
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let before = main_entry(&daemon);
        assert!(before.flip_helps && !before.placement.as_ref().unwrap().mirrored, "{:?}", before.placement);
        switch(&daemon, |options| options.mirror = true);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let after = main_entry(&daemon);
        assert!(after.placement.as_ref().unwrap().mirrored && !after.flip_helps && after.output != before.output, "{:?}", after.placement);
        switch(&daemon, |options| options.scenery = scooch_core::Scenery::Off);
        let ticked = outcome(&daemon.tick());
        assert_eq!(main_entry(&daemon).placed, Some(solved_with(&Options { mirror: true, ..Options::default() })));
        assert!(ticked == Outcome::Rendered || (ticked == Outcome::Kept && !after.painted), "{ticked:?}");
    }

    #[test]
    fn the_flip_hint_needs_the_solve_with_flip_allowed_to_end_mirrored() {
        let mirrored = Placement { fx: Vec::new(), fy: Vec::new(), zoom: 1.0, mirrored: true, crop: [0.0; 4], shift: [0.0; 2] };
        let straight = Placement { mirrored: false, ..mirrored.clone() };
        assert!(flip_hint(0.4, &mirrored, 0.1));
        assert!(!flip_hint(0.4, &straight, 0.1));
        assert!(!flip_hint(0.4, &mirrored, 0.3));
    }

    #[test]
    fn a_placement_is_kept_only_under_the_options_it_was_solved_with() {
        let base = Options { scenery: scooch_core::Scenery::Auto, ..Options::default() };
        let key = solved_with(&base);
        for (what, changed) in [("flip", Options { mirror: true, ..base }), ("scenery", Options { scenery: scooch_core::Scenery::Off, ..base }), ("strength", Options { strength: 0.5, ..base }), ("zoom", Options { max_zoom: 1.1, ..base })] {
            assert_ne!(solved_with(&changed), key, "{what}");
        }
        assert_eq!(solved_with(&Options { pads: 0.2, ..base }), key);
    }

    #[test]
    fn a_state_saved_before_the_option_record_starts_without_drawing_and_then_follows_a_flip() {
        let (daemon, fake) = flip_case("flip-case-old");
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let mut state = daemon.store.load();
        state.screens.values_mut().for_each(|entry| entry.placed = None);
        daemon.store.save(&state).unwrap();
        assert!(!String::from_utf8(std::fs::read(daemon.store.state_file()).unwrap()).unwrap().contains("\"placed\""));
        let (old, sets) = (main_entry(&daemon), fake.sets.load(Ordering::SeqCst));
        let restarted = Daemon::new(fake.clone(), daemon.store.clone());
        assert_eq!(outcome(&restarted.tick()), Outcome::Unchanged);
        let now = main_entry(&restarted);
        assert_eq!((fake.sets.load(Ordering::SeqCst), &now.output, &now.placement), (sets, &old.output, &old.placement));
        assert!(now.placed.is_some());
        switch(&restarted, |options| options.mirror = true);
        assert_eq!(outcome(&restarted.tick()), Outcome::Rendered);
        assert!(main_entry(&restarted).placement.unwrap().mirrored);
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
    fn after_a_late_allow_the_wallpaper_follows_what_use_wrote() {
        let asking = "macOS is asking whether scooch may control Finder, so choose Allow in that window.";
        let (daemon, fake, original) = rig("late-allow", vec![screen("main", 0.0, 1.0)]);
        let signal = Arc::new(Signal::default());
        prime(fake.as_ref(), &signal);
        let wake = fake.watched.lock().unwrap().take().expect("the daemon handed the adapter a wake before ticking");
        fake.place(vec![subject()]);
        *fake.broken.lock().unwrap() = Some(asking.into());
        assert_eq!(daemon.tick().problem.as_deref(), Some(asking));
        let chosen = picture(original.parent().unwrap(), "chosen.png");
        let mut state = daemon.store.load();
        crate::commands::choose_picture(&mut state, fake.as_ref(), &daemon.store, &chosen).unwrap();
        daemon.store.save(&state).unwrap();
        assert_eq!(daemon.tick().problem.as_deref(), Some(asking));
        *fake.broken.lock().unwrap() = None;
        wake();
        let (sender, receiver) = std::sync::mpsc::channel();
        let waiting = signal.clone();
        std::thread::spawn(move || {
            waiting.first();
            let _ = sender.send(());
        });
        assert!(receiver.recv_timeout(Duration::from_secs(2)).is_ok(), "the late answer did not wake the daemon");
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!(entry.source, chosen);
        assert_eq!(Some(fake.showing("main")), entry.output);
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
    fn a_move_that_keeps_the_picture_writes_its_one_line() {
        let (daemon, fake, _, _) = glide_rig("glide-kept-line", true);
        set_scenery(&daemon, false);
        fake.place(vec![subject(), calm()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.place(vec![subject(), calm_moved()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Kept);
        let said = daemon.glider.lock().unwrap().said.clone();
        let lines: Vec<&String> = said.iter().filter(|line| line.contains(" times from the ")).collect();
        assert_eq!(lines.len(), 2, "{said:?}");
        assert!(lines[1].starts_with("skipped, kept, "), "{said:?}");
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
    fn the_trash_removes_the_login_item_at_once_even_while_a_screen_is_pending_and_an_orderly_end_parks_the_rest() {
        let (daemon, fake, original) = rig("trash-pending", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.connect(Vec::new());
        off_here(&daemon);
        assert_eq!(fake.leaves.load(Ordering::SeqCst), 1, "the login item stayed while a screen was pending");
        assert_eq!(fake.installs.load(Ordering::SeqCst), 0);
        let state = daemon.store.load();
        assert!(state.restoring && !state.on && state.screens.contains_key("main"));
        assert!(!park_left(&daemon.store, true), "a restorer with its login item keeps restoring");
        assert!(daemon.store.load().restoring);
        assert!(park_left(&daemon.store, false));
        let state = daemon.store.load();
        assert!(!state.restoring && !state.on && state.screens["main"].original == original, "the record of the pending screen was not kept parked");
        assert!(!park_left(&daemon.store, false));
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

    fn navy_image(width: u32, height: u32) -> RgbImage {
        RgbImage::from_fn(width, height, |x, y| {
            let (dx, dy) = (x as f32 / width as f32 - 0.5, y as f32 / height as f32 - 0.5);
            if dx * dx * 9.0 + dy * dy < 0.02 { [250, 240, 220].into() } else { [24, 34, 58].into() }
        })
    }

    fn sky_image(width: u32, height: u32) -> RgbImage {
        let (w, h) = (width as f32, height as f32);
        RgbImage::from_fn(width, height, |x, y| {
            let (fx, fy) = (x as f32, y as f32);
            let sky = [30.0 + 50.0 * fy / h, 60.0 + 70.0 * fy / h, 140.0 - 60.0 * fy / h + 20.0 * fx / w];
            let glow = (1.0 - ((fx - 0.6 * w).powi(2) + (fy - 0.65 * h).powi(2)).sqrt() / (0.12 * h)).clamp(0.0, 1.0).powf(0.6);
            let lit = [250.0, 196.0, 110.0];
            std::array::from_fn::<u8, 3, _>(|i| (sky[i] + (lit[i] - sky[i]) * glow).round().clamp(0.0, 255.0) as u8).into()
        })
    }

    fn navy_picture(folder: &Path) -> PathBuf {
        let path = folder.join("navy.png");
        std::fs::write(&path, scooch_core::encode_png(&navy_image(480, 100)).unwrap()).unwrap();
        path
    }

    fn navy_rig(name: &str, on: bool) -> (Daemon, Arc<Fake>, PathBuf) {
        let (daemon, fake, original) = scenery_rig(name, on);
        let navy = navy_picture(original.parent().unwrap());
        fake.show("main", &navy);
        (daemon, fake, navy)
    }

    fn bare_layout() -> Vec<Rect> {
        vec![Rect { x: 60.0, y: 30.0, w: 20.0, h: 20.0 }, Rect { x: 4.0, y: 4.0, w: 20.0, h: 20.0 }, Rect { x: 84.0, y: 24.0, w: 20.0, h: 20.0 }]
    }

    fn forget_pixels(daemon: &Daemon, picture: &Path) {
        std::fs::remove_file(cache::pixel_file(&daemon.store, cache::key(picture).unwrap())).unwrap();
    }

    fn with_scenery(options: Options, on: bool) -> Options {
        Options { scenery: if on { scooch_core::Scenery::Auto } else { scooch_core::Scenery::Off }, ..options }
    }

    #[test]
    fn a_bare_picture_with_scenery_on_sits_where_scenery_off_puts_it() {
        let (on, on_fake, navy) = navy_rig("bare-on", true);
        let (off, off_fake, _) = navy_rig("bare-off", false);
        for fake in [&on_fake, &off_fake] {
            fake.place(bare_layout());
        }
        assert_eq!(outcome(&on.tick()), Outcome::Rendered);
        assert_eq!(outcome(&off.tick()), Outcome::Rendered);
        let image = scooch_core::open(&navy).unwrap();
        let analysis = scooch_core::analyze(&image);
        let scene = scene::scene(&screen("main", 0.0, 2.0), &bare_layout());
        let options = on.store.load().options;
        assert_ne!(scooch_core::solve(&analysis, &scene, &with_scenery(options, true), None), scooch_core::solve(&analysis, &scene, &with_scenery(options, false), None));
        let (a, b) = (on.store.load().screens["main"].clone(), off.store.load().screens["main"].clone());
        assert_eq!(a.source, navy);
        assert_eq!(a.placement, b.placement);
        assert_eq!(a.flip_helps, b.flip_helps);
        assert_eq!(std::fs::read(on_fake.showing("main")).unwrap(), std::fs::read(off_fake.showing("main")).unwrap());
        assert!(!a.painted);
        assert_eq!(a.belongs.map(|known| known.answer), Some(false));
    }

    #[test]
    fn switching_scenery_off_on_a_bare_picture_keeps_placement_and_file() {
        let (daemon, fake, navy) = navy_rig("bare-switch", true);
        fake.place(bare_layout());
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        forget_pixels(&daemon, &navy);
        let before = daemon.store.load().screens["main"].clone();
        let (shown, sets, decodes) = (fake.showing("main"), fake.sets.load(Ordering::SeqCst), fake.decodes.load(Ordering::SeqCst));
        set_scenery(&daemon, false);
        assert_eq!(outcome(&daemon.tick()), Outcome::Kept);
        let after = daemon.store.load().screens["main"].clone();
        assert_eq!((after.placement, after.output), (before.placement, before.output));
        assert_eq!(fake.showing("main"), shown);
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
        assert_eq!(fake.decodes.load(Ordering::SeqCst), decodes);
    }

    #[test]
    fn with_pads_switching_scenery_off_after_a_move_on_a_bare_picture_keeps_the_file() {
        let (daemon, fake, _) = navy_rig("bare-pads", true);
        let mut state = daemon.store.load();
        state.options.pads = 0.7;
        daemon.store.save(&state).unwrap();
        fake.place(vec![calm(), calm_moved()]);
        daemon.tick();
        fake.place(bare_layout());
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let before = daemon.store.load().screens["main"].clone();
        let (shown, sets) = (fake.showing("main"), fake.sets.load(Ordering::SeqCst));
        set_scenery(&daemon, false);
        assert_eq!(outcome(&daemon.tick()), Outcome::Kept);
        let after = daemon.store.load().screens["main"].clone();
        assert_eq!((after.placement, after.output), (before.placement, before.output));
        assert_eq!(fake.showing("main"), shown);
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
    }

    #[test]
    fn switching_scenery_off_on_a_bare_picture_right_after_a_set_keeps_the_file() {
        let (mut daemon, fake, _) = navy_rig("bare-settling", true);
        daemon.grace = Duration::from_secs(30);
        let mut state = daemon.store.load();
        state.options.pads = 0.7;
        daemon.store.save(&state).unwrap();
        fake.place(bare_layout());
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let before = daemon.store.load().screens["main"].clone();
        let (shown, sets) = (fake.showing("main"), fake.sets.load(Ordering::SeqCst));
        set_scenery(&daemon, false);
        assert_eq!(outcome(&daemon.tick()), Outcome::Kept);
        let after = daemon.store.load().screens["main"].clone();
        assert_eq!((after.placement, after.output), (before.placement, before.output));
        assert_eq!(fake.showing("main"), shown);
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
    }

    #[test]
    fn a_foreign_wallpaper_inside_the_settle_window_is_taken_once_the_window_ends() {
        let (mut daemon, fake, navy) = navy_rig("bare-foreign", true);
        daemon.grace = Duration::from_secs(3600);
        fake.place(bare_layout());
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let ours = daemon.store.load().screens["main"].output.clone();
        let sets = fake.sets.load(Ordering::SeqCst);
        let foreign = picture(navy.parent().unwrap(), "foreign.png");
        fake.show("main", &foreign);
        set_scenery(&daemon, false);
        assert_eq!(outcome(&daemon.tick()), Outcome::Kept);
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
        assert_eq!(fake.showing("main"), foreign);
        assert_eq!(daemon.store.load().screens["main"].output, ours);
        daemon.grace = Duration::ZERO;
        let after = outcome(&daemon.tick());
        assert!(matches!(after, Outcome::Rendered | Outcome::Plain), "{after:?}");
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.source, entry.original), (foreign.clone(), foreign));
    }

    #[test]
    fn the_belonging_answer_holds_until_what_it_rests_on_changes() {
        let (daemon, fake, navy) = navy_rig("bare-answer", true);
        let entry = |daemon: &Daemon| daemon.store.load().screens["main"].clone();
        fake.place(vec![subject(), calm()]);
        daemon.tick();
        let first = entry(&daemon).belongs.unwrap();
        assert!(!first.answer);
        forget_pixels(&daemon, &navy);
        let decodes = fake.decodes.load(Ordering::SeqCst);
        fake.place(vec![subject(), calm_moved()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Kept);
        assert_eq!(entry(&daemon).belongs, Some(first));
        assert_eq!(fake.decodes.load(Ordering::SeqCst), decodes);
        fake.place(vec![Rect { w: 50.0, h: 50.0, ..subject() }, Rect { w: 30.0, h: 30.0, ..calm_moved() }]);
        daemon.tick();
        let sized = entry(&daemon).belongs.unwrap();
        assert_ne!(sized.stamp, first.stamp);
        fake.connect(vec![screen("main", 0.0, 1.0)]);
        daemon.tick();
        let screened = entry(&daemon).belongs.unwrap();
        assert_ne!(screened.stamp, sized.stamp);
        set_scenery(&daemon, false);
        daemon.tick();
        assert_eq!(entry(&daemon).belongs, Some(screened));
        set_scenery(&daemon, true);
        daemon.tick();
        assert_eq!(entry(&daemon).belongs, Some(screened));
        let teal = picture(navy.parent().unwrap(), "teal.png");
        fake.show("main", &teal);
        daemon.tick();
        let changed = entry(&daemon).belongs.unwrap();
        assert_ne!(changed.key, first.key);
        assert!(changed.answer);
    }

    #[test]
    fn scenery_off_then_on_again_on_a_bare_picture_decodes_nothing() {
        let (daemon, fake, navy) = navy_rig("bare-off-on", true);
        fake.place(bare_layout());
        daemon.tick();
        let first = daemon.store.load().screens["main"].belongs.unwrap();
        forget_pixels(&daemon, &navy);
        let decodes = fake.decodes.load(Ordering::SeqCst);
        set_scenery(&daemon, false);
        daemon.tick();
        set_scenery(&daemon, true);
        daemon.tick();
        assert_eq!(daemon.store.load().screens["main"].belongs, Some(first));
        assert_eq!(fake.decodes.load(Ordering::SeqCst), decodes);
    }

    #[test]
    fn a_stored_answer_with_another_stamp_is_worked_out_again() {
        let (daemon, fake, _) = navy_rig("bare-stamp", true);
        fake.place(bare_layout());
        daemon.tick();
        let real = daemon.store.load().screens["main"].belongs.unwrap();
        let mut state = daemon.store.load();
        state.screens.get_mut("main").unwrap().belongs = Some(crate::state::Belongs { stamp: real.stamp ^ 1, answer: true, ..real });
        daemon.store.save(&state).unwrap();
        let mut moved = bare_layout();
        moved[2].y += 8.0;
        fake.place(moved);
        daemon.tick();
        assert_eq!(daemon.store.load().screens["main"].belongs, Some(real));
    }

    #[test]
    fn a_picture_with_scenery_paints_and_places_as_before() {
        let (daemon, fake, original) = scenery_rig("scenery-belongs", true);
        fake.place(bare_layout());
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!(entry.belongs.map(|known| known.answer), Some(true));
        assert!(entry.painted);
        let image = scooch_core::open(&original).unwrap();
        let scene = scene::scene(&screen("main", 0.0, 2.0), &bare_layout());
        assert_eq!(entry.placement, Some(scooch_core::solve(&scooch_core::analyze(&image), &scene, &daemon.store.load().options, None)));
    }

    #[test]
    fn every_step_of_a_live_drag_on_a_bare_picture_places_like_scenery_off() {
        let rigs: Vec<(Daemon, Arc<Fake>)> = [true, false]
            .into_iter()
            .map(|on| {
                let (daemon, fake, _, _) = glide_rig(&format!("bare-drag-{on}"), true);
                set_scenery(&daemon, on);
                let navy = navy_picture(&scratch(&format!("bare-drag-picture-{on}")));
                fake.show("main", &navy);
                (daemon, fake)
            })
            .collect();
        let held = bare_layout();
        for (step, x) in [8.0, 24.0, 40.0, 60.0, 72.0, 88.0].into_iter().enumerate() {
            let mut icons = held.clone();
            icons[0] = Rect { x, ..held[0] };
            for (daemon, fake) in &rigs {
                fake.place(icons.clone());
                daemon.tick();
            }
            let placements: Vec<Option<Placement>> = rigs.iter().map(|(daemon, _)| daemon.store.load().screens["main"].placement.clone()).collect();
            assert!(placements[0].is_some(), "step {step}");
            assert_eq!(placements[0], placements[1], "step {step}");
        }
    }

    #[test]
    fn the_stored_answer_gives_the_options_the_session_gives() {
        for (name, make) in [("navy", navy_image as fn(u32, u32) -> RgbImage), ("sky", sky_image)] {
            let (daemon, fake, original) = scenery_rig(&format!("agree-{name}"), true);
            let path = original.parent().unwrap().join(format!("{name}.png"));
            std::fs::write(&path, scooch_core::encode_png(&make(480, 100)).unwrap()).unwrap();
            fake.show("main", &path);
            fake.place(bare_layout());
            daemon.tick();
            let state = daemon.store.load();
            let entry = state.screens["main"].clone();
            let image = scooch_core::open(&path).unwrap();
            let analysis = scooch_core::analyze(&image);
            let scene = scene::scene(&screen("main", 0.0, 2.0), &bare_layout());
            let chosen = scooch_core::Session::new().options(&image, &analysis, &scene, &state.options);
            let stored = if entry.belongs.unwrap().answer { state.options } else { with_scenery(state.options, false) };
            assert_eq!(stored, chosen, "{name}");
            assert_eq!(entry.placement, Some(scooch_core::solve(&analysis, &scene, &chosen, None)), "{name}");
        }
    }

    #[test]
    fn the_render_command_places_a_bare_picture_like_the_daemon() {
        let (daemon, fake, navy) = navy_rig("bare-cli", true);
        fake.place(bare_layout());
        daemon.tick();
        let scene = scene::scene(&screen("main", 0.0, 2.0), &bare_layout());
        let made = crate::commands::rendered(&scooch_core::open(&navy).unwrap(), &scene, false);
        let placement: Placement = serde_json::from_value(made.report["placement"].clone()).unwrap();
        assert_eq!(Some(placement), daemon.store.load().screens["main"].placement);
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
        waits.iter().all(|(_, wait)| *wait <= crate::glide::PATIENCE) && shared.windows(2).all(|pair| pair[1] <= pair[0])
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

    fn marked_rig(name: &str) -> Rigged {
        let rigged = glide_rig(name, true);
        *rigged.0.marks.lock().unwrap() = Some(rigged.1.events.clone());
        rigged
    }

    fn moved_again(daemon: &Daemon, fake: &Fake) -> (Report, Vec<String>) {
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        let report = daemon.tick();
        (report, events(fake))
    }

    fn name_of(path: &Path) -> String {
        path.file_name().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn the_glide_command_goes_out_before_the_still_is_drawn() {
        let (daemon, fake, _, _) = marked_rig("glide-first");
        let (report, events) = moved_again(&daemon, &fake);
        assert_eq!(outcome(&report), Outcome::Rendered);
        let (tables, glide, drawing, set) = (at(&events, "tables main"), at(&events, "glide "), at(&events, "drawing main"), at(&events, "set "));
        assert!(tables < glide && tables < drawing && glide < set && drawing < set, "{events:?}");
    }

    #[test]
    fn the_settle_carries_the_new_picture_and_the_wallpaper_is_set_to_it() {
        let (daemon, fake, _, _) = marked_rig("glide-settle-new");
        let old = daemon.store.output("main", 1, "jpg");
        let (report, events) = moved_again(&daemon, &fake);
        assert_eq!(outcome(&report), Outcome::Rendered);
        let new = fake.showing("main");
        assert!(new != old && daemon.store.load().screens["main"].output.as_ref() == Some(&new), "{new:?}");
        let (set, settle, settled) = (at(&events, "set "), at(&events, "settle "), at(&events, "settled 1"));
        assert!(at(&events, "glide ") < set && set < settle && settle < settled, "{events:?}");
        assert_eq!((events[set].clone(), events[settle].clone()), (format!("set {}", name_of(&new)), format!("settle 1 {}", name_of(&new))));
        assert_eq!(std::fs::read(&new).unwrap(), today("glide-settle-new-today"));
    }

    #[test]
    fn a_failed_encode_after_the_glide_started_ends_on_the_old_picture_with_nothing_of_the_helper_left() {
        let (daemon, fake, _, _) = marked_rig("glide-bad-encode");
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let old = fake.showing("main");
        fake.events.lock().unwrap().clear();
        *fake.bad_profile.lock().unwrap() = true;
        fake.place(vec![moved_subject()]);
        let report = daemon.tick();
        let events = events(&fake);
        assert!(report.problem.is_some() && report.outcomes.iter().all(|(_, outcome)| *outcome == Outcome::Skipped), "{report:?} {events:?}");
        let (glide, settle, settled, close) = (at(&events, "glide "), at(&events, "settle "), at(&events, "settled 1"), at(&events, "close 1"));
        assert!(glide < settle && settle < settled && settled < close, "{events:?}");
        assert_eq!(events[settle], format!("settle 1 {}", name_of(&old)), "{events:?}");
        assert!(!events.iter().any(|event| event.starts_with("set ")), "{events:?}");
        assert_eq!(fake.showing("main"), old);
        assert_eq!(daemon.store.load().screens["main"].output.as_ref(), Some(&old));
        let opened = events.iter().filter(|event| event.starts_with("open ")).count();
        let closed = events.iter().filter(|event| event.starts_with("close ")).count();
        assert!(opened == 1 && closed == 1 && !daemon.glider.lock().unwrap().in_flight(), "{events:?}");
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.starts_with("settle 1 back on") && line.contains("could not be encoded")), "{said:?}");
    }

    #[test]
    fn a_move_that_changes_nothing_starts_no_glide() {
        let mut seen = Vec::new();
        for (name, picture, scenery) in [("unmoved-sample", None, false), ("unmoved-sample-on", None, true), ("unmoved-navy-on", Some(navy_image(480, 100)), true)] {
            let (daemon, fake, _, _) = glide_rig(name, true);
            let mut state = daemon.store.load();
            state.options.pads = 0.7;
            state.options.scenery = if scenery { scooch_core::Scenery::Auto } else { scooch_core::Scenery::Off };
            daemon.store.save(&state).unwrap();
            if let Some(image) = &picture {
                let path = daemon.store.out().parent().unwrap().join(format!("{name}.png"));
                std::fs::write(&path, scooch_core::encode_png(image).unwrap()).unwrap();
                fake.show("main", &path);
            }
            fake.place(vec![subject(), calm()]);
            assert_eq!(outcome(&daemon.tick()), Outcome::Rendered, "{name}");
            for (step, icons) in [("calm", vec![subject(), calm_moved()]), ("subject", vec![moved_subject(), calm_moved()])] {
                fake.events.lock().unwrap().clear();
                daemon.glider.lock().unwrap().said.clear();
                let before = fake.showing("main");
                fake.place(icons);
                let ticked = outcome(&daemon.tick());
                let events = events(&fake);
                let said = daemon.glider.lock().unwrap().said.clone();
                let glided = events.iter().any(|event| event.starts_with("glide "));
                let unmoved = said.iter().any(|line| line.starts_with("skipped, nothing moves"));
                let same = fake.showing("main") == before;
                assert_eq!((ticked == Outcome::Kept, same), (unmoved, unmoved), "{name} {step}: the tables check and the equal bytes check disagree, {ticked:?} {events:?} {said:?}");
                assert_eq!(glided, !unmoved, "{name} {step} {ticked:?} {events:?} {said:?}");
                seen.push((name, step, unmoved));
            }
        }
        assert!(seen.iter().any(|(_, _, unmoved)| *unmoved) && seen.iter().any(|(_, _, unmoved)| !unmoved) && seen.iter().all(|(_, step, unmoved)| *step == "calm" || !unmoved), "{seen:?}");
    }

    #[test]
    fn two_screens_that_both_change_each_start_their_glide_at_their_own_tables() {
        let (daemon, fake, _) = two_screen_rig("glide-two-first");
        *daemon.marks.lock().unwrap() = Some(fake.events.clone());
        fake.place(vec![subject(), right(subject())]);
        daemon.tick();
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject(), right(moved_subject())]);
        let report = daemon.tick();
        assert_eq!(report.outcomes.iter().map(|(_, outcome)| *outcome).collect::<Vec<_>>(), vec![Outcome::Rendered, Outcome::Rendered]);
        let events = events(&fake);
        let place = |start: &str| at(&events, start);
        let set_of = |screen: &str| events.iter().position(|event| event.starts_with(&format!("set {screen}-"))).unwrap_or_else(|| panic!("{events:?}"));
        let (left, right) = ((place("tables left"), place("glide 1 "), place("drawing left"), set_of("left")), (place("tables right"), place("glide 2 "), place("drawing right"), set_of("right")));
        for (tables, glide, drawing, set) in [left, right] {
            assert!(tables < glide && tables < drawing && glide < set && drawing < set, "{events:?}");
        }
        assert!(left.1 < right.0 && left.3 < right.0, "{events:?}");
    }

    #[test]
    fn with_a_helper_shown_300_ms_after_the_wake_the_still_is_drawn_beside_the_wait() {
        let (daemon, fake, script, _) = marked_rig("glide-beside");
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        script.lock().unwrap().shown_after = Duration::from_millis(300);
        fake.events.lock().unwrap().clear();
        let woke = Instant::now();
        *daemon.woke.lock().unwrap() = Some(woke);
        daemon.glider.lock().unwrap().woke(woke);
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        let (tables, drawing, shown, glide, set) = (at(&events, "tables main"), at(&events, "drawing main"), at(&events, "shown "), at(&events, "glide "), at(&events, "set "));
        assert!(tables < drawing && drawing < shown && shown < glide && glide < set, "{events:?}");
        assert_eq!(events.iter().filter(|event| event.starts_with("drawing ")).count(), 1, "{events:?}");
    }

    #[test]
    fn a_helper_shown_at_once_gets_its_glide_before_the_still_is_done() {
        let (daemon, fake, _, _) = marked_rig("glide-due");
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        *fake.slow_profile.lock().unwrap() = Duration::from_millis(200);
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        let (glide, drawn, set) = (at(&events, "glide "), at(&events, "drawn main"), at(&events, "set "));
        assert!(glide < set && drawn < set, "{events:?}");
        assert!(when(&fake, "glide ") + Duration::from_millis(150) <= when(&fake, "set "), "the glide waited for the still {events:?}");
    }

    #[test]
    fn the_helper_parks_after_the_first_tick_and_lives_through_moves() {
        let (daemon, fake, _, spawned) = glide_rig("glide-parks", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(*spawned.lock().unwrap(), 1);
        assert!(events(&fake).contains(&"park".to_string()), "{:?}", events(&fake));
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.starts_with("skipped, system picture, the system's own picture is on screen")), "the first set did not say the system's picture was on screen {said:?}");
        let mut rendered = 0;
        for spot in 0..12 {
            fake.place(vec![Rect { x: 30.0 + (spot * 13 % 70) as f32, y: 20.0 + (spot * 7 % 40) as f32, w: 40.0, h: 40.0 }]);
            if outcome(&daemon.tick()) == Outcome::Rendered {
                rendered += 1;
            }
        }
        assert_eq!(*spawned.lock().unwrap(), 1, "{:?}", events(&fake));
        assert!(rendered >= 3, "{rendered}");
        assert_eq!(events(&fake).iter().filter(|event| event.starts_with("glide ")).count(), rendered, "{:?}", events(&fake));
        assert!(!events(&fake).contains(&"quit".to_string()));
        *fake.reduce.lock().unwrap() = true;
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert!(events(&fake).contains(&"quit".to_string()) && !daemon.glider.lock().unwrap().running(), "reduced motion left the helper parked");
        *fake.reduce.lock().unwrap() = false;
        daemon.tick();
        assert_eq!(*spawned.lock().unwrap(), 2, "the helper did not park again when the glide could apply");
    }

    #[test]
    fn a_glide_opens_before_the_set_and_settles_and_closes_after_it() {
        let (daemon, fake, _, spawned) = glide_rig("glide-order", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        if cfg!(any(windows, target_os = "macos")) {
            let key = tables_key(&daemon.store.load().screens["main"].source, &Options::default()).unwrap();
            let first = [daemon.store.canvas_for("main", key), daemon.store.still_for(&fake.showing("main"))];
            let started = Instant::now();
            while !first.iter().all(|file| file.is_file()) && started.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(first.iter().all(|file| file.is_file()), "the first set left no canvas or raw still {first:?}");
        }
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
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
        let key = tables_key(&daemon.store.load().screens["main"].source, &Options::default()).unwrap();
        let mut wanted = vec![daemon.store.tables_for(&second, key)];
        if cfg!(any(windows, target_os = "macos")) {
            wanted.push(daemon.store.canvas_for("main", key));
            wanted.push(daemon.store.still_for(&second));
            let started = Instant::now();
            while !wanted.iter().all(|file| file.is_file()) && started.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(20));
            }
            let canvas = crate::live::canvas::read(&daemon.store.canvas_for("main", key)).unwrap().canvas().unwrap();
            let tables = std::fs::read(daemon.store.tables_for(&second, key)).unwrap();
            assert_eq!(crate::live::canvas::tables_print(&tables), Some(crate::live::canvas::fingerprint(canvas.width, canvas.height, &canvas.pixels)));
            let raw = crate::live::still::read(&daemon.store.still_for(&second)).unwrap();
            let decoded = fake.decode(&second).unwrap();
            assert_eq!((raw.width, raw.height), decoded.dimensions());
            assert!(raw.rgba().as_chunks::<4>().0.iter().zip(decoded.as_raw().as_chunks::<3>().0).all(|(raw, rgb)| raw[..3] == rgb[..]), "the raw still is not the picture set");
            assert!(events.iter().any(|event| event.starts_with("warm ") && event.contains(" canvas")), "{events:?}");
        }
        let mut kept: Vec<_> = std::fs::read_dir(daemon.store.tables()).unwrap().flatten().map(|file| file.path()).collect();
        kept.sort();
        wanted.sort();
        assert_eq!(kept, wanted);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn the_log_is_cut_to_its_newest_quarter_megabyte_once_it_passes_a_megabyte() {
        let folder = scratch("log-cut");
        let path = folder.join("watch.log");
        let line = |index: usize| format!("{index:08} glide skipped, helper cold, a line of the log that is long enough to count\n");
        let mut text = String::new();
        let mut index = 0;
        while text.len() as u64 <= LOG_MOST {
            text.push_str(&line(index));
            index += 1;
        }
        std::fs::write(&path, &text).unwrap();
        let open_log = |path: &Path| {
            use std::io::Seek;
            let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(false).append(cfg!(unix)).open(path).unwrap();
            file.seek(std::io::SeekFrom::End(0)).unwrap();
            file
        };
        let open = open_log(&path);
        let other = open_log(&folder.join("other.log"));
        assert!(!trim(&path, &other), "a file other than the log was cut");
        assert!(trim(&path, &open));
        let kept = std::fs::read_to_string(&path).unwrap();
        assert!(kept.len() as u64 <= LOG_KEPT && kept.len() as u64 > LOG_KEPT - 200, "{}", kept.len());
        assert!(kept.starts_with(char::is_numeric) && kept.ends_with(&line(index - 1)), "the cut did not keep whole newest lines");
        use std::io::Write;
        writeln!(&open, "after the cut").unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().ends_with("after the cut\n"));
        assert!(!trim(&path, &open), "a short log was cut");
    }

    #[test]
    fn a_picture_chosen_in_scooch_fades_from_the_picture_on_screen() {
        let (daemon, fake, script, _) = glide_rig("glide-cross", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let old = fake.showing("main");
        let other = daemon.store.out().parent().unwrap().join("chosen.png");
        std::fs::write(&other, scooch_core::encode_png(&navy_image(480, 100)).unwrap()).unwrap();
        let choose = |daemon: &Daemon| {
            let mut state = daemon.store.load();
            for entry in state.screens.values_mut() {
                (entry.source, entry.placement, entry.fingerprint, entry.chosen) = (other.clone(), None, None, true);
            }
            state.picture = Some(other.clone());
            daemon.store.save(&state).unwrap();
        };
        choose(&daemon);
        fake.events.lock().unwrap().clear();
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let seen = events(&fake);
        let new = fake.showing("main");
        assert!(new != old, "{seen:?}");
        let (open, shown, set, settle, settled, close) = (at(&seen, "open "), at(&seen, "shown "), at(&seen, "set "), at(&seen, "settle "), at(&seen, "settled"), at(&seen, "close"));
        assert!(open < shown && shown < set && set < settle && settle < settled && settled < close, "{seen:?}");
        assert_eq!(seen[open], format!("open 1 {}", name_of(&old)));
        assert_eq!(seen[settle], format!("settle 1 {} over {} ms", name_of(&new), crate::glide::GLIDE_MS));
        assert!(!seen.iter().any(|event| event.starts_with("glide ")), "{seen:?}");
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.starts_with("slid, crossfaded, the picture changed")), "{said:?}");
        script.lock().unwrap().slow_hello = Duration::from_secs(60);
        daemon.glider.lock().unwrap().reset_for_test();
        daemon.glider.lock().unwrap().keep_parked();
        std::fs::write(&other, scooch_core::encode_png(&scene::sample(480, 100)).unwrap()).unwrap();
        choose(&daemon);
        fake.events.lock().unwrap().clear();
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(!events(&fake).iter().any(|event| event.starts_with("open ")), "{:?}", events(&fake));
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.starts_with("skipped, helper cold") && line.contains("the picture changed")), "{said:?}");
    }

    #[test]
    fn a_tick_over_scoochs_own_picture_makes_its_missing_tables_and_raw_still() {
        let (daemon, fake, _, _) = glide_rig("glide-ready-files", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let state = daemon.store.load();
        let output = state.screens["main"].output.clone().unwrap();
        let tables = daemon.store.tables_for(&output, tables_key(&state.screens["main"].source, &state.options).unwrap());
        let raw = daemon.store.still_for(&output);
        let started = Instant::now();
        while !(tables.is_file() && (raw.is_file() || cfg!(not(any(windows, target_os = "macos"))))) && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let made = std::fs::read(&tables).unwrap();
        std::fs::remove_file(&tables).unwrap();
        let _ = std::fs::remove_file(&raw);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!(std::fs::read(&tables).ok(), Some(made), "the tables made again differ from the tables of the set");
        if cfg!(any(windows, target_os = "macos")) {
            let started = Instant::now();
            while !raw.is_file() && started.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(raw.is_file(), "no raw still was made for the picture on screen");
        }
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(events(&fake).iter().any(|event| event.starts_with("glide ")), "{:?}", events(&fake));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_late_canvas_thread_leaves_the_canvas_on_screen_and_a_missing_canvas_is_made_before_a_move() {
        let (daemon, fake, _, _) = glide_rig("glide-late-canvas", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let state = daemon.store.load();
        let key = tables_key(&state.screens["main"].source, &state.options).unwrap();
        let canvas = daemon.store.canvas_for("main", key);
        let wait = |path: &Path| {
            let started = Instant::now();
            while !path.is_file() && started.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(20));
            }
            path.is_file()
        };
        assert!(wait(&canvas), "no canvas after the first set");
        let image = RgbImage::from_fn(32, 24, |x, y| [(x * 8) as u8, (y * 10) as u8, 90].into());
        let analysis = scooch_core::analyze(&image);
        let scene = Scene { width: 32, height: 24, icons: Vec::new() };
        let options = Options::default();
        let placement = scooch_core::solve(&analysis, &scene, &options, None);
        daemon.keep_canvas("main", key ^ 1, Arc::new(scooch_core::bundle(&image, &analysis, &placement, &scene, &options)));
        assert!(wait(&daemon.store.canvas_for("main", key ^ 1)), "the late canvas was never written");
        std::thread::sleep(Duration::from_millis(300));
        assert!(canvas.is_file(), "a late canvas thread deleted the canvas of the picture on screen");
        std::fs::remove_file(&canvas).unwrap();
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert!(wait(&canvas), "a canvas missing beside its tables was not made again before a move");
    }

    #[test]
    fn the_tidy_spares_a_file_being_written_and_clears_one_left_by_a_crash() {
        let (daemon, fake, _, _) = glide_rig("glide-tidy", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let (fresh, stale, stray) = (daemon.store.tables().join(".main-9.jpg.still.1.2.part"), daemon.store.tables().join(".main-8.jpg.still.1.1.part"), daemon.store.tables().join("main-7.jpg.still"));
        for file in [&fresh, &stale, &stray] {
            std::fs::write(file, b"part").unwrap();
        }
        std::fs::File::options().write(true).open(&stale).unwrap().set_modified(std::time::SystemTime::now() - Duration::from_secs(120)).unwrap();
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(fresh.is_file(), "a file being written was deleted");
        assert!(!stale.exists() && !stray.exists(), "a file left by a crash or an old output was kept");
    }

    #[test]
    fn a_torn_tables_file_ends_in_a_plain_swap_and_a_fresh_file() {
        let (daemon, fake, _, _) = glide_rig("torn-tables", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let tables_of = |daemon: &Daemon| {
            let state = daemon.store.load();
            daemon.store.tables_for(state.screens["main"].output.as_ref().unwrap(), tables_key(&state.screens["main"].source, &state.options).unwrap())
        };
        let torn = tables_of(&daemon);
        let bytes = std::fs::read(&torn).unwrap();
        std::fs::write(&torn, &bytes[..bytes.len() / 2]).unwrap();
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(!events(&fake).iter().any(|event| event.starts_with("glide ")), "a torn tables file glided");
        let fresh = tables_of(&daemon);
        assert!(fresh != torn && fresh.is_file() && stale_tables(&fresh).is_none(), "{fresh:?}");
    }

    fn engine(events: &[String]) -> Vec<String> {
        events.iter().filter(|event| ["tables ", "drawing ", "drawn ", "rendering "].iter().any(|call| event.starts_with(call))).cloned().collect()
    }

    fn cold_skip(name: &str, scenery: bool) -> (Daemon, Arc<Fake>, Arc<Mutex<crate::glide::tests::Script>>, Vec<String>) {
        let (daemon, fake, script, _) = marked_rig(name);
        set_scenery(&daemon, scenery);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        script.lock().unwrap().slow_hello = Duration::from_secs(60);
        daemon.glider.lock().unwrap().reset_for_test();
        daemon.glider.lock().unwrap().keep_parked();
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        (daemon, fake, script, events)
    }

    fn glide_off_move(name: &str, scenery: bool) -> Vec<String> {
        let (daemon, fake, _, _) = glide_rig(name, false);
        *daemon.marks.lock().unwrap() = Some(fake.events.clone());
        set_scenery(&daemon, scenery);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        events(&fake)
    }

    fn timed_tick(daemon: &Daemon) -> (Outcome, String) {
        begin_laps(Instant::now());
        let report = daemon.tick();
        let laps = laps();
        (outcome(&report), format!("{laps}{}", spent_line()))
    }

    fn writers_done() {
        let started = Instant::now();
        while WRITERS.load(std::sync::atomic::Ordering::SeqCst) > 0 && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    #[test]
    #[ignore]
    fn the_pieces_of_a_cold_move_and_an_off_move_timed() {
        for (width, height, scenery) in [(1024u32, 768u32, scooch_core::Scenery::Off), (1920, 1080, scooch_core::Scenery::Off), (1024, 768, scooch_core::Scenery::Auto), (1920, 1080, scooch_core::Scenery::Auto)] {
            let name = format!("pieces-{width}-{scenery:?}");
            let (mut daemon, fake, _) = rig(&name, vec![Screen { id: "main".into(), frame: Rect { x: 0.0, y: 0.0, w: width as f32, h: height as f32 }, scale: 1.0 }]);
            daemon.grace = Duration::ZERO;
            *fake.live.lock().unwrap() = true;
            let spawner = crate::glide::tests::FakeSpawner::new(fake.events.clone());
            let script = spawner.script.clone();
            daemon.glider = Arc::new(Mutex::new(Glider::new(Box::new(spawner))));
            let mut state = daemon.store.load();
            state.options = Options { scenery, ..Options::default() };
            daemon.store.save(&state).unwrap();
            let folder = scratch(&format!("{name}-picture"));
            let (cx, cy, radius) = (width as f32 / 2.0, height as f32 / 2.0, height as f32 / 7.0);
            let disc = RgbImage::from_fn(width, height, |x, y| if ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < radius { [245, 196, 92].into() } else { scooch_core::ISLAND_WATER.into() });
            let path = folder.join("disc.jpg");
            std::fs::write(&path, scooch_core::encode_jpeg(&disc, 92).unwrap()).unwrap();
            fake.show("main", &path);
            let icon = |x: f32, y: f32| Rect { x: x - 52.0, y: y - 56.0, w: 104.0, h: 112.0 };
            let column: Vec<Rect> = (1..4).map(|index| icon(width as f32 * 0.85, height as f32 * 0.25 + 110.0 * index as f32)).collect();
            let at = |x: f32| [vec![icon(x, height as f32 * 0.5)], column.clone()].concat();
            let (subject, aside) = (width as f32 * 0.5, width as f32 * 0.38);
            fake.place(at(subject));
            println!("{width}x{height} {scenery:?} first {:?}", timed_tick(&daemon));
            writers_done();
            fake.place(at(aside));
            println!("{width}x{height} {scenery:?} second {:?}", timed_tick(&daemon));
            writers_done();
            let glide = |on: bool| {
                let mut state = daemon.store.load();
                state.glide = on;
                daemon.store.save(&state).unwrap();
            };
            for pair in 0..7 {
                script.lock().unwrap().slow_hello = Duration::from_secs(60);
                daemon.glider.lock().unwrap().reset_for_test();
                daemon.glider.lock().unwrap().keep_parked();
                println!("{width}x{height} {scenery:?} pair {pair} toggle on {:?}", timed_tick(&daemon));
                writers_done();
                fake.place(at(aside));
                println!("{width}x{height} {scenery:?} pair {pair} back {:?}", timed_tick(&daemon));
                writers_done();
                fake.place(at(subject));
                println!("{width}x{height} {scenery:?} pair {pair} slow {:?}", timed_tick(&daemon));
                writers_done();
                glide(false);
                println!("{width}x{height} {scenery:?} pair {pair} toggle off {:?}", timed_tick(&daemon));
                writers_done();
                fake.place(at(aside));
                println!("{width}x{height} {scenery:?} pair {pair} back {:?}", timed_tick(&daemon));
                writers_done();
                fake.place(at(subject));
                println!("{width}x{height} {scenery:?} pair {pair} off {:?}", timed_tick(&daemon));
                writers_done();
                glide(true);
            }
        }
    }

    #[test]
    fn the_tick_line_gives_the_cpu_from_the_wake_to_the_set_and_the_writers_beside_it() {
        let woke = Spent { thread: Duration::from_millis(10), process: Duration::from_millis(30), writers: 1 };
        let set = Spent { thread: Duration::from_millis(25), process: Duration::from_millis(70), writers: 0 };
        assert_eq!(spent_words(Some(woke), Some(set)), ", cpu to the set 15 ms on the tick thread and 40 ms in the daemon, writers 1 at the wake and 0 at the set");
        assert_eq!(spent_words(Some(woke), None), "");
    }

    #[cfg(unix)]
    #[test]
    fn the_cpu_of_a_tick_counts_its_own_thread_and_not_its_sleep() {
        let (before, began) = (spent(), Instant::now());
        std::thread::sleep(Duration::from_millis(60));
        let busy = Instant::now();
        while spent().thread.saturating_sub(before.thread) < Duration::from_millis(25) && busy.elapsed() < Duration::from_secs(2) {
            std::hint::black_box((0..1000u64).sum::<u64>());
        }
        let (after, wall) = (spent(), began.elapsed());
        let (thread, process) = (after.thread.saturating_sub(before.thread), after.process.saturating_sub(before.process));
        assert!(thread >= Duration::from_millis(20) && thread + Duration::from_millis(40) <= wall && process + Duration::from_millis(1) >= thread, "{before:?} {after:?} {wall:?}");
    }

    #[test]
    fn a_helper_that_has_not_parked_builds_no_tables_and_opens_no_layer_before_the_set() {
        let (_, _, _, events) = cold_skip("cold-skip-tables", true);
        assert!(!events.iter().any(|event| event.starts_with("tables ") || event.starts_with("open ") || event.starts_with("glide ")), "{events:?}");
        let set = events.iter().position(|event| event.starts_with("set ")).unwrap_or_else(|| panic!("{events:?}"));
        assert!(at(&events, "drawing main") < set, "{events:?}");
    }

    fn before_set(events: &[String]) -> Vec<String> {
        let set = at(events, "set ");
        engine(&events[..set])
    }

    #[test]
    fn a_cold_helper_draws_like_the_glide_off_and_makes_the_next_tables_after_the_set() {
        for scenery in [true, false] {
            let (daemon, fake, _, cold) = cold_skip(&format!("cold-skip-same-{scenery}"), scenery);
            let off = glide_off_move(&format!("cold-skip-same-off-{scenery}"), scenery);
            assert_eq!(before_set(&cold), before_set(&off), "scenery {scenery} {cold:?} against {off:?}");
            let drawn = if scenery { vec!["drawing main".to_string(), "drawn main".to_string()] } else { vec!["rendering main".to_string()] };
            assert_eq!(before_set(&cold), drawn, "scenery {scenery} {cold:?}");
            if !scenery {
                assert!(at(&cold, "set ") < at(&cold, "tables main"), "{cold:?}");
            }
            let name = name_of(&fake.showing("main"));
            let tables: Vec<String> = std::fs::read_dir(daemon.store.tables()).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
            assert!(tables.iter().any(|file| file.starts_with(&format!("{name}.")) && file.ends_with(".tables")), "scenery {scenery} {tables:?}");
        }
    }

    #[test]
    fn a_cold_helper_reads_no_glide_file_before_the_set() {
        let (daemon, fake, script, _) = marked_rig("cold-skip-reads");
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let state = daemon.store.load();
        let entry = &state.screens["main"];
        let on_screen = daemon.store.tables_for(entry.output.as_ref().unwrap(), tables_key(&entry.source, &state.options).unwrap());
        std::fs::write(&on_screen, b"SCBT torn").unwrap();
        script.lock().unwrap().slow_hello = Duration::from_secs(60);
        daemon.glider.lock().unwrap().reset_for_test();
        daemon.glider.lock().unwrap().keep_parked();
        daemon.glider.lock().unwrap().forget_wake_for_test();
        *fake.pointer.lock().unwrap() = Some(center(subject()));
        fake.events.lock().unwrap().clear();
        assert!(!daemon.wake_early(), "a wake primed a cold helper");
        assert!(!daemon.launch_at_drag(center(subject())), "a drag start primed a cold helper");
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        let set = at(&events, "set ");
        assert!(!events[..set].iter().any(|event| event.starts_with("read tables") || event.starts_with("prime look")), "{events:?}");
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.starts_with("skipped, helper cold")), "{said:?}");
    }

    #[test]
    fn a_parked_helper_gets_its_tables_before_the_still_and_glides() {
        let (daemon, fake, _, _) = marked_rig("cold-skip-warm");
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        assert!(at(&events, "tables main") < at(&events, "drawing main"), "{events:?}");
        assert!(events.iter().any(|event| event.starts_with("glide ")), "{events:?}");
    }

    #[test]
    fn after_a_skip_the_new_picture_has_its_tables_and_the_next_move_glides_from_them() {
        for scenery in [true, false] {
            let (daemon, fake, script, _) = cold_skip(&format!("cold-skip-next-{scenery}"), scenery);
            let name = name_of(&fake.showing("main"));
            let tables: Vec<String> = std::fs::read_dir(daemon.store.tables()).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
            assert!(tables.iter().any(|file| file.starts_with(&format!("{name}.")) && file.ends_with(".tables")), "scenery {scenery} {tables:?}");
            script.lock().unwrap().slow_hello = Duration::ZERO;
            fake.events.lock().unwrap().clear();
            fake.place(vec![if scenery { Rect { x: 66.0, y: 42.0, w: 28.0, h: 28.0 } } else { Rect { x: 30.0, y: 20.0, w: 50.0, h: 60.0 } }]);
            assert_eq!(outcome(&daemon.tick()), Outcome::Rendered, "scenery {scenery} {:?}", daemon.glider.lock().unwrap().said);
            let events = events(&fake);
            assert!(events.iter().any(|event| event.starts_with("glide ") && event.ends_with(" true true")), "scenery {scenery} {events:?}");
        }
    }

    #[test]
    fn a_slow_helper_never_holds_the_set_back() {
        let off = {
            let (daemon, fake, _, _) = glide_rig("glide-slow-off", false);
            fake.place(vec![subject()]);
            daemon.tick();
            fake.place(vec![moved_subject()]);
            let started = Instant::now();
            assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
            started.elapsed()
        };
        let ((daemon, fake, script, _), waits) = glide_rig_waits("glide-slow-hello", true);
        fake.place(vec![subject()]);
        daemon.tick();
        script.lock().unwrap().slow_hello = Duration::from_secs(60);
        daemon.glider.lock().unwrap().reset_for_test();
        daemon.glider.lock().unwrap().keep_parked();
        fake.events.lock().unwrap().clear();
        waits.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        let started = Instant::now();
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let took = started.elapsed();
        assert!(waits.lock().unwrap().iter().all(|(what, _)| *what == "spawn"), "{:?}", waits.lock().unwrap());
        let events = events(&fake);
        assert!(events.iter().any(|event| event.starts_with("set ")) && !events.iter().any(|event| event.starts_with("glide ") || event.starts_with("open ")), "{events:?}");
        assert!(took < off + Duration::from_secs(5), "the tick took {took:?} against {off:?} with the glide off");
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.contains("skipped, helper cold")) || events.iter().any(|event| event.starts_with("set ")), "{said:?}");
    }

    #[test]
    fn a_parked_helper_that_shows_late_swaps_plainly_at_the_deadline_and_is_kept() {
        let ((daemon, fake, script, spawned), waits) = glide_rig_waits("glide-late", true);
        fake.place(vec![subject()]);
        daemon.tick();
        script.lock().unwrap().shown_after = Duration::from_secs(5);
        let mut rendered = 0;
        for spot in 0..12 {
            waits.lock().unwrap().clear();
            fake.place(vec![Rect { x: 30.0 + (spot * 13 % 70) as f32, y: 20.0 + (spot * 7 % 40) as f32, w: 40.0, h: 40.0 }]);
            if outcome(&daemon.tick()) == Outcome::Rendered {
                rendered += 1;
                assert!(inside_budget(&waits.lock().unwrap()), "{:?}", waits.lock().unwrap());
            }
        }
        assert!(rendered >= 3, "{rendered}");
        assert!(!events(&fake).iter().any(|event| event.starts_with("glide ")), "{:?}", events(&fake));
        assert_eq!(*spawned.lock().unwrap(), 1, "a late helper was replaced");
        assert!(daemon.glider.lock().unwrap().parked());
        script.lock().unwrap().shown_after = Duration::ZERO;
        for spot in 12..24 {
            fake.place(vec![Rect { x: 30.0 + (spot * 13 % 70) as f32, y: 20.0 + (spot * 7 % 40) as f32, w: 40.0, h: 40.0 }]);
            if outcome(&daemon.tick()) == Outcome::Rendered {
                break;
            }
        }
        assert!(events(&fake).iter().any(|event| event.starts_with("glide ")), "{:?}", events(&fake));
    }

    fn cleared_before_set(fake: &Fake) -> bool {
        let events = events(fake);
        let cleared = events.iter().position(|event| event == "unprime main");
        let set = events.iter().position(|event| event.starts_with("set "));
        cleared.zip(set).is_some_and(|(cleared, set)| cleared < set)
    }

    #[test]
    fn a_move_that_does_not_slide_clears_its_screen_before_the_new_picture_is_set() {
        let (daemon, fake, script, _) = glide_rig("glide-clear-budget", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(daemon.launch_at_drag(center(subject())), "the drag start did not prime");
        script.lock().unwrap().shown_fails = true;
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(cleared_before_set(&fake), "{:?}", events(&fake));
        let (mut daemon, fake, _, _) = glide_rig("glide-clear-lag", true);
        daemon.grace = Duration::from_secs(5);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(daemon.launch_at_drag(center(subject())), "the drag start inside the read lag did not prime");
        fake.events.lock().unwrap().clear();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.starts_with("skipped, output not shown")), "{said:?}");
        assert!(cleared_before_set(&fake), "the window primed inside the read lag stayed over the plain swap {:?}", events(&fake));
    }

    #[test]
    fn a_wake_primes_only_over_scoochs_own_picture() {
        let (daemon, fake, _, _) = glide_rig("glide-wake-system", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        *fake.pointer.lock().unwrap() = Some(center(subject()));
        daemon.wake_early();
        let output = fake.showing("main");
        let folder = scratch("glide-wake-system-picked");
        fake.show("main", &picture(&folder, "picked.png"));
        daemon.glider.lock().unwrap().forget_wake_for_test();
        assert!(!daemon.wake_early(), "a wake primed Scooch's old picture over the picture picked in the system's settings");
        assert!(!daemon.launch_at_drag(center(subject())), "a drag start primed Scooch's old picture over the picture picked in the system's settings");
        fake.show("main", &output);
        daemon.glider.lock().unwrap().forget_wake_for_test();
        assert!(daemon.wake_early(), "a wake over Scooch's own picture did not prime");
    }

    #[test]
    fn a_bare_wake_primes_the_parked_helper_once_and_stays_in_bounds() {
        let (daemon, fake, _, spawned) = glide_rig("glide-early-bare", true);
        fake.place(vec![subject()]);
        daemon.tick();
        *fake.pointer.lock().unwrap() = Some(center(subject()));
        fake.events.lock().unwrap().clear();
        let primes = |fake: &Fake| events(fake).iter().filter(|event| event.starts_with("prime ")).count();
        assert!(!daemon.wake_early(), "the wake that echoes Scooch's own set primed");
        assert!(daemon.wake_early());
        assert!(!daemon.wake_early(), "a second prime inside ten seconds");
        daemon.tick();
        daemon.glider.lock().unwrap().forget_wake_for_test();
        assert!(daemon.wake_early());
        daemon.tick();
        daemon.glider.lock().unwrap().forget_wake_for_test();
        assert!(!daemon.wake_early(), "a prime after two bare wakes in a row");
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        daemon.glider.lock().unwrap().forget_wake_for_test();
        assert!(!daemon.wake_early(), "the wake that echoes the move's own set primed");
        assert!(daemon.wake_early(), "a real move lets the next wake prime again");
        assert_eq!(primes(&fake), 3, "{:?}", events(&fake));
        assert_eq!(*spawned.lock().unwrap(), 1, "a wake launched a helper");
    }

    #[test]
    fn a_wake_primes_the_screen_under_the_pointer_and_none_off_every_screen() {
        let (mut daemon, fake, _) = rig("glide-early-window", vec![screen("left", 0.0, 2.0), screen("right", 160.0, 2.0)]);
        *fake.live.lock().unwrap() = true;
        daemon.glider = Arc::new(Mutex::new(Glider::new(Box::new(crate::glide::tests::FakeSpawner::new(fake.events.clone())))));
        fake.place(vec![subject(), Rect { x: subject().x + 160.0, ..subject() }]);
        daemon.tick();
        daemon.wake_early();
        for (pointer, wanted) in [((200.0, 50.0), Some("prime right")), ((500.0, 50.0), None)] {
            daemon.glider.lock().unwrap().forget_wake_for_test();
            fake.events.lock().unwrap().clear();
            *fake.pointer.lock().unwrap() = Some(pointer);
            assert_eq!(daemon.wake_early(), wanted.is_some(), "the pointer at {pointer:?}");
            let primes: Vec<String> = events(&fake).into_iter().filter(|event| event.starts_with("prime")).collect();
            assert!(primes.iter().map(|event| event.rsplit_once(' ').map_or(event.as_str(), |(head, _)| head)).eq(wanted), "the pointer at {pointer:?} {primes:?}");
        }
    }

    fn drag_rig(name: &str) -> (Arc<Daemon>, Arc<Fake>, Arc<Mutex<usize>>) {
        let (daemon, fake, _, spawned) = glide_rig(name, true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.events.lock().unwrap().clear();
        (Arc::new(daemon), fake, spawned)
    }

    fn center(rect: Rect) -> (f32, f32) {
        (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
    }

    fn wait_for(fake: &Fake, start: &str) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if events(fake).iter().any(|event| event.starts_with(start)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn a_drag_start_on_an_icon_primes_the_parked_helper_and_starts_no_tick() {
        let (daemon, fake, spawned) = drag_rig("drag-start");
        let before = (*spawned.lock().unwrap(), fake.snapshots.load(Ordering::SeqCst), fake.sets.load(Ordering::SeqCst));
        *fake.button.lock().unwrap() = true;
        assert!(daemon.drag_starting(center(subject())));
        assert!(wait_for(&fake, "prime"), "{:?}", events(&fake));
        let seen = events(&fake);
        let shown = name_of(&fake.showing("main"));
        assert!(seen.iter().any(|event| event.starts_with("warm 320x200")) && seen.contains(&format!("prime main {shown}")), "{seen:?}");
        assert_eq!((*spawned.lock().unwrap(), fake.snapshots.load(Ordering::SeqCst), fake.sets.load(Ordering::SeqCst)), before, "{seen:?}");
        assert!(daemon.handed.lock().unwrap().is_none());
        *fake.button.lock().unwrap() = false;
    }

    #[test]
    fn a_down_outside_every_icon_rect_starts_no_polling() {
        let (daemon, _fake, _) = drag_rig("drag-down");
        assert_eq!(crate::platform::pressed(Some((140.0, 90.0)), |point| daemon.over_icon(point)), None);
        assert_eq!(crate::platform::pressed(Some(center(subject())), |point| daemon.over_icon(point)), Some(center(subject())));
    }

    #[test]
    fn a_drag_that_begins_off_every_icon_or_under_a_window_primes_nothing() {
        let (daemon, fake, spawned) = drag_rig("drag-off");
        let before = *spawned.lock().unwrap();
        *fake.button.lock().unwrap() = true;
        assert!(!daemon.drag_starting((140.0, 90.0)));
        *fake.covered.lock().unwrap() = true;
        assert!(!daemon.drag_starting(center(subject())));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(*spawned.lock().unwrap(), before);
        assert!(events(&fake).is_empty(), "{:?}", events(&fake));
        *fake.button.lock().unwrap() = false;
    }

    fn drop_after_a_drag_start(name: &str, after: Duration) {
        let (daemon, fake, spawned) = drag_rig(name);
        let before = *spawned.lock().unwrap();
        *fake.button.lock().unwrap() = true;
        assert!(daemon.drag_starting(center(subject())));
        assert!(wait_for(&fake, "prime"), "{:?}", events(&fake));
        std::thread::sleep(after);
        *fake.button.lock().unwrap() = false;
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let seen = events(&fake);
        let said = daemon.glider.lock().unwrap().said.clone();
        assert_eq!(*spawned.lock().unwrap(), before, "{seen:?} {said:?}");
        assert!(seen.iter().any(|event| event.starts_with("glide ")) && !seen.iter().any(|event| event == "quit"), "{seen:?} {said:?}");
        let warms = seen.iter().filter(|event| event.starts_with("warm ")).count();
        assert_eq!(warms, 1, "the picture was sent again after the drag start {seen:?}");
    }

    #[test]
    fn a_drop_soon_after_a_drag_start_glides_with_what_the_drag_start_loaded() {
        drop_after_a_drag_start("drag-drop-soon", Duration::from_millis(300));
    }

    #[test]
    fn a_drop_six_seconds_into_a_drag_still_glides() {
        drop_after_a_drag_start("drag-long", Duration::from_secs(6));
    }

    #[test]
    fn tables_from_another_version_swap_plainly_with_no_layer() {
        let (daemon, fake, _, spawned) = glide_rig("glide-old-tables", true);
        fake.place(vec![subject()]);
        daemon.tick();
        let output = daemon.store.load().screens["main"].output.clone().unwrap();
        let name = output.file_name().unwrap().to_string_lossy().into_owned();
        let file = std::fs::read_dir(daemon.store.tables()).unwrap().flatten().map(|entry| entry.path()).find(|path| path.file_name().is_some_and(|file| file.to_string_lossy().starts_with(&name)) && path.extension().is_some_and(|ext| ext == "tables")).unwrap();
        let mut bytes = std::fs::read(&file).unwrap();
        assert_eq!(bytes[4..8], scooch_core::TABLES_VERSION.to_le_bytes());
        bytes[4..8].copy_from_slice(&(scooch_core::TABLES_VERSION - 1).to_le_bytes());
        let body = bytes.len() - 8;
        let sum = crate::live::canvas::fingerprint(0, 0, &bytes[..body]);
        bytes[body..].copy_from_slice(&sum.to_le_bytes());
        std::fs::write(&file, &bytes).unwrap();
        fake.events.lock().unwrap().clear();
        let before = *spawned.lock().unwrap();
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let seen = events(&fake);
        assert!(!seen.iter().any(|event| event.starts_with("open ") || event.starts_with("glide ")), "{seen:?}");
        assert!(seen.iter().any(|event| event.starts_with("set ")), "{seen:?}");
        assert_eq!(*spawned.lock().unwrap(), before, "{seen:?}");
    }

    #[test]
    fn the_scale_a_glide_ends_at_is_kept_for_the_next_helper() {
        let (daemon, fake, script, _) = glide_rig("glide-scale-kept", true);
        script.lock().unwrap().glided = Some((20, 0));
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        let scales = daemon.store.load().scales;
        assert_eq!(scales.len(), 1, "{scales:?}");
        assert!(scales.keys().all(|key| key.starts_with("Fake GPU|")) && scales.values().all(|scale| *scale == 1.0), "{scales:?}");
        daemon.glider.lock().unwrap().reset_for_test();
        daemon.glider.lock().unwrap().keep_parked();
        fake.events.lock().unwrap().clear();
        fake.place(vec![Rect { x: 40.0, ..moved_subject() }]);
        daemon.tick();
        assert!(events(&fake).iter().any(|event| event.starts_with("warm ") && event.ends_with(" Fake GPU=1")), "{:?}", events(&fake));
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

    fn helper_commands(fake: &Fake) -> Vec<String> {
        events(fake).into_iter().filter(|event| ["spawn", "warm ", "open ", "glide ", "settle ", "close "].iter().any(|command| event.starts_with(command))).collect()
    }

    #[test]
    fn the_layer_opens_after_a_snapshot_that_moved_icons_and_before_the_glide() {
        let (daemon, fake, _, _) = glide_rig("glide-early", true);
        fake.place(vec![subject()]);
        daemon.tick();
        assert_eq!(helper_commands(&fake), vec!["spawn".to_string()], "{:?}", events(&fake));
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        let open = at(&events, "open");
        assert!(events.iter().rposition(|event| event == "snapshot").unwrap() < open && open < at(&events, "glide"), "{events:?}");
        assert!(daemon.glider.lock().unwrap().said.iter().any(|line| line.starts_with("open after snapshot 1 ")), "{:?}", daemon.glider.lock().unwrap().said);
        assert!(daemon.held_bytes() > 0);
    }

    #[test]
    fn thirty_wakes_with_nothing_moved_send_the_helper_nothing() {
        let (daemon, fake, _, spawned) = glide_rig("glide-quiet-wakes", true);
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        fake.events.lock().unwrap().clear();
        let sets = fake.sets.load(Ordering::SeqCst);
        for _ in 0..30 {
            assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        }
        assert!(helper_commands(&fake).is_empty(), "{:?}", events(&fake));
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
        assert_eq!(*spawned.lock().unwrap(), 1);
    }

    #[test]
    fn icons_that_move_without_moving_the_picture_open_and_close_with_nothing_set() {
        let (daemon, fake, _, _) = glide_rig("glide-moved-kept", true);
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        fake.events.lock().unwrap().clear();
        let sets = fake.sets.load(Ordering::SeqCst);
        fake.place(vec![moved_subject(), Rect { x: 2.0, y: 2.0, w: 4.0, h: 4.0 }]);
        let ticked = outcome(&daemon.tick());
        let events = events(&fake);
        assert!(matches!(ticked, Outcome::Unchanged | Outcome::Kept), "{ticked:?}");
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
        assert!(at(&events, "open") < at(&events, "close") && !events.iter().any(|event| event.starts_with("glide ")), "{events:?}");
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

    #[test]
    fn a_drop_the_quick_look_saw_reads_the_desktop_once_before_the_glide() {
        let (daemon, fake, _, _) = glide_rig("one-read", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.place(vec![moved_subject()]);
        *fake.drop_seen.lock().unwrap() = true;
        fake.events.lock().unwrap().clear();
        let signal = Signal::default();
        signal.wake();
        daemon.gather(&signal);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        let glide = at(&events, "glide");
        assert_eq!(events[..glide].iter().filter(|event| *event == "snapshot").count(), 1, "{events:?}");
    }

    #[test]
    fn a_quick_look_that_saw_nothing_hands_nothing_to_the_tick() {
        let (daemon, fake, _) = rig("quick-nothing", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        *fake.drop_seen.lock().unwrap() = true;
        assert!(!daemon.quick_look());
        fake.place(vec![corner()]);
        let before = fake.snapshots.load(Ordering::SeqCst);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1);
    }

    #[test]
    fn a_drop_seen_on_the_first_read_is_read_once_and_never_waits() {
        let (daemon, fake, _, _) = glide_rig("quick-first", true);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.place(vec![moved_subject()]);
        *fake.drop_seen.lock().unwrap() = true;
        fake.events.lock().unwrap().clear();
        let before = fake.snapshots.load(Ordering::SeqCst);
        let started = Instant::now();
        assert!(daemon.quick_look());
        assert!(started.elapsed() < QUICK, "the quick look waited {:?}", started.elapsed());
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let events = events(&fake);
        assert_eq!(events[..at(&events, "glide")].iter().filter(|event| *event == "snapshot").count(), 1, "{events:?}");
    }

    #[test]
    fn a_move_seen_only_on_the_second_read_is_still_taken() {
        let (daemon, fake, _) = rig("quick-second", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        *fake.drop_seen.lock().unwrap() = true;
        *fake.after_read.lock().unwrap() = Some(vec![moved_subject()]);
        let before = fake.snapshots.load(Ordering::SeqCst);
        let started = Instant::now();
        assert!(daemon.quick_look());
        assert!(started.elapsed() >= QUICK);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 2);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 2);
    }

    #[test]
    fn a_drop_with_nothing_moved_reads_twice_and_then_settles() {
        let (daemon, fake, _) = rig("quick-none", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        *fake.drop_seen.lock().unwrap() = true;
        let before = fake.snapshots.load(Ordering::SeqCst);
        let signal = Signal::default();
        signal.wake();
        let started = Instant::now();
        daemon.gather(&signal);
        assert!(started.elapsed() >= QUICK, "the gather took {:?}", started.elapsed());
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 2);
        assert!(daemon.handed.lock().unwrap().is_none());
        assert!(signal.pending.lock().unwrap().last.is_none());
    }

    fn on_subject() -> Rect {
        Rect { x: 70.0, y: 40.0, w: 20.0, h: 20.0 }
    }

    fn at_once_rig(name: &str) -> (Daemon, Arc<Fake>) {
        let (daemon, fake, _) = rig(name, vec![screen("main", 0.0, 1.0)]);
        *fake.at_once.lock().unwrap() = true;
        fake.place(vec![corner()]);
        assert!(daemon.tick().problem.is_none());
        (daemon, fake)
    }

    #[test]
    fn a_pure_move_is_taken_at_once_with_one_read() {
        let (daemon, fake) = at_once_rig("pure-once");
        fake.place(vec![on_subject()]);
        let before = fake.snapshots.load(Ordering::SeqCst);
        let started = Instant::now();
        assert!(daemon.quick_look());
        assert!(started.elapsed() < QUICK, "the quick look took {:?}", started.elapsed());
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1);
    }

    #[test]
    fn a_desktop_that_does_not_read_at_once_never_takes_the_quick_way() {
        let (daemon, fake) = at_once_rig("pure-off");
        *fake.at_once.lock().unwrap() = false;
        fake.place(vec![on_subject()]);
        let before = fake.snapshots.load(Ordering::SeqCst);
        assert!(!daemon.quick_look());
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before);
        let signal = Signal::default();
        signal.wake();
        let started = Instant::now();
        daemon.gather(&signal);
        assert!(started.elapsed() >= Duration::from_millis(200), "the move did not settle, {:?}", started.elapsed());
        assert!(daemon.handed.lock().unwrap().is_none());
    }

    #[test]
    fn a_new_icon_settles() {
        let (daemon, fake) = at_once_rig("pure-new");
        fake.place(vec![corner(), on_subject()]);
        let signal = Signal::default();
        signal.wake();
        let started = Instant::now();
        daemon.gather(&signal);
        assert!(started.elapsed() >= Duration::from_millis(200), "a new icon did not settle, {:?}", started.elapsed());
        assert!(daemon.handed.lock().unwrap().is_none());
    }

    #[test]
    fn a_burst_of_ten_wakes_with_one_pure_move_makes_one_render() {
        let (daemon, fake) = at_once_rig("pure-burst");
        let sets = fake.sets.load(Ordering::SeqCst);
        fake.place(vec![on_subject()]);
        let signal = Signal::default();
        for _ in 0..5 {
            signal.wake();
        }
        daemon.gather(&signal);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        for _ in 0..5 {
            signal.wake();
        }
        daemon.gather(&signal);
        assert!(daemon.tick().problem.is_none());
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets + 1);
    }

    #[test]
    fn a_pure_move_while_wakes_keep_coming_renders_once_then_follows_after_the_settle() {
        let (daemon, fake) = at_once_rig("pure-then-more");
        let sets = fake.sets.load(Ordering::SeqCst);
        fake.place(vec![on_subject()]);
        let signal = Signal::default();
        signal.wake();
        daemon.gather(&signal);
        assert!(daemon.handed.lock().unwrap().is_some(), "the pure move was not taken at once");
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets + 1);
        fake.place(vec![corner()]);
        signal.wake();
        let started = Instant::now();
        daemon.gather(&signal);
        assert!(started.elapsed() >= Duration::from_millis(200), "the later change did not settle, {:?}", started.elapsed());
        assert!(daemon.handed.lock().unwrap().is_none());
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
    }

    #[test]
    fn a_wake_while_the_tick_of_a_quick_take_still_runs_settles() {
        let (daemon, fake) = at_once_rig("pure-during");
        fake.place(vec![on_subject()]);
        let signal = Arc::new(Signal::default());
        signal.wake();
        daemon.gather(&signal);
        assert!(daemon.handed.lock().unwrap().is_some(), "the pure move was not taken at once");
        fake.place(vec![corner()]);
        let woke: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let (waker, noted) = (signal.clone(), woke.clone());
        *fake.on_set.lock().unwrap() = Some(Box::new(move || {
            std::thread::sleep(QUIET + Duration::from_millis(100));
            *noted.lock().unwrap() = Some(Instant::now());
            waker.wake();
        }));
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let woke = woke.lock().unwrap().expect("the tick set no wallpaper");
        let before = fake.snapshots.load(Ordering::SeqCst);
        daemon.gather(&signal);
        assert!(Instant::now() >= woke + QUIET, "the wake during the tick did not settle, gather ended {:?} after it", woke.elapsed());
        assert!(daemon.handed.lock().unwrap().is_none(), "the wake during the tick was taken at once");
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1);
    }

    #[test]
    fn a_handed_snapshot_older_than_the_bound_is_not_used() {
        let (daemon, fake, _) = rig("quick-old", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        *fake.drop_seen.lock().unwrap() = true;
        fake.place(vec![moved_subject()]);
        assert!(daemon.quick_look());
        std::thread::sleep(QUIET + Duration::from_millis(50));
        fake.place(vec![corner()]);
        let before = fake.snapshots.load(Ordering::SeqCst);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1);
    }

    fn saved_rig(name: &str) -> (Daemon, Arc<Fake>) {
        let (daemon, fake, _) = rig(name, vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.place(vec![moved_subject()]);
        (daemon, fake)
    }

    #[test]
    fn a_burst_of_three_saves_ticks_once_after_the_quiet_from_the_last_and_reads_once() {
        let (daemon, fake) = saved_rig("saves-burst");
        let first = Instant::now();
        let last = first + Duration::from_millis(20);
        *fake.saves.lock().unwrap() = vec![first, first + Duration::from_millis(10), last];
        let signal = Signal::default();
        signal.wake();
        let before = fake.snapshots.load(Ordering::SeqCst);
        daemon.gather(&signal);
        let ended = Instant::now();
        assert!(ended >= last + SAVE_QUIET, "the tick began {:?} after the last save", ended.saturating_duration_since(last));
        assert_eq!(*daemon.saves.lock().unwrap(), Some(last), "the saves were not taken as saves");
        assert!(daemon.handed.lock().unwrap().is_some(), "the saves took the settle path");
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1);
        assert!(signal.pending.lock().unwrap().last.is_none());
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1, "the tick read the desktop again");
    }

    #[test]
    fn a_save_skips_the_settle_and_any_other_wake_still_settles() {
        let (daemon, fake) = saved_rig("saves-settle");
        let saved = Instant::now();
        *fake.saves.lock().unwrap() = vec![saved];
        let signal = Signal::default();
        signal.wake();
        let before = fake.snapshots.load(Ordering::SeqCst);
        daemon.gather(&signal);
        assert!(Instant::now() >= saved + SAVE_QUIET, "the save was read before its quiet");
        assert_eq!(*daemon.saves.lock().unwrap(), Some(saved), "the save was not taken as a save");
        assert!(daemon.handed.lock().unwrap().is_some(), "the save took the settle path");
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before + 1);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.place(vec![corner()]);
        let woke = Instant::now();
        signal.wake();
        daemon.gather(&signal);
        assert!(Instant::now() >= woke + QUIET, "a wake with no new save did not settle, gather ended {:?} after it", woke.elapsed());
        assert!(daemon.handed.lock().unwrap().is_none(), "a wake with no new save was taken at once");
        assert_eq!(*daemon.saves.lock().unwrap(), Some(saved));
    }

    #[test]
    fn the_late_notice_after_a_save_tick_renders_nothing() {
        let (daemon, fake) = saved_rig("saves-late");
        *fake.saves.lock().unwrap() = vec![Instant::now()];
        let signal = Signal::default();
        signal.wake();
        daemon.gather(&signal);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let sets = fake.sets.load(Ordering::SeqCst);
        signal.wake();
        daemon.gather(&signal);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!(fake.sets.load(Ordering::SeqCst), sets);
    }

    #[test]
    fn a_desktop_that_hears_no_saves_stays_on_the_settle() {
        let (daemon, fake) = saved_rig("saves-none");
        assert_eq!(fake.place_saved(), None);
        let signal = Signal::default();
        signal.wake();
        let started = Instant::now();
        daemon.gather(&signal);
        assert!(started.elapsed() >= Duration::from_millis(200), "no save heard and yet no settle, {:?}", started.elapsed());
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
    }

    fn predicted_rig(name: &str) -> (Daemon, Arc<Fake>, Signal, Tracker, Guess, PathBuf) {
        let (daemon, fake, _) = rig(name, vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let first = fake.showing("main");
        let mut tracker = Tracker::new();
        let press = Press { at: (80.0, 43.0), button: 1, control: false, shift: false, desktop: true, icon: Some(subject()) };
        tracker.pressed(press);
        assert_eq!(tracker.released((81.0, 44.0), true, |_| None), Guess::Nothing);
        tracker.pressed(press);
        let guess = tracker.released((91.0, 61.0), true, |_| Some(moved_subject()));
        assert_eq!(guess, Guess::Move { from: subject(), to: moved_subject(), shown: true });
        *fake.predicted.lock().unwrap() = Some(Snapshot { screens: fake.screens.lock().unwrap().clone(), icons: vec![moved_subject()] });
        let signal = Signal::default();
        signal.wake();
        let started = Instant::now();
        daemon.gather(&signal);
        assert!(started.elapsed() < Duration::from_millis(200), "the prediction waited for the settle, {:?}", started.elapsed());
        let before = fake.snapshots.load(Ordering::SeqCst);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(fake.snapshots.load(Ordering::SeqCst), before, "the tick read the desktop instead of taking the prediction");
        assert_ne!(fake.showing("main"), first);
        (daemon, fake, signal, tracker, guess, first)
    }

    #[test]
    fn a_file_that_disagrees_with_the_prediction_ticks_again_from_the_file_and_turns_prediction_off() {
        let (daemon, fake, signal, mut tracker, guess, _) = predicted_rig("predict-miss");
        let guessed = fake.showing("main");
        fake.place(vec![corner()]);
        assert_eq!(tracker.verify(&guess, &[subject()], &[corner()]), Some(Verdict::Missed));
        assert!(tracker.off);
        signal.wake();
        daemon.gather(&signal);
        assert_ne!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_ne!(fake.showing("main"), guessed, "the second tick left the guessed picture up");
        tracker.pressed(Press { at: (10.0, 7.0), button: 1, control: false, shift: false, desktop: true, icon: Some(corner()) });
        assert!(matches!(tracker.released((80.0, 60.0), true, |_| Some(subject())), Guess::Move { shown: false, .. }), "a prediction was shown after a miss");
    }

    #[test]
    fn no_file_by_the_nets_time_ticks_from_the_file_as_it_is() {
        let (daemon, fake, signal, mut tracker, guess, _) = predicted_rig("predict-none");
        assert_eq!(tracker.verify(&guess, &[subject()], &[subject()]), Some(Verdict::Missed));
        assert_eq!(daemon.icons.lock().unwrap().clone(), Some(vec![moved_subject()]));
        signal.wake();
        daemon.gather(&signal);
        assert!(matches!(outcome(&daemon.tick()), Outcome::Rendered | Outcome::Kept));
        assert_eq!(daemon.icons.lock().unwrap().clone(), Some(vec![subject()]), "the tick did not take the places in the file");
        assert!(fake.showing("main").is_file());
        signal.wake();
        daemon.gather(&signal);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
    }

    #[test]
    fn a_prediction_waits_for_the_file_when_predict_is_off() {
        let (daemon, fake, _) = rig("predict-off", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let mut state = daemon.store.load();
        state.predict = false;
        daemon.store.save(&state).unwrap();
        *fake.predicted.lock().unwrap() = Some(Snapshot { screens: fake.screens.lock().unwrap().clone(), icons: vec![moved_subject()] });
        *fake.offered.lock().unwrap() = Some(Hold::new(&[subject()], vec![moved_subject()], Instant::now() + Duration::from_secs(5)));
        let signal = Signal::default();
        signal.wake();
        let started = Instant::now();
        daemon.gather(&signal);
        assert!(started.elapsed() >= Duration::from_millis(200), "the prediction was taken with predict off, {:?}", started.elapsed());
        assert!(daemon.handed.lock().unwrap().is_none());
        assert!(fake.hold.lock().unwrap().is_none(), "a guess the person switched off still held the reads");
    }

    #[test]
    fn a_held_guess_outlasts_a_read_from_before_the_save_and_a_save_that_disagrees_is_drawn_at_once() {
        let (daemon, fake, _) = rig("predict-hold", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let first = fake.showing("main");
        *fake.predicted.lock().unwrap() = Some(Snapshot { screens: fake.screens.lock().unwrap().clone(), icons: vec![moved_subject()] });
        *fake.offered.lock().unwrap() = Some(Hold::new(&[subject()], vec![moved_subject()], Instant::now() + Duration::from_secs(5)));
        let signal = Signal::default();
        signal.wake();
        daemon.gather(&signal);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let guessed = fake.showing("main");
        assert_ne!(guessed, first);
        let sets = fake.sets.load(Ordering::SeqCst);
        signal.wake();
        daemon.gather(&signal);
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged, "a read from before the save drew the old places back");
        assert_eq!((fake.sets.load(Ordering::SeqCst), fake.showing("main")), (sets, guessed.clone()));
        fake.place(vec![corner()]);
        signal.wake();
        daemon.gather(&signal);
        assert_ne!(outcome(&daemon.tick()), Outcome::Unchanged, "the save that disagrees with the guess was not drawn");
        assert_ne!(fake.showing("main"), guessed);
        assert!(fake.hold.lock().unwrap().is_none());
        fake.place(vec![subject()]);
        signal.wake();
        daemon.gather(&signal);
        assert_ne!(outcome(&daemon.tick()), Outcome::Unchanged, "the hold outlived the save");
    }

    fn map_rig(name: &str) -> (Daemon, Arc<Fake>, PathBuf) {
        let (mut daemon, fake, original) = rig(name, vec![screen("main", 0.0, 2.0)]);
        *fake.live.lock().unwrap() = true;
        daemon.glider = Arc::new(Mutex::new(Glider::new(Box::new(crate::glide::tests::FakeSpawner::new(fake.events.clone())))));
        set_scenery(&daemon, true);
        (daemon, fake, original)
    }

    fn map_of(daemon: &Daemon, screen: &str) -> PathBuf {
        cache::map_file(&daemon.store, screen, cache::key(&daemon.store.load().screens[screen].source).unwrap())
    }

    fn cold_drag_out(daemon: &Daemon, fake: &Fake, kept: impl FnOnce(&Path)) -> (Vec<u8>, Vec<u8>) {
        fake.place(vec![subject(), calm()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        daemon.sessions.lock().unwrap().clear();
        kept(map_of(daemon, "main").as_path());
        fake.place(vec![subject(), calm_moved()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let state = daemon.store.load();
        let output = state.screens["main"].output.clone().unwrap();
        let tables = daemon.store.tables_for(&output, tables_key(&state.screens["main"].source, &state.options).unwrap());
        (std::fs::read(&output).unwrap(), std::fs::read(tables).unwrap())
    }

    #[test]
    fn a_cold_drag_with_a_kept_map_draws_the_same_picture_and_tables() {
        let (daemon, fake, _) = map_rig("map-kept");
        let kept = cold_drag_out(&daemon, &fake, |file| assert!(file.is_file(), "no map was kept after the first tick"));
        let (cold, cold_fake, _) = map_rig("map-none");
        let none = cold_drag_out(&cold, &cold_fake, |file| std::fs::remove_file(file).unwrap());
        assert!(kept == none, "a kept map drew another picture or other tables");
    }

    #[test]
    fn a_cold_drag_with_a_kept_map_does_not_work_the_map_out_again() {
        let (daemon, fake, _) = map_rig("map-reuse");
        cold_drag_out(&daemon, &fake, |_| {});
        let (adopted, drawn) = daemon.sessions.lock().unwrap()["main"].1.map_ids();
        assert!(adopted.is_some() && adopted == drawn, "the cold drag worked the map out again, adopted {adopted:?} drawn {drawn:?}");
    }

    #[test]
    fn a_kept_map_it_cannot_use_is_drawn_over_and_replaced() {
        let (good, good_fake, _) = map_rig("map-trusted");
        let expected = cold_drag_out(&good, &good_fake, |_| {});
        let file = std::fs::read(map_of(&good, "main")).unwrap();
        let mut version = file.clone();
        version[12..16].copy_from_slice(&(scooch_core::BELONGING_VERSION + 1).to_le_bytes());
        let sum = version[..48].iter().chain(&version[56..]).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
        version[48..56].copy_from_slice(&sum.to_le_bytes());
        let other = {
            let image = sky_image(480, 100);
            let mut session = scooch_core::Session::new();
            session.belongs(&image, &scooch_core::analyze(&image), &scene::scene(&screen("main", 0.0, 2.0), &[subject(), calm()]), &with_scenery(Options::default(), true));
            session.map().unwrap().to_bytes()
        };
        for (why, bad) in [("cut-short", file[..file.len() / 2].to_vec()), ("another-picture", other), ("another-version", version)] {
            let (daemon, fake, _) = map_rig(&format!("map-{why}"));
            let got = cold_drag_out(&daemon, &fake, |path| std::fs::write(path, &bad).unwrap());
            assert!(got == expected, "{why}: the cold drag drew another picture or other tables");
            assert!(std::fs::read(map_of(&daemon, "main")).unwrap() == file, "{why}: the bad file was not replaced by a good one");
        }
    }

    #[test]
    fn a_second_screen_never_reads_the_first_screens_map() {
        let (daemon, fake, _) = two_screen_rig("map-two");
        set_scenery(&daemon, true);
        fake.place(vec![subject(), calm(), right(subject()), right(calm())]);
        assert!(daemon.tick().problem.is_none());
        let (left, other) = (map_of(&daemon, "left"), map_of(&daemon, "right"));
        assert!(left != other && left.is_file() && other.is_file(), "{left:?} {other:?}");
        daemon.sessions.lock().unwrap().clear();
        std::fs::remove_file(&other).unwrap();
        fake.place(vec![subject(), calm_moved(), right(subject()), right(calm_moved())]);
        assert!(daemon.tick().problem.is_none());
        let sessions = daemon.sessions.lock().unwrap();
        assert!(sessions["left"].1.map_ids().0.is_some(), "the first screen did not read its own map");
        assert!(sessions["right"].1.map_ids().0.is_none(), "the second screen read a map that was not its own");
    }

    #[test]
    fn a_picture_change_removes_the_old_map_of_that_screen() {
        let (daemon, fake, original) = map_rig("map-change");
        fake.place(vec![subject(), calm()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let old = map_of(&daemon, "main");
        assert!(old.is_file());
        let mut image = scene::sample(480, 100);
        image.put_pixel(0, 0, [1, 2, 3].into());
        let chosen = original.parent().unwrap().join("chosen.png");
        std::fs::write(&chosen, scooch_core::encode_png(&image).unwrap()).unwrap();
        fake.show("main", &chosen);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let new = map_of(&daemon, "main");
        assert!(new != old && new.is_file() && !old.exists(), "{old:?} {new:?}");
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
        let quit = names.iter().position(|event| *event == "quit").unwrap_or_else(|| panic!("the failed helper was kept {names:?}"));
        assert!(names.iter().skip(quit).any(|event| *event == "spawn") && daemon.glider.lock().unwrap().running(), "the failed helper was not replaced {names:?}");
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
            assert_eq!(daemon.glider.lock().unwrap().running(), matches!(name, "shown" | "dies" | "settle"), "{name}");
            if matches!(name, "dies" | "settle") {
                let said = daemon.glider.lock().unwrap().said.clone();
                let lines = said.iter().filter(|line| (line.starts_with("slid, ") || line.starts_with("skipped, ")) && line.contains(" times from the ")).count();
                assert_eq!(lines, 2, "{name}: each move writes one line {said:?}");
            }
            if matches!(name, "dies" | "settle") {
                assert_eq!(events.iter().filter(|event| *event == "spawn").count(), 2, "{name}: a fresh helper parks after one dies or stops answering {events:?}");
            }
        }
    }

    fn landing_words(daemon: &Daemon) -> (bool, bool) {
        let said = daemon.glider.lock().unwrap().said.clone();
        (said.iter().any(|line| line.starts_with("settled after ")), said.iter().any(|line| line.starts_with("gave up the settle after ")))
    }

    #[test]
    fn a_settle_the_helper_reports_writes_the_settled_words() {
        let (daemon, fake, _, _) = glide_rig("glide-settle-words", true);
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        assert!(events(&fake).iter().any(|event| event == "settled 1"), "{:?}", events(&fake));
        assert_eq!(landing_words(&daemon), (true, false), "{:?}", daemon.glider.lock().unwrap().said);
    }

    #[test]
    fn a_slow_glide_that_reports_its_end_gets_time_to_settle_and_keeps_its_helper() {
        let (daemon, fake, script, _) = glide_rig("glide-slow-settle", true);
        {
            let mut script = script.lock().unwrap();
            script.glided = Some((27, 0));
            script.glided_after = Duration::from_millis(1800);
            script.settle_after = Duration::from_millis(2600);
        }
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        assert_eq!(landing_words(&daemon), (true, false), "{:?}", daemon.glider.lock().unwrap().said);
        assert_eq!(events(&fake).iter().filter(|event| *event == "spawn").count(), 1, "{:?}", events(&fake));
    }

    #[test]
    fn a_glide_the_helper_could_not_start_is_a_skip_and_not_a_slide() {
        let (daemon, fake, script, _) = glide_rig("glide-could-not-start", true);
        script.lock().unwrap().glide_fails = true;
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.starts_with("skipped, helper failed, the helper failed before the first moving frame")), "{said:?}");
        assert!(!said.iter().any(|line| line.starts_with("slid")), "{said:?}");
    }

    #[test]
    fn a_settle_given_up_writes_its_own_words_and_not_the_settled_words() {
        let (daemon, fake, script, _) = glide_rig("glide-settle-given-up", true);
        script.lock().unwrap().settle_never = true;
        assert_eq!(first_and_moved(&daemon, &fake), (Outcome::Rendered, Outcome::Rendered));
        let said = daemon.glider.lock().unwrap().said.clone();
        assert!(said.iter().any(|line| line.starts_with("skipped, settle budget")), "{said:?}");
        assert!(events(&fake).iter().any(|event| event.starts_with("close")), "{:?}", events(&fake));
        assert_eq!(landing_words(&daemon), (false, true), "{said:?}");
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
        let layered = |fake: &Fake| events(fake).iter().any(|event| event.starts_with("open") || event.starts_with("glide "));
        let (daemon, fake, _, _) = glide_rig("glide-no-tables", true);
        fake.place(vec![subject()]);
        daemon.tick();
        let started = Instant::now();
        while std::fs::remove_dir_all(daemon.store.tables()).is_err() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_dir_all(daemon.store.tables());
        fake.place(vec![moved_subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(!layered(&fake), "{:?}", events(&fake));
        let (daemon, fake, _, _) = glide_rig("glide-not-shown", true);
        fake.place(vec![subject()]);
        daemon.tick();
        let other = daemon.store.out().join("main-90.jpg");
        std::fs::copy(fake.showing("main"), &other).unwrap();
        fake.show("main", &other);
        fake.place(vec![moved_subject()]);
        daemon.tick();
        assert!(!layered(&fake), "{:?}", events(&fake));
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
        assert!(!daemon.glider.lock().unwrap().running(), "a helper that can no longer glide stayed parked");
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

    fn gnome_rig(name: &str) -> (Daemon, Arc<Fake>, PathBuf) {
        let (daemon, fake, original) = rig(name, vec![screen("main", 0.0, 1.0)]);
        *fake.reloads.lock().unwrap() = true;
        *fake.unknowable.lock().unwrap() = true;
        (daemon, fake, original)
    }

    fn fixed_of(daemon: &Daemon) -> PathBuf {
        daemon.store.fixed("main", "jpg")
    }

    fn writes(fake: &Fake) -> usize {
        fake.writes.load(Ordering::SeqCst)
    }

    fn files_in(folder: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(folder).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
        files.sort();
        files
    }

    fn pass() -> Turn {
        Box::new(|| Ok(()))
    }

    fn jpeg(path: &Path) -> bool {
        std::fs::read(path).is_ok_and(|bytes| bytes.starts_with(&[0xff, 0xd8])) && scooch_core::open(path).is_ok()
    }

    #[cfg(unix)]
    fn inode(path: &Path) -> u64 {
        std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(path).unwrap())
    }

    #[cfg(not(unix))]
    fn inode(_path: &Path) -> u64 {
        0
    }

    fn plain_then(daemon: &Daemon, fake: &Fake, source: &[u8]) {
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.place(Vec::new());
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert_eq!(std::fs::read(fixed_of(daemon)).unwrap(), source);
    }

    #[test]
    fn a_gnome_set_writes_no_setting_while_the_picture_stays_even_between_plain_and_painted() {
        let (daemon, fake, original) = gnome_rig("fixed-quiet");
        let fixed = fixed_of(&daemon);
        let source = std::fs::read(&original).unwrap();
        assert!(source.starts_with(&[0x89, b'P', b'N', b'G']));
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!((fake.showing("main"), writes(&fake)), (fixed.clone(), 1));
        for round in 0..3 {
            fake.place(Vec::new());
            assert_eq!(outcome(&daemon.tick()), Outcome::Plain, "{round}");
            assert_eq!(std::fs::read(&fixed).unwrap(), source, "{round}");
            assert!(daemon.store.load().screens["main"].placement.as_ref().is_some_and(scene::is_identity), "{round}");
            fake.place(vec![subject()]);
            assert_eq!(outcome(&daemon.tick()), Outcome::Rendered, "{round}");
            assert!(jpeg(&fixed), "{round}");
        }
        assert!(named_for("main", &fixed) && named_for("main", &daemon.store.staged("main")));
        assert_eq!(daemon.store.outputs_of("main"), vec![fixed.clone()]);
        assert_eq!((fake.showing("main"), writes(&fake), fake.sets.load(Ordering::SeqCst)), (fixed.clone(), 1, 7));
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.output, entry.counter), (Some(fixed.clone()), 0));
        assert_eq!(files_in(&daemon.store.out()), vec![fixed]);
    }

    #[test]
    fn a_gnome_set_writes_the_whole_picture_beside_the_shown_file_and_renames_it_into_place() {
        let (daemon, fake, original) = gnome_rig("fixed-rename");
        let (fixed, staged, source) = (fixed_of(&daemon), daemon.store.staged("main"), std::fs::read(&original).unwrap());
        plain_then(&daemon, &fake, &source);
        let seen = Arc::new(Mutex::new(None));
        let (look, at, shown) = (seen.clone(), staged.clone(), fixed.clone());
        *fake.on_read.lock().unwrap() = vec![
            pass(),
            Box::new(move || {
                *look.lock().unwrap() = Some((std::fs::read(&at)?, std::fs::read(&shown)?, inode(&at)));
                Ok(())
            }) as Turn,
        ]
        .into();
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let (beside, then, moved) = seen.lock().unwrap().take().expect("the check did not run");
        assert_eq!(then, source);
        assert!(beside.starts_with(&[0xff, 0xd8]));
        assert_eq!(std::fs::read(&fixed).unwrap(), beside);
        assert_eq!(inode(&fixed), moved);
        assert!(!staged.exists());
        assert_eq!(writes(&fake), 1);
    }

    fn fail_at(daemon: &Daemon, fake: &Fake, step: &str) {
        let staged = daemon.store.staged("main");
        let second = |turn: Turn| *fake.on_read.lock().unwrap() = vec![pass(), turn].into();
        match step {
            "write" => std::fs::create_dir_all(staged.join("held")).unwrap(),
            "check" => second(Box::new(|| anyhow::bail!("The desktop did not say which picture it shows."))),
            "rename" => second(Box::new(move || Ok(std::fs::remove_file(&staged)?))),
            "note" => {
                let store = daemon.store.clone();
                second(Box::new(move || {
                    let mut state = store.load();
                    state.on = false;
                    store.save(&state)
                }));
            }
            _ => fake.refuse.lock().unwrap().push("main".into()),
        }
    }

    fn heal(daemon: &Daemon, fake: &Fake) {
        let _ = std::fs::remove_dir_all(daemon.store.staged("main"));
        fake.on_read.lock().unwrap().clear();
        fake.refuse.lock().unwrap().clear();
        let mut state = daemon.store.load();
        state.on = true;
        daemon.store.save(&state).unwrap();
    }

    fn render_then(daemon: &Daemon, fake: &Fake) {
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert!(jpeg(&fixed_of(daemon)));
    }

    fn icons_of(painted: bool) -> Vec<Rect> {
        if painted {
            vec![subject()]
        } else {
            Vec::new()
        }
    }

    fn holds(fixed: &Path, painted: bool, source: &[u8]) -> bool {
        if painted {
            jpeg(fixed)
        } else {
            std::fs::read(fixed).is_ok_and(|bytes| bytes == source)
        }
    }

    #[test]
    fn a_gnome_set_that_fails_at_any_step_leaves_the_shown_file_whole_and_the_state_claims_nothing_the_file_lacks() {
        for (step, wanted, then) in ["write", "check", "rename", "note", "set"].into_iter().flat_map(|step| [(step, true, false), (step, false, true), (step, false, false)]) {
            let case = format!("{step} toward {} then {}", if wanted { "painted" } else { "plain" }, if then { "painted" } else { "plain" });
            let (daemon, fake, original) = gnome_rig(&format!("fixed-fail-{step}-{wanted}-{then}"));
            let (fixed, source) = (fixed_of(&daemon), std::fs::read(&original).unwrap());
            if wanted {
                plain_then(&daemon, &fake, &source);
            } else {
                render_then(&daemon, &fake);
            }
            let before = (std::fs::read(&fixed).unwrap(), inode(&fixed));
            fake.place(icons_of(wanted));
            fail_at(&daemon, &fake, step);
            let failed = daemon.tick();
            assert!(failed.problem.is_some() || failed.outcomes[0].1 == Outcome::Interrupted, "{case} {:?}", failed.outcomes);
            assert_eq!(fake.showing("main"), fixed, "{case}");
            assert!(scooch_core::open(&fixed).is_ok(), "{case}");
            let entry = daemon.store.load().screens["main"].clone();
            assert_eq!((entry.fingerprint, entry.placement, entry.output), (None, None, Some(fixed.clone())), "{case}");
            if step == "set" {
                assert!(holds(&fixed, wanted, &source), "{case}");
            } else {
                assert_eq!((std::fs::read(&fixed).unwrap(), inode(&fixed)), before, "{case}");
            }
            heal(&daemon, &fake);
            fake.place(icons_of(then));
            daemon.tick();
            assert!(holds(&fixed, then, &source), "{case} kept a picture the icons no longer ask for");
            assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged, "{case}");
            fake.place(icons_of(!then));
            daemon.tick();
            assert!(holds(&fixed, !then, &source), "{case}");
            assert_eq!((writes(&fake), files_in(&daemon.store.out())), (1, vec![fixed.clone()]), "{case}");
        }
    }

    #[test]
    fn a_gnome_daemon_killed_at_any_step_of_a_set_comes_back_to_the_picture_the_icons_ask_for() {
        for (step, wanted) in ["check", "set", "after"].into_iter().flat_map(|step| [(step, true), (step, false)]) {
            let case = format!("{step} toward {}", if wanted { "painted" } else { "plain" });
            let (daemon, fake, original) = gnome_rig(&format!("fixed-kill-{step}-{wanted}"));
            let (fixed, source) = (fixed_of(&daemon), std::fs::read(&original).unwrap());
            if wanted {
                plain_then(&daemon, &fake, &source);
            } else {
                render_then(&daemon, &fake);
            }
            fake.place(icons_of(wanted));
            match step {
                "check" => *fake.on_read.lock().unwrap() = vec![pass(), Box::new(|| -> Result<()> { panic!("the process was killed during the check") }) as Turn].into(),
                "set" => *fake.on_set.lock().unwrap() = Some(Box::new(|| panic!("the process was killed at the settings call"))),
                _ => *fake.crash_after_set.lock().unwrap() = true,
            }
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| daemon.tick())).is_err(), "{case}");
            assert!(scooch_core::open(&fixed).is_ok(), "{case}");
            let mut again = Daemon::new(fake.clone(), daemon.store.clone());
            again.grace = Duration::ZERO;
            fake.place(icons_of(!wanted));
            again.tick();
            assert!(holds(&fixed, !wanted, &source), "{case} kept a picture the icons no longer ask for");
            assert_eq!(outcome(&again.tick()), Outcome::Unchanged, "{case}");
            assert_eq!(writes(&fake), 1, "{case}");
        }
    }

    #[test]
    fn a_picture_picked_during_a_gnome_set_stays_and_the_shown_file_keeps_its_picture() {
        let (daemon, fake, original) = gnome_rig("fixed-picked");
        let (fixed, source) = (fixed_of(&daemon), std::fs::read(&original).unwrap());
        plain_then(&daemon, &fake, &source);
        let picked = picture(original.parent().unwrap(), "picked.png");
        let (person, chosen) = (fake.clone(), picked.clone());
        *fake.on_read.lock().unwrap() = vec![
            pass(),
            Box::new(move || {
                person.show("main", &chosen);
                Ok(())
            }) as Turn,
        ]
        .into();
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Interrupted);
        assert_eq!((fake.showing("main"), std::fs::read(&fixed).unwrap()), (picked, source));
        assert_eq!(files_in(&daemon.store.out()), vec![fixed]);
    }

    #[test]
    fn nothing_removes_the_fixed_file_while_the_desktop_shows_it() {
        let (daemon, fake, _) = gnome_rig("fixed-kept");
        let fixed = fixed_of(&daemon);
        fake.place(vec![subject()]);
        daemon.tick();
        let mut state = daemon.store.load();
        sweep(fake.as_ref(), &daemon.store, &state, "main", &[]);
        assert_eq!(prune(&mut state, fake.as_ref(), &daemon.store, None, &[], unix_now() + 2 * ABSENT_FOR), 0);
        discard(fake.as_ref(), &daemon.store, std::slice::from_ref(&fixed), &[]);
        forget(fake.as_ref(), &daemon.store, &mut State { screens: Default::default(), ..state });
        assert!(jpeg(&fixed));
        *fake.on_read.lock().unwrap() = vec![Box::new(|| anyhow::bail!("gsettings did not finish within 10 seconds, so check that the desktop still runs.")) as Turn].into();
        assert!(matches!(crate::commands::wind_down(fake.as_ref(), &daemon.store).unwrap(), crate::commands::Off::Pending(_)));
        assert_eq!(fake.showing("main"), fixed);
        assert!(jpeg(&fixed));
    }

    #[test]
    fn gnome_writes_a_setting_only_on_first_use_a_picked_picture_and_off_then_on() {
        let (daemon, fake, original) = gnome_rig("fixed-needed");
        let fixed = fixed_of(&daemon);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!((writes(&fake), fake.showing("main")), (1, fixed.clone()));
        for icons in [vec![corner(), subject()], Vec::new(), vec![subject()]] {
            fake.place(icons);
            daemon.tick();
        }
        assert_eq!(writes(&fake), 1);
        let picked = picture(original.parent().unwrap(), "picked.png");
        fake.show("main", &picked);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!((writes(&fake), fake.showing("main"), daemon.store.load().screens["main"].source.clone()), (2, fixed.clone(), picked.clone()));
        let seen = Arc::new(Mutex::new(None));
        let (noted, watched) = (seen.clone(), fixed.clone());
        *fake.on_set.lock().unwrap() = Some(Box::new(move || *noted.lock().unwrap() = Some(watched.is_file())));
        std::fs::write(daemon.store.staged("main"), b"left by a set that stopped").unwrap();
        assert!(matches!(crate::commands::wind_down(fake.as_ref(), &daemon.store).unwrap(), crate::commands::Off::Done(_)));
        assert_eq!((writes(&fake), fake.showing("main"), *seen.lock().unwrap()), (3, picked.clone(), Some(true)));
        assert!(!fixed.exists());
        assert_eq!(files_in(&daemon.store.out()), Vec::<PathBuf>::new());
        let mut state = daemon.store.load();
        state.on = true;
        daemon.store.save(&state).unwrap();
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!((writes(&fake), fake.showing("main")), (4, fixed));
    }

    #[test]
    fn an_upgraded_gnome_screen_moves_from_its_counted_file_to_the_fixed_file_with_one_write() {
        let (daemon, fake, _) = rig("fixed-upgrade", vec![screen("main", 0.0, 1.0)]);
        *fake.unknowable.lock().unwrap() = true;
        fake.place(vec![subject()]);
        daemon.tick();
        let counted = daemon.store.output("main", 1, "jpg");
        assert_eq!((fake.showing("main"), writes(&fake)), (counted.clone(), 1));
        *fake.reloads.lock().unwrap() = true;
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        fake.place(Vec::new());
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        let fixed = fixed_of(&daemon);
        assert_eq!((fake.showing("main"), writes(&fake)), (fixed.clone(), 2));
        assert_eq!(files_in(&daemon.store.out()), vec![fixed.clone()]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        fake.place(Vec::new());
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert_eq!((fake.showing("main"), writes(&fake)), (fixed, 2));
    }

    #[test]
    fn a_desktop_that_does_not_load_the_file_again_keeps_counted_names_and_the_same_calls() {
        let (daemon, fake, _) = rig("counted-stays", vec![screen("main", 0.0, 1.0)]);
        for icons in [vec![subject()], Vec::new(), vec![subject()]] {
            fake.place(icons);
            daemon.tick();
        }
        let sets: Vec<String> = fake.events.lock().unwrap().iter().map(|(what, _)| what.clone()).filter(|what| what.starts_with("set ")).collect();
        assert_eq!(sets, vec!["set main-1.jpg", "set main-2.png", "set main-3.jpg"]);
        assert_eq!(writes(&fake), 3);
        assert_eq!(files_in(&daemon.store.out()), vec![daemon.store.output("main", 3, "jpg")]);
        let before = daemon.store.load().screens["main"].clone();
        *fake.crash_after_set.lock().unwrap() = true;
        fake.place(Vec::new());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| daemon.tick())).is_err());
        let after = daemon.store.load().screens["main"].clone();
        assert_eq!((after.fingerprint, after.placement, after.output), (before.fingerprint, before.placement, Some(daemon.store.output("main", 4, "png"))));
        let (refusing, desk, _) = rig("counted-refused", vec![screen("main", 0.0, 1.0)]);
        desk.place(vec![subject()]);
        refusing.tick();
        let before = refusing.store.load().screens["main"].clone();
        desk.refuse.lock().unwrap().push("main".into());
        desk.place(Vec::new());
        assert!(refusing.tick().problem.is_some());
        let refused = refusing.store.load().screens["main"].clone();
        assert!(refused.fingerprint == before.fingerprint && refused.placement.as_ref().is_some_and(scene::is_identity), "{:?}", refused.fingerprint);
    }

    #[test]
    fn a_gnome_desktop_that_goes_silent_after_the_rename_is_tried_again_and_converges() {
        let (daemon, fake, original) = gnome_rig("fixed-silent");
        let (fixed, source) = (fixed_of(&daemon), std::fs::read(&original).unwrap());
        plain_then(&daemon, &fake, &source);
        fake.place(vec![subject()]);
        *fake.silent_set.lock().unwrap() = 1;
        let held = daemon.tick();
        assert!(held.silent, "{:?}", held.problem);
        assert_eq!(held.problem.as_deref(), Some("gsettings did not finish within 10 seconds, so check that the desktop still runs."));
        assert!(jpeg(&fixed));
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.fingerprint, entry.placement, entry.output), (None, None, Some(fixed.clone())));
        let worker = Signal::default();
        assert_eq!(rearm(&worker, &held, 0), 1);
        let back = daemon.tick();
        assert_eq!((back.silent, back.problem.clone()), (false, None));
        assert!(matches!(back.outcomes[0].1, Outcome::Kept | Outcome::Rendered), "{:?}", back.outcomes);
        assert_eq!(rearm(&worker, &back, 1), 0);
        assert!(jpeg(&fixed));
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!((writes(&fake), fake.showing("main")), (1, fixed));
    }

    #[test]
    fn a_gnome_retry_that_finds_its_picture_already_in_place_records_whether_it_is_painted() {
        let (daemon, fake, _) = gnome_rig("fixed-painted");
        set_scenery(&daemon, true);
        fake.place(vec![calm()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let fixed = fixed_of(&daemon);
        let painted = std::fs::read(&fixed).unwrap();
        let mut state = daemon.store.load();
        let entry = state.screens.get_mut("main").unwrap();
        assert!(entry.painted);
        (entry.painted, entry.fingerprint, entry.placement) = (false, None, None);
        daemon.store.save(&state).unwrap();
        assert_eq!(outcome(&daemon.tick()), Outcome::Kept);
        assert_eq!(std::fs::read(&fixed).unwrap(), painted);
        assert!(daemon.store.load().screens["main"].painted);
    }
}
