use super::kde::{self as plasma, Board};
use super::parse::desktop::{self, Kind, Session};
use super::parse::drops::{self as rules, hand, key, off_line, through, Guess, Hold, Press, Tracker, Verdict, HOLD_AFTER_RELEASE, NET_PLASMA, NET_POLL, NET_RC, NET_YAML};
use super::parse::kde::{self as plasma_rules, Spot};
use super::parse::xfce::contains;
use super::pointer::{self, Input};
use super::screens::Shared;
use super::{xfce, Linux};
use crate::platform::{Desktop, Screen, Snapshot};
use scooch_core::Rect;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub static PREDICTED: Mutex<Option<(Snapshot, Option<Hold>)>> = Mutex::new(None);
static HOLD: Mutex<Option<Hold>> = Mutex::new(None);

pub fn predicts() -> bool {
    let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    applies(desktop::pick(&current), desktop::session(std::env::var("WAYLAND_DISPLAY").ok().as_deref()))
}

pub fn applies(kind: Option<Kind>, session: Session) -> bool {
    rules::applies(kind, session, || !plasma::plasma6())
}

fn guard<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
    slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn take(shown: &dyn Fn() -> bool) -> Option<Snapshot> {
    let offered = guard(&PREDICTED).take();
    hand(offered, &mut guard(&HOLD), shown)
}

pub fn held(snapshot: Snapshot) -> Snapshot {
    let mut hold = guard(&HOLD);
    if hold.is_none() {
        return snapshot;
    }
    let icons = through(&mut hold, snapshot.icons, Instant::now());
    if crate::daemon::trace() {
        let what = if hold.is_some() { "kept the guessed places over a read from before the save" } else { "ended" };
        eprintln!("{} drop hold {what}", crate::daemon::stamp());
    }
    Snapshot { screens: snapshot.screens, icons }
}

fn extend_hold(until: Instant) {
    if let Some(hold) = guard(&HOLD).as_mut() {
        hold.until = hold.until.max(until);
    }
}

fn screens(linux: &Linux) -> Vec<Screen> {
    linux.all_screens().map(|found| found.into_iter().map(|found| found.screen).collect()).unwrap_or_default()
}

fn moved(snapshot: &Snapshot, from: &Rect, to: Rect) -> Snapshot {
    let mut icons = snapshot.icons.clone();
    if let Some(icon) = icons.iter_mut().find(|icon| key(icon) == key(from)) {
        *icon = to;
    }
    Snapshot { screens: snapshot.screens.clone(), icons }
}

fn place(rect: &Rect) -> String {
    format!("{:.0},{:.0}", rect.x, rect.y)
}

enum Wait {
    For(Duration),
    Save { positions: String, released: Arc<Mutex<Instant>> },
}

impl Wait {
    fn pass(self) {
        match self {
            Wait::For(time) => std::thread::sleep(time),
            Wait::Save { positions, released } => loop {
                std::thread::sleep(NET_POLL);
                if saved_positions() != positions || guard(&released).elapsed() >= NET_PLASMA {
                    return;
                }
            },
        }
    }
}

fn saved_positions() -> String {
    plasma_rules::positions_text(&plasma::groups(plasma_rules::APPLETS))
}

