pub const EXTENSION: &str = "XInputExtension";
pub const QUERY_VERSION: u8 = 47;
pub const SELECT_EVENTS: u8 = 46;
pub const RAW_KEY_PRESS: u16 = 13;
pub const RAW_BUTTON_PRESS: u16 = 15;
pub const RAW_BUTTON_RELEASE: u16 = 16;
pub const RAW_MOTION: u16 = 17;
pub const ALL_MASTER_DEVICES: u16 = 1;
pub const POINTER_EMULATED: u32 = 1 << 16;
pub const PRIMARY_BUTTON: u32 = 1;
pub const GENERIC_EVENT: u8 = 35;
pub const WANTED: (u16, u16) = (2, 2);

fn request(major: u8, minor: u8, body: &[u8]) -> Vec<u8> {
    let length = ((4 + body.len()) / 4) as u16;
    let mut bytes = vec![major, minor];
    bytes.extend_from_slice(&length.to_ne_bytes());
    bytes.extend_from_slice(body);
    bytes
}

pub fn query_version(major: u8) -> Vec<u8> {
    let mut body = WANTED.0.to_ne_bytes().to_vec();
    body.extend_from_slice(&WANTED.1.to_ne_bytes());
    request(major, QUERY_VERSION, &body)
}

pub fn select_releases(major: u8, window: u32) -> Vec<u8> {
    select_raw(major, window, &[RAW_BUTTON_RELEASE])
}

