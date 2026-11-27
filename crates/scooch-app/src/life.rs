use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Runtime};

pub const WAIT: &str = "--wait";
pub const HANDOVER: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_secs(5);
#[cfg(target_os = "linux")]
pub const ANSWER: Duration = Duration::from_secs(3);
#[cfg(target_os = "linux")]
pub const PAUSE: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    WindowClosed,
    Off,
    Quit,
    Replaced,
    Fresh,
    Gone,
    NoIcon,
    Moving,
    System,
}

#[cfg(any(feature = "updater", test))]
pub fn installs(exit: Exit) -> bool {
    exit == Exit::WindowClosed
}

#[cfg(any(target_os = "macos", test))]
pub fn restarts(exit: Exit, icon: bool, on: bool, powering_off: bool) -> bool {
    exit == Exit::System && icon && on && !powering_off
}

static REASON: Mutex<Option<Exit>> = Mutex::new(None);

pub fn mark(exit: Exit) {
    let mut held = REASON.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if held.is_none() {
        *held = Some(exit);
    }
}

pub fn reason() -> Exit {
    REASON.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).unwrap_or(Exit::System)
}

pub fn end<R: Runtime>(app: &AppHandle<R>, exit: Exit) {
    mark(exit);
    app.exit(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Icon,
    Window,
    End,
}

pub fn start(background: bool, held: bool) -> Start {
    match (held, background) {
        (true, _) => Start::Icon,
        (false, true) => Start::End,
        (false, false) => Start::Window,
    }
}

pub fn waits(arguments: &[String]) -> bool {
    arguments.iter().skip(1).any(|argument| argument == WAIT)
}

pub fn ends_without_icon(background: bool, made: bool) -> bool {
    background && !made
}

pub fn answers(held: bool, fresh: bool) -> bool {
    held || !fresh
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, PartialEq, Eq)]
pub enum Entry<H> {
    Holds(H),
    Sent,
    Alone,
}

#[cfg(any(target_os = "linux", test))]
pub fn enter<H>(patience: Duration, pause: Duration, mut take: impl FnMut() -> Option<H>, mut send: impl FnMut() -> bool) -> Entry<H> {
    let deadline = std::time::Instant::now() + patience;
    loop {
        if let Some(held) = take() {
            return Entry::Holds(held);
        }
        if send() {
            return Entry::Sent;
        }
        if std::time::Instant::now() >= deadline {
            return Entry::Alone;
        }
        std::thread::sleep(pause);
    }
}

pub struct Gate(AtomicBool);

