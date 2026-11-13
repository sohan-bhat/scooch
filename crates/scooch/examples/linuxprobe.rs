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
    use anyhow::{anyhow, bail, Context, Result};
    use scooch::platform::linux::parse::desktop::{Kind, Session};
    use scooch::platform::linux::parse::geometry::Insets;
    use scooch::platform::linux::parse::{caja, ding, gio, kde as kde_rules, keyfile, nemo, record, wakes, xfce as xfce_rules};
    use scooch::platform::linux::settings::{self, Gsettings, Plasma, Xfconf};
    use scooch::platform::linux::{bus, cinnamon, decode, gnome, gvfs, kde, mate, screens, tool, xfce, Linux};
    use scooch::platform::{Desktop, Screen};
    use scooch_core::{Rect, RgbImage};
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use x11rb::connection::Connection as _;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, MOTION_NOTIFY_EVENT};
    use x11rb::protocol::xtest::ConnectionExt as _;
    use zbus::zvariant;

    const FOLDER: &str = "scooch-probe-folder";
    const PROBES: [(&str, bool); 5] = [
        (FOLDER, true),
        ("scooch-probe-a.txt", false),
        ("scooch-probe-twentyeight.txt", false),
        ("scooch-probe-u with spaces that wraps.txt", false),
        ("scooch-probe-v-a-name-long-enough-that-the-desktop-has-to-cut-it-after-two-lines.txt", false),
    ];
    const SHOW_WAIT: Duration = Duration::from_secs(10);
    const SETTLE: Duration = Duration::from_millis(1500);
