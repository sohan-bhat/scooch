use super::parse::desktop::Session;
use super::parse::geometry;
use crate::platform::Screen;
use anyhow::{anyhow, bail, Context, Result};
use scooch_core::Rect;
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::time::Instant;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::{self, ZxdgOutputV1};
use x11rb::connection::Connection as _;
use x11rb::protocol::randr::{ConnectionExt as _, NotifyMask};
use x11rb::protocol::xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask};
use x11rb::protocol::Event;

pub type Shared = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub screen: Screen,
    pub primary: bool,
}

pub fn read(session: Session) -> Result<Vec<Found>> {
    let found = match session {
        Session::Wayland => wayland(),
        Session::X11 => x11(),
    }?;
    if found.is_empty() {
        bail!("The display reports no screens, so try again once a screen is connected.");
    }
    Ok(found)
}

pub fn x11_within<T: Send + 'static>(what: &'static str, work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("scooch-x11".into()).spawn(move || {
        let _ = sender.send(work());
    })?;
    match receiver.recv_timeout(super::LIMIT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(anyhow::Error::new(super::Silent(format!("The X display did not answer {what} within {} seconds, so check that the desktop still runs.", super::LIMIT.as_secs())))),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!("The X display stopped answering, so try again.")),
    }
}

fn x11() -> Result<Vec<Found>> {
    x11_within("the monitor list", x11_monitors)
}

fn x11_monitors() -> Result<Vec<Found>> {
    let (conn, number) = x11rb::connect(None).map_err(|_| anyhow!("Scooch could not reach the X display, so check that DISPLAY is set."))?;
    let root = conn.setup().roots.get(number).context("The X display has no screen.")?.root;
    conn.randr_query_version(1, 5)?.reply().map_err(|_| anyhow!("The X display has no RandR, so Scooch cannot see its monitors."))?;
    let reply = conn.randr_get_monitors(root, true)?.reply()?;
    let mut found = Vec::new();
    for monitor in reply.monitors {
        let name = conn.get_atom_name(monitor.name)?.reply().map(|reply| String::from_utf8_lossy(&reply.name).into_owned()).unwrap_or_default();
        let frame = Rect { x: monitor.x as f32, y: monitor.y as f32, w: monitor.width as f32, h: monitor.height as f32 };
        found.push(Found { screen: Screen { id: name, frame, scale: 1.0 }, primary: monitor.primary });
    }
    if !found.iter().any(|found| found.primary) {
        if let Some(first) = found.first_mut() {
            first.primary = true;
        }
    }
    Ok(found)
}

pub fn cardinals(name: &str) -> Result<Vec<u32>> {
    let name = name.to_string();
    x11_within("a root property", move || root_cardinals(&name))
}

fn root_cardinals(name: &str) -> Result<Vec<u32>> {
    let (conn, number) = x11rb::connect(None).map_err(|_| anyhow!("Scooch could not reach the X display, so check that DISPLAY is set."))?;
    let root = conn.setup().roots.get(number).context("The X display has no screen.")?.root;
    let atom = conn.intern_atom(true, name.as_bytes())?.reply()?.atom;
    if atom == 0 {
        return Ok(Vec::new());
    }
    let reply = conn.get_property(false, root, atom, AtomEnum::CARDINAL, 0, 4096)?.reply()?;
    Ok(reply.value32().map(|values| values.collect()).unwrap_or_default())
}

#[derive(Default)]
struct Output {
    proxy: Option<wl_output::WlOutput>,
    global: u32,
    name: Option<String>,
    mode: Option<(i32, i32)>,
    transform: u32,
    position: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
    xdg: Option<ZxdgOutputV1>,
}

#[derive(Default)]
struct State {
    outputs: Vec<Output>,
    manager: Option<ZxdgOutputManagerV1>,
    changed: bool,
    synced: bool,
}

impl State {
    fn attach(&mut self, qh: &QueueHandle<Self>) {
        let Some(manager) = self.manager.clone() else { return };
        for (index, output) in self.outputs.iter_mut().enumerate() {
            if output.xdg.is_none() {
                if let Some(proxy) = &output.proxy {
                    output.xdg = Some(manager.get_xdg_output(proxy, qh, index));
                }
            }
        }
    }

