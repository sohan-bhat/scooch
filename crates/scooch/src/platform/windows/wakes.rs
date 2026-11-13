pub const SPI_ICONHORIZONTALSPACING: u32 = 0x000D;
pub const SPI_SETDESKWALLPAPER: u32 = 0x0014;
pub const SPI_SETICONTITLELOGFONT: u32 = 0x0022;
pub const SPI_SETICONMETRICS: u32 = 0x002E;
pub const SPI_SETWORKAREA: u32 = 0x002F;

pub const EVENT_SYSTEM_CAPTUREEND: u32 = 0x0009;
pub const EVENT_OBJECT_CREATE: u32 = 0x8000;
pub const EVENT_OBJECT_DESTROY: u32 = 0x8001;
pub const EVENT_OBJECT_SHOW: u32 = 0x8002;
pub const EVENT_OBJECT_HIDE: u32 = 0x8003;
pub const EVENT_OBJECT_REORDER: u32 = 0x8004;
pub const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
pub const OBJID_WINDOW: i32 = 0;
pub const OBJID_CLIENT: i32 = -4;

pub const WTS_SESSION_LOGON: usize = 0x5;
pub const WTS_SESSION_UNLOCK: usize = 0x8;
pub const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

pub const TRIES: u32 = 10;
pub const OWN_ECHO_MS: u64 = 1000;

