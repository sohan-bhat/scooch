use super::parse::xinput;
use super::screens::Shared;
use std::io::IoSlice;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::errors::ParseError;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::x11_utils::TryParse;

const DEPTH: usize = 8;

struct Raw(Vec<u8>);

impl TryParse for Raw {
    fn try_parse(value: &[u8]) -> Result<(Self, &[u8]), ParseError> {
        Ok((Raw(value.to_vec()), &[]))
    }
}

struct Atoms {
    kind: Atom,
    desktop: Atom,
}

enum Setup {
    Ready(Box<RustConnection>, Window, u8, Atoms),
    Absent,
    Lost,
}

fn atom(conn: &RustConnection, name: &[u8]) -> Option<Atom> {
    conn.intern_atom(false, name).ok()?.reply().ok().map(|reply| reply.atom)
}

fn is_desktop(conn: &RustConnection, window: Window, atoms: &Atoms) -> bool {
    conn.get_property(false, window, atoms.kind, AtomEnum::ATOM, 0, 16)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| reply.value32().is_some_and(|mut values| values.any(|value| value == atoms.desktop)))
}

fn over_desktop(conn: &RustConnection, root: Window, atoms: &Atoms) -> bool {
    let mut window = root;
    for _ in 0..DEPTH {
        let Some(child) = conn.query_pointer(window).ok().and_then(|cookie| cookie.reply().ok()).map(|reply| reply.child) else { return false };
        if child == x11rb::NONE {
            return false;
        }
        if is_desktop(conn, child, atoms) {
            return true;
        }
        window = child;
    }
    false
}

fn setup() -> Setup {
    let Ok((conn, number)) = x11rb::connect(None) else { return Setup::Lost };
    let Some(root) = conn.setup().roots.get(number).map(|screen| screen.root) else { return Setup::Absent };
    let Some(major) = conn.extension_information(xinput::EXTENSION).ok().flatten().map(|info| info.major_opcode) else { return Setup::Absent };
    let ask = xinput::query_version(major);
    let agreed = conn.send_request_with_reply::<Raw>(&[IoSlice::new(&ask)], Vec::new()).ok().and_then(|cookie| cookie.reply().ok()).and_then(|raw| xinput::version(&raw.0));
    if !agreed.is_some_and(xinput::supports_raw_master) {
        return Setup::Absent;
    }
    let select = xinput::select_releases(major, root);
    if conn.send_request_without_reply(&[IoSlice::new(&select)], Vec::new()).ok().and_then(|cookie| cookie.check().ok()).is_none() {
        return Setup::Absent;
    }
    let (Some(kind), Some(desktop)) = (atom(&conn, b"_NET_WM_WINDOW_TYPE"), atom(&conn, b"_NET_WM_WINDOW_TYPE_DESKTOP")) else { return Setup::Absent };
    Setup::Ready(Box::new(conn), root, major, Atoms { kind, desktop })
}

pub fn watch(wake: &Shared, desktop_only: bool) -> bool {
    let (conn, root, major, atoms) = match setup() {
        Setup::Ready(conn, root, major, atoms) => (conn, root, major, atoms),
        Setup::Absent => loop {
            std::thread::park();
        },
        Setup::Lost => return false,
    };
    loop {
        let Ok(event) = conn.wait_for_event() else { return false };
        let Event::Unknown(raw) = event else { continue };
        if xinput::primary_release(&raw, major) && (!desktop_only || over_desktop(&conn, root, &atoms)) {
            wake();
        }
    }
}
