pub mod children;
pub mod parse;
pub mod saves;

pub use children::child_env;

#[derive(Debug)]
pub struct Silent(pub String);

impl std::fmt::Display for Silent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Silent {}

pub fn silent(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<Silent>())
}

#[cfg(target_os = "linux")]
pub mod autostart;
#[cfg(target_os = "linux")]
pub mod cinnamon;
#[cfg(target_os = "linux")]
pub mod drops;
#[cfg(target_os = "linux")]
pub mod gnome;
#[cfg(target_os = "linux")]
pub mod gvfs;
#[cfg(target_os = "linux")]
pub mod kde;
#[cfg(target_os = "linux")]
pub mod mate;
#[cfg(target_os = "linux")]
pub mod pointer;
#[cfg(target_os = "linux")]
pub mod screens;
#[cfg(target_os = "linux")]
pub mod settings;
#[cfg(target_os = "linux")]
pub mod watch;
#[cfg(target_os = "linux")]
pub mod xfce;

#[cfg(target_os = "linux")]
pub use system::{bus, decode, desktop, desktop_folder, pending, set_recently, shell, tool, Linux, Shell, Signal, Watches, FileWatch, LIMIT};

#[cfg(target_os = "linux")]
mod system {
    use super::parse::desktop::{self as kinds, Kind, Session};
    use super::{autostart, cinnamon, gnome, kde, mate, screens, watch, xfce};
    use crate::platform::{Desktop, Format, Screen, Snapshot, Wake};
    use anyhow::{anyhow, bail, Context, Result};
    use scooch_core::{Rect, RgbImage};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    static LAST_SET: AtomicU64 = AtomicU64::new(0);
    const OWN_ECHO: Duration = Duration::from_secs(1);
    pub const LIMIT: Duration = Duration::from_secs(10);

