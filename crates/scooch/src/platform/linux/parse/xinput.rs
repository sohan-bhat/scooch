pub const EXTENSION: &str = "XInputExtension";
pub const QUERY_VERSION: u8 = 47;
pub const SELECT_EVENTS: u8 = 46;
pub const RAW_BUTTON_RELEASE: u16 = 16;
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
    let mut body = window.to_ne_bytes().to_vec();
    body.extend_from_slice(&1u16.to_ne_bytes());
    body.extend_from_slice(&0u16.to_ne_bytes());
    body.extend_from_slice(&ALL_MASTER_DEVICES.to_ne_bytes());
    body.extend_from_slice(&1u16.to_ne_bytes());
    body.extend_from_slice(&(1u32 << RAW_BUTTON_RELEASE).to_ne_bytes());
    request(major, SELECT_EVENTS, &body)
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
