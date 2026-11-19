use super::parse::drops::{key, Guess, Press, Tracker, Verdict, NET_RC, NET_YAML};
use super::parse::xfce::contains;
use super::pointer::{self, Input};
use super::screens::Shared;
use super::{xfce, Linux};
use crate::platform::{Desktop, Screen, Snapshot};
use scooch_core::Rect;
use std::sync::{Arc, Mutex, MutexGuard};

pub static PREDICTED: Mutex<Option<Snapshot>> = Mutex::new(None);

pub const OFF: &str = "Scooch stopped moving the picture at the drop on Xfce after a wrong guess, and waits for Xfce to save the places until three guesses in a row hold again.";

fn lock(tracker: &Mutex<Tracker>) -> MutexGuard<'_, Tracker> {
    tracker.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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

fn judge(tracker: &Arc<Mutex<Tracker>>, guess: Guess, before: Vec<Rect>, net: &Shared, yaml: bool) {
    let (tracker, net) = (tracker.clone(), net.clone());
    let wait = if yaml { NET_YAML } else { NET_RC };
    let _ = std::thread::Builder::new().name("scooch-drop-net".into()).spawn(move || {
        std::thread::sleep(wait);
        let Ok(linux) = Linux::open() else { return };
        let Ok(after) = linux.snapshot() else { return };
        let was_off = lock(&tracker).off;
        let verdict = lock(&tracker).verify(&guess, &before, &after.icons);
        let stamp = crate::daemon::stamp();
        match (verdict, guess) {
            (Some(Verdict::Missed), Guess::Move { to, shown: true, .. }) => {
                eprintln!("{stamp} predicted drop missed, guessed {} and the file holds {:?}", place(&to), after.icons.iter().map(place).collect::<Vec<_>>());
                if !was_off {
                    eprintln!("{OFF}");
                }
                net();
            }
            (Some(Verdict::Back), _) => eprintln!("{stamp} predicted drop back on after {} guesses in a row held", super::parse::drops::SHADOWS),
            (Some(Verdict::Agreed), Guess::Move { shown: true, .. }) => eprintln!("{stamp} predicted drop agreed with the file"),
            _ => {}
        }
    });
}

fn place(rect: &Rect) -> String {
    format!("{:.0},{:.0}", rect.x, rect.y)
}

pub fn run(predicted: &Shared, net: &Shared) -> bool {
    let Ok(linux) = Linux::open() else { return false };
    let Some(home) = dirs::home_dir() else { return false };
    let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
    let shell = xfce::Xfce { desktop: linux.desktop.clone(), home, config, session: linux.session };
    let factor = xfce::scale().max(1) as f32;
    let tracker = Arc::new(Mutex::new(Tracker::new()));
    let mut held: Option<Snapshot> = None;
    pointer::inputs(&mut |input| match input {
        Input::Press { button, at, control, shift, desktop } => {
            let point = (at.0 / factor, at.1 / factor);
            let snapshot = if button == 1 && desktop { linux.snapshot().ok() } else { None };
            let icon = snapshot.as_ref().and_then(|snapshot| snapshot.icons.iter().find(|icon| contains(icon, point)).copied());
            let mut tracker = lock(&tracker);
            if let Some(snapshot) = &snapshot {
                tracker.seen(&snapshot.icons);
            }
            tracker.pressed(Press { at: point, button, control, shift, desktop, icon });
            held = snapshot;
        }
        Input::Key { desktop_active } => lock(&tracker).key(desktop_active),
        Input::Release { at, desktop, .. } => {
            let point = (at.0 / factor, at.1 / factor);
            let snapshot = held.take();
            let lattice = snapshot.as_ref().and_then(|_| shell.lattice(&screens(&linux)));
            let icons = snapshot.as_ref().map(|snapshot| snapshot.icons.clone()).unwrap_or_default();
            let (guess, reason) = {
                let mut tracker = lock(&tracker);
                let guess = tracker.released(point, desktop, |_| lattice.as_ref().and_then(|lattice| lattice.landing(&icons, point)));
                (guess, tracker.reason)
            };
            if let Some(reason) = reason {
                eprintln!("{} drop not predicted, {reason}", crate::daemon::stamp());
            }
            let Some(snapshot) = snapshot else { return };
            if let Guess::Move { from, to, shown: true } = guess {
                *PREDICTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(moved(&snapshot, &from, to));
                predicted();
            }
            if guess != Guess::Nothing {
                judge(&tracker, guess, snapshot.icons, net, shell.configs().is_some());
            }
        }
    })
}
