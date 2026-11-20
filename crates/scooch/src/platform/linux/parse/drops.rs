use super::desktop::{Kind, Session};
use crate::platform::Snapshot;
use scooch_core::Rect;
use std::time::{Duration, Instant};

pub const THRESHOLD: f32 = 8.0;
pub const NET_RC: Duration = Duration::from_millis(1250);
pub const NET_YAML: Duration = Duration::from_millis(2000);
pub const NET_PLASMA: Duration = Duration::from_millis(3000);
pub const NET_POLL: Duration = Duration::from_millis(50);
pub const HOLD_AFTER_RELEASE: Duration = Duration::from_millis(3500);
pub const SHADOWS: usize = 3;

pub fn applies(kind: Option<Kind>, session: Session, plasma5: impl FnOnce() -> bool) -> bool {
    session == Session::X11
        && match kind {
            Some(Kind::Xfce) => true,
            Some(Kind::Kde) => plasma5(),
            _ => false,
        }
}

pub fn off_line(desktop: &str) -> String {
    format!("Scooch stopped moving the picture at the drop on {desktop} after a wrong guess, and waits for {desktop} to save the places until three guesses in a row hold again.")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hold {
    pub before: Vec<[i64; 4]>,
    pub icons: Vec<Rect>,
    pub until: Instant,
}

impl Hold {
    pub fn new(before: &[Rect], icons: Vec<Rect>, until: Instant) -> Hold {
        Hold { before: keys(before), icons, until }
    }
}

pub fn hand(offered: Option<(Snapshot, Option<Hold>)>, hold: &mut Option<Hold>, shown: &dyn Fn() -> bool) -> Option<Snapshot> {
    let (snapshot, kept) = offered?;
    if !shown() {
        return None;
    }
    if let Some(kept) = kept {
        *hold = Some(kept);
    }
    Some(snapshot)
}

pub fn through(hold: &mut Option<Hold>, read: Vec<Rect>, now: Instant) -> Vec<Rect> {
    let Some(kept) = hold.as_ref() else { return read };
    if now < kept.until && keys(&read) == kept.before {
        return kept.icons.clone();
    }
    *hold = None;
    read
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Selection {
    Unknown,
    One(Option<Rect>),
    Many,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Press {
    pub at: (f32, f32),
    pub button: u32,
    pub control: bool,
    pub shift: bool,
    pub desktop: bool,
    pub icon: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guess {
    Nothing,
    Unsure { from: Rect },
    Move { from: Rect, to: Rect, shown: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    Agreed,
    Missed,
    Back,
}

pub fn key(rect: &Rect) -> [i64; 4] {
    [rect.x.round() as i64, rect.y.round() as i64, rect.w.round() as i64, rect.h.round() as i64]
}

fn keys(icons: &[Rect]) -> Vec<[i64; 4]> {
    let mut keys: Vec<[i64; 4]> = icons.iter().map(key).collect();
    keys.sort_unstable();
    keys
}

fn less(from: &[[i64; 4]], take: &[[i64; 4]]) -> Vec<[i64; 4]> {
    let mut left = from.to_vec();
    for each in take {
        if let Some(at) = left.iter().position(|kept| kept == each) {
            left.remove(at);
        }
    }
    left
}

pub fn moved_alone(before: &[Rect], after: &[Rect], from: &Rect) -> Option<[i64; 4]> {
    let (before, after) = (keys(before), keys(after));
    let (gone, came) = (less(&before, &after), less(&after, &before));
    came.first().copied().filter(|_| gone == [key(from)] && came.len() == 1)
}

pub struct Tracker {
    pub selection: Selection,
    pub off: bool,
    pub reason: Option<&'static str>,
    press: Option<Press>,
    keyed: bool,
    shadows: usize,
    known: Option<Vec<[i64; 4]>>,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    pub fn new() -> Self {
        Self { selection: Selection::Unknown, off: false, reason: None, press: None, keyed: false, shadows: 0, known: None }
    }

    pub fn seen(&mut self, icons: &[Rect]) {
        let now = keys(icons);
        if self.known.as_ref().is_some_and(|known| *known != now) {
            self.selection = Selection::Unknown;
        }
        self.known = Some(now);
    }

    pub fn pressed(&mut self, press: Press) {
        self.keyed = false;
        if press.desktop && (press.button == 3 || (press.button == 1 && (press.control || press.shift))) {
            self.selection = Selection::Many;
        }
        self.press = Some(press);
    }

    pub fn key(&mut self, desktop_active: bool) {
        if self.press.is_some() {
            self.keyed = true;
        }
        if desktop_active {
            self.selection = Selection::Many;
        }
    }

    pub fn released(&mut self, at: (f32, f32), desktop: bool, land: impl FnOnce(&Rect) -> Option<Rect>) -> Guess {
        self.reason = None;
        let Some(press) = self.press.take() else { return Guess::Nothing };
        let keyed = std::mem::take(&mut self.keyed);
        if press.button != 1 || !press.desktop || press.control || press.shift {
            return Guess::Nothing;
        }
        let moved = (at.0 - press.at.0).hypot(at.1 - press.at.1) > THRESHOLD;
        let Some(from) = press.icon else {
            self.selection = if moved { Selection::Many } else { Selection::One(None) };
            return Guess::Nothing;
        };
        if !moved {
            self.selection = Selection::One(Some(from));
            return Guess::Nothing;
        }
        let was = std::mem::replace(&mut self.selection, Selection::Unknown);
        let why = match was {
            _ if keyed => Some("a key went down during the drag"),
            _ if !desktop => Some("the release was not over the desktop"),
            Selection::Many => Some("the selection may hold more than one icon"),
            Selection::Unknown => Some("the selection is unknown"),
            Selection::One(_) => None,
        };
        if let Some(why) = why {
            self.reason = Some(why);
            return Guess::Unsure { from };
        }
        let shown = !self.off;
        match land(&from) {
            Some(to) => Guess::Move { from, to, shown },
            None => {
                self.reason = Some("the slot under the release is taken or off the grid");
                Guess::Nothing
            }
        }
    }

    pub fn verify(&mut self, guess: &Guess, before: &[Rect], after: &[Rect]) -> Option<Verdict> {
        let from = match guess {
            Guess::Nothing => return None,
            Guess::Unsure { from } | Guess::Move { from, .. } => *from,
        };
        let alone = moved_alone(before, after, &from);
        let held = keys(after);
        let gone = less(&keys(before), &held).len();
        self.selection = match alone {
            Some(place) => Selection::One(Some(Rect { x: place[0] as f32, y: place[1] as f32, w: place[2] as f32, h: place[3] as f32 })),
            None if gone > 1 => Selection::Many,
            None => Selection::Unknown,
        };
        self.known = Some(keys(after));
        let Guess::Move { to, shown, .. } = guess else { return None };
        let (agreed, shown) = (held.contains(&key(to)), *shown);
        if shown {
            if agreed {
                return Some(Verdict::Agreed);
            }
            self.off = true;
            self.shadows = 0;
            return Some(Verdict::Missed);
        }
        if !agreed {
            self.shadows = 0;
            return Some(Verdict::Missed);
        }
        self.shadows += 1;
        if self.shadows >= SHADOWS {
            self.off = false;
            self.shadows = 0;
            return Some(Verdict::Back);
        }
        Some(Verdict::Agreed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Rect {
        Rect { x, y, w: 93.0, h: 92.0 }
    }

    fn plain(on: (f32, f32), icon: Option<Rect>) -> Press {
        Press { at: on, button: 1, control: false, shift: false, desktop: true, icon }
    }

    fn landing(_: &Rect) -> Option<Rect> {
        Some(at(649.0, 506.0))
    }

    fn known_single(tracker: &mut Tracker, icon: Rect) {
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        assert_eq!(tracker.released((1008.0, 537.0), true, landing), Guess::Nothing);
        assert_eq!(tracker.selection, Selection::One(Some(icon)));
    }

    #[test]
    fn a_plain_drag_of_one_known_icon_predicts_its_landing() {
        let mut tracker = Tracker::new();
        let icon = at(961.0, 506.0);
        known_single(&mut tracker, icon);
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        assert_eq!(tracker.released((672.0, 594.0), true, landing), Guess::Move { from: icon, to: at(649.0, 506.0), shown: true });
        let other = at(1065.0, 506.0);
        known_single(&mut tracker, icon);
        tracker.pressed(plain((1110.0, 536.0), Some(other)));
        assert_eq!(tracker.released((672.0, 594.0), true, landing), Guess::Move { from: other, to: at(649.0, 506.0), shown: true }, "a press on an icon the state does not name selects it alone");
    }

    #[test]
    fn a_drag_after_a_control_click_predicts_nothing() {
        let mut tracker = Tracker::new();
        let icon = at(961.0, 506.0);
        known_single(&mut tracker, icon);
        tracker.pressed(Press { control: true, ..plain((1110.0, 536.0), Some(at(1065.0, 506.0))) });
        assert_eq!(tracker.released((1110.0, 536.0), true, landing), Guess::Nothing);
        assert_eq!(tracker.selection, Selection::Many);
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        assert_eq!(tracker.released((672.0, 594.0), true, landing), Guess::Unsure { from: icon });
    }

    #[test]
    fn a_drag_with_the_selection_unknown_predicts_nothing_until_one_verified_single_drag() {
        let mut tracker = Tracker::new();
        let icon = at(961.0, 506.0);
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        let unsure = tracker.released((672.0, 594.0), true, landing);
        assert_eq!(unsure, Guess::Unsure { from: icon });
        assert_eq!(tracker.reason, Some("the selection is unknown"));
        let moved = at(649.0, 506.0);
        assert_eq!(tracker.verify(&unsure, &[icon, at(18.0, 40.0)], &[moved, at(18.0, 40.0)]), None);
        assert_eq!(tracker.selection, Selection::One(Some(moved)));
        tracker.pressed(plain((695.0, 536.0), Some(moved)));
        assert!(matches!(tracker.released((960.0, 540.0), true, landing), Guess::Move { shown: true, .. }));
    }

    #[test]
    fn a_release_on_an_occupied_slot_or_off_the_grid_gives_no_guess() {
        let mut tracker = Tracker::new();
        let icon = at(961.0, 506.0);
        known_single(&mut tracker, icon);
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        assert_eq!(tracker.released((672.0, 594.0), true, |_| None), Guess::Nothing);
        assert_eq!(tracker.reason, Some("the slot under the release is taken or off the grid"));
        assert_eq!(tracker.selection, Selection::Unknown, "after a drop that xfdesktop may have turned into anything");
        assert_eq!(tracker.verify(&Guess::Nothing, &[icon], &[]), None);
        assert!(!tracker.off);
    }

    #[test]
    fn a_second_drag_before_the_net_does_not_count_as_a_miss() {
        let mut tracker = Tracker::new();
        let icon = at(961.0, 506.0);
        let other = at(18.0, 40.0);
        known_single(&mut tracker, icon);
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        let guess = tracker.released((672.0, 594.0), true, landing);
        assert_eq!(tracker.verify(&guess, &[icon, other], &[at(649.0, 506.0), at(122.0, 40.0)]), Some(Verdict::Agreed));
        assert!(!tracker.off);
        assert!(!matches!(tracker.selection, Selection::One(_)), "two moves in one net window cannot name the selection");
    }

    #[test]
    fn a_key_going_down_during_the_drag_predicts_nothing() {
        let mut tracker = Tracker::new();
        let icon = at(961.0, 506.0);
        known_single(&mut tracker, icon);
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        tracker.key(false);
        assert_eq!(tracker.released((672.0, 594.0), true, landing), Guess::Unsure { from: icon });
        assert_eq!(tracker.reason, Some("a key went down during the drag"));
        known_single(&mut tracker, icon);
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        assert_eq!(tracker.released((672.0, 594.0), false, landing), Guess::Unsure { from: icon });
        assert_eq!(tracker.reason, Some("the release was not over the desktop"));
    }

    #[test]
    fn a_miss_turns_prediction_off_until_three_guesses_in_a_row_held() {
        let mut tracker = Tracker::new();
        let icon = at(961.0, 506.0);
        known_single(&mut tracker, icon);
        tracker.pressed(plain((1007.0, 536.0), Some(icon)));
        let guess = tracker.released((672.0, 594.0), true, landing);
        assert_eq!(tracker.verify(&guess, &[icon], &[at(545.0, 506.0)]), Some(Verdict::Missed));
        assert!(tracker.off);
        let mut place = at(545.0, 506.0);
        for (round, target) in [at(649.0, 506.0), at(961.0, 506.0), at(649.0, 506.0)].into_iter().enumerate() {
            tracker.pressed(plain((place.x + 46.0, 536.0), Some(place)));
            let shadow = tracker.released((target.x + 23.0, 594.0), true, |_| Some(target));
            assert!(matches!(shadow, Guess::Move { shown: false, .. }), "{shadow:?}");
            let verdict = tracker.verify(&shadow, &[place], &[target]);
            assert_eq!(verdict, Some(if round + 1 == SHADOWS { Verdict::Back } else { Verdict::Agreed }));
            place = target;
        }
        assert!(!tracker.off);
    }

    #[test]
    fn prediction_applies_to_xfce_and_plasma_5_on_x11_only() {
        assert!(applies(Some(Kind::Xfce), Session::X11, || panic!("Xfce needs no Plasma version")));
        assert!(applies(Some(Kind::Kde), Session::X11, || true));
        assert!(!applies(Some(Kind::Kde), Session::X11, || false), "Plasma 6 saves at the move");
        assert!(!applies(Some(Kind::Kde), Session::Wayland, || true));
        assert!(!applies(Some(Kind::Xfce), Session::Wayland, || true));
        for kind in [Kind::Gnome, Kind::Cinnamon, Kind::Mate] {
            assert!(!applies(Some(kind), Session::X11, || true));
        }
        assert!(!applies(None, Session::X11, || true));
        assert!(off_line("Plasma").contains("on Plasma after a wrong guess, and waits for Plasma to save"));
    }

    #[test]
    fn the_hold_keeps_the_guess_over_reads_from_before_the_drop_until_the_save_or_its_time() {
        let now = Instant::now();
        let before = vec![at(961.0, 506.0), at(18.0, 40.0)];
        let guessed = vec![at(649.0, 506.0), at(18.0, 40.0)];
        let mut hold = Some(Hold::new(&before, guessed.clone(), now + Duration::from_secs(3)));
        let stale = vec![at(18.0, 40.0), at(961.0, 506.0)];
        assert_eq!(through(&mut hold, stale.clone(), now), guessed, "a read in another order is still the stale one");
        assert!(hold.is_some());
        let saved = vec![at(545.0, 506.0), at(18.0, 40.0)];
        assert_eq!(through(&mut hold, saved.clone(), now), saved, "the save brings the truth, right or wrong");
        assert_eq!(hold, None);
        assert_eq!(through(&mut hold, stale.clone(), now), stale, "the hold ends with the save");
        let mut late = Some(Hold::new(&before, guessed, now));
        assert_eq!(through(&mut late, stale.clone(), now + Duration::from_millis(1)), stale);
        assert_eq!(late, None);
        assert_eq!(through(&mut None, stale.clone(), now), stale);
    }

    #[test]
    fn icons_that_change_by_other_ways_make_the_selection_unknown() {
        let mut tracker = Tracker::new();
        let icon = at(961.0, 506.0);
        tracker.seen(&[icon]);
        known_single(&mut tracker, icon);
        tracker.seen(&[icon]);
        assert_eq!(tracker.selection, Selection::One(Some(icon)));
        tracker.seen(&[icon, at(18.0, 40.0)]);
        assert_eq!(tracker.selection, Selection::Unknown);
    }
}
