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
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};
    use scooch::platform::macos::layer;
    use scooch::platform::macos::{finder, launchd, picture, spaces, store};
    use scooch_live::{Gpu, Look as Layered, Pattern, Renderer, Target};
    use scooch::platform::{self, Screen};
    use scooch_core::{Rect, RgbImage};
    use std::ffi::c_void;
    use std::fs;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode, Stdio};
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

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ThreadInfo {
        user: u64,
        system: u64,
        cpu: i32,
        policy: i32,
        run_state: i32,
        flags: i32,
        sleep_time: i32,
        current_priority: i32,
        priority: i32,
        max_priority: i32,
        name: [u8; 64],
    }

    extern "C" {
        fn proc_pidinfo(pid: i32, flavor: i32, argument: u64, buffer: *mut c_void, size: i32) -> i32;
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut Usage) -> i32;
        fn mach_timebase_info(info: *mut Timebase) -> i32;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, point: Point, button: u32) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
        fn CGPreflightPostEventAccess() -> bool;
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGWindowListCopyWindowInfo(option: u32, relative: u32) -> *const c_void;
        fn CGDisplayCreateImage(display: u32) -> *const c_void;
        fn CGWindowListCreateImage(bounds: Frame, option: u32, window: u32, image: u32) -> *const c_void;
        fn CGImageGetWidth(image: *const c_void) -> usize;
        fn CGImageGetHeight(image: *const c_void) -> usize;
        fn CGImageGetBytesPerRow(image: *const c_void) -> usize;
        fn CGImageGetBitsPerPixel(image: *const c_void) -> usize;
        fn CGImageGetDataProvider(image: *const c_void) -> *const c_void;
        fn CGDataProviderCopyData(provider: *const c_void) -> *const c_void;
        static kCGDisplayShowDuplicateLowResolutionModes: *const c_void;
        fn CGDisplayCopyAllDisplayModes(display: u32, options: *const c_void) -> *const c_void;
        fn CGDisplayCopyDisplayMode(display: u32) -> *const c_void;
        fn CGDisplayModeGetWidth(mode: *const c_void) -> usize;
        fn CGDisplayModeGetHeight(mode: *const c_void) -> usize;
        fn CGDisplayModeGetPixelWidth(mode: *const c_void) -> usize;
        fn CGDisplayModeGetPixelHeight(mode: *const c_void) -> usize;
        fn CGDisplayModeIsUsableForDesktopGUI(mode: *const c_void) -> bool;
        fn CGDisplayModeRetain(mode: *const c_void) -> *const c_void;
        fn CGDisplayModeRelease(mode: *const c_void);
        fn CGBeginDisplayConfiguration(config: *mut *mut c_void) -> i32;
        fn CGConfigureDisplayWithDisplayMode(config: *mut c_void, display: u32, mode: *const c_void, options: *const c_void) -> i32;
        fn CGCompleteDisplayConfiguration(config: *mut c_void, option: u32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: *const c_void);
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFDataGetLength(data: *const c_void) -> isize;
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        static kCFBooleanTrue: *const c_void;
        fn CFDictionaryCreate(allocator: *const c_void, keys: *const *const c_void, values: *const *const c_void, count: isize, key_callbacks: *const c_void, value_callbacks: *const c_void) -> *const c_void;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: *const c_void;
        fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, after_source: u8) -> i32;
    }

    extern "C" {
        static mach_task_self_: u32;
        fn mach_port_names(task: u32, names: *mut *mut u32, count: *mut u32, types: *mut *mut u32, types_count: *mut u32) -> i32;
        fn vm_deallocate(task: u32, address: usize, size: usize) -> i32;
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Frame {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
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
        stand: Option<Child>,
        gpu: Option<Gpu>,
        finder: bool,
        motion: Option<Option<String>>,
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
        if std::env::args().nth(1).as_deref() == Some("pace") {
            return pace(PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| "macpace".into())));
        }
        if std::env::args().nth(1).as_deref() == Some("helper") {
            return helper();
        }
        if std::env::args().nth(1).as_deref() == Some("gpu-bench") {
            return gpu_bench(PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| "macbench".into())));
        }
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
            stand: None,
            gpu: None,
            finder: true,
            motion: None,
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
        probe.section("3b Live layer", live);
        probe.section("3c Helper colour and scale", colour_scale);
        probe.section("4 First run", first_run);
        probe.section("5 Move to new wallpaper", moves);
        probe.section("5b Scenery", scenery_check);
        probe.section("5c Glide", glide_check);
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
        let sizes: Vec<String> = [p.bin.clone(), p.bin.with_file_name("scoochw"), std::env::current_exe().unwrap_or_default()].iter().map(|path| format!("{} {:.2} MB", path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(), fs::metadata(path).map_or(0, |meta| meta.len()) as f64 / 1_048_576.0)).collect();
        p.line(format!("- program sizes {sizes:?}"));
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
        problem(p).is_some_and(|problem| problem == finder::ASKING || problem == finder::DENIED)
    }

    fn windows() -> Vec<(String, i64, String)> {
        let list = unsafe { CGWindowListCopyWindowInfo(1, 0) };
        if list.is_null() {
            return Vec::new();
        }
        let parsed = store::convert(unsafe { &*(list as *const objc2::runtime::AnyObject) });
        unsafe { CFRelease(list) };
        parsed
            .items()
            .unwrap_or(&[])
            .iter()
            .map(|window| {
                let owner = window.get("kCGWindowOwnerName").and_then(store::Plist::text).unwrap_or_default().to_string();
                let layer = window.get("kCGWindowLayer").and_then(store::Plist::number).unwrap_or_default() as i64;
                let name = window.get("kCGWindowName").and_then(store::Plist::text).unwrap_or_default().to_string();
                (owner, layer, name)
            })
            .collect()
    }

    fn prompt_owners(p: &mut Probe) -> Vec<String> {
        let common = ["Finder", "Dock", "Window Server", "WindowServer", "SystemUIServer", "Control Center", "Window Manager", "Spotlight", "Notification Center", "TextInputMenuAgent"];
        let shown: Vec<(String, i64, String)> = windows().into_iter().filter(|(owner, _, _)| !common.contains(&owner.as_str())).collect();
        p.line(format!("- windows on screen besides the desktop ones {shown:?}"));
        let mut owners: Vec<String> = shown.into_iter().map(|(owner, _, _)| owner).collect();
        owners.dedup();
        owners
    }

    fn wait_render(p: &Probe, limit: Duration, source: Option<&Path>, stop_on_consent: bool) -> Option<Duration> {
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
            if stop_on_consent && state["problem"].as_str().is_some_and(|problem| problem == finder::ASKING || problem == finder::DENIED) {
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

    fn dismiss_prompt(p: &mut Probe) -> Option<u128> {
        if let Some(answers) = finder_answers(p) {
            p.line(format!("- Finder answers Apple Events in {answers} ms"));
            return Some(answers);
        }
        let owners = prompt_owners(p);
        for owner in owners.iter().filter(|owner| ["UserNotificationCenter", "tccd", "universalAccessAuthWarn", "CoreServicesUIAgent"].contains(&owner.as_str())) {
            let ended = p.tool("/usr/bin/killall", &[owner]);
            p.line(format!("- closed the unanswered consent window by quitting {owner} {}, which answers nothing", if ended.ok() { "ok".to_string() } else { ended.text() }));
        }
        std::thread::sleep(Duration::from_secs(2));
        let answers = finder_answers(p);
        p.line(format!("- Finder answers Apple Events again {answers:?} ms"));
        if answers.is_none() {
            p.limit("an unanswered Finder consent window blocks Apple Events to Finder for every app on the runner until someone answers it");
        }
        answers
    }

    fn stand_in(p: &mut Probe, what: &str) -> Result<Option<Duration>> {
        if !asking(p) {
            return Ok(None);
        }
        p.limit("on a hosted runner the login item daemon waits for a Finder automation prompt that nobody can click, and the runner image pre-grants Finder automation only to bash, osascript and its own agents, which a launchd job does not inherit");
        let job = daemon_pid(p);
        let _ = p.tool("/bin/launchctl", &["bootout", &target()]);
        let _ = wait_until(Duration::from_secs(10), || !launchd::loaded());
        p.finder = dismiss_prompt(p).is_some();
        let _ = fs::remove_file(p.home.join("watch.pid"));
        let log = fs::OpenOptions::new().create(true).append(true).open(p.home.join("watch.log"))?;
        let started = Instant::now();
        let child = Command::new(&p.bin).arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).spawn()?;
        p.stand = Some(child);
        let home = p.home.clone();
        wait_until(Duration::from_secs(15), || fs::read_to_string(home.join("watch.pid")).ok().and_then(|text| text.trim().parse::<i32>().ok()).is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0)).context("the probe started daemon wrote no pid file")?;
        let pid = scooch_pid(p);
        p.line(format!("- {what}: booted out the login item job {job:?}, closed its consent window, and started scooch watch from the probe as {pid:?}, which inherits the runner's Finder automation"));
        Ok(Some(started.elapsed()))
    }

    fn reap(p: &mut Probe) {
        if let Some(mut child) = p.stand.take() {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(10) {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill();
            let _ = child.wait();
            p.line("- the probe started daemon had to be killed".to_string());
        }
    }

    fn ticks(p: &Probe) -> usize {
        daemon_log(p).lines().filter(|line| line.contains(" tick ")).count()
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
        let _ = place(p, PROBES[0].0, (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5));
        p.line(format!("- {} sits on the subject and the other probe items sit in a calm column", PROBES[0].0));
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
        let rendered_at = wait_render(p, Duration::from_secs(45), None, true).map(|_| started.elapsed());
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
        let swapped = stand_in(p, "first run")?;
        let rendered = wait_render(p, Duration::from_secs(45), None, false);
        let shown = desktop.wallpaper(&screen).ok().flatten();
        let ours = shown.as_ref().is_some_and(|path| path.starts_with(p.home.join("out")));
        p.line(format!(
            "- first render by the probe started daemon {:?} ms after it started, wallpaper now {shown:?}, inside the probe home {ours}",
            swapped.zip(rendered).map(|(load, render)| (load + render).as_millis())
        ));
        if rendered.is_none() || !ours {
            p.fail("the probe started daemon never rendered");
        }
        if let Some(pid) = scooch_pid(p) {
            quiet_minute(p, pid, "the probe started daemon right after its launch, with no consent pending")?;
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

    fn poke(p: &mut Probe) {
        if let Some(pid) = scooch_pid(p) {
            let _ = p.tool("/bin/kill", &["-USR1", &pid.to_string()]);
        }
    }

    fn away(p: &mut Probe, name: &str, calm: (f32, f32)) {
        let desktop = platform::current();
        let screen = main_screen(p);
        let before = desktop.as_ref().ok().zip(screen.as_ref().ok()).and_then(|(desktop, screen)| desktop.wallpaper(screen).ok().flatten());
        let _ = place(p, name, calm);
        std::thread::sleep(Duration::from_millis(300));
        poke(p);
        let _ = wait_until(Duration::from_secs(6), || desktop.as_ref().ok().zip(screen.as_ref().ok()).and_then(|(desktop, screen)| desktop.wallpaper(screen).ok().flatten()) != before);
        std::thread::sleep(Duration::from_millis(1200));
    }

    fn moves(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let calm = (screen.frame.x + screen.frame.w * 0.85, screen.frame.y + screen.frame.h * 0.25);
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.finder = false;
            p.line("- the moves need Apple Events to Finder, which the open consent window blocks, so they were skipped".to_string());
            return Ok(());
        }
        p.line(format!("- each round first moves {name} to a calm spot without timing it and pokes the daemon, then times one move onto the subject, with the scenery setting `{}`", state(p)["options"]["scenery"].as_str().unwrap_or("missing")));
        let mut times = Vec::new();
        let mut sources = std::collections::BTreeMap::<String, usize>::new();
        let first_line = daemon_log(p).lines().count();
        for round in 0..5 {
            away(p, name, calm);
            let before = desktop.wallpaper(&screen).ok().flatten();
            let seen = daemon_log(p).lines().count();
            let ran = place(p, name, subject);
            let moved = Instant::now();
            let changed = wait_until(Duration::from_secs(12), || desktop.wallpaper(&screen).ok().flatten() != before);
            let latency = changed.map(|_| moved.elapsed().as_millis());
            std::thread::sleep(Duration::from_millis(400));
            let wakes: Vec<String> = new_lines(&daemon_log(p), seen).iter().filter_map(|line| line.split(" wake ").nth(1).map(|source| source.trim().to_string())).collect();
            for wake in &wakes {
                *sources.entry(wake.clone()).or_default() += 1;
            }
            p.line(format!("- Apple Event move {} onto the subject took {} ms {}, new wallpaper after {latency:?} ms, wakes {wakes:?}", round + 1, ran.took.as_millis(), if ran.ok() { "ok" } else { "failed" }));
            times.extend(latency);
        }
        times.sort_unstable();
        p.line(format!("- Apple Event moves that brought a new wallpaper {} of 5, times {times:?} ms, median {:?}", times.len(), times.get(times.len() / 2)));
        p.line(format!("- wake sources during the Apple Event moves {sources:?}, render laps {:?} ms", render_laps(p, first_line)));
        if times.is_empty() {
            p.fail("no Apple Event move brought a new wallpaper");
        }
        let allowed = unsafe { CGPreflightPostEventAccess() };
        p.line(format!("- posting mouse events allowed by preflight {allowed}"));
        away(p, name, calm);
        let start = position(p, name).unwrap_or(calm);
        let before = desktop.wallpaper(&screen).ok().flatten();
        let seen = daemon_log(p).lines().count();
        drag(start, subject);
        let dropped = Instant::now();
        let changed = wait_until(Duration::from_secs(12), || desktop.wallpaper(&screen).ok().flatten() != before);
        let latency = changed.map(|_| dropped.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(400));
        let after = position(p, name);
        let wakes: Vec<String> = new_lines(&daemon_log(p), seen).iter().filter_map(|line| line.split(" wake ").nth(1).map(|source| source.trim().to_string())).collect();
        let moved = after.is_some_and(|after| (after.0 - start.0).abs() > 20.0 || (after.1 - start.1).abs() > 20.0);
        p.line(format!("- CGEvent drag from {:.0},{:.0} onto the subject at {:.0},{:.0}: item moved {moved} to {after:?}, new wallpaper after {latency:?} ms, wakes {wakes:?}", start.0, start.1, subject.0, subject.1));
        if allowed && (!moved || latency.is_none()) {
            p.fail("the CGEvent drag did not move the item and bring a new wallpaper");
        }
        if !allowed {
            p.limit("the runner does not let the probe post mouse events, so no real drag was tried");
        }
        Ok(())
    }

    fn tick_lines(p: &Probe, from: usize) -> Vec<String> {
        new_lines(&daemon_log(p), from).into_iter().filter(|line| line.contains(" tick ")).map(|line| line.chars().take(200).collect()).collect()
    }

    fn render_laps(p: &Probe, from: usize) -> Vec<u64> {
        new_lines(&daemon_log(p), from).iter().filter(|line| line.contains(" tick ") && line.contains("Rendered")).filter_map(|line| line.split(", ").find_map(|lap| lap.trim().strip_prefix("render ")).and_then(|millis| millis.trim().parse().ok())).collect()
    }

    fn icon_rect(name: &str) -> Option<Rect> {
        finder::named(&finder::view()).ok()?.into_iter().find(|(found, _)| found == name).map(|(_, rect)| rect)
    }

    fn ring(a: &Shot, b: &Shot, icon: &Rect, screen: &Screen) -> usize {
        let scale = screen.scale;
        let (x0, y0) = (((icon.x - screen.frame.x - 40.0) * scale).max(0.0) as usize, ((icon.y - screen.frame.y - 40.0) * scale).max(0.0) as usize);
        let (x1, y1) = ((((icon.x - screen.frame.x + icon.w + 40.0) * scale) as usize).min(a.width), (((icon.y - screen.frame.y + icon.h + 40.0) * scale) as usize).min(a.height));
        let inside = |x: usize, y: usize| {
            let (px, py) = (x as f32 / scale + screen.frame.x, y as f32 / scale + screen.frame.y);
            px >= icon.x && px < icon.x + icon.w && py >= icon.y && py < icon.y + icon.h
        };
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|(x, y)| !inside(*x, *y) && !near(a.at(*x, *y), b.at(*x, *y))).count()
    }

    fn matches_file(shot: &Shot, file: &Path, icons: &[Rect], screen: &Screen) -> Option<f32> {
        let picture = scooch_core::open(file).ok()?;
        let (width, height) = (picture.width() as usize, picture.height() as usize);
        let clear = |x: usize, y: usize| {
            let (px, py) = (x as f32 / screen.scale + screen.frame.x, y as f32 / screen.scale + screen.frame.y);
            !icons.iter().any(|icon| px >= icon.x - 20.0 && px < icon.x + icon.w + 20.0 && py >= icon.y - 20.0 && py < icon.y + icon.h + 20.0)
        };
        let points: Vec<(usize, usize)> = (0..height).step_by(16).flat_map(|y| (0..width).step_by(16).map(move |x| (x, y))).filter(|(x, y)| clear(*x, *y) && *y > 40).collect();
        let same = points.iter().filter(|(x, y)| near(shot.at(*x, *y), picture.get_pixel(*x as u32, *y as u32).0)).count();
        Some(same as f32 / points.len().max(1) as f32)
    }

    fn scenery_check(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.finder = false;
            p.line("- the scenery check needs Apple Events to Finder, which the open consent window blocks, so it was skipped".to_string());
            return Ok(());
        }
        let setting = |p: &Probe| state(p)["options"]["scenery"].as_str().unwrap_or("missing").to_string();
        p.line(format!("- scenery setting in the state file at the start `{}`, the default for a new state", setting(p)));
        if setting(p) == "off" {
            let ran = p.scooch(&["scenery"], LIMIT);
            p.line(format!("- scooch scenery said {:?}", ran.text()));
        }
        let from = (screen.frame.x + screen.frame.w * 0.85, screen.frame.y + screen.frame.h * 0.25);
        let to = (screen.frame.x + screen.frame.w * 0.15, screen.frame.y + screen.frame.h * 0.75);
        away(p, name, from);
        let start = position(p, name).unwrap_or(from);
        let shown = || desktop.wallpaper(&screen).ok().flatten();
        let before = shown();
        let seen = daemon_log(p).lines().count();
        drag(start, to);
        let dropped = Instant::now();
        let on_time = wait_until(Duration::from_secs(12), || shown() != before).map(|_| dropped.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(1200));
        let icon = icon_rect(name);
        p.line(format!("- scenery on, CGEvent drag from {:.0},{:.0} to calm ground at {:.0},{:.0}: new wallpaper after {on_time:?} ms, the item now at {icon:?}", start.0, start.1, to.0, to.1));
        p.line(format!("  - daemon ticks {:?}", tick_lines(p, seen)));
        let on_shot = grab(display).context("the screen could not be captured with scenery on")?;
        let _ = scooch_core::encode_png(&on_shot.image()).map(|bytes| fs::write(p.out.join("scenery-on.png"), bytes));
        let before = shown();
        let seen = daemon_log(p).lines().count();
        let ran = p.scooch(&["scenery"], LIMIT);
        let switched = Instant::now();
        let off_switch = wait_until(Duration::from_secs(12), || shown() != before).map(|_| switched.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(1200));
        p.line(format!("- scooch scenery said {:?} and the setting is now `{}`, new wallpaper after {off_switch:?} ms", ran.text(), setting(p)));
        p.line(format!("  - daemon ticks {:?}", tick_lines(p, seen)));
        let off_shot = grab(display).context("the screen could not be captured with scenery off")?;
        let _ = scooch_core::encode_png(&off_shot.image()).map(|bytes| fs::write(p.out.join("scenery-off.png"), bytes));
        let painted = icon.as_ref().map(|icon| ring(&on_shot, &off_shot, icon, &screen));
        p.line(format!("- pixels within 40 points around the dragged item that differ between the scenery on and off captures {painted:?}"));
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let current = shown();
        let matched = current.as_deref().and_then(|file| matches_file(&off_shot, file, &icons, &screen));
        p.line(format!("- share of sample points away from the icons where the scenery off capture matches the wallpaper file {current:?} {matched:?}"));
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let back = position(p, name).unwrap_or(to);
        let before = shown();
        let seen = daemon_log(p).lines().count();
        drag(back, subject);
        let dropped = Instant::now();
        let off_time = wait_until(Duration::from_secs(12), || shown() != before).map(|_| dropped.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(600));
        p.line(format!("- scenery off, CGEvent drag onto the subject at {:.0},{:.0}: new wallpaper after {off_time:?} ms", subject.0, subject.1));
        p.line(format!("  - daemon ticks {:?}", tick_lines(p, seen)));
        let seen = daemon_log(p).lines().count();
        let mut times = Vec::new();
        for _ in 0..5 {
            away(p, name, from);
            let before = shown();
            let moved = Instant::now();
            let _ = place(p, name, subject);
            times.extend(wait_until(Duration::from_secs(12), || shown() != before).map(|_| moved.elapsed().as_millis()));
            std::thread::sleep(Duration::from_millis(400));
        }
        times.sort_unstable();
        p.line(format!("- scenery off, Apple Event moves onto the subject that brought a new wallpaper {} of 5, times {times:?} ms, median {:?}, render laps {:?} ms, to set beside the scenery on moves in section 5", times.len(), times.get(times.len() / 2), render_laps(p, seen)));
        let ran = p.scooch(&["scenery"], LIMIT);
        p.line(format!("- scooch scenery said {:?} and the setting is back to `{}`", ran.text(), setting(p)));
        if on_time.is_none() || off_switch.is_none() || off_time.is_none() {
            p.fail("a scenery step brought no new wallpaper");
        }
        if painted.is_none_or(|count| count < 200) {
            p.fail("no painted scenery showed around the dragged item");
        }
        if matched.is_none_or(|share| share < 0.9) {
            p.fail("the scenery off capture does not match the scenery off render");
        }
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

    fn threads(pid: i32) -> Vec<(u64, String, i32, u64)> {
        let mut handles = vec![0u64; 512];
        let bytes = unsafe { proc_pidinfo(pid, 6, 0, handles.as_mut_ptr() as *mut c_void, (handles.len() * 8) as i32) };
        let count = (bytes.max(0) as usize / 8).min(handles.len());
        let mut found = Vec::new();
        for handle in &handles[..count] {
            let mut info: ThreadInfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<ThreadInfo>() as i32;
            if unsafe { proc_pidinfo(pid, 5, *handle, &mut info as *mut ThreadInfo as *mut c_void, size) } == size {
                let end = info.name.iter().position(|byte| *byte == 0).unwrap_or(info.name.len());
                let name = String::from_utf8_lossy(&info.name[..end]).into_owned();
                found.push((*handle, if name.is_empty() { "unnamed".to_string() } else { name }, info.run_state, info.user + info.system));
            }
        }
        found
    }

    fn stacks(text: &str) -> Vec<(u64, Vec<String>)> {
        let mut found = Vec::new();
        let mut frames: Vec<String> = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            match line.parse::<u64>() {
                Ok(count) if !frames.is_empty() => found.push((count, std::mem::take(&mut frames))),
                Ok(_) => frames.clear(),
                Err(_) => frames.push(line.to_string()),
            }
        }
        found.sort_by_key(|stack| std::cmp::Reverse(stack.0));
        found
    }

    fn quiet_minute(p: &mut Probe, pid: i32, label: &str) -> Result<()> {
        let age = p.tool("/bin/ps", &["-o", "etime=", "-p", &pid.to_string()]).out.trim().to_string();
        let seen = daemon_log(p).lines().count();
        let traced = p.out.join(format!("oncpu-{pid}-{}.txt", now()));
        let script = format!("sched:::on-cpu /pid == {pid}/ {{ @[ustack(32)] = count(); }} tick-60s {{ exit(0); }}");
        let dtrace = Command::new("/usr/bin/sudo").args(["-n", "/usr/sbin/dtrace", "-q", "-x", "ustackframes=32", "-o", &traced.to_string_lossy(), "-n", &script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        let threads_before = threads(pid);
        let before = rusage(pid).context("the daemon usage could not be read")?;
        let from = epoch();
        let mut buckets = Vec::new();
        let mut last = before;
        for _ in 0..12 {
            std::thread::sleep(Duration::from_secs(5));
            if let Some(now) = rusage(pid) {
                buckets.push(now.idle_wakeups.saturating_sub(last.idle_wakeups));
                last = now;
            }
        }
        let to = epoch();
        let after = rusage(pid).context("the daemon usage could not be read after the quiet minute")?;
        let threads_after = threads(pid);
        if let Ok(mut child) = dtrace {
            let _ = wait_until(Duration::from_secs(15), || child.try_wait().ok().flatten().is_some());
            let _ = child.kill();
            let _ = child.wait();
        }
        let cpu = nanos((after.user + after.system).saturating_sub(before.user + before.system)) / 1_000_000;
        p.line(format!(
            "- {label} pid {pid}, running for {age} when the minute began, over a quiet 60 s: CPU {cpu} ms, idle wakeups {}, interrupt wakeups {}, footprint {:.1} MB",
            after.idle_wakeups.saturating_sub(before.idle_wakeups),
            after.interrupt_wakeups.saturating_sub(before.interrupt_wakeups),
            after.footprint as f64 / 1_048_576.0
        ));
        p.line(format!("- idle wakeups in each 5 s of that minute {buckets:?}"));
        let quiet = new_lines(&daemon_log(p), seen);
        p.line(format!("- the daemon logged {} ticks and wakes {:?}", quiet.iter().filter(|line| line.contains(" tick ")).count(), quiet.iter().filter_map(|line| line.split(" wake ").nth(1).map(str::trim)).collect::<Vec<_>>()));
        let busy: Vec<String> = threads_after
            .iter()
            .filter_map(|(handle, name, _, total)| {
                let earlier = threads_before.iter().find(|(seen, _, _, _)| seen == handle).map(|(_, _, _, total)| *total)?;
                let spent = total.saturating_sub(earlier) / 1000;
                (spent > 0).then(|| format!("{name} {spent} us"))
            })
            .collect();
        p.line(format!("- {} threads, CPU by thread during the minute {busy:?}", threads_after.len()));
        let stacks = stacks(&fs::read_to_string(&traced).unwrap_or_default());
        p.line(format!("- dtrace on-cpu stacks during the minute {} distinct, {} on-cpu events, kept as {}", stacks.len(), stacks.iter().map(|(count, _)| count).sum::<u64>(), traced.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
        for (count, frames) in stacks.iter().take(8) {
            let shown: Vec<String> = frames.iter().map(|frame| frame.split('+').next().unwrap_or(frame).to_string()).filter(|frame| !frame.starts_with("libsystem_kernel") && !frame.starts_with("scooch`0x")).take(9).collect();
            p.line(format!("  - {count} times: {}", shown.join(" < ")));
        }
        let logged = p.tool("/usr/bin/log", &["show", "--last", "100s", "--info", "--debug", "--style", "ndjson", "--predicate", &format!("processIdentifier == {pid}")]);
        let mut inside = Vec::new();
        for line in logged.out.lines() {
            let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let stamp = entry["timestamp"].as_str().unwrap_or("");
            let when = chrono_free_epoch(stamp);
            if when >= from && when <= to {
                inside.push(format!("+{:.1} s {} {} {}: {}", when - from, entry["senderImagePath"].as_str().and_then(|path| path.rsplit('/').next()).unwrap_or("?"), entry["subsystem"].as_str().unwrap_or(""), entry["category"].as_str().unwrap_or(""), entry["eventMessage"].as_str().unwrap_or("").chars().take(110).collect::<String>()));
            }
        }
        p.line(format!("- unified log entries from the daemon inside the minute {}", inside.len()));
        for entry in inside.iter().take(15) {
            p.line(format!("  - `{entry}`"));
        }
        let file = p.out.join(format!("sample-{pid}-{}.txt", now()));
        let sampled = p.tool("/usr/bin/sample", &[&pid.to_string(), "5", "-file", &file.to_string_lossy()]);
        let text = fs::read_to_string(&file).unwrap_or_default();
        let tops: Vec<String> = text.lines().skip_while(|line| !line.starts_with("Sort by top of stack")).skip(1).take_while(|line| !line.trim().is_empty()).map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        p.line(format!("- sample 5 s after the minute {}, leaf frames {tops:?}", if sampled.ok() { "ok" } else { "failed" }));
        Ok(())
    }

    fn chrono_free_epoch(stamp: &str) -> f64 {
        let parse = || -> Option<f64> {
            let (date, rest) = stamp.split_once(' ')?;
            let mut parts = date.split('-').map(|part| part.parse::<i64>().ok());
            let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
            let clock = rest.get(..15)?;
            let mut time = clock.split(':');
            let (hour, minute) = (time.next()?.parse::<i64>().ok()?, time.next()?.parse::<i64>().ok()?);
            let second = time.next()?.parse::<f64>().ok()?;
            let offset = rest.get(15..)?;
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let offset_minutes = sign * (offset.get(1..3)?.parse::<i64>().ok()? * 60 + offset.get(3..5)?.parse::<i64>().ok()?);
            let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
            let days = 365 * y + y / 4 - y / 100 + y / 400 + (153 * m + 2) / 5 + day - 719469;
            Some((days * 86400 + hour * 3600 + minute * 60 - offset_minutes * 60) as f64 + second)
        };
        parse().unwrap_or(0.0)
    }

    fn idle(p: &mut Probe) -> Result<()> {
        std::thread::sleep(Duration::from_secs(3));
        let pid = scooch_pid(p).context("no scooch daemon runs")?;
        let label = if p.stand.is_some() { "the probe started daemon" } else { "the launchd daemon" };
        let ps = p.tool("/bin/ps", &["-o", "cputime=,rss=,ppid=", "-p", &pid.to_string()]);
        p.line(format!("- ps cputime, rss and parent {}", ps.out.trim()));
        quiet_minute(p, pid, label)
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
        reap(p);
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
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let second = picture_file(p, "probe-second.jpg", &subject_picture(width, height, (0.35, 0.55)))?;
        let third = picture_file(p, "probe-third.jpg", &subject_picture(width, height, (0.65, 0.45)))?;
        let ran = p.scooch(&["use", &second.to_string_lossy()], Duration::from_secs(60));
        p.line(format!("- scooch use from off exit {:?} after {} ms", ran.code, ran.took.as_millis()));
        for line in ran.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(20), || fs::read_to_string(home.join("watch.log")).unwrap_or_default().lines().any(|line| line.contains(" tick ")) || fs::read(home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()).is_some_and(|state| !state["problem"].is_null()));
        let job = daemon_pid(p);
        let parent = job.map(|pid| p.tool("/bin/ps", &["-o", "ppid=", "-p", &pid.to_string()]).out.trim().to_string());
        p.line(format!("- login item loaded {}, daemon pid {job:?} with parent {parent:?}, first tick {:?} ms, problem {:?}", launchd::loaded(), first.map(|time| time.as_millis()), problem(p)));
        if job.is_none() || first.is_none() {
            p.fail("scooch use did not start a daemon under launchd");
        }
        for (what, arguments) in [("scooch use while it runs", vec!["use".to_string(), third.to_string_lossy().into_owned()]), ("scooch flip", vec!["flip".to_string()])] {
            let before = ticks(p);
            let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
            let ran = p.scooch(&arguments, Duration::from_secs(60));
            let home = p.home.clone();
            let ticked = wait_until(Duration::from_secs(15), || fs::read_to_string(home.join("watch.log")).unwrap_or_default().lines().filter(|line| line.contains(" tick ")).count() > before);
            let after = daemon_pid(p);
            p.line(format!("- {what} exit {:?} in {} ms, the same launchd daemon {} ticked {:?} ms later", ran.code, ran.took.as_millis(), after == job, ticked.map(|time| time.as_millis())));
            if ticked.is_none() || after != job {
                if let Some(pid) = after {
                    stuck(p, pid);
                }
                p.fail(format!("{what} did not reach the daemon under launchd"));
            }
        }
        p.line(format!("- mirror saved {}, source now {:?}", state(p)["options"]["mirror"], state(p)["screens"].as_object().and_then(|screens| screens.values().next().map(|entry| entry["source"].clone()))));
        if let Some(pid) = scooch_pid(p) {
            quiet_minute(p, pid, "the launchd daemon waiting for consent")?;
            std::thread::sleep(Duration::from_secs(120));
            let still = problem(p).is_some_and(|problem| problem == finder::ASKING) && windows().iter().any(|(owner, _, _)| owner == "UserNotificationCenter");
            p.line(format!("- two minutes later, the consent window is still open {still}"));
            quiet_minute(p, pid, "the same daemon still waiting for consent")?;
        }
        let ran = p.scooch(&["off"], Duration::from_secs(60));
        p.line(format!("- scooch off exit {:?} after {} ms: {}", ran.code, ran.took.as_millis(), ran.text()));
        std::thread::sleep(Duration::from_millis(1500));
        gone(p, "the second off");
        p.finder = dismiss_prompt(p).is_some();
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

    struct Shot {
        width: usize,
        height: usize,
        rgb: Vec<u8>,
    }

    impl Shot {
        fn at(&self, x: usize, y: usize) -> [u8; 3] {
            let index = (y.min(self.height.saturating_sub(1)) * self.width + x.min(self.width.saturating_sub(1))) * 3;
            [self.rgb[index], self.rgb[index + 1], self.rgb[index + 2]]
        }

        fn image(&self) -> RgbImage {
            RgbImage::from_raw(self.width as u32, self.height as u32, self.rgb.clone()).unwrap_or_default()
        }
    }

    fn unpack(image: *const c_void) -> Option<Shot> {
        if image.is_null() {
            return None;
        }
        let shot = unsafe {
            let (width, height, row, bits) = (CGImageGetWidth(image), CGImageGetHeight(image), CGImageGetBytesPerRow(image), CGImageGetBitsPerPixel(image));
            let data = CGDataProviderCopyData(CGImageGetDataProvider(image));
            let shot = if data.is_null() || bits != 32 {
                None
            } else {
                let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data), CFDataGetLength(data).max(0) as usize);
                let mut rgb = Vec::with_capacity(width * height * 3);
                for y in 0..height {
                    for x in 0..width {
                        let at = y * row + x * 4;
                        rgb.extend_from_slice(&[bytes[at + 2], bytes[at + 1], bytes[at]]);
                    }
                }
                Some(Shot { width, height, rgb })
            };
            if !data.is_null() {
                CFRelease(data);
            }
            CFRelease(image);
            shot
        };
        shot
    }

    static SLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn shot_file(arguments: &[&str]) -> Option<Shot> {
        let file = std::env::temp_dir().join(format!("scooch-shot-{}.png", std::process::id()));
        let done = Command::new("/usr/sbin/screencapture").args(arguments).arg(&file).stdout(Stdio::null()).stderr(Stdio::null()).status().ok()?.success();
        let image = done.then(|| scooch_core::open(&file).ok()).flatten();
        let _ = fs::remove_file(&file);
        let image = image?;
        Some(Shot { width: image.width() as usize, height: image.height() as usize, rgb: image.into_raw() })
    }

    fn grab(display: u32) -> Option<Shot> {
        if let Some(shot) = unpack(unsafe { CGDisplayCreateImage(display) }) {
            return Some(shot);
        }
        SLOW.store(true, std::sync::atomic::Ordering::SeqCst);
        shot_file(&["-x", "-C", "-D", "1", "-t", "png"]).or_else(|| shot_file(&["-x", "-D", "1", "-t", "png"]))
    }

    fn grab_window(number: i64) -> Option<Shot> {
        let null = Frame { x: f64::INFINITY, y: f64::INFINITY, w: 0.0, h: 0.0 };
        if let Some(shot) = unpack(unsafe { CGWindowListCreateImage(null, 8, number as u32, 1) }) {
            return Some(shot);
        }
        SLOW.store(true, std::sync::atomic::Ordering::SeqCst);
        shot_file(&["-x", "-o", &format!("-l{number}"), "-t", "png"])
    }

    fn picture_window(screen: &Screen) -> Option<layer::Seen> {
        let (desktop, _) = layer::levels();
        let mut found: Vec<layer::Seen> = layer::listing().into_iter().filter(|window| window.layer < desktop && layer::is_picture(window) && window.bounds.x < screen.frame.x + screen.frame.w && screen.frame.x < window.bounds.x + window.bounds.w && window.bounds.w >= screen.frame.w * 0.9).collect();
        found.sort_by_key(|window| std::cmp::Reverse(window.layer));
        found.into_iter().next()
    }

    fn magenta(pixel: [u8; 3]) -> bool {
        pixel[0] > 180 && pixel[1] < 100 && pixel[2] > 180
    }

    fn bar_at(shot: &Shot, row: usize) -> Option<f32> {
        let hits: Vec<usize> = (0..shot.width).filter(|x| magenta(shot.at(*x, row))).collect();
        (!hits.is_empty()).then(|| hits.iter().sum::<usize>() as f32 / hits.len() as f32)
    }

    fn near(pixel: [u8; 3], color: [u8; 3]) -> bool {
        (0..3).map(|channel| (pixel[channel] as i32 - color[channel] as i32).abs()).sum::<i32>() < 60
    }

    fn rgba(image: &RgbImage, width: u32, height: u32) -> Vec<u8> {
        let fitted = if image.width() == width && image.height() == height {
            image.clone()
        } else {
            let scene = scooch_core::Scene { width, height, icons: Vec::new() };
            let placement = scooch_core::fill(image.width(), image.height(), &scene);
            scooch_core::render(image, &placement, &scene, &scooch_core::Options::default())
        };
        fitted.pixels().flat_map(|pixel| [pixel.0[0], pixel.0[1], pixel.0[2], 255]).collect()
    }

    fn stats(values: &[f64]) -> String {
        if values.is_empty() {
            return "none".into();
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let pick = |share: f64| sorted[((sorted.len() - 1) as f64 * share).round() as usize];
        format!("{} values, median {:.2} ms, p99 {:.2} ms, max {:.2} ms", sorted.len(), pick(0.5), pick(0.99), sorted[sorted.len() - 1])
    }

    fn over(values: &[f64], period: f64) -> usize {
        values.iter().filter(|value| **value > period * 1.5).count()
    }

    fn ports() -> Option<u32> {
        unsafe {
            let task = mach_task_self_;
            let (mut names, mut count, mut types, mut type_count) = (std::ptr::null_mut(), 0u32, std::ptr::null_mut(), 0u32);
            if mach_port_names(task, &mut names, &mut count, &mut types, &mut type_count) != 0 {
                return None;
            }
            vm_deallocate(task, names as usize, count as usize * 4);
            vm_deallocate(task, types as usize, type_count as usize * 4);
            Some(count)
        }
    }

    fn ours() -> Vec<layer::Seen> {
        let pid = std::process::id() as i64;
        layer::every_window().into_iter().filter(|window| window.pid == pid).collect()
    }

    fn accessory(marker: MainThreadMarker) {
        NSApplication::sharedApplication(marker).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }

    fn table(p: &mut Probe, screen: &Screen) {
        let (desktop, icons) = layer::levels();
        p.line(format!("- desktop level {desktop}, desktop icon level {icons}"));
        p.line("- on-screen windows from the bottom up to just above the icons, owner, layer, number, name, bounds".to_string());
        let mut seen: Vec<layer::Seen> = layer::listing().into_iter().filter(|window| window.layer <= icons + 5).collect();
        seen.sort_by_key(|window| window.layer);
        for window in seen {
            let here = window.bounds.x < screen.frame.x + screen.frame.w && screen.frame.x < window.bounds.x + window.bounds.w;
            p.line(format!("  - `{}` layer {} ({:+} from desktop) number {} name `{}` bounds {} on this screen {here}", window.owner, window.layer, window.layer - desktop, window.number, window.name, show(&window.bounds)));
        }
    }

    struct Run {
        frames: Vec<Instant>,
        arrivals: Vec<f64>,
        outputs: Vec<f64>,
        callbacks: usize,
        rate: f64,
        hidden: usize,
    }

    fn animate(renderer: Renderer, display: u32, cross: f64, hold: f64, park: Option<f32>) -> std::thread::JoinHandle<(Renderer, Run)> {
        std::thread::spawn(move || {
            let mut renderer = renderer;
            let pace = layer::Pace::start(display);
            let rate = pace.as_ref().map_or(0.0, |pace| pace.rate);
            let begin = Instant::now();
            let mut frames = Vec::new();
            let mut hidden = 0;
            loop {
                let t = begin.elapsed().as_secs_f64();
                let bar = if t < cross {
                    Some((t / cross) as f32)
                } else if t < cross + hold {
                    park
                } else {
                    break;
                };
                match layer::present(|| renderer.frame(bar)) {
                    Ok(()) => frames.push(Instant::now()),
                    Err(error) if error.contains("occluded") => {
                        hidden += 1;
                        std::thread::sleep(Duration::from_millis(4));
                    }
                    Err(_) => break,
                }
            }
            let paced = pace.map(|pace| pace.stop());
            let millis = |values: Vec<f64>| values.into_iter().map(|seconds| seconds * 1000.0).collect::<Vec<f64>>();
            let (arrivals, outputs, callbacks) = paced.map_or((Vec::new(), Vec::new(), 0), |paced| (millis(paced.arrivals), millis(paced.outputs), paced.callbacks));
            (renderer, Run { frames, arrivals, outputs, callbacks, rate, hidden })
        })
    }

    struct Look {
        at: f64,
        bar: Option<f32>,
        changed: Vec<f32>,
        covered: Vec<f32>,
        through: usize,
    }

    fn intervals(frames: &[Instant]) -> Vec<f64> {
        frames.windows(2).map(|pair| (pair[1] - pair[0]).as_secs_f64() * 1000.0).collect()
    }

    fn report_run(p: &mut Probe, run: &Run) {
        let period = if run.rate > 0.0 { 1000.0 / run.rate } else { 16.67 };
        let presents = intervals(&run.frames);
        p.line(format!("- display link nominal rate {:.2} Hz, {} callbacks, arrival intervals {}, longer than 1.5 periods {}", run.rate, run.callbacks, stats(&run.arrivals), over(&run.arrivals, period)));
        p.line(format!("- display link output times {}, longer than 1.5 periods {}", stats(&run.outputs), over(&run.outputs, period)));
        p.line(format!("- frames presented {}, skipped while macOS called the window occluded {}, intervals {}, longer than 1.5 periods {}", run.frames.len(), run.hidden, stats(&presents), over(&presents, period)));
    }

    fn cycles(p: &mut Probe, marker: MainThreadMarker, screen: &Screen, shared: bool) -> Result<()> {
        let pid = std::process::id() as i32;
        let pattern = |layer: &layer::Layer| Pattern { width: layer.pixels.0, height: layer.pixels.1, rgba: GRAY.iter().copied().chain([255]).cycle().take(layer.pixels.0 as usize * layer.pixels.1 as usize * 4).collect() };
        let before = (ours().len(), ports(), threads(pid).len(), rusage(pid));
        let mut opens = Vec::new();
        let mut shown = Vec::new();
        let mut stages = [0usize; 2];
        let mut first: Vec<String> = Vec::new();
        for index in 0..20 {
            let started = Instant::now();
            let layer = layer::open(marker, screen)?;
            shown.push(started.elapsed().as_secs_f64() * 1000.0);
            let gpu = match (&p.gpu, shared) {
                (Some(gpu), true) => gpu.clone(),
                _ => Gpu::new(false).map_err(anyhow::Error::msg)?,
            };
            let mut renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &pattern(&layer)).map_err(anyhow::Error::msg)?;
            layer.commit();
            layer::present(|| renderer.frame(Some(0.2))).map_err(anyhow::Error::msg)?;
            opens.push(started.elapsed().as_secs_f64() * 1000.0);
            layer::present(|| renderer.frame(Some(0.5))).map_err(anyhow::Error::msg)?;
            layer::present(|| renderer.frame(Some(0.8))).map_err(anyhow::Error::msg)?;
            let traces = layer.traces();
            let open_held = traces.held();
            drop(renderer);
            drop(gpu);
            let rendered_held = traces.held();
            drop(layer);
            let dropped = traces.held();
            let windows_dropped = ours().len();
            layer::spin(0.05);
            let spun = traces.held();
            for (slot, held) in [dropped, spun].iter().enumerate() {
                if held[0].is_some() {
                    stages[slot] += 1;
                }
            }
            if index == 0 {
                first = vec![
                    format!("open {open_held:?}"),
                    format!("renderer dropped {rendered_held:?}"),
                    format!("layer dropped {dropped:?} with our windows {windows_dropped}"),
                    format!("run loop spun {spun:?} with our windows {}", ours().len()),
                ];
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        let after = (ours().len(), ports(), threads(pid).len(), rusage(pid));
        let memory = |usage: Option<Usage>| usage.map(|usage| (usage.resident as f64 / 1_048_576.0, usage.footprint as f64 / 1_048_576.0));
        p.line(format!(
            "- twenty opens with {} device: our windows {} then {}, Mach ports {:?} then {:?}, threads {} then {}, resident and footprint MB {:?} then {:?}",
            if shared { "one shared" } else { "a fresh" },
            before.0,
            after.0,
            before.1,
            after.1,
            before.2,
            after.2,
            memory(before.3),
            memory(after.3)
        ));
        p.line(format!("- open to visible {}, open to first frame {}", stats(&shown), stats(&opens)));
        p.line(format!("- references still held on the window, view and Metal layer in the first cycle, beyond the probe's own {first:?}"));
        p.line(format!("- of twenty windows, still alive after the layer dropped {}, after the run loop spun {}", stages[0], stages[1]));
        Ok(())
    }

    fn live(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let desktop = platform::current()?;
        let screen = desktop.screens()?.into_iter().next().context("no screen shows a desktop")?;
        let recorded = desktop.wallpaper(&screen).ok().flatten();
        p.line(format!("- screen `{}` {} scale {}, wallpaper recorded {recorded:?}", screen.id, show(&screen.frame), screen.scale));
        table(p, &screen);
        let (level, icons) = layer::levels();
        let chosen = layer::choose(&layer::listing(), &screen.frame, level, std::process::id() as i64);
        p.line(format!("- chosen level {chosen:?}, the desktop level, above the picture window and {} below the icon level", icons - level));
        let widgets: Vec<String> = layer::listing().into_iter().filter(|window| ["Widget", "NotificationCenter", "chronod"].iter().any(|word| window.owner.contains(word))).map(|window| format!("{} layer {}", window.owner, window.layer)).collect();
        p.line(format!("- widget windows {widgets:?}"));
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let baseline = grab(display).context("CGDisplayCreateImage gave nothing")?;
        let timed = Instant::now();
        let _ = grab(display);
        let grab_ms = timed.elapsed().as_secs_f64() * 1000.0;
        let cross = (grab_ms * 30.0 / 1000.0).clamp(0.5, 4.0);
        p.line(format!("- one screen capture takes {grab_ms:.0} ms here, so the bar crosses in {cross:.2} s to be caught several times on the way"));
        let _ = scooch_core::encode_png(&baseline.image()).map(|bytes| fs::write(p.out.join("live-before.png"), bytes));
        let paper = picture_window(&screen).context("no wallpaper window was found for this screen")?;
        let field = grab_window(paper.number).context("the wallpaper window could not be captured")?;
        p.line(format!("- field taken from the wallpaper window `{}` number {} layer {}, {}x{}", paper.owner, paper.number, paper.layer, field.width, field.height));
        let named: Vec<(String, Rect)> = finder::named(&finder::view()).unwrap_or_default().into_iter().filter(|(name, _)| name.starts_with("scooch")).collect();
        let scale = screen.scale;
        let field_color = field.at(field.width / 2, field.height * 3 / 4);
        let masks: Vec<(String, Vec<(usize, usize)>)> = named
            .iter()
            .map(|(name, rect)| {
                let mut ink = Vec::new();
                for y in ((rect.y - screen.frame.y) * scale) as usize..((rect.y - screen.frame.y + rect.h) * scale) as usize {
                    for x in ((rect.x - screen.frame.x) * scale) as usize..((rect.x - screen.frame.x + rect.w) * scale) as usize {
                        if !near(baseline.at(x, y), field.at(x, y)) {
                            ink.push((x, y));
                        }
                    }
                }
                (name.clone(), ink)
            })
            .collect();
        p.line(format!("- icon ink pixels {:?}", masks.iter().map(|(name, ink)| format!("{name} {}", ink.len())).collect::<Vec<_>>()));
        let cyan = picture_file(p, "probe-cyan.jpg", &RgbImage::from_raw(64, 64, [40u8, 200, 220].repeat(64 * 64)).unwrap_or_default())?;
        let opened = Instant::now();
        let layer = layer::open(marker, &screen)?;
        let (width, height) = layer.pixels;
        let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
        let device_ready = opened.elapsed();
        let renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &Pattern { width, height, rgba: rgba(&field.image(), width, height) }).map_err(anyhow::Error::msg)?;
        layer.commit();
        p.gpu = Some(gpu.clone());
        p.line(format!("- layer number {} at level {}, {}x{} pixels, adapter {}, device ready after {} ms", layer.number(), layer.level, width, height, gpu.name, device_ready.as_millis()));
        let row = (height as f32 * 0.55) as usize;
        let samples: Vec<(usize, usize)> = [(0.1, 0.7), (0.5, 0.7), (0.7, 0.7), (0.9, 0.7), (0.55, 0.45), (0.85, 0.45)].iter().map(|(x, y)| ((width as f32 * x) as usize, (height as f32 * y) as usize)).collect();
        let set_after = opened.elapsed().as_secs_f64() * 1000.0 + cross * 1000.0 + 400.0;
        let worker = animate(renderer, layer.display, cross, 4.0, Some(0.3));
        let mut looks: Vec<Look> = Vec::new();
        let mut set_at: Option<Instant> = None;
        let mut system_shows: Option<f64> = None;
        let mut saved = 0;
        while !worker.is_finished() {
            let at = opened.elapsed().as_secs_f64() * 1000.0;
            let Some(shot) = grab(display) else { continue };
            let bar = bar_at(&shot, row);
            let share = |test: &dyn Fn(usize, usize) -> bool| -> Vec<f32> { masks.iter().map(|(_, ink)| ink.iter().filter(|(x, y)| test(*x, *y)).count() as f32 / ink.len().max(1) as f32).collect() };
            let changed = share(&|x, y| !near(shot.at(x, y), baseline.at(x, y)));
            let covered = share(&|x, y| near(shot.at(x, y), field.at(x, y)) || magenta(shot.at(x, y)));
            let through = samples.iter().filter(|(x, y)| near(shot.at(*x, *y), [40, 200, 220])).count();
            if (saved == 0 && at > 250.0) || (saved == 1 && set_at.is_some_and(|set| set.elapsed().as_millis() > 1500)) {
                let _ = scooch_core::encode_png(&shot.image()).map(|bytes| fs::write(p.out.join(format!("live-during-{saved}.png")), bytes));
                saved += 1;
            }
            looks.push(Look { at, bar, changed, covered, through });
            if set_at.is_none() && at > set_after {
                desktop.set_wallpaper(&screen, &cyan)?;
                set_at = Some(Instant::now());
            }
            if let (Some(set), None) = (set_at, system_shows) {
                let shown = picture_window(&screen).and_then(|window| grab_window(window.number)).is_some_and(|window| near(window.at(window.width / 2, window.height * 3 / 4), [40, 200, 220]));
                if shown {
                    system_shows = Some(set.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
        let (renderer, run) = worker.join().map_err(|_| anyhow::anyhow!("the render thread panicked"))?;
        drop(renderer);
        let traces = layer.traces();
        layer.close();
        let closed = Instant::now();
        layer::spin(0.4);
        let after = grab(display);
        let leftover = ours();
        let held = traces.held();
        let first = run.frames.first().map(|frame| (*frame - opened).as_secs_f64() * 1000.0);
        let last = run.frames.last().map(|frame| (*frame - opened).as_secs_f64() * 1000.0);
        p.line(format!("- cold open to first presented frame {:?} ms, last frame at {:?} ms, captures used the screencapture tool as a fallback {}", first.map(|value| value.round()), last.map(|value| value.round()), SLOW.load(std::sync::atomic::Ordering::SeqCst)));
        report_run(p, &run);
        let period = if run.rate > 0.0 { 1000.0 / run.rate } else { 1000.0 / 60.0 };
        let crossing: Vec<(f64, f32)> = looks.iter().filter(|look| first.is_some_and(|first| look.at >= first && look.at <= first + cross * 1000.0)).filter_map(|look| look.bar.map(|bar| (look.at, bar))).collect();
        let steady = crossing.windows(2).all(|pair| pair[1].1 >= pair[0].1 - 2.0);
        p.line(format!("- {} captures in all, {} during the {:.0} ms crossing, bar centres in pixels {:?}, increasing {steady}", looks.len(), crossing.len(), cross * 1000.0, crossing.iter().map(|(at, bar)| format!("{:.0} ms {:.0}", at, bar)).collect::<Vec<_>>()));
        let inside: Vec<&Look> = looks.iter().filter(|look| first.zip(last).is_some_and(|(first, last)| look.at >= first && look.at <= last)).collect();
        let settled_from = first.map(|first| first + 4.0 * period);
        let bare_times: Vec<f64> = inside.iter().filter(|look| look.bar.is_none()).map(|look| look.at).collect();
        let bare = bare_times.iter().filter(|at| settled_from.is_some_and(|from| **at > from)).count();
        p.line(format!("- captures started between the first and last pattern frame {}, without the layer {} at {:?} ms, of which started more than four refreshes ({:.0} ms) after the first frame was queued {bare}", inside.len(), bare_times.len(), bare_times.iter().map(|at| at.round()).collect::<Vec<_>>(), 4.0 * period));
        let worst = |pick: fn(&Look) -> &Vec<f32>| -> Vec<String> { masks.iter().enumerate().map(|(index, (name, _))| format!("{name} {:.3}", looks.iter().map(|look| pick(look).get(index).copied().unwrap_or_default()).fold(0.0f32, f32::max))).collect() };
        p.line(format!("- largest share of icon ink that changed in any capture, edge and shadow pixels blend with whatever lies below so they move with the bar {:?}", worst(|look| &look.changed)));
        p.line(format!("- largest share of icon ink showing only what the layer drew, as it would if the layer covered the icon {:?}", worst(|look| &look.covered)));
        let marked: Vec<String> = looks.iter().filter(|look| look.covered.iter().any(|share| *share > 0.5)).map(|look| format!("{:.0} ms {:?}", look.at, look.covered.iter().map(|share| (share * 1000.0).round() / 1000.0).collect::<Vec<_>>())).collect();
        p.line(format!("- captures where the layer covered more than half of an icon's ink {} {:?}", marked.len(), marked.iter().take(6).collect::<Vec<_>>()));
        let through = looks.iter().filter(|look| look.through > 0 && first.zip(last).is_some_and(|(first, last)| look.at >= first && look.at <= last)).count();
        p.line(format!("- wallpaper set {:?} ms after open, the system's own window showed it {:?} ms after the set, captures where the new picture showed through or above the layer {through}", set_at.map(|set| (set - opened).as_millis()), system_shows.map(|value| value.round())));
        let after_bar = after.as_ref().and_then(|shot| bar_at(shot, row));
        let after_cyan = after.as_ref().map(|shot| samples.iter().filter(|(x, y)| near(shot.at(*x, *y), [40, 200, 220])).count());
        if let Some(shot) = &after {
            let _ = scooch_core::encode_png(&shot.image()).map(|bytes| fs::write(p.out.join("live-after.png"), bytes));
        }
        p.line(format!("- after close at +{} ms: bar on screen {after_bar:?}, field samples showing the new picture {after_cyan:?} of {}, windows of ours {}, window, view and Metal layer still alive {held:?}", closed.elapsed().as_millis(), samples.len(), leftover.len()));
        if !steady || crossing.len() < 2 || bare > 0 || through > 0 || after_bar.is_some() || !leftover.is_empty() || after_cyan != Some(samples.len()) {
            p.fail("the live layer did not sit cleanly under the icons and go");
        }
        if !marked.is_empty() {
            p.fail("the layer covered an icon");
        }
        cycles(p, marker, &screen, true)?;
        cycles(p, marker, &screen, false)?;
        if let Err(error) = helpers(p) {
            p.fail(format!("the helper process could not be measured {error:#}"));
        }
        let _ = field_color;
        if let Some(recorded) = recorded.filter(|path| path.is_file()) {
            let _ = desktop.set_wallpaper(&screen, &recorded);
        }
        Ok(())
    }

    fn spin(seconds: f64) {
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 0) };
    }

    fn memory(pid: i32) -> String {
        rusage(pid).map_or("unread".into(), |usage| format!("footprint {:.1} MB resident {:.1} MB", usage.footprint as f64 / 1_048_576.0, usage.resident as f64 / 1_048_576.0))
    }

    fn front() -> String {
        NSWorkspace::sharedWorkspace().frontmostApplication().and_then(|application| application.bundleIdentifier()).map(|name| name.to_string()).unwrap_or_default()
    }

    fn helper() -> ExitCode {
        let pid = std::process::id() as i32;
        let started = Instant::now();
        let say = |stage: &str, extra: String| {
            println!("stage {stage} at {} ms, {}{extra}", started.elapsed().as_millis(), memory(pid));
            let _ = std::io::stdout().flush();
        };
        say("start", String::new());
        let result = (|| -> Result<()> {
            let marker = MainThreadMarker::new().context("not on the main thread")?;
            let before = front();
            let application = NSApplication::sharedApplication(marker);
            application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            application.finishLaunching();
            say("app", String::new());
            let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
            say("device", format!(", {}", gpu.name));
            let screen = platform::current()?.screens()?.into_iter().next().context("no screen")?;
            let layer = layer::open(marker, &screen)?;
            let (width, height) = layer.pixels;
            let mut renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &Pattern { width, height, rgba: GRAY.iter().copied().chain([255]).cycle().take(width as usize * height as usize * 4).collect() }).map_err(anyhow::Error::msg)?;
            layer.commit();
            layer::present(|| renderer.frame(Some(0.0))).map_err(anyhow::Error::msg)?;
            say("frame", String::new());
            for index in 1..30 {
                layer::present(|| renderer.frame(Some(index as f32 / 30.0))).map_err(anyhow::Error::msg)?;
            }
            spin(0.3);
            let me = NSRunningApplication::currentApplication();
            say("shown", format!(", front app before {before} after {}, this helper active {}, policy {:?}", front(), me.isActive(), me.activationPolicy()));
            drop(renderer);
            let traces = layer.traces();
            drop(layer);
            layer::spin(0.3);
            say("closed", format!(", window, view and Metal layer still alive {:?}, windows of ours {}", traces.held(), ours().len()));
            drop(gpu);
            spin(0.5);
            say("dropped", String::new());
            let calm = rusage(pid);
            spin(5.0);
            let later = rusage(pid);
            let woke = calm.zip(later).map(|(calm, later)| (later.idle_wakeups.saturating_sub(calm.idle_wakeups), later.interrupt_wakeups.saturating_sub(calm.interrupt_wakeups)));
            say("idle", format!(", idle and interrupt wakeups over the last 5 s {woke:?}"));
            Ok(())
        })();
        if let Err(error) = result {
            println!("stage failed {error:#}");
        }
        ExitCode::SUCCESS
    }

    fn helpers(p: &mut Probe) -> Result<()> {
        use std::io::BufRead as _;
        let exe = std::env::current_exe()?;
        for round in ["first", "second"] {
            let spawned = Instant::now();
            let mut child = Command::new(&exe).arg("helper").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
            let pid = child.id();
            let lines: Vec<(u128, String)> = std::io::BufReader::new(child.stdout.take().context("no helper output")?).lines().map_while(|line| line.ok()).map(|line| (spawned.elapsed().as_millis(), line)).collect();
            let status = child.wait()?;
            p.line(format!("- {round} helper process {pid} exited {:?} after {} ms", status.code(), spawned.elapsed().as_millis()));
            for (at, line) in &lines {
                p.line(format!("  - +{at} ms from spawn: {line}"));
            }
            let logged = p.tool("/usr/bin/log", &["show", "--last", "60s", "--info", "--debug", "--style", "ndjson", "--predicate", &format!("processIdentifier == {pid}")]);
            let launch: Vec<String> = logged.out.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()).filter(|entry| entry["senderImagePath"].as_str().is_some_and(|path| path.contains("launch_measurement"))).map(|entry| format!("{} {}", entry["timestamp"].as_str().unwrap_or(""), entry["eventMessage"].as_str().unwrap_or("").chars().take(100).collect::<String>())).collect();
            p.line(format!("  - launch measurement log lines {launch:?}"));
        }
        Ok(())
    }

    fn space_layer(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let screen = platform::current()?.screens()?.into_iter().next().context("no screen")?;
        let layer = layer::open(marker, &screen)?;
        let gpu = match &p.gpu {
            Some(gpu) => gpu.clone(),
            None => Gpu::new(false).map_err(anyhow::Error::msg)?,
        };
        let (width, height) = layer.pixels;
        let mut renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &Pattern { width, height, rgba: GRAY.iter().copied().chain([255]).cycle().take(width as usize * height as usize * 4).collect() }).map_err(anyhow::Error::msg)?;
        layer.commit();
        layer::present(|| renderer.frame(Some(0.5))).map_err(anyhow::Error::msg)?;
        let number = layer.number();
        let showing = || layer::listing().iter().any(|window| window.number == number);
        let one = showing();
        switch(p, 124);
        let two = showing();
        switch(p, 123);
        let back = showing();
        drop(renderer);
        layer.close();
        p.line(format!("- the layer on the first Space is listed on screen there {one}, on the second Space {two}, back on the first {back}"));
        if !one || two || !back {
            p.fail("the layer did not stay on its own Space");
        }
        Ok(())
    }

    fn pace(out: PathBuf) -> ExitCode {
        let _ = fs::create_dir_all(&out);
        let mut lines = vec!["# Scooch layer pacing on this Mac".to_string(), String::new()];
        let result = (|| -> Result<()> {
            let marker = MainThreadMarker::new().context("not on the main thread")?;
            accessory(marker);
            let desktop = platform::current()?;
            let screen = desktop.screens()?.into_iter().next().context("no screen shows a desktop")?;
            let source = picture::reported(marker, &screen.id).ok().flatten().filter(|path| path.is_file()).unwrap_or_else(|| PathBuf::from(store::DEFAULT_DESKTOP));
            let picture = desktop.decode(&source)?;
            lines.push(format!("- screen `{}` {} scale {}, field from {}", screen.id, show(&screen.frame), screen.scale, source.display()));
            let pid = std::process::id() as i32;
            lines.push(format!("- before any device {}", memory(pid)));
            let made = Instant::now();
            let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
            lines.push(format!("- device {} ready after {} ms, {}", gpu.name, made.elapsed().as_millis(), memory(pid)));
            let mut field: Option<Vec<u8>> = None;
            for index in 0..5 {
                let started = Instant::now();
                let layer = layer::open(marker, &screen)?;
                let shown = started.elapsed().as_millis();
                let (width, height) = layer.pixels;
                let rgba = field.get_or_insert_with(|| rgba(&picture, width, height)).clone();
                let renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &Pattern { width, height, rgba }).map_err(anyhow::Error::msg)?;
                layer.commit();
                let (renderer, run) = animate(renderer, layer.display, 0.5, 0.6, None).join().map_err(|_| anyhow::anyhow!("the render thread panicked"))?;
                drop(renderer);
                let traces = layer.traces();
                layer.close();
                let lifetime = started.elapsed().as_millis();
                layer::spin(0.1);
                let first = run.frames.first().map(|frame| (*frame - started).as_millis());
                let period = if run.rate > 0.0 { 1000.0 / run.rate } else { 16.67 };
                let presents = intervals(&run.frames);
                lines.push(format!("- open {} level {}, {}x{}, open to visible {shown} ms, open to first frame {first:?} ms, window open for {lifetime} ms, window, view and Metal layer alive after close {:?}, windows of ours {}", index + 1, layer::levels().0, width, height, traces.held(), ours().len()));
                lines.push(format!("  - display link {:.2} Hz, {} callbacks, arrival intervals {}, longer than 1.5 periods {}", run.rate, run.callbacks, stats(&run.arrivals), over(&run.arrivals, period)));
                lines.push(format!("  - display link output times {}, longer than 1.5 periods {}", stats(&run.outputs), over(&run.outputs, period)));
                lines.push(format!("  - frames {}, skipped while occluded {}, intervals {}, longer than 1.5 periods {}", run.frames.len(), run.hidden, stats(&presents), over(&presents, period)));
                std::thread::sleep(Duration::from_millis(300));
            }
            lines.push(format!("- with the device after five opens {}", memory(pid)));
            drop(gpu);
            spin(0.5);
            lines.push(format!("- after the device was dropped {}", memory(pid)));
            lines.push(format!("- windows of ours left {}", ours().len()));
            Ok(())
        })();
        if let Err(error) = result {
            lines.push(format!("- **stopped** {error:#}"));
        }
        for line in &lines {
            println!("{line}");
        }
        let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
        ExitCode::SUCCESS
    }

    fn test_still(width: u32, height: u32) -> RgbImage {
        let third = (height / 3).max(1);
        let bars: [[u8; 3]; 10] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0], [0, 255, 255], [255, 0, 255], [255, 255, 255], [30, 30, 30], [128, 128, 128], [200, 120, 60]];
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let pixel = if y < third {
                    [(x * 255 / width.max(1)) as u8, (y * 255 / third) as u8, 160]
                } else if y < 2 * third {
                    bars[(x as usize * bars.len() / width.max(1) as usize).min(bars.len() - 1)]
                } else if (x + y) % 2 == 0 {
                    [250, 250, 250]
                } else {
                    [10, 10, 10]
                };
                rgb.extend_from_slice(&pixel);
            }
        }
        RgbImage::from_raw(width, height, rgb).unwrap_or_default()
    }

    fn still_file(path: &Path, image: &RgbImage) -> Result<()> {
        let mut out = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut out, 100);
        encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
        if let Some(profile) = picture::profile() {
            encoder.add_icc_profile(&profile)?;
        }
        encoder.encode(image.as_raw(), image.width() as u16, image.height() as u16, jpeg_encoder::ColorType::Rgb)?;
        fs::write(path, out)?;
        Ok(())
    }

    struct Difference {
        mean: f64,
        p99: u8,
        max: u8,
        over: usize,
        counted: usize,
    }

    fn difference(a: &Shot, b: &Shot, keep: &dyn Fn(usize, usize) -> bool) -> Difference {
        let mut counts = [0usize; 256];
        for y in 0..a.height.min(b.height) {
            for x in 0..a.width.min(b.width) {
                if keep(x, y) {
                    let (one, two) = (a.at(x, y), b.at(x, y));
                    let worst = (0..3).map(|channel| one[channel].abs_diff(two[channel])).max().unwrap_or(0);
                    counts[worst as usize] += 1;
                }
            }
        }
        let counted: usize = counts.iter().sum();
        let total: usize = counts.iter().enumerate().map(|(value, count)| value * count).sum();
        let mut seen = 0;
        let mut p99 = 0u8;
        for (value, count) in counts.iter().enumerate() {
            seen += count;
            if seen * 100 >= counted * 99 {
                p99 = value as u8;
                break;
            }
        }
        let max = counts.iter().rposition(|count| *count > 0).unwrap_or(0) as u8;
        Difference { mean: total as f64 / counted.max(1) as f64, p99, max, over: counts[3..].iter().sum(), counted }
    }

    fn calm_shot(display: u32) -> Option<Shot> {
        let mut last = grab(display)?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(300));
            let next = grab(display)?;
            let same = difference(&last, &next, &|_, _| true).max == 0;
            last = next;
            if same {
                break;
            }
        }
        Some(last)
    }

    fn away_from_icons(screen: &Screen) -> impl Fn(usize, usize) -> bool {
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let (scale, frame) = (screen.scale, screen.frame);
        move |x, y| {
            let (px, py) = (x as f32 / scale + frame.x, y as f32 / scale + frame.y);
            py - frame.y >= 30.0 && !icons.iter().any(|icon| px >= icon.x - 12.0 && px < icon.x + icon.w + 12.0 && py >= icon.y - 12.0 && py < icon.y + icon.h + 12.0)
        }
    }

    fn still_through_helper(p: &mut Probe, screen: &Screen, label: &str) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        let desktop = platform::current()?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let (width, height) = pixels(screen);
        let file = p.home.join(format!("probe-still-{width}x{height}.jpg"));
        still_file(&file, &test_still(width, height))?;
        desktop.set_wallpaper(screen, &file)?;
        std::thread::sleep(Duration::from_millis(800));
        let plain = calm_shot(display).context("the plain desktop could not be captured")?;
        let mut helper = scooch::live::Helper::spawn(&p.bin, Duration::from_secs(5))?;
        let ready = helper.warm(scooch::live::Hints::default(), None, None, Duration::from_secs(30))?;
        let shown = helper.open(1, screen, &file, Duration::from_secs(5))?;
        std::thread::sleep(Duration::from_millis(400));
        let layered = grab(display).context("the layer could not be captured")?;
        let keep = away_from_icons(screen);
        let found = difference(&plain, &layered, &keep);
        let pid = helper.pid as i64;
        let closed = helper.close(1, Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(300));
        let left = layer::every_window().into_iter().filter(|window| window.pid == pid).count();
        drop(helper);
        let _ = scooch_core::encode_png(&layered.image()).map(|bytes| fs::write(p.out.join(format!("still-{width}x{height}.png")), bytes));
        p.line(format!(
            "- {label}: still {width}x{height} on a {}x{} point screen at scale {}, adapter {} ready after {} ms, open to shown {shown} ms, closed {:?}, windows of the helper left {left}",
            screen.frame.w, screen.frame.h, screen.scale, ready.adapter, ready.ms, closed.map_err(|error| error.to_string())
        ));
        p.line(format!("  - plain desktop against the layer showing the same still, {} pixels outside the icons and menu bar: mean {:.3}, p99 {}, max {} of 255, pixels off by more than 2 {}", found.counted, found.mean, found.p99, found.max, found.over));
        if found.max > 2 {
            p.fail(format!("the layer did not show the still exactly as the system does at {label}"));
        }
        Ok(())
    }

    fn modes(display: u32) -> Vec<(usize, usize, usize, usize, bool, *const c_void)> {
        unsafe {
            let keys = [kCGDisplayShowDuplicateLowResolutionModes];
            let values = [kCFBooleanTrue];
            let options = CFDictionaryCreate(std::ptr::null(), keys.as_ptr(), values.as_ptr(), 1, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
            let list = CGDisplayCopyAllDisplayModes(display, options);
            let mut found = Vec::new();
            if !list.is_null() {
                for index in 0..CFArrayGetCount(list) {
                    let mode = CFArrayGetValueAtIndex(list, index);
                    found.push((CGDisplayModeGetWidth(mode), CGDisplayModeGetHeight(mode), CGDisplayModeGetPixelWidth(mode), CGDisplayModeGetPixelHeight(mode), CGDisplayModeIsUsableForDesktopGUI(mode), CGDisplayModeRetain(mode)));
                }
                CFRelease(list);
            }
            if !options.is_null() {
                CFRelease(options);
            }
            found
        }
    }

    fn switch_mode(display: u32, mode: *const c_void) -> bool {
        unsafe {
            let mut config: *mut c_void = std::ptr::null_mut();
            if CGBeginDisplayConfiguration(&mut config) != 0 {
                return false;
            }
            CGConfigureDisplayWithDisplayMode(config, display, mode, std::ptr::null());
            CGCompleteDisplayConfiguration(config, 1) == 0
        }
    }

    fn colour_scale(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let desktop = platform::current()?;
        let screen = desktop.screens()?.into_iter().next().context("no screen")?;
        let recorded = desktop.wallpaper(&screen).ok().flatten();
        still_through_helper(p, &screen, "native mode")?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let current = unsafe { CGDisplayCopyDisplayMode(display) };
        let listed = modes(display);
        p.line(format!("- display modes, points and pixels {:?}", listed.iter().map(|(w, h, pw, ph, usable, _)| format!("{w}x{h} as {pw}x{ph}{}", if *usable { "" } else { " unusable" })).collect::<Vec<_>>()));
        let doubled = listed.iter().filter(|(w, _, pw, _, usable, _)| *usable && *pw == 2 * *w && *w >= 640).max_by_key(|(w, _, _, _, _, _)| *w).map(|mode| mode.5);
        match doubled {
            Some(mode) if switch_mode(display, mode) => {
                std::thread::sleep(Duration::from_secs(3));
                let scaled = platform::current()?.screens()?.into_iter().next().context("no screen after the switch")?;
                p.line(format!("- switched to a doubled mode, the screen now reads {} by {} points at scale {}", scaled.frame.w, scaled.frame.h, scaled.scale));
                let result = still_through_helper(p, &scaled, "HiDPI mode");
                let back = !current.is_null() && switch_mode(display, current);
                std::thread::sleep(Duration::from_secs(3));
                p.line(format!("- switched back to the first mode {back}"));
                result?;
            }
            Some(_) => p.limit("the runner listed a doubled display mode but would not switch to it, so scale 2 is unproven here"),
            None => p.limit("the runner's virtual display offers no doubled mode, so scale 2 is unproven here"),
        }
        for (_, _, _, _, _, mode) in listed {
            unsafe { CGDisplayModeRelease(mode) };
        }
        if !current.is_null() {
            unsafe { CGDisplayModeRelease(current) };
        }
        if let Some(recorded) = recorded.filter(|path| path.is_file()) {
            let _ = desktop.set_wallpaper(&screen, &recorded);
        }
        Ok(())
    }

    struct Seen {
        at: f64,
        half: RgbImage,
        inked: f32,
        step: u8,
    }

    struct Dragged {
        dropped: f64,
        seen: Vec<Seen>,
        log: Vec<String>,
        old: Option<PathBuf>,
        old_still: Option<RgbImage>,
        old_tables: Option<Vec<u8>>,
    }

    fn half_shot(shot: &Shot) -> RgbImage {
        let (width, height) = (shot.width / 2, shot.height / 2);
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&shot.at(x * 2, y * 2));
            }
        }
        RgbImage::from_raw(width as u32, height as u32, rgb).unwrap_or_default()
    }

    fn halved(image: &RgbImage) -> RgbImage {
        let (width, height) = (image.width() / 2, image.height() / 2);
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&image.get_pixel(x * 2, y * 2).0);
            }
        }
        RgbImage::from_raw(width, height, rgb).unwrap_or_default()
    }

    fn stamp_of(line: &str) -> Option<f64> {
        line.split_whitespace().next()?.parse().ok()
    }

    fn helper_pids(p: &mut Probe) -> Vec<i32> {
        p.tool("/usr/bin/pgrep", &["-f", "live-helper"]).out.lines().filter_map(|line| line.trim().parse().ok()).collect()
    }

    fn strong_ink(baseline: &Shot, field: &Shot, rect: &Rect, screen: &Screen) -> Vec<(usize, usize)> {
        let scale = screen.scale;
        let mut ink = Vec::new();
        for y in (((rect.y - screen.frame.y) * scale).max(0.0) as usize..((rect.y - screen.frame.y + rect.h) * scale) as usize).step_by(2) {
            for x in (((rect.x - screen.frame.x) * scale).max(0.0) as usize..((rect.x - screen.frame.x + rect.w) * scale) as usize).step_by(2) {
                let (one, two) = (baseline.at(x, y), field.at(x, y));
                if (0..3).map(|channel| one[channel].abs_diff(two[channel]) as u32).sum::<u32>() > 150 {
                    ink.push((x, y));
                }
            }
        }
        ink
    }

    fn tables_of(p: &Probe, output: &Path) -> Option<Vec<u8>> {
        let name = output.file_name()?.to_string_lossy().into_owned();
        let found = fs::read_dir(p.home.join("cache").join("tables")).ok()?.flatten().find(|file| file.file_name().to_string_lossy().starts_with(&format!("{name}.")))?;
        fs::read(found.path()).ok()
    }

    fn watch_drag(p: &mut Probe, display: u32, screen: &Screen, from: (f32, f32), to: (f32, f32), masks: &[Vec<(usize, usize)>]) -> Option<Dragged> {
        let baseline = grab(display)?;
        let old = platform::current().ok().and_then(|desktop| desktop.wallpaper(screen).ok().flatten());
        let old_tables = old.as_deref().and_then(|old| tables_of(p, old));
        let old_still = old.as_deref().and_then(|path| objc2::rc::autoreleasepool(|_| picture::decode(path)).ok()).map(|image| halved(&image));
        let menu = (30.0 * screen.scale) as usize;
        let lines = daemon_log(p).lines().count();
        drag(from, to);
        let dropped = epoch();
        let started = Instant::now();
        let mut seen = Vec::new();
        let mut previous: Option<Shot> = None;
        let mut done: Option<f64> = None;
        let mut checks = 0;
        while started.elapsed() < Duration::from_secs(8) {
            let at = epoch();
            let Some(shot) = grab(display) else { continue };
            let inked = masks.iter().map(|ink| ink.iter().filter(|(x, y)| !near(shot.at(*x, *y), baseline.at(*x, *y))).count() as f32 / ink.len().max(1) as f32).fold(0.0, f32::max);
            let step = previous.as_ref().map_or(0, |previous| difference(previous, &shot, &|_, y| y >= menu).max);
            seen.push(Seen { at, half: half_shot(&shot), inked, step });
            previous = Some(shot);
            checks += 1;
            if done.is_none() && checks % 3 == 0 && new_lines(&daemon_log(p), lines).iter().any(|line| line.contains(" tick ")) {
                done = Some(epoch());
            }
            if done.is_some_and(|done| epoch() > done + 1.0) {
                break;
            }
        }
        let log = new_lines(&daemon_log(p), lines).into_iter().filter(|line| line.contains(" glide ") || line.contains(" tick ")).collect();
        Some(Dragged { dropped, seen, log, old, old_still, old_tables })
    }

    fn logged(dragged: &Dragged, word: &str) -> Option<f64> {
        dragged.log.iter().find(|line| line.contains(word)).and_then(|line| stamp_of(line))
    }

    fn mean_gap(a: &RgbImage, b: &RgbImage, region: &[usize]) -> f32 {
        if region.is_empty() || a.dimensions() != b.dimensions() {
            return f32::MAX;
        }
        let (x, y) = (a.as_raw(), b.as_raw());
        let total: u64 = region.iter().map(|index| (0..3).map(|channel| u64::from(x[index * 3 + channel].abs_diff(y[index * 3 + channel]))).sum::<u64>()).sum();
        total as f32 / (region.len() * 3) as f32
    }

    fn side(shot: &RgbImage, old: &RgbImage, new: &RgbImage, apart: &[usize]) -> char {
        if apart.is_empty() {
            return '=';
        }
        let (a, b) = (mean_gap(shot, old, apart), mean_gap(shot, new, apart));
        if a < 3.0 && a * 3.0 < b {
            'A'
        } else if b < 3.0 && b * 3.0 < a {
            'B'
        } else {
            '~'
        }
    }

    fn judge_glide(p: &mut Probe, dragged: &Dragged, screen: &Screen, label: &str, evidence: Option<&str>) -> bool {
        let desktop = platform::current().ok();
        let new = desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten());
        let decode = |path: &Option<PathBuf>| path.as_deref().and_then(|path| objc2::rc::autoreleasepool(|_| picture::decode(path)).ok()).map(|image| halved(&image));
        let (Some(old_still), Some(new_still)) = (dragged.old_still.clone(), decode(&new)) else {
            p.line(format!("- {label}: the two stills could not be read, old {:?} new {new:?}", dragged.old));
            return false;
        };
        let (width, height) = (old_still.width() as usize, old_still.height() as usize);
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let half = screen.scale / 2.0;
        let keep: Vec<usize> = (0..height)
            .step_by(2)
            .flat_map(|y| (0..width).step_by(2).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let (px, py) = (*x as f32 / half + screen.frame.x, *y as f32 / half + screen.frame.y);
                py - screen.frame.y >= 30.0 && !icons.iter().any(|icon| px >= icon.x - 12.0 && px < icon.x + icon.w + 12.0 && py >= icon.y - 12.0 && py < icon.y + icon.h + 12.0)
            })
            .map(|(x, y)| y * width + x)
            .collect();
        let (o, n) = (old_still.as_raw(), new_still.as_raw());
        let apart: Vec<usize> = keep.iter().copied().filter(|index| (0..3).any(|channel| o[index * 3 + channel].abs_diff(n[index * 3 + channel]) > 24)).collect();
        let new_tables = new.as_deref().and_then(|new| tables_of(p, new));
        let frames: Vec<(f32, RgbImage)> = match (&dragged.old_tables, &new_tables, p.subject.clone()) {
            (Some(old_tables), Some(new_tables), Some(subject)) => objc2::rc::autoreleasepool(|_| picture::decode(&subject)).ok().map_or(Vec::new(), |image| {
                let canvas = scooch_core::canvas(&image, (width * 2) as u32, (height * 2) as u32);
                match (scooch_core::Bundle::tables_from_bytes(old_tables, canvas.clone()), scooch_core::Bundle::tables_from_bytes(new_tables, canvas)) {
                    (Ok(a), Ok(b)) => (0..=20).map(|step| step as f32 / 20.0).map(|s| (s, halved(&scooch_core::frame(&a, &b, s)))).collect(),
                    _ => Vec::new(),
                }
            }),
            _ => Vec::new(),
        };
        let classes: Vec<char> = dragged.seen.iter().map(|seen| side(&seen.half, &old_still, &new_still, &apart)).collect();
        let fitted: Vec<Option<(f32, f32)>> = dragged.seen.iter().map(|seen| frames.iter().map(|(s, frame)| (*s, mean_gap(&seen.half, frame, &keep))).min_by(|a, b| a.1.total_cmp(&b.1))).collect();
        let glide_at = logged(dragged, " glide open");
        let settled = logged(dragged, " glide settled");
        let closed = logged(dragged, " glide closed");
        let set = logged(dragged, " glide settle ");
        let glided = logged(dragged, " glide glided");
        let appeared = classes.iter().position(|class| *class == 'B');
        let returned = appeared.map_or(0, |first| classes[first..].iter().filter(|class| **class == 'A').count());
        let moving: Vec<usize> = (0..classes.len()).filter(|index| classes[*index] == '~' && fitted[*index].is_some_and(|(s, _)| s > 0.04 && s < 0.96)).collect();
        let window: Vec<f32> = (0..classes.len()).filter(|index| glide_at.is_some_and(|at| dragged.seen[*index].at >= at) && settled.is_none_or(|settled| dragged.seen[*index].at <= settled)).filter_map(|index| fitted[index].map(|(s, _)| s)).collect();
        let steady = window.windows(2).all(|pair| pair[1] >= pair[0] - 0.1);
        let first_move = (0..classes.len()).find(|index| classes[*index] != 'A' && classes[*index] != '=').map(|index| (dragged.seen[index].at - dragged.dropped) * 1000.0);
        let span = glide_at.zip(glided).map(|(from, to)| to - from);
        let rate = span.map(|span| dragged.seen.iter().filter(|seen| glide_at.is_some_and(|at| seen.at >= at) && seen.at <= glide_at.unwrap_or(0.0) + span).count() as f64 / span.max(0.001));
        let expected = rate.map(|rate| rate * 0.45 * 0.64);
        let inked = dragged.seen.iter().map(|seen| seen.inked).fold(0.0, f32::max);
        let hand_over = settled.map(|settled| dragged.seen.iter().filter(|seen| seen.at > settled && closed.is_none_or(|closed| seen.at <= closed + 1.0)).map(|seen| seen.step).max().unwrap_or(0));
        let at = |time: Option<f64>| time.map(|time| ((time - dragged.dropped) * 1000.0).round());
        p.line(format!(
            "- {label}: {} captures in {:.0} ms, {} pixels differ between the two stills, classes {}, {} moving between the two places, old picture after the new {returned}, glide progress steady {steady} over {} captures",
            dragged.seen.len(),
            dragged.seen.last().map_or(0.0, |seen| (seen.at - dragged.dropped) * 1000.0),
            apart.len(),
            classes.iter().collect::<String>(),
            moving.len(),
            window.len()
        ));
        p.line(format!("  - captures a second during the glide {:?}, so a 450 ms glide should show about {:?} captures with progress between 0.05 and 0.95", rate.map(|rate| rate.round()), expected.map(|value| value.round())));
        p.line(format!("  - drop to first moving frame on screen {:?} ms, glide sent {:?} ms, wallpaper set {:?} ms, glided {:?} ms, settled {:?} ms, closed {:?} ms", first_move.map(f64::round), at(glide_at), at(set), at(glided), at(settled), at(closed)));
        p.line(format!("  - largest share of a still icon's solid ink that changed {inked:.3}, largest change between captures from settled to one second after close {hand_over:?} of 255, CPU frames to fit {}", frames.len()));
        p.line(format!("  - daemon log {:?}", dragged.log.iter().map(|line| line.chars().take(200).collect::<String>()).collect::<Vec<_>>()));
        if let Some(name) = evidence {
            let folder = p.out.join(name);
            let _ = fs::create_dir_all(&folder);
            let mut table = vec!["| capture | ms after drop | class | fitted progress | gap to it | gap to old | gap to new |".to_string(), "|---|---|---|---|---|---|---|".to_string()];
            for (index, seen) in dragged.seen.iter().enumerate() {
                let ms = ((seen.at - dragged.dropped) * 1000.0).round();
                let fit = fitted[index];
                let _ = scooch_core::encode_jpeg(&seen.half, 85).map(|bytes| fs::write(folder.join(format!("{index:03}-{ms}ms-{}.jpg", classes[index])), bytes));
                table.push(format!("| {index} | {ms} | {} | {} | {} | {:.2} | {:.2} |", classes[index], fit.map_or("none".into(), |(s, _)| format!("{s:.2}")), fit.map_or("none".into(), |(_, gap)| format!("{gap:.2}")), mean_gap(&seen.half, &old_still, &apart), mean_gap(&seen.half, &new_still, &apart)));
            }
            let _ = fs::write(folder.join("captures.md"), table.join("\n") + "\n");
            let picked: Vec<&Seen> = dragged.seen.iter().filter(|seen| settled.is_none_or(|settled| seen.at <= settled + 0.1)).collect();
            if picked.len() >= 9 {
                let chosen: Vec<RgbImage> = (0..9).map(|index| halved(&picked[index * (picked.len() - 1) / 8].half)).collect();
                let (w, h) = (chosen[0].width(), chosen[0].height());
                let mut rgb = vec![0u8; (w * 9 * h * 3) as usize];
                for (index, image) in chosen.iter().enumerate() {
                    for y in 0..h.min(image.height()) {
                        for x in 0..w.min(image.width()) {
                            let at = ((y * w * 9 + index as u32 * w + x) * 3) as usize;
                            rgb[at..at + 3].copy_from_slice(&image.get_pixel(x, y).0);
                        }
                    }
                }
                if let Some(strip) = RgbImage::from_raw(w * 9, h, rgb) {
                    let _ = scooch_core::encode_png(&strip).map(|bytes| fs::write(p.out.join(format!("{name}-strip.png")), bytes));
                }
            }
            p.line(format!("  - every capture saved in {name} with captures.md, nine frame strip {name}-strip.png"));
        }
        returned == 0 && steady && moving.len() >= 2 && inked <= 0.1 && hand_over.is_some_and(|value| value <= 2) && settled.is_some()
    }

    fn stuck(p: &mut Probe, pid: i32) {
        let file = p.out.join(format!("stuck-{pid}-{}.txt", now()));
        let _ = p.tool("/usr/bin/sample", &[&pid.to_string(), "2", "-file", &file.to_string_lossy()]);
        let text = fs::read_to_string(&file).unwrap_or_default();
        let ours: Vec<String> = text.lines().filter(|line| line.contains("scooch::") || line.contains("Thread_")).map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect()).take(40).collect();
        p.line(format!("  - sample of the daemon {pid} kept as {}, its threads and our frames {ours:?}", file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
        let log = daemon_log(p);
        let tail: Vec<&str> = log.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect();
        p.line(format!("  - the last lines of the daemon log {tail:?}"));
    }

    fn quiet_log(p: &Probe, quiet: Duration, limit: Duration) -> bool {
        let started = Instant::now();
        let mut count = daemon_log(p).lines().count();
        let mut since = Instant::now();
        while started.elapsed() < limit {
            std::thread::sleep(Duration::from_millis(100));
            let now = daemon_log(p).lines().count();
            if now != count {
                count = now;
                since = Instant::now();
            } else if since.elapsed() >= quiet {
                return true;
            }
        }
        false
    }

    fn wallpaper_after(p: &mut Probe, screen: &Screen, act: impl FnOnce(&mut Probe)) -> (Option<u128>, Vec<String>) {
        let desktop = platform::current().ok();
        let before = desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten());
        let lines = daemon_log(p).lines().count();
        act(p);
        let acted = Instant::now();
        let changed = wait_until(Duration::from_secs(12), || desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten()) != before).map(|_| acted.elapsed().as_millis());
        let home = p.home.clone();
        let _ = wait_until(Duration::from_secs(5), || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().any(|line| line.contains(" tick ")));
        std::thread::sleep(Duration::from_millis(600));
        let log = new_lines(&daemon_log(p), lines).into_iter().filter(|line| line.contains(" glide ") || line.contains(" tick ")).map(|line| line.chars().take(180).collect()).collect();
        (changed, log)
    }

    fn cover(marker: MainThreadMarker, screen: &Screen) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
        use objc2::MainThreadOnly as _;
        let (display, _) = picture::screen_for(marker, &screen.id)?;
        let frame = display.frame();
        let window = unsafe {
            objc2_app_kit::NSWindow::initWithContentRect_styleMask_backing_defer_screen(objc2_app_kit::NSWindow::alloc(marker), objc2_foundation::NSRect::new(objc2_foundation::NSPoint::new(0.0, 0.0), frame.size), objc2_app_kit::NSWindowStyleMask::Borderless, objc2_app_kit::NSBackingStoreType::Buffered, false, Some(&display))
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setFrame_display(frame, true);
        window.setOpaque(true);
        window.setBackgroundColor(Some(&objc2_app_kit::NSColor::blackColor()));
        window.orderFrontRegardless();
        spin(0.6);
        Ok(window)
    }

    fn motion_now(p: &mut Probe) -> Option<String> {
        let read = p.tool("/usr/bin/defaults", &["read", "com.apple.universalaccess", "reduceMotion"]);
        read.ok().then(|| read.out.trim().to_string())
    }

    fn set_motion(p: &mut Probe, reduce: bool) -> bool {
        if p.motion.is_none() {
            p.motion = Some(motion_now(p));
        }
        p.tool("/usr/bin/defaults", &["write", "com.apple.universalaccess", "reduceMotion", "-bool", if reduce { "true" } else { "false" }]).ok()
    }

    fn put_motion_back(p: &mut Probe) {
        let Some(before) = p.motion.take() else { return };
        let put = match &before {
            None => p.tool("/usr/bin/defaults", &["delete", "com.apple.universalaccess", "reduceMotion"]),
            Some(value) => p.tool("/usr/bin/defaults", &["write", "com.apple.universalaccess", "reduceMotion", "-bool", if value == "1" { "true" } else { "false" }]),
        };
        let after = motion_now(p);
        p.line(format!("- reduce motion put back as found, before {before:?} now {after:?} {}", if put.ok() || before.is_none() { "ok" } else { "failed" }));
        if after != before {
            p.fail("reduce motion was not put back as the probe found it");
        }
    }

    fn glide_check(p: &mut Probe) -> Result<()> {
        let result = glide_steps(p);
        put_motion_back(p);
        result
    }

    fn glide_steps(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.finder = false;
            p.line("- the glide check needs Apple Events to Finder, which the open consent window blocks, so it was skipped".to_string());
            return Ok(());
        }
        let Some(daemon) = scooch_pid(p) else {
            p.fail("no daemon runs for the glide check");
            return Ok(());
        };
        p.line(format!("- glide setting `{}`, scenery `{}`, daemon {daemon}", state(p)["glide"], state(p)["options"]["scenery"].as_str().unwrap_or("missing")));
        let calm = quiet_log(p, Duration::from_millis(1500), Duration::from_secs(15));
        let found = motion_now(p);
        let wrote = set_motion(p, false);
        let now = motion_now(p);
        let lines = daemon_log(p).lines().count();
        poke(p);
        let home = p.home.clone();
        let read = wait_until(Duration::from_secs(6), || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().any(|line| line.contains("glide reduce motion off")));
        let last_reading = daemon_log(p).lines().rev().find(|line| line.contains(" glide reduce motion ")).map(str::to_string);
        p.line(format!("- daemon log quiet before the setup {calm}, reduce motion found as {found:?}, turned off {wrote}, now {now:?}, the daemon logged reading it as off after {:?} ms, its last reading `{}`", read.map(|time| time.as_millis()), last_reading.clone().unwrap_or_default()));
        if !last_reading.is_some_and(|line| line.ends_with(" off")) {
            p.fail("the daemon never logged reduce motion as off after the probe turned it off");
            return Ok(());
        }
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let aside = (screen.frame.x + screen.frame.w * 0.38, screen.frame.y + screen.frame.h * 0.5);
        let mut setup_log = Vec::new();
        for (step, target) in [(1, aside), (2, subject)] {
            quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
            let (setup, log) = wallpaper_after(p, &screen, |p| {
                let _ = place(p, name, target);
                poke(p);
            });
            p.line(format!("- setup move {step} of {name} to {:.0},{:.0}, new wallpaper {setup:?} ms after the Apple Event returned, daemon log {log:?}", target.0, target.1));
            setup_log = log;
        }
        let tables = setup_log.iter().any(|line| line.contains(" tick ") && line.contains("Rendered") && line.contains("tables "));
        p.line(format!("- the last setup move rendered and wrote tables {tables}"));
        if !tables {
            p.fail("the setup moves wrote no tables, so the timed drag could not glide");
        }
        quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
        let paper = picture_window(&screen).and_then(|window| grab_window(window.number));
        let baseline = grab(display).context("the desktop could not be captured")?;
        let still: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().filter(|(found, _)| found.starts_with("scooch") && found != name).map(|(_, rect)| rect).collect();
        let masks: Vec<Vec<(usize, usize)>> = paper.as_ref().map(|field| still.iter().map(|rect| strong_ink(&baseline, field, rect, &screen)).collect()).unwrap_or_default();
        p.line(format!("- solid icon ink pixels of the still icons {:?}", masks.iter().map(Vec::len).collect::<Vec<_>>()));
        let start = position(p, name).unwrap_or(subject);
        let dragged = watch_drag(p, display, &screen, start, aside, &masks).context("the glide drag could not be watched")?;
        let good = judge_glide(p, &dragged, &screen, "glide on, CGEvent drag", Some("glide"));
        std::thread::sleep(Duration::from_millis(1000));
        let pids = helper_pids(p);
        let left = layer::every_window().into_iter().filter(|window| pids.contains(&(window.pid as i32))).count();
        p.line(format!("- helper processes {pids:?} with windows left {left}"));
        if !good || left > 0 {
            p.fail("the glide did not move steadily under the icons and hand over cleanly");
        }
        quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
        let menu = (30.0 * screen.scale) as usize;
        let before = grab(display).context("the desktop could not be captured")?;
        let lines = daemon_log(p).lines().count();
        poke(p);
        let poked = Instant::now();
        let mut worst = 0u8;
        let mut shots = 0;
        while poked.elapsed() < Duration::from_millis(2500) {
            if let Some(shot) = grab(display) {
                worst = worst.max(difference(&before, &shot, &|_, y| y >= menu).max);
                shots += 1;
            }
        }
        let woke: Vec<String> = new_lines(&daemon_log(p), lines).into_iter().filter(|line| line.contains(" glide ") || line.contains(" tick ")).map(|line| line.chars().take(180).collect()).collect();
        let opened = woke.iter().any(|line| line.contains("glide early open"));
        let closed = woke.iter().any(|line| line.contains("closed early open"));
        p.line(format!("- a wake that changes nothing: early open {opened}, closed again {closed}, {shots} captures over 2.5 s, largest change from before the wake {worst} of 255, log {woke:?}"));
        if !opened || !closed || worst > 2 {
            p.fail("an early open with nothing to glide was not invisible");
        }
        let calm_a = (screen.frame.x + screen.frame.w * 0.15, screen.frame.y + screen.frame.h * 0.75);
        let calm_b = (screen.frame.x + screen.frame.w * 0.25, screen.frame.y + screen.frame.h * 0.75);
        let mut laps = Vec::new();
        for (index, target) in [calm_a, calm_b, calm_a, calm_b].into_iter().enumerate() {
            quiet_log(p, Duration::from_millis(800), Duration::from_secs(10));
            let from = position(p, name).unwrap_or(subject);
            let (time, log) = wallpaper_after(p, &screen, |_| drag(from, target));
            let render = log.iter().find(|line| line.contains(" tick ") && line.contains("Rendered")).map(|line| line.chars().take(260).collect::<String>());
            laps.push(format!("move {} new wallpaper {time:?} ms, tick {render:?}", index + 1));
        }
        p.line(format!("- scenery moves on calm ground, the first leaves the subject and the rest hold the placement {laps:?}"));
        let (mut looked, mut moved, mut right, mut half) = (0, 0, 0, 0);
        let mut seen = Vec::new();
        for index in 0..12 {
            quiet_log(p, Duration::from_millis(800), Duration::from_secs(10));
            let lines = daemon_log(p).lines().count();
            let from = position(p, name).unwrap_or(subject);
            let target = if index % 2 == 0 { aside } else { subject };
            drag(from, target);
            let home = p.home.clone();
            let ticked = |count: usize| {
                let home = home.clone();
                move || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().filter(|line| line.contains(" tick ")).count() >= count
            };
            let _ = wait_until(Duration::from_secs(6), ticked(1));
            std::thread::sleep(Duration::from_millis(1500));
            poke(p);
            let _ = wait_until(Duration::from_secs(4), ticked(2));
            let log = new_lines(&daemon_log(p), lines);
            let prints: Vec<(String, bool)> = log.iter().enumerate().filter_map(|(at, line)| line.split("snapshot icons ").nth(1).map(|print| (print.trim().to_string(), log[at..].iter().find(|later| later.contains(" tick ")).is_some_and(|tick| tick.contains("Rendered"))))).collect();
            let last = prints.last().map(|(print, _)| print.clone()).unwrap_or_default();
            let quick = log.iter().find_map(|line| line.split("quick look after ").nth(1).map(str::to_string));
            if let Some(quick) = &quick {
                looked += 1;
                let went = quick.ends_with("moved true");
                let print = quick.split("icons ").nth(1).and_then(|rest| rest.split_whitespace().next()).unwrap_or_default().to_string();
                if went {
                    moved += 1;
                    if print == last {
                        right += 1;
                    }
                }
            }
            half += prints.iter().filter(|(print, rendered)| *rendered && *print != last).count();
            seen.push(format!("{}: {}", index + 1, quick.unwrap_or_else(|| "no quick look".into())));
        }
        p.line(format!("- twelve drags: quick looks {looked}, saw the icons moved {moved}, of those had the final layout {right}, renders from a layout that was not the final one {half}, per drag {seen:?}"));
        if half > 0 {
            p.fail("a quick look rendered a half finished move");
        }
        let ran = p.scooch(&["glide"], LIMIT);
        p.line(format!("- scooch glide said {:?}", ran.text()));
        let back = position(p, name).unwrap_or(aside);
        let (off, off_log) = wallpaper_after(p, &screen, |_| drag(back, subject));
        p.line(format!("- glide off, the same drag back: new wallpaper after {off:?} ms, log {off_log:?}"));
        let ran = p.scooch(&["glide"], LIMIT);
        p.line(format!("- scooch glide said {:?}", ran.text()));
        let wrote = set_motion(p, true);
        let back = position(p, name).unwrap_or(subject);
        let (time, log) = wallpaper_after(p, &screen, |_| drag(back, aside));
        let glided = log.iter().any(|line| line.contains(" glide open"));
        let read = log.iter().any(|line| line.contains("glide skipped, reduce motion"));
        p.line(format!("- reduce motion on {wrote}, drag: new wallpaper after {time:?} ms, glided {glided}, the daemon logged reduce motion {read}, log {log:?}"));
        if glided || !read {
            p.fail("the daemon did not take today's path with reduce motion on");
        }
        set_motion(p, false);
        std::thread::sleep(Duration::from_millis(500));
        let from = position(p, name).unwrap_or(subject);
        let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let (_, warm_log) = wallpaper_after(p, &screen, |_| drag(from, to));
        p.line(format!("- a drag with reduce motion off again, so the next move has tables, log {warm_log:?}"));
        quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
        let window = cover(marker, &screen)?;
        let target = if position(p, name).is_some_and(|at| (at.0 - subject.0).abs() < 5.0) { aside } else { subject };
        let (covered, covered_log) = wallpaper_after(p, &screen, |p| {
            let _ = place(p, name, target);
            poke(p);
        });
        window.orderOut(None);
        window.close();
        drop(window);
        spin(0.4);
        let tick = |log: &[String]| log.iter().find_map(|line| line.split(" tick ").nth(1).and_then(|rest| rest.split_whitespace().next()).and_then(|ms| ms.parse::<u64>().ok()));
        let hidden = covered_log.iter().any(|line| line.contains("glide skipped, hidden") || line.contains("glide skipped, helper budget"));
        p.line(format!("- a move while a window covers the whole desktop: new wallpaper after {covered:?} ms, the daemon answered hidden {hidden}, tick {:?} ms against {:?} ms with glide off, log {covered_log:?}", tick(&covered_log), tick(&off_log)));
        if covered.is_none() || !hidden || tick(&covered_log).zip(tick(&off_log)).is_some_and(|(with, without)| with > without + 400 + 300) {
            p.fail("a covered desktop delayed the plain swap");
        }
        std::thread::sleep(Duration::from_millis(1500));
        let from = position(p, name).unwrap_or(subject);
        let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let lines = daemon_log(p).lines().count();
        let shown_before = desktop.wallpaper(&screen).ok().flatten();
        drag(from, to);
        let home = p.home.clone();
        let opened = wait_until(Duration::from_secs(5), || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().any(|line| line.contains(" glide open")));
        let killed = helper_pids(p);
        for pid in &killed {
            let _ = p.tool("/bin/kill", &["-9", &pid.to_string()]);
        }
        let _ = wait_until(Duration::from_secs(6), || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().any(|line| line.contains(" tick ")));
        std::thread::sleep(Duration::from_millis(1200));
        let now_shown = desktop.wallpaper(&screen).ok().flatten();
        let recorded = state(p)["screens"].as_object().and_then(|screens| screens.values().next().and_then(|entry| entry["output"].as_str().map(PathBuf::from)));
        let shot = grab(display);
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let matches = shot.as_ref().zip(now_shown.as_ref()).and_then(|(shot, file)| matches_file(shot, file, &icons, &screen));
        let alive = scooch_pid(p).is_some();
        let kill_log: Vec<String> = new_lines(&daemon_log(p), lines).into_iter().filter(|line| line.contains(" glide ")).map(|line| line.chars().take(180).collect()).collect();
        p.line(format!("- helper killed mid glide after the layer opened {:?} ms: killed {killed:?}, wallpaper changed {}, it is the recorded output {}, screen matches the file {matches:?}, daemon alive {alive}, log {kill_log:?}", opened.map(|time| time.as_millis()), now_shown != shown_before, now_shown == recorded));
        std::thread::sleep(Duration::from_millis(1500));
        let from = position(p, name).unwrap_or(subject);
        let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let again = watch_drag(p, display, &screen, from, to, &masks).context("the drag after the kill could not be watched")?;
        let glided_again = logged(&again, " glide settled").is_some();
        p.line(format!("- the next drag glides again {glided_again}, log {:?}", again.log.iter().map(|line| line.chars().take(160).collect::<String>()).collect::<Vec<_>>()));
        if killed.is_empty() || now_shown != recorded || !matches.is_some_and(|share| share >= 0.9) || !alive || !glided_again {
            p.fail("killing the helper mid glide was not handled cleanly");
        }
        let last = Instant::now();
        let gone = wait_until(Duration::from_secs(40), || helper_pids(p).is_empty());
        p.line(format!("- no helper process {:?} s after the last drag", gone.map(|_| last.elapsed().as_secs())));
        std::thread::sleep(Duration::from_secs(35).saturating_sub(last.elapsed()));
        let pids = helper_pids(p);
        p.line(format!("- 35 s after the last drag helper processes {pids:?}"));
        if !pids.is_empty() {
            p.fail("a helper was still running 35 s after the last drag");
        }
        quiet_minute(p, daemon, "the daemon with glide on, 35 s after its last glide")?;
        Ok(())
    }

    const BALLOON: &[u8] = include_bytes!("../../../assets/photos/balloon-sky.jpg");

    fn smoother(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    fn bench_scene(width: u32, height: u32, moved: bool) -> scooch_core::Scene {
        let (w, h) = (152.0f32, 168.0f32);
        let mut icons: Vec<Rect> = (0..6).map(|index| Rect { x: width as f32 - 230.0, y: 60.0 + 200.0 * index as f32, w, h }).collect();
        icons.push(if moved { Rect { x: width as f32 * 0.25, y: height as f32 * 0.70, w, h } } else { Rect { x: width as f32 * 0.48, y: height as f32 * 0.42, w, h } });
        scooch_core::Scene { width, height, icons }
    }

    fn timed(values: &[f64]) -> String {
        format!("{}, over 16.7 ms {}", stats(values), values.iter().filter(|value| **value > 16.7).count())
    }

    fn gpu_bench(out: PathBuf) -> ExitCode {
        let _ = fs::create_dir_all(&out);
        let mut lines = vec!["# Scooch GPU glide bench".to_string(), String::new()];
        let result = (|| -> Result<()> {
            let marker = MainThreadMarker::new().context("not on the main thread")?;
            accessory(marker);
            let load = Command::new("/usr/bin/uptime").output().map(|ran| String::from_utf8_lossy(&ran.stdout).trim().to_string()).unwrap_or_default();
            lines.push(format!("- uptime before `{load}`"));
            let pid = std::process::id() as i32;
            let file = out.join("balloon-sky.jpg");
            fs::write(&file, BALLOON)?;
            let image = objc2::rc::autoreleasepool(|_| picture::decode(&file))?;
            let _ = fs::remove_file(&file);
            let (width, height) = (2880u32, 1800u32);
            let options = scooch_core::Options { scenery: scooch_core::Scenery::Auto, ..Default::default() };
            let made = Instant::now();
            let analysis = scooch_core::analyze(&image);
            let (one, two) = (bench_scene(width, height, false), bench_scene(width, height, true));
            let first = scooch_core::solve(&analysis, &one, &options, None);
            let second = scooch_core::solve(&analysis, &two, &options, Some(&first));
            let a = scooch_core::bundle(&image, &analysis, &first, &one, &options);
            let b = scooch_core::bundle(&image, &analysis, &second, &two, &options);
            let canvas = scooch_core::canvas(&image, width, height);
            lines.push(format!("- balloon picture {}x{}, two {width}x{height} bundles with scenery and the canvas made on the CPU in {} ms, compatible {}", image.width(), image.height(), made.elapsed().as_millis(), scooch_core::compatible(&a, &b)));
            lines.push(format!("- before any device {}", memory(pid)));
            let started = Instant::now();
            let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
            gpu.ready().map_err(anyhow::Error::msg)?;
            lines.push(format!("- device {} with shaders ready after {} ms, {}", gpu.name, started.elapsed().as_millis(), memory(pid)));
            let started = Instant::now();
            let picture = gpu.picture(&canvas);
            let glide = gpu.glide(&picture, &a, &b).map_err(anyhow::Error::msg)?;
            lines.push(format!("- canvas {}x{} with {} mip levels and both tables uploaded in {} ms, {}", picture.width, picture.height, picture.levels, started.elapsed().as_millis(), memory(pid)));
            let frame = |index: usize| smoother((index % 27) as f32 / 26.0);
            let metal = objc2_quartz_core::CAMetalLayer::new();
            metal.setOpaque(true);
            let mut screen = gpu.screen(Target::Layer(objc2::rc::Retained::as_ptr(&metal) as *mut c_void), width, height).map_err(anyhow::Error::msg)?;
            for index in 0..10 {
                layer::present(|| screen.show(&[(Layered::Glide(&glide, frame(index)), 1.0)])).map_err(anyhow::Error::msg)?;
            }
            let mut paced = Vec::new();
            let mut last = Instant::now();
            for index in 0..300 {
                layer::present(|| screen.show(&[(Layered::Glide(&glide, frame(index)), 1.0)])).map_err(anyhow::Error::msg)?;
                let now = Instant::now();
                paced.push((now - last).as_secs_f64() * 1000.0);
                last = now;
            }
            let drained = Instant::now();
            drop(screen);
            lines.push(format!("- 300 glide frames presented to an offscreen Metal layer with three drawables, time between frames {}, the last frames finished {} ms after the loop", timed(&paced), drained.elapsed().as_millis()));
            let mut timed_frames = Vec::new();
            for index in 0..300 {
                let took = gpu.time(width, height, &[(Layered::Glide(&glide, frame(index)), 1.0)]).map_err(anyhow::Error::msg)?;
                timed_frames.push(took.as_secs_f64() * 1000.0);
            }
            lines.push(format!("- 300 glide frames each drawn alone to an offscreen target, submit until the GPU finished {}", timed(&timed_frames)));
            lines.push(format!("- with the device and pictures {}", memory(pid)));
            drop(glide);
            drop(picture);
            drop(gpu);
            spin(0.5);
            lines.push(format!("- after the device was dropped {}", memory(pid)));
            let load = Command::new("/usr/bin/uptime").output().map(|ran| String::from_utf8_lossy(&ran.stdout).trim().to_string()).unwrap_or_default();
            lines.push(format!("- uptime after `{load}`"));
            Ok(())
        })();
        if let Err(error) = result {
            lines.push(format!("- **stopped** {error:#}"));
        }
        for line in &lines {
            println!("{line}");
        }
        let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
        ExitCode::SUCCESS
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
        if let Err(error) = space_layer(p) {
            p.fail(format!("the layer could not be checked on Spaces {error:#}"));
        }
        let subject = p.subject.clone().context("no test picture")?;
        let _ = place(p, PROBES[0].0, (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5));
        let second_id = spaces::join(&display, second.as_deref());
        let ran = p.scooch(&["use", &subject.to_string_lossy()], Duration::from_secs(60));
        p.line(format!("- scooch use on the first Space exit {:?}", ran.code));
        if wait_render(p, Duration::from_secs(30), Some(&subject), true).is_none() {
            stand_in(p, "Spaces")?;
        }
        let one = wait_render(p, Duration::from_secs(30), Some(&subject), false);
        p.line(format!("- first Space rendered {:?}, entries {:?}", one.map(|time| time.as_millis()), entries(p)));
        switch(p, 124);
        let home = p.home.clone();
        let two = wait_until(Duration::from_secs(25), || {
            let state: serde_json::Value = fs::read(home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
            state["screens"].as_object().is_some_and(|screens| screens.len() >= 2 && screens.get(&second_id).is_some_and(|entry| !entry["output"].is_null()))
        });
        p.line(format!("- second Space entry with its own output after {:?} ms: {:?}", two.map(|time| time.as_millis()), entries(p)));
        switch(p, 123);
        let ran = p.scooch(&["off"], Duration::from_secs(60));
        reap(p);
        p.line(format!("- scooch off from the first Space exit {:?}: {}", ran.code, ran.text()));
        p.line(format!("- after off, restoring {}, entries {:?}, login item loaded {}", state(p)["restoring"], entries(p), launchd::loaded()));
        let plist = fs::read_to_string(launchd::plist_path()?).unwrap_or_default();
        let restorer = daemon_pid(p);
        let parent = restorer.map(|pid| p.tool("/bin/ps", &["-o", "ppid=", "-p", &pid.to_string()]).out.trim().to_string());
        p.line(format!("- the restoring daemon is the real login item {}, pid {restorer:?} with parent {parent:?}", !plist.is_empty() && plist.contains("<string>watch</string>")));
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
        put_motion_back(p);
        if p.stand.is_some() || launchd::loaded() || launchd::plist_path().is_ok_and(|path| path.exists()) {
            let ran = p.scooch(&["off"], Duration::from_secs(60));
            p.line(format!("- scooch off during cleanup exit {:?}: {}", ran.code, ran.text()));
        }
        reap(p);
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
