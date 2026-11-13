pub mod parse;

#[cfg(target_os = "linux")]
pub mod autostart;
#[cfg(target_os = "linux")]
pub mod cinnamon;
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
pub use system::{bus, decode, desktop, desktop_folder, set_recently, shell, tool, Linux, Shell, Signal, Watches, FileWatch};

#[cfg(target_os = "linux")]
mod system {
    use super::parse::desktop::{self as kinds, Kind, Session};
    use super::{autostart, cinnamon, gnome, kde, mate, screens, watch, xfce};
    use crate::platform::{Desktop, Format, Screen, Snapshot, Wake};
    use anyhow::{anyhow, bail, Context, Result};
    use scooch_core::{Rect, RgbImage};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static LAST_SET: AtomicU64 = AtomicU64::new(0);
    const OWN_ECHO: Duration = Duration::from_secs(1);

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

    pub mod tool {
        use anyhow::{anyhow, Result};
        use std::ffi::{OsStr, OsString};
        use std::path::PathBuf;
        use std::process::{Command, Output, Stdio};

        pub fn run(program: &str, arguments: &[&str]) -> Result<Output> {
            let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
            run_os(program, &arguments)
        }

        pub fn run_os(program: &str, arguments: &[OsString]) -> Result<Output> {
            Command::new(program)
                .args(arguments)
                .env("LC_ALL", "C.UTF-8")
                .env_remove("LANGUAGE")
                .stdin(Stdio::null())
                .output()
                .map_err(|_| anyhow!("Scooch could not run {program}, so install it or check your PATH."))
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
        use std::sync::Mutex;
        use zbus::blocking::Connection;
        use zbus::zvariant::{DynamicType, Type};

        static SESSION: Mutex<Option<Connection>> = Mutex::new(None);

        pub fn session() -> Result<Connection> {
            let mut slot = SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(connection) = slot.as_ref() {
                return Ok(connection.clone());
            }
            let connection = Connection::session().map_err(|_| anyhow!("Scooch could not reach the session bus, so log in to a desktop session first."))?;
            *slot = Some(connection.clone());
            Ok(connection)
        }

        pub fn forget() {
            *SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        }

        pub fn call<B, R>(destination: &str, path: &str, interface: &str, method: &str, body: &B) -> Result<R>
        where
            B: Serialize + DynamicType,
            R: DeserializeOwned + Type,
        {
            let reply = match session()?.call_method(Some(destination), path, Some(interface), method, body) {
                Ok(reply) => reply,
                Err(zbus::Error::InputOutput(_)) => {
                    forget();
                    session()?.call_method(Some(destination), path, Some(interface), method, body)?
                }
                Err(error) => return Err(error.into()),
            };
            Ok(reply.body().deserialize()?)
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

        fn pick(&self, found: Vec<screens::Found>) -> Vec<Screen> {
            if self.shell.per_screen() {
                return found.into_iter().map(|found| found.screen).collect();
            }
            let all: Vec<Screen> = found.iter().map(|found| found.screen.clone()).collect();
            let index = self.shell.primary(&all).or_else(|| found.iter().position(|found| found.primary)).unwrap_or(0);
            all.into_iter().nth(index).into_iter().collect()
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
        fn name(&self) -> String {
            format!("{} on {}", self.kind.name(), self.session_name())
        }

        fn snapshot(&self) -> Result<Snapshot> {
            let found = self.all_screens()?;
            let all: Vec<Screen> = found.iter().map(|found| found.screen.clone()).collect();
            let icons = self.shell.icons(&all)?;
            Ok(Snapshot { screens: self.pick(found), icons })
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
            watch::run(wake, self.shell.watches(), &self.desktop, self.session)
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