pub const HOOKS: [(u32, u32); 4] = [
    (EVENT_SYSTEM_CAPTUREEND, EVENT_SYSTEM_CAPTUREEND),
    (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
    (EVENT_OBJECT_REORDER, EVENT_OBJECT_REORDER),
    (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
];

pub fn setting(action: usize, own_echo: bool) -> bool {
    match u32::try_from(action) {
        Ok(SPI_SETDESKWALLPAPER) => !own_echo,
        Ok(SPI_SETWORKAREA | SPI_SETICONMETRICS | SPI_SETICONTITLELOGFONT | SPI_ICONHORIZONTALSPACING) => true,
        _ => false,
    }
}

pub fn event(kind: u32, on_list: bool, on_view: bool, object: i32, child: i32) -> bool {
    match kind {
        EVENT_SYSTEM_CAPTUREEND => true,
        EVENT_OBJECT_CREATE | EVENT_OBJECT_DESTROY | EVENT_OBJECT_REORDER => on_list && object == OBJID_CLIENT,
        EVENT_OBJECT_SHOW | EVENT_OBJECT_HIDE => (on_list || on_view) && object == OBJID_WINDOW && child == 0,
        EVENT_OBJECT_LOCATIONCHANGE => on_list && object == OBJID_CLIENT && child > 0,
        _ => false,
    }
}

pub fn session(change: usize) -> bool {
    change == WTS_SESSION_UNLOCK || change == WTS_SESSION_LOGON
}

pub fn power(change: usize) -> bool {
    change == PBT_APMRESUMEAUTOMATIC
}

pub fn echo(now: u64, last: u64) -> bool {
    last != 0 && now.saturating_sub(last) < OWN_ECHO_MS
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Retry {
    tries: u32,
    running: bool,
}

impl Retry {
    pub const fn new() -> Self {
        Self { tries: 0, running: false }
    }

    pub fn begin(&mut self) -> bool {
        self.tries = 0;
        !std::mem::replace(&mut self.running, true)
    }

    pub fn next(&mut self, attached: bool) -> bool {
        if attached || self.tries >= TRIES {
            self.running = false;
            return false;
        }
        self.tries += 1;
        true
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    pub fn running(&self) -> bool {
        self.running
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_wallpaper_change_is_ignored_for_a_moment() {
        assert!(setting(SPI_SETDESKWALLPAPER as usize, false));
        assert!(!setting(SPI_SETDESKWALLPAPER as usize, true));
        assert!(setting(SPI_SETWORKAREA as usize, true));
    }

    #[test]
    fn only_desktop_settings_wake() {
        for action in [SPI_SETWORKAREA, SPI_SETICONMETRICS, SPI_SETICONTITLELOGFONT, SPI_ICONHORIZONTALSPACING] {
            assert!(setting(action as usize, false));
        }
        assert!(!setting(0x0071, false));
        assert!(!setting(0, false));
        assert!(!setting(usize::MAX, false));
    }

    #[test]
    fn list_events_wake_only_for_the_desktop_list() {
        assert!(event(EVENT_OBJECT_CREATE, true, false, OBJID_CLIENT, 3));
        assert!(event(EVENT_OBJECT_DESTROY, true, false, OBJID_CLIENT, 0));
        assert!(event(EVENT_OBJECT_REORDER, true, false, OBJID_CLIENT, 0));
        assert!(!event(EVENT_OBJECT_CREATE, false, true, OBJID_CLIENT, 3));
        assert!(!event(EVENT_OBJECT_REORDER, true, false, 0, 0));
    }

    #[test]
    fn a_location_change_needs_an_item() {
        assert!(event(EVENT_OBJECT_LOCATIONCHANGE, true, false, OBJID_CLIENT, 1));
        assert!(!event(EVENT_OBJECT_LOCATIONCHANGE, true, false, OBJID_CLIENT, 0));
        assert!(!event(EVENT_OBJECT_LOCATIONCHANGE, true, false, -9, 1));
    }

    #[test]
    fn showing_or_hiding_the_icons_wakes() {
        for kind in [EVENT_OBJECT_SHOW, EVENT_OBJECT_HIDE] {
            assert!(event(kind, true, false, OBJID_WINDOW, 0));
            assert!(event(kind, false, true, OBJID_WINDOW, 0));
            assert!(!event(kind, false, false, OBJID_WINDOW, 0));
            assert!(!event(kind, true, false, OBJID_CLIENT, 4));
            assert!(!event(kind, true, false, OBJID_WINDOW, 2));
        }
    }

    #[test]
    fn a_drop_ends_capture_anywhere_on_the_desktop_thread() {
        assert!(event(EVENT_SYSTEM_CAPTUREEND, false, false, 0, 0));
        assert!(!event(0x8005, true, true, OBJID_CLIENT, 1));
    }

    #[test]
    fn unlock_logon_and_resume_wake() {
        assert!(session(WTS_SESSION_UNLOCK) && session(WTS_SESSION_LOGON));
        assert!(!session(0x7));
        assert!(power(PBT_APMRESUMEAUTOMATIC) && !power(0x4));
    }

    #[test]
    fn the_hooks_cover_each_event_once() {
        let covered: Vec<u32> = HOOKS.iter().flat_map(|(low, high)| *low..=*high).collect();
        assert_eq!(
            covered,
            vec![EVENT_SYSTEM_CAPTUREEND, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_SHOW, EVENT_OBJECT_HIDE, EVENT_OBJECT_REORDER, EVENT_OBJECT_LOCATIONCHANGE]
        );
    }

    #[test]
    fn the_echo_window_runs_on_its_own_clock() {
        assert!(!echo(0, 0));
        assert!(!echo(5_000, 0));
        assert!(echo(6, 5));
        assert!(echo(1_004, 5));
        assert!(!echo(1_005, 5));
        assert!(echo(3, 5));
    }

    #[test]
    fn ten_failures_make_ten_tries_and_no_eleventh() {
        let mut retry = Retry::new();
        assert!(retry.begin());
        let mut tries = 0;
        while retry.next(false) {
            tries += 1;
            assert!(tries <= TRIES);
        }
        assert_eq!(tries, TRIES);
        assert!(!retry.running());
        assert!(!retry.next(false));
        assert!(!retry.next(false));
    }

    #[test]
    fn finding_the_desktop_ends_the_run() {
        let mut retry = Retry::new();
        retry.begin();
        assert!(retry.next(false));
        assert!(!retry.next(true));
        assert!(!retry.running());
    }

    #[test]
    fn a_second_start_extends_the_run_without_a_second_thread() {
        let mut retry = Retry::new();
        assert!(retry.begin());
        for _ in 0..7 {
            assert!(retry.next(false));
        }
        assert!(!retry.begin());
        let mut tries = 0;
        while retry.next(false) {
            tries += 1;
        }
        assert_eq!(tries, TRIES);
        assert!(retry.begin());
    }
}