impl Gate {
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub fn first(&self) -> bool {
        !self.0.swap(true, Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
pub enum Change {
    Same,
    Moved,
    Replaced,
    Gone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Stay,
    End,
    Handover,
    AtClose,
}

pub fn step(change: Change, on: bool, window: bool) -> Step {
    match change {
        Change::Same | Change::Moved => Step::Stay,
        _ if window => Step::AtClose,
        Change::Replaced if on => Step::Handover,
        Change::Replaced | Change::Gone => Step::End,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Close {
    Keep,
    End(Exit),
    Handover,
    Fresh,
}

pub fn at_close(on: bool, icon: bool, update: bool, change: Change, fresh: bool) -> Close {
    if !(on && icon) || update {
        return Close::End(Exit::WindowClosed);
    }
    match step(change, on, false) {
        Step::Handover => Close::Handover,
        Step::End => Close::End(Exit::Gone),
        Step::Stay | Step::AtClose if fresh && change == Change::Same => Close::Fresh,
        Step::Stay | Step::AtClose => Close::Keep,
    }
}

struct Wanted {
    opening: usize,
    working: usize,
    closes: usize,
    closing: bool,
    leaving: bool,
}

pub struct Door {
    wanted: Mutex<Wanted>,
    settled: Condvar,
}

impl Door {
    pub const fn new() -> Self {
        Self { wanted: Mutex::new(Wanted { opening: 0, working: 0, closes: 0, closing: false, leaving: false }), settled: Condvar::new() }
    }

    fn held(&self) -> MutexGuard<'_, Wanted> {
        self.wanted.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn closes(&self) {
        let mut held = self.held();
        held.closing = true;
        held.closes += 1;
    }

    pub fn settle(&self) {
        self.held().closing = false;
        self.settled.notify_all();
    }

    pub fn quiet(&self) -> usize {
        let (held, _) = self.settled.wait_timeout_while(self.held(), SETTLE, |wanted| wanted.closing).unwrap_or_else(|poisoned| poisoned.into_inner());
        held.closes
    }

    pub fn opens(&self) -> bool {
        let mut held = self.held();
        if held.leaving {
            return false;
        }
        held.opening += 1;
        true
    }

    pub fn opened(&self) {
        let mut held = self.held();
        held.opening = held.opening.saturating_sub(1);
    }

    pub fn works(&self) -> Option<Working<'_>> {
        let mut held = self.held();
        if held.leaving {
            return None;
        }
        held.working += 1;
        Some(Working(self))
    }

    fn leaves(&self) -> bool {
        let mut held = self.held();
        if held.opening > 0 || held.working > 0 {
            return false;
        }
        held.leaving = true;
        true
    }

    fn stays(&self) {
        self.held().leaving = false;
    }
}

pub struct Working<'a>(&'a Door);

impl Drop for Working<'_> {
    fn drop(&mut self) {
        let mut held = self.0.held();
        held.working = held.working.saturating_sub(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leave {
    Gone,
    Busy,
    Failed,
}

pub fn leave(door: &Door, start: impl FnOnce() -> bool) -> Leave {
    if !door.leaves() {
        return Leave::Busy;
    }
    if start() {
        return Leave::Gone;
    }
    door.stays();
    Leave::Failed
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
}

#[cfg(unix)]
fn stamp(meta: &std::fs::Metadata) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    Stamp { device: meta.dev(), inode: meta.ino(), size: meta.size(), modified: (meta.mtime(), meta.mtime_nsec()) }
}

pub struct Program {
    pub path: PathBuf,
    #[cfg(unix)]
    stamp: Stamp,
    #[cfg(unix)]
    file: std::fs::File,
}

impl Program {
    #[cfg(unix)]
    pub fn at(path: PathBuf) -> Option<Self> {
        let file = std::fs::File::open(&path).ok()?;
        let stamp = stamp(&file.metadata().ok()?);
        Some(Self { path, stamp, file })
    }

    #[cfg(not(unix))]
    pub fn at(path: PathBuf) -> Option<Self> {
        Some(Self { path })
    }

    pub fn own() -> Option<Self> {
        #[cfg(target_os = "linux")]
        if let Some(image) = std::env::var_os("APPIMAGE").filter(|value| !value.is_empty()) {
            return Self::at(PathBuf::from(image));
        }
        Self::at(std::env::current_exe().ok()?)
    }

    #[cfg(unix)]
    pub fn change(&self) -> Change {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&self.path) {
            Ok(now) if stamp(&now) == self.stamp => Change::Same,
            Ok(_) => Change::Replaced,
            Err(_) if self.file.metadata().is_ok_and(|held| held.nlink() > 0) => Change::Moved,
            Err(_) => Change::Gone,
        }
    }

    #[cfg(not(unix))]
    pub fn change(&self) -> Change {
        Change::Same
    }
}

pub fn relaunch(program: &Path) -> std::io::Result<()> {
    started(program, &[crate::tray::BACKGROUND, WAIT])
}

pub fn reopen(program: &Path) -> std::io::Result<()> {
    started(program, &[])
}

#[cfg(unix)]
fn started(program: &Path, arguments: &[&str]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut command = Command::new(program);
    command.args(arguments).current_dir("/").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(target_os = "macos")]
    for name in ["XPC_SERVICE_NAME", "XPC_FLAGS", "__CFBundleIdentifier"] {
        command.env_remove(name);
    }
    #[cfg(target_os = "linux")]
    crate::appimage::apply(&mut command);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().map(drop)
}

#[cfg(not(unix))]
fn started(program: &Path, arguments: &[&str]) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    Command::new(program).args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY: [Exit; 9] = [Exit::WindowClosed, Exit::Off, Exit::Quit, Exit::Replaced, Exit::Fresh, Exit::Gone, Exit::NoIcon, Exit::Moving, Exit::System];

    #[test]
    fn an_update_installs_only_when_a_person_closes_the_window() {
        for exit in EVERY {
            assert_eq!(installs(exit), exit == Exit::WindowClosed, "{exit:?}");
        }
    }

    #[test]
    fn a_window_close_with_an_update_waiting_ends_the_process_even_with_the_icon() {
        assert_eq!(at_close(true, true, true, Change::Same, false), Close::End(Exit::WindowClosed));
        assert_eq!(at_close(true, true, false, Change::Same, false), Close::Keep);
        assert_eq!(at_close(false, true, false, Change::Same, false), Close::End(Exit::WindowClosed));
        assert_eq!(at_close(true, false, false, Change::Same, false), Close::End(Exit::WindowClosed));
    }

    #[test]
    fn linux_gives_the_icon_to_a_fresh_process_at_the_close() {
        assert_eq!(at_close(true, true, false, Change::Same, true), Close::Fresh);
    }

    #[test]
    fn the_other_systems_keep_the_icon_in_the_same_process() {
        assert_eq!(at_close(true, true, false, Change::Same, false), Close::Keep);
        assert_eq!(at_close(true, true, false, Change::Moved, false), Close::Keep);
    }

    #[test]
    fn an_update_waiting_still_ends_the_process_on_linux() {
        assert_eq!(at_close(true, true, true, Change::Same, true), Close::End(Exit::WindowClosed));
        assert_eq!(at_close(true, true, true, Change::Replaced, true), Close::End(Exit::WindowClosed));
    }

    #[test]
    fn scooch_off_or_no_icon_still_ends_the_process_on_linux() {
        assert_eq!(at_close(false, true, false, Change::Same, true), Close::End(Exit::WindowClosed));
        assert_eq!(at_close(true, false, false, Change::Same, true), Close::End(Exit::WindowClosed));
    }

    #[test]
    fn a_replaced_program_still_hands_over_on_linux() {
        assert_eq!(at_close(true, true, false, Change::Replaced, true), Close::Handover);
        assert_eq!(at_close(true, true, false, Change::Gone, true), Close::End(Exit::Gone));
    }

    #[test]
    fn a_moved_program_has_no_path_to_start_so_the_icon_stays() {
        assert_eq!(at_close(true, true, false, Change::Moved, true), Close::Keep);
    }

    #[test]
    fn a_failed_start_keeps_the_icon() {
        let door = Door::new();
        assert_eq!(leave(&door, || false), Leave::Failed);
        assert!(door.opens(), "the door stayed shut after a start that failed");
        door.opened();
        assert_eq!(leave(&door, || true), Leave::Gone);
    }

    #[test]
    fn a_window_asked_for_during_the_hand_over_is_not_opened_here() {
        let door = Door::new();
        assert!(door.opens());
        door.opened();
        assert_eq!(leave(&door, || true), Leave::Gone);
        assert!(!door.opens());
    }

    #[test]
    fn a_close_while_a_window_opens_starts_nothing() {
        let door = Door::new();
        assert!(door.opens());
        assert_eq!(leave(&door, || panic!("a fresh process was started")), Leave::Busy);
        door.opened();
        assert_eq!(leave(&door, || true), Leave::Gone);
    }

    #[test]
    fn a_close_while_work_runs_starts_nothing() {
        let door = Door::new();
        let turning_off = door.works();
        let saving = door.works();
        assert_eq!(leave(&door, || panic!("a fresh process was started")), Leave::Busy);
        drop(turning_off);
        assert_eq!(leave(&door, || panic!("a fresh process was started")), Leave::Busy);
        drop(saving);
        assert_eq!(leave(&door, || true), Leave::Gone);
        assert!(door.works().is_none(), "work began in a process that hands the icon over");
    }

    #[test]
    fn a_window_asked_for_during_a_close_waits_until_the_close_is_over() {
        let door = std::sync::Arc::new(Door::new());
        assert_eq!(door.quiet(), 0);
        door.closes();
        let waiting = door.clone();
        let started = std::time::Instant::now();
        let asked = std::thread::spawn(move || waiting.quiet());
        std::thread::sleep(Duration::from_millis(150));
        door.settle();
        assert_eq!(asked.join().unwrap(), 1);
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert_eq!(leave(&door, || true), Leave::Gone);
    }

    #[test]
    fn a_fresh_hand_over_installs_nothing_and_restarts_nothing() {
        assert!(!installs(Exit::Fresh));
        assert!(!restarts(Exit::Fresh, true, true, false));
    }

    #[test]
    fn a_plain_start_takes_the_lock_or_hands_the_start_to_its_holder() {
        let none = Duration::ZERO;
        assert_eq!(enter(none, none, || Some(7), || panic!("the holder was asked")), Entry::Holds(7));
        assert_eq!(enter(none, none, || None::<u8>, || true), Entry::Sent);
        assert_eq!(enter(none, none, || None::<u8>, || false), Entry::Alone);
    }

    #[test]
    fn a_plain_start_during_the_hand_over_waits_for_the_lock_or_the_next_holder() {
        let mut tries = 0;
        let freed = enter(Duration::from_secs(5), Duration::from_millis(1), || {
            tries += 1;
            (tries == 3).then_some(tries)
        }, || false);
        assert_eq!(freed, Entry::Holds(3));
        let mut asked = 0;
        let answered = enter(Duration::from_secs(5), Duration::from_millis(1), || None::<u8>, || {
            asked += 1;
            asked == 4
        });
        assert_eq!(answered, Entry::Sent);
        assert_eq!(asked, 4);
        let started = std::time::Instant::now();
        assert_eq!(enter(Duration::from_millis(120), Duration::from_millis(10), || None::<u8>, || false), Entry::Alone);
        assert!(started.elapsed() >= Duration::from_millis(120));
    }

    #[test]
    fn with_fresh_hand_overs_only_the_lock_holder_answers_later_starts() {
        assert!(answers(true, true));
        assert!(!answers(false, true));
        assert!(answers(true, false));
        assert!(answers(false, false));
    }

    #[test]
    fn only_the_lock_holder_makes_an_icon_and_a_background_start_without_it_ends() {
        assert_eq!(start(true, true), Start::Icon);
        assert_eq!(start(false, true), Start::Icon);
        assert_eq!(start(true, false), Start::End);
        assert_eq!(start(false, false), Start::Window);
    }

    #[test]
    fn only_the_wait_argument_after_the_program_marks_a_hand_over() {
        let list = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
        assert!(waits(&list(&["scooch-app", "--background", "--wait"])));
        assert!(!waits(&list(&["scooch-app", "--background"])));
        assert!(!waits(&list(&["--wait"])));
    }

    #[test]
    fn a_background_start_whose_icon_failed_ends() {
        assert!(ends_without_icon(true, false));
        assert!(!ends_without_icon(true, true));
        assert!(!ends_without_icon(false, false));
    }

    #[test]
    fn the_window_work_runs_once() {
        let gate = Gate::new();
        assert!(gate.first());
        assert!(!gate.first());
        assert!(!gate.first());
    }

    #[test]
    fn a_replaced_program_hands_over_only_while_on_and_waits_for_an_open_window() {
        assert_eq!(step(Change::Same, true, false), Step::Stay);
        assert_eq!(step(Change::Moved, true, false), Step::Stay);
        assert_eq!(step(Change::Replaced, true, false), Step::Handover);
        assert_eq!(step(Change::Replaced, false, false), Step::End);
        assert_eq!(step(Change::Gone, true, false), Step::End);
        assert_eq!(step(Change::Replaced, true, true), Step::AtClose);
        assert_eq!(step(Change::Gone, true, true), Step::AtClose);
        assert_eq!(at_close(true, true, false, Change::Replaced, false), Close::Handover);
        assert_eq!(at_close(true, true, false, Change::Gone, false), Close::End(Exit::Gone));
        assert_eq!(at_close(true, true, false, Change::Moved, false), Close::Keep);
    }

    #[test]
    fn only_a_system_quit_with_scooch_on_and_no_power_off_starts_the_icon_again() {
        for exit in EVERY {
            assert_eq!(restarts(exit, true, true, false), exit == Exit::System, "{exit:?}");
        }
        assert!(!restarts(Exit::System, true, true, true));
        assert!(!restarts(Exit::System, true, false, false));
        assert!(!restarts(Exit::System, false, true, false));
    }

    #[cfg(unix)]
    #[test]
    fn the_program_file_reads_as_same_moved_replaced_or_gone() {
        let folder = std::env::temp_dir().join(format!("scooch-app-program-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("scooch-app");
        std::fs::write(&path, b"old").unwrap();
        let program = Program::at(path.clone()).unwrap();
        assert_eq!(program.change(), Change::Same);
        let aside = folder.join("aside");
        std::fs::rename(&path, &aside).unwrap();
        assert_eq!(program.change(), Change::Moved);
        let staged = folder.join("staged");
        std::fs::write(&staged, b"new").unwrap();
        std::fs::rename(&staged, &path).unwrap();
        assert_eq!(program.change(), Change::Replaced);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(&aside).unwrap();
        assert_eq!(program.change(), Change::Gone);
        let rewritten = Program::at({
            std::fs::write(&path, b"one").unwrap();
            path.clone()
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&path, b"two!").unwrap();
        assert_eq!(rewritten.change(), Change::Replaced);
    }
}
