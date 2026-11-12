#[cfg(not(target_os = "linux"))]
fn main() {
    println!("linuxprobe runs on Linux only, so start it inside a Linux desktop session.");
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    probe::main()
}

#[cfg(target_os = "linux")]
mod probe {
    use anyhow::Result;
    use scooch::platform::linux::parse::desktop::{Kind, Session};
    use scooch::platform::linux::parse::{ding, gio, kde as kde_rules, record, xfce as xfce_rules};
    use scooch::platform::linux::settings::{self, Gsettings, Plasma, Xfconf};
    use scooch::platform::linux::{bus, decode, gnome, gvfs, kde, screens, tool, xfce, Linux};
    use scooch::platform::{Desktop, Screen};
    use scooch_core::Rect;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::process::{Child, ExitCode};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    const FOLDER: &str = "scooch-probe-folder";
    const PROBES: [(&str, bool); 5] = [
        (FOLDER, true),
        ("scooch-probe-a.txt", false),
        ("scooch-probe-twentyeight.txt", false),
        ("scooch-probe-u with spaces that wraps.txt", false),
        ("scooch-probe-v-a-name-long-enough-that-the-desktop-has-to-cut-it-after-two-lines.txt", false),
    ];

    static START: OnceLock<Instant> = OnceLock::new();

    fn now() -> u128 {
        START.get_or_init(Instant::now).elapsed().as_millis()
    }

    struct Probe {
        out: PathBuf,
        report: Vec<String>,
        log: fs::File,
        home: PathBuf,
        desktop: PathBuf,
        kind: Option<Kind>,
        session: Session,
        made: Vec<PathBuf>,
        originals: Vec<(Screen, Option<PathBuf>)>,
        recorded: Vec<(String, Option<String>)>,
        daemon: Option<Child>,
    }

    impl Probe {
        fn line(&mut self, text: impl Into<String>) {
            let text = text.into();
            println!("{text}");
            let _ = writeln!(self.log, "{:>8} {text}", now());
            self.report.push(text);
        }

        fn raw(&mut self, text: impl AsRef<str>) {
            let _ = writeln!(self.log, "{:>8} {}", now(), text.as_ref());
        }

        fn save(&self) {
            let _ = fs::write(self.out.join("report.md"), self.report.join("\n") + "\n");
        }

        fn section(&mut self, title: &str, check: fn(&mut Probe) -> Result<()>) {
            self.line(String::new());
            self.line(format!("## {title}"));
            self.line(String::new());
            let started = Instant::now();
            if let Err(error) = check(self) {
                self.line(format!("- **check failed** {error:#}"));
            }
            self.line(format!("- took {} ms", started.elapsed().as_millis()));
            self.save();
        }

        fn linux(&self) -> Result<Linux> {
            Linux::open()
        }
    }

