#[cfg(not(target_os = "linux"))]
fn main() {
    println!("linuxprobe runs on Linux only, so start it inside a Linux desktop session.");
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    probe::main()
}

#[cfg(any(target_os = "linux", test))]
const TRIES: usize = 3;

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
enum Footing {
    Proven,
    Again,
    Wrong,
    Unreached,
}

#[cfg(any(target_os = "linux", test))]
fn footing(tries: usize, reached: bool, held: bool) -> Footing {
    match (reached, held) {
        (true, true) => Footing::Proven,
        (true, false) => Footing::Wrong,
        (false, _) if tries < TRIES => Footing::Again,
        (false, _) => Footing::Unreached,
    }
}

#[cfg(any(target_os = "linux", test))]
fn untaught(plain: &[String], alone: bool) -> Option<&'static str> {
    if !plain.iter().any(|line| line.contains("predicted drop agreed")) {
        Some("the daemon showed no guess its net agreed with for the plain drag just before it, so it may not have known the selection or shown its guesses")
    } else if !alone {
        Some("the plain drag just before it did not move the probe icon alone with its release over the desktop")
    } else {
        None
    }
}

#[cfg(any(target_os = "linux", test))]
fn unfooted(plain: &[String], alone: bool, hook: &[String], released: f64, over: bool) -> Option<&'static str> {
    let keyed = hook.iter().filter(|line| line.contains(" drop key went down")).any(|line| line.split_whitespace().next().and_then(|stamp| stamp.parse::<f64>().ok()).is_none_or(|stamp| stamp <= released));
    untaught(plain, alone).or(if keyed {
        Some("a key went down during the drag")
    } else if !over {
        Some("its release was not seen over the desktop")
    } else {
        None
    })
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Footed,
    Late,
    Shadow,
}