    fn found(&self) -> Vec<Found> {
        let mut found = Vec::new();
        for output in &self.outputs {
            let (Some((x, y)), Some((w, h))) = (output.position, output.size) else { continue };
            let scale = output.mode.map_or(1.0, |(mw, mh)| geometry::scale(mw, mh, output.transform, w));
            let frame = Rect { x: x as f32, y: y as f32, w: w as f32, h: h as f32 };
            let id = output.name.clone().unwrap_or_else(|| format!("output-{}", output.global));
            found.push(Found { screen: Screen { id, frame, scale }, primary: false });
        }
        if let Some(index) = found.iter().position(|found| found.screen.frame.x == 0.0 && found.screen.frame.y == 0.0).or(if found.is_empty() { None } else { Some(0) }) {
            found[index].primary = true;
        }
        found
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(state: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        match event {
            wl_registry::Event::Global { name, interface, version } => match interface.as_str() {
                "wl_output" => {
                    let index = state.outputs.len();
                    let proxy = registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, index);
                    state.outputs.push(Output { proxy: Some(proxy), global: name, ..Output::default() });
                    state.changed = true;
                    state.attach(qh);
                }
                "zxdg_output_manager_v1" => {
                    state.manager = Some(registry.bind::<ZxdgOutputManagerV1, _, _>(name, version.min(3), qh, ()));
                    state.attach(qh);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(index) = state.outputs.iter().position(|output| output.global == name) {
                    state.outputs[index] = Output { global: u32::MAX, ..Output::default() };
                    state.changed = true;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, usize> for State {
    fn event(state: &mut Self, _: &wl_output::WlOutput, event: wl_output::Event, index: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let Some(output) = state.outputs.get_mut(*index) else { return };
        match event {
            wl_output::Event::Geometry { transform: WEnum::Value(transform), .. } => output.transform = u32::from(transform),
            wl_output::Event::Mode { flags: WEnum::Value(flags), width, height, .. } if flags.contains(wl_output::Mode::Current) => output.mode = Some((width, height)),
            wl_output::Event::Name { name } => output.name = Some(name),
            wl_output::Event::Done => state.changed = true,
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, usize> for State {
    fn event(state: &mut Self, _: &ZxdgOutputV1, event: zxdg_output_v1::Event, index: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let Some(output) = state.outputs.get_mut(*index) else { return };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => output.position = Some((x, y)),
            zxdg_output_v1::Event::LogicalSize { width, height } => output.size = Some((width, height)),
            zxdg_output_v1::Event::Name { name } => {
                if output.name.is_none() {
                    output.name = Some(name);
                }
            }
            zxdg_output_v1::Event::Done => state.changed = true,
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(State: ignore ZxdgOutputManagerV1);

impl Dispatch<WlCallback, ()> for State {
    fn event(state: &mut Self, _: &WlCallback, event: wl_callback::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_callback::Event::Done { .. } = event {
            state.synced = true;
        }
    }
}

fn stopped() -> anyhow::Error {
    anyhow!("The Wayland display stopped answering, so try again.")
}

fn roundtrip_within(conn: &Connection, queue: &mut EventQueue<State>, state: &mut State, deadline: Instant) -> Result<()> {
    state.synced = false;
    conn.display().sync(&queue.handle(), ());
    loop {
        queue.dispatch_pending(state).map_err(|_| stopped())?;
        if state.synced {
            return Ok(());
        }
        queue.flush().map_err(|_| stopped())?;
        let Some(guard) = queue.prepare_read() else { continue };
        let left = deadline.saturating_duration_since(Instant::now());
        let mut ready = [libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        let found = if left.is_zero() { 0 } else { unsafe { libc::poll(ready.as_mut_ptr(), 1, left.as_millis().min(i32::MAX as u128) as i32) } };
        if found == 0 {
            return Err(anyhow::Error::new(super::Silent(format!("The Wayland display did not answer within {} seconds, so check that the desktop still runs.", super::LIMIT.as_secs()))));
        }
        if found < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(stopped());
        }
        match guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return Err(stopped()),
        }
    }
}

fn open() -> Result<(EventQueue<State>, State)> {
    let conn = Connection::connect_to_env().map_err(|_| anyhow!("Scooch could not reach the Wayland display, so check that WAYLAND_DISPLAY is set."))?;
    open_on(conn)
}

fn open_on(conn: Connection) -> Result<(EventQueue<State>, State)> {
    let _open = super::pending::open("the Wayland roundtrips for the screens".to_string());
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State::default();
    let deadline = Instant::now() + super::LIMIT;
    for _ in 0..3 {
        roundtrip_within(&conn, &mut queue, &mut state, deadline)?;
    }
    if state.manager.is_none() {
        bail!("This Wayland desktop does not share screen positions, so Scooch cannot place the picture.");
    }
    Ok((queue, state))
}

fn wayland() -> Result<Vec<Found>> {
    let (_queue, state) = open()?;
    Ok(state.found())
}

pub fn watch_once(session: Session, wake: &Shared) -> bool {
    match session {
        Session::Wayland => watch_wayland(wake),
        Session::X11 => watch_x11(wake),
    }
}

fn watch_wayland(wake: &Shared) -> bool {
    let Ok((mut queue, mut state)) = open() else { return false };
    state.changed = false;
    while queue.blocking_dispatch(&mut state).is_ok() {
        if std::mem::take(&mut state.changed) {
            wake();
        }
    }
    true
}

fn watch_x11(wake: &Shared) -> bool {
    let armed = x11_within("the screen watch", || {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).map(|screen| screen.root).context("The X display has no screen.")?;
        conn.randr_query_version(1, 5)?.reply()?;
        conn.randr_select_input(root, NotifyMask::SCREEN_CHANGE | NotifyMask::OUTPUT_CHANGE | NotifyMask::CRTC_CHANGE)?;
        conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE))?;
        let workarea = conn.intern_atom(false, b"_NET_WORKAREA")?.reply()?.atom;
        let desktop = conn.intern_atom(false, b"_NET_CURRENT_DESKTOP")?.reply()?.atom;
        let resources = conn.intern_atom(false, b"RESOURCE_MANAGER")?.reply()?.atom;
        conn.flush()?;
        Ok((conn, (workarea, desktop, resources)))
    });
    let Ok((conn, (workarea, desktop, resources))) = armed else { return false };
    while let Ok(event) = conn.wait_for_event() {
        match event {
            Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => wake(),
            Event::PropertyNotify(event) if event.atom == workarea || event.atom == desktop || event.atom == resources => wake(),
            _ => {}
        }
    }
    true
}

pub fn resources() -> Option<String> {
    x11_within("its resources", || root_resources().context("no resources")).ok()
}

fn root_resources() -> Option<String> {
    let (conn, number) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(number)?.root;
    let reply = conn.get_property(false, root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 1 << 20).ok()?.reply().ok()?;
    Some(String::from_utf8_lossy(&reply.value).into_owned())
}

#[cfg(test)]
mod tests {
    use super::open_on;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn a_compositor_that_never_answers_ends_in_plain_words_at_the_limit() {
        let (client, server) = UnixStream::pair().unwrap();
        let conn = wayland_client::Connection::from_socket(client).unwrap();
        let (said, answer) = mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            let _ = said.send(open_on(conn).map(|_| ()).map_err(|error| error.to_string()));
        });
        let reply = answer.recv_timeout(super::super::LIMIT + Duration::from_secs(5)).expect("the roundtrip still waited 5 s past the limit");
        drop(server);
        assert!(started.elapsed() >= super::super::LIMIT - Duration::from_millis(500), "the roundtrip ended before the limit {:?}", started.elapsed());
        assert_eq!(reply, Err("The Wayland display did not answer within 10 seconds, so check that the desktop still runs.".to_string()));
    }
}
