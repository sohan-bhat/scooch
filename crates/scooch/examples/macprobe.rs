#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macprobe runs on macOS only, so start it on a Mac runner.");
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    probe::main()
}

#[cfg(target_os = "macos")]
mod probe {
    use anyhow::{Context, Result};
    use scooch::platform::macos::{finder, launchd, spaces, store};
    use scooch::platform::{self, Screen};
    use scooch_core::{Rect, RgbImage};
    use std::ffi::c_void;
    use std::fs;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode, Stdio};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const PROBES: [(&str, bool); 4] = [("scooch-probe-a.txt", false), ("scooch-probe-twentyeight.txt", false), ("scooch probe with spaces that wraps.txt", false), ("scooch-probe-folder", true)];
    const RUNNER_ONLY: &str = "macprobe changes the wallpaper, Desktop files and the login item, so it runs those checks only on a GitHub Actions runner where GITHUB_ACTIONS is true.";
    const GRAY: [u8; 3] = [118, 118, 118];
    const LIMIT: Duration = Duration::from_secs(20);

    static START: OnceLock<Instant> = OnceLock::new();

    fn now() -> u128 {
        START.get_or_init(Instant::now).elapsed().as_millis()
    }

    fn epoch() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    struct Usage {
        uuid: [u8; 16],
        user: u64,
        system: u64,
        idle_wakeups: u64,
        interrupt_wakeups: u64,
        pageins: u64,
        wired: u64,
        resident: u64,
        footprint: u64,
        started: u64,
        exited: u64,
        child_user: u64,
        child_system: u64,
        child_idle: u64,
        child_interrupts: u64,
        child_pageins: u64,
        child_elapsed: u64,
        read: u64,
        written: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }

    extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut Usage) -> i32;
        fn mach_timebase_info(info: *mut Timebase) -> i32;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, point: Point, button: u32) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
        fn CGPreflightPostEventAccess() -> bool;
        fn CGPreflightScreenCaptureAccess() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: *const c_void);
    }

    struct Ran {
        code: Option<i32>,
        out: String,
        err: String,
        took: Duration,
    }

    impl Ran {
        fn text(&self) -> String {
            let mut text = self.out.trim().to_string();
            if !self.err.trim().is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&format!("[stderr {}]", self.err.trim()));
            }
            text
        }

        fn ok(&self) -> bool {
            self.code == Some(0)
        }
    }

    struct Probe {
        out: PathBuf,
        report: Vec<String>,
        log: fs::File,
        home: PathBuf,
        bin: PathBuf,
        desktop: PathBuf,
        runner: bool,
        failed: Vec<String>,
        made: Vec<PathBuf>,
        original: Vec<(Screen, Option<PathBuf>)>,
        subject: Option<PathBuf>,
        kind_before: Option<String>,
        allowed: bool,
        runs: u32,
        limits: Vec<String>,
        wrapped: bool,
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

        fn limit(&mut self, text: impl Into<String>) {
            let text = text.into();
            self.line(format!("- **runner limit** {text}"));
            if !self.limits.contains(&text) {
                self.limits.push(text);
            }
        }

        fn fail(&mut self, text: impl Into<String>) {
            let text = text.into();
            self.line(format!("- **missing** {text}"));
            self.failed.push(text);
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

        fn run(&mut self, program: &Path, arguments: &[&str], timeout: Duration) -> Ran {
            self.runs += 1;
            let stdout = self.out.join(format!("run-{}.out", self.runs));
            let stderr = self.out.join(format!("run-{}.err", self.runs));
            let started = Instant::now();
            let spawned = match (fs::File::create(&stdout), fs::File::create(&stderr)) {
                (Ok(out), Ok(err)) => Command::new(program).args(arguments).env("SCOOCH_HOME", &self.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(out).stderr(err).spawn(),
                _ => Err(std::io::Error::other("could not make the output files")),
            };
            let mut child = match spawned {
                Ok(child) => child,
                Err(error) => return Ran { code: None, out: String::new(), err: format!("could not start {}: {error}", program.display()), took: started.elapsed() },
            };
            let code = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                }
            };
            let ran = Ran { code, out: fs::read_to_string(&stdout).unwrap_or_default(), err: fs::read_to_string(&stderr).unwrap_or_default(), took: started.elapsed() };
            let _ = fs::remove_file(stdout);
            let _ = fs::remove_file(stderr);
            self.raw(format!("ran {} {:?} code {:?} in {} ms\n{}\n{}", program.display(), arguments, ran.code, ran.took.as_millis(), ran.out, ran.err));
            ran
        }

        fn tool(&mut self, program: &str, arguments: &[&str]) -> Ran {
            self.run(Path::new(program), arguments, LIMIT)
        }

        fn osa(&mut self, script: &str) -> Ran {
            self.run(Path::new("/usr/bin/osascript"), &["-e", script], LIMIT)
        }

        fn scooch(&mut self, arguments: &[&str], timeout: Duration) -> Ran {
            let bin = self.bin.clone();
            self.run(&bin, arguments, timeout)
        }
    }

    pub fn main() -> ExitCode {
        START.get_or_init(Instant::now);
        let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "macprobe".into()));
        let home = std::env::temp_dir().join(format!("scooch-probe-{}", std::process::id()));
        if fs::create_dir_all(&out).is_err() || fs::create_dir_all(&home).is_err() {
            eprintln!("macprobe could not make its folders, so check the disk.");
            return ExitCode::from(2);
        }
        let home = home.canonicalize().unwrap_or(home);
        std::env::set_var("SCOOCH_HOME", &home);
        let Ok(log) = fs::File::create(out.join("raw.log")) else {
            eprintln!("macprobe could not write its log, so check the disk.");
            return ExitCode::from(2);
        };
        let bin = std::env::current_exe().ok().and_then(|exe| exe.parent().and_then(Path::parent).map(|folder| folder.join("scooch"))).unwrap_or_default();
        let runner = std::env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true");
        let mut probe = Probe {
            out,
            report: vec![format!("# Scooch macOS probe on {}", std::env::var("RUNNER_NAME").unwrap_or_else(|_| "this Mac".into()))],
            log,
            home,
            bin,
            desktop: dirs::desktop_dir().unwrap_or_default(),
            runner,
            failed: Vec::new(),
            made: Vec::new(),
            original: Vec::new(),
            subject: None,
            kind_before: None,
            allowed: false,
            runs: 0,
            limits: Vec::new(),
            wrapped: false,
        };
        probe.section("1 Session facts", facts);
        if !probe.runner {
            probe.line(String::new());
            probe.line(format!("- {RUNNER_ONLY}"));
            probe.save();
            let _ = fs::remove_dir_all(&probe.home);
            return ExitCode::SUCCESS;
        }
        probe.section("2 Permission", permission);
        probe.section("3 Icons", icons);
        probe.section("4 First run", first_run);
        probe.section("5 Move to new wallpaper", moves);
        probe.section("6 Idle", idle);
        probe.section("7 Off", off);
        probe.section("8 Second cycle", second_cycle);
        probe.section("9 Spaces", spaces_check);
        probe.section("10 Cleanup", cleanup);
        probe.line(String::new());
        probe.line("## Result".to_string());
        probe.line(String::new());
        let failed = probe.failed.clone();
        if failed.is_empty() {
            probe.line("- every required measurement came back".to_string());
        }
        for failure in &failed {
            probe.line(format!("- **failed** {failure}"));
        }
        let limits = probe.limits.clone();
        for limit in &limits {
            probe.line(format!("- runner limit, not a failure: {limit}"));
        }
        probe.save();
        if failed.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    fn show(rect: &Rect) -> String {
        format!("{:.0},{:.0} {:.0}x{:.0}", rect.x, rect.y, rect.w, rect.h)
    }

    fn uid() -> u32 {
        unsafe { libc::getuid() }
    }

    fn target() -> String {
        format!("gui/{}/{}", uid(), launchd::LABEL)
    }

    fn facts(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        p.line(format!("- adapter {}", desktop.name()));
        let version = p.tool("/usr/bin/sw_vers", &[]);
        p.line(format!("- sw_vers {}", version.out.split_whitespace().collect::<Vec<_>>().join(" ")));
        let machine = p.tool("/usr/bin/uname", &["-m"]);
        p.line(format!("- machine {}", machine.out.trim()));
        let user = p.tool("/usr/bin/id", &["-un"]);
        let console = p.tool("/usr/bin/stat", &["-f", "%Su", "/dev/console"]);
        let manager = p.tool("/bin/launchctl", &["managername"]);
        p.line(format!("- user {}, console owner {}, launchd session {}", user.out.trim(), console.out.trim(), manager.out.trim()));
        p.line(format!("- SCOOCH_HOME {}, scooch binary {} exists {}", p.home.display(), p.bin.display(), p.bin.is_file()));
        p.line(format!("- Desktop folder {} exists {}", p.desktop.display(), p.desktop.is_dir()));
        let displays = p.tool("/usr/sbin/system_profiler", &["SPDisplaysDataType"]);
        for line in displays.out.lines().map(str::trim).filter(|line| line.starts_with("Resolution") || line.starts_with("UI Looks like") || line.starts_with("Display Type") || line.starts_with("Main Display") || line.ends_with(':') && !line.starts_with("Graphics")) {
            p.line(format!("- display info {line}"));
        }
        let screens = desktop.screens()?;
        if screens.is_empty() {
            p.fail("no screen shows a desktop");
        }
        for screen in &screens {
            let shown = desktop.wallpaper(screen).ok().flatten();
            let (display, space) = spaces::split(&screen.id);
            let kind = store::describe(&screen.id);
            p.line(format!("- screen `{}` frame {} scale {} display {display} space {:?}", screen.id, show(&screen.frame), screen.scale, space));
            p.line(format!("  - wallpaper {:?}, store kind {:?}", shown, kind));
            p.original.push((screen.clone(), shown));
        }
        p.line(format!("- known Spaces {:?}", desktop.known().map_err(|error| error.to_string())));
        match spaces::window_server() {
            Some(monitors) => {
                for monitor in monitors {
                    p.line(format!("- window server monitor `{}` current {:?} full screen {} desktop Spaces {:?}", monitor.name, monitor.current, monitor.full_screen, monitor.spaces));
                }
            }
            None => p.line("- window server Spaces unreadable".to_string()),
        }
        p.line(format!("- preference Spaces {:?}", spaces::preferences()));
        let view = finder::view();
        p.line(format!("- Finder desktop view {view:?}"));
        p.line(format!("- in_use for the probe home {:?}", store::in_use(&p.home.join("out"))));
        let doctor = p.scooch(&["doctor"], LIMIT);
        p.line(format!("- scooch doctor exit {:?}", doctor.code));
        for line in doctor.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        if !doctor.ok() {
            p.fail("scooch doctor did not finish cleanly");
        }
        Ok(())
    }

    fn access_words(access: finder::Access) -> String {
        format!("{access:?}")
    }

    fn permission(p: &mut Probe) -> Result<()> {
        let access = finder::access(false);
        p.allowed = access == finder::Access::Allowed;
        p.line(format!("- Finder automation for the probe, asked without a prompt: {}", access_words(access)));
        p.line(format!("- screen capture preflight {}, post event preflight {}", unsafe { CGPreflightScreenCaptureAccess() }, unsafe { CGPreflightPostEventAccess() }));
        let chain = p.tool("/bin/ps", &["-o", "pid=,ppid=,comm=", "-ax"]);
        let mut parent = std::process::id() as i32;
        let table: Vec<(i32, i32, String)> = chain.out.lines().filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.collect::<Vec<_>>().join(" ")))
        }).collect();
        let mut ancestry = Vec::new();
        for _ in 0..12 {
            let Some((pid, ppid, name)) = table.iter().find(|(pid, _, _)| *pid == parent).cloned() else { break };
            ancestry.push(format!("{pid} {name}"));
            if ppid <= 1 {
                ancestry.push(format!("{ppid} launchd"));
                break;
            }
            parent = ppid;
        }
        p.line(format!("- probe ancestry {}", ancestry.join(" < ")));
        for (label, database) in [("user", dirs::home_dir().unwrap_or_default().join("Library/Application Support/com.apple.TCC/TCC.db")), ("system", PathBuf::from("/Library/Application Support/com.apple.TCC/TCC.db"))] {
            let query = "select service, client, auth_value, coalesce(indirect_object_identifier, '') from access where service in ('kTCCServiceAppleEvents', 'kTCCServiceAccessibility', 'kTCCServiceScreenCapture', 'kTCCServicePostEvent', 'kTCCServiceSystemPolicyAllFiles') order by service, client";
            let rows = p.tool("/usr/bin/sqlite3", &["-separator", " | ", &database.to_string_lossy(), query]);
            if rows.ok() {
                let lines: Vec<String> = rows.out.lines().map(str::to_string).collect();
                p.line(format!("- {label} TCC database {} rows for automation, accessibility, capture, events and disk", lines.len()));
                for row in lines {
                    p.line(format!("  - `{row}`"));
                }
            } else {
                p.line(format!("- {label} TCC database unreadable: {}", rows.text()));
            }
        }
        let plist = p.home.join("doctor-probe.plist");
        let output = p.home.join("doctor-launchd.txt");
        let label = "me.sohanbhat.scooch.doctorprobe";
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n<key>Label</key><string>{label}</string>\n<key>ProgramArguments</key><array><string>{}</string><string>doctor</string></array>\n<key>RunAtLoad</key><true/>\n<key>EnvironmentVariables</key><dict><key>SCOOCH_HOME</key><string>{}</string></dict>\n<key>StandardOutPath</key><string>{}</string>\n<key>StandardErrorPath</key><string>{}</string>\n</dict>\n</plist>\n",
            p.bin.display(),
            p.home.display(),
            output.display(),
            output.display()
        );
        fs::write(&plist, text)?;
        let domain = format!("gui/{}", uid());
        let loaded = p.tool("/bin/launchctl", &["bootstrap", &domain, &plist.to_string_lossy()]);
        p.line(format!("- doctor started by launchd as {label}, bootstrap {}", if loaded.ok() { "ok".to_string() } else { loaded.text() }));
        let done = wait_until(Duration::from_secs(30), || fs::read_to_string(&output).is_ok_and(|text| text.contains("Login item")));
        let _ = p.tool("/bin/launchctl", &["bootout", &format!("{domain}/{label}")]);
        let text = fs::read_to_string(&output).unwrap_or_default();
        p.line(format!("- doctor under launchd finished after {:?} ms", done.map(|time| time.as_millis())));
        for line in text.lines() {
            p.line(format!("  - `{line}`"));
        }
        if !text.lines().any(|line| line.contains("Automation")) {
            p.fail("doctor under launchd printed no Automation line");
        }
        Ok(())
    }

    fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if done() {
                return Some(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    fn filled(width: u32, height: u32, color: [u8; 3]) -> RgbImage {
        RgbImage::from_raw(width, height, color.repeat((width * height) as usize)).unwrap_or_default()
    }

    fn flat(width: u32, height: u32) -> RgbImage {
        filled(width, height, GRAY)
    }

    fn subject_picture(width: u32, height: u32, at: (f32, f32)) -> RgbImage {
        let mut image = filled(width, height, [24, 34, 58]);
        let (cx, cy) = (at.0 * width as f32, at.1 * height as f32);
        let radius = height as f32 / 7.0;
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let distance = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if distance < radius {
                pixel.0 = [245, 196, 92];
            } else if (x / 24 + y / 24) % 2 == 0 {
                pixel.0 = [30, 42, 70];
            }
        }
        image
    }

    fn picture_file(p: &Probe, name: &str, image: &RgbImage) -> Result<PathBuf> {
        let path = p.home.join(name);
        fs::write(&path, scooch_core::encode_jpeg(image, 92)?)?;
        Ok(path)
    }

    fn main_screen(p: &Probe) -> Result<Screen> {
        p.original.first().map(|(screen, _)| screen.clone()).context("no screen was read in check 1")
    }

    fn pixels(screen: &Screen) -> (u32, u32) {
        ((screen.frame.w * screen.scale).round() as u32, (screen.frame.h * screen.scale).round() as u32)
    }

    fn place(p: &mut Probe, name: &str, at: (f32, f32)) -> Ran {
        let script = format!("with timeout of 15 seconds\ntell application \"Finder\" to set desktop position of item \"{name}\" of desktop to {{{}, {}}}\nend timeout", at.0.round() as i32, at.1.round() as i32);
        p.osa(&script)
    }

    fn position(p: &mut Probe, name: &str) -> Option<(f32, f32)> {
        let ran = p.osa(&format!("with timeout of 15 seconds\ntell application \"Finder\" to get desktop position of item \"{name}\" of desktop\nend timeout"));
        let numbers: Vec<f32> = ran.out.trim().split(", ").filter_map(|part| part.trim().parse().ok()).collect();
        (numbers.len() == 2).then(|| (numbers[0], numbers[1]))
    }

    fn make_probes(p: &mut Probe) -> Result<()> {
        for (name, folder) in PROBES {
            let path = p.desktop.join(name);
            if folder {
                fs::create_dir_all(&path)?;
            } else {
                fs::write(&path, b"scooch probe")?;
            }
            p.made.push(path);
        }
        Ok(())
    }

    fn capture(p: &mut Probe, name: &str) -> Option<RgbImage> {
        let path = p.out.join(name);
        let ran = p.tool("/usr/sbin/screencapture", &["-x", "-D", "1", "-t", "png", &path.to_string_lossy()]);
        if !ran.ok() {
            p.line(format!("- screencapture failed {}", ran.text()));
            return None;
        }
        match scooch_core::open(&path) {
            Ok(image) => Some(image),
            Err(error) => {
                p.line(format!("- capture unreadable {error}"));
                None
            }
        }
    }

    fn ink(image: &RgbImage, around: &Rect, scale: f32, background: [u8; 3]) -> Option<Rect> {
        let spare = 30.0;
        let x0 = ((around.x - spare) * scale).max(0.0) as u32;
        let y0 = ((around.y - spare) * scale).max(0.0) as u32;
        let x1 = (((around.x + around.w + spare) * scale) as u32).min(image.width());
        let y1 = (((around.y + around.h + spare) * scale) as u32).min(image.height());
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        for y in y0..y1 {
            for x in x0..x1 {
                let pixel = image.get_pixel(x, y).0;
                let distance: i32 = (0..3).map(|channel| (pixel[channel] as i32 - background[channel] as i32).abs()).sum();
                if distance > 45 {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x);
                    bottom = bottom.max(y);
                }
            }
        }
        (left <= right && top <= bottom).then(|| Rect { x: left as f32 / scale, y: top as f32 / scale, w: (right - left + 1) as f32 / scale, h: (bottom - top + 1) as f32 / scale })
    }

    fn icons(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let gray = picture_file(p, "probe-flat.jpg", &flat(width, height))?;
        desktop.set_wallpaper(&screen, &gray)?;
        p.line(format!("- wallpaper set to a flat gray picture {}", gray.display()));
        make_probes(p)?;
        std::thread::sleep(Duration::from_millis(2500));
        let spots: Vec<(f32, f32)> = (0..PROBES.len()).map(|index| (screen.frame.x + 160.0 + 230.0 * index as f32, screen.frame.y + 200.0 + 60.0 * (index % 2) as f32)).collect();
        for ((name, _), spot) in PROBES.iter().zip(&spots) {
            let ran = place(p, name, *spot);
            p.line(format!("- set {name} to {:.0},{:.0} by Apple Event in {} ms {}", spot.0, spot.1, ran.took.as_millis(), if ran.ok() { "ok".to_string() } else { ran.text() }));
        }
        std::thread::sleep(Duration::from_millis(1500));
        for (name, _) in PROBES {
            let at = position(p, name);
            p.line(format!("- Finder reports {name} at {at:?}"));
        }
        if !p.allowed {
            p.fail("the probe may not read Finder, so the adapter rectangles could not be read");
            return Ok(());
        }
        let view = finder::view();
        let named = finder::named(&view)?;
        p.line(format!("- adapter read {} desktop items", named.len()));
        let shot = capture(p, "icons.png");
        let background = shot.as_ref().map(|image| image.get_pixel(image.width() * 3 / 5, image.height() * 3 / 4).0).unwrap_or(GRAY);
        p.line(format!("- background pixel {background:?}, capture {}", shot.as_ref().map_or("missing".to_string(), |image| format!("{}x{}", image.width(), image.height()))));
        let mut matched = 0;
        for (name, _) in PROBES {
            let stem = name.split('.').next().unwrap_or(name);
            let Some((seen, rect)) = named.iter().find(|(seen, _)| seen == name || seen.starts_with(stem)).cloned() else {
                p.line(format!("- adapter has no item named like {name}"));
                continue;
            };
            matched += 1;
            let truth = shot.as_ref().and_then(|image| ink(image, &rect, screen.scale, background));
            match truth {
                Some(truth) => p.line(format!(
                    "- `{seen}` adapter {} pixels {} margins left {:.0} top {:.0} right {:.0} bottom {:.0}",
                    show(&rect),
                    show(&truth),
                    truth.x - rect.x,
                    truth.y - rect.y,
                    (rect.x + rect.w) - (truth.x + truth.w),
                    (rect.y + rect.h) - (truth.y + truth.h)
                )),
                None => p.line(format!("- `{seen}` adapter {} pixels none found", show(&rect))),
            }
        }
        if matched < PROBES.len() {
            p.fail(format!("the adapter found {matched} of {} probe items", PROBES.len()));
        }
        Ok(())
    }

    fn state(p: &Probe) -> serde_json::Value {
        fs::read(p.home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn daemon_pid(p: &mut Probe) -> Option<i32> {
        let printed = p.tool("/bin/launchctl", &["print", &target()]);
        printed.out.lines().map(str::trim).find_map(|line| line.strip_prefix("pid = ").and_then(|pid| pid.trim().parse().ok()))
    }

    fn scooch_processes(p: &mut Probe) -> Vec<String> {
        let found = p.tool("/usr/bin/pgrep", &["-lx", "scooch"]);
        found.out.lines().map(str::to_string).filter(|line| !line.is_empty()).collect()
    }

    fn daemon_log(p: &Probe) -> String {
        fs::read_to_string(p.home.join("watch.log")).unwrap_or_default()
    }

    fn scooch_pid(p: &Probe) -> Option<i32> {
        let pid: i32 = fs::read_to_string(p.home.join("watch.pid")).ok()?.trim().parse().ok()?;
        (unsafe { libc::kill(pid, 0) } == 0).then_some(pid)
    }

    fn problem(p: &Probe) -> Option<String> {
        state(p)["problem"].as_str().map(str::to_string)
    }

    fn asking(p: &Probe) -> bool {
        problem(p).as_deref() == Some(finder::ASKING)
    }

    fn wait_render(p: &Probe, limit: Duration, source: Option<&Path>) -> Option<Duration> {
        let home = p.home.clone();
        let source = source.map(|path| path.to_string_lossy().into_owned());
        let started = Instant::now();
        while started.elapsed() < limit {
            let state: serde_json::Value = fs::read(home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
            let screens = state["screens"].as_object();
            let done = screens.is_some_and(|screens| !screens.is_empty() && screens.values().all(|entry| !entry["fingerprint"].is_null() && source.as_deref().is_none_or(|source| entry["source"].as_str() == Some(source))));
            if done {
                return Some(started.elapsed());
            }
            if state["problem"].as_str() == Some(finder::ASKING) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    fn finder_answers(p: &mut Probe) -> Option<u128> {
        let ran = p.osa("with timeout of 10 seconds\ntell application \"Finder\" to get name of startup disk\nend timeout");
        ran.ok().then_some(ran.took.as_millis())
    }

    fn start_wrapped(p: &mut Probe) -> Result<Duration> {
        let plist = launchd::plist_path()?;
        let domain = format!("gui/{}", uid());
        let _ = p.tool("/bin/launchctl", &["bootout", &target()]);
        let _ = wait_until(Duration::from_secs(10), || !launchd::loaded());
        let command = format!("'{}' watch; exit $?", p.bin.display());
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n<key>Label</key><string>{}</string>\n<key>ProgramArguments</key><array><string>/bin/bash</string><string>-c</string><string>{}</string></array>\n<key>RunAtLoad</key><true/>\n<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n<key>ProcessType</key><string>Standard</string>\n<key>EnvironmentVariables</key><dict><key>SCOOCH_HOME</key><string>{}</string><key>SCOOCH_TRACE</key><string>1</string></dict>\n<key>StandardErrorPath</key><string>{}</string>\n</dict>\n</plist>\n",
            launchd::LABEL,
            command.replace('&', "&amp;").replace('<', "&lt;"),
            p.home.display(),
            p.home.join("watch.log").display()
        );
        fs::write(&plist, text)?;
        let _ = fs::remove_file(p.home.join("watch.pid"));
        let started = Instant::now();
        let loaded = p.tool("/bin/launchctl", &["bootstrap", &domain, &plist.to_string_lossy()]);
        if !loaded.ok() {
            anyhow::bail!("launchctl bootstrap of the bash started job failed: {}", loaded.text());
        }
        let home = p.home.clone();
        wait_until(Duration::from_secs(15), || fs::read_to_string(home.join("watch.pid")).ok().and_then(|text| text.trim().parse::<i32>().ok()).is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0)).context("the bash started job wrote no pid file")?;
        p.wrapped = true;
        Ok(started.elapsed())
    }

    fn need_wrapped(p: &mut Probe, what: &str) -> Result<Option<Duration>> {
        if !asking(p) {
            return Ok(None);
        }
        p.limit("on a hosted runner the login item daemon waits for the Finder automation prompt that nobody can click, and the runner image pre-grants Finder automation only to bash, osascript and its own agents");
        let job = daemon_pid(p);
        let started = Instant::now();
        start_wrapped(p)?;
        let pid = scooch_pid(p);
        p.line(format!("- {what}: swapped the login item job {job:?} for the same label and plist path started through /bin/bash, scooch now runs as {pid:?}"));
        let answers = finder_answers(p);
        p.line(format!("- Finder answers Apple Events again after the swap {answers:?} ms"));
        Ok(Some(started.elapsed()))
    }

    fn first_run(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let subject = picture_file(p, "probe-subject.jpg", &subject_picture(width, height, (0.5, 0.5)))?;
        desktop.set_wallpaper(&screen, &subject)?;
        p.subject = Some(subject.clone());
        let calm = (screen.frame.x + screen.frame.w * 0.85, screen.frame.y + screen.frame.h * 0.25);
        for (index, (name, _)) in PROBES.iter().enumerate() {
            let _ = place(p, name, (calm.0, calm.1 + 110.0 * index as f32));
        }
        std::thread::sleep(Duration::from_millis(1500));
        let kind = format!("{:?}", store::describe(&screen.id));
        p.line(format!("- wallpaper set to a plain test JPEG with one bright subject in the middle {}, store kind {kind}", subject.display()));
        p.kind_before = Some(kind);
        let started = Instant::now();
        let ran = p.scooch(&[], Duration::from_secs(60));
        p.line(format!("- bare scooch exit {:?} after {} ms", ran.code, ran.took.as_millis()));
        for line in ran.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        let rendered_at = wait_render(p, Duration::from_secs(45), None).map(|_| started.elapsed());
        let shown = desktop.wallpaper(&screen).ok().flatten();
        let ours = shown.as_ref().is_some_and(|path| path.starts_with(p.home.join("out")));
        p.line(format!("- first render by the login item {:?} ms after scooch started, wallpaper now {:?}, inside the probe home {ours}", rendered_at.map(|time| time.as_millis()), shown));
        let plist = launchd::plist_path()?;
        p.line(format!("- login item file {} exists {}", plist.display(), plist.exists()));
        if let Ok(text) = fs::read_to_string(&plist) {
            p.raw(format!("login item\n{text}"));
            p.line(format!("- login item passes SCOOCH_HOME {} and SCOOCH_TRACE {}", text.contains("SCOOCH_HOME"), text.contains("SCOOCH_TRACE")));
        }
        let pid = daemon_pid(p);
        let parent = pid.map(|pid| p.tool("/bin/ps", &["-o", "ppid=,comm=", "-p", &pid.to_string()]).out.trim().to_string());
        p.line(format!("- launchctl print finds the job {}, daemon pid {pid:?}, parent and command {parent:?}", launchd::loaded()));
        p.line(format!("- state on {} problem {:?}", state(p)["on"], problem(p)));
        let status = p.scooch(&["status"], LIMIT);
        p.line(format!("- scooch status says {:?}", status.text()));
        let log = daemon_log(p);
        p.line(format!("- daemon log {} lines", log.lines().count()));
        for line in log.lines().filter(|line| !line.contains("mouse up")).take(25) {
            p.line(format!("  - `{}`", line.chars().take(200).collect::<String>()));
        }
        if !plist.exists() || !launchd::loaded() {
            p.fail("bare scooch did not leave a loaded login item");
        }
        if pid.is_none() {
            p.fail("no daemon process runs under launchd");
        }
        if rendered_at.is_some() && ours {
            return Ok(());
        }
        if !asking(p) {
            p.fail("the daemon under launchd never rendered");
            return Ok(());
        }
        let swapped = need_wrapped(p, "first run")?;
        let rendered = wait_render(p, Duration::from_secs(45), None);
        let shown = desktop.wallpaper(&screen).ok().flatten();
        let ours = shown.as_ref().is_some_and(|path| path.starts_with(p.home.join("out")));
        p.line(format!(
            "- first render by the bash started launchd job {:?} ms after it was loaded, wallpaper now {shown:?}, inside the probe home {ours}",
            swapped.zip(rendered).map(|(load, render)| (load + render).as_millis())
        ));
        if rendered.is_none() || !ours {
            p.fail("the bash started launchd job never rendered");
        }
        Ok(())
    }

    fn new_lines(text: &str, from: usize) -> Vec<String> {
        text.lines().skip(from).map(str::to_string).collect()
    }

    fn post(kind: u32, at: (f32, f32)) {
        unsafe {
            let event = CGEventCreateMouseEvent(std::ptr::null(), kind, Point { x: at.0 as f64, y: at.1 as f64 }, 0);
            if !event.is_null() {
                CGEventPost(0, event);
                CFRelease(event);
            }
        }
    }

    fn drag(from: (f32, f32), to: (f32, f32)) {
        post(5, from);
        std::thread::sleep(Duration::from_millis(120));
        post(1, from);
        std::thread::sleep(Duration::from_millis(250));
        for step in 1..=20 {
            let share = step as f32 / 20.0;
            post(6, (from.0 + (to.0 - from.0) * share, from.1 + (to.1 - from.1) * share));
            std::thread::sleep(Duration::from_millis(25));
        }
        std::thread::sleep(Duration::from_millis(150));
        post(2, to);
    }

    fn moves(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let calm = (screen.frame.x + screen.frame.w * 0.85, screen.frame.y + screen.frame.h * 0.25);
        let name = PROBES[0].0;
        let mut times = Vec::new();
        let mut sources = std::collections::BTreeMap::<String, usize>::new();
        for round in 0..5 {
            let before = desktop.wallpaper(&screen).ok().flatten();
            let seen = daemon_log(p).lines().count();
            let to = if round % 2 == 0 { subject } else { calm };
            let ran = place(p, name, to);
            let moved = Instant::now();
            let changed = wait_until(Duration::from_secs(12), || desktop.wallpaper(&screen).ok().flatten() != before);
            let latency = changed.map(|_| moved.elapsed().as_millis());
            std::thread::sleep(Duration::from_millis(400));
            let wakes: Vec<String> = new_lines(&daemon_log(p), seen).into_iter().filter(|line| line.contains(" wake ")).collect();
            for wake in &wakes {
                if let Some(source) = wake.split(" wake ").nth(1) {
                    *sources.entry(source.trim().to_string()).or_default() += 1;
                }
            }
            p.line(format!("- Apple Event move {} to {:.0},{:.0} took {} ms {}, new wallpaper after {latency:?} ms, wakes {:?}", round + 1, to.0, to.1, ran.took.as_millis(), if ran.ok() { "ok" } else { "failed" }, wakes.iter().filter_map(|line| line.split(" wake ").nth(1)).collect::<Vec<_>>()));
            match latency {
                Some(time) => times.push(time),
                None => p.fail(format!("Apple Event move {} brought no new wallpaper", round + 1)),
            }
            std::thread::sleep(Duration::from_millis(800));
        }
        times.sort_unstable();
        p.line(format!("- Apple Event move to new wallpaper times {times:?} ms, median {:?}", times.get(times.len() / 2)));
        p.line(format!("- wake sources during the moves {sources:?}"));
        let allowed = unsafe { CGPreflightPostEventAccess() };
        p.line(format!("- posting mouse events allowed by preflight {allowed}"));
        let start = position(p, name).unwrap_or(calm);
        let view = finder::view();
        let from = (start.0, start.1 + view.icon / 2.0 - view.icon / 2.0);
        let before = desktop.wallpaper(&screen).ok().flatten();
        let seen = daemon_log(p).lines().count();
        drag(from, subject);
        let dropped = Instant::now();
        let changed = wait_until(Duration::from_secs(12), || desktop.wallpaper(&screen).ok().flatten() != before);
        std::thread::sleep(Duration::from_millis(400));
        let after = position(p, name);
        let wakes: Vec<String> = new_lines(&daemon_log(p), seen).into_iter().filter(|line| line.contains(" wake ")).collect();
        let moved = after.is_some_and(|after| (after.0 - start.0).abs() > 20.0 || (after.1 - start.1).abs() > 20.0);
        p.line(format!(
            "- CGEvent drag from {:.0},{:.0} to {:.0},{:.0}: item moved {moved} from {start:?} to {after:?}, new wallpaper after {:?} ms, wakes {:?}",
            from.0,
            from.1,
            subject.0,
            subject.1,
            changed.map(|_| dropped.elapsed().as_millis()),
            wakes.iter().filter_map(|line| line.split(" wake ").nth(1)).collect::<Vec<_>>()
        ));
        Ok(())
    }

    fn rusage(pid: i32) -> Option<Usage> {
        let mut usage = Usage::default();
        (unsafe { proc_pid_rusage(pid, 2, &mut usage) } == 0).then_some(usage)
    }

    fn nanos(ticks: u64) -> u64 {
        let mut base = Timebase::default();
        unsafe { mach_timebase_info(&mut base) };
        if base.denom == 0 {
            return ticks;
        }
        ticks.saturating_mul(base.numer as u64) / base.denom as u64
    }

    fn idle(p: &mut Probe) -> Result<()> {
        std::thread::sleep(Duration::from_secs(3));
        let job = daemon_pid(p);
        let pid = scooch_pid(p).context("no scooch daemon runs")?;
        p.line(format!("- launchd job pid {job:?}, scooch daemon pid {pid}, started through bash {}", p.wrapped));
        let before = rusage(pid).context("the daemon usage could not be read")?;
        let from = epoch();
        std::thread::sleep(Duration::from_secs(60));
        let to = epoch();
        let after = rusage(pid).context("the daemon usage could not be read after the quiet minute")?;
        let cpu = nanos((after.user + after.system).saturating_sub(before.user + before.system)) / 1_000_000;
        p.line(format!("- daemon pid {pid}, CPU used over 60 s quiet {cpu} ms"));
        p.line(format!("- idle wakeups {}, interrupt wakeups {}", after.idle_wakeups.saturating_sub(before.idle_wakeups), after.interrupt_wakeups.saturating_sub(before.interrupt_wakeups)));
        p.line(format!("- footprint {:.1} MB, resident {:.1} MB", after.footprint as f64 / 1_048_576.0, after.resident as f64 / 1_048_576.0));
        let ps = p.tool("/bin/ps", &["-o", "cputime=,rss=,ppid=", "-p", &pid.to_string()]);
        p.line(format!("- ps cputime, rss and parent {}", ps.out.trim()));
        let log = daemon_log(p);
        let quiet: Vec<&str> = log.lines().filter(|line| line.split_whitespace().next().and_then(|stamp| stamp.parse::<f64>().ok()).is_some_and(|stamp| stamp >= from && stamp <= to)).collect();
        p.line(format!("- daemon log lines during the quiet minute {}", quiet.len()));
        for line in quiet.iter().take(10) {
            p.line(format!("  - `{}`", line.chars().take(160).collect::<String>()));
        }
        Ok(())
    }

    fn gone(p: &mut Probe, what: &str) {
        let plist = launchd::plist_path().map(|path| path.exists()).unwrap_or(true);
        let loaded = launchd::loaded();
        let left = scooch_processes(p);
        p.line(format!("- after {what}: login item file present {plist}, launchctl print finds the job {loaded}, scooch processes {left:?}"));
        if plist || loaded || !left.is_empty() {
            p.fail(format!("something of Scooch was left after {what}"));
        }
    }

    fn off(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let ran = p.scooch(&["off"], Duration::from_secs(60));
        p.line(format!("- scooch off exit {:?} after {} ms", ran.code, ran.took.as_millis()));
        for line in ran.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        std::thread::sleep(Duration::from_millis(1500));
        p.wrapped = false;
        let shown = desktop.wallpaper(&screen).ok().flatten();
        let kind = format!("{:?}", store::describe(&screen.id));
        let same_path = shown.as_ref().zip(p.subject.as_ref()).is_some_and(|(shown, subject)| shown == subject || shown.canonicalize().ok() == subject.canonicalize().ok());
        let same_kind = p.kind_before.as_deref() == Some(kind.as_str());
        p.line(format!("- wallpaper now {shown:?}, back to the test JPEG {same_path}; store kind {kind}, same as before {same_kind}"));
        p.line(format!("- in_use for the probe home {:?}", store::in_use(&p.home.join("out"))));
        if !same_path || !same_kind {
            p.fail("off did not put the test JPEG back");
        }
        gone(p, "off");
        Ok(())
    }

    fn second_cycle(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let second = picture_file(p, "probe-second.jpg", &subject_picture(width, height, (0.35, 0.55)))?;
        let third = picture_file(p, "probe-third.jpg", &subject_picture(width, height, (0.65, 0.45)))?;
        let started = Instant::now();
        let ran = p.scooch(&["use", &second.to_string_lossy()], Duration::from_secs(60));
        p.line(format!("- scooch use from off exit {:?} after {} ms", ran.code, ran.took.as_millis()));
        for line in ran.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        let mut rendered = wait_render(p, Duration::from_secs(30), Some(&second));
        p.line(format!("- the login item it installed is loaded {}, rendered the new picture after {:?} ms, problem {:?}", launchd::loaded(), rendered.map(|_| started.elapsed().as_millis()), problem(p)));
        if rendered.is_none() && need_wrapped(p, "second cycle")?.is_some() {
            rendered = wait_render(p, Duration::from_secs(30), Some(&second));
            p.line(format!("- the bash started job rendered the new picture {:?}", rendered.map(|time| time.as_millis())));
        }
        if rendered.is_none() {
            p.fail("scooch use did not bring a render from a daemon under launchd");
        }
        let job = daemon_pid(p);
        let started = Instant::now();
        let ran = p.scooch(&["use", &third.to_string_lossy()], Duration::from_secs(60));
        let rendered = wait_render(p, Duration::from_secs(30), Some(&third));
        let kept = fs::read_to_string(launchd::plist_path()?).is_ok_and(|text| !p.wrapped || text.contains("/bin/bash"));
        let after = daemon_pid(p);
        p.line(format!("- scooch use while it runs exit {:?}, job {job:?} then {after:?}, plist untouched {kept}, rendered after {:?} ms, wallpaper {:?}", ran.code, rendered.map(|_| started.elapsed().as_millis()), desktop.wallpaper(&screen).ok().flatten()));
        if rendered.is_none() || !kept {
            p.fail("scooch use did not reach the daemon running under launchd");
        }
        let ticks_before = daemon_log(p).lines().filter(|line| line.contains(" tick ")).count();
        let ran = p.scooch(&["flip"], LIMIT);
        p.line(format!("- scooch flip exit {:?}: {}", ran.code, ran.text()));
        let home = p.home.clone();
        let ticked = wait_until(Duration::from_secs(10), || fs::read_to_string(home.join("watch.log")).unwrap_or_default().lines().filter(|line| line.contains(" tick ")).count() > ticks_before);
        p.line(format!("- mirror saved {}, the daemon ticked {:?} ms after the flip", state(p)["options"]["mirror"], ticked.map(|time| time.as_millis())));
        if ticked.is_none() {
            p.fail("scooch flip did not reach the daemon under launchd");
        }
        let ran = p.scooch(&["off"], Duration::from_secs(60));
        p.line(format!("- scooch off exit {:?} after {} ms: {}", ran.code, ran.took.as_millis(), ran.text()));
        std::thread::sleep(Duration::from_millis(1500));
        p.wrapped = false;
        gone(p, "the second off");
        Ok(())
    }

    fn switch(p: &mut Probe, code: u32) -> Ran {
        let ran = p.osa(&format!("tell application \"System Events\" to key code {code} using control down"));
        std::thread::sleep(Duration::from_millis(2500));
        ran
    }

    fn entries(p: &Probe) -> Vec<String> {
        state(p)["screens"].as_object().map(|screens| screens.iter().map(|(id, entry)| format!("{id} output {}", entry["output"].as_str().unwrap_or("none"))).collect()).unwrap_or_default()
    }

    fn spaces_check(p: &mut Probe) -> Result<()> {
        let before = spaces::window_server().map(|monitors| monitors.iter().map(|monitor| monitor.spaces.len()).sum::<usize>());
        let ui = p.osa("tell application \"System Events\" to get UI elements enabled");
        p.line(format!("- System Events UI scripting enabled {}", ui.text()));
        let opened = p.tool("/usr/bin/open", &["-a", "Mission Control"]);
        std::thread::sleep(Duration::from_millis(1500));
        let added = p.osa("with timeout of 15 seconds\ntell application \"System Events\" to tell process \"Dock\" to click (first button whose description is \"add desktop\") of group 2 of group 1 of group 1\nend timeout");
        std::thread::sleep(Duration::from_millis(1500));
        let _ = p.osa("tell application \"System Events\" to key code 53");
        std::thread::sleep(Duration::from_millis(1500));
        let after = spaces::window_server().map(|monitors| monitors.iter().map(|monitor| monitor.spaces.len()).sum::<usize>());
        p.line(format!("- Mission Control opened {}, add desktop click {}, desktop Spaces before {before:?} after {after:?}", if opened.ok() { "ok" } else { "failed" }, if added.ok() { "ok".to_string() } else { added.text() }));
        if after <= before {
            p.line("- a second Space could not be made on this runner without private APIs, so the Spaces cycle stops here".to_string());
            return Ok(());
        }
        let screen = main_screen(p)?;
        let display = spaces::split(&screen.id).0.to_string();
        let first = spaces::current(&display);
        let switched = switch(p, 124);
        let second = spaces::current(&display);
        p.line(format!("- switch right {}, showing Space {second:?} was {first:?}", if switched.ok() { "ok".to_string() } else { switched.text() }));
        if second == first || second.is_none() {
            p.line("- switching Spaces did not work on this runner, so the Spaces cycle stops here".to_string());
            return Ok(());
        }
        switch(p, 123);
        let subject = p.subject.clone().context("no test picture")?;
        let ran = p.scooch(&["use", &subject.to_string_lossy()], Duration::from_secs(60));
        p.line(format!("- scooch use on the first Space exit {:?}", ran.code));
        if wait_render(p, Duration::from_secs(30), Some(&subject)).is_none() {
            need_wrapped(p, "Spaces")?;
        }
        let one = wait_render(p, Duration::from_secs(30), Some(&subject));
        p.line(format!("- first Space rendered {:?}, entries {:?}", one.map(|time| time.as_millis()), entries(p)));
        switch(p, 124);
        let home = p.home.clone();
        let two = wait_until(Duration::from_secs(25), || {
            let state: serde_json::Value = fs::read(home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
            state["screens"].as_object().is_some_and(|screens| screens.len() >= 2 && screens.values().all(|entry| !entry["output"].is_null()))
        });
        p.line(format!("- second Space entry with its own output after {:?} ms: {:?}", two.map(|time| time.as_millis()), entries(p)));
        switch(p, 123);
        let ran = p.scooch(&["off"], Duration::from_secs(60));
        p.wrapped = false;
        p.line(format!("- scooch off from the first Space exit {:?}: {}", ran.code, ran.text()));
        p.line(format!("- after off, restoring {}, entries {:?}, login item loaded {}", state(p)["restoring"], entries(p), launchd::loaded()));
        let plist = fs::read_to_string(launchd::plist_path()?).unwrap_or_default();
        p.line(format!("- the restoring daemon is the real login item {}", !plist.is_empty() && !plist.contains("/bin/bash")));
        switch(p, 124);
        let finished = wait_until(Duration::from_secs(30), || !launchd::loaded() && !launchd::installed());
        let back = platform::current()?.screens()?.first().cloned().and_then(|screen| platform::current().ok()?.wallpaper(&screen).ok().flatten());
        p.line(format!("- second Space shows {back:?}, the login item removed itself after {:?} ms, tally {}", finished.map(|time| time.as_millis()), state(p)["tally"]));
        switch(p, 123);
        p.line(format!("- back on the first Space, it now reads as {:?}, and the window server lists {:?}", spaces::current(&display), spaces::window_server().map(|monitors| monitors.into_iter().map(|monitor| monitor.spaces).collect::<Vec<_>>())));
        if one.is_none() || two.is_none() || finished.is_none() || back.as_ref() != Some(&subject) {
            p.fail("the Spaces cycle did not finish");
        }
        Ok(())
    }

    fn cleanup(p: &mut Probe) -> Result<()> {
        if launchd::loaded() || launchd::plist_path().is_ok_and(|path| path.exists()) {
            let ran = p.scooch(&["off"], Duration::from_secs(60));
            p.line(format!("- scooch off during cleanup exit {:?}: {}", ran.code, ran.text()));
        }
        if launchd::loaded() {
            let _ = p.tool("/bin/launchctl", &["bootout", &target()]);
        }
        if let Ok(path) = launchd::plist_path() {
            let _ = fs::remove_file(path);
        }
        for left in scooch_processes(p) {
            if let Some(pid) = left.split_whitespace().next() {
                let _ = p.tool("/bin/kill", &[pid]);
            }
        }
        let made = p.made.clone();
        for path in made {
            let removed = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            if path.exists() {
                p.line(format!("- could not delete {} {:?}", path.display(), removed.err()));
            }
        }
        let desktop = platform::current()?;
        let original = p.original.iter().find_map(|(_, shown)| shown.clone()).filter(|path| path.is_file());
        for screen in desktop.screens().unwrap_or_default() {
            if let Some(shown) = &original {
                let restored = desktop.set_wallpaper(&screen, shown);
                p.line(format!("- wallpaper of `{}` set back to {} {:?}", screen.id, shown.display(), restored.map_err(|error| error.to_string())));
            }
        }
        let removed = fs::remove_dir_all(&p.home);
        p.line(format!("- probe home removed {:?}", removed.map_err(|error| error.to_string())));
        gone(p, "cleanup");
        let leftovers: Vec<String> = fs::read_dir(&p.desktop).map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.starts_with("scooch")).collect()).unwrap_or_default();
        p.line(format!("- probe files left on the Desktop {leftovers:?}"));
        if !leftovers.is_empty() {
            p.fail("probe files were left on the Desktop");
        }
        if p.out.join("icons.png").exists() {
            p.line("- the icon capture is kept in the artifact as icons.png".to_string());
        }
        Ok(())
    }
}