fn judge(tracker: &Arc<Mutex<Tracker>>, guess: Guess, before: Vec<Rect>, net: &Shared, wait: Wait, name: &'static str) {
    let (tracker, net) = (tracker.clone(), net.clone());
    let _ = std::thread::Builder::new().name("scooch-drop-net".into()).spawn(move || {
        wait.pass();
        let Ok(linux) = Linux::open() else { return };
        let Ok(after) = linux.fresh() else { return };
        let was_off = guard(&tracker).off;
        let verdict = guard(&tracker).verify(&guess, &before, &after.icons);
        if matches!(guess, Guess::Move { shown: true, .. }) {
            *guard(&HOLD) = None;
        }
        let stamp = crate::daemon::stamp();
        match (verdict, guess) {
            (Some(Verdict::Missed), Guess::Move { to, shown: true, .. }) => {
                eprintln!("{stamp} predicted drop missed, guessed {} and the file holds {:?}", place(&to), after.icons.iter().map(place).collect::<Vec<_>>());
                if !was_off {
                    eprintln!("{}", off_line(name));
                }
                net();
            }
            (Some(Verdict::Back), _) => eprintln!("{stamp} predicted drop back on after {} guesses in a row held", rules::SHADOWS),
            (Some(Verdict::Agreed), Guess::Move { shown: true, .. }) => eprintln!("{stamp} predicted drop agreed with the file"),
            _ => {}
        }
    });
}

fn said(reason: Option<&str>) {
    if let Some(reason) = reason {
        eprintln!("{} drop not predicted, {reason}", crate::daemon::stamp());
    }
}

pub fn run(predicted: &Shared, net: &Shared) -> bool {
    let Ok(linux) = Linux::open() else { return false };
    let Some(home) = dirs::home_dir() else { return false };
    let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
    let shell = xfce::Xfce { desktop: linux.desktop.clone(), home, config, session: linux.session };
    let factor = xfce::scale().max(1) as f32;
    let tracker = Arc::new(Mutex::new(Tracker::new()));
    let mut held: Option<Snapshot> = None;
    pointer::inputs(None, &mut |input| match input {
        Input::Press { button, at, control, shift, desktop } => {
            let point = (at.0 / factor, at.1 / factor);
            let snapshot = if button == 1 && desktop { linux.snapshot().ok() } else { None };
            let icon = snapshot.as_ref().and_then(|snapshot| snapshot.icons.iter().find(|icon| contains(icon, point)).copied());
            let mut tracker = guard(&tracker);
            if let Some(snapshot) = &snapshot {
                tracker.seen(&snapshot.icons);
            }
            tracker.pressed(Press { at: point, button, control, shift, desktop, icon });
            held = snapshot;
        }
        Input::Key { desktop_active } => guard(&tracker).key(desktop_active),
        Input::Start { .. } => {}
        Input::Release { at, desktop, .. } => {
            let point = (at.0 / factor, at.1 / factor);
            let snapshot = held.take();
            let lattice = snapshot.as_ref().and_then(|_| shell.lattice(&screens(&linux)));
            let icons = snapshot.as_ref().map(|snapshot| snapshot.icons.clone()).unwrap_or_default();
            let (guess, reason) = {
                let mut tracker = guard(&tracker);
                let guess = tracker.released(point, desktop, |_| lattice.as_ref().and_then(|lattice| lattice.landing(&icons, point)));
                (guess, tracker.reason)
            };
            said(reason);
            let Some(snapshot) = snapshot else { return };
            if let Guess::Move { from, to, shown: true } = guess {
                *guard(&PREDICTED) = Some((moved(&snapshot, &from, to), None));
                predicted();
            }
            if guess != Guess::Nothing {
                let wait = Wait::For(if shell.configs().is_some() { NET_YAML } else { NET_RC });
                judge(&tracker, guess, snapshot.icons, net, wait, "Xfce");
            }
        }
    })
}

struct Pressed {
    at: (f32, f32),
    start: Option<(f32, f32)>,
    snapshot: Snapshot,
    boards: Vec<Board>,
    positions: String,
}

impl Pressed {
    fn read(linux: &Linux, shell: &plasma::Kde, at: (f32, f32)) -> Option<Pressed> {
        let positions = saved_positions();
        let found = linux.all_screens().ok()?;
        let state = plasma::state().ok()?;
        let mapping = plasma::mapping();
        let screens = linux.pick(found);
        let icons = screens.iter().flat_map(|screen| shell.icons_on(&state, screen, false, &mapping)).collect();
        let boards = shell.boards(&state, &screens, &mapping);
        Some(Pressed { at, start: None, snapshot: Snapshot { screens, icons }, boards, positions })
    }