    pub fn main() -> ExitCode {
        START.get_or_init(Instant::now);
        let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "linuxprobe".into()));
        let home = std::env::temp_dir().join(format!("scooch-probe-{}", std::process::id()));
        if fs::create_dir_all(&out).is_err() || fs::create_dir_all(&home).is_err() {
            eprintln!("linuxprobe could not make its folders, so check the disk.");
            return ExitCode::from(2);
        }
        std::env::set_var("SCOOCH_HOME", &home);
        let Ok(log) = fs::File::create(out.join("raw.log")) else {
            eprintln!("linuxprobe could not write its log, so check the disk.");
            return ExitCode::from(2);
        };
        let kind = scooch::platform::linux::parse::desktop::pick(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default());
        let session = scooch::platform::linux::parse::desktop::session(std::env::var("WAYLAND_DISPLAY").ok().as_deref());
        let mut probe = Probe {
            out,
            report: vec![format!("# Scooch Linux probe, {}", std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "no desktop".into()))],
            log,
            home,
            desktop: scooch::platform::linux::desktop_folder(),
            kind,
            session,
            made: Vec::new(),
            originals: Vec::new(),
            recorded: Vec::new(),
            daemon: None,
        };
        probe.section("1 Session facts", facts);
        probe.section("7 Cleanup", cleanup);
        probe.save();
        ExitCode::SUCCESS
    }

    fn show(rect: &Rect) -> String {
        format!("{:.0},{:.0} {:.0}x{:.0}", rect.x, rect.y, rect.w, rect.h)
    }

    fn output(program: &str, arguments: &[&str]) -> String {
        match tool::run(program, arguments) {
            Ok(result) => {
                let mut text = String::from_utf8_lossy(&result.stdout).trim().to_string();
                let error = String::from_utf8_lossy(&result.stderr).trim().to_string();
                if !error.is_empty() {
                    text.push_str(&format!(" [stderr {error}]"));
                }
                text
            }
            Err(error) => format!("could not run, {error}"),
        }
    }

    fn version(kind: Option<Kind>) -> String {
        match kind {
            Some(Kind::Gnome) => output("gnome-shell", &["--version"]),
            Some(Kind::Kde) => output("plasmashell", &["--version"]),
            Some(Kind::Xfce) => output("xfdesktop", &["--version"]).lines().next().unwrap_or_default().to_string(),
            Some(Kind::Cinnamon) => output("cinnamon", &["--version"]),
            Some(Kind::Mate) => output("caja", &["--version"]),
            None => "unknown".into(),
        }
    }

    fn facts(p: &mut Probe) -> Result<()> {
        p.line(format!("- desktop {:?}, version {}", p.kind, version(p.kind)));
        p.line(format!("- session {:?}, WAYLAND_DISPLAY {:?}, DISPLAY {:?}, XDG_SESSION_TYPE {:?}", p.session, std::env::var("WAYLAND_DISPLAY").ok(), std::env::var("DISPLAY").ok(), std::env::var("XDG_SESSION_TYPE").ok()));
        p.line(format!("- Desktop folder {}", p.desktop.display()));
        p.line(format!("- SCOOCH_HOME {}", p.home.display()));
        for (name, found) in ["gio", "gsettings", "xdg-user-dir", "xfconf-query", "xfdesktop", "plasmashell", "kwriteconfig6", "kwriteconfig5", "nemo", "caja", "dconf", "gdbus"].iter().map(|name| (*name, tool::on_path(name))) {
            p.line(format!("- tool {name} {}", found.map_or("missing".into(), |path| path.display().to_string())));
        }
        match screens::read(p.session) {
            Ok(found) => {
                for found in found {
                    p.line(format!("- monitor `{}` at {} scale {} primary {}", found.screen.id, show(&found.screen.frame), found.screen.scale, found.primary));
                }
            }
            Err(error) => p.line(format!("- monitors failed {error:#}")),
        }
        if p.session == Session::X11 {
            p.line(format!("- _NET_WORKAREA {:?}", screens::cardinals("_NET_WORKAREA")));
            p.line(format!("- _NET_CURRENT_DESKTOP {:?}", screens::cardinals("_NET_CURRENT_DESKTOP")));
        }
        match p.kind {
            Some(Kind::Gnome) => gnome_facts(p),
            Some(Kind::Kde) => kde_facts(p),
            Some(Kind::Xfce) => xfce_facts(p),
            Some(Kind::Cinnamon) => file_manager_facts(p, "nemo", &["org.cinnamon.desktop.background picture-uri", "org.cinnamon.desktop.background picture-options", "org.nemo.desktop desktop-layout", "org.nemo.desktop font", "org.nemo.icon-view default-zoom-level"]),
            Some(Kind::Mate) => file_manager_facts(p, "caja", &["org.mate.background picture-filename", "org.mate.background picture-options", "org.mate.caja.desktop font", "org.mate.caja.icon-view default-zoom-level", "org.mate.caja.desktop computer-icon-visible"]),
            None => {}
        }
        let adapter = p.linux();
        match &adapter {
            Ok(linux) => {
                p.line(format!("- adapter {}", linux.name()));
                for (name, result) in linux.check() {
                    p.line(format!("- doctor {name} {}", result.map_or_else(|error| format!("fail {error}"), |detail| format!("ok {detail}"))));
                }
                if let Ok(screens) = linux.screens() {
                    for screen in screens {
                        let shown = linux.wallpaper(&screen).ok().flatten();
                        p.line(format!("- adapter screen `{}` at {} shows {:?}", screen.id, show(&screen.frame), shown));
                        p.originals.push((screen, shown));
                    }
                }
                match linux.snapshot() {
                    Ok(snapshot) => {
                        p.line(format!("- adapter snapshot {} icons on {} screens", snapshot.icons.len(), snapshot.screens.len()));
                        for icon in &snapshot.icons {
                            p.raw(format!("snapshot icon {}", show(icon)));
                        }
                    }
                    Err(error) => p.line(format!("- adapter snapshot failed {error:#}")),
                }
            }
            Err(error) => p.line(format!("- adapter failed {error:#}")),
        }
        defaults(p);
        Ok(())
    }

    fn gnome_facts(p: &mut Probe) {
        p.line(format!("- DING running {:?}, Gtk4 fork bus {}", gnome::running().map_err(|error| error.to_string()), bus::has_owner(ding::GTK4_FORK_BUS_NAME)));
        p.line(format!("- enabled extensions {}", output("gsettings", &["get", "org.gnome.shell", "enabled-extensions"])));
        for key in ["icon-size", "start-corner", "show-home", "show-trash", "show-volumes", "keep-arranged", "keep-stacked", "add-volumes-opposite"] {
            p.line(format!("- ding {key} {}", output("gsettings", &["get", gnome::SCHEMA, key])));
        }
        p.line(format!("- mutter experimental-features {}", output("gsettings", &["get", "org.gnome.mutter", "experimental-features"])));
        match gnome::geometry() {
            Ok(monitors) => {
                for monitor in &monitors {
                    p.line(format!("- DING geometry {monitor:?}"));
                }
                let prefs = gnome::prefs();
                let mode = gnome::mode(p.session == Session::X11);
                for monitor in &monitors {
                    if let Some(grid) = ding::Grid::new(monitor, prefs.size, mode) {
                        p.line(format!("- DING grid {} columns by {} rows, cell {}x{}, divisor {}, area {:?}", grid.cols, grid.rows, grid.cell_w, grid.cell_h, grid.divisor, grid.area));
                    }
                }
            }
            Err(error) => p.line(format!("- DING geometry failed {error:#}")),
        }
        let home = dirs::home_dir().unwrap_or_default();
        let mut locations: Vec<OsString> = vec![home.into_os_string(), "trash:///".into()];
        locations.extend(fs::read_dir(&p.desktop).map(|entries| entries.flatten().map(|entry| entry.path().into_os_string()).collect::<Vec<_>>()).unwrap_or_default());
        let raw = tool::run_os("gio", &[vec![OsString::from("info"), "-a".into(), "metadata::*".into()], locations.clone()].concat());
        match raw {
            Ok(result) => {
                p.raw(format!("gio info stdout\n{}", String::from_utf8_lossy(&result.stdout)));
                p.raw(format!("gio info stderr\n{}", String::from_utf8_lossy(&result.stderr)));
                let parsed = gio::parse_info(&String::from_utf8_lossy(&result.stdout));
                p.line(format!("- gio info took {} locations and returned {} blocks, exit {:?}", locations.len(), parsed.len(), result.status.code()));
                for info in parsed {
                    p.line(format!("  - `{}` position {:?}", info.uri, info.attributes.get(ding::POSITION_KEY)));
                }
            }
            Err(error) => p.line(format!("- gio info failed {error:#}")),
        }
        for key in ["picture-uri", "picture-uri-dark", "picture-options"] {
            p.line(format!("- background {key} {}", output("gsettings", &["get", gnome::BACKGROUND, key])));
        }
        p.line(format!("- color-scheme {}", output("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"])));
    }

    fn kde_facts(p: &mut Probe) {
        p.line(format!("- KDE_SESSION_VERSION {:?}, plasma6 by the adapter {}", std::env::var("KDE_SESSION_VERSION").ok(), kde::plasma6()));
        p.line(format!("- current activity {:?}", kde::activity()));
        let folder = kde::config_folder();
        for name in [kde_rules::APPLETS, kde_rules::SHELL, "kactivitymanagerdrc"] {
            let text = fs::read_to_string(folder.join(name)).unwrap_or_else(|error| format!("unreadable {error}"));
            p.raw(format!("{name}\n{text}"));
            p.line(format!("- {name} {} bytes", text.len()));
        }
        for containment in kde_rules::containments(&kde::groups(kde_rules::APPLETS)) {
            p.line(format!("- containment {} screen {} plugin {} activity {:?} image {:?}", containment.id, containment.screen, containment.plugin, containment.activity, containment.image));
            for (key, value) in &containment.general {
                p.line(format!("  - {key} = `{value}`"));
            }
        }
        p.line(format!("- panels {:?}", kde::evaluate(kde_rules::PANELS_SCRIPT)));
        p.line(format!("- screens from the script {:?}", kde::evaluate("var out = []; for (var i = 0; i < screenCount; i++) { out.push(JSON.stringify(screenGeometry(i))); } print(out.join(' '));")));
    }

    fn xfce_facts(p: &mut Probe) {
        let listed = settings::xfconf_list();
        for property in listed.iter().filter(|property| property.starts_with("/backdrop/") || property.starts_with("/desktop-icons/")) {
            let value = settings::xfconf(property).unwrap_or_default();
            p.line(format!("- xfconf {property} = `{value}`"));
        }
        let folder = kde::config_folder().join("xfce4").join("desktop");
        if let Ok(entries) = fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let text = fs::read_to_string(entry.path()).unwrap_or_default();
                p.line(format!("- positions file {} {} bytes", entry.path().display(), text.len()));
                p.raw(format!("{}\n{text}", entry.path().display()));
            }
        }
        p.line(format!("- adapter work area {:?}, workspace {}", xfce::workarea(p.session).map(|rect| show(&rect)), xfce::workspace(p.session)));
    }

    fn file_manager_facts(p: &mut Probe, program: &str, keys: &[&str]) {
        for key in keys {
            let mut parts = key.split(' ');
            let (schema, name) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
            p.line(format!("- gsettings {key} {}", output("gsettings", &["get", schema, name])));
        }
        let metadata = kde::config_folder().join(program).join("desktop-metadata");
        let text = fs::read_to_string(&metadata).unwrap_or_else(|error| format!("unreadable {error}"));
        p.line(format!("- {} {} bytes", metadata.display(), text.len()));
        p.raw(format!("{}\n{text}", metadata.display()));
        for line in text.lines().take(40) {
            p.line(format!("  - `{line}`"));
        }
        let home = dirs::home_dir().unwrap_or_default();
        let mut locations: Vec<OsString> = vec![home.into_os_string(), "trash:///".into(), "computer:///".into()];
        locations.extend(fs::read_dir(&p.desktop).map(|entries| entries.flatten().map(|entry| entry.path().into_os_string()).collect::<Vec<_>>()).unwrap_or_default());
        match gvfs::metadata(&locations) {
            Ok(found) => {
                for info in found {
                    p.line(format!("  - `{}` {:?}", info.uri, info.attributes));
                }
            }
            Err(error) => p.line(format!("- gio info failed {error:#}")),
        }
    }

    fn defaults(p: &mut Probe) {
        let mut tried = 0;
        let mut failed: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut folders = vec![PathBuf::from("/usr/share/backgrounds"), PathBuf::from("/usr/share/wallpapers")];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = fs::read_dir(&folder) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                    continue;
                }
                let kind = path.extension().map(|kind| kind.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                if ["xml", "desktop", "json", "txt", "md", "license"].contains(&kind.as_str()) || tried >= 60 {
                    continue;
                }
                tried += 1;
                if decode(&path).is_err() {
                    failed.entry(kind).or_default().push(path.display().to_string());
                }
            }
        }
        p.line(format!("- default wallpapers tried {tried}, unreadable by kind {:?}", failed.iter().map(|(kind, list)| (kind.clone(), list.len())).collect::<Vec<_>>()));
        for (kind, list) in failed {
            p.line(format!("  - {kind} {}", list.join(", ")));
        }
    }

























    fn keys(p: &Probe, screen: &Screen) -> Vec<(Box<dyn record::Settings>, String)> {
        match p.kind {
            Some(Kind::Gnome) => ["org.gnome.desktop.background picture-uri", "org.gnome.desktop.background picture-uri-dark", "org.gnome.desktop.background picture-options", "org.gnome.desktop.interface color-scheme"]
                .iter()
                .map(|key| (Box::new(Gsettings) as Box<dyn record::Settings>, key.to_string()))
                .collect(),
            Some(Kind::Xfce) => {
                let prefix = xfce_rules::backdrop_prefix(&screen.id, xfce::workspace(p.session));
                vec![(Box::new(Xfconf) as Box<dyn record::Settings>, format!("{prefix}/last-image")), (Box::new(Xfconf), format!("{prefix}/image-style"))]
            }
            Some(Kind::Kde) => vec![(Box::new(Plasma) as Box<dyn record::Settings>, settings::plasma_key(kde::number_of(screen)))],
            _ => Vec::new(),
        }
    }


    fn restore(p: &mut Probe, linux: &Linux, screen: &Screen) {
        let original = p.originals.iter().find(|(seen, _)| seen.id == screen.id).and_then(|(_, path)| path.clone());
        match original {
            Some(path) => {
                let result = linux.set_wallpaper(screen, &path);
                p.line(format!("- restored to {} {:?}", path.display(), result.map_err(|error| error.to_string())));
            }
            None => p.line("- no original picture was recorded".to_string()),
        }
        std::thread::sleep(Duration::from_secs(1));
        for (store, key) in keys(p, screen) {
            let now_value = store.get(&key).ok().flatten();
            let before = p.recorded.iter().find(|(seen, _)| *seen == key).map(|(_, value)| value.clone());
            p.line(format!("- `{key}` now {now_value:?}, equal to the recorded value {}", before.as_ref().is_some_and(|before| *before == now_value)));
        }
        p.line(format!("- record file left behind {}", record::file(&p.home).exists()));
    }




    fn cleanup(p: &mut Probe) -> Result<()> {
        if let Some(mut child) = p.daemon.take() {
            let _ = child.kill();
            let _ = child.wait();
            p.line("- the daemon was still running and was killed".to_string());
        }
        let mut names: Vec<PathBuf> = p.made.clone();
        names.extend(PROBES.iter().map(|(name, _)| p.desktop.join(name)));
        for path in names {
            let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            if path.exists() {
                p.line(format!("- could not delete {}", path.display()));
            }
        }
        if let Ok(linux) = p.linux() {
            if let Ok(screens) = linux.screens() {
                for screen in screens {
                    restore(p, &linux, &screen);
                }
            }
        }
        let leftover = record::load(&record::file(&p.home));
        if !leftover.settings.is_empty() {
            p.line(format!("- a record was left, restoring it directly {:?}", leftover.settings));
            let store: Option<Box<dyn record::Settings>> = match p.kind {
                Some(Kind::Gnome) => Some(Box::new(Gsettings)),
                Some(Kind::Xfce) => Some(Box::new(Xfconf)),
                Some(Kind::Kde) => Some(Box::new(Plasma)),
                _ => None,
            };
            if let Some(store) = store {
                p.line(format!("- direct restore {:?}", record::restore(store.as_ref(), &record::file(&p.home), &leftover).map_err(|error| error.to_string())));
            }
        }
        let removed = fs::remove_dir_all(&p.home);
        p.line(format!("- temp folder removed {:?}", removed.map_err(|error| error.to_string())));
        Ok(())
    }
}