#[cfg(any(target_os = "linux", test))]
fn footings(drags: &[(bool, bool)], window: usize) -> Vec<Drag> {
    let mut since: Option<usize> = None;
    drags
        .iter()
        .map(|&(moved, predicted)| {
            if !moved {
                since = Some(0);
                return Drag::Late;
            }
            match since {
                Some(count) if !predicted && count < window => {
                    since = Some(count + 1);
                    Drag::Shadow
                }
                _ => {
                    since = None;
                    Drag::Footed
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{footing, footings, unfooted, Drag, Footing, TRIES};

    fn lines(list: &[&str]) -> Vec<String> {
        list.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn a_hook_after_a_plain_drag_the_daemon_did_not_show_is_not_judged() {
        let plain = lines(&["1795301513.452 drop not predicted, the selection is unknown"]);
        let hook = lines(&["1795301516.451 drop drag began at 1914,303"]);
        let lacking = unfooted(&plain, true, &hook, 1795301517.9, true);
        assert!(lacking.is_some_and(|why| why.contains("showed no guess")));
    }

    #[test]
    fn a_hook_after_a_shown_plain_drag_its_net_agreed_with_is_footed() {
        let plain = lines(&["1795301492.415 predicted drop agreed with the file"]);
        let hook = lines(&["1795301516.451 drop drag began at 1914,303", "1795301522.004 drop key went down, desktop active true"]);
        assert_eq!(unfooted(&plain, true, &hook, 1795301517.9, true), None);
        assert!(unfooted(&plain, false, &hook, 1795301517.9, true).is_some_and(|why| why.contains("alone")));
        assert!(unfooted(&plain, true, &hook, 1795301517.9, false).is_some_and(|why| why.contains("release")));
    }

    #[test]
    fn a_key_before_the_release_leaves_the_hook_without_footing() {
        let plain = lines(&["1795301492.415 predicted drop agreed with the file"]);
        let hook = lines(&["1795301517.100 drop key went down, desktop active true"]);
        assert_eq!(unfooted(&plain, true, &hook, 1795301517.9, true), Some("a key went down during the drag"));
    }

    #[test]
    fn the_failed_run_reaches_ten_single_drags_with_footing_by_the_fifteenth() {
        let mut drags = vec![(true, false)];
        drags.extend([(true, true); 7]);
        drags.push((false, true));
        drags.extend([(true, false); 4]);
        drags.extend([(true, true); 2]);
        let marks = footings(&drags, 4);
        assert_eq!(marks[8], Drag::Late);
        assert_eq!(&marks[9..13], &[Drag::Shadow; 4]);
        assert_eq!(marks.iter().filter(|mark| **mark == Drag::Footed).count(), 10);
    }

    #[test]
    fn a_daemon_that_stays_off_after_a_late_save_is_counted_after_the_window() {
        let mut drags = vec![(false, true)];
        drags.extend([(true, false); 6]);
        assert_eq!(footings(&drags, 4), vec![Drag::Late, Drag::Shadow, Drag::Shadow, Drag::Shadow, Drag::Shadow, Drag::Footed, Drag::Footed]);
    }

    #[test]
    fn a_daemon_that_stops_guessing_with_every_save_seen_is_counted() {
        let drags = [(true, true), (true, false), (true, false), (true, false)];
        assert_eq!(footings(&drags, 4), vec![Drag::Footed; 4]);
        assert_eq!(footings(&[(false, false), (true, true), (true, false)], 4), vec![Drag::Late, Drag::Footed, Drag::Footed]);
    }

    #[test]
    fn a_release_over_the_desktop_with_the_right_answer_holds() {
        assert_eq!(footing(1, true, true), Footing::Proven);
        assert_eq!(footing(TRIES, true, true), Footing::Proven);
    }

    #[test]
    fn a_wrong_answer_after_a_release_elsewhere_is_tried_again() {
        assert_eq!(footing(1, false, false), Footing::Again);
        assert_eq!(footing(TRIES - 1, false, false), Footing::Again);
    }

    #[test]
    fn a_right_answer_after_a_release_elsewhere_proves_nothing() {
        assert_eq!(footing(1, false, true), Footing::Again);
        assert_eq!(footing(TRIES, false, true), Footing::Unreached);
    }

    #[test]
    fn three_tries_that_never_reach_the_desktop_fail_as_unreached() {
        assert_eq!(footing(TRIES, false, false), Footing::Unreached);
    }

    #[test]
    fn a_wrong_answer_over_the_desktop_fails_on_any_try() {
        assert_eq!(footing(1, true, false), Footing::Wrong);
        assert_eq!(footing(TRIES, true, false), Footing::Wrong);
    }
}

#[cfg(target_os = "linux")]
mod probe {
    use super::{footing, footings, unfooted, untaught, Drag, Footing, TRIES};
    use anyhow::{anyhow, bail, Context, Result};
    use scooch::platform::linux::parse::desktop::{Kind, Session};
    use scooch::platform::linux::parse::geometry::{intersect, Insets};
    use scooch::platform::linux::parse::{caja, ding, gio, gvariant, kde as kde_rules, keyfile, nemo, record, wakes, xfce as xfce_rules};
    use scooch::platform::linux::settings::{self, Gsettings, Plasma, Xfconf};
    use scooch::platform::linux::{bus, cinnamon, decode, drops, gnome, gvfs, kde, mate, screens, tool, xfce, Linux};
    use scooch::platform::{Desktop, Screen};
    use scooch_core::{Rect, RgbImage};
    use std::collections::{BTreeMap, VecDeque};
    use std::ffi::OsString;
    use std::fs;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    use x11rb::connection::Connection as _;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, ImageFormat, KeyButMask, PropMode, WindowClass, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT};
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
        failed: Vec<String>,
        soft: Vec<String>,
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
                self.failed.push(format!("{title} stopped with {error:#}"));
            }
            self.line(format!("- took {} ms", started.elapsed().as_millis()));
            self.save();
        }

        fn fail(&mut self, text: impl Into<String>) {
            let text = text.into();
            self.line(format!("- **missing** {text}"));
            self.failed.push(text);
        }

        fn kde_wayland(&self) -> bool {
            self.kind == Some(Kind::Kde) && self.session == Session::Wayland
        }

        fn xfce_x11(&self) -> bool {
            self.kind == Some(Kind::Xfce) && self.session == Session::X11
        }

        fn soft(&mut self, name: &str, reason: &str) {
            let text = format!("{name} on {}, {reason}", std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "no desktop".into()));
            self.line(format!("- **soft** {text}"));
            self.soft.push(text);
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
            failed: Vec::new(),
            soft: Vec::new(),
        };
        probe.section("1 Session facts", facts);
        probe.section("2 Read", read);
        probe.section("3 Geometry truth from pixels", geometry);
        probe.section("4 Move and events", events);
        probe.section("5 Wallpaper", wallpaper);
        probe.section("6 End to end", end_to_end);
        probe.section("7 Two monitors", monitors);
        probe.section("7b Desktop after the drop", after_drop);
        probe.section("7c The AppImage thrown away", thrown_away);
        probe.section("7d The final off", final_off_check);
        probe.section("8 Cleanup", cleanup);
        probe.line(String::new());
        probe.line("## Result".to_string());
        probe.line(String::new());
        if probe.failed.is_empty() {
            probe.line("- every check passed".to_string());
        }
        for failed in probe.failed.clone() {
            probe.line(format!("- **failed** {failed}"));
        }
        for soft in probe.soft.clone() {
            probe.line(format!("- soft, not a failure: {soft}"));
        }
        probe.save();
        if probe.failed.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
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
        let system = fs::read_to_string("/etc/os-release").ok().and_then(|text| text.lines().find_map(|line| line.strip_prefix("PRETTY_NAME=").map(|name| name.trim_matches('"').to_string()))).unwrap_or_default();
        p.line(format!("- system {system}"));
        p.line(format!("- desktop {:?}, version {}", p.kind, version(p.kind)));
        p.line(format!("- session {:?}, WAYLAND_DISPLAY {:?}, DISPLAY {:?}, XDG_SESSION_TYPE {:?}", p.session, std::env::var("WAYLAND_DISPLAY").ok(), std::env::var("DISPLAY").ok(), std::env::var("XDG_SESSION_TYPE").ok()));
        p.line(format!("- Desktop folder {}", p.desktop.display()));
        for line in fs::read_to_string(p.out.join("versions.txt")).unwrap_or_default().lines() {
            p.line(format!("- package {line}"));
        }
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
                    Err(error) => p.fail(format!("the adapter could not read the icons {error:#}")),
                }
            }
            Err(error) => p.fail(format!("the adapter for this desktop failed {error:#}")),
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
        p.line(format!("- screen mapping {:?}", kde::mapping()));
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
        if let Some(screen) = p.linux().ok().and_then(|linux| linux.screens().ok()).and_then(|screens| screens.first().cloned()) {
            return Some(physical(&screen.frame, screen.scale));
        }
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
            Some(Kind::Kde) => if kde::plasma6() { kde_rules::INSETS_6 } else { kde_rules::INSETS },
            Some(Kind::Xfce) => if xfce::version().is_some_and(|version| version >= (4, 19)) { xfce_rules::INSETS_4_20 } else { xfce_rules::INSETS },
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
        if p.kind.is_some_and(Kind::hears_saves) {
            let push = push.clone();
            std::thread::spawn(move || {
                let messages = match scooch::platform::linux::watch::monitor() {
                    Ok(messages) => messages,
                    Err(error) => {
                        push(format!("metadata monitor refused {error}"));
                        return;
                    }
                };
                for message in messages.map_while(Result::ok).filter(scooch::platform::linux::watch::is_save) {
                    let (signature, body, little) = scooch::platform::linux::watch::save_body(&message);
                    let file = wakes::saved_file(&signature, &body, little);
                    let hex: Vec<String> = body.iter().map(|byte| format!("{byte:02x}")).collect();
                    push(format!("metadata save {file:?}"));
                    if let Ok(mut kept) = heard().lock() {
                        kept.push(format!("{} ms, signature {signature}, little endian {little}, file {file:?}, body {}", now(), hex.concat()));
                    }
                }
            });
        }
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
        if p.session == Session::X11 {
            let push = push.clone();
            std::thread::spawn(move || {
                let release: screens::Shared = Arc::new(move || push("xinput primary release".into()));
                scooch::platform::linux::pointer::watch(&release, false)
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

    fn heard() -> &'static Mutex<Vec<String>> {
        static HEARD: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
        HEARD.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn heard_saves(p: &mut Probe) {
        if !p.kind.is_some_and(Kind::hears_saves) {
            return;
        }
        let kept = heard().lock().map(|kept| kept.clone()).unwrap_or_default();
        p.line(format!("- metadata saves heard so far by the probe's own bus monitor {}", kept.len()));
        for each in kept.iter().take(2) {
            p.line(format!("  - `{each}`"));
        }
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

    fn wander(p: &mut Probe) -> Result<String> {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).context("no root")?.root;
        let frame = primary_frame(p).context("no screen")?;
        for step in 0..40i32 {
            let x = frame.x + frame.w * (0.3 + 0.4 * (step as f32 / 40.0));
            let y = frame.y + frame.h * (0.4 + 0.2 * ((step % 10) as f32 / 10.0));
            conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, x as i16, y as i16, 0)?;
            conn.flush()?;
            std::thread::sleep(Duration::from_millis(25));
        }
        conn.sync()?;
        Ok("40 pointer steps across the screen, no button".into())
    }

    fn drag(from: (i16, i16), to: (i16, i16)) -> Result<()> {
        drag_ending(from, to, false)
    }

    fn drag_ending(from: (i16, i16), to: (i16, i16), escape: bool) -> Result<()> {
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
        if escape {
            let code = key_code(&conn, 0xff1b).context("no Escape key")?;
            conn.xtest_fake_input(KEY_PRESS_EVENT, code, 0, root, 0, 0, 0)?;
            conn.flush()?;
            pause(60);
            conn.xtest_fake_input(KEY_RELEASE_EVENT, code, 0, root, 0, 0, 0)?;
            conn.flush()?;
            pause(200);
        }
        conn.xtest_fake_input(BUTTON_RELEASE_EVENT, 1, 0, root, 0, 0, 0)?;
        conn.flush()?;
        conn.sync()?;
        Ok(())
    }

    fn key_code(conn: &x11rb::rust_connection::RustConnection, keysym: u32) -> Option<u8> {
        let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let map = conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
        let per = usize::from(map.keysyms_per_keycode).max(1);
        map.keysyms.chunks(per).position(|syms| syms.contains(&keysym)).and_then(|index| u8::try_from(usize::from(min) + index).ok())
    }

    fn control_code(conn: &x11rb::rust_connection::RustConnection) -> Option<u8> {
        key_code(conn, 0xffe3)
    }

    fn control_click(point: (i16, i16)) -> Result<()> {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).context("no root")?.root;
        let control = control_code(&conn).context("no Control key")?;
        let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
        conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, point.0, point.1, 0)?;
        conn.flush()?;
        pause(120);
        conn.xtest_fake_input(KEY_PRESS_EVENT, control, 0, root, 0, 0, 0)?;
        conn.flush()?;
        pause(80);
        conn.xtest_fake_input(BUTTON_PRESS_EVENT, 1, 0, root, 0, 0, 0)?;
        conn.flush()?;
        pause(60);
        conn.xtest_fake_input(BUTTON_RELEASE_EVENT, 1, 0, root, 0, 0, 0)?;
        conn.flush()?;
        pause(80);
        conn.xtest_fake_input(KEY_RELEASE_EVENT, control, 0, root, 0, 0, 0)?;
        conn.flush()?;
        conn.sync()?;
        pause(300);
        Ok(())
    }

    fn clicks(points: &[(i16, i16)]) -> Result<bool> {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).context("no root")?.root;
        let Some(control) = control_code(&conn) else { return Ok(false) };
        let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
        for (index, point) in points.iter().enumerate() {
            if index == 0 && points.len() > 1 {
                conn.xtest_fake_input(KEY_PRESS_EVENT, control, 0, root, 0, 0, 0)?;
                conn.flush()?;
                pause(80);
            }
            conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, point.0, point.1, 0)?;
            conn.flush()?;
            pause(120);
            conn.xtest_fake_input(BUTTON_PRESS_EVENT, 1, 0, root, 0, 0, 0)?;
            conn.flush()?;
            pause(60);
            conn.xtest_fake_input(BUTTON_RELEASE_EVENT, 1, 0, root, 0, 0, 0)?;
            conn.flush()?;
            pause(200);
        }
        if points.len() > 1 {
            conn.xtest_fake_input(KEY_RELEASE_EVENT, control, 0, root, 0, 0, 0)?;
            conn.flush()?;
        }
        conn.sync()?;
        pause(200);
        Ok(true)
    }

    const KDE_DROP_BAR: f64 = 500.0;
    const OFFSETS: [u64; 16] = [0, 20, 40, 60, 80, 120, 160, 200, 300, 400, 600, 800, 1000, 1300, 1600, 2000];
    const STEADY: Duration = Duration::from_millis(400);
    const TRUTH: Duration = Duration::from_millis(2500);
    const SINGLES: usize = 10;
    const MOST_SINGLES: usize = 16;
    const WINDOW: usize = scooch::platform::linux::parse::drops::SHADOWS + 1;
    const LIVE_UNTIL: Duration = Duration::from_millis(2300);
    const PREFIX: &str = "scooch-probe";

    type Rects = Vec<[i64; 4]>;
    type Named = Vec<(String, [i64; 4])>;
    type Node = (String, String);
    type Stamps = Arc<Mutex<Vec<(Instant, &'static str)>>>;

    struct Taken<T> {
        began: Duration,
        took: Duration,
        seen: T,
    }

    struct Live {
        bus: zbus::blocking::Connection,
        holder: Node,
    }

    struct Lattice {
        grids: Vec<(Rect, xfce_rules::Grid)>,
        insets: Insets,
        file: &'static str,
    }

    struct Bench<'a> {
        linux: &'a Linux,
        log: PathBuf,
        stamps: Stamps,
        live: Option<Live>,
        lattice: Option<Lattice>,
        steady: bool,
        scale: f32,
        plasma5: bool,
        pid: u32,
    }

    impl Bench<'_> {
        fn predicts(&self) -> bool {
            self.lattice.is_some() || self.plasma5
        }
    }

    struct Plan {
        group: bool,
        index: usize,
        before: Rects,
        moving: Rects,
        from: (i16, i16),
        to: (i16, i16),
    }

    #[derive(Default)]
    struct Tally {
        whole: [usize; 2],
        firsts: [Vec<u128>; 2],
        xinput: Vec<f64>,
        poll: Vec<f64>,
        over: usize,
        sets: Vec<f64>,
        partly: [usize; 2],
        rule: [[usize; 3]; 2],
        cost: Vec<u128>,
        predicted: [usize; 2],
        last_clicked: usize,
        pressed_only: usize,
        guessed: [usize; 2],
        held: [usize; 2],
        guessed_sets: Vec<f64>,
        live_firsts: Vec<u128>,
        made: [usize; 2],
        judged: [usize; 2],
        singles: Vec<(bool, bool)>,
    }

    fn enough(tally: &Tally, predicts: bool, made: usize) -> bool {
        if !predicts {
            return made >= SINGLES;
        }
        made >= MOST_SINGLES || footings(&tally.singles, WINDOW).iter().filter(|mark| **mark == Drag::Footed).count() >= SINGLES
    }

    fn after_drop(p: &mut Probe) -> Result<()> {
        if p.session != Session::X11 {
            p.line("- the probe drags with XTest, which a Wayland session does not take, so this section reads nothing here".to_string());
            return Ok(());
        }
        let linux = p.linux()?;
        if let Some(mut child) = p.daemon.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let screen = linux.screens()?.first().cloned().context("no screen")?;
        if let Some(picture) = p.pictures.first().cloned() {
            let cleared = fs::remove_file(record::file(&p.home)).is_ok();
            let set = linux.set_wallpaper(&screen, &picture).map_err(|error| format!("{error:#}"));
            let now_showing = linux.wallpaper(&screen).map_err(|error| format!("{error:#}"));
            p.line(format!("- the probe picture set on `{}` before the daemon starts, after the record of section 6's set was removed {cleared}, so the set cannot turn into putting back the original, {set:?}, the desktop now shows {now_showing:?}", screen.id));
        }
        let program = scooch_program()?;
        let process = start_daemon(p, &program, "drop.log")?;
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(60), || rendered(&home));
        p.line(format!("- the daemon {process} rendered after {:?} ms, its wakes and wallpaper sets are read from drop.log, and the probe reads the places itself through the adapter", first.map(|time| time.as_millis())));
        let stamps = releases();
        let kde = p.kind == Some(Kind::Kde);
        let live = if kde { open_live(p) } else { None };
        let lattice = if p.kind == Some(Kind::Xfce) { xfce_lattice(&linux) } else { None };
        if p.kind == Some(Kind::Xfce) {
            let described = lattice.as_ref().map_or_else(|| "nothing, since the probe could not rebuild the grid".to_string(), Lattice::describe);
            p.line(format!("- the XFCE prediction snaps to {described}"));
        }
        let steady = matches!(p.kind, Some(Kind::Cinnamon | Kind::Mate));
        let scale = factor(p);
        let frame = physical(&screen.frame, scale);
        let at = |x: f32, y: f32| ((frame.x + frame.w * x) as i16, (frame.y + frame.h * y) as i16);
        let plasma5 = kde && drops::applies(p.kind, p.session);
        if plasma5 {
            p.line("- the Plasma 5 prediction lands each single drag by the folder view rule, and the probe checks that rule against the places Plasma saves 2.5 s after the drop");
        }
        let bench = Bench { linux: &linux, log: p.out.join("drop.log"), stamps, live, lattice, steady, scale, plasma5, pid: process };
        let back = if steady { ", back to back for the first 400 ms on this desktop" } else { "" };
        p.line(format!("- ten drags of the probe icon, or where the daemon predicts up to sixteen until ten have footing, each to whichever of two spots lies farther from it, then five drags of the same three icons from the leftmost column, selected by three control clicks after a click on empty desktop, each by XTest, with reads at 0, 20, 40, 60, 80, 120, 160, 200, 300, 400, 600, 800, 1000, 1300, 1600 and 2000 ms after the drop{back}, against the places read 2.5 s after the drop, each read printed as began+took:moved places it shows"));
        let mut tally = Tally::default();
        let mut tracked = probe_icon(p, &linux).ok().map(|rect| ((rect.x + rect.w / 2.0) / scale, (rect.y + rect.h / 2.0) / scale));
        for index in 0..MOST_SINGLES {
            if enough(&tally, bench.predicts(), index) {
                break;
            }
            let before = places(&linux);
            let Some(icon) = tracked.and_then(|point| nearest(&before, point)) else {
                p.line(format!("  - single drag {}: the probe icon could not be found", index + 1));
                continue;
            };
            let from = grab(&icon, scale);
            let to = farther(from, [at(0.5, 0.5), at(0.35, 0.55)]);
            let changed = one_drag(p, &bench, &mut tally, Plan { group: false, index, before, moving: vec![icon], from, to });
            if let [moved] = changed.as_slice() {
                tracked = Some(center(moved));
            }
        }
        if bench.lattice.is_some() {
            if let Some(point) = tracked {
                specials(p, &bench, point, &at);
            }
        }
        if bench.predicts() {
            tracked = key_cases(p, &bench, tracked, &at);
        }
        if plasma5 {
            if let Some(point) = tracked {
                plasma_specials(p, &bench, point, &at);
            }
        }
        let mut group: Rects = Vec::new();
        for index in 0..5 {
            let _ = clicks(&[at(0.92, 0.92)]);
            let before = places(&linux);
            let mut chosen: Rects = if group.len() == 3 && group.iter().all(|rect| before.contains(rect)) { group.clone() } else { before.iter().filter(|rect| inside(&screen.frame, center(rect))).take(3).copied().collect() };
            chosen.sort_unstable();
            let points: Vec<(i16, i16)> = chosen.iter().map(|rect| grab(rect, scale)).collect();
            if points.len() < 3 || !clicks(&points).unwrap_or(false) {
                p.line(format!("  - group drag {}: three icons could not be selected", index + 1));
                continue;
            }
            let to = farther(points[0], [at(0.62, 0.25), at(0.85, 0.25)]);
            group = one_drag(p, &bench, &mut tally, Plan { group: true, index, before, moving: chosen, from: points[0], to });
        }
        let _ = clicks(&[at(0.92, 0.92)]);
        summary(p, &bench, &tally);
        heard_saves(p);
        if kde {
            let _ = screen_reader(false);
        }
        if let Some(mut child) = p.daemon.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }

    fn one_drag(p: &mut Probe, bench: &Bench, tally: &mut Tally, plan: Plan) -> Rects {
        let (kind, slot) = if plan.group { ("group", 1) } else { ("single", 0) };
        let log_mark = log_lines(&bench.log).len();
        let stamp_mark = bench.stamps.lock().map(|list| list.len()).unwrap_or(0);
        let live_before = bench.live.as_ref().map(Live::read);
        let boards = if bench.plasma5 && !plan.group { plasma_boards(bench.linux) } else { None };
        let sent = drag(plan.from, plan.to).is_ok();
        let dropped = Instant::now();
        let dropped_wall = wall();
        let read = || places(bench.linux);
        let (reads, lives) = std::thread::scope(|scope| {
            let watcher = bench.live.as_ref().map(|live| scope.spawn(move || live_series(live, dropped)));
            let reads = series(&read, dropped, bench.steady);
            (reads, watcher.and_then(|handle| handle.join().ok()).unwrap_or_default())
        });
        std::thread::sleep(TRUTH.saturating_sub(dropped.elapsed()));
        let last = places(bench.linux);
        let live_last = bench.live.as_ref().map(Live::read);
        let changed: Rects = last.iter().filter(|rect| !plan.before.contains(rect)).copied().collect();
        tally.made[slot] += 1;
        let first_final = reads.iter().find(|taken| !changed.is_empty() && taken.seen == last).map(|taken| taken.began.as_millis());
        if !changed.is_empty() && reads.first().is_some_and(|taken| taken.seen == last) {
            tally.whole[slot] += 1;
        }
        tally.firsts[slot].extend(first_final);
        let lines = log_lines(&bench.log);
        let (wakes, set) = daemon_after(lines.get(log_mark..).unwrap_or_default(), dropped_wall);
        tally.sets.extend(set);
        let seen: Vec<(Instant, &'static str)> = bench.stamps.lock().map(|list| list.get(stamp_mark..).map(<[_]>::to_vec).unwrap_or_default()).unwrap_or_default();
        let first_of = |name: &str| seen.iter().find(|(_, which)| *which == name).map(|(at, _)| since(*at, dropped));
        let (xinput, desktop, poll) = (first_of("xinput"), first_of("xinput desktop"), first_of("poll"));
        tally.xinput.extend(xinput);
        tally.poll.extend(poll);
        tally.over += usize::from(desktop.is_some());
        let woke = if wakes.is_empty() { "none".to_string() } else { wakes.join(", ") };
        p.line(format!(
            "  - {kind} drag {}: sent {sent}, from {},{} to {},{}, places that moved {}, first final read began at {} ms, XInput2 saw the release at {} ms and over the desktop at {} ms, the 10 ms poll at {} ms, wakes after the drop {woke}, the daemon set the wallpaper at {} ms",
            plan.index + 1,
            plan.from.0,
            plan.from.1,
            plan.to.0,
            plan.to.1,
            changed.len(),
            shown_ms(first_final.map(|ms| ms as f64)),
            shown_ms(xinput),
            shown_ms(desktop),
            shown_ms(poll),
            shown_ms(set)
        ));
        let shown: Vec<String> = reads.iter().map(|taken| format!("{}+{}:{}", taken.began.as_millis(), taken.took.as_millis(), changed.iter().filter(|rect| taken.seen.contains(rect)).count())).collect();
        p.line(format!("    - reads {}", shown.join(" ")));
        if let Some(boards) = boards {
            let start = logical(drag_start(plan.from, plan.to, drag_distance()), bench.scale);
            let said = match plasma_landing(&boards, logical(plan.from, bench.scale), start, logical(plan.to, bench.scale)) {
                Ok((_, to, rect)) => {
                    let mut expected: Rects = plan.before.iter().filter(|rect| !plan.moving.contains(rect)).copied().collect();
                    expected.push(rounded(&rect));
                    expected.sort_unstable();
                    let right = expected == last;
                    tally.predicted[slot] += usize::from(right);
                    format!("lands it in cell {to}, equal to the final places {right}")
                }
                Err(why) => format!("gives no landing since {why}"),
            };
            p.line(format!("    - the Plasma 5 landing rule from the press {said}"));
        }
        if bench.predicts() {
            let (guessed, verdict) = guessed(&wakes, lines.get(log_mark..).unwrap_or_default());
            tally.guessed[slot] += usize::from(guessed);
            if guessed && slot == 0 {
                tally.guessed_sets.extend(set);
            }
            let moved = !changed.is_empty();
            tally.judged[slot] += usize::from(guessed && moved);
            tally.held[slot] += usize::from(verdict == "agreed" && moved);
            let why = unguessed(lines.get(log_mark..).unwrap_or_default());
            p.line(format!("    - the daemon predicted a move {guessed}{why}, its net found {verdict}"));
            if slot == 0 {
                tally.singles.push((moved, guessed));
                let counted = match footings(&tally.singles, WINDOW).last() {
                    Some(Drag::Late) => "no place moved by 2.5 s after the drop, so the save came late or never and this drag is not counted",
                    Some(Drag::Shadow) => "the daemon did not predict it while it came back after a drop whose save came late, so it is not counted",
                    _ => "the probe saw the save, so this drag is counted",
                };
                p.line(format!("    - footing: {counted}"));
            }
        }
        if bench.steady {
            steady_lines(p, tally, &plan, &reads, &last, slot);
        }
        if let (Some(before), Some(after)) = (live_before, live_last) {
            let moved: Named = after.iter().filter(|item| !before.contains(item)).cloned().collect();
            let first_live = lives.iter().find(|taken| !moved.is_empty() && moved.iter().all(|item| taken.seen.contains(item)));
            tally.live_firsts.extend(first_live.map(|taken| taken.began.as_millis()));
            p.line(format!(
                "    - live reads of Plasma's accessibility tree {}, items that moved {}, first final live read began at {} ms and took {} ms",
                lives.len(),
                moved.len(),
                shown_ms(first_live.map(|taken| taken.began.as_millis() as f64)),
                shown_ms(first_live.map(|taken| taken.took.as_millis() as f64))
            ));
        }
        if let Some(lattice) = bench.lattice.as_ref() {
            let to = (f32::from(plan.to.0) / bench.scale, f32::from(plan.to.1) / bench.scale);
            let pressed = plan.moving.first().copied();
            let full = pressed.and_then(|anchor| lattice.predict(&plan.before, &plan.moving, anchor, to));
            let right = full.as_deref() == Some(last.as_slice());
            tally.predicted[slot] += usize::from(right);
            let wrong: Rects = full.as_ref().map(|full| full.iter().filter(|rect| !last.contains(rect)).copied().collect::<Rects>()).unwrap_or_default();
            if plan.group {
                let clicked = plan.moving.last().copied().and_then(|anchor| lattice.predict(&plan.before, &plan.moving, anchor, to));
                let by_last = clicked.as_deref() == Some(last.as_slice());
                tally.last_clicked += usize::from(by_last);
                let alone = pressed.and_then(|anchor| lattice.predict(&plan.before, &[anchor], anchor, to));
                let misses = alone.as_ref().map_or(changed.len(), |alone| last.iter().filter(|rect| !alone.contains(rect)).count());
                tally.pressed_only += usize::from(misses == 0);
                p.line(format!("    - prediction from the press anchored at the pressed icon equal to the final places {right}, anchored at the last clicked icon {by_last}, knowing only the pressed icon it misses {misses} of the {} moved places, wrong places {wrong:?}", changed.len()));
            } else {
                p.line(format!("    - prediction from the press equal to the final places {right}, wrong places {wrong:?}"));
            }
        }
        changed
    }

    fn quick_limit(p: &Probe) -> Option<&'static str> {
        match p.kind {
            Some(kind) if kind.hears_saves() => Some("a desktop that saves places through gvfs"),
            Some(Kind::Kde) if p.session == Session::X11 && kde::plasma6() => Some("Plasma 6 with the pointer wake"),
            _ => None,
        }
    }

    fn set_limit(p: &mut Probe, what: &str, sets: &[f64], limit: f64) {
        match median(sets) {
            None => p.fail(format!("{what} measured no time from the drop to the wallpaper set, so the limit of {limit:.0} ms cannot be checked")),
            Some(ms) if ms >= limit => p.fail(format!("{what} took a median {ms:.0} ms from the drop to the wallpaper set, over the limit of {limit:.0} ms")),
            Some(_) => {}
        }
    }

    fn unguessed(lines: &[String]) -> String {
        lines.iter().find_map(|line| line.split_once("drop not predicted, ").map(|(_, why)| format!(", not predicted since {why}"))).unwrap_or_default()
    }

    fn guessed(wakes: &[String], lines: &[String]) -> (bool, &'static str) {
        let guessed = wakes.iter().any(|wake| wake.starts_with("predicted drop "));
        let said = [("predicted drop agreed", "agreed"), ("predicted drop missed", "missed")];
        let verdict = lines.iter().find_map(|line| said.iter().find(|(needle, _)| line.contains(needle)).map(|(_, verdict)| *verdict)).unwrap_or("nothing to judge");
        (guessed, verdict)
    }

    fn special(p: &mut Probe, bench: &Bench, name: &str, act: impl FnOnce() -> Result<String>) -> (bool, usize, bool) {
        let before = places(bench.linux);
        let mark = log_lines(&bench.log).len();
        let detail = act().unwrap_or_else(|error| format!("failed with {error:#}"));
        let dropped = wall();
        std::thread::sleep(TRUTH);
        let after = places(bench.linux);
        let lines = log_lines(&bench.log);
        let fresh = lines.get(mark..).unwrap_or_default();
        let (wakes, set) = daemon_after(fresh, dropped);
        let sets = fresh.iter().filter(|line| line.split_whitespace().nth(1) == Some("set")).count();
        let (guessed, verdict) = guessed(&wakes, fresh);
        let woke = if wakes.is_empty() { "none".to_string() } else { wakes.join(", ") };
        let why = unguessed(fresh);
        p.line(format!("  - {name}: {detail}, the daemon predicted a move {guessed}{why}, its net found {verdict}, wakes after the drop {woke}, the daemon set the wallpaper {sets} times, first at {} ms, places unchanged 2.5 s after the drop {}", shown_ms(set), after == before));
        (guessed, sets, after == before)
    }

    fn untouched(name: &str, (guessed, sets, unchanged): (bool, usize, bool), moves: bool) -> Option<String> {
        if guessed {
            Some(format!("the daemon predicted the {name}, which it must never do"))
        } else if unchanged && sets != 0 {
            Some(format!("the {name} changed no place and yet the daemon set the wallpaper {sets} times"))
        } else if !unchanged && !moves {
            Some(format!("the {name} changed the places, where it must change nothing"))
        } else {
            None
        }
    }

    fn stamps_seen(bench: &Bench) -> usize {
        bench.stamps.lock().map(|list| list.len()).unwrap_or(0)
    }

    fn over_desktop_since(bench: &Bench, mark: usize) -> bool {
        bench.stamps.lock().is_ok_and(|list| list.iter().skip(mark).any(|(_, which)| *which == "xinput desktop"))
    }

    fn footed(p: &mut Probe, name: &str, mut attempt: impl FnMut(&mut Probe, usize) -> Option<(bool, bool)>) -> Option<(Footing, usize)> {
        for tries in 1..=TRIES {
            let (reached, held) = attempt(p, tries)?;
            let step = footing(tries, reached, held);
            let next = match step {
                Footing::Proven => "the check holds",
                Footing::Wrong => "the daemon's answer is wrong",
                Footing::Again => "the check has not run and the probe tries again",
                Footing::Unreached => "the footing was never proven",
            };
            p.line(format!("    - {name}, try {tries} of {TRIES}: its footing proven {reached}, the daemon answered as it must {held}, so {next}"));
            if step != Footing::Again {
                return Some((step, tries));
            }
        }
        None
    }

    fn unreached(name: &str) -> String {
        format!("{name} never ended over the desktop in {TRIES} tries, so the daemon was not judged on it")
    }

    fn specials(p: &mut Probe, bench: &Bench, point: (f32, f32), at: &dyn Fn(f32, f32) -> (i16, i16)) {
        let scale = bench.scale;
        let before = places(bench.linux);
        let Some(icon) = nearest(&before, point) else { return };
        let plain = p.measures.iter().find(|measure| measure.name == PROBES[2].0).and_then(|measure| measure.adapter).map(|rect| ((rect.x + rect.w / 2.0) / scale, (rect.y + rect.h / 2.0) / scale));
        let Some(other) = plain.and_then(|spot| nearest(&before, spot)).filter(|rect| *rect != icon) else {
            p.line(format!("  - no plain file icon to drop onto, {} was not found", PROBES[2].0));
            return;
        };
        let (from, onto) = (grab(&icon, scale), grab(&other, scale));
        let _ = clicks(&[from]);
        special(p, bench, "drag onto a taken slot, which gives no guess", || {
            drag(from, onto)?;
            Ok(format!("the probe icon from {},{} onto {} at {},{}", from.0, from.1, PROBES[2].0, onto.0, onto.1))
        });
        let (mut start, mut failure, mut guessed) = (from, None, None);
        let control = footed(p, "drag after a Control click", |p, _| {
            let (from, free) = (start, farther(start, [at(0.5, 0.5), at(0.35, 0.55)]));
            let before = places(bench.linux);
            let _ = clicks(&[from]);
            let mark = std::cell::Cell::new(stamps_seen(bench));
            let seen = special(p, bench, "drag after a Control click", || {
                control_click(onto)?;
                mark.set(stamps_seen(bench));
                drag(from, free)?;
                Ok(format!("a Control click on the icon at {},{}, then the probe icon from {},{} to {},{}", onto.0, onto.1, from.0, from.1, free.0, free.1))
            });
            let _ = clicks(&[at(0.92, 0.92)]);
            let now = places(bench.linux);
            if now != before {
                start = nearest(&now, (f32::from(free.0) / scale, f32::from(free.1) / scale)).map_or(from, |icon| grab(&icon, scale));
            }
            failure = untouched("drag after a Control click", seen, true);
            if seen.0 {
                guessed.clone_from(&failure);
            }
            Some((over_desktop_since(bench, mark.get()), failure.is_none()))
        });
        match (guessed, control) {
            (Some(guess), _) => p.fail(guess),
            (None, Some((Footing::Wrong, tries))) => p.fail(format!("{}, with its release over the desktop on try {tries}", failure.unwrap_or_default())),
            (None, Some((Footing::Unreached, _))) => p.fail(unreached("the drag after a Control click")),
            _ => {}
        }
        let from = start;
        let _ = clicks(&[from]);
        let free = farther(from, [at(0.5, 0.5), at(0.35, 0.55)]);
        let escape = special(p, bench, "drag ended with Escape", || {
            drag_ending(from, free, true)?;
            Ok(format!("the probe icon from {},{} toward {},{}, Escape before the release", from.0, from.1, free.0, free.1))
        });
        let _ = clicks(&[at(0.92, 0.92)]);
        if let Some(failure) = untouched("drag ended with Escape", escape, false) {
            p.fail(failure);
        }
    }

    fn plasma_boards(linux: &Linux) -> Option<Vec<kde::Board>> {
        let shell = kde::Kde { desktop: linux.desktop.clone(), config: kde::config_folder(), session: linux.session };
        let found = linux.all_screens().ok()?;
        let state = kde::state().ok()?;
        let screens: Vec<Screen> = found.into_iter().map(|found| found.screen).collect();
        Some(shell.boards(&state, &screens, &kde::mapping()))
    }

    fn plasma_board(boards: &[kde::Board], press: (f32, f32)) -> Result<(&kde::Board, i64), &'static str> {
        let board = boards.iter().find(|board| board.grid.cell_at(press).is_some()).ok_or("the press was off Plasma's icons")?;
        match board.grid.spot(press) {
            kde_rules::Spot::Icon(from) => Ok((board, from)),
            _ => Err("the press was not on an icon"),
        }
    }

    fn plasma_landing(boards: &[kde::Board], press: (f32, f32), start: (f32, f32), release: (f32, f32)) -> Result<(i64, i64, Rect), &'static str> {
        let (board, from) = plasma_board(boards, press)?;
        if let Some(why) = board.why_not {
            return Err(why);
        }
        let to = board.grid.landing(from, start, release)?;
        Ok((from, to, board.grid.icon(to)))
    }

    fn drag_distance() -> f32 {
        kde_rules::drag_distance(&kde::groups("kdeglobals"))
    }

    fn drag_start(from: (i16, i16), to: (i16, i16), distance: f32) -> (i16, i16) {
        (1..=20i32)
            .map(|step| ((from.0 as i32 + (to.0 as i32 - from.0 as i32) * step / 20) as i16, (from.1 as i32 + (to.1 as i32 - from.1 as i32) * step / 20) as i16))
            .find(|point| f32::from((point.0 - from.0).abs() + (point.1 - from.1).abs()) >= distance)
            .unwrap_or(to)
    }

    fn logical(point: (i16, i16), scale: f32) -> (f32, f32) {
        (f32::from(point.0) / scale, f32::from(point.1) / scale)
    }

    fn set_times(lines: &[String], dropped: f64) -> Vec<f64> {
        lines.iter().filter_map(|line| {
            let mut words = line.split_whitespace();
            let stamp = words.next()?.parse::<f64>().ok()?;
            (words.next() == Some("set")).then_some((stamp - dropped) * 1000.0)
        }).collect()
    }

    fn drops_switches(pid: u32) -> Option<u64> {
        threads_of(pid).iter().find(|thread| thread.name == "scooch-drops").map(|thread| thread.voluntary + thread.involuntary)
    }

    fn intern(conn: &x11rb::rust_connection::RustConnection, name: &[u8]) -> Result<u32> {
        Ok(conn.intern_atom(false, name)?.reply()?.atom)
    }

    fn active_window(conn: &x11rb::rust_connection::RustConnection, root: u32) -> Option<u32> {
        let atom = intern(conn, b"_NET_ACTIVE_WINDOW").ok()?;
        conn.get_property(false, root, atom, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?.value32()?.next()
    }

    fn desktop_active() -> Result<bool> {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).context("no root")?.root;
        let Some(window) = active_window(&conn, root) else { return Ok(false) };
        let (kind, desktop) = (intern(&conn, b"_NET_WM_WINDOW_TYPE")?, intern(&conn, b"_NET_WM_WINDOW_TYPE_DESKTOP")?);
        let types = conn.get_property(false, window, kind, AtomEnum::ATOM, 0, 16)?.reply()?;
        Ok(types.value32().is_some_and(|mut values| values.any(|value| value == desktop)))
    }

    fn typed_elsewhere(pid: u32) -> Result<(bool, Option<u64>, Option<u64>)> {
        let (conn, number) = x11rb::connect(None)?;
        let screen = conn.setup().roots.get(number).context("no root")?.clone();
        let window = conn.generate_id()?;
        conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, screen.root, 240, 200, 480, 240, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new().background_pixel(screen.white_pixel).event_mask(EventMask::KEY_PRESS))?;
        conn.change_property8(PropMode::REPLACE, window, AtomEnum::WM_NAME, AtomEnum::STRING, b"scooch probe typing")?;
        conn.map_window(window)?;
        conn.flush()?;
        std::thread::sleep(Duration::from_millis(300));
        let message = ClientMessageEvent::new(32, window, intern(&conn, b"_NET_ACTIVE_WINDOW")?, [2, 0, 0, 0, 0]);
        conn.send_event(false, screen.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, message)?;
        conn.flush()?;
        let active = wait_until(Duration::from_secs(3), || active_window(&conn, screen.root) == Some(window)).is_some();
        std::thread::sleep(Duration::from_millis(600));
        let code = key_code(&conn, 0x61).context("no key for the letter a")?;
        conn.xtest_fake_input(KEY_PRESS_EVENT, code, 0, screen.root, 0, 0, 0)?;
        conn.xtest_fake_input(KEY_RELEASE_EVENT, code, 0, screen.root, 0, 0, 0)?;
        conn.sync()?;
        std::thread::sleep(Duration::from_millis(400));
        let before = drops_switches(pid);
        for _ in 0..50 {
            conn.xtest_fake_input(KEY_PRESS_EVENT, code, 0, screen.root, 0, 0, 0)?;
            conn.flush()?;
            std::thread::sleep(Duration::from_millis(15));
            conn.xtest_fake_input(KEY_RELEASE_EVENT, code, 0, screen.root, 0, 0, 0)?;
            conn.flush()?;
            std::thread::sleep(Duration::from_millis(15));
        }
        conn.sync()?;
        std::thread::sleep(Duration::from_millis(400));
        let after = drops_switches(pid);
        conn.destroy_window(window)?;
        conn.flush()?;
        std::thread::sleep(Duration::from_millis(300));
        Ok((active, before, after))
    }

    fn tap_shift() -> Result<()> {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).context("no root")?.root;
        let code = key_code(&conn, 0xffe1).context("no Shift key")?;
        conn.xtest_fake_input(KEY_PRESS_EVENT, code, 0, root, 0, 0, 0)?;
        conn.flush()?;
        std::thread::sleep(Duration::from_millis(60));
        conn.xtest_fake_input(KEY_RELEASE_EVENT, code, 0, root, 0, 0, 0)?;
        conn.sync()?;
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    }

    fn key_lines(lines: &[String]) -> Vec<String> {
        lines.iter().filter(|line| line.contains(" drop key") || line.contains(" drop thread woke")).cloned().collect()
    }

    fn key_cases(p: &mut Probe, bench: &Bench, tracked: Option<(f32, f32)>, at: &dyn Fn(f32, f32) -> (i16, i16)) -> Option<(f32, f32)> {
        let mark = log_lines(&bench.log).len();
        match typed_elsewhere(bench.pid) {
            Ok((active, before, after)) => {
                let switched = before.zip(after).map(|(before, after)| after.saturating_sub(before));
                p.line(format!("  - 50 keys typed into a window of the probe's own, which the window manager made the active window {active}, after one key that makes the X server's test keyboard the one in use, the scooch-drops thread switched {switched:?} times while they went in"));
                match switched {
                    None => p.fail("the scooch-drops thread could not be read while keys went to another window"),
                    Some(0) => {}
                    Some(count) => p.fail(format!("the scooch-drops thread woke {count} times while 50 keys went to another window, where it must not wake at all")),
                }
                if !active {
                    p.soft("keys typed into another window", "the window manager did not make the probe's window the active window");
                }
            }
            Err(error) => p.fail(format!("keys could not be typed into another window, {error:#}")),
        }
        let lines = log_lines(&bench.log);
        p.line(format!("  - drop.log lines about keys while the keys went elsewhere {:?}", key_lines(lines.get(mark..).unwrap_or_default())));
        let (mut tracked, mut predicted, mut why) = (tracked, false, String::new());
        let step = footed(p, "a drag after Shift went down with the desktop active", |p, _| {
            let _ = clicks(&[at(0.92, 0.92)]);
            let on_desktop = desktop_active().unwrap_or(false);
            let before = places(bench.linux);
            let icon = tracked.and_then(|point| nearest(&before, point))?;
            let from = grab(&icon, bench.scale);
            let to = farther(from, [at(0.5, 0.5), at(0.35, 0.55)]);
            let mark = log_lines(&bench.log).len();
            let stamp = std::cell::Cell::new(stamps_seen(bench));
            let typed = special(p, bench, "a drag after Shift went down with the desktop active", || {
                tap_shift()?;
                stamp.set(stamps_seen(bench));
                drag(from, to)?;
                Ok(format!("Shift pressed and let go with the desktop the active window {on_desktop}, then the probe icon from {},{} to {},{}", from.0, from.1, to.0, to.1))
            });
            let lines = log_lines(&bench.log);
            let fresh = lines.get(mark..).unwrap_or_default();
            p.line(format!("  - drop.log lines about keys around that drag {:?}", key_lines(fresh)));
            why = unguessed(fresh);
            predicted |= typed.0;
            let after = places(bench.linux);
            tracked = after.iter().find(|rect| !before.contains(rect)).map(center).or(tracked);
            Some((on_desktop && over_desktop_since(bench, stamp.get()), !typed.0 && why.contains("the selection may hold more than one icon")))
        });
        match (predicted, step) {
            (true, _) => p.fail("the daemon predicted a drag after a key went down on the active desktop, where the selection may hold more than one icon"),
            (false, Some((Footing::Wrong, tries))) => p.fail(format!("a key on the active desktop did not make the selection unknown to the daemon{why}, with the desktop active and the release over the desktop on try {tries}")),
            (false, Some((Footing::Unreached, _))) => p.fail(format!("the drag after a key never had the desktop active and its release over the desktop in {TRIES} tries, so the daemon was not judged on a key on the active desktop")),
            _ => {}
        }
        step?;
        tracked
    }

    fn walked(grid: &kde_rules::Grid, from: i64, start: i64) -> Option<i64> {
        (start..start + 64).find(|index| *index == from || !grid.taken.contains(index)).filter(|index| *index != from)
    }

    fn by_center(area: &Rect, points: impl Iterator<Item = (f32, f32)>) -> Vec<(f32, f32)> {
        let middle = (area.x + area.w / 2.0, area.y + area.h / 2.0);
        let mut points: Vec<(f32, f32)> = points.collect();
        points.sort_by(|a, b| (a.0 - middle.0).hypot(a.1 - middle.1).total_cmp(&(b.0 - middle.0).hypot(b.1 - middle.1)));
        points
    }

    fn lattice_points(area: &Rect) -> impl Iterator<Item = (f32, f32)> + '_ {
        let (columns, rows) = (((area.w - 48.0) / 8.0) as i32, ((area.h - 48.0) / 8.0) as i32);
        (0..rows.max(0)).flat_map(move |row| (0..columns.max(0)).map(move |column| (area.x + 24.0 + 8.0 * column as f32, area.y + 24.0 + 8.0 * row as f32)))
    }

    fn taken_drop(boards: &[kde::Board], press: (f32, f32), scale: f32) -> Option<((f32, f32), i64, i64)> {
        let (board, from) = plasma_board(boards, press).ok()?;
        let grid = &board.grid;
        let distance = drag_distance();
        by_center(&grid.area, lattice_points(&grid.area)).into_iter().find_map(|point| {
            let start = logical(drag_start(physical_point(press, scale), physical_point(point, scale), distance), scale);
            let landing = grid.target(from, start, point).ok()?;
            let pointer = grid.cell_at(point)?;
            let usable = grid.landing(from, start, point) == Err("the landing cell is taken") && pointer != from && !grid.taken.contains(&pointer);
            usable.then(|| walked(grid, from, landing).map(|walk| (point, landing, walk))).flatten()
        })
    }

    type Hook = ((f32, f32), (f32, f32), i64, i64);

    fn hook_plan(boards: &[kde::Board], press: (f32, f32)) -> Option<Hook> {
        let (board, from) = plasma_board(boards, press).ok()?;
        let grid = &board.grid;
        let (_, tall) = grid.size();
        let jump = (press.0, press.1 - tall as f32);
        grid.cell_at(jump)?;
        by_center(&grid.area, lattice_points(&grid.area)).into_iter().find_map(|point| {
            let hooked = grid.landing(from, jump, point).ok()?;
            let straight = grid.target(from, press, point).ok()?;
            (straight != hooked).then_some((jump, point, hooked, straight))
        })
    }

    fn hook_drag(from: (i16, i16), jump: (i16, i16), to: (i16, i16)) -> Result<()> {
        let (conn, number) = x11rb::connect(None)?;
        let root = conn.setup().roots.get(number).context("no root")?.root;
        let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
        conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, from.0, from.1, 0)?;
        conn.flush()?;
        pause(200);
        conn.xtest_fake_input(BUTTON_PRESS_EVENT, 1, 0, root, 0, 0, 0)?;
        conn.flush()?;
        pause(200);
        conn.xtest_fake_input(MOTION_NOTIFY_EVENT, 0, 0, root, jump.0, jump.1, 0)?;
        conn.flush()?;
        pause(150);
        for step in 1..=20i32 {
            let x = jump.0 as i32 + (to.0 as i32 - jump.0 as i32) * step / 20;
            let y = jump.1 as i32 + (to.1 as i32 - jump.1 as i32) * step / 20;
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

    fn physical_point(point: (f32, f32), scale: f32) -> (i16, i16) {
        ((point.0 * scale) as i16, (point.1 * scale) as i16)
    }

    struct Plain {
        at: (i16, i16),
        over: bool,
        alone: bool,
        lines: Vec<String>,
    }

    fn learning_drag(p: &mut Probe, bench: &Bench, from: (i16, i16), at: &dyn Fn(f32, f32) -> (i16, i16)) -> Plain {
        let scale = bench.scale;
        let free = farther(from, [at(0.5, 0.5), at(0.35, 0.55)]);
        let before = places(bench.linux);
        let stamp = stamps_seen(bench);
        let mark = log_lines(&bench.log).len();
        special(p, bench, "a plain drag so the daemon learns the selection again", || {
            drag(from, free)?;
            Ok(format!("the probe icon from {},{} to {},{}", from.0, from.1, free.0, free.1))
        });
        let now = places(bench.linux);
        let lines = log_lines(&bench.log).get(mark..).map(<[String]>::to_vec).unwrap_or_default();
        let alone = now.iter().filter(|rect| !before.contains(rect)).count() == 1 && before.iter().filter(|rect| !now.contains(rect)).count() == 1;
        let moved = if now == before { from } else { nearest(&now, (f32::from(free.0) / scale, f32::from(free.1) / scale)).map_or(from, |icon| grab(&icon, scale)) };
        Plain { at: moved, over: over_desktop_since(bench, stamp), alone, lines }
    }

    fn moved_from(before: &Rects, now: &Rects, from: (i16, i16), scale: f32) -> (i16, i16) {
        now.iter().find(|rect| !before.contains(rect)).map_or(from, |icon| grab(icon, scale))
    }

    fn plasma_specials(p: &mut Probe, bench: &Bench, point: (f32, f32), at: &dyn Fn(f32, f32) -> (i16, i16)) {
        let scale = bench.scale;
        let Some(icon) = nearest(&places(bench.linux), point) else { return };
        let mut start = grab(&icon, scale);
        let (mut predicted, mut seen, mut why, mut sets) = (false, (false, 0, true), String::new(), Vec::new());
        let taken = footed(p, "a drop whose landing cell is taken", |p, tries| {
            let learned = if tries > 1 {
                let plain = learning_drag(p, bench, start, at);
                start = plain.at;
                plain.over
            } else {
                true
            };
            let from = start;
            let press = (f32::from(from.0) / scale, f32::from(from.1) / scale);
            let Some(boards) = plasma_boards(bench.linux) else {
                p.fail("Plasma's grid could not be read for the drop on a taken cell");
                return None;
            };
            let Some((spot, landing, walk)) = taken_drop(&boards, press, scale) else {
                p.fail("no release point lands the probe icon on a taken cell with a free cell under the pointer");
                return None;
            };
            let onto = physical_point(spot, scale);
            let before = places(bench.linux);
            let mark = log_lines(&bench.log).len();
            let stamp = stamps_seen(bench);
            let released = std::cell::Cell::new(wall());
            seen = special(p, bench, "a drop whose landing cell is taken, which gives no guess and follows the save", || {
                drag(from, onto)?;
                released.set(wall());
                Ok(format!("the probe icon from {},{} to {},{}, where Plasma's rule lands it on the taken cell {landing} and walks it on to cell {walk}", from.0, from.1, onto.0, onto.1))
            });
            let fresh = log_lines(&bench.log);
            sets = set_times(fresh.get(mark..).unwrap_or_default(), released.get());
            why = unguessed(fresh.get(mark..).unwrap_or_default());
            predicted |= seen.0;
            start = moved_from(&before, &places(bench.linux), from, scale);
            Some((learned && over_desktop_since(bench, stamp), !seen.0 && why.contains("the landing cell is taken")))
        });
        let Some((step, tries)) = taken else { return };
        if predicted {
            p.fail("the daemon predicted a drop whose landing cell is taken, which it must never do");
        } else if step == Footing::Wrong {
            p.fail(format!("the drop on a taken cell was not refused for its taken cell{why}, with its release over the desktop on try {tries}"));
        } else if step == Footing::Unreached {
            p.fail(unreached("the drop on a taken cell"));
        } else if seen.2 {
            p.fail("the drop on a taken cell moved no icon, so it could not show the save being followed");
        } else if !sets.first().is_some_and(|first| *first >= 1500.0) {
            p.fail(format!("the drop on a taken cell set the wallpaper at {sets:?} ms after the drop, where it must follow Plasma's save at about 2 s"));
        }
        let (mut missed, mut lacked) = (String::new(), "");
        let hook = footed(p, "a hook drag whose first motion leaves the icon's row", |p, _| {
            let mut plain = learning_drag(p, bench, start, at);
            for _ in 0..WINDOW {
                if untaught(&plain.lines, plain.alone && plain.over).is_none() {
                    break;
                }
                plain = learning_drag(p, bench, plain.at, at);
            }
            start = plain.at;
            let from = start;
            let press = (f32::from(from.0) / scale, f32::from(from.1) / scale);
            let plan = plasma_boards(bench.linux).and_then(|boards| hook_plan(&boards, press).map(|plan| (boards, plan)));
            let Some((boards, (jump, release, hooked, straight))) = plan else {
                p.fail("no release point gives the hook drag a landing apart from a straight drag");
                return None;
            };
            let (board, _) = plasma_board(&boards, press).ok()?;
            let (hooked_rect, straight_rect) = (rounded(&board.grid.icon(hooked)), rounded(&board.grid.icon(straight)));
            let (jump_at, release_at) = (physical_point(jump, scale), physical_point(release, scale));
            let before = places(bench.linux);
            let mark = log_lines(&bench.log).len();
            let stamp = stamps_seen(bench);
            let released = std::cell::Cell::new(wall());
            let guess = special(p, bench, "a hook drag whose first motion leaves the icon's row, which the prediction follows", || {
                hook_drag(from, jump_at, release_at)?;
                released.set(wall());
                Ok(format!("the probe icon pressed at {},{}, first moved to {},{} in the row above, then let go at {},{}, where Plasma, which takes the drag from that first motion, lands it in cell {hooked} and a drag taken from the press would land in cell {straight}", from.0, from.1, jump_at.0, jump_at.1, release_at.0, release_at.1))
            });
            let fresh = log_lines(&bench.log);
            let fresh = fresh.get(mark..).unwrap_or_default();
            let sets = set_times(fresh, released.get());
            let (agreed, began) = (fresh.iter().any(|line| line.contains("predicted drop agreed")), fresh.iter().filter(|line| line.contains(" drop drag began at ")).cloned().collect::<Vec<_>>());
            let now = places(bench.linux);
            let (landed, astray) = (now.contains(&hooked_rect), now.contains(&straight_rect));
            p.line(format!("    - drag starts seen {began:?}, wallpaper sets at {sets:?} ms after the drop, the net agreed {agreed}, the icon in the hooked cell {landed} and in the straight cell {astray}"));
            missed = format!("guessed {}, agreed {agreed}, in the hooked cell {landed}, sets {sets:?} ms", guess.0);
            start = moved_from(&before, &now, from, scale);
            let held = guess.0 && agreed && landed && sets.len() == 1 && sets.first().is_some_and(|first| *first < KDE_DROP_BAR);
            let lacking = unfooted(&plain.lines, plain.alone && plain.over, fresh, released.get(), over_desktop_since(bench, stamp));
            lacked = lacking.unwrap_or_default();
            p.line(format!("    - footing: {}", lacking.unwrap_or("the plain drag just before it had a shown guess its net agreed with and moved the probe icon alone, no key went down during the hook drag and its release was over the desktop")));
            Some((lacking.is_none(), held))
        });
        match hook {
            Some((Footing::Wrong, tries)) => p.fail(format!("the hook drag was not predicted where Plasma put it, {missed}, with its footing proven on try {tries}")),
            Some((Footing::Unreached, _)) => p.fail(format!("the hook drag never had its footing in {TRIES} tries, the last since {lacked}, so the daemon was not judged on it")),
            None => return,
            _ => {}
        }
        let _ = clicks(&[at(0.92, 0.92)]);
        let from = start;
        let free = farther(from, [at(0.5, 0.5), at(0.35, 0.55)]);
        let escape = special(p, bench, "drag ended with Escape", || {
            drag_ending(from, free, true)?;
            Ok(format!("the probe icon from {},{} toward {},{}, Escape before the release", from.0, from.1, free.0, free.1))
        });
        if let Some(failure) = untouched("drag ended with Escape", escape, false) {
            p.fail(failure);
        }
    }

    fn steady_lines(p: &mut Probe, tally: &mut Tally, plan: &Plan, reads: &[Taken<Rects>], last: &[[i64; 4]], slot: usize) {
        let partial: Vec<&Taken<Rects>> = reads.iter().filter(|taken| taken.seen.as_slice() != last && taken.seen != plan.before).collect();
        tally.partly[slot] += usize::from(!partial.is_empty());
        let quick = reads.iter().take_while(|taken| taken.began < STEADY).count();
        let ended = |taken: &Taken<Rects>| taken.began + taken.took;
        let verdict = match reads[..quick].iter().position(|taken| taken.seen != plan.before) {
            None => "no read within 400 ms saw a move".to_string(),
            Some(at) if at + 1 >= quick => "the read that saw the move was the last within 400 ms".to_string(),
            Some(at) => {
                let (first, second) = (&reads[at], &reads[at + 1]);
                let cost = ended(second).saturating_sub(ended(first)).as_millis();
                tally.rule[slot][0] += 1;
                tally.cost.push(cost);
                if second.seen == first.seen {
                    let half = first.seen.as_slice() != last;
                    tally.rule[slot][1] += 1;
                    tally.rule[slot][2] += usize::from(half);
                    let what = if half { "a half done move" } else { "the whole move" };
                    format!("the read twice rule goes on at {} ms with {what}, its second read cost {cost} ms", ended(second).as_millis())
                } else {
                    format!("the read twice rule sees two different reads ending at {} ms and does not go on, its second read cost {cost} ms", ended(second).as_millis())
                }
            }
        };
        let tail = partial.last().map_or_else(String::new, |taken| format!(", the last of them began at {} ms", taken.began.as_millis()));
        p.line(format!("    - partly moved in {} reads{tail}, {verdict}", partial.len()));
    }

    fn summary(p: &mut Probe, bench: &Bench, tally: &Tally) {
        p.line(format!("- the first read after the drop showed the whole move in {} of {} single drags and in {} of {} group drags", tally.whole[0], tally.made[0], tally.whole[1], tally.made[1]));
        p.line(format!("- the first final read began at {} ms in single drags and at {} ms in group drags", sorted(&tally.firsts[0]), sorted(&tally.firsts[1])));
        p.line(format!(
            "- XInput2 saw the release at a median {} ms after the drop in {} drags and over the desktop in {}, the 10 ms poll at a median {} ms in {} drags",
            shown_ms(median(&tally.xinput)),
            tally.xinput.len(),
            tally.over,
            shown_ms(median(&tally.poll)),
            tally.poll.len()
        ));
        p.line(format!("- the daemon set the wallpaper at a median {} ms after the drop, in {} of {} drags", shown_ms(median(&tally.sets)), tally.sets.len(), tally.made[0] + tally.made[1]));
        if let Some(why) = quick_limit(p) {
            set_limit(p, &format!("section 7b on {why}"), &tally.sets, 600.0);
        }
        if bench.predicts() {
            let (name, limit) = if bench.plasma5 { ("Plasma 5", KDE_DROP_BAR) } else { ("XFCE", if bench.scale > 1.0 { 900.0 } else { 700.0 }) };
            set_limit(p, &format!("the predicted single drags of section 7b on {name}"), &tally.guessed_sets, limit);
            let (guessed, judged, held) = (tally.guessed[0] + tally.guessed[1], tally.judged[0] + tally.judged[1], tally.held[0] + tally.held[1]);
            p.line(format!("- the net is judged on the {judged} of {guessed} guesses whose drop moved a place within 2.5 s, the others had no save to judge by"));
            if held < judged {
                p.fail(format!("{} of {judged} guesses of a move on {name} did not agree with the file", judged - held));
            }
            let marks = footings(&tally.singles, WINDOW);
            let count = |want: Drag| marks.iter().filter(|mark| **mark == want).count();
            let (footed, late, shadow) = (count(Drag::Footed), count(Drag::Late), count(Drag::Shadow));
            let predicted = tally.singles.iter().zip(&marks).filter(|((_, guessed), mark)| *guessed && **mark == Drag::Footed).count();
            let others = format!("{late} moved no place by 2.5 s after the drop and {shadow} came unpredicted while the daemon came back after such a drop");
            p.line(format!("- {footed} of {} single drags had footing, {others}, and the daemon predicted {predicted} of the {footed}", tally.singles.len()));
            if footed < SINGLES {
                p.fail(format!("only {footed} of {} single drags on {name} had footing, where {SINGLES} must, {others}", tally.singles.len()));
            } else if predicted < 6 {
                p.fail(format!("the daemon predicted only {predicted} of {footed} single drags with footing on {name}, where at least 6 must be predicted"));
            }
        }
        if bench.steady {
            let costs: Vec<f64> = tally.cost.iter().map(|cost| *cost as f64).collect();
            p.line(format!("- a read showed the move partly done in {} of {} single drags and in {} of {} group drags", tally.partly[0], tally.made[0], tally.partly[1], tally.made[1]));
            p.line(format!(
                "- the read twice rule had a pair to judge in {} single and {} group drags, went on in {} and {}, with a half done move in {} and {}, its second read cost a median {} ms and at most {} ms",
                tally.rule[0][0],
                tally.rule[1][0],
                tally.rule[0][1],
                tally.rule[1][1],
                tally.rule[0][2],
                tally.rule[1][2],
                shown_ms(median(&costs)),
                shown_ms(tally.cost.iter().max().map(|cost| *cost as f64))
            ));
        }
        if bench.plasma5 {
            p.line(format!("- the daemon predicted {} of {} single drags and {} of {} group drags, and its net found {} and {} of those guesses agreed", tally.guessed[0], tally.made[0], tally.guessed[1], tally.made[1], tally.held[0], tally.held[1]));
            p.line(format!("- the Plasma 5 landing rule from the press equalled the final places in {} of {} single drags", tally.predicted[0], tally.made[0]));
        }
        if bench.lattice.is_some() {
            p.line(format!("- the daemon predicted {} of {} single drags and {} of {} group drags, and its net found {} and {} of those guesses agreed", tally.guessed[0], tally.made[0], tally.guessed[1], tally.made[1], tally.held[0], tally.held[1]));
            p.line(format!(
                "- the prediction from the press equalled the final places in {} single drags and {} group drags, anchored at the last clicked icon in {} group drags, and knowing only the pressed icon in {} group drags",
                tally.predicted[0], tally.predicted[1], tally.last_clicked, tally.pressed_only
            ));
        }
        if bench.live.is_some() {
            p.line(format!("- the live accessibility read was final first at {} ms after the drop", sorted(&tally.live_firsts)));
        }
    }

    fn series(read: &impl Fn() -> Rects, dropped: Instant, steady: bool) -> Vec<Taken<Rects>> {
        let mut taken = Vec::new();
        while steady && dropped.elapsed() < STEADY {
            let began = dropped.elapsed();
            let seen = read();
            taken.push(Taken { began, took: dropped.elapsed().saturating_sub(began), seen });
        }
        for offset in OFFSETS {
            if steady && Duration::from_millis(offset) <= STEADY {
                continue;
            }
            std::thread::sleep(Duration::from_millis(offset).saturating_sub(dropped.elapsed()));
            let began = dropped.elapsed();
            let seen = read();
            taken.push(Taken { began, took: dropped.elapsed().saturating_sub(began), seen });
        }
        taken
    }

    fn live_series(live: &Live, dropped: Instant) -> Vec<Taken<Named>> {
        let mut taken = Vec::new();
        while dropped.elapsed() < LIVE_UNTIL {
            let began = dropped.elapsed();
            let seen = live.read();
            taken.push(Taken { began, took: dropped.elapsed().saturating_sub(began), seen });
            std::thread::sleep(Duration::from_millis(5));
        }
        taken
    }

    fn releases() -> Stamps {
        let stamps: Stamps = Arc::new(Mutex::new(Vec::new()));
        for (name, desktop_only) in [("xinput", false), ("xinput desktop", true)] {
            let stamps = stamps.clone();
            std::thread::spawn(move || {
                let record: screens::Shared = Arc::new(move || {
                    if let Ok(mut list) = stamps.lock() {
                        list.push((Instant::now(), name));
                    }
                });
                scooch::platform::linux::pointer::watch(&record, desktop_only)
            });
        }
        let polled = stamps.clone();
        std::thread::spawn(move || {
            let Ok((conn, number)) = x11rb::connect(None) else { return };
            let Some(root) = conn.setup().roots.get(number).map(|screen| screen.root) else { return };
            let record = || {
                if let Ok(mut list) = polled.lock() {
                    list.push((Instant::now(), "poll"));
                }
            };
            let mut down = false;
            loop {
                let Some(mask) = conn.query_pointer(root).ok().and_then(|cookie| cookie.reply().ok()).map(|reply| u16::from(reply.mask)) else { return };
                let pressed = mask & u16::from(KeyButMask::BUTTON1) != 0;
                if down && !pressed {
                    record();
                }
                down = pressed;
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        stamps
    }

    fn screen_reader(on: bool) -> Result<()> {
        bus::session()?.call_method(Some("org.a11y.Bus"), "/org/a11y/bus", Some("org.freedesktop.DBus.Properties"), "Set", &("org.a11y.Status", "ScreenReaderEnabled", zvariant::Value::from(on)))?;
        Ok(())
    }

    fn open_live(p: &mut Probe) -> Option<Live> {
        let set = screen_reader(true);
        p.line(format!(
            "- live read, ScreenReaderEnabled set to true on the accessibility bus launcher before anything else asks it, so Qt finds it on when the launcher appears, {}",
            set.as_ref().map_or_else(|error| format!("refused with {error:#}"), |_| "accepted".to_string())
        ));
        let address = match bus::call::<_, String>("org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress", &()) {
            Ok(address) => address,
            Err(error) => {
                p.line(format!("- live read, no accessibility bus in this session, {error:#}"));
                return None;
            }
        };
        let a11y = match zbus::blocking::connection::Builder::address(address.as_str()).and_then(|builder| builder.method_timeout(Duration::from_secs(2)).build()) {
            Ok(a11y) => a11y,
            Err(error) => {
                p.line(format!("- live read, the accessibility bus at {address} took no connection, {error}"));
                return None;
            }
        };
        let registry: Node = ("org.a11y.atspi.Registry".to_string(), "/org/a11y/atspi/accessible/root".to_string());
        let started = Instant::now();
        let mut shell = None;
        while shell.is_none() && started.elapsed() < Duration::from_secs(15) {
            shell = a11y_children(&a11y, &registry).into_iter().find(|app| a11y_name(&a11y, app).as_deref() == Some("plasmashell"));
            if shell.is_none() {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        let Some(shell) = shell else {
            let apps: Vec<String> = a11y_children(&a11y, &registry).iter().filter_map(|app| a11y_name(&a11y, app)).collect();
            p.line(format!("- live read, plasmashell did not join the accessibility bus within 15 s, the applications there {apps:?}"));
            return None;
        };
        p.line(format!("- live read, plasmashell on the accessibility bus after {} ms", started.elapsed().as_millis()));
        let mut queue: VecDeque<(Node, usize)> = VecDeque::from([(shell, 0)]);
        let mut holders: BTreeMap<Node, usize> = BTreeMap::new();
        let mut walked = 0usize;
        while let Some((node, depth)) = queue.pop_front() {
            walked += 1;
            if walked > 3000 {
                break;
            }
            for child in a11y_children(&a11y, &node) {
                if a11y_name(&a11y, &child).is_some_and(|name| name.starts_with(PREFIX)) {
                    *holders.entry(node.clone()).or_default() += 1;
                }
                if depth < 16 {
                    queue.push_back((child, depth + 1));
                }
            }
        }
        let best = holders.into_iter().max_by_key(|(_, count)| *count);
        let most = best.as_ref().map_or(0, |(_, count)| *count);
        let Some((holder, _)) = best.filter(|_| most >= 2) else {
            p.line(format!("- live read, {walked} accessible nodes walked and no node holds two probe icons by name, at most {most}"));
            return None;
        };
        p.line(format!("- live read, {walked} accessible nodes walked, the folder view's items sit under one node with {most} probe icons, and each live read takes their names and screen extents"));
        Some(Live { bus: a11y, holder })
    }

    fn a11y_children(bus: &zbus::blocking::Connection, node: &Node) -> Vec<Node> {
        bus.call_method(Some(node.0.as_str()), node.1.as_str(), Some("org.a11y.atspi.Accessible"), "GetChildren", &())
            .ok()
            .and_then(|reply| reply.body().deserialize::<Vec<(String, zvariant::OwnedObjectPath)>>().ok())
            .map(|list| list.into_iter().map(|(name, path)| (name, path.as_str().to_string())).collect())
            .unwrap_or_default()
    }

    fn a11y_name(bus: &zbus::blocking::Connection, node: &Node) -> Option<String> {
        let reply = bus.call_method(Some(node.0.as_str()), node.1.as_str(), Some("org.freedesktop.DBus.Properties"), "Get", &("org.a11y.atspi.Accessible", "Name")).ok()?;
        let value = reply.body().deserialize::<zvariant::OwnedValue>().ok()?;
        String::try_from(value).ok()
    }

    fn a11y_extents(bus: &zbus::blocking::Connection, node: &Node) -> Option<[i64; 4]> {
        let reply = bus.call_method(Some(node.0.as_str()), node.1.as_str(), Some("org.a11y.atspi.Component"), "GetExtents", &(0u32,)).ok()?;
        let (x, y, w, h) = reply.body().deserialize::<(i32, i32, i32, i32)>().ok()?;
        Some([i64::from(x), i64::from(y), i64::from(w), i64::from(h)])
    }

    impl Live {
        fn read(&self) -> Named {
            let mut found: Named = a11y_children(&self.bus, &self.holder)
                .iter()
                .filter_map(|node| {
                    let name = a11y_name(&self.bus, node).filter(|name| name.starts_with(PREFIX))?;
                    Some((name, a11y_extents(&self.bus, node)?))
                })
                .collect();
            found.sort();
            found
        }
    }

    fn xfce_lattice(linux: &Linux) -> Option<Lattice> {
        let icon = settings::xfconf("/desktop-icons/icon-size").and_then(|text| text.trim().parse::<i64>().ok()).map_or(xfce_rules::DEFAULT_ICON_SIZE, |size| size as f64);
        let factor = xfce::scale() as f32;
        let shrink = |rect: Rect| Rect { x: (rect.x / factor).trunc(), y: (rect.y / factor).trunc(), w: (rect.w / factor).trunc(), h: (rect.h / factor).trunc() };
        let work = xfce::workarea(Session::X11, true).map(shrink);
        let screens: Vec<Screen> = linux.all_screens().ok()?.into_iter().map(|found| found.screen).collect();
        if kde::config_folder().join("xfce4").join("desktop").join(xfce_rules::YAML_FILE).is_file() {
            let grids = screens
                .iter()
                .filter_map(|screen| {
                    let area = work.and_then(|work| intersect(&work, &screen.frame)).unwrap_or(screen.frame);
                    xfce_rules::grid(&area, icon, &xfce_rules::STYLE).map(|grid| (screen.frame, grid))
                })
                .collect();
            return Some(Lattice { grids, insets: xfce_rules::INSETS_4_20, file: xfce_rules::YAML_FILE });
        }
        let primary_only = settings::xfconf("/desktop-icons/primary").and_then(|text| gvariant::boolean(&text)).unwrap_or(false);
        let single = xfce::workarea(Session::X11, false).map(|work| xfce_rules::inset_area(&work, xfce::scale()));
        let first = screens.first()?;
        let area = match (primary_only, single) {
            (true, _) => work.and_then(|work| intersect(&work, &first.frame)).unwrap_or(first.frame),
            (false, Some(single)) => single,
            (false, None) => first.frame,
        };
        let grid = xfce_rules::grid(&area, icon, &xfce_rules::STYLE)?;
        Some(Lattice { grids: vec![(area, grid)], insets: xfce_rules::INSETS, file: xfce_rules::LATEST_RC })
    }

    impl Lattice {
        fn describe(&self) -> String {
            let first = self.grids.first().map_or_else(String::new, |(area, grid)| format!(", the first over {} with {} rows by {} columns of {:.1} px slots", show(area), grid.rows, grid.cols, grid.slot));
            format!("{} grids rebuilt the way the adapter reads {}{first}", self.grids.len(), self.file)
        }

        fn cell(grid: &xfce_rules::Grid, point: (f32, f32)) -> Option<(i64, i64)> {
            let column = ((f64::from(point.0) - (grid.origin.0 + grid.xmargin) as f64) / (grid.slot + grid.xspacing as f64)).floor() as i64;
            let row = ((f64::from(point.1) - (grid.origin.1 + grid.ymargin) as f64) / (grid.slot + grid.yspacing as f64)).floor() as i64;
            grid.rect(row, column).map(|_| (row, column))
        }

        fn predict(&self, before: &[[i64; 4]], moving: &[[i64; 4]], anchor: [i64; 4], to: (f32, f32)) -> Option<Rects> {
            let grid = self.grids.iter().find(|(frame, _)| inside(frame, to)).or(self.grids.first()).map(|(_, grid)| grid)?;
            let start = Self::cell(grid, center(&anchor))?;
            let end = Self::cell(grid, to)?;
            let mut placed: Rects = before.iter().filter(|rect| !moving.contains(rect)).copied().collect();
            for rect in moving {
                let (row, column) = Self::cell(grid, center(rect))?;
                let landed = grid.rect(row + end.0 - start.0, column + end.1 - start.1)?;
                placed.push(rounded(&self.insets.apply(landed)));
            }
            placed.sort_unstable();
            Some(placed)
        }
    }

    fn rounded(rect: &Rect) -> [i64; 4] {
        [rect.x.round() as i64, rect.y.round() as i64, rect.w.round() as i64, rect.h.round() as i64]
    }

    fn places(linux: &Linux) -> Rects {
        let mut rects: Rects = icons(linux).iter().map(rounded).collect();
        rects.sort_unstable();
        rects
    }

    fn center(rect: &[i64; 4]) -> (f32, f32) {
        (rect[0] as f32 + rect[2] as f32 / 2.0, rect[1] as f32 + rect[3] as f32 / 2.0)
    }

    fn nearest(rects: &[[i64; 4]], point: (f32, f32)) -> Option<[i64; 4]> {
        let away = |rect: &[i64; 4]| {
            let (x, y) = center(rect);
            (x - point.0).hypot(y - point.1)
        };
        rects.iter().copied().min_by(|a, b| away(a).total_cmp(&away(b)))
    }

    fn grab(rect: &[i64; 4], scale: f32) -> (i16, i16) {
        (((rect[0] as f32 + rect[2] as f32 / 2.0) * scale) as i16, ((rect[1] as f32 + rect[3] as f32 / 3.0) * scale) as i16)
    }

    fn farther(from: (i16, i16), spots: [(i16, i16); 2]) -> (i16, i16) {
        let away = |spot: (i16, i16)| (i32::from(spot.0) - i32::from(from.0)).pow(2) + (i32::from(spot.1) - i32::from(from.1)).pow(2);
        if away(spots[0]) >= away(spots[1]) {
            spots[0]
        } else {
            spots[1]
        }
    }

    fn since(at: Instant, base: Instant) -> f64 {
        match at.checked_duration_since(base) {
            Some(after) => after.as_secs_f64() * 1000.0,
            None => -base.duration_since(at).as_secs_f64() * 1000.0,
        }
    }

    fn wall() -> f64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
    }

    fn log_lines(log: &Path) -> Vec<String> {
        fs::read_to_string(log).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn daemon_after(lines: &[String], dropped: f64) -> (Vec<String>, Option<f64>) {
        let mut wakes = Vec::new();
        let mut set = None;
        for line in lines {
            let mut words = line.split_whitespace();
            let Some(stamp) = words.next().and_then(|word| word.parse::<f64>().ok()) else { continue };
            let after = (stamp - dropped) * 1000.0;
            match words.next() {
                Some("wake") => wakes.push(format!("{} {after:.0}", words.collect::<Vec<_>>().join(" "))),
                Some("set") if set.is_none() => set = Some(after),
                _ => {}
            }
        }
        (wakes, set)
    }

    fn shown_ms(value: Option<f64>) -> String {
        value.map_or_else(|| "none".to_string(), |value| format!("{value:.1}"))
    }

    fn median(values: &[f64]) -> Option<f64> {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted.get(sorted.len() / 2).copied()
    }

    fn sorted(values: &[u128]) -> String {
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        format!("{sorted:?}")
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
            if let Some(measure) = p.measures.iter_mut().find(|measure| measure.name == name) {
                if let Some(rect) = measure.adapter.as_mut() {
                    rect.x = sx as f32 * scale;
                    rect.y = sy as f32 * scale;
                }
            }
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
        act(p, "move the pointer without clicking", wander);
        act(p, "move a probe icon", |p| {
            let linux = p.linux()?;
            let before = icons(&linux);
            let target = aside(p)?;
            let detail = move_icon(p, target)?;
            let released = Instant::now();
            let mut changed = None;
            while released.elapsed() < Duration::from_secs(6) {
                if icons(&linux) != before {
                    changed = Some(released.elapsed().as_millis());
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(format!("{detail}, the adapter's reading changed {changed:?} ms after the release"))
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
        photo_at(width, height, SUBJECT)
    }

    fn photo_at(width: u32, height: u32, subject: (f32, f32, f32)) -> RgbImage {
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let (cx, cy, radius) = (subject.0 * w, subject.1 * h, subject.2 * h);
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
            Some(Kind::Cinnamon) => {
                let listed = cinnamon::background_list().is_some();
                ["org.cinnamon.desktop.background picture-uri", "org.cinnamon.desktop.background picture-options", "org.cinnamon.desktop.background picture-uri-list", "org.cinnamon.desktop.background background-mode"]
                    .iter()
                    .take(if listed { 4 } else { 2 })
                    .map(|key| (Box::new(Gsettings) as Box<dyn record::Settings>, key.to_string()))
                    .collect()
            }
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
            if p.kind == Some(Kind::Cinnamon) {
                let key = "org.cinnamon.desktop.background picture-uri";
                let uri = record::Settings::get(&Gsettings, key).ok().flatten();
                let recorded = p.recorded.iter().find(|(seen, _)| seen == key).map(|(_, value)| value.clone());
                let listed = cinnamon::background_list().is_some();
                p.line(format!("  - Cinnamon `picture-uri` right after the adapter's set, before any write back {uri:?}, a picture list {listed}, equal to the recorded value {}", recorded.as_ref() == Some(&uri)));
                if listed && recorded.as_ref().is_some_and(|recorded| *recorded != uri) {
                    p.fail(format!("Cinnamon `picture-uri` changed under the adapter's set of the {kind} picture"));
                }
            }
            std::thread::sleep(Duration::from_secs(2));
            let back = linux.wallpaper(&screen);
            let same = back.as_ref().is_ok_and(|back| back.as_deref() == Some(path.as_path()));
            p.line(format!("  - read back {:?}, same path {same}", back.as_ref().map_err(|error| error.to_string())));
            if set.is_err() || !same {
                p.fail(format!("setting the {kind} picture and reading it back did not give the same path"));
            }
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
        let recorded: Vec<(Box<dyn record::Settings>, String, Option<String>)> = keys(p, screen).into_iter().filter_map(|(store, key)| p.recorded.iter().find(|(seen, _)| *seen == key).map(|(_, value)| value.clone()).map(|value| (store, key, value))).collect();
        if recorded.is_empty() {
            let original = p.originals.iter().find(|(seen, _)| seen.id == screen.id).and_then(|(_, path)| path.clone());
            let person = p.home.join("person");
            let home = p.home.clone();
            let result = original.as_ref().map(|path| {
                let _ = fs::create_dir_all(&person);
                std::env::set_var("SCOOCH_HOME", &person);
                let result = linux.set_wallpaper(screen, path);
                std::env::set_var("SCOOCH_HOME", &home);
                result
            });
            p.line(format!("- no setting was recorded for `{}` at the start, so its original picture {:?} was set the way a person would {:?}", screen.id, original, result.map(|result| result.map_err(|error| error.to_string()))));
            return;
        }
        let listed = p.kind == Some(Kind::Cinnamon) && recorded.iter().any(|(_, key, _)| key.ends_with(" picture-uri-list"));
        for (store, key, value) in &recorded {
            if listed && key.ends_with(" picture-uri") {
                p.line(format!("- `{key}` not written back, since Scooch never writes it on a Cinnamon with a picture list"));
                continue;
            }
            let result = store.set(key, value.as_deref());
            p.line(format!("- `{key}` written back to the recorded value {:?}", result.map_err(|error| error.to_string())));
        }
        let _ = fs::remove_file(record::file(&p.home));
        std::thread::sleep(Duration::from_secs(1));
        for (store, key, value) in recorded {
            let now_value = store.get(&key).ok().flatten();
            p.line(format!("- `{key}` now {now_value:?}, equal to the recorded value {}", now_value == value));
            if now_value == value {
                continue;
            }
            p.fail(format!("`{key}` did not go back to the value recorded at the start"));
        }
    }

    struct Thread {
        name: String,
        id: String,
        voluntary: u64,
        involuntary: u64,
        cpu: u64,
    }

    fn threads_of(process: u32) -> Vec<Thread> {
        let Ok(tasks) = fs::read_dir(format!("/proc/{process}/task")) else { return Vec::new() };
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
        let mut found: Vec<Thread> = tasks
            .flatten()
            .filter_map(|task| {
                let folder = task.path();
                let name = fs::read_to_string(folder.join("comm")).ok()?.trim().to_string();
                let status = fs::read_to_string(folder.join("status")).ok()?;
                let field = |key: &str| status.lines().find_map(|line| line.strip_prefix(key)).and_then(|rest| rest.trim().parse::<u64>().ok());
                let stat = fs::read_to_string(folder.join("stat")).ok()?;
                let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
                let cpu = (fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?) * 1000 / ticks;
                Some(Thread { name, id: task.file_name().to_string_lossy().into_owned(), voluntary: field("voluntary_ctxt_switches:")?, involuntary: field("nonvoluntary_ctxt_switches:")?, cpu })
            })
            .collect();
        found.sort_by_key(|thread| thread.id.parse::<u64>().unwrap_or(0));
        found
    }

    const REST_BAR: u64 = 2;

    fn thread_rows(before: &[Thread], after: &[Thread]) -> (Vec<String>, Vec<(String, u64)>) {
        let mut rows = Vec::new();
        let mut woke = Vec::new();
        for now in after {
            let (voluntary, involuntary, cpu, began) = match before.iter().find(|was| was.id == now.id) {
                Some(was) => (now.voluntary.saturating_sub(was.voluntary), now.involuntary.saturating_sub(was.involuntary), now.cpu.saturating_sub(was.cpu), ""),
                None => (now.voluntary, now.involuntary, now.cpu, "began during the minute, "),
            };
            rows.push(format!("`{}` {}: {began}voluntary {voluntary}, involuntary {involuntary}, CPU {cpu} ms", now.name, now.id));
            woke.push((format!("{} {}", now.name, now.id), voluntary + involuntary));
        }
        rows.extend(before.iter().filter(|was| !after.iter().any(|now| now.id == was.id)).map(|was| format!("`{}` {}: ended during the minute", was.name, was.id)));
        (rows, woke)
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

    fn scooch_program() -> Result<PathBuf> {
        let exe = std::env::current_exe()?;
        exe.parent().and_then(Path::parent).map(|folder| folder.join("scooch")).filter(|path| path.is_file()).context("the scooch binary is missing, so build with --bins --examples")
    }

    fn start_daemon(p: &mut Probe, program: &Path, log_name: &str) -> Result<u32> {
        fs::write(p.home.join("state.json"), br#"{"version":1,"on":true}"#)?;
        let log = fs::File::create(p.out.join(log_name))?;
        let child = Command::new(program).arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).spawn()?;
        let process = child.id();
        p.daemon = Some(child);
        Ok(process)
    }

    fn end_to_end(p: &mut Probe) -> Result<()> {
        let linux = p.linux()?;
        let screen = linux.screens()?.first().cloned().context("no screen")?;
        let program = scooch_program()?;
        if let Some(picture) = p.pictures.first().cloned() {
            linux.set_wallpaper(&screen, &picture)?;
            p.line(format!("- wallpaper set to {}", picture.display()));
        }
        let process = start_daemon(p, &program, "watch.log")?;
        p.line(format!("- started {} as process {process}", program.display()));
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(60), || rendered(&home));
        p.line(format!("- first render after {:?} ms", first.map(|time| time.as_millis())));
        if first.is_none() {
            p.fail("the daemon did not render within 60 s of its start");
        }
        std::thread::sleep(Duration::from_secs(2));
        let mut times = Vec::new();
        let mut sets = Vec::new();
        let mut each: Vec<Option<f64>> = Vec::new();
        let plasma5 = p.kind == Some(Kind::Kde) && drops::applies(p.kind, p.session);
        let frame = primary_frame(p).unwrap_or(screen.frame);
        let log = p.out.join("watch.log");
        for round in 1..=5 {
            let before = linux.wallpaper(&screen).ok().flatten();
            let rects_before = icons(&linux);
            let seen = subject_on_screen(p, &linux).or_else(|| {
                std::thread::sleep(Duration::from_millis(1500));
                subject_on_screen(p, &linux)
            });
            let target = seen.unwrap_or((frame.x + SUBJECT.0 * frame.w, frame.y + SUBJECT.1 * frame.h));
            let moved = Instant::now();
            let mark = log_lines(&log).len();
            let tried = move_icon(p, target);
            let dropped = wall();
            let could = tried.is_ok();
            let detail = tried.unwrap_or_else(|error| format!("failed {error:#}"));
            let changed = wait_until(Duration::from_secs(10), || linux.wallpaper(&screen).ok().flatten() != before);
            let latency = changed.map(|_| moved.elapsed().as_millis());
            std::thread::sleep(if plasma5 { Duration::from_millis(2600) } else { Duration::from_millis(1800) });
            let moved_icons = icons(&linux) != rects_before;
            let place = seen.map_or("not found, so the picture's own spot".to_string(), |(x, y)| format!("{x:.0},{y:.0}"));
            p.line(format!("- move {round} onto the subject seen at {place}, {detail}, icons moved by the adapter's reading {moved_icons}, new wallpaper after {latency:?} ms"));
            if !could && p.kde_wayland() {
                if round == 1 {
                    p.soft("the moves onto the subject", "KDE on Wayland gives the probe no way to move an icon without a person");
                }
            } else if latency.is_none() {
                p.fail(format!("move {round} onto the subject brought no new wallpaper within 10 s, {detail}"));
            }
            times.extend(latency);
            let lines = log_lines(&log);
            let (wakes, set) = daemon_after(lines.get(mark..).unwrap_or_default(), dropped);
            sets.extend(set);
            each.push(set);
            let woke = if wakes.is_empty() { "none".to_string() } else { wakes.join(", ") };
            p.line(format!("  - wakes after the drop {woke}, the daemon set the wallpaper at {} ms", shown_ms(set)));
        }
        times.sort_unstable();
        p.line(format!("- median move to new wallpaper {:?} ms", times.get(times.len() / 2)));
        p.line(format!("- the daemon set the wallpaper at a median {} ms after the drop, in {} of 5 moves", shown_ms(median(&sets)), sets.len()));
        if let Some(why) = quick_limit(p) {
            set_limit(p, &format!("section 6 on {why}"), &sets, 600.0);
        }
        if p.kind == Some(Kind::Kde) && p.session == Session::X11 {
            let later: Vec<f64> = each.iter().skip(1).flatten().copied().collect();
            p.line(format!("- named apart, move 1 set the wallpaper at {} ms, since the daemon starts with no knowledge of the selection, and moves 2 to 5 at a median {} ms in {} of 4", shown_ms(each.first().copied().flatten()), shown_ms(median(&later)), later.len()));
            set_limit(p, "moves 2 to 5 of section 6 on KDE", &later, KDE_DROP_BAR);
        }
        heard_saves(p);
        switch_check(p, &linux, &screen, (&program, process))?;
        let resting = Instant::now();
        let rested = wait_until(Duration::from_secs(45), || !threads_of(process).iter().any(|thread| thread.name == "scooch-reap")).map(|_| resting.elapsed().as_millis());
        std::thread::sleep(Duration::from_secs(3));
        let before = proc_numbers(process);
        let threads_before = threads_of(process);
        std::thread::sleep(Duration::from_secs(60));
        let after = proc_numbers(process);
        let threads_after = threads_of(process);
        p.line(format!("- every thread of the daemon over a quiet minute begun 3 s after its render sessions were let go, {rested:?} ms after the last check, as name, id, voluntary and involuntary switches and CPU ms"));
        let (rows, woke) = thread_rows(&threads_before, &threads_after);
        for row in rows {
            p.line(format!("  - {row}"));
        }
        let busiest = woke.iter().map(|(_, count)| *count).max().unwrap_or(0);
        let over: Vec<String> = woke.iter().filter(|(_, count)| *count > REST_BAR).map(|(name, count)| format!("{name} {count}")).collect();
        p.line(format!("- the busiest thread switched {busiest} times in the quiet minute, the bar is {REST_BAR} for every thread"));
        if threads_after.is_empty() {
            p.fail("the daemon's threads could not be read for the quiet minute");
        } else if !over.is_empty() {
            p.fail(format!("daemon threads woke at rest {over:?}"));
        }
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
        if code.is_none() {
            p.fail("the daemon did not end within 5 s of SIGTERM");
        }
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

    fn navy_picture(width: u32, height: u32) -> RgbImage {
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        RgbImage::from_fn(width, height, |x, y| {
            let distance = ((x as f32 - 0.5 * w).powi(2) + (y as f32 - 0.5 * h).powi(2)).sqrt();
            if distance < h / 7.0 { [245, 196, 92].into() } else if (x / 24 + y / 24) % 2 == 0 { [30, 42, 70].into() } else { [24, 34, 58].into() }
        })
    }

    fn switch_check(p: &mut Probe, linux: &Linux, screen: &Screen, (program, process): (&Path, u32)) -> Result<()> {
        let frame = primary_frame(p).unwrap_or(screen.frame);
        let navy = p.home.join("probe-navy.jpg");
        fs::write(&navy, jpeg(&navy_picture((screen.frame.w * screen.scale) as u32, (screen.frame.h * screen.scale) as u32))?)?;
        let home = p.home.clone();
        let taken = |home: &Path| state_entries(home).iter().any(|(_, entry)| entry["source"].as_str().is_some_and(|source| source.ends_with("probe-navy.jpg")));
        let person = p.home.join("person");
        fs::create_dir_all(&person)?;
        std::env::set_var("SCOOCH_HOME", &person);
        let set = linux.set_wallpaper(screen, &navy);
        std::env::set_var("SCOOCH_HOME", &home);
        p.line(format!("- the navy picture set the way another program would, with a store of its own so Scooch's record of the original plays no part, {:?}, the desktop now reads {:?}", set.map_err(|error| error.to_string()), linux.wallpaper(screen).map_err(|error| error.to_string())));
        let adopted = wait_until(Duration::from_secs(30), || {
            let _ = scooch::poke::send(process);
            std::thread::sleep(Duration::from_millis(500));
            taken(&home)
        });
        std::thread::sleep(Duration::from_secs(2));
        let before = linux.wallpaper(screen).ok().flatten();
        let moved_icon = move_icon(p, (frame.x + frame.w * 0.5, frame.y + frame.h * 0.5));
        let moved = moved_icon.is_ok();
        let detail = moved_icon.unwrap_or_else(|error| format!("failed {error:#}"));
        let swapped = wait_until(Duration::from_secs(10), || linux.wallpaper(screen).ok().flatten() != before);
        std::thread::sleep(Duration::from_secs(2));
        let sources: Vec<String> = state_entries(&home).into_iter().map(|(id, entry)| format!("{id} {}", entry["source"].as_str().unwrap_or("none"))).collect();
        p.line(format!("- the navy picture taken in after {:?} ms, entries {sources:?}, the icon moved onto its subject, {detail}, new wallpaper after {:?} ms", adopted.map(|time| time.as_millis()), swapped.map(|time| time.as_millis())));
        let held = |home: &Path| state_entries(home).into_iter().map(|(id, entry)| (id, entry["placement"].clone(), entry["output"].clone())).collect::<Vec<_>>();
        let answers = |home: &Path| state_entries(home).into_iter().map(|(_, entry)| entry["belongs"]["answer"].clone()).collect::<Vec<_>>();
        let toggle = || Command::new(program).arg("scenery").env("SCOOCH_HOME", &home).output().map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string()).unwrap_or_default();
        let log = p.out.join("watch.log");
        let seen = fs::read_to_string(&log).unwrap_or_default().lines().count();
        let (kept, belongs, shown) = (held(&home), answers(&home), linux.wallpaper(screen).ok().flatten());
        let said = toggle();
        std::thread::sleep(Duration::from_secs(3));
        let (now, showing) = (held(&home), linux.wallpaper(screen).ok().flatten());
        let window: Vec<String> = fs::read_to_string(&log).unwrap_or_default().lines().skip(seen).filter(|line| line.contains(" tick ") || line.contains(" set ")).map(|line| line.chars().take(200).collect()).collect();
        let sets = window.iter().filter(|line| line.split_whitespace().nth(1) == Some("set")).count();
        let back = toggle();
        std::thread::sleep(Duration::from_secs(3));
        let same = !kept.is_empty() && kept == now;
        let line = format!("navy picture with the icon on the subject, scenery auto then off, scooch scenery said {said:?} then {back:?}, the daemon's answers belongs {belongs:?}, same placements and files {same}, wallpaper unchanged {}, wallpapers set {sets}, daemon lines {window:?}", shown == showing);
        if !taken(&home) {
            p.line(format!("- {line}"));
            p.fail("the daemon never took in a wallpaper another program set while it ran");
        } else if !moved && p.kde_wayland() {
            p.line(format!("- {line}"));
            p.soft("the scenery switch line", "KDE on Wayland gives the probe no way to move an icon without a person, so the icon never reached the subject");
        } else if !moved {
            p.line(format!("- {line}"));
            p.fail(format!("the icon could not be moved onto the navy picture's subject, {detail}"));
        } else if swapped.is_none() {
            p.line(format!("- {line}"));
            p.fail("the icon reached the navy picture's subject and no new wallpaper came within 10 s");
        } else if same && shown == showing && sets == 0 {
            p.line(format!("- {line}"));
        } else {
            p.fail(line);
        }
        Ok(())
    }

    fn touches(frame: &Rect, icon: &Rect) -> bool {
        icon.x < frame.x + frame.w && icon.x + icon.w > frame.x && icon.y < frame.y + frame.h && icon.y + icon.h > frame.y
    }

    fn inside(frame: &Rect, point: (f32, f32)) -> bool {
        point.0 >= frame.x && point.0 < frame.x + frame.w && point.1 >= frame.y && point.1 < frame.y + frame.h
    }

    fn monitor_view(p: &mut Probe) {
        if p.session == Session::X11 {
            p.line(format!("- xrandr monitors `{}`", output("xrandr", &["--listmonitors"]).replace('\n', " | ")));
        }
        match p.kind {
            Some(Kind::Kde) => {
                if let Ok(state) = kde::state() {
                    p.line(format!("- Plasma screens {:?}", state.screens));
                    for desktop in &state.desktops {
                        p.line(format!("- Plasma desktop {} on screen {} image {:?}", desktop.id, desktop.screen, desktop.image));
                    }
                }
                p.line(format!("- screen mapping {:?}", kde::mapping()));
            }
            Some(Kind::Xfce) => {
                for property in settings::xfconf_list().into_iter().filter(|property| property.starts_with("/backdrop/") && property.ends_with("/last-image")) {
                    let value = settings::xfconf(&property).unwrap_or_default();
                    p.line(format!("- xfconf {property} = `{value}`"));
                }
                if let Ok(entries) = fs::read_dir(kde::config_folder().join("xfce4").join("desktop")) {
                    for entry in entries.flatten() {
                        p.line(format!("- positions file {}", entry.path().display()));
                    }
                }
            }
            Some(Kind::Gnome) => p.line(format!("- DING monitors {:?}", gnome::geometry().map(|found| found.len()).map_err(|error| error.to_string()))),
            _ => {}
        }
    }

    fn state_entries(home: &Path) -> Vec<(String, serde_json::Value)> {
        let Ok(bytes) = fs::read(home.join("state.json")) else { return Vec::new() };
        let Ok(state) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return Vec::new() };
        state["screens"].as_object().map(|screens| screens.iter().map(|(id, entry)| (id.clone(), entry.clone())).collect()).unwrap_or_default()
    }

    fn monitors(p: &mut Probe) -> Result<()> {
        let linux = p.linux()?;
        let all = linux.all_screens()?;
        for found in &all {
            p.line(format!("- the adapter sees monitor `{}` at {} scale {} primary {}", found.screen.id, show(&found.screen.frame), found.screen.scale, found.primary));
        }
        monitor_view(p);
        let picked = linux.screens()?;
        p.line(format!("- Scooch arranges {} of them, one picture per screen {}", picked.len(), linux.shell.per_screen()));
        if all.len() < 2 {
            p.line("- one monitor only, so there is nothing more to check here".to_string());
            return Ok(());
        }
        let scale = factor(p);
        let second = all.iter().find(|found| !found.primary).unwrap_or(&all[1]).screen.clone();
        let second_frame = physical(&second.frame, scale);
        let target = (second_frame.x + 0.5 * second_frame.w, second_frame.y + 0.3 * second_frame.h);
        let tried = move_icon(p, target);
        let could = tried.is_ok();
        let detail = tried.unwrap_or_else(|error| format!("failed {error:#}"));
        std::thread::sleep(Duration::from_secs(4));
        let icon = probe_icon(p, &linux).ok();
        let landed = icon.is_some_and(|icon| inside(&second_frame, (icon.x + icon.w / 2.0, icon.y + icon.h / 2.0)));
        p.line(format!("- moved the probe icon toward `{}`, {detail}, the adapter now reads it at {}, inside that monitor {landed}", second.id, icon.as_ref().map_or("nothing".into(), show)));
        if !landed {
            if !could && p.kde_wayland() {
                p.soft("the move to the second monitor", "KDE on Wayland gives the probe no way to move an icon without a person");
            } else if could && p.xfce_x11() {
                p.soft("the move to the second monitor", "xfdesktop under Xvfb takes the probe's drag but does not carry the icon across to the second monitor");
            } else {
                p.fail(format!("the probe icon did not reach the second monitor, {detail}"));
            }
        }
        if let Ok(snapshot) = linux.snapshot() {
            for found in &all {
                let mine: Vec<String> = snapshot.icons.iter().filter(|icon| touches(&found.screen.frame, icon)).map(show).collect();
                let arranged = snapshot.screens.iter().any(|screen| screen.id == found.screen.id);
                p.line(format!("- monitor `{}` holds {} icons {mine:?}, arranged by Scooch {arranged}", found.screen.id, mine.len()));
            }
        }
        for screen in &picked {
            restore(p, &linux, screen);
        }
        let mut pictures = Vec::new();
        for (index, screen) in picked.iter().enumerate() {
            let (width, height) = ((screen.frame.w * screen.scale) as u32, (screen.frame.h * screen.scale) as u32);
            let subject = if index % 2 == 0 { SUBJECT } else { (1.0 - SUBJECT.0, SUBJECT.1, SUBJECT.2) };
            let path = p.home.join(format!("monitor-{index}.jpg"));
            fs::write(&path, jpeg(&photo_at(width, height, subject))?)?;
            let set = linux.set_wallpaper(screen, &path);
            if set.is_err() {
                p.fail(format!("setting a picture for `{}` failed", screen.id));
            }
            p.line(format!("- set `{}` to {} {:?}", screen.id, path.display(), set.map_err(|error| error.to_string())));
            pictures.push((screen.clone(), path));
        }
        let _ = fs::remove_file(record::file(&p.home));
        p.line("- the probe set those pictures the way a person would, so no record of them is kept".to_string());
        std::thread::sleep(Duration::from_secs(2));
        for (screen, path) in &pictures {
            let frame = physical(&screen.frame, scale);
            let shown = capture(p.session, &frame).ok();
            let file = scooch_core::open(path).ok();
            let share = match (&shown, &file) {
                (Some(shown), Some(file)) => matching(shown, file, &[Rect { x: 0.0, y: 0.0, w: frame.w, h: 64.0 * scale }, Rect { x: 0.0, y: frame.h - 64.0 * scale, w: frame.w, h: 64.0 * scale }]),
                _ => None,
            };
            if let Some(shown) = &shown {
                let _ = fs::write(p.out.join(format!("monitor-{}-set.png", screen.id)), scooch_core::encode_png(shown)?);
            }
            p.line(format!("- `{}` shows its picture, pixels within 12 levels {share:?}%", screen.id));
        }
        let _ = fs::remove_file(p.home.join("state.json"));
        let program = scooch_program()?;
        let process = start_daemon(p, &program, "watch-monitors.log")?;
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(60), || rendered(&home) && state_entries(&home).len() >= picked.len());
        p.line(format!("- started Scooch as process {process}, every screen rendered after {:?} ms", first.map(|time| time.as_millis())));
        if first.is_none() {
            p.fail("Scooch did not render every screen within 60 s");
        }
        std::thread::sleep(Duration::from_secs(2));
        for (id, entry) in state_entries(&home) {
            p.line(format!("- state for `{id}` source {} output {} icons {}", entry["source"], entry["output"], entry["icons"]));
        }
        let mut outputs = Vec::new();
        for (screen, _) in &pictures {
            let now_showing = linux.wallpaper(screen).ok().flatten();
            let frame = physical(&screen.frame, scale);
            let share = match (capture(p.session, &frame).ok(), now_showing.as_ref().and_then(|path| scooch_core::open(path).ok())) {
                (Some(shown), Some(file)) => {
                    let _ = fs::write(p.out.join(format!("monitor-{}-scooch.png", screen.id)), scooch_core::encode_png(&shown)?);
                    matching(&shown, &file, &[Rect { x: 0.0, y: 0.0, w: frame.w, h: 64.0 * scale }, Rect { x: 0.0, y: frame.h - 64.0 * scale, w: frame.w, h: 64.0 * scale }])
                }
                _ => None,
            };
            p.line(format!("- `{}` now shows {:?}, on screen within 12 levels {share:?}%", screen.id, now_showing));
            outputs.push(now_showing);
        }
        let distinct = outputs.iter().flatten().collect::<std::collections::BTreeSet<_>>().len();
        p.line(format!("- distinct pictures across the screens {distinct} of {}", outputs.len()));
        if distinct < outputs.len() {
            p.fail("two screens showed the same picture where each has its own");
        }
        let reaper = p.daemon.take().map(|mut child| std::thread::spawn(move || child.wait().ok()));
        let off = Command::new(&program).arg("off").env("SCOOCH_HOME", &p.home).stdin(Stdio::null()).output();
        if !off.as_ref().is_ok_and(|result| result.status.success()) {
            p.fail("scooch off did not end cleanly");
        }
        match &off {
            Ok(result) => p.line(format!("- scooch off exit {:?}, said `{}` `{}`", result.status.code(), String::from_utf8_lossy(&result.stdout).trim().replace('\n', " "), String::from_utf8_lossy(&result.stderr).trim().replace('\n', " "))),
            Err(error) => p.line(format!("- scooch off could not run {error}")),
        }
        if let Some(reaper) = reaper {
            let ended = wait_until(Duration::from_secs(5), || reaper.is_finished());
            p.line(format!("- the watcher ended {}", ended.is_some()));
            if ended.is_none() {
                p.fail("the watcher did not end within 5 s of scooch off");
            }
        }
        std::thread::sleep(Duration::from_secs(2));
        for (screen, path) in &pictures {
            let now_showing = linux.wallpaper(screen).ok().flatten();
            p.line(format!("- after off `{}` shows {:?}, its own picture again {}", screen.id, now_showing, now_showing.as_deref() == Some(path.as_path())));
            if now_showing.as_deref() != Some(path.as_path()) {
                p.fail(format!("after scooch off `{}` did not show its own picture again", screen.id));
            }
        }
        let left = record::file(&p.home).exists();
        p.line(format!("- record file left behind {left}"));
        if left {
            p.fail("scooch off left its record file behind");
        }
        Ok(())
    }

    struct Thrown {
        child: Child,
        copy: PathBuf,
        bin: PathBuf,
        stand_in: PathBuf,
        moved: PathBuf,
        entry: PathBuf,
        login: Vec<u8>,
        log: PathBuf,
        places: String,
        real: PathBuf,
        before: Option<PathBuf>,
    }

    fn logged_since(log: &Path, mark: usize, started: Instant, limit: Duration, wanted: &str) -> Option<u128> {
        wait_until(limit, || log_lines(log).iter().skip(mark).any(|line| line.contains(wanted))).map(|_| started.elapsed().as_millis())
    }

    fn program_said(log: &Path, mark: usize) -> Vec<String> {
        log_lines(log).into_iter().skip(mark).filter(|line| line.contains(" program ") && !line.contains(" program event ")).collect()
    }

    fn saved_origin(real: &Path) -> Option<String> {
        let state: serde_json::Value = serde_json::from_slice(&fs::read(real.join("state.json")).ok()?).ok()?;
        state["origin"].as_str().map(str::to_string)
    }

    fn start_thrown(copy: &Path, log: &Path, places: &str) -> Result<Child> {
        let file = fs::OpenOptions::new().create(true).append(true).open(log)?;
        Ok(Command::new(copy).arg("watch").env_remove("SCOOCH_HOME").env("SCOOCH_TRACE", "1").env("SCOOCH_LINK_PLACES", places).stdin(Stdio::null()).stdout(Stdio::null()).stderr(file).spawn()?)
    }

    fn stop_thrown(child: &mut Child) {
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        if wait_until(Duration::from_secs(10), || matches!(child.try_wait(), Ok(Some(_)))).is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }

    fn still_running(child: &mut Child) -> bool {
        matches!(child.try_wait(), Ok(None))
    }

    fn task_switches(process: u32) -> Vec<(String, u64)> {
        let Ok(tasks) = fs::read_dir(format!("/proc/{process}/task")) else { return Vec::new() };
        tasks
            .flatten()
            .filter_map(|task| {
                let name = fs::read_to_string(task.path().join("comm")).ok()?.trim().to_string();
                let status = fs::read_to_string(task.path().join("status")).ok()?;
                let field = |key: &str| status.lines().find_map(|line| line.strip_prefix(key)).and_then(|rest| rest.trim().parse::<u64>().ok());
                Some((format!("{name} {}", task.file_name().to_string_lossy()), field("voluntary_ctxt_switches:")? + field("nonvoluntary_ctxt_switches:")?))
            })
            .collect()
    }

    fn freedesktop_trash(home: &Path, file: &Path) -> Result<()> {
        let trash = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|path| path.is_absolute()).unwrap_or_else(|| home.join(".local/share")).join("Trash");
        fs::create_dir_all(trash.join("files"))?;
        fs::create_dir_all(trash.join("info"))?;
        let name = file.file_name().context("the stand in has no file name")?;
        fs::write(trash.join("info").join(format!("{}.trashinfo", name.to_string_lossy())), format!("[Trash Info]\nPath={}\nDeletionDate=2026-11-20T00:00:00\n", file.display()))?;
        fs::rename(file, trash.join("files").join(name))?;
        Ok(())
    }

    fn thrown_report(p: &mut Probe, name: &str, result: Result<()>) {
        if let Err(error) = result {
            p.fail(format!("the {name} case stopped on an error of the probe's own, {error:#}"));
        }
    }

    fn thrown_quiet(p: &mut Probe, run: &mut Thrown) -> Result<()> {
        let started = Instant::now();
        let rest = logged_since(&run.log, 0, started, Duration::from_secs(45), " sessions let go of ");
        std::thread::sleep(Duration::from_secs(3));
        let mark = log_lines(&run.log).len();
        let pid = run.child.id();
        let first = task_switches(pid);
        std::thread::sleep(Duration::from_secs(60));
        let last = task_switches(pid);
        let lines: Vec<String> = log_lines(&run.log).into_iter().skip(mark).collect();
        let events = lines.iter().filter(|line| line.contains(" program event ")).count();
        let delta = |name: &str, count: u64| first.iter().find(|(seen, _)| seen == name).map(|(_, earlier)| count.saturating_sub(*earlier));
        let woke: Vec<String> = last.iter().filter_map(|(name, count)| delta(name, *count).filter(|change| *change > 0).map(|change| format!("{name} +{change}"))).collect();
        let watch = last.iter().find(|(name, _)| name.starts_with("scooch-program ")).and_then(|(name, count)| delta(name, *count));
        p.line(format!("- a quiet 60 s with the AppImage watched, begun 3 s after the daemon let its render sessions go {rest:?} ms in: inotify events {events}, the watch thread scooch-program switched {watch:?} times, threads that woke {woke:?}, other log lines {lines:?}"));
        if events > 0 || watch != Some(0) {
            p.fail("the watch of the AppImage woke at rest");
        }
        Ok(())
    }

    fn thrown_rename(p: &mut Probe, run: &mut Thrown, linux: &Linux, screen: &Screen) -> Result<()> {
        let showing = linux.wallpaper(screen).ok().flatten();
        let mark = log_lines(&run.log).len();
        let started = Instant::now();
        fs::rename(&run.stand_in, &run.moved)?;
        let decided = logged_since(&run.log, mark, started, Duration::from_secs(25), " program moved from ");
        let moved = run.moved.to_string_lossy().into_owned();
        let followed = wait_until(Duration::from_secs(2), || saved_origin(&run.real).as_deref() == Some(moved.as_str())).is_some();
        let said = program_said(&run.log, mark);
        let mark = log_lines(&run.log).len();
        let started = Instant::now();
        fs::rename(&run.moved, &run.stand_in)?;
        let back = logged_since(&run.log, mark, started, Duration::from_secs(25), " program moved from ");
        let origin = run.stand_in.to_string_lossy().into_owned();
        let followed_back = wait_until(Duration::from_secs(2), || saved_origin(&run.real).as_deref() == Some(origin.as_str())).is_some();
        let login = fs::read(&run.entry).ok().as_deref() == Some(run.login.as_slice());
        let alive = still_running(&mut run.child);
        let now = linux.wallpaper(screen).ok().flatten();
        p.line(format!(
            "- renamed to {}: decided after {decided:?} ms, origin followed {followed}, then back: decided after {back:?} ms, origin followed back {followed_back}, login item untouched {login}, daemon alive {alive}, wallpaper before {showing:?} and after {now:?}, log {said:?} then {:?}",
            run.moved.display(),
            program_said(&run.log, mark)
        ));
        if decided.is_none() || !followed || back.is_none() || !followed_back || !login || !alive {
            p.fail("a rename of the AppImage did not take origin with it, or changed something else");
        }
        Ok(())
    }

    fn thrown_nothing(p: &mut Probe, run: &mut Thrown, what: &str, wanted: &str, act: impl FnOnce(&Path) -> std::io::Result<()>) -> Result<()> {
        let mark = log_lines(&run.log).len();
        let started = Instant::now();
        act(&run.stand_in)?;
        let judged = logged_since(&run.log, mark, started, Duration::from_secs(10), wanted);
        std::thread::sleep(Duration::from_secs(2));
        let said = program_said(&run.log, mark);
        let acted = said.iter().any(|line| line.contains("moved to the Trash") || line.contains("moved from"));
        let login = fs::read(&run.entry).ok().as_deref() == Some(run.login.as_slice());
        let alive = still_running(&mut run.child);
        let origin = run.stand_in.to_string_lossy().into_owned();
        let kept = saved_origin(&run.real).as_deref() == Some(origin.as_str());
        p.line(format!("- {what}: judged nothing after {judged:?} ms, acted anyway {acted}, login item untouched {login}, daemon alive {alive}, origin unchanged {kept}, log {said:?}"));
        if judged.is_none() || acted || !login || !alive || !kept {
            p.fail(format!("{what} changed Scooch"));
        }
        Ok(())
    }

    fn thrown_off(p: &mut Probe, run: &mut Thrown) -> Result<()> {
        let started = Instant::now();
        let out = Command::new(&run.copy).arg("off").env_remove("SCOOCH_HOME").env("SCOOCH_LINK_PLACES", &run.places).stdin(Stdio::null()).output()?;
        let error = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let exited = wait_until(Duration::from_secs(10), || !still_running(&mut run.child)).map(|_| started.elapsed().as_millis());
        let copy_gone = !run.copy.exists();
        let link_gone = fs::symlink_metadata(run.bin.join("scooch")).is_err();
        let others = fs::read_link(run.bin.join("foreign")).is_ok_and(|points| points == Path::new("/usr/bin/env")) && fs::read(run.bin.join("scooch-old")).is_ok_and(|bytes| bytes == b"a program of its own");
        let entry_gone = !run.entry.exists();
        let origin = saved_origin(&run.real);
        p.line(format!(
            "- scooch off run from the copy with origin {origin:?} and nothing pending, exit {:?}, said {:?} {error:?}: the daemon exited after {exited:?} ms, the copy removed {copy_gone}, the link at the copy removed {link_gone}, a foreign link and a regular file untouched {others}, the login item removed {entry_gone}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).trim()
        ));
        if !out.status.success() || exited.is_none() || !copy_gone || !link_gone || !others || !entry_gone {
            p.fail("an off with nothing pending did not take away the AppImage copy and its link, or touched a file it must leave alone");
        }
        Ok(())
    }

    fn thrown_restart(p: &mut Probe, run: &mut Thrown) -> Result<()> {
        if !run.stand_in.exists() {
            fs::write(&run.stand_in, b"a stand in for the AppImage")?;
        }
        if !run.copy.exists() {
            fs::copy(scooch_program()?, &run.copy)?;
            let link = run.bin.join("scooch");
            if fs::symlink_metadata(&link).is_err() {
                std::os::unix::fs::symlink(&run.copy, &link)?;
            }
        }
        if fs::read(&run.entry).ok().as_deref() != Some(run.login.as_slice()) {
            fs::write(&run.entry, &run.login)?;
        }
        let mut state: serde_json::Value = serde_json::from_slice(&fs::read(run.real.join("state.json"))?)?;
        if state["on"] != serde_json::Value::Bool(true) {
            state["on"] = serde_json::Value::Bool(true);
            fs::write(run.real.join("state.json"), serde_json::to_vec(&state)?)?;
        }
        stop_thrown(&mut run.child);
        let mark = log_lines(&run.log).len();
        let started = Instant::now();
        run.child = start_thrown(&run.copy, &run.log, &run.places)?;
        let armed = logged_since(&run.log, mark, started, Duration::from_secs(60), " program watched at ");
        p.line(format!("- the stand in made again and the daemon started again, watch armed after {armed:?} ms"));
        armed.map(drop).context("the restarted daemon made no watch on the AppImage")
    }

    fn thrown_trash(p: &mut Probe, run: &mut Thrown, linux: &Linux, screen: &Screen) -> Result<()> {
        let home = dirs::home_dir().context("no home folder")?;
        let mark = log_lines(&run.log).len();
        let started = Instant::now();
        let gio = Command::new("gio").arg("trash").arg(&run.stand_in).output();
        let used = match &gio {
            Ok(out) if out.status.success() => "gio trash".to_string(),
            other => {
                freedesktop_trash(&home, &run.stand_in)?;
                format!("a Freedesktop move made by the probe, since gio trash gave {:?}", other.as_ref().map(|out| out.status.code()).map_err(|error| error.to_string()))
            }
        };
        let decided = logged_since(&run.log, mark, started, Duration::from_secs(25), " program moved to the Trash at ");
        let turned = logged_since(&run.log, mark, started, Duration::from_secs(25), " program in the Trash, Scooch turned off");
        let exited = wait_until(Duration::from_secs(30), || !still_running(&mut run.child)).map(|_| started.elapsed().as_millis());
        let gone = wait_until(Duration::from_secs(10), || !run.entry.exists()).map(|_| started.elapsed().as_millis());
        let copy_gone = !run.copy.exists();
        let link_gone = fs::symlink_metadata(run.bin.join("scooch")).is_err();
        let others = fs::read_link(run.bin.join("foreign")).is_ok_and(|points| points == Path::new("/usr/bin/env")) && fs::read(run.bin.join("scooch-old")).is_ok_and(|bytes| bytes == b"a program of its own");
        let after = linux.wallpaper(screen).ok().flatten();
        let restored = after == run.before;
        p.line(format!(
            "- moved to the Trash through {used}: decided after {decided:?} ms, turned off after {turned:?} ms, the daemon exited after {exited:?} ms, the login item was gone after {gone:?} ms, the copy removed {copy_gone}, the link at the copy removed {link_gone}, a foreign link and a regular file untouched {others}, wallpaper now {after:?}, the person's own {:?}, back {restored}, log {:?}",
            run.before,
            program_said(&run.log, mark)
        ));
        if decided.is_none() || turned.is_none() || exited.is_none() || gone.is_none() || !copy_gone || !link_gone || !others || !restored {
            p.fail("the AppImage in the Trash did not turn Scooch off, put the wallpaper back and take away the copy and its link");
        }
        Ok(())
    }

    fn thrown_away(p: &mut Probe) -> Result<()> {
        let single = std::env::var("SCOOCH_MONITORS").map_or(true, |value| value == "1");
        if p.kind != Some(Kind::Gnome) || !single {
            p.line("- this check runs on the single screen GNOME job, which carries gio".to_string());
            return Ok(());
        }
        if let Some(mut child) = p.daemon.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let linux = p.linux()?;
        let screen = linux.screens()?.first().cloned().context("no screen")?;
        let home = dirs::home_dir().context("no home folder")?;
        let real = dirs::data_local_dir().context("no data folder")?.join("scooch");
        let scratch = std::env::temp_dir().join(format!("scooch-thrown-{}", std::process::id()));
        let (local, bin) = (scratch.join("local"), scratch.join("bin"));
        fs::create_dir_all(&local)?;
        fs::create_dir_all(&bin)?;
        p.made.push(scratch.clone());
        let copy = local.join("scooch");
        fs::copy(scooch_program()?, &copy)?;
        std::os::unix::fs::symlink(&copy, bin.join("scooch"))?;
        std::os::unix::fs::symlink("/usr/bin/env", bin.join("foreign"))?;
        fs::write(bin.join("scooch-old"), b"a program of its own")?;
        let apps = home.join("Applications");
        let moved_folder = home.join("Applications-moved");
        fs::create_dir_all(&apps)?;
        fs::create_dir_all(&moved_folder)?;
        p.made.push(moved_folder.clone());
        let stand_in = apps.join("Scooch-probe.AppImage");
        fs::write(&stand_in, b"a stand in for the AppImage")?;
        p.made.push(stand_in.clone());
        fs::create_dir_all(&real)?;
        p.made.push(real.clone());
        fs::write(real.join("state.json"), br#"{"version":1,"on":true}"#)?;
        std::env::remove_var("SCOOCH_HOME");
        let set = scooch::facade::set_origin(Some(stand_in.as_path()));
        std::env::set_var("SCOOCH_HOME", &p.home);
        p.line(format!("- the stand in AppImage {} recorded through facade::set_origin {:?}, saved origin {:?}", stand_in.display(), set.map_err(|error| format!("{error:#}")), saved_origin(&real)));
        let entry = scooch::platform::linux::autostart::path()?;
        if let Some(folder) = entry.parent() {
            fs::create_dir_all(folder)?;
        }
        fs::write(&entry, scooch::platform::linux::parse::entry::text(&copy, &entry))?;
        let login = fs::read(&entry)?;
        let before = linux.wallpaper(&screen).ok().flatten();
        let log = p.out.join("program-watch.log");
        let places = format!("{}:{}", local.display(), bin.display());
        let started = Instant::now();
        let child = start_thrown(&copy, &log, &places)?;
        let mut run = Thrown { child, copy, bin, stand_in, moved: moved_folder.join("Scooch-probe.AppImage"), entry, login, log, places, real, before };
        let armed = logged_since(&run.log, 0, started, Duration::from_secs(60), " program watched at ");
        let ready = wait_until(Duration::from_secs(60), || rendered(&run.real)).map(|_| started.elapsed().as_millis());
        let showing = linux.wallpaper(&screen).ok().flatten();
        p.line(format!("- the daemon run from the copy {} with the real data folder, a login item at {} and link places {}, rendered after {ready:?} ms, watch armed after {armed:?} ms, wallpaper before {:?}, now {showing:?}", run.copy.display(), run.entry.display(), run.places, run.before));
        if armed.is_none() {
            p.fail("the daemon made no watch on the recorded AppImage");
        } else {
            let result = thrown_quiet(p, &mut run);
            thrown_report(p, "quiet minute", result);
            let result = thrown_rename(p, &mut run, &linux, &screen);
            thrown_report(p, "rename", result);
            let result = thrown_nothing(p, &mut run, "a replace in place, the file written over", "changed in place, so nothing changes", |path| fs::write(path, b"a newer stand in for the AppImage"));
            thrown_report(p, "replace in place", result);
            let result = thrown_nothing(p, &mut run, "a delete", "was deleted, so nothing changes", |path| fs::remove_file(path));
            thrown_report(p, "delete", result);
            let result = thrown_restart(p, &mut run);
            thrown_report(p, "restart", result);
            let result = thrown_off(p, &mut run);
            thrown_report(p, "off", result);
            let result = thrown_restart(p, &mut run);
            thrown_report(p, "restart after the off", result);
            let result = thrown_trash(p, &mut run, &linux, &screen);
            thrown_report(p, "Trash", result);
        }
        if still_running(&mut run.child) {
            stop_thrown(&mut run.child);
        }
        let _ = fs::remove_file(&run.entry);
        p.line(format!("- the stand in stays in the runner's Trash, the daemon's log is in the artifact as {}", run.log.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
        Ok(())
    }

    fn real_scooch(program: &Path, arguments: &[&str]) -> Result<std::process::Output> {
        Ok(Command::new(program).args(arguments).env_remove("SCOOCH_HOME").env("SCOOCH_TRACE", "1").stdin(Stdio::null()).output()?)
    }

    fn real_state(real: &Path) -> serde_json::Value {
        fs::read(real.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(serde_json::Value::Null)
    }

    fn real_daemon(real: &Path) -> Option<u32> {
        let pid: u32 = fs::read_to_string(real.join("watch.pid")).ok()?.trim().parse().ok()?;
        (unsafe { libc::kill(pid as libc::pid_t, 0) } == 0).then_some(pid)
    }

    fn said(out: &std::process::Output) -> String {
        String::from_utf8_lossy(&out.stdout).trim().replace('\n', " / ")
    }

    fn launched_by_gio(entry: &Path) -> String {
        match Command::new("gio").arg("launch").arg(entry).env_remove("SCOOCH_HOME").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
            Ok(status) => format!("gio launch exit {:?}", status.code()),
            Err(error) => format!("gio launch could not run, {error}"),
        }
    }

    fn final_off_check(p: &mut Probe) -> Result<()> {
        let single = std::env::var("SCOOCH_MONITORS").map_or(true, |value| value == "1");
        if p.kind != Some(Kind::Gnome) || !single {
            p.line("- this check runs on the single screen GNOME job, which carries gio".to_string());
            return Ok(());
        }
        let linux = p.linux()?;
        let screen = linux.screens()?.first().cloned().context("no screen")?;
        let program = scooch_program()?;
        let real = dirs::data_local_dir().context("no data folder")?.join("scooch");
        let picture = p.pictures.first().cloned().context("no probe picture")?;
        let entry = scooch::platform::linux::autostart::path()?;
        if let Some(folder) = entry.parent() {
            fs::create_dir_all(folder)?;
        }
        let _ = fs::remove_dir_all(&real);
        fs::create_dir_all(real.join("out"))?;
        if !p.made.contains(&real) {
            p.made.push(real.clone());
        }
        let set = linux.set_wallpaper(&screen, &picture).map_err(|error| format!("{error:#}"));
        let parked_out = real.join("out").join("probe-absent-1.jpg");
        fs::copy(&picture, &parked_out)?;
        let record = serde_json::json!({"version": 1, "on": true, "screens": {"probe-absent": {"source": picture, "original": picture, "output": parked_out}}});
        fs::write(real.join("state.json"), serde_json::to_vec(&record)?)?;
        p.line(format!("- the real data folder {} holds a record for a screen probe-absent that no desktop shows, its own picture {} and its output {}, the wallpaper set to that picture {set:?}", real.display(), picture.display(), parked_out.display()));
        let before = linux.wallpaper(&screen).ok().flatten();
        let on = real_scooch(&program, &[])?;
        let id = screen.id.clone();
        let drawn = wait_until(Duration::from_secs(60), || !real_state(&real)["screens"][id.as_str()]["fingerprint"].is_null());
        let daemon = real_daemon(&real);
        p.line(format!("- scooch turned on exit {:?}, said {:?}, the screen drawn after {:?} ms, daemon {daemon:?}, login item {}", on.status.code(), said(&on), drawn.map(|time| time.as_millis()), entry.exists()));
        let started = Instant::now();
        let off = real_scooch(&program, &["off", "--final"])?;
        let took = started.elapsed().as_millis();
        let state = real_state(&real);
        let left = daemon.filter(|pid| unsafe { libc::kill(*pid as libc::pid_t, 0) } == 0);
        let kept = state["screens"]["probe-absent"]["original"].as_str() == picture.to_str();
        let parked = state["on"] != serde_json::Value::Bool(true) && state["restoring"] != serde_json::Value::Bool(true) && kept && state["screens"].get(id.as_str()).is_none();
        let back = linux.wallpaper(&screen).ok().flatten();
        p.line(format!(
            "- scooch off --final exit {:?} after {took} ms, said {:?}: daemon left {left:?}, login item left {}, record of probe-absent kept with its own picture {kept}, state parked {parked}, its output still there {}, the shown screen back on the wallpaper it had before Scooch {}, {back:?}",
            off.status.code(),
            said(&off),
            entry.exists(),
            parked_out.exists(),
            back == before
        ));
        if off.status.code() != Some(0) || left.is_some() || entry.exists() || !parked || !parked_out.exists() || back != before || !said(&off).contains("could not reach") {
            p.fail("the final off left a daemon or a login item, or did not keep the record of the screen it could not reach");
        }
        let status = real_scooch(&program, &["status"])?;
        p.line(format!("- scooch status on the parked state said {:?}", said(&status)));
        if !said(&status).contains("still shows a picture Scooch made") {
            p.fail("status on a parked state did not count the screen that still shows Scooch's picture");
        }
        let again = real_scooch(&program, &[])?;
        let drawn = wait_until(Duration::from_secs(60), || !real_state(&real)["screens"][id.as_str()]["fingerprint"].is_null());
        let state = real_state(&real);
        let woke = state["on"] == serde_json::Value::Bool(true) && state["screens"]["probe-absent"]["original"].as_str() == picture.to_str();
        p.line(format!("- scooch turned on again exit {:?}, said {:?}, the shown screen drawn after {:?} ms, on with the parked record of probe-absent and its own picture still kept {woke}, daemon {:?}", again.status.code(), said(&again), drawn.map(|time| time.as_millis()), real_daemon(&real)));
        if !woke {
            p.fail("turning on after a final off lost the record of the screen it could not reach");
        }
        let entry_named = fs::read_to_string(&entry).unwrap_or_default();
        p.line(format!("- the login item the daemon wrote {:?}", entry_named.lines().find(|line| line.starts_with("Exec="))));
        let ended = real_scooch(&program, &["off", "--final"])?;
        p.line(format!("- scooch off --final again exit {:?}, said {:?}", ended.status.code(), said(&ended)));
        let missing = real.join("gone").join("scooch");
        fs::write(&entry, scooch::platform::linux::parse::entry::text(&missing, &entry))?;
        let how = launched_by_gio(&entry);
        let removed = wait_until(Duration::from_secs(10), || !entry.exists()).map(|time| time.as_millis());
        p.line(format!("- a login item naming {} which is not there, started the way GNOME starts it, {how}: the entry removed itself after {removed:?} ms", missing.display()));
        if removed.is_none() {
            p.fail("a login item whose program is gone did not remove itself");
        }
        let mut on_again = real_state(&real);
        on_again["on"] = serde_json::Value::Bool(true);
        fs::write(real.join("state.json"), serde_json::to_vec(&on_again)?)?;
        fs::write(&entry, scooch::platform::linux::parse::entry::text(&program, &entry))?;
        let how = launched_by_gio(&entry);
        let found = wait_until(Duration::from_secs(20), || real_daemon(&real).is_some()).and_then(|_| real_daemon(&real));
        let exe = found.and_then(|pid| fs::read_link(format!("/proc/{pid}/exe")).ok());
        let line = found.and_then(|pid| fs::read(format!("/proc/{pid}/cmdline")).ok()).map(|bytes| bytes.split(|byte| *byte == 0).map(|part| String::from_utf8_lossy(part).into_owned()).filter(|part| !part.is_empty()).collect::<Vec<_>>());
        p.line(format!("- a login item naming the program, started the same way, {how}: daemon {found:?}, its program {exe:?}, its command line {line:?}, the entry still there {}", entry.exists()));
        if exe.as_deref() != Some(program.as_path()) || !line.as_deref().is_some_and(|parts| parts.last().map(String::as_str) == Some("watch")) {
            p.fail("the login item did not start the program itself as scooch watch");
        }
        let cleared = real_scooch(&program, &["off", "--final"])?;
        let _ = fs::write(real.join("state.json"), br#"{"version":1,"on":false}"#);
        let _ = fs::remove_file(&entry);
        p.line(format!("- cleared with scooch off --final exit {:?}, daemon left {:?}", cleared.status.code(), real_daemon(&real)));
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