    fn now_millis() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
    }

    pub fn set_recently() -> bool {
        now_millis().saturating_sub(LAST_SET.load(Ordering::SeqCst)) < OWN_ECHO.as_millis() as u64
    }

    #[derive(Clone, Debug, PartialEq)]
    pub enum Signal {
        Metadata(Option<String>),
        Dconf(&'static [&'static str]),
        Ding,
        Xfconf,
    }

    #[derive(Clone, Debug)]
    pub struct FileWatch {
        pub folder: PathBuf,
        pub matches: fn(&str) -> bool,
        pub wallpaper: bool,
    }

    #[derive(Clone, Debug, Default)]
    pub struct Watches {
        pub files: Vec<FileWatch>,
        pub signals: Vec<Signal>,
        pub pointer: bool,
    }

    pub trait Shell: Send + Sync {
        fn icons(&self, screens: &[Screen]) -> Result<Vec<Rect>>;
        fn primary(&self, _screens: &[Screen]) -> Option<usize> {
            None
        }
        fn x11_scale(&self) -> f32 {
            1.0
        }
        fn adjust(&self, session: Session, found: &mut Vec<screens::Found>) {
            if session != Session::X11 {
                return;
            }
            let factor = self.x11_scale();
            for each in found.iter_mut() {
                each.screen = super::parse::geometry::scaled(&each.screen, factor);
            }
        }
        fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>>;
        fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()>;
        fn watches(&self) -> Watches;
        fn per_screen(&self) -> bool;
        fn tools(&self) -> &'static [&'static str];
    }

    pub mod pending {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::{Mutex, MutexGuard};
        use std::time::{Duration, Instant};

        static NEXT: AtomicU64 = AtomicU64::new(0);
        static OPEN: Mutex<Vec<(u64, String, Instant)>> = Mutex::new(Vec::new());

        pub struct Open(u64);

        fn list() -> MutexGuard<'static, Vec<(u64, String, Instant)>> {
            OPEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        pub fn open(what: String) -> Open {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            list().push((id, what, Instant::now()));
            Open(id)
        }

        impl Drop for Open {
            fn drop(&mut self) {
                list().retain(|(id, _, _)| *id != self.0);
            }
        }

        pub fn now() -> Vec<(String, Duration)> {
            list().iter().map(|(_, what, since)| (what.clone(), since.elapsed())).collect()
        }
    }

    pub mod tool {
        use anyhow::{anyhow, Result};
        use std::ffi::{OsStr, OsString};
        use std::io::Read;
        use std::path::PathBuf;
        use std::process::{Child, Command, Output, Stdio};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        pub use super::LIMIT;

        pub fn run(program: &str, arguments: &[&str]) -> Result<Output> {
            let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
            run_os(program, &arguments)
        }

        pub fn run_os(program: &str, arguments: &[OsString]) -> Result<Output> {
            let named: Vec<String> = arguments.iter().map(|argument| argument.to_string_lossy().into_owned()).collect();
            let _open = super::pending::open(format!("the program {program} {}", named.join(" ").chars().take(160).collect::<String>()));
            let mut command = Command::new(program);
            super::super::children::clean(&mut command).args(arguments).env("LC_ALL", "C.UTF-8").env_remove("LANGUAGE");
            match within(&mut command, LIMIT) {
                Ok(Some(output)) => Ok(output),
                Ok(None) => Err(anyhow::Error::new(super::super::Silent(format!("{program} did not finish within {} seconds, so check that the desktop still runs.", LIMIT.as_secs())))),
                Err(_) => Err(anyhow!("Scooch could not run {program}, so install it or check your PATH.")),
            }
        }

        pub fn within(command: &mut Command, limit: Duration) -> std::io::Result<Option<Output>> {
            let deadline = Instant::now() + limit;
            let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
            let (sender, receiver) = mpsc::channel::<(usize, Vec<u8>)>();
            let pipes: [Option<Box<dyn Read + Send>>; 2] = [child.stdout.take().map(|pipe| Box::new(pipe) as Box<dyn Read + Send>), child.stderr.take().map(|pipe| Box::new(pipe) as Box<dyn Read + Send>)];
            for (index, pipe) in pipes.into_iter().enumerate() {
                let sender = sender.clone();
                let Some(mut pipe) = pipe else {
                    let _ = sender.send((index, Vec::new()));
                    continue;
                };
                let reader = std::thread::Builder::new().name("scooch-tool".into()).spawn(move || {
                    let mut bytes = Vec::new();
                    let _ = pipe.read_to_end(&mut bytes);
                    let _ = sender.send((index, bytes));
                });
                if let Err(error) = reader {
                    stop(child);
                    return Err(error);
                }
            }
            let mut read = [Vec::new(), Vec::new()];
            for _ in 0..2 {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok((index, bytes)) => read[index] = bytes,
                    Err(_) => {
                        stop(child);
                        return Ok(None);
                    }
                }
            }
            loop {
                if let Some(status) = child.try_wait()? {
                    let [stdout, stderr] = read;
                    return Ok(Some(Output { status, stdout, stderr }));
                }
                if Instant::now() >= deadline {
                    stop(child);
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        fn stop(mut child: Child) {
            let _ = child.kill();
            let _ = child.wait();
        }

        pub fn gtk_scale(schema: &str, key: &str) -> f32 {
            let setting = super::super::settings::scale_setting(schema, key);
            let dpi = super::super::screens::resources().and_then(|text| super::super::parse::geometry::xft_dpi(&text));
            super::super::parse::geometry::x11_scale(setting, dpi)
        }

        pub fn on_path(program: &str) -> Option<PathBuf> {
            let paths = std::env::var_os("PATH")?;
            std::env::split_paths(&paths).map(|folder| folder.join(OsStr::new(program))).find(|candidate| candidate.is_file())
        }
    }

    pub mod bus {
        use anyhow::{anyhow, Result};
        use serde::de::DeserializeOwned;
        use serde::Serialize;
        use std::io::ErrorKind;
        use std::sync::Mutex;
        use zbus::blocking::connection::Builder;
        use zbus::blocking::Connection;
        use zbus::zvariant::{DynamicType, Type};

        pub use super::LIMIT;

        static SESSION: Mutex<Option<Connection>> = Mutex::new(None);

        pub fn connect(address: Option<&str>) -> zbus::Result<Connection> {
            let builder = match address {
                Some(address) => Builder::address(address)?,
                None => Builder::session()?,
            };
            builder.method_timeout(LIMIT).build()
        }

        pub fn session() -> Result<Connection> {
            let mut slot = SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(connection) = slot.as_ref() {
                return Ok(connection.clone());
            }
            let connection = connect(None).map_err(|_| anyhow!("Scooch could not reach the session bus, so log in to a desktop session first."))?;
            *slot = Some(connection.clone());
            Ok(connection)
        }

        pub fn forget() {
            *SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        }

        pub fn late(error: &zbus::Error) -> bool {
            matches!(error, zbus::Error::InputOutput(io) if io.kind() == ErrorKind::TimedOut)
        }

        fn raw<B>(connection: &Connection, destination: &str, path: &str, interface: &str, method: &str, body: &B) -> zbus::Result<zbus::Message>
        where
            B: Serialize + DynamicType,
        {
            let _open = super::pending::open(format!("the bus call {interface}.{method} to {destination}"));
            connection.call_method(Some(destination), path, Some(interface), method, body)
        }

        fn finish<R>(reply: zbus::Result<zbus::Message>, destination: &str, method: &str) -> Result<R>
        where
            R: DeserializeOwned + Type,
        {
            match reply {
                Ok(reply) => Ok(reply.body().deserialize()?),
                Err(error) if late(&error) => Err(anyhow::Error::new(super::super::Silent(format!("{destination} did not answer {method} within {} seconds, so check that the desktop still runs.", LIMIT.as_secs())))),
                Err(error) => Err(error.into()),
            }
        }

        pub fn call_on<B, R>(connection: &Connection, destination: &str, path: &str, interface: &str, method: &str, body: &B) -> Result<R>
        where
            B: Serialize + DynamicType,
            R: DeserializeOwned + Type,
        {
            finish(raw(connection, destination, path, interface, method, body), destination, method)
        }

        pub fn call<B, R>(destination: &str, path: &str, interface: &str, method: &str, body: &B) -> Result<R>
        where
            B: Serialize + DynamicType,
            R: DeserializeOwned + Type,
        {
            let reply = match raw(&session()?, destination, path, interface, method, body) {
                Err(zbus::Error::InputOutput(io)) if io.kind() != ErrorKind::TimedOut => {
                    forget();
                    raw(&session()?, destination, path, interface, method, body)
                }
                other => other,
            };
            finish(reply, destination, method)
        }

        pub fn has_owner(name: &str) -> bool {
            call::<_, bool>("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner", &(name,)).unwrap_or(false)
        }
    }

    pub fn desktop_folder() -> PathBuf {
        let asked = tool::run("xdg-user-dir", &["DESKTOP"]).ok().filter(|output| output.status.success()).map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()));
        asked.filter(|path| path.is_absolute()).or_else(dirs::desktop_dir).unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Desktop"))
    }

    pub fn shell(kind: Kind, session: Session, desktop: &Path) -> Result<Box<dyn Shell>> {
        let home = dirs::home_dir().context("Scooch could not find your home folder.")?;
        let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        Ok(match kind {
            Kind::Gnome => Box::new(gnome::Gnome { desktop: desktop.to_path_buf(), home, x11: session == Session::X11 }),
            Kind::Kde => Box::new(kde::Kde { desktop: desktop.to_path_buf(), config, session }),
            Kind::Xfce => Box::new(xfce::Xfce { desktop: desktop.to_path_buf(), home, config, session }),
            Kind::Cinnamon => Box::new(cinnamon::Cinnamon { desktop: desktop.to_path_buf(), home, config, session }),
            Kind::Mate => Box::new(mate::Mate { desktop: desktop.to_path_buf(), home, config, session }),
        })
    }

    pub struct Linux {
        pub kind: Kind,
        pub session: Session,
        pub desktop: PathBuf,
        pub shell: Box<dyn Shell>,
    }

    pub fn desktop() -> Result<Box<dyn Desktop>> {
        Ok(Box::new(Linux::open()?))
    }

    impl Linux {
        pub fn open() -> Result<Linux> {
            let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
            let kind = kinds::pick(&current).ok_or_else(|| anyhow!(kinds::UNKNOWN))?;
            let session = kinds::session(std::env::var("WAYLAND_DISPLAY").ok().as_deref());
            let desktop = desktop_folder();
            let shell = shell(kind, session, &desktop)?;
            Ok(Linux { kind, session, desktop, shell })
        }

        pub fn all_screens(&self) -> Result<Vec<screens::Found>> {
            let mut found = screens::read(self.session)?;
            self.shell.adjust(self.session, &mut found);
            Ok(found)
        }

        pub fn pick(&self, found: Vec<screens::Found>) -> Vec<Screen> {
            if self.shell.per_screen() {
                return found.into_iter().map(|found| found.screen).collect();
            }
            let all: Vec<Screen> = found.iter().map(|found| found.screen.clone()).collect();
            let index = self.shell.primary(&all).or_else(|| found.iter().position(|found| found.primary)).unwrap_or(0);
            all.into_iter().nth(index).into_iter().collect()
        }

        pub fn fresh(&self) -> Result<Snapshot> {
            let found = self.all_screens()?;
            let all: Vec<Screen> = found.iter().map(|found| found.screen.clone()).collect();
            let icons = self.shell.icons(&all)?;
            Ok(Snapshot { screens: self.pick(found), icons })
        }

        fn session_name(&self) -> &'static str {
            match self.session {
                Session::Wayland => "Wayland",
                Session::X11 => "X11",
            }
        }

        fn check_icons(&self) -> Result<String> {
            let snapshot = self.snapshot()?;
            Ok(format!("{} on {}", snapshot.icons.len(), crate::commands::plural(snapshot.screens.len(), "screen")))
        }

        fn check_wallpaper(&self) -> Result<String> {
            let mut found = Vec::new();
            for screen in self.screens()? {
                let path = self.wallpaper(&screen)?.ok_or_else(|| anyhow!("not a picture file, so pick a photo as your wallpaper"))?;
                if !path.is_file() {
                    return Err(anyhow!("{} is not a picture file", path.display()));
                }
                self.decode(&path).map_err(|_| anyhow!("{} cannot be read as a picture", path.display()))?;
                found.push(path.display().to_string());
            }
            Ok(found.join(" and "))
        }

        fn check_tools(&self) -> Result<String> {
            let missing: Vec<&str> = self.shell.tools().iter().copied().filter(|tool| tool::on_path(tool).is_none()).collect();
            if missing.is_empty() {
                Ok(self.shell.tools().join(" and "))
            } else {
                Err(anyhow!("{} missing, so install them", missing.join(" and ")))
            }
        }
    }

    impl Desktop for Linux {
        fn reads_at_once(&self) -> bool {
            self.kind.reads_at_once()
        }

        fn place_saved(&self) -> Option<Instant> {
            super::saves::SAVES.last()
        }

        fn predicted(&self, shown: &dyn Fn() -> bool) -> Option<Snapshot> {
            super::drops::take(shown)
        }

        fn name(&self) -> String {
            format!("{} on {}", self.kind.name(), self.session_name())
        }

        fn snapshot(&self) -> Result<Snapshot> {
            Ok(super::drops::held(self.fresh()?))
        }

        fn screens(&self) -> Result<Vec<Screen>> {
            Ok(self.pick(self.all_screens()?))
        }

        fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>> {
            self.shell.wallpaper(screen)
        }

        fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()> {
            LAST_SET.store(now_millis(), Ordering::SeqCst);
            let result = self.shell.set_wallpaper(screen, path);
            LAST_SET.store(now_millis(), Ordering::SeqCst);
            result
        }

        fn run(&self, wake: Wake) -> Result<()> {
            let wake: screens::Shared = std::sync::Arc::from(wake);
            let mut watches = self.shell.watches();
            if super::drops::applies(Some(self.kind), self.session) {
                let (predicted, net) = (watch::tagged(&wake, "predicted drop"), watch::tagged(&wake, "drop net"));
                let plasma = self.kind == Kind::Kde;
                watches.pointer &= !plasma;
                let _ = std::thread::Builder::new().name("scooch-drops".into()).spawn(move || if plasma { super::drops::run_plasma(&predicted, &net) } else { super::drops::run(&predicted, &net) });
            }
            let shared = wake.clone();
            watch::run(Box::new(move || shared()), watches, &self.desktop, self.session, self.kind.hears_saves())
        }

        fn autostart(&self, exe: Option<&Path>) -> Result<()> {
            match exe {
                Some(exe) => autostart::install(exe),
                None => autostart::remove(),
            }
        }

        fn check(&self) -> Vec<(String, Result<String>)> {
            vec![
                ("Desktop".into(), Ok(format!("{} with icons in {}", self.kind.name(), crate::commands::tilde(&self.desktop)))),
                ("Session".into(), Ok(self.session_name().into())),
                ("Icons".into(), self.check_icons()),
                ("Wallpaper".into(), self.check_wallpaper()),
                ("Tools".into(), self.check_tools()),
                ("Login item".into(), Ok(if autostart::installed() { "installed".into() } else { "not installed".into() })),
            ]
        }

        fn format(&self) -> Format {
            Format::Jpeg
        }

        fn decode(&self, path: &Path) -> Result<RgbImage> {
            decode(path)
        }

        fn gallery(&self) -> Vec<std::path::PathBuf> {
            crate::platform::folder_and_children(Path::new("/usr/share/backgrounds"))
        }
    }

    pub fn decode(path: &Path) -> Result<RgbImage> {
        match scooch_core::open(path) {
            Ok(image) => Ok(image),
            Err(first) => jxl(path).map_err(|_| anyhow!(first)),
        }
    }

    fn jxl(path: &Path) -> Result<RgbImage> {
        let mut image = jxl_oxide::JxlImage::builder().open(path).map_err(|_| anyhow!("{} is not a JPEG XL picture.", path.display()))?;
        image.request_color_encoding(jxl_oxide::EnumColourEncoding::srgb(jxl_oxide::RenderingIntent::Relative));
        let render = image.render_frame(0).map_err(|_| anyhow!("{} could not be decoded.", path.display()))?;
        let mut stream = render.stream_no_alpha();
        let (width, height, channels) = (stream.width(), stream.height(), stream.channels() as usize);
        if width == 0 || height == 0 || channels == 0 {
            bail!("{} is empty.", path.display());
        }
        let mut samples = vec![0u8; width as usize * height as usize * channels];
        stream.write_to_buffer(&mut samples);
        let rgb = match channels {
            1 => samples.iter().flat_map(|value| [*value; 3]).collect(),
            3 => samples,
            _ => samples.chunks_exact(channels).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect(),
        };
        RgbImage::from_raw(width, height, rgb).context("The picture did not fit in memory.")
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{bus, tool, LIMIT};
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    pub const SILENT: &str = "org.scooch.Silent";

    pub struct Private {
        daemon: Child,
        pub address: String,
    }

    impl Drop for Private {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    pub fn private_bus() -> Option<Private> {
        let started = Command::new("dbus-daemon").args(["--session", "--nofork", "--print-address=1"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut daemon) = started else {
            assert!(std::env::var_os("SCOOCH_NEED_BUS").is_none(), "no dbus-daemon, and this runner must have one");
            return None;
        };
        let mut line = String::new();
        if let Some(out) = daemon.stdout.take() {
            let _ = BufReader::new(out).read_line(&mut line);
        }
        let address = line.trim().to_string();
        let private = Private { daemon, address };
        assert!(!private.address.is_empty(), "dbus-daemon printed no address");
        Some(private)
    }

    pub fn silent_peer(address: &str) -> (zbus::blocking::Connection, mpsc::Receiver<String>) {
        let peer = bus::connect(Some(address)).unwrap();
        peer.request_name(SILENT).unwrap();
        let calls = zbus::blocking::MessageIterator::from(peer.clone());
        let (took, taken) = mpsc::channel();
        std::thread::spawn(move || {
            for message in calls.flatten() {
                if let Some(member) = message.header().member() {
                    let _ = took.send(member.to_string());
                }
            }
        });
        (peer, taken)
    }

    #[test]
    fn a_peer_that_takes_the_call_and_never_answers_ends_in_plain_words_at_the_limit() {
        let Some(private) = private_bus() else { return };
        let (_peer, taken) = silent_peer(&private.address);
        let caller = bus::connect(Some(&private.address)).unwrap();
        let (said, answer) = mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            let reply: anyhow::Result<bool> = bus::call_on(&caller, SILENT, "/org/scooch/Silent", "org.gtk.Actions", "Describe", &("desktopGeometry",));
            let _ = said.send(reply.map_err(|error| error.to_string()));
        });
        assert_eq!(taken.recv_timeout(Duration::from_secs(5)).as_deref(), Ok("Describe"), "the silent peer never got the call");
        let reply = answer.recv_timeout(LIMIT + Duration::from_secs(5)).expect("the call still waited 5 s past the limit");
        assert!(started.elapsed() >= LIMIT - Duration::from_millis(500), "the call ended before the limit {:?}", started.elapsed());
        assert_eq!(reply, Err(format!("{SILENT} did not answer Describe within 10 seconds, so check that the desktop still runs.")));
    }

    #[test]
    fn a_tick_held_by_a_call_nobody_answers_comes_back_and_the_next_tick_draws() {
        let Some(private) = private_bus() else { return };
        let (_peer, taken) = silent_peer(&private.address);
        let (daemon, fake, _) = crate::daemon::tests::rig("held-tick", vec![crate::daemon::tests::screen("main", 0.0, 1.0)]);
        let caller = bus::connect(Some(&private.address)).unwrap();
        let (said, answer) = mpsc::channel();
        fake.during(move || {
            let reply: anyhow::Result<bool> = bus::call_on(&caller, SILENT, "/org/scooch/Silent", "org.gtk.Actions", "Describe", &("desktopGeometry",));
            let _ = said.send(reply.map_err(|error| error.to_string()));
        });
        let daemon = std::sync::Arc::new(daemon);
        let ticking = daemon.clone();
        let (done, finished) = mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            let held = ticking.tick();
            let after = ticking.tick();
            let _ = done.send((held.outcomes.len(), after.problem, after.outcomes.len()));
        });
        let ticks = finished.recv_timeout(LIMIT + Duration::from_secs(10)).expect("the tick still sat in the call 10 s past the limit");
        assert!(started.elapsed() >= LIMIT - Duration::from_millis(500), "the tick ended before the limit {:?}", started.elapsed());
        assert_eq!(taken.try_recv().as_deref(), Ok("Describe"));
        assert_eq!(answer.try_recv(), Ok(Err(format!("{SILENT} did not answer Describe within 10 seconds, so check that the desktop still runs."))));
        assert_eq!(ticks, (1, None, 1));
    }

    #[test]
    fn a_tick_the_desktop_did_not_answer_is_tried_again_with_no_wake_and_the_waits_end_with_a_tick_that_works() {
        use crate::daemon::{rearm, retry_wait, Signal};
        let (daemon, fake, _) = crate::daemon::tests::rig("silent-retry", vec![crate::daemon::tests::screen("main", 0.0, 1.0)]);
        *fake.silent.lock().unwrap() = 1;
        let held = daemon.tick();
        assert!(held.silent);
        assert_eq!(held.problem.as_deref(), Some("The Wayland display did not answer within 10 seconds, so check that the desktop still runs."));
        let worker = std::sync::Arc::new(Signal::default());
        assert_eq!(rearm(&worker, &held, 0), 1);
        let (waiting, (woke, heard)) = (worker.clone(), mpsc::channel());
        let started = Instant::now();
        std::thread::spawn(move || {
            waiting.first();
            let _ = woke.send(());
        });
        heard.recv_timeout(retry_wait(0) + Duration::from_secs(5)).expect("no tick was tried again without a wake");
        assert!(started.elapsed() >= retry_wait(0) - Duration::from_millis(100), "the retry came after {:?}", started.elapsed());
        worker.take();
        let back = daemon.tick();
        assert_eq!((back.silent, back.problem.clone(), back.outcomes.len()), (false, None, 1));
        assert_eq!(rearm(&worker, &back, 1), 0);
        let (idle, (late, never)) = (worker.clone(), mpsc::channel());
        std::thread::spawn(move || {
            idle.first();
            let _ = late.send(());
        });
        assert!(never.recv_timeout(Duration::from_secs(1)).is_err(), "a timer ran while nothing was wrong");
        *fake.broken.lock().unwrap() = Some("Scooch only moves picture files, so pick a photo as your wallpaper to use it again.".into());
        assert!(!daemon.tick().silent);
        assert_eq!((0..5).map(|tries| retry_wait(tries).as_secs()).collect::<Vec<_>>(), vec![5, 30, 120, 120, 120]);
    }

    #[test]
    fn a_program_that_never_ends_is_stopped_at_the_limit_in_plain_words() {
        let (said, answer) = mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            let _ = said.send(tool::run("sleep", &["600"]).map(|_| ()).map_err(|error| error.to_string()));
        });
        let reply = answer.recv_timeout(LIMIT + Duration::from_secs(5)).expect("the program still ran 5 s past the limit");
        assert!(started.elapsed() >= LIMIT - Duration::from_millis(500), "the program was stopped before the limit {:?}", started.elapsed());
        assert_eq!(reply, Err("sleep did not finish within 10 seconds, so check that the desktop still runs.".to_string()));
        let left = Command::new("pgrep").args(["-f", "^sleep 600$"]).output().map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string()).unwrap_or_default();
        assert!(left.is_empty(), "the stopped program is still running as {left}");
    }

    #[test]
    fn a_program_that_ends_gives_its_output_and_status() {
        let output = tool::run("sh", &["-c", "echo out; echo error >&2; exit 3"]).unwrap();
        assert_eq!((String::from_utf8_lossy(&output.stdout).trim(), String::from_utf8_lossy(&output.stderr).trim(), output.status.code()), ("out", "error", Some(3)));
    }
}
