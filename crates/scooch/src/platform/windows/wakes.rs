pub const SPI_ICONHORIZONTALSPACING: u32 = 0x000D;
pub const SPI_SETDESKWALLPAPER: u32 = 0x0014;
pub const SPI_SETICONTITLELOGFONT: u32 = 0x0022;
pub const SPI_SETICONMETRICS: u32 = 0x002E;
pub const SPI_SETWORKAREA: u32 = 0x002F;

pub const EVENT_SYSTEM_CAPTUREEND: u32 = 0x0009;
pub const EVENT_OBJECT_CREATE: u32 = 0x8000;
pub const EVENT_OBJECT_DESTROY: u32 = 0x8001;
pub const EVENT_OBJECT_REORDER: u32 = 0x8004;
pub const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
pub const OBJID_CLIENT: i32 = -4;

pub const WTS_SESSION_LOGON: usize = 0x5;
pub const WTS_SESSION_UNLOCK: usize = 0x8;
pub const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

pub const HOOKS: [(u32, u32); 4] = [
    (EVENT_SYSTEM_CAPTUREEND, EVENT_SYSTEM_CAPTUREEND),
    (EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY),
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

pub fn event(kind: u32, on_list: bool, object: i32, child: i32) -> bool {
    match kind {
        EVENT_SYSTEM_CAPTUREEND => true,
        EVENT_OBJECT_CREATE | EVENT_OBJECT_DESTROY | EVENT_OBJECT_REORDER => on_list && object == OBJID_CLIENT,
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
        assert!(event(EVENT_OBJECT_CREATE, true, OBJID_CLIENT, 3));
        assert!(event(EVENT_OBJECT_DESTROY, true, OBJID_CLIENT, 0));
        assert!(event(EVENT_OBJECT_REORDER, true, OBJID_CLIENT, 0));
        assert!(!event(EVENT_OBJECT_CREATE, false, OBJID_CLIENT, 3));
        assert!(!event(EVENT_OBJECT_REORDER, true, 0, 0));
    }

    #[test]
    fn a_location_change_needs_an_item() {
        assert!(event(EVENT_OBJECT_LOCATIONCHANGE, true, OBJID_CLIENT, 1));
        assert!(!event(EVENT_OBJECT_LOCATIONCHANGE, true, OBJID_CLIENT, 0));
        assert!(!event(EVENT_OBJECT_LOCATIONCHANGE, true, -9, 1));
    }

    #[test]
    fn a_drop_ends_capture_anywhere_on_the_desktop_thread() {
        assert!(event(EVENT_SYSTEM_CAPTUREEND, false, 0, 0));
        assert!(!event(0x8005, true, OBJID_CLIENT, 1));
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
        assert_eq!(covered, vec![EVENT_SYSTEM_CAPTUREEND, EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_REORDER, EVENT_OBJECT_LOCATIONCHANGE]);
    }
}