    fn board(&self, point: (f32, f32)) -> Option<&Board> {
        self.boards.iter().find(|board| board.grid.cell_at(point).is_some())
    }

    fn spot(&self) -> (Spot, Option<Rect>) {
        let Some(board) = self.board(self.at) else { return (Spot::Elsewhere, None) };
        match board.grid.spot(self.at) {
            Spot::Icon(index) => (Spot::Icon(index), Some(board.grid.icon(index))),
            other => (other, None),
        }
    }

    fn land(&self, from: &Rect, release: (f32, f32)) -> Result<Rect, &'static str> {
        let board = self.board(self.at).ok_or("the press was off Plasma's icons")?;
        if let Some(why) = board.why_not {
            return Err(why);
        }
        let index = board.cells.iter().find(|(_, rect)| key(rect) == key(from)).map(|(index, _)| *index).ok_or("the pressed icon was not on the grid")?;
        let start = self.start.ok_or("the pointer never went far enough for Plasma to begin a drag")?;
        board.grid.landing(index, start, release).map(|to| board.grid.icon(to))
    }
}

pub fn run_plasma(predicted: &Shared, net: &Shared) -> bool {
    let Ok(linux) = Linux::open() else { return false };
    let shell = plasma::Kde { desktop: linux.desktop.clone(), config: plasma::config_folder(), session: linux.session };
    let tracker = Arc::new(Mutex::new(Tracker::new()));
    let released = Arc::new(Mutex::new(Instant::now()));
    let mut held: Option<Pressed> = None;
    let distance = plasma_rules::drag_distance(&plasma::groups("kdeglobals"));
    pointer::inputs(Some(distance), &mut |input| match input {
        Input::Press { button, at, control, shift, desktop } => {
            let pressed = if button == 1 && desktop { Pressed::read(&linux, &shell, at) } else { None };
            let (spot, icon) = pressed.as_ref().map_or((Spot::Elsewhere, None), Pressed::spot);
            let mut tracker = guard(&tracker);
            if let Some(pressed) = &pressed {
                tracker.seen(&pressed.snapshot.icons);
            }
            tracker.pressed(Press { at, button, control: control || matches!(spot, Spot::Corner(_)), shift, desktop, icon });
            held = pressed;
        }
        Input::Key { desktop_active } => guard(&tracker).key(desktop_active),
        Input::Start { at } => {
            if let Some(pressed) = held.as_mut() {
                pressed.start = Some(at);
            }
        }
        Input::Release { at, desktop, .. } => {
            if desktop {
                *guard(&released) = Instant::now();
                extend_hold(Instant::now() + HOLD_AFTER_RELEASE);
            }
            let pressed = held.take();
            let mut why = None;
            let (guess, reason) = {
                let mut tracker = guard(&tracker);
                let guess = tracker.released(at, desktop, |from| pressed.as_ref().and_then(|pressed| pressed.land(from, at).map_err(|reason| why = Some(reason)).ok()));
                if tracker.reason.is_some() && why.is_some() {
                    tracker.reason = why;
                }
                (guess, tracker.reason)
            };
            said(reason);
            let Some(pressed) = pressed else { return };
            if let Guess::Move { from, to, shown: true } = guess {
                let after = moved(&pressed.snapshot, &from, to);
                let hold = Hold::new(&pressed.snapshot.icons, after.icons.clone(), Instant::now() + HOLD_AFTER_RELEASE);
                *guard(&PREDICTED) = Some((after, Some(hold)));
                predicted();
            }
            if guess != Guess::Nothing {
                let wait = Wait::Save { positions: pressed.positions, released: released.clone() };
                judge(&tracker, guess, pressed.snapshot.icons, net, wait, "Plasma");
            }
        }
    })
}