const SUBJECT: (f32, f32, f32) = (0.6, 0.65, 0.12);

    static START: OnceLock<Instant> = OnceLock::new();

    fn now() -> u128 {
        START.get_or_init(Instant::now).elapsed().as_millis()
    }

    type Events = Arc<Mutex<Vec<(u128, String)>>>;

    struct Measure {
        name: String,
        adapter: Option<Rect>,
        pixels: Option<Rect>,
        stored: String,
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
        measures: Vec<Measure>,
        originals: Vec<(Screen, Option<PathBuf>)>,
        recorded: Vec<(String, Option<String>)>,
        pictures: Vec<PathBuf>,
        daemon: Option<Child>,
        events: Events,
        listening: bool,
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
            measures: Vec::new(),
            originals: Vec::new(),
            recorded: Vec::new(),
            pictures: Vec::new(),
            daemon: None,
            events: Arc::new(Mutex::new(Vec::new())),
            listening: false,
        };
        probe.section("1 Session facts", facts);
        probe.section("2 Read", read);
        probe.section("3 Geometry truth from pixels", geometry);
        probe.section("4 Move and events", events);
        probe.section("5 Wallpaper", wallpaper);
        probe.section("6 End to end", end_to_end);
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
        let readiness = fs::read_to_string(p.out.join("readiness.txt")).unwrap_or_default();
        for line in readiness.lines() {
            p.line(format!("- session start {line}"));
        }
        p.line(format!("- Xft.dpi {:?}, X11 scale settings GNOME {:?} Cinnamon {:?} MATE {:?} Xfce {}", screens::resources().and_then(|text| scooch::platform::linux::parse::geometry::xft_dpi(&text)), settings::scale_setting("org.gnome.desktop.interface", "scaling-factor"), settings::scale_setting("org.cinnamon.desktop.interface", "scaling-factor"), settings::scale_setting("org.mate.interface", "window-scaling-factor"), xfce::scale()));
        p.line(format!("- xfdesktop version {:?}", xfce::version()));
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
            Some(Kind::Cinnamon) => {
                file_manager_facts(p, "nemo", &["org.cinnamon.desktop.background picture-uri", "org.cinnamon.desktop.background picture-options", "org.nemo.desktop desktop-layout", "org.nemo.desktop font", "org.nemo.desktop home-icon-visible", "org.nemo.desktop trash-icon-visible", "org.nemo.desktop text-ellipsis-limit", "org.nemo.preferences show-hidden-files"]);
                nemo_facts(p);
            }
            Some(Kind::Mate) => {
                file_manager_facts(p, "caja", &["org.mate.background picture-filename", "org.mate.background picture-options", "org.mate.background show-desktop-icons", "org.mate.caja.desktop font", "org.mate.caja.desktop text-ellipsis-limit", "org.mate.caja.icon-view default-zoom-level", "org.mate.caja.desktop computer-icon-visible"]);
                caja_facts(p);
            }
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
        let raw = kde::evaluate(&kde_rules::state_script());
        p.raw(format!("live Plasma state {raw:?}"));
        match raw.as_deref().ok().and_then(kde_rules::state) {
            Some(state) => {
                p.line(format!("- live activity {:?}, Plasma screens {:?}, panels {:?}", state.activity, state.screens, state.panels));
                for desktop in &state.desktops {
                    p.line(format!("- live desktop {} screen {} plugin {} wallpaper {:?} image {:?} general {:?}", desktop.id, desktop.screen, desktop.plugin, desktop.wallpaper_plugin, desktop.image, desktop.general));
                }
            }
            None => p.line(format!("- live Plasma state unreadable {:?}", raw.err().map(|error| error.to_string()))),
        }
        p.line(format!("- default wallpaper package {:?}", kde::default_package()));
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
        p.line(format!("- adapter work area {:?}, workspace {}", xfce::workarea(p.session, true).map(|rect| show(&rect)), xfce::workspace(p.session)));
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

    fn nemo_facts(p: &mut Probe) {
        p.line(format!("- org.Cinnamon work rects {:?}", cinnamon::work_rects().map_err(|error| error.to_string())));
        let Some(home) = dirs::home_dir() else { return };
        let shell = cinnamon::Cinnamon { desktop: p.desktop.clone(), home, config: kde::config_folder(), session: p.session };
        let groups = shell.metadata();
        for window in shell.windows() {
            let settings = nemo::settings(&groups, window.index);
            let grid = nemo::Grid::new(window.canvas, &settings);
            p.line(format!("- Nemo window {} primary {} canvas {} settings {settings:?} grid {:?}", window.index, window.primary, show(&window.canvas), grid.map(|grid| (grid.snap_x, grid.snap_y, grid.cols, grid.rows, grid.left, grid.top))));
        }
        p.line(format!("- Nemo items {:?}", shell.items(&groups).map_err(|error| error.to_string())));
    }

    fn caja_facts(p: &mut Probe) {
        let Some(home) = dirs::home_dir() else { return };
        let shell = mate::Mate { desktop: p.desktop.clone(), home, config: kde::config_folder(), session: p.session };
        let groups = shell.metadata();
        let all: Vec<Screen> = screens::read(p.session).map(|found| found.into_iter().map(|found| found.screen).collect()).unwrap_or_default();
        p.line(format!("- Caja layout {:?}", shell.layout(&groups, &all)));
        p.line(format!("- Caja items {:?}", shell.items(&groups).map_err(|error| error.to_string())));
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

    fn icons(linux: &Linux) -> Vec<Rect> {
        linux.snapshot().map(|snapshot| snapshot.icons).unwrap_or_default()
    }

    fn maybe_icons(linux: &Option<Linux>) -> Vec<Rect> {
        linux.as_ref().map(icons).unwrap_or_default()
    }

    fn new_rect(before: &[Rect], after: &[Rect]) -> Option<Rect> {
        let fresh: Vec<Rect> = after.iter().filter(|rect| !before.iter().any(|old| (old.x - rect.x).abs() < 1.0 && (old.y - rect.y).abs() < 1.0)).copied().collect();
        (fresh.len() == 1).then(|| fresh[0])
    }

    fn stored(p: &Probe, path: &Path) -> String {
        match p.kind {
            Some(Kind::Gnome) => gvfs::metadata(&[path.as_os_str().to_os_string()]).ok().and_then(|found| found.first().and_then(|info| info.attributes.get(ding::POSITION_KEY).cloned())).unwrap_or_else(|| "none".into()),
            Some(Kind::Kde) => {
                let url = format!("desktop:/{}", path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default());
                let all = kde_rules::containments(&kde::groups(kde_rules::APPLETS));
                let found = all.iter().find(|containment| containment.screen == 0).and_then(|containment| containment.general.get("positions").cloned()).unwrap_or_default();
                let at = found.replace("\\,", ",").find(&url).map(|at| found.replace("\\,", ",")[at..].chars().take(url.len() + 12).collect::<String>());
                at.unwrap_or_else(|| "not in positions yet, placed in reading order".into())
            }
            Some(Kind::Xfce) => {
                let id = path.display().to_string();
                let folder = kde::config_folder().join("xfce4").join("desktop");
                let yaml = fs::read_to_string(folder.join(xfce_rules::YAML_FILE)).ok().map(|text| xfce_rules::configs(&scooch::platform::linux::parse::yaml::parse(&text)));
                if let Some(configs) = yaml {
                    return configs.iter().find_map(|config| config.icons.get(&id).map(|(row, col)| format!("row {row} col {col}"))).unwrap_or_else(|| "none".into());
                }
                let rc = fs::read_to_string(folder.join(xfce_rules::LATEST_RC)).map(|text| xfce_rules::rc_positions(&keyfile::parse(&text))).unwrap_or_default();
                rc.get(&id).map(|(row, col)| format!("row {row} col {col}")).unwrap_or_else(|| "none".into())
            }
            _ => gvfs::metadata(&[path.as_os_str().to_os_string()]).ok().and_then(|found| found.first().map(|info| format!("{:?}", info.attributes))).unwrap_or_else(|| "none".into()),
        }
    }

    fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if done() {
                return Some(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        done().then(|| started.elapsed())
    }

    fn factor(p: &Probe) -> f32 {
        p.linux().ok().and_then(|linux| linux.screens().ok()).and_then(|screens| screens.first().map(|screen| screen.scale)).filter(|scale| *scale > 0.0).unwrap_or(1.0)
    }

    fn physical(rect: &Rect, factor: f32) -> Rect {
        Rect { x: rect.x * factor, y: rect.y * factor, w: rect.w * factor, h: rect.h * factor }
    }

    fn grown(rect: &Rect, by: f32) -> Rect {
        Rect { x: rect.x - by, y: rect.y - by, w: rect.w + 2.0 * by, h: rect.h + 2.0 * by }
    }

    fn primary_frame(p: &Probe) -> Option<Rect> {
        let found = screens::read(p.session).ok()?;
        found.iter().find(|found| found.primary).or(found.first()).map(|found| found.screen.frame)
    }

    fn read(p: &mut Probe) -> Result<()> {
        let linux = p.linux().ok();
        let frame = primary_frame(p).context("no screen")?;
        let scale = factor(p);
        p.line(format!("- adapter scale {scale}, rectangles below are in screen pixels"));
        for (name, folder) in PROBES {
            let path = p.desktop.join(name);
            let before = maybe_icons(&linux);
            let quiet_a = capture(p.session, &frame).ok();
            std::thread::sleep(Duration::from_millis(300));
            let quiet_b = capture(p.session, &frame).ok();
            let made = if folder { fs::create_dir_all(&path) } else { fs::write(&path, b"scooch probe") };
            made.with_context(|| format!("could not create {}", path.display()))?;
            p.made.push(path.clone());
            let shown = wait_until(SHOW_WAIT, || maybe_icons(&linux).len() > before.len());
            std::thread::sleep(SETTLE);
            let after = maybe_icons(&linux);
            let shot = capture(p.session, &frame).ok();
            let adapter = new_rect(&before, &after).map(|rect| physical(&rect, scale));
            let focus = adapter.map(|rect| grown(&rect, 80.0));
            let pixels = match (&quiet_a, &quiet_b, &shot) {
                (Some(a), Some(b), Some(c)) => changed_box(a, b, c, (frame.x as i32, frame.y as i32), focus),
                _ => None,
            };
            let index = p.measures.len();
            if let Some(shot) = &shot {
                let _ = fs::write(p.out.join(format!("after-{index}.png")), scooch_core::encode_png(shot)?);
            }
            if let Some(before) = &quiet_b {
                let _ = fs::write(p.out.join(format!("before-{index}.png")), scooch_core::encode_png(before)?);
            }
            let stored = stored(p, &path);
            p.line(format!("- {name} shown after {:?} ms, adapter icons {} to {}", shown.map(|time| time.as_millis()), before.len(), after.len()));
            p.measures.push(Measure { name: name.into(), adapter, pixels, stored });
        }
        p.line(String::new());
        p.line("| item | stored | adapter rect |".to_string());
        p.line("|---|---|---|".to_string());
        let rows: Vec<String> = p.measures.iter().map(|m| format!("| {} | {} | {} |", m.name, m.stored, m.adapter.as_ref().map_or("none".into(), show))).collect();
        for row in rows {
            p.line(row);
        }
        Ok(())
    }

    fn capture(session: Session, area: &Rect) -> Result<RgbImage> {
        match session {
            Session::X11 => capture_x11(area),
            Session::Wayland => capture_gnome(area),
        }
    }

    fn capture_x11(area: &Rect) -> Result<RgbImage> {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).context("no root")?.root;
        let (width, height) = (area.w as u16, area.h as u16);
        let reply = conn.get_image(ImageFormat::Z_PIXMAP, root, area.x as i16, area.y as i16, width, height, !0)?.reply()?;
        let pixels = width as usize * height as usize;
        if reply.data.len() < pixels * 4 {
            bail!("the X image has {} bytes for {pixels} pixels at depth {}", reply.data.len(), reply.depth);
        }
        let rgb = reply.data.as_chunks::<4>().0.iter().take(pixels).flat_map(|pixel| [pixel[2], pixel[1], pixel[0]]).collect();
        RgbImage::from_raw(width as u32, height as u32, rgb).context("the capture did not fit")
    }

    fn screenshot_bus() -> Result<zbus::blocking::Connection> {
        static SHOT: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();
        SHOT.get_or_init(|| {
            let connection = zbus::blocking::Connection::session().ok()?;
            connection.request_name("org.gnome.Screenshot").ok()?;
            Some(connection)
        })
        .clone()
        .ok_or_else(|| anyhow!("could not own org.gnome.Screenshot"))
    }

    fn capture_gnome(area: &Rect) -> Result<RgbImage> {
        let connection = screenshot_bus()?;
        let file = std::env::temp_dir().join(format!("scooch-shot-{}-{}.png", std::process::id(), now()));
        let reply = connection.call_method(Some("org.gnome.Shell.Screenshot"), "/org/gnome/Shell/Screenshot", Some("org.gnome.Shell.Screenshot"), "Screenshot", &(false, false, file.to_string_lossy().as_ref()))?;
        let (done, used): (bool, String) = reply.body().deserialize()?;
        if !done {
            bail!("the shell refused the screenshot");
        }
        let image = scooch_core::open(Path::new(&used))?;
        let _ = fs::remove_file(&used);
        crop(&image, area).context("the screenshot is smaller than the screen")
    }

    fn crop(image: &RgbImage, area: &Rect) -> Option<RgbImage> {
        let (left, top) = (area.x.max(0.0) as u32, area.y.max(0.0) as u32);
        let right = ((area.x + area.w) as u32).min(image.width());
        let bottom = ((area.y + area.h) as u32).min(image.height());
        if right <= left || bottom <= top {
            return None;
        }
        let mut bytes = Vec::with_capacity(((right - left) * (bottom - top) * 3) as usize);
        for y in top..bottom {
            for x in left..right {
                bytes.extend_from_slice(&image.get_pixel(x, y).0);
            }
        }
        RgbImage::from_raw(right - left, bottom - top, bytes)
    }

    fn changed_box(quiet_a: &RgbImage, before: &RgbImage, after: &RgbImage, origin: (i32, i32), focus: Option<Rect>) -> Option<Rect> {
        if before.dimensions() != after.dimensions() || quiet_a.dimensions() != before.dimensions() {
            return None;
        }
        let differs = |a: &[u8], b: &[u8]| a.iter().zip(b).any(|(x, y)| x.abs_diff(*y) > 10);
        let inside = |x: u32, y: u32| focus.is_none_or(|rect| {
            let (gx, gy) = (origin.0 as f32 + x as f32, origin.1 as f32 + y as f32);
            gx >= rect.x && gx < rect.x + rect.w && gy >= rect.y && gy < rect.y + rect.h
        });
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (x, y, pixel) in after.enumerate_pixels() {
            if !inside(x, y) {
                continue;
            }
            let old = before.get_pixel(x, y);
            if differs(&pixel.0, &old.0) && !differs(&quiet_a.get_pixel(x, y).0, &old.0) {
                (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
            }
        }
        (left != u32::MAX).then(|| Rect { x: origin.0 as f32 + left as f32, y: origin.1 as f32 + top as f32, w: (right - left + 1) as f32, h: (bottom - top + 1) as f32 })
    }

    fn margins(rect: &Rect, truth: &Rect) -> [f32; 4] {
        [truth.x - rect.x, truth.y - rect.y, rect.x + rect.w - (truth.x + truth.w), rect.y + rect.h - (truth.y + truth.h)]
    }

    fn geometry(p: &mut Probe) -> Result<()> {
        p.line("| item | adapter | pixels | margins left top right bottom |".to_string());
        p.line("|---|---|---|---|".to_string());
        let mut lows = [f32::MAX; 4];
        let mut rows = Vec::new();
        for measure in &p.measures {
            let edges = match (&measure.adapter, &measure.pixels) {
                (Some(adapter), Some(pixels)) => {
                    let edges = margins(adapter, pixels);
                    for (low, edge) in lows.iter_mut().zip(edges) {
                        *low = low.min(edge);
                    }
                    format!("{edges:?}")
                }
                _ => "none".into(),
            };
            rows.push(format!("| {} | {} | {} | {edges} |", measure.name, measure.adapter.as_ref().map_or("none".into(), show), measure.pixels.as_ref().map_or("none".into(), show)));
        }
        for row in rows {
            p.line(row);
        }
        if lows[0] == f32::MAX {
            p.line("- no measurement to solve from".to_string());
            return Ok(());
        }
        let current = match p.kind {
            Some(Kind::Gnome) => ding::INSETS,
            Some(Kind::Kde) => kde_rules::INSETS,
            Some(Kind::Xfce) => xfce_rules::INSETS,
            Some(Kind::Cinnamon) => nemo::INSETS,
            Some(Kind::Mate) => caja::INSETS,
            _ => Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 },
        };
        let solved = Insets { left: current.left + lows[0].floor() as f64, top: current.top + lows[1].floor() as f64, right: current.right + lows[2].floor() as f64, bottom: current.bottom + lows[3].floor() as f64 };
        p.line(format!("- smallest margins over every probe {lows:?}, negative means the drawing spills past the adapter rect"));
        p.line(format!("- corrected constants `Insets {{ left: {:.0}, top: {:.0}, right: {:.0}, bottom: {:.0} }}` against the current `{current:?}`", solved.left, solved.top, solved.right, solved.bottom));
        let placed: Vec<Rect> = p.measures.iter().filter_map(|measure| measure.pixels).collect();
        for pair in placed.windows(2) {
            p.line(format!("- pixel step between neighbours {:.0},{:.0}", pair[1].x - pair[0].x, pair[1].y - pair[0].y));
        }
        Ok(())
    }

    fn listen(p: &mut Probe) {
        if p.listening {
            return;
        }
        p.listening = true;
        let events = p.events.clone();
        let push = move |text: String| {
            if let Ok(mut list) = events.lock() {
                list.push((now(), text));
            }
        };
        let adapter_push = push.clone();
        std::thread::spawn(move || {
            if let Ok(linux) = Linux::open() {
                let _ = linux.run(Box::new(move || adapter_push("adapter wake".into())));
            }
        });
        for (rule, name) in [(wakes::GVFS_RULE, "gvfs AttributeChanged"), (wakes::DCONF_RULE, "dconf Notify"), (wakes::XFCONF_RULE, "xfconf PropertyChanged"), (wakes::DING_RULE, "DING geometry Changed")] {
            let push = push.clone();
            std::thread::spawn(move || {
                let Ok(connection) = zbus::blocking::Connection::session() else { return };
                let Ok(messages) = zbus::blocking::MessageIterator::for_match_rule(rule, &connection, Some(256)) else { return };
                for message in messages.flatten() {
                    let detail = describe(&message);
                    push(format!("{name} {detail}"));
                }
            });
        }
        let mut folders = vec![p.desktop.clone(), kde::config_folder(), kde::config_folder().join("xfce4").join("desktop")];
        if let Some(data) = dirs::data_dir() {
            folders.push(data.join("gvfs-metadata"));
        }
        folders.push(kde::config_folder().join("dconf"));
        std::thread::spawn(move || {
            let Ok(mut inotify) = inotify::Inotify::init() else { return };
            let mut names = BTreeMap::new();
            for folder in &folders {
                let mask = inotify::WatchMask::CREATE | inotify::WatchMask::DELETE | inotify::WatchMask::MOVED_FROM | inotify::WatchMask::MOVED_TO | inotify::WatchMask::CLOSE_WRITE;
                if let Ok(descriptor) = inotify.watches().add(folder, mask) {
                    names.insert(descriptor, folder.display().to_string());
                }
            }
            let mut buffer = [0u8; 8192];
            while let Ok(found) = inotify.read_events_blocking(&mut buffer) {
                for event in found {
                    let folder = names.get(&event.wd).cloned().unwrap_or_default();
                    push(format!("inotify {folder} {:?} {:?}", event.name, event.mask));
                }
            }
        });
        std::thread::sleep(Duration::from_secs(2));
    }

    fn describe(message: &zbus::Message) -> String {
        let body = message.body();
        if let Ok((a, b)) = body.deserialize::<(String, String)>() {
            return format!("{a} {b}");
        }
        if let Ok((prefix, changes, _)) = body.deserialize::<(String, Vec<String>, String)>() {
            return format!("{prefix} {changes:?}");
        }
        if let Ok((channel, property, _)) = body.deserialize::<(String, String, zvariant::OwnedValue)>() {
            return format!("{channel} {property}");
        }
        String::new()
    }

    fn act(p: &mut Probe, name: &str, work: impl FnOnce(&mut Probe) -> Result<String>) {
        let mark = p.events.lock().map(|list| list.len()).unwrap_or(0);
        let base = now();
        let detail = work(p).unwrap_or_else(|error| format!("failed with {error:#}"));
        std::thread::sleep(Duration::from_millis(2500));
        let seen: Vec<(u128, String)> = p.events.lock().map(|list| list.get(mark..).map(<[_]>::to_vec).unwrap_or_default()).unwrap_or_default();
        p.line(format!("- **{name}** {detail}"));
        let wakes: Vec<u128> = seen.iter().filter(|(_, text)| text == "adapter wake").map(|(at, _)| at.saturating_sub(base)).collect();
        p.line(format!("  - adapter wakes {} first after {:?} ms", wakes.len(), wakes.first()));
        let mut kinds: BTreeMap<String, (usize, u128)> = BTreeMap::new();
        for (at, text) in &seen {
            if text == "adapter wake" {
                continue;
            }
            let key: String = text.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            let entry = kinds.entry(key).or_insert((0, at.saturating_sub(base)));
            entry.0 += 1;
        }
        for (kind, (count, first)) in kinds {
            p.line(format!("  - source {kind} x{count} first after {first} ms"));
        }
        for (at, text) in seen {
            p.raw(format!("  event +{} {text}", at.saturating_sub(base)));
        }
    }

    fn drag(from: (i16, i16), to: (i16, i16)) -> Result<()> {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).context("no root")?.root;
        let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
        conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, from.0, from.1, 0)?;
        conn.flush()?;
        pause(200);
        conn.xtest_fake_input(BUTTON_PRESS_EVENT, 1, 0, root, 0, 0, 0)?;
        conn.flush()?;
        pause(200);
        for step in 1..=20i32 {
            let x = from.0 as i32 + (to.0 as i32 - from.0 as i32) * step / 20;
            let y = from.1 as i32 + (to.1 as i32 - from.1 as i32) * step / 20;
            conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, x as i16, y as i16, 0)?;
            conn.flush()?;
            pause(30);
        }
        pause(200);
        conn.xtest_fake_input(BUTTON_RELEASE_EVENT, 1, 0, root, 0, 0, 0)?;
        conn.flush()?;
        conn.sync()?;
        Ok(())
    }

    fn probe_icon(p: &Probe, linux: &Linux) -> Result<Rect> {
        let name = PROBES[1].0;
        let scale = factor(p);
        let now_rects: Vec<Rect> = icons(linux).iter().map(|rect| physical(rect, scale)).collect();
        let from = p.measures.iter().find(|measure| measure.name == name).and_then(|measure| measure.adapter).context("no adapter rect for the probe icon")?;
        Ok(now_rects.iter().min_by(|a, b| (a.x - from.x).abs().max((a.y - from.y).abs()).total_cmp(&(b.x - from.x).abs().max((b.y - from.y).abs()))).copied().unwrap_or(from))
    }

    fn grab_point(rect: &Rect) -> (f32, f32) {
        (rect.x + rect.w / 2.0, rect.y + rect.h / 3.0)
    }

    fn move_icon(p: &mut Probe, target: (f32, f32)) -> Result<String> {
        let name = PROBES[1].0;
        let path = p.desktop.join(name);
        let scale = factor(p);
        if p.kind == Some(Kind::Gnome) {
            let monitors = gnome::geometry()?;
            let prefs = gnome::prefs();
            let (x, y) = (f64::from(target.0 / scale), f64::from(target.1 / scale));
            let grids: Vec<ding::Grid> = monitors.iter().filter_map(|monitor| ding::Grid::new(monitor, prefs.size, gnome::mode(p.session == Session::X11))).collect();
            let grid = grids.iter().find(|grid| grid.belongs(x, y)).or(grids.first()).context("no DING grid")?;
            let (lx, ly) = grid.local(x, y);
            let column = ((lx / grid.cell_w).floor() as i64).clamp(0, grid.cols - 1);
            let row = ((ly / grid.cell_h).floor() as i64).clamp(0, grid.rows - 1);
            let (sx, sy) = grid.stored(column, row);
            gvfs::set(&path.clone().into_os_string(), ding::POSITION_KEY, Some(&format!("{sx},{sy}")))?;
            return Ok(format!("wrote {sx},{sy}, column {column} row {row}, to {name}"));
        }
        if p.session != Session::X11 {
            bail!("no way to move an icon without a person on this session");
        }
        let linux = p.linux()?;
        let current = probe_icon(p, &linux)?;
        let grab = grab_point(&current);
        let start = (grab.0 as i16, grab.1 as i16);
        let end = (target.0 as i16, target.1 as i16);
        drag(start, end)?;
        if let Some(measure) = p.measures.iter_mut().find(|measure| measure.name == name) {
            measure.adapter = Some(Rect { x: current.x + f32::from(end.0 - start.0), y: current.y + f32::from(end.1 - start.1), ..current });
        }
        Ok(format!("dragged from {},{} to {},{}", start.0, start.1, end.0, end.1))
    }

    fn aside(p: &Probe) -> Result<(f32, f32)> {
        let linux = p.linux()?;
        let current = probe_icon(p, &linux)?;
        let grab = grab_point(&current);
        Ok((grab.0 + 700.0 * factor(p), grab.1))
    }

    fn subject_on_screen(p: &Probe, linux: &Linux) -> Option<(f32, f32)> {
        let frame = primary_frame(p)?;
        let shot = capture(p.session, &frame).ok()?;
        let scale = factor(p);
        let skip: Vec<Rect> = icons(linux).iter().map(|rect| grown(&physical(rect, scale), 8.0)).collect();
        let band = 64.0 * scale;
        let (mut sx, mut sy, mut weight) = (0.0f64, 0.0f64, 0.0f64);
        for (x, y, pixel) in shot.enumerate_pixels() {
            let (fx, fy) = (x as f32, y as f32);
            if fy < band || fy >= frame.h - band {
                continue;
            }
            let (gx, gy) = (frame.x + fx, frame.y + fy);
            if skip.iter().any(|rect| gx >= rect.x && gx < rect.x + rect.w && gy >= rect.y && gy < rect.y + rect.h) {
                continue;
            }
            let [r, _, b] = pixel.0;
            let warmth = f64::from(r) - f64::from(b);
            if r > 150 && warmth > 60.0 {
                sx += f64::from(gx) * warmth;
                sy += f64::from(gy) * warmth;
                weight += warmth;
            }
        }
        (weight > 2000.0).then(|| ((sx / weight) as f32, (sy / weight) as f32))
    }

    fn events(p: &mut Probe) -> Result<()> {
        listen(p);
        act(p, "rest for 5 s", |_| {
            std::thread::sleep(Duration::from_secs(5));
            Ok("no input".into())
        });
        act(p, "move a probe icon", |p| {
            let target = aside(p)?;
            move_icon(p, target)
        });
        let fresh = p.desktop.join("scooch-probe-new.txt");
        let renamed = p.desktop.join("scooch-probe-renamed.txt");
        p.made.push(fresh.clone());
        p.made.push(renamed.clone());
        act(p, "create a file", |_| {
            fs::write(&fresh, b"new")?;
            Ok("scooch-probe-new.txt".into())
        });
        act(p, "rename a file", |_| {
            fs::rename(&fresh, &renamed)?;
            Ok("to scooch-probe-renamed.txt".into())
        });
        act(p, "delete a file", |_| {
            fs::remove_file(&renamed)?;
            Ok("scooch-probe-renamed.txt".into())
        });
        act(p, "create a hidden file", |p| {
            let hidden = p.desktop.join(".scooch-probe-hidden");
            p.made.push(hidden.clone());
            fs::write(&hidden, b"x")?;
            Ok(".scooch-probe-hidden".into())
        });
        Ok(())
    }

    fn photo(width: u32, height: u32) -> RgbImage {
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let (cx, cy, radius) = (SUBJECT.0 * w, SUBJECT.1 * h, SUBJECT.2 * h);
        let mut bytes = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                let (fx, fy) = (x as f32, y as f32);
                let sky = [30.0 + 50.0 * fy / h, 60.0 + 70.0 * fy / h, 140.0 - 60.0 * fy / h + 20.0 * (fx / w)];
                let distance = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() / radius;
                let glow = (1.0 - distance).clamp(0.0, 1.0).powf(0.6);
                let subject = [250.0, 196.0, 110.0];
                let pixel: Vec<u8> = sky.iter().zip(subject).map(|(base, lit)| (base + (lit - base) * glow).round().clamp(0.0, 255.0) as u8).collect();
                bytes.extend_from_slice(&pixel);
            }
        }
        RgbImage::from_raw(width, height, bytes).unwrap_or_default()
    }


    fn jpeg(picture: &RgbImage) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut out, 95);
        encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2);
        encoder.encode(picture.as_raw(), u16::try_from(picture.width())?, u16::try_from(picture.height())?, jpeg_encoder::ColorType::Rgb)?;
        Ok(out)
    }

    fn psnr(a: &RgbImage, b: &RgbImage, skip: &[Rect]) -> Option<f64> {
        if a.dimensions() != b.dimensions() {
            return None;
        }
        let (mut total, mut count) = (0.0f64, 0usize);
        for (x, y, pixel) in a.enumerate_pixels() {
            let (fx, fy) = (x as f32, y as f32);
            if skip.iter().any(|rect| fx >= rect.x - 8.0 && fx < rect.x + rect.w + 8.0 && fy >= rect.y - 8.0 && fy < rect.y + rect.h + 8.0) {
                continue;
            }
            let other = b.get_pixel(x, y);
            for (one, two) in pixel.0.iter().zip(other.0.iter()) {
                total += (f64::from(*one) - f64::from(*two)).powi(2);
            }
            count += 3;
        }
        let mse = total / count.max(1) as f64;
        Some(if mse == 0.0 { f64::INFINITY } else { 10.0 * (255.0f64 * 255.0 / mse).log10() })
    }

    fn matching(a: &RgbImage, b: &RgbImage, skip: &[Rect]) -> Option<f64> {
        if a.dimensions() != b.dimensions() {
            return None;
        }
        let (mut close, mut count) = (0usize, 0usize);
        for (x, y, pixel) in a.enumerate_pixels() {
            let (fx, fy) = (x as f32, y as f32);
            if skip.iter().any(|rect| fx >= rect.x - 8.0 && fx < rect.x + rect.w + 8.0 && fy >= rect.y - 8.0 && fy < rect.y + rect.h + 8.0) {
                continue;
            }
            count += 1;
            if pixel.0.iter().zip(b.get_pixel(x, y).0.iter()).all(|(one, two)| one.abs_diff(*two) <= 12) {
                close += 1;
            }
        }
        (count > 0).then(|| 100.0 * close as f64 / count as f64)
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
            Some(Kind::Kde) => kde::state().ok().and_then(|state| kde::containment(&state, screen).and_then(|desktop| desktop.id.parse::<i64>().ok())).map(|id| vec![(Box::new(Plasma) as Box<dyn record::Settings>, kde_rules::record_key(id))]).unwrap_or_default(),
            Some(Kind::Cinnamon) => ["org.cinnamon.desktop.background picture-uri", "org.cinnamon.desktop.background picture-options"].iter().map(|key| (Box::new(Gsettings) as Box<dyn record::Settings>, key.to_string())).collect(),
            Some(Kind::Mate) => ["org.mate.background picture-filename", "org.mate.background picture-options"].iter().map(|key| (Box::new(Gsettings) as Box<dyn record::Settings>, key.to_string())).collect(),
            _ => Vec::new(),
        }
    }

    fn wallpaper(p: &mut Probe) -> Result<()> {
        let linux = p.linux()?;
        let screen = linux.screens()?.first().cloned().context("no screen")?;
        for (store, key) in keys(p, &screen) {
            let value = store.get(&key).ok().flatten();
            p.line(format!("- recorded `{key}` = {value:?}"));
            p.recorded.push((key, value));
        }
        let (width, height) = ((screen.frame.w * screen.scale) as u32, (screen.frame.h * screen.scale) as u32);
        let source = photo(width, height);
        let jpeg_path = p.home.join("probe-picture.jpg");
        let png_path = p.home.join("probe-picture.png");
        fs::write(&jpeg_path, jpeg(&source)?)?;
        fs::write(&png_path, scooch_core::encode_png(&source)?)?;
        p.pictures = vec![jpeg_path.clone(), png_path.clone()];
        let scale = factor(p);
        let skip: Vec<Rect> = icons(&linux).iter().map(|rect| grown(&physical(rect, scale), 8.0)).collect();
        let physical_frame = primary_frame(p).unwrap_or(screen.frame);
        let mut bands = vec![Rect { x: 0.0, y: 0.0, w: width as f32, h: 64.0 * scale }, Rect { x: 0.0, y: height as f32 - 64.0 * scale, w: width as f32, h: 64.0 * scale }];
        bands.extend(skip.iter().map(|rect| Rect { x: rect.x - physical_frame.x, y: rect.y - physical_frame.y, ..*rect }));
        for (kind, path) in [("JPEG", jpeg_path), ("PNG", png_path)] {
            let file = scooch_core::open(&path)?;
            let started = Instant::now();
            let set = linux.set_wallpaper(&screen, &path);
            p.line(format!("- set the {kind} picture {:?} in {} ms", set.as_ref().map_err(|error| error.to_string()), started.elapsed().as_millis()));
            std::thread::sleep(Duration::from_secs(2));
            let back = linux.wallpaper(&screen);
            p.line(format!("  - read back {:?}, same path {}", back.as_ref().map_err(|error| error.to_string()), back.as_ref().is_ok_and(|back| back.as_deref() == Some(path.as_path()))));
            match capture(p.session, &physical_frame) {
                Ok(shot) => {
                    let _ = fs::write(p.out.join(format!("wallpaper-{}.png", kind.to_lowercase())), scooch_core::encode_png(&shot)?);
                    p.line(format!("  - screen {}x{} against the picture {}x{}, icons and panels left out, PSNR {:?}, pixels within 12 levels {:.1}%, file against the picture PSNR {:?}, file {} bytes", shot.width(), shot.height(), file.width(), file.height(), psnr(&shot, &file, &bands), matching(&shot, &file, &bands).unwrap_or(0.0), psnr(&file, &source, &[]), fs::metadata(&path).map_or(0, |meta| meta.len())));
                }
                Err(error) => p.line(format!("  - capture failed {error:#}")),
            }
            p.line(format!("  - record file {:?}", fs::read_to_string(record::file(&p.home)).ok()));
        }
        restore(p, &linux, &screen);
        Ok(())
    }

    fn restore(p: &mut Probe, linux: &Linux, screen: &Screen) {
        let original = p.originals.iter().find(|(seen, _)| seen.id == screen.id).and_then(|(_, path)| path.clone());
        match original {
            Some(path) => {
                let result = linux.set_wallpaper(screen, &path);
                p.line(format!("- restored to {} {:?}", path.display(), result.map_err(|error| error.to_string())));
            }
            None => {
                let desktop = match p.kind {
                    Some(Kind::Gnome) => "gnome",
                    Some(Kind::Kde) => "kde",
                    Some(Kind::Xfce) => "xfce",
                    Some(Kind::Cinnamon) => "cinnamon",
                    Some(Kind::Mate) => "mate",
                    None => "",
                };
                let store: Box<dyn record::Settings> = match p.kind {
                    Some(Kind::Xfce) => Box::new(Xfconf),
                    Some(Kind::Kde) => Box::new(Plasma),
                    _ => Box::new(Gsettings),
                };
                let result = record::restore(store.as_ref(), &record::file(&p.home), desktop);
                p.line(format!("- no original picture was recorded, restored the record directly {:?}", result.map_err(|error| error.to_string())));
            }
        }
        std::thread::sleep(Duration::from_secs(1));
        for (store, key) in keys(p, screen) {
            let now_value = store.get(&key).ok().flatten();
            let before = p.recorded.iter().find(|(seen, _)| *seen == key).map(|(_, value)| value.clone());
            p.line(format!("- `{key}` now {now_value:?}, equal to the recorded value {}", before.as_ref().is_some_and(|before| *before == now_value)));
        }
        p.line(format!("- record file left behind {}", record::file(&p.home).exists()));
    }

    fn proc_numbers(process: u32) -> Option<(u64, u64, u64)> {
        let stat = fs::read_to_string(format!("/proc/{process}/stat")).ok()?;
        let rest = stat.rsplit_once(')')?.1;
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
        let cpu = (fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?) * 1000 / ticks;
        let status = fs::read_to_string(format!("/proc/{process}/status")).ok()?;
        let field = |name: &str| status.lines().find_map(|line| line.strip_prefix(name)).and_then(|rest| rest.split_whitespace().next()).and_then(|value| value.parse::<u64>().ok());
        Some((cpu, field("VmRSS:")?, field("Threads:")?))
    }

    fn rendered(home: &Path) -> bool {
        let Ok(bytes) = fs::read(home.join("state.json")) else { return false };
        let Ok(state) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return false };
        state["screens"].as_object().is_some_and(|screens| !screens.is_empty() && screens.values().all(|screen| !screen["fingerprint"].is_null()))
    }

    fn end_to_end(p: &mut Probe) -> Result<()> {
        let linux = p.linux()?;
        let screen = linux.screens()?.first().cloned().context("no screen")?;
        let exe = std::env::current_exe()?;
        let program = exe.parent().and_then(Path::parent).map(|folder| folder.join("scooch")).filter(|path| path.is_file()).context("the scooch binary is missing, so build with --bins --examples")?;
        if let Some(picture) = p.pictures.first().cloned() {
            linux.set_wallpaper(&screen, &picture)?;
            p.line(format!("- wallpaper set to {}", picture.display()));
        }
        fs::write(p.home.join("state.json"), br#"{"version":1,"on":true}"#)?;
        let log = fs::File::create(p.out.join("watch.log"))?;
        let child = Command::new(&program).arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).spawn()?;
        let process = child.id();
        p.daemon = Some(child);
        p.line(format!("- started {} as process {process}", program.display()));
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(60), || rendered(&home));
        p.line(format!("- first render after {:?} ms", first.map(|time| time.as_millis())));
        let mut times = Vec::new();
        let frame = primary_frame(p).unwrap_or(screen.frame);
        for round in 1..=5 {
            let before = linux.wallpaper(&screen).ok().flatten();
            let rects_before = icons(&linux);
            let seen = subject_on_screen(p, &linux);
            let target = seen.unwrap_or((frame.x + SUBJECT.0 * frame.w, frame.y + SUBJECT.1 * frame.h));
            let moved = Instant::now();
            let detail = move_icon(p, target).unwrap_or_else(|error| format!("failed {error:#}"));
            let changed = wait_until(Duration::from_secs(10), || linux.wallpaper(&screen).ok().flatten() != before);
            let latency = changed.map(|_| moved.elapsed().as_millis());
            let moved_icons = icons(&linux) != rects_before;
            let place = seen.map_or("not found, so the picture's own spot".to_string(), |(x, y)| format!("{x:.0},{y:.0}"));
            p.line(format!("- move {round} onto the subject seen at {place}, {detail}, icons moved by the adapter's reading {moved_icons}, new wallpaper after {latency:?} ms"));
            times.extend(latency);
            std::thread::sleep(Duration::from_millis(800));
        }
        times.sort_unstable();
        p.line(format!("- median move to new wallpaper {:?} ms", times.get(times.len() / 2)));
        std::thread::sleep(Duration::from_secs(3));
        let before = proc_numbers(process);
        std::thread::sleep(Duration::from_secs(60));
        let after = proc_numbers(process);
        p.line(format!("- CPU ms, resident KB and threads before {before:?} and after {after:?} a quiet minute"));
        if let (Some(before), Some(after)) = (before, after) {
            p.line(format!("- CPU used while idle {} ms", after.0.saturating_sub(before.0)));
        }
        let stopped = unsafe { libc::kill(process as libc::pid_t, libc::SIGTERM) } == 0;
        let mut code = None;
        if let Some(child) = p.daemon.as_mut() {
            code = wait_until(Duration::from_secs(5), || matches!(child.try_wait(), Ok(Some(_)))).map(|_| "ended");
        }
        p.line(format!("- SIGTERM sent {stopped}, process {:?}", code.unwrap_or("still running")));
        if code.is_some() {
            p.daemon = None;
        }
        let trace = fs::read_to_string(p.out.join("watch.log")).unwrap_or_default();
        p.line(format!("- trace lines {}", trace.lines().count()));
        for line in trace.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev() {
            p.line(format!("  - `{line}`"));
        }
        Ok(())
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
        for (desktop, saved) in &leftover.desktops {
            p.line(format!("- a record was left for {desktop}, restoring it directly {saved:?}"));
            let store: Box<dyn record::Settings> = match desktop.as_str() {
                "xfce" => Box::new(Xfconf),
                "kde" => Box::new(Plasma),
                _ => Box::new(Gsettings),
            };
            p.line(format!("- direct restore {:?}", record::restore(store.as_ref(), &record::file(&p.home), desktop).map_err(|error| error.to_string())));
        }
        let removed = fs::remove_dir_all(&p.home);
        p.line(format!("- temp folder removed {:?}", removed.map_err(|error| error.to_string())));
        Ok(())
    }
}
