use super::parse::xfce::contains;
use super::parse::xinput::{self, Layer, Raw};
use super::screens::Shared;
use scooch_core::Rect;
use std::io::IoSlice;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::errors::ParseError;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, KeyButMask, MapState, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::x11_utils::TryParse;

const DEPTH: usize = 8;

struct Raw32(Vec<u8>);

impl TryParse for Raw32 {
    fn try_parse(value: &[u8]) -> Result<(Self, &[u8]), ParseError> {
        Ok((Raw32(value.to_vec()), &[]))
    }
}

struct Atoms {
    kind: Atom,
    desktop: Atom,
    dnd: Atom,
    active: Atom,
}

enum Setup {
    Ready(Box<RustConnection>, Window, u8, Atoms),
    Absent,
    Lost,
}

fn atom(conn: &RustConnection, name: &[u8]) -> Option<Atom> {
    conn.intern_atom(false, name).ok()?.reply().ok().map(|reply| reply.atom)
}

fn types(conn: &RustConnection, window: Window, kind: Atom) -> Option<Vec<u32>> {
    let reply = conn.get_property(false, window, kind, AtomEnum::ATOM, 0, 16).ok()?.reply().ok()?;
    Some(reply.value32().map(|values| values.collect()).unwrap_or_default())
}

fn has_type(conn: &RustConnection, window: Window, kind: Atom, wanted: Atom) -> bool {
    types(conn, window, kind).is_some_and(|types| types.contains(&wanted))
}

fn stack(conn: &RustConnection, root: Window, point: (f32, f32), atoms: &Atoms) -> Option<(Vec<Window>, Vec<Layer>)> {
    let children = conn.query_tree(root).ok()?.reply().ok()?.children;
    let cookies: Vec<_> = children.iter().map(|child| (conn.get_window_attributes(*child).ok(), conn.get_geometry(*child).ok())).collect();
    let mut layers = Vec::with_capacity(children.len());
    for (attributes, geometry) in cookies {
        let viewable = attributes.and_then(|cookie| cookie.reply().ok()).is_some_and(|reply| reply.map_state == MapState::VIEWABLE);
        let frame = geometry.and_then(|cookie| cookie.reply().ok()).map_or(Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, |reply| {
            let border = 2.0 * f32::from(reply.border_width);
            Rect { x: f32::from(reply.x), y: f32::from(reply.y), w: f32::from(reply.width) + border, h: f32::from(reply.height) + border }
        });
        layers.push(Layer { viewable, dnd: false, frame });
    }
    for (layer, child) in layers.iter_mut().zip(&children) {
        if layer.viewable && contains(&layer.frame, point) {
            layer.dnd = types(conn, *child, atoms.kind).is_none_or(|types| types.contains(&atoms.dnd));
        }
    }
    Some((children, layers))
}

fn over_desktop(conn: &RustConnection, root: Window, atoms: &Atoms) -> bool {
    let Some(pointer) = conn.query_pointer(root).ok().and_then(|cookie| cookie.reply().ok()) else { return false };
    let point = (f32::from(pointer.root_x), f32::from(pointer.root_y));
    let Some((children, layers)) = stack(conn, root, point, atoms) else { return false };
    let Some(top) = xinput::topmost(&layers, point).and_then(|index| children.get(index).copied()) else { return false };
    if has_type(conn, top, atoms.kind, atoms.desktop) {
        return true;
    }
    let mut window = top;
    for _ in 0..DEPTH {
        let Some(child) = conn.query_pointer(window).ok().and_then(|cookie| cookie.reply().ok()).map(|reply| reply.child) else { return false };
        if child == x11rb::NONE {
            return false;
        }
        if has_type(conn, child, atoms.kind, atoms.desktop) {
            return true;
        }
        window = child;
    }
    false
}

fn active_is_desktop(conn: &RustConnection, root: Window, atoms: &Atoms) -> bool {
    let active = conn
        .get_property(false, root, atoms.active, AtomEnum::WINDOW, 0, 1)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().and_then(|mut values| values.next()));
    active.is_some_and(|window| window != x11rb::NONE && has_type(conn, window, atoms.kind, atoms.desktop))
}

fn setup(kinds: &[u16]) -> Setup {
    let Ok((conn, number)) = x11rb::connect(None) else { return Setup::Lost };
    let Some(root) = conn.setup().roots.get(number).map(|screen| screen.root) else { return Setup::Absent };
    let Some(major) = conn.extension_information(xinput::EXTENSION).ok().flatten().map(|info| info.major_opcode) else { return Setup::Absent };
    let ask = xinput::query_version(major);
    let agreed = conn.send_request_with_reply::<Raw32>(&[IoSlice::new(&ask)], Vec::new()).ok().and_then(|cookie| cookie.reply().ok()).and_then(|raw| xinput::version(&raw.0));
    if !agreed.is_some_and(xinput::supports_raw_master) {
        return Setup::Absent;
    }
    let select = xinput::select_raw(major, root, kinds);
    if conn.send_request_without_reply(&[IoSlice::new(&select)], Vec::new()).ok().and_then(|cookie| cookie.check().ok()).is_none() {
        return Setup::Absent;
    }
    let names = (atom(&conn, b"_NET_WM_WINDOW_TYPE"), atom(&conn, b"_NET_WM_WINDOW_TYPE_DESKTOP"), atom(&conn, b"_NET_WM_WINDOW_TYPE_DND"), atom(&conn, b"_NET_ACTIVE_WINDOW"));
    let (Some(kind), Some(desktop), Some(dnd), Some(active)) = names else { return Setup::Absent };
    Setup::Ready(Box::new(conn), root, major, Atoms { kind, desktop, dnd, active })
}

pub fn watch(wake: &Shared, desktop_only: bool) -> bool {
    let (conn, root, major, atoms) = match setup(&[xinput::RAW_BUTTON_RELEASE]) {
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Press { button: u32, at: (f32, f32), control: bool, shift: bool, desktop: bool },
    Release { button: u32, at: (f32, f32), desktop: bool },
    Key { desktop_active: bool },
}

pub fn inputs(handle: &mut dyn FnMut(Input)) -> bool {
    let (conn, root, major, atoms) = match setup(&[xinput::RAW_KEY_PRESS, xinput::RAW_BUTTON_PRESS, xinput::RAW_BUTTON_RELEASE]) {
        Setup::Ready(conn, root, major, atoms) => (conn, root, major, atoms),
        Setup::Absent => return true,
        Setup::Lost => return false,
    };
    loop {
        let Ok(event) = conn.wait_for_event() else { return false };
        let Event::Unknown(raw) = event else { continue };
        let Some(kind) = xinput::raw(&raw, major) else { continue };
        let input = match kind {
            Raw::Key => Input::Key { desktop_active: active_is_desktop(&conn, root, &atoms) },
            Raw::Press(button) | Raw::Release(button) => {
                let Some(pointer) = conn.query_pointer(root).ok().and_then(|cookie| cookie.reply().ok()) else { continue };
                let at = (f32::from(pointer.root_x), f32::from(pointer.root_y));
                let desktop = over_desktop(&conn, root, &atoms);
                let mask = u16::from(pointer.mask);
                if matches!(kind, Raw::Press(_)) {
                    Input::Press { button, at, control: mask & u16::from(KeyButMask::CONTROL) != 0, shift: mask & u16::from(KeyButMask::SHIFT) != 0, desktop }
                } else {
                    Input::Release { button, at, desktop }
                }
            }
        };
        handle(input);
    }
}