pub fn select_raw(major: u8, window: u32, kinds: &[u16]) -> Vec<u8> {
    let mask = kinds.iter().fold(0u32, |mask, kind| mask | 1 << kind);
    let mut body = window.to_ne_bytes().to_vec();
    body.extend_from_slice(&1u16.to_ne_bytes());
    body.extend_from_slice(&0u16.to_ne_bytes());
    body.extend_from_slice(&ALL_MASTER_DEVICES.to_ne_bytes());
    body.extend_from_slice(&1u16.to_ne_bytes());
    body.extend_from_slice(&mask.to_ne_bytes());
    request(major, SELECT_EVENTS, &body)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Raw {
    Press(u32),
    Release(u32),
    Key,
    Motion,
}

pub fn raw(event: &[u8], major: u8) -> Option<Raw> {
    if event.len() < 28 || event[0] & 0x7f != GENERIC_EVENT || event[1] != major {
        return None;
    }
    let kind = u16::from_ne_bytes([event[8], event[9]]);
    let detail = u32::from_ne_bytes([event[16], event[17], event[18], event[19]]);
    let flags = u32::from_ne_bytes([event[24], event[25], event[26], event[27]]);
    match kind {
        RAW_KEY_PRESS => Some(Raw::Key),
        RAW_MOTION => Some(Raw::Motion),
        RAW_BUTTON_PRESS if flags & POINTER_EMULATED == 0 => Some(Raw::Press(detail)),
        RAW_BUTTON_RELEASE if flags & POINTER_EMULATED == 0 => Some(Raw::Release(detail)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyGate {
    pub active: bool,
    pub held: bool,
    pub tracking: bool,
    pub on: bool,
    pub motion: bool,
}

impl KeyGate {
    pub fn change(&mut self) -> Option<bool> {
        let wanted = self.active || self.held;
        if wanted == self.on && self.tracking == self.motion {
            return None;
        }
        self.on = wanted;
        self.motion = self.tracking;
        Some(wanted)
    }

    pub fn kinds(&self) -> Vec<u16> {
        let mut kinds = vec![RAW_BUTTON_PRESS, RAW_BUTTON_RELEASE];
        kinds.extend(self.on.then_some(RAW_KEY_PRESS));
        kinds.extend(self.motion.then_some(RAW_MOTION));
        kinds
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub viewable: bool,
    pub dnd: bool,
    pub frame: scooch_core::Rect,
}

pub fn topmost(stack: &[Layer], point: (f32, f32)) -> Option<usize> {
    stack.iter().rposition(|layer| layer.viewable && !layer.dnd && super::xfce::contains(&layer.frame, point))
}

pub fn version(reply: &[u8]) -> Option<(u16, u16)> {
    let field = |at: usize| reply.get(at..at + 2).map(|bytes| u16::from_ne_bytes([bytes[0], bytes[1]]));
    Some((field(8)?, field(10)?))
}

pub fn supports_raw_master(version: (u16, u16)) -> bool {
    version >= (2, 1)
}

pub fn primary_release(event: &[u8], major: u8) -> bool {
    if event.len() < 28 || event[0] & 0x7f != GENERIC_EVENT || event[1] != major {
        return false;
    }
    let kind = u16::from_ne_bytes([event[8], event[9]]);
    let detail = u32::from_ne_bytes([event[16], event[17], event[18], event[19]]);
    let flags = u32::from_ne_bytes([event[24], event[25], event[26], event[27]]);
    kind == RAW_BUTTON_RELEASE && detail == PRIMARY_BUTTON && flags & POINTER_EMULATED == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_event(major: u8, kind: u16, detail: u32, flags: u32) -> Vec<u8> {
        let mut event = vec![GENERIC_EVENT, major, 0, 0];
        event.extend_from_slice(&2u32.to_ne_bytes());
        event.extend_from_slice(&kind.to_ne_bytes());
        event.extend_from_slice(&2u16.to_ne_bytes());
        event.extend_from_slice(&0u32.to_ne_bytes());
        event.extend_from_slice(&detail.to_ne_bytes());
        event.extend_from_slice(&4u16.to_ne_bytes());
        event.extend_from_slice(&0u16.to_ne_bytes());
        event.extend_from_slice(&flags.to_ne_bytes());
        event.extend_from_slice(&[0; 12]);
        event
    }

    #[test]
    fn requests_carry_their_length_in_words() {
        let ask = query_version(131);
        assert_eq!(ask.len(), 8);
        assert_eq!((ask[0], ask[1]), (131, QUERY_VERSION));
        assert_eq!(u16::from_ne_bytes([ask[2], ask[3]]), 2);
        let select = select_releases(131, 0x1e5);
        assert_eq!(select.len(), 20);
        assert_eq!(u16::from_ne_bytes([select[2], select[3]]), 5);
        assert_eq!(u32::from_ne_bytes([select[4], select[5], select[6], select[7]]), 0x1e5);
        assert_eq!(u16::from_ne_bytes([select[12], select[13]]), ALL_MASTER_DEVICES);
        assert_eq!(u32::from_ne_bytes([select[16], select[17], select[18], select[19]]), 1 << 16);
    }

    #[test]
    fn only_a_real_left_release_counts() {
        assert!(primary_release(&raw_event(131, RAW_BUTTON_RELEASE, 1, 0), 131));
        assert!(!primary_release(&raw_event(131, RAW_BUTTON_RELEASE, 3, 0), 131));
        assert!(!primary_release(&raw_event(131, RAW_BUTTON_RELEASE, 1, POINTER_EMULATED), 131));
        assert!(!primary_release(&raw_event(131, 15, 1, 0), 131));
        assert!(!primary_release(&raw_event(140, RAW_BUTTON_RELEASE, 1, 0), 131));
        assert!(!primary_release(&raw_event(131, RAW_BUTTON_RELEASE, 1, 0)[..20], 131));
    }

    #[test]
    fn presses_releases_and_keys_are_told_apart() {
        assert_eq!(raw(&raw_event(131, RAW_BUTTON_PRESS, 1, 0), 131), Some(Raw::Press(1)));
        assert_eq!(raw(&raw_event(131, RAW_BUTTON_PRESS, 3, 0), 131), Some(Raw::Press(3)));
        assert_eq!(raw(&raw_event(131, RAW_BUTTON_RELEASE, 1, 0), 131), Some(Raw::Release(1)));
        assert_eq!(raw(&raw_event(131, RAW_KEY_PRESS, 9, 0), 131), Some(Raw::Key));
        assert_eq!(raw(&raw_event(131, RAW_BUTTON_PRESS, 1, POINTER_EMULATED), 131), None);
        assert_eq!(raw(&raw_event(131, 14, 9, 0), 131), None);
        assert_eq!(raw(&raw_event(140, RAW_KEY_PRESS, 9, 0), 131), None);
        let select = select_raw(131, 0x1e5, &[RAW_KEY_PRESS, RAW_BUTTON_PRESS, RAW_BUTTON_RELEASE]);
        assert_eq!(u32::from_ne_bytes([select[16], select[17], select[18], select[19]]), (1 << 13) | (1 << 15) | (1 << 16));
        assert_eq!(select_releases(131, 0x1e5), select_raw(131, 0x1e5, &[RAW_BUTTON_RELEASE]));
    }

    #[test]
    fn a_release_with_a_dnd_window_on_top_of_the_desktop_still_counts_as_over_the_desktop() {
        let frame = |x: f32, y: f32, w: f32, h: f32| scooch_core::Rect { x, y, w, h };
        let desktop = Layer { viewable: true, dnd: false, frame: frame(0.0, 0.0, 1920.0, 1080.0) };
        let hidden = Layer { viewable: false, dnd: false, frame: frame(600.0, 500.0, 400.0, 300.0) };
        let drag_icon = Layer { viewable: true, dnd: true, frame: frame(640.0, 560.0, 64.0, 64.0) };
        let panel = Layer { viewable: true, dnd: false, frame: frame(0.0, 0.0, 1920.0, 27.0) };
        let stack = [desktop, hidden, panel, drag_icon];
        assert_eq!(topmost(&stack, (672.0, 594.0)), Some(0));
        assert_eq!(topmost(&stack, (672.0, 10.0)), Some(2));
        assert_eq!(topmost(&stack[1..], (672.0, 594.0)), None);
    }

    #[test]
    fn keys_are_heard_only_while_the_desktop_is_active_or_a_press_on_it_is_held() {
        let mut gate = KeyGate::default();
        assert_eq!(gate.change(), None);
        assert_eq!(gate.kinds(), vec![RAW_BUTTON_PRESS, RAW_BUTTON_RELEASE]);
        gate.active = true;
        assert_eq!(gate.change(), Some(true));
        assert!(gate.kinds().contains(&RAW_KEY_PRESS));
        assert_eq!(gate.change(), None);
        gate.held = true;
        assert_eq!(gate.change(), None);
        gate.active = false;
        assert_eq!(gate.change(), None, "a drag on the desktop still hears Escape");
        gate.held = false;
        assert_eq!(gate.change(), Some(false));
        assert!(!gate.kinds().contains(&RAW_KEY_PRESS), "typing in another window must wake nothing");
        gate.tracking = true;
        assert_eq!(gate.change(), Some(false));
        assert_eq!(gate.kinds(), vec![RAW_BUTTON_PRESS, RAW_BUTTON_RELEASE, RAW_MOTION], "motion only while a drag may be starting");
        gate.tracking = false;
        assert_eq!(gate.change(), Some(false));
        assert!(!gate.kinds().contains(&RAW_MOTION));
        assert_eq!(raw(&raw_event(131, RAW_MOTION, 0, 0), 131), Some(Raw::Motion));
    }

    #[test]
    fn a_key_event_carries_no_key_out_of_the_parse() {
        let a = raw(&raw_event(131, RAW_KEY_PRESS, 38, 0), 131);
        let b = raw(&raw_event(131, RAW_KEY_PRESS, 9, 0), 131);
        assert_eq!(a, Some(Raw::Key));
        assert_eq!(a, b);
        assert_eq!(format!("{a:?}"), "Some(Key)");
    }

    #[test]
    fn the_reply_gives_the_agreed_version() {
        let mut reply = vec![1, 0, 0, 0, 0, 0, 0, 0];
        reply.extend_from_slice(&2u16.to_ne_bytes());
        reply.extend_from_slice(&2u16.to_ne_bytes());
        reply.extend_from_slice(&[0; 20]);
        assert_eq!(version(&reply), Some((2, 2)));
        assert!(supports_raw_master((2, 2)));
        assert!(!supports_raw_master((2, 0)));
        assert_eq!(version(&reply[..9]), None);
    }
}
