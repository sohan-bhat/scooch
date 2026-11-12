#[cfg(not(windows))]
fn main() {
    println!("winprobe runs on Windows only, so start it on a Windows machine.");
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    probe::main()
}

#[cfg(windows)]
mod probe {
    use anyhow::{anyhow, bail, Context, Result};
    use scooch::platform::windows::layout::{self, Cell, Label, Spacing, SPACING};
    use scooch::platform::windows::{com, picture, shell, startup, wakes};
    use scooch::platform::{self, Screen};
    use scooch_core::{Rect, RgbImage};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::ffi::c_void;
    use std::fs;
    use std::io::Write as _;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use ::windows::core::{w, PCWSTR};
    use ::windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, FILETIME, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use ::windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, MapWindowPoints, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HDC, HMONITOR, LOGFONTW, MONITORINFO, SRCCOPY,
    };
    use ::windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use ::windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next, PROCESSENTRY32W, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32};
    use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use ::windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use ::windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ};
    use ::windows::Win32::System::Threading::GetProcessTimes;
    use ::windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK, TreeScope_Children};
    use ::windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForSystem, GetDpiForWindow, SystemParametersInfoForDpi, MDT_EFFECTIVE_DPI};
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, MOUSE_EVENT_FLAGS};
    use ::windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use ::windows::Win32::UI::Shell::{
        SHChangeNotification_Lock, SHChangeNotification_Unlock, SHChangeNotifyDeregister, SHChangeNotifyEntry, SHChangeNotifyRegister, SHGetSpecialFolderLocation, CSIDL_DESKTOP, CSIDL_DESKTOPDIRECTORY, FOLDERVIEWMODE,
        FVM_ICON, FWF_AUTOARRANGE, FWF_SNAPTOGRID, SHCNE_ALLEVENTS, SHCNE_CREATE, SHCNE_DELETE, SHCNE_MKDIR, SHCNE_RENAMEFOLDER, SHCNE_RENAMEITEM, SHCNE_RMDIR, SHCNE_UPDATEDIR, SHCNRF_InterruptLevel,
        SHCNRF_NewDelivery, SHCNRF_ShellLevel, SVSI_POSITIONITEM,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClassNameW, GetClientRect, GetSystemMetrics, GetWindowThreadProcessId, MsgWaitForMultipleObjects, PeekMessageW, RegisterClassExW,
        SetCursorPos, SystemParametersInfoW, TranslateMessage, EVENT_MAX, EVENT_MIN, MSG, PM_REMOVE, QS_ALLINPUT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        SPI_GETICONTITLELOGFONT, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_SETTINGCHANGE, WNDCLASSEXW,
    };

    const ROOT_NOTE: u32 = WM_APP + 11;
    const FOLDER_NOTE: u32 = WM_APP + 12;
    const SETTLE: Duration = Duration::from_millis(1500);
    const FILE_WAIT: Duration = Duration::from_secs(5);
    const PROBES: [(&str, bool); 5] = [
        ("scooch-probe-a.txt", false),
        ("scooch-probe-twentyeight.txt", false),
        ("scooch probe with spaces that wraps.txt", false),
        ("scooch-probe-a-name-long-enough-that-the-desktop-has-to-cut-it-after-two-lines.txt", false),
        ("scooch-probe-folder", true),
    ];
    const ADAPTER_EVENTS: u32 = SHCNE_CREATE.0 | SHCNE_DELETE.0 | SHCNE_MKDIR.0 | SHCNE_RMDIR.0 | SHCNE_RENAMEITEM.0 | SHCNE_RENAMEFOLDER.0 | SHCNE_UPDATEDIR.0;

    static START: OnceLock<Instant> = OnceLock::new();

    fn now() -> u128 {
        START.get_or_init(Instant::now).elapsed().as_millis()
    }

    fn epoch() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
    }

    #[derive(Clone, Debug)]
    struct Seen {
        at: u128,
        kind: u32,
        window: isize,
        class: String,
        object: i32,
        child: i32,
    }

    type Note = (u128, &'static str, i32);
    type Setting = (u128, usize);
    type Marks = (usize, usize, usize);

    thread_local! {
        static SEEN: RefCell<Vec<Seen>> = const { RefCell::new(Vec::new()) };
        static NOTES: RefCell<Vec<Note>> = const { RefCell::new(Vec::new()) };
        static SETTINGS: RefCell<Vec<Setting>> = const { RefCell::new(Vec::new()) };
    }

    struct Measure {
        name: String,
        at: (f32, f32),
        label: Label,
        adapter: Rect,
        automation: Option<Rect>,
        pixels: Option<Rect>,
    }

    struct Probe {
        out: PathBuf,
        report: Vec<String>,
        log: fs::File,
        home: PathBuf,
        desktop: PathBuf,
        window: HWND,
        notifications: Vec<u32>,
        made: Vec<PathBuf>,
        flags: Option<u32>,
        mode: Option<(FOLDERVIEWMODE, i32)>,
        wallpapers: Vec<(Screen, Option<PathBuf>)>,
        pictures: Vec<PathBuf>,
        daemon: Option<Child>,
        target: Option<POINT>,
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
    }

    pub fn main() -> ExitCode {
        START.get_or_init(Instant::now);
        let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "winprobe".into()));
        let home = std::env::temp_dir().join(format!("scooch-probe-{}", std::process::id()));
        if fs::create_dir_all(&out).is_err() || fs::create_dir_all(&home).is_err() {
            eprintln!("winprobe could not make its folders, so check the disk.");
            return ExitCode::from(2);
        }
        std::env::set_var("SCOOCH_HOME", &home);
        let Ok(log) = fs::File::create(out.join("raw.log")) else {
            eprintln!("winprobe could not write its log, so check the disk.");
            return ExitCode::from(2);
        };
        let desktop = dirs::desktop_dir().unwrap_or_else(|| home.join("Desktop"));
        let mut probe = Probe {
            out,
            report: vec!["# Scooch Windows probe".into()],
            log,
            home,
            desktop,
            window: HWND::default(),
            notifications: Vec::new(),
            made: Vec::new(),
            flags: None,
            mode: None,
            wallpapers: Vec::new(),
            pictures: Vec::new(),
            daemon: None,
            target: None,
        };
        probe.section("Setup", setup);
        probe.section("1 Session facts", facts);
        probe.section("2 Read", read);
        probe.section("3 Geometry truth", geometry);
        probe.section("4 Move and events", events);
        probe.section("5 Wallpaper", wallpaper);
        probe.section("6 End to end", end_to_end);
        probe.section("7 Cleanup", cleanup);
        probe.save();
        ExitCode::SUCCESS
    }

    fn setup(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        com::enter(false)?;
        p.window = window()?;
        let root = notify(p.window, CSIDL_DESKTOP, ROOT_NOTE);
        let folder = notify(p.window, CSIDL_DESKTOPDIRECTORY, FOLDER_NOTE);
        p.notifications = vec![root, folder];
        p.line(format!("- SCOOCH_HOME {}", p.home.display()));
        p.line(format!("- Desktop folder {}", p.desktop.display()));
        p.line(format!("- adapter {}", desktop.name()));
        p.line(format!("- shell notification ids, desktop root {root}, desktop folder {folder}"));
        Ok(())
    }

    fn window() -> Result<HWND> {
        let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
        let class = WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: instance.into(), lpszClassName: w!("ScoochProbe"), ..Default::default() };
        unsafe { RegisterClassExW(&class) };
        Ok(unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("ScoochProbe"), w!("Scooch probe"), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, Some(instance.into()), None) }?)
    }

    fn notify(window: HWND, csidl: u32, message: u32) -> u32 {
        let Ok(pidl) = (unsafe { SHGetSpecialFolderLocation(None, csidl as i32) }) else { return 0 };
        let pidl = com::Pidl(pidl);
        let entry = SHChangeNotifyEntry { pidl: pidl.0, fRecursive: false.into() };
        let sources = SHCNRF_ShellLevel | SHCNRF_InterruptLevel | SHCNRF_NewDelivery;
        unsafe { SHChangeNotifyRegister(window, sources, SHCNE_ALLEVENTS.0 as i32, message, 1, &entry) }
    }

    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match message {
            ROOT_NOTE | FOLDER_NOTE => {
                let mut pidls: *mut *mut ITEMIDLIST = std::ptr::null_mut();
                let mut kind = 0i32;
                let lock = unsafe { SHChangeNotification_Lock(HANDLE(wparam.0 as *mut c_void), lparam.0 as u32, Some(&mut pidls), Some(&mut kind)) };
                if !lock.is_invalid() {
                    let _ = unsafe { SHChangeNotification_Unlock(lock) };
                }
                let source = if message == ROOT_NOTE { "root" } else { "folder" };
                NOTES.with(|notes| notes.borrow_mut().push((now(), source, kind)));
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                SETTINGS.with(|settings| settings.borrow_mut().push((now(), wparam.0)));
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
        }
    }

    unsafe extern "system" fn record(_hook: HWINEVENTHOOK, kind: u32, window: HWND, object: i32, child: i32, _thread: u32, _time: u32) {
        let class = class_name(window);
        SEEN.with(|seen| seen.borrow_mut().push(Seen { at: now(), kind, window: window.0 as isize, class, object, child }));
    }

    fn class_name(window: HWND) -> String {
        if window.is_invalid() {
            return String::new();
        }
        let mut buffer = [0u16; 128];
        let length = unsafe { GetClassNameW(window, &mut buffer) }.clamp(0, 128) as usize;
        String::from_utf16_lossy(&buffer[..length])
    }

    fn drain() {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            let _ = unsafe { TranslateMessage(&message) };
            unsafe { DispatchMessageW(&message) };
        }
    }

    fn pump(duration: Duration) {
        let end = Instant::now() + duration;
        loop {
            drain();
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            unsafe { MsgWaitForMultipleObjects(None, false, left.as_millis().min(u128::from(u32::MAX)) as u32, QS_ALLINPUT) };
        }
    }

    fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if done() {
                return Some(started.elapsed());
            }
            pump(Duration::from_millis(100));
        }
        done().then(|| started.elapsed())
    }

    fn mark() -> Marks {
        (SEEN.with(|seen| seen.borrow().len()), NOTES.with(|notes| notes.borrow().len()), SETTINGS.with(|settings| settings.borrow().len()))
    }

    fn since(mark: Marks) -> (Vec<Seen>, Vec<Note>, Vec<Setting>) {
        (
            SEEN.with(|seen| seen.borrow().get(mark.0..).map(<[Seen]>::to_vec).unwrap_or_default()),
            NOTES.with(|notes| notes.borrow().get(mark.1..).map(<[_]>::to_vec).unwrap_or_default()),
            SETTINGS.with(|settings| settings.borrow().get(mark.2..).map(<[_]>::to_vec).unwrap_or_default()),
        )
    }

    fn rect_of(rect: RECT) -> Rect {
        Rect { x: rect.left as f32, y: rect.top as f32, w: (rect.right - rect.left) as f32, h: (rect.bottom - rect.top) as f32 }
    }

    fn show(rect: &Rect) -> String {
        format!("{:.0},{:.0} {:.0}x{:.0}", rect.x, rect.y, rect.w, rect.h)
    }

    fn margins(rect: &Rect, truth: &Rect) -> [f32; 4] {
        [truth.x - rect.x, truth.y - rect.y, rect.x + rect.w - (truth.x + truth.w), rect.y + rect.h - (truth.y + truth.h)]
    }

    fn display_monitors() -> Vec<(Rect, u32)> {
        unsafe extern "system" fn each(monitor: HMONITOR, _dc: HDC, _clip: *mut RECT, data: LPARAM) -> ::windows::core::BOOL {
            let found = unsafe { &mut *(data.0 as *mut Vec<(Rect, u32)>) };
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let (mut x, mut y) = (0u32, 0u32);
            let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
            if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
                found.push((rect_of(info.rcMonitor), x));
            }
            true.into()
        }
        let mut found: Vec<(Rect, u32)> = Vec::new();
        let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut found as *mut Vec<(Rect, u32)> as isize)) };
        found
    }

    fn work_area() -> Option<RECT> {
        let mut area = RECT::default();
        unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut area as *mut RECT as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.ok()?;
        Some(area)
    }

    fn items_on_thread(single: bool) -> String {
        let outcome = std::thread::spawn(move || -> Result<usize> {
            com::enter(single)?;
            let view = shell::view()?;
            Ok(shell::items(&view)?.len())
        })
        .join();
        match outcome {
            Ok(Ok(count)) => format!("{count} items"),
            Ok(Err(error)) => format!("failed with {error:#}"),
            Err(_) => "the thread stopped".into(),
        }
    }

    fn facts(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        p.line(format!("- Windows build {:?}", startup::build()));
        p.line(format!("- Explorer running {}", shell::explorer()));
        p.line(format!("- system DPI {}", unsafe { GetDpiForSystem() }));
        let wallpaper = picture::object()?;
        let monitors = picture::monitors(&wallpaper)?;
        p.line(format!("- IDesktopWallpaper monitors {}", monitors.len()));
        for monitor in &monitors {
            p.line(format!("  - `{}` at {}", monitor.id, show(&monitor.frame)));
        }
        for (rect, dpi) in display_monitors() {
            p.line(format!("  - GetMonitorInfo {} at DPI {dpi}", show(&rect)));
        }
        p.line(format!("- wallpaper position {:?}, slideshow state {:?}", unsafe { wallpaper.GetPosition() }.map(|position| position.0), unsafe { wallpaper.GetStatus() }.map(|state| state.0)));
        match desktop.screens() {
            Ok(screens) => {
                for screen in screens {
                    let shown = desktop.wallpaper(&screen);
                    p.line(format!("- adapter screen `{}` at {} shows {:?}", screen.id, show(&screen.frame), shown.as_ref().map_err(|error| error.to_string())));
                    p.wallpapers.push((screen, shown.ok().flatten()));
                }
            }
            Err(error) => p.line(format!("- adapter screens failed with {error:#}")),
        }
        match shell::view() {
            Ok(view) => {
                p.line(format!("- desktop list found, host class `{}`, list class `{}`, visible {}", class_name(view.host), class_name(view.list), shell::visible(&view)));
                p.line(format!("- list DPI {}", unsafe { GetDpiForWindow(view.list) }));
                let mut client = RECT::default();
                let _ = unsafe { GetClientRect(view.list, &mut client) };
                let mut corner = [POINT { x: 0, y: 0 }];
                unsafe { MapWindowPoints(Some(view.list), None, &mut corner) };
                p.line(format!("- list client {}x{} with its origin at {},{} on screen", client.right, client.bottom, corner[0].x, corner[0].y));
                p.line(format!("- view mode and icon size {:?}", shell::mode(&view).map(|(mode, icon)| (mode.0, icon))));
                p.line(format!("- spacing {:?}", shell::spacing(&view).map(|point| (point.x, point.y))));
                p.line(format!("- folder flags {:?}", unsafe { view.folder.GetCurrentFolderFlags() }.map(|flags| format!("{flags:#x}"))));
                let font = shell::Font::open();
                p.line(format!("- icon title font line height {:?}", font.as_ref().map(|font| font.line).map_err(|error| error.to_string())));
                let dpi = unsafe { GetDpiForWindow(view.list) };
                let mut logical = LOGFONTW::default();
                let scaled = unsafe { SystemParametersInfoForDpi(SPI_GETICONTITLELOGFONT.0, size_of::<LOGFONTW>() as u32, Some(&mut logical as *mut LOGFONTW as *mut c_void), 0, dpi) };
                p.line(format!("- icon title font height for DPI {dpi} is {:?}", scaled.map(|()| logical.lfHeight)));
            }
            Err(error) => p.line(format!("- desktop list not found {error:#}")),
        }
        p.line(format!("- items read from a multithreaded apartment {}", items_on_thread(false)));
        p.line(format!("- items read from a single threaded apartment {}", items_on_thread(true)));
        let mut key = HKEY::default();
        let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops"), None, KEY_READ, &mut key) } == ERROR_SUCCESS;
        if opened {
            let _ = unsafe { RegCloseKey(key) };
        }
        p.line(format!("- VirtualDesktops key present {opened}"));
        p.line(format!("- login entry {:?}", startup::entry()));
        match desktop.snapshot() {
            Ok(snapshot) => {
                p.line(format!("- adapter snapshot {} icons", snapshot.icons.len()));
                for icon in &snapshot.icons {
                    p.raw(format!("snapshot icon {}", show(icon)));
                }
            }
            Err(error) => p.line(format!("- adapter snapshot failed {error:#}")),
        }
        for (name, result) in desktop.check() {
            p.line(format!("- doctor {name} {}", result.map_or_else(|error| format!("fail {error}"), |detail| format!("ok {detail}"))));
        }
        Ok(())
    }

    fn names(view: &shell::View) -> Result<Vec<String>> {
        Ok(shell::items(view)?.into_iter().map(|item| item.name).collect())
    }

    fn shown_name(name: &str, folder: bool) -> String {
        if folder {
            name.to_string()
        } else {
            name.trim_end_matches(".txt").to_string()
        }
    }

    fn present(found: &[String], name: &str, folder: bool) -> bool {
        found.iter().any(|seen| seen == name || *seen == shown_name(name, folder))
    }

    fn read(p: &mut Probe) -> Result<()> {
        let view = shell::view()?;
        let flags = unsafe { view.folder.GetCurrentFolderFlags() }?;
        p.flags = Some(flags);
        p.mode = shell::mode(&view).ok();
        let mask = (FWF_AUTOARRANGE.0 | FWF_SNAPTOGRID.0) as u32;
        let set = unsafe { view.folder.SetCurrentFolderFlags(mask, 0) };
        p.line(format!("- folder flags were {flags:#x}, turning auto arrange and snap off gave {:?} and now {:?}", set, unsafe { view.folder.GetCurrentFolderFlags() }.map(|flags| format!("{flags:#x}"))));
        let before = mark();
        let base = now();
        for (name, folder) in PROBES {
            let path = p.desktop.join(name);
            let made = if folder { fs::create_dir_all(&path) } else { fs::write(&path, b"scooch probe") };
            made.with_context(|| format!("could not create {}", path.display()))?;
            p.made.push(path);
        }
        let waited = wait_until(FILE_WAIT, || names(&view).is_ok_and(|found| PROBES.iter().all(|(name, folder)| present(&found, name, *folder))));
        let (_, notes, _) = since(before);
        p.line(format!("- all five items in the view after {:?} ms, waited at most {} ms", waited.map(|time| time.as_millis()), FILE_WAIT.as_millis()));
        for source in ["root", "folder"] {
            let kinds: Vec<String> = notes.iter().filter(|note| note.1 == source).map(|note| format!("{:#x} at +{} ms", note.2, note.0.saturating_sub(base))).collect();
            p.line(format!("- notifications on the desktop {source} {}", if kinds.is_empty() { "none".into() } else { kinds.join(" ") }));
        }
        let font = shell::Font::open()?;
        let cell = shell::cell(&view, font.line)?;
        p.line(format!("- cell width {} icon {} line {} wrap {}", cell.width, cell.icon, cell.line, cell.wrap(&SPACING)));
        let items = shell::items(&view)?;
        let rects = shell::rects(&items, &cell, &font, &SPACING);
        p.line(String::new());
        p.line("| item | position | label | adapter rect |".to_string());
        p.line("|---|---|---|---|".to_string());
        for (item, rect) in items.iter().zip(&rects) {
            let label = font.measure(&item.name, cell.wrap(&SPACING).round() as i32);
            p.line(format!("| {} | {},{} | {}x{} | {} |", item.name, item.at.x, item.at.y, label.width, label.height, show(rect)));
        }
        Ok(())
    }

    fn capture(area: RECT) -> Result<RgbImage> {
        let width = area.right - area.left;
        let height = area.bottom - area.top;
        if width <= 0 || height <= 0 {
            bail!("the capture area is empty");
        }
        let screen = unsafe { GetDC(None) };
        if screen.is_invalid() {
            bail!("no screen device context");
        }
        let memory = unsafe { CreateCompatibleDC(Some(screen)) };
        let bitmap = unsafe { CreateCompatibleBitmap(screen, width, height) };
        let old = unsafe { SelectObject(memory, bitmap.into()) };
        let copied = unsafe { BitBlt(memory, 0, 0, width, height, Some(screen), area.left, area.top, SRCCOPY) };
        unsafe { SelectObject(memory, old) };
        let header = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() };
        let mut info = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let mut data = vec![0u8; width as usize * height as usize * 4];
        let lines = unsafe { GetDIBits(memory, bitmap, 0, height as u32, Some(data.as_mut_ptr() as *mut c_void), &mut info, DIB_RGB_COLORS) };
        let _ = unsafe { DeleteObject(bitmap.into()) };
        let _ = unsafe { DeleteDC(memory) };
        unsafe { ReleaseDC(None, screen) };
        copied?;
        if lines != height {
            bail!("GetDIBits copied {lines} of {height} lines");
        }
        let (pixels, _) = data.as_chunks::<4>();
        let rgb = pixels.iter().flat_map(|pixel| [pixel[2], pixel[1], pixel[0]]).collect();
        RgbImage::from_raw(width as u32, height as u32, rgb).context("the capture did not fit")
    }

    fn changed_box(before: &RgbImage, after: &RgbImage, origin: (i32, i32)) -> Option<Rect> {
        if before.dimensions() != after.dimensions() {
            return None;
        }
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (x, y, pixel) in after.enumerate_pixels() {
            let old = before.get_pixel(x, y);
            if pixel.0.iter().zip(old.0.iter()).any(|(a, b)| a.abs_diff(*b) > 10) {
                (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
            }
        }
        (left != u32::MAX).then(|| Rect { x: origin.0 as f32 + left as f32, y: origin.1 as f32 + top as f32, w: (right - left + 1) as f32, h: (bottom - top + 1) as f32 })
    }

    fn automation_rects(automation: &IUIAutomation, list: HWND) -> Result<Vec<(String, Rect)>> {
        let element = unsafe { automation.ElementFromHandle(list) }?;
        let condition = unsafe { automation.CreateTrueCondition() }?;
        let found = unsafe { element.FindAll(TreeScope_Children, &condition) }?;
        let count = unsafe { found.Length() }?;
        let mut out = Vec::new();
        for index in 0..count {
            let Ok(child) = (unsafe { found.GetElement(index) }) else { continue };
            let name = unsafe { child.CurrentName() }.map(|name| name.to_string()).unwrap_or_default();
            let Ok(rect) = (unsafe { child.CurrentBoundingRectangle() }) else { continue };
            out.push((name, rect_of(rect)));
        }
        Ok(out)
    }

    fn pidl_of(view: &shell::View, name: &str, folder: bool) -> Result<com::Pidl> {
        let shell_folder: ::windows::Win32::UI::Shell::IShellFolder = unsafe { view.folder.GetFolder() }?;
        for pidl in shell::pidls(view)? {
            let seen = shell::name(&shell_folder, &pidl);
            if seen == name || seen == shown_name(name, folder) {
                return Ok(pidl);
            }
        }
        bail!("{name} is not in the view")
    }

    fn position(view: &shell::View, pidl: &com::Pidl) -> Result<(POINT, POINT)> {
        let client = unsafe { view.folder.GetItemPosition(pidl.0) }?;
        let mut screen = [client];
        unsafe { MapWindowPoints(Some(view.list), None, &mut screen) };
        Ok((client, screen[0]))
    }

    fn place(view: &shell::View, pidl: &com::Pidl, at: POINT) -> Result<()> {
        let list = [pidl.0 as *const ITEMIDLIST];
        let points = [at];
        unsafe { view.folder.SelectAndPositionItems(1, list.as_ptr(), Some(points.as_ptr()), SVSI_POSITIONITEM.0 as u32) }?;
        Ok(())
    }

    fn target(view: &shell::View) -> Result<POINT> {
        let mut client = RECT::default();
        unsafe { GetClientRect(view.list, &mut client) }?;
        let primary = display_monitors().first().map(|(rect, _)| *rect);
        let mut origin = [POINT { x: 0, y: 0 }];
        unsafe { MapWindowPoints(Some(view.list), None, &mut origin) };
        let center = match primary {
            Some(rect) => POINT { x: (rect.x + rect.w / 2.0) as i32 - origin[0].x, y: (rect.y + rect.h / 3.0) as i32 - origin[0].y },
            None => POINT { x: client.right / 2, y: client.bottom / 3 },
        };
        Ok(center)
    }

    fn park_cursor() {
        if let Some(area) = work_area() {
            let _ = unsafe { SetCursorPos(area.right - 20, area.top + 20) };
        }
    }

    fn geometry(p: &mut Probe) -> Result<()> {
        let view = shell::view()?;
        let font = shell::Font::open()?;
        let cell = shell::cell(&view, font.line)?;
        let wrap = cell.wrap(&SPACING).round() as i32;
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        let spot = target(&view)?;
        p.target = Some(spot);
        let mut spot_screen = [spot];
        unsafe { MapWindowPoints(Some(view.list), None, &mut spot_screen) };
        let reach = (cell.width.max(cell.icon) * 2.0) as i32 + 40;
        let area = RECT { left: spot_screen[0].x - reach, top: spot_screen[0].y - reach / 2, right: spot_screen[0].x + reach, bottom: spot_screen[0].y + reach * 2 };
        p.line(format!("- target spot {},{} in list coordinates, capture area {}", spot.x, spot.y, show(&rect_of(area))));
        park_cursor();
        let mut measured = Vec::new();
        for (index, (name, folder)) in PROBES.iter().enumerate() {
            let result = (|| -> Result<Measure> {
                let pidl = pidl_of(&view, name, *folder)?;
                let (home, _) = position(&view, &pidl)?;
                pump(Duration::from_millis(300));
                let before = capture(area)?;
                place(&view, &pidl, spot)?;
                pump(Duration::from_millis(700));
                let after = capture(area)?;
                let (_, at) = position(&view, &pidl)?;
                let shown = shown_name(name, *folder);
                let automation = automation_rects(&automation, view.list)?.into_iter().find(|(seen, _)| *seen == shown || seen == name).map(|(_, rect)| rect);
                let pixels = changed_box(&before, &after, (area.left, area.top));
                let _ = fs::write(p.out.join(format!("before-{index}.png")), scooch_core::encode_png(&before)?);
                let _ = fs::write(p.out.join(format!("after-{index}.png")), scooch_core::encode_png(&after)?);
                place(&view, &pidl, home)?;
                pump(Duration::from_millis(400));
                let label = font.measure(&shown, wrap);
                let at = (at.x as f32, at.y as f32);
                Ok(Measure { name: shown, at, label, adapter: layout::item_rect(at, &cell, label, &SPACING), automation, pixels })
            })();
            match result {
                Ok(measure) => measured.push(measure),
                Err(error) => p.line(format!("- {name} could not be measured {error:#}")),
            }
        }
        let monitors = display_monitors();
        if let Some((primary, _)) = monitors.first() {
            if let Ok(shot) = capture(rect_area(Some(*primary))) {
                let _ = fs::write(p.out.join("screen.png"), scooch_core::encode_png(&shot)?);
                p.raw(format!("full screen capture of {}", show(primary)));
            }
        }
        p.line(String::new());
        p.line("| item | position | label | adapter | UI Automation | pixels | margins to UIA | margins to pixels |".to_string());
        p.line("|---|---|---|---|---|---|---|---|".to_string());
        for measure in &measured {
            let to = |truth: &Option<Rect>| truth.map_or("none".to_string(), |truth| format!("{:?}", margins(&measure.adapter, &truth)));
            p.line(format!(
                "| {} | {:.0},{:.0} | {:.0}x{:.0} | {} | {} | {} | {} | {} |",
                measure.name,
                measure.at.0,
                measure.at.1,
                measure.label.width,
                measure.label.height,
                show(&measure.adapter),
                measure.automation.as_ref().map_or("none".into(), show),
                measure.pixels.as_ref().map_or("none".into(), show),
                to(&measure.automation),
                to(&measure.pixels)
            ));
        }
        solve(p, &measured, &cell);
        Ok(())
    }

    fn rect_area(rect: Option<Rect>) -> RECT {
        rect.map_or(RECT::default(), |rect| RECT { left: rect.x as i32, top: rect.y as i32, right: (rect.x + rect.w) as i32, bottom: (rect.y + rect.h) as i32 })
    }

    fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
        match (a, b) {
            (Some(a), Some(b)) => layout::union(&[a, b]),
            (one, other) => one.or(other),
        }
    }

    fn solve(p: &mut Probe, measured: &[Measure], cell: &Cell) {
        let mut above = f32::MIN;
        let mut tail = f32::MIN;
        let mut sides = f32::MIN;
        let mut offsets = Vec::new();
        let mut fits = Vec::new();
        for measure in measured {
            let Some(truth) = union(measure.automation, measure.pixels) else { continue };
            let lines = layout::lines(cell, measure.label);
            above = above.max(measure.at.1 - truth.y);
            tail = tail.max(truth.y + truth.h - (measure.at.1 + cell.icon + lines * cell.line));
            let anchor = measure.at.0 + cell.icon / 2.0;
            let half = (anchor - truth.x).max(truth.x + truth.w - anchor);
            let content = cell.icon.max(measure.label.width.min(cell.wrap(&SPACING)));
            sides = sides.max(2.0 * half - content);
            offsets.push(format!("{} {:.1}", measure.name, truth.x + truth.w / 2.0 - measure.at.0));
            let edges = margins(&measure.adapter, &truth);
            fits.push((measure.name.clone(), edges.iter().all(|edge| *edge >= 0.0 && *edge < 8.0)));
        }
        if above == f32::MIN {
            p.line("- no measurement to solve from".to_string());
            return;
        }
        p.line(String::new());
        p.line(format!("- truth center minus reported x, expected {} under the current reading, {}", cell.icon / 2.0, offsets.join(", ")));
        for (name, fit) in &fits {
            p.line(format!("- {name} {}", if *fit { "fits, every edge inside 0 to 8 px" } else { "does not fit the 0 to 8 px rule" }));
        }
        let solved = Spacing { inset: SPACING.inset, above: above.ceil().max(0.0), gap: SPACING.gap, below: (tail.ceil() - SPACING.gap).max(0.0), sides: sides.ceil().max(0.0) };
        p.line(format!("- needed above {above:.1}, below the label lines {tail:.1}, both sides together {sides:.1}"));
        p.line(format!(
            "- corrected constants `Spacing {{ inset: {:.0}, above: {:.0}, gap: {:.0}, below: {:.0}, sides: {:.0} }}` against the current `{:?}`",
            solved.inset, solved.above, solved.gap, solved.below, solved.sides, SPACING
        ));
    }

    fn virtual_screen() -> (i32, i32, i32, i32) {
        unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2), GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2)) }
    }

    fn mouse(x: i32, y: i32, extra: MOUSE_EVENT_FLAGS) -> bool {
        let (left, top, width, height) = virtual_screen();
        let dx = (i64::from(x - left) * 65535 / i64::from(width - 1)) as i32;
        let dy = (i64::from(y - top) * 65535 / i64::from(height - 1)) as i32;
        let flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK | extra;
        let input = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } } };
        let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
        sent == 1
    }

    fn drag(from: POINT, distance: i32) -> bool {
        let mut sent = mouse(from.x, from.y, MOUSE_EVENT_FLAGS(0));
        pump(Duration::from_millis(150));
        sent &= mouse(from.x, from.y, MOUSEEVENTF_LEFTDOWN);
        pump(Duration::from_millis(150));
        for step in 1..=15 {
            sent &= mouse(from.x + distance * step / 15, from.y, MOUSE_EVENT_FLAGS(0));
            pump(Duration::from_millis(40));
        }
        sent &= mouse(from.x + distance, from.y, MOUSEEVENTF_LEFTUP);
        sent
    }

    fn rule_names() -> [&'static str; 4] {
        ["capture end on the desktop thread", "create or destroy on the list client", "reorder on the list client", "location change of an item on the list"]
    }

    fn adapter_rule(seen: &Seen, list: isize) -> Option<usize> {
        let on_list = seen.window == list;
        if !wakes::event(seen.kind, on_list, seen.object, seen.child) {
            return None;
        }
        wakes::HOOKS.iter().position(|(low, high)| (*low..=*high).contains(&seen.kind))
    }

    fn candidate(seen: &Seen, list: isize) -> Vec<String> {
        let mut found = vec![format!("{:#06x} any window", seen.kind)];
        if seen.window == list {
            found.push(format!("{:#06x} list object {}", seen.kind, seen.object));
            if seen.child > 0 {
                found.push(format!("{:#06x} list object {} item", seen.kind, seen.object));
            }
        }
        found
    }

    struct Action {
        name: String,
        seen: Vec<Seen>,
        notes: Vec<Note>,
        settings: Vec<Setting>,
    }

    fn act(p: &mut Probe, name: &str, work: impl FnOnce(&mut Probe) -> Result<String>) -> Action {
        let before = mark();
        let outcome = work(p);
        pump(SETTLE);
        let (seen, notes, settings) = since(before);
        let detail = match outcome {
            Ok(detail) => detail,
            Err(error) => format!("failed with {error:#}"),
        };
        p.raw(format!("action {name} {detail}"));
        for event in &seen {
            p.raw(format!("  event {} {:#06x} hwnd {:#x} class {} object {} child {}", event.at, event.kind, event.window, event.class, event.object, event.child));
        }
        for note in &notes {
            p.raw(format!("  shell {} {} {:#x}", note.0, note.1, note.2));
        }
        p.line(format!("- {name} {detail}"));
        Action { name: name.into(), seen, notes, settings }
    }

    fn summarize(p: &mut Probe, action: &Action, list: isize) {
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for seen in &action.seen {
            let target = if seen.window == list { "list" } else { seen.class.as_str() };
            *kinds.entry(format!("{:#06x} {} obj {} {}", seen.kind, target, seen.object, if seen.child > 0 { "item" } else { "self" })).or_default() += 1;
        }
        let rules: Vec<&str> = (0..4).filter(|rule| action.seen.iter().any(|seen| adapter_rule(seen, list) == Some(*rule))).map(|rule| rule_names()[rule]).collect();
        let shell_hits = action.notes.iter().filter(|note| note.1 == "root" && (note.2 as u32) & ADAPTER_EVENTS != 0).count();
        let settings: Vec<String> = action.settings.iter().map(|(_, value)| format!("{value:#x}")).collect();
        p.line(format!("  - events {}", if kinds.is_empty() { "none".into() } else { kinds.iter().map(|(kind, count)| format!("{kind} x{count}")).collect::<Vec<_>>().join(", ") }));
        p.line(format!("  - adapter hooks {}", if rules.is_empty() { "silent".into() } else { rules.join(", ") }));
        p.line(format!("  - adapter shell notification {}", if shell_hits > 0 { format!("fired {shell_hits} times") } else { "silent".into() }));
        if !settings.is_empty() {
            p.line(format!("  - setting changes {}", settings.join(" ")));
        }
    }

    fn whole_hook(list: HWND) -> Option<HWINEVENTHOOK> {
        let mut process = 0u32;
        let thread = unsafe { GetWindowThreadProcessId(list, Some(&mut process)) };
        if thread == 0 {
            return None;
        }
        let hook = unsafe { SetWinEventHook(EVENT_MIN, EVENT_MAX, None, Some(record), process, thread, WINEVENT_OUTOFCONTEXT) };
        (!hook.is_invalid()).then_some(hook)
    }

    fn events(p: &mut Probe) -> Result<()> {
        let view = shell::view()?;
        let list = view.list.0 as isize;
        let hook = whole_hook(view.list).context("could not hook the desktop thread")?;
        let spot = p.target.unwrap_or(target(&view)?);
        let (name, folder) = PROBES[0];
        let font = shell::Font::open()?;
        let cell = shell::cell(&view, font.line)?;
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        park_cursor();
        pump(SETTLE);
        let mut actions = Vec::new();
        actions.push(act(p, "move with SelectAndPositionItems", |_| {
            let pidl = pidl_of(&view, name, folder)?;
            place(&view, &pidl, spot)?;
            Ok(format!("to {},{}", spot.x, spot.y))
        }));
        actions.push(act(p, "drag 300 px with SendInput", |_| {
            let shown = shown_name(name, folder);
            let rect = automation_rects(&automation, view.list)?.into_iter().find(|(seen, _)| *seen == shown).map(|(_, rect)| rect).context("no UI Automation rect for the probe item")?;
            let from = POINT { x: (rect.x + rect.w / 2.0) as i32, y: (rect.y + cell.icon / 2.0) as i32 };
            let pidl = pidl_of(&view, name, folder)?;
            let (_, before) = position(&view, &pidl)?;
            let sent = drag(from, 300);
            pump(Duration::from_millis(800));
            let (_, after) = position(&view, &pidl)?;
            Ok(format!("from {},{} sent {sent}, item moved from {},{} to {},{}", from.x, from.y, before.x, before.y, after.x, after.y))
        }));
        let fresh = p.desktop.join("scooch-probe-new.txt");
        let renamed = p.desktop.join("scooch-probe-renamed.txt");
        p.made.push(fresh.clone());
        p.made.push(renamed.clone());
        actions.push(act(p, "create a file", |_| {
            fs::write(&fresh, b"new")?;
            Ok("scooch-probe-new.txt".into())
        }));
        actions.push(act(p, "rename a file", |_| {
            fs::rename(&fresh, &renamed)?;
            Ok("to scooch-probe-renamed.txt".into())
        }));
        actions.push(act(p, "delete a file", |_| {
            fs::remove_file(&renamed)?;
            Ok("scooch-probe-renamed.txt".into())
        }));
        let original = shell::mode(&view)?;
        let other = if original.1 == 32 { 48 } else { 32 };
        actions.push(act(p, "change the icon size", |_| {
            unsafe { view.folder.SetViewModeAndIconSize(FVM_ICON, other) }?;
            Ok(format!("from {} to {other}", original.1))
        }));
        actions.push(act(p, "put the icon size back", |_| {
            unsafe { view.folder.SetViewModeAndIconSize(original.0, original.1) }?;
            Ok(format!("to {}", original.1))
        }));
        for action in &actions {
            p.line(format!("- **{}**", action.name));
            summarize(p, action, list);
        }
        park_cursor();
        pump(SETTLE);
        let rest = act(p, "rest for 20 s", |_| {
            pump(Duration::from_secs(20));
            Ok("no input".into())
        });
        summarize(p, &rest, list);
        let moving = act(p, "move the mouse for 5 s over empty desktop", |_| {
            let area = work_area().context("no work area")?;
            let width = area.right - area.left;
            let height = area.bottom - area.top;
            let started = Instant::now();
            let mut step = 0i32;
            while started.elapsed() < Duration::from_secs(5) {
                let x = area.left + width * 55 / 100 + (step * 13) % (width * 40 / 100).max(1);
                let y = area.top + height * 8 / 100 + (step * 7) % (height * 15 / 100).max(1);
                mouse(x, y, MOUSE_EVENT_FLAGS(0));
                pump(Duration::from_millis(16));
                step += 1;
            }
            Ok(format!("{step} moves"))
        });
        summarize(p, &moving, list);
        let _ = unsafe { UnhookWinEvent(hook) };
        smallest(p, &actions, &[&rest, &moving], list);
        Ok(())
    }

    fn smallest(p: &mut Probe, actions: &[Action], quiet: &[&Action], list: isize) {
        let noisy: std::collections::BTreeSet<String> = quiet.iter().flat_map(|action| action.seen.iter().flat_map(|seen| candidate(seen, list))).collect();
        let mut covers: BTreeMap<String, std::collections::BTreeSet<usize>> = BTreeMap::new();
        for (index, action) in actions.iter().enumerate() {
            for seen in &action.seen {
                for rule in candidate(seen, list) {
                    if !noisy.contains(&rule) {
                        covers.entry(rule).or_default().insert(index);
                    }
                }
            }
        }
        let mut left: std::collections::BTreeSet<usize> = (0..actions.len()).collect();
        let mut chosen = Vec::new();
        while !left.is_empty() {
            let best = covers.iter().max_by_key(|(_, hits)| hits.intersection(&left).count()).filter(|(_, hits)| hits.intersection(&left).count() > 0).map(|(rule, hits)| (rule.clone(), hits.clone()));
            let Some((rule, hits)) = best else { break };
            for index in &hits {
                left.remove(index);
            }
            chosen.push(rule);
        }
        let missed: Vec<&str> = left.iter().map(|index| actions[*index].name.as_str()).collect();
        p.line(String::new());
        p.line(format!("- rules that fired at rest or during mouse moves {}", if noisy.is_empty() { "none".into() } else { noisy.iter().cloned().collect::<Vec<_>>().join(", ") }));
        p.line(format!("- smallest hook set silent at rest {}", if chosen.is_empty() { "none".into() } else { chosen.join(", ") }));
        p.line(format!("- actions no quiet hook catches {}", if missed.is_empty() { "none".into() } else { missed.join(", ") }));
        let adapter_noise: Vec<&str> = (0..4).filter(|rule| quiet.iter().any(|action| action.seen.iter().any(|seen| adapter_rule(seen, list) == Some(*rule)))).map(|rule| rule_names()[rule]).collect();
        p.line(format!("- adapter hooks that fired at rest or during mouse moves {}", if adapter_noise.is_empty() { "none".into() } else { adapter_noise.join(", ") }));
    }

    fn detailed(width: u32, height: u32) -> RgbImage {
        let mut bytes = Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                let fine = if (x + y) % 2 == 0 { 40i32 } else { -40 };
                let stripe = if (x / 3 + y / 5) % 2 == 0 { 30i32 } else { -30 };
                let r = (x * 255 / width.max(1)) as i32 + fine;
                let g = (y * 255 / height.max(1)) as i32 - fine;
                let b = 128 + stripe + if (x % 17 == 0) || (y % 13 == 0) { 90 } else { 0 };
                bytes.extend_from_slice(&[r.clamp(0, 255) as u8, g.clamp(0, 255) as u8, b.clamp(0, 255) as u8]);
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

    fn psnr(a: &RgbImage, b: &RgbImage) -> Option<f64> {
        if a.dimensions() != b.dimensions() {
            return None;
        }
        let total: f64 = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2)).sum();
        let mse = total / a.as_raw().len().max(1) as f64;
        Some(if mse == 0.0 { f64::INFINITY } else { 10.0 * (255.0f64 * 255.0 / mse).log10() })
    }

    fn crop(image: &RgbImage, area: RECT, origin: (i32, i32)) -> Option<RgbImage> {
        let left = u32::try_from(area.left - origin.0).ok()?;
        let top = u32::try_from(area.top - origin.1).ok()?;
        let right = u32::try_from(area.right - origin.0).ok()?.min(image.width());
        let bottom = u32::try_from(area.bottom - origin.1).ok()?.min(image.height());
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

    fn themes_since(when: SystemTime) -> Vec<PathBuf> {
        let Some(roaming) = dirs::config_dir() else { return Vec::new() };
        let mut found = Vec::new();
        let mut folders = vec![roaming.join(r"Microsoft\Windows\Themes")];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = fs::read_dir(&folder) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    folders.push(path);
                } else if meta.modified().is_ok_and(|changed| changed + Duration::from_secs(1) >= when) {
                    found.push(path);
                }
            }
        }
        found
    }

    fn wallpaper(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screens = desktop.screens()?;
        let object = picture::object()?;
        p.line(format!("- position {:?}", unsafe { object.GetPosition() }.map(|position| position.0)));
        if p.wallpapers.is_empty() {
            for screen in &screens {
                let shown = desktop.wallpaper(screen).ok().flatten();
                p.wallpapers.push((screen.clone(), shown));
            }
        }
        for (screen, shown) in p.wallpapers.clone() {
            p.line(format!("- recorded `{}` showing {:?}", screen.id, shown));
        }
        let screen = screens.first().context("no screen")?.clone();
        let (width, height) = (screen.frame.w as u32, screen.frame.h as u32);
        let source = detailed(width, height);
        let jpeg_path = p.home.join("probe-picture.jpg");
        let png_path = p.home.join("probe-picture.png");
        fs::write(&jpeg_path, jpeg(&source)?)?;
        fs::write(&png_path, scooch_core::encode_png(&source)?)?;
        p.pictures = vec![jpeg_path.clone(), png_path.clone()];
        p.line(format!("- test picture {width}x{height}, JPEG {} bytes, PNG {} bytes", fs::metadata(&jpeg_path)?.len(), fs::metadata(&png_path)?.len()));
        let system = picture::system_decode(&png_path).map(|decoded| decoded.as_raw().iter().zip(source.as_raw()).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0));
        p.line(format!("- WIC decode of the PNG, largest channel difference {:?}", system.map_err(|error| error.to_string())));
        let view = shell::view().ok();
        let hook = view.as_ref().and_then(|view| whole_hook(view.list));
        let list = view.as_ref().map_or(0, |view| view.list.0 as isize);
        let mut scores = Vec::new();
        for (kind, path) in [("JPEG", jpeg_path.clone()), ("PNG", png_path.clone())] {
            let file = scooch_core::open(&path)?;
            let when = SystemTime::now();
            let base = now();
            let started = Instant::now();
            let action = act(p, &format!("set the {kind} picture"), |_| {
                desktop.set_wallpaper(&screen, &path)?;
                Ok(format!("in {} ms", started.elapsed().as_millis()))
            });
            summarize(p, &action, list);
            let echo: Vec<String> = action.settings.iter().map(|(at, value)| format!("{value:#x} at +{} ms", at.saturating_sub(base))).collect();
            p.line(format!("  - WM_SETTINGCHANGE {}", if echo.is_empty() { "none".into() } else { echo.join(", ") }));
            let back = desktop.wallpaper(&screen);
            p.line(format!("  - read back {:?}, same path {}", back.as_ref().map_err(|error| error.to_string()), back.as_ref().is_ok_and(|back| back.as_deref() == Some(path.as_path()))));
            let bytes = fs::read(&path)?;
            for copy in themes_since(when) {
                let copied = fs::read(&copy).unwrap_or_default();
                let equal = copied == bytes;
                let quality = if equal { Some(f64::INFINITY) } else { scooch_core::open(&copy).ok().and_then(|decoded| psnr(&decoded, &file)) };
                p.line(format!("  - copy {} {} bytes, equal {equal}, PSNR {:?}", copy.display(), copied.len(), quality));
                if copy.file_name().is_some_and(|name| name == "TranscodedWallpaper") {
                    scores.push((kind, equal, quality));
                }
            }
            pump(Duration::from_millis(800));
            let area = work_area().unwrap_or(rect_area(Some(screen.frame)));
            if let (Ok(shot), Some(expected)) = (capture(area), crop(&source, area, (screen.frame.x as i32, screen.frame.y as i32))) {
                p.line(format!("  - screen against the source over the work area, icons included, PSNR {:?}", psnr(&shot, &expected)));
                let _ = fs::write(p.out.join(format!("wallpaper-{}.png", kind.to_lowercase())), scooch_core::encode_png(&shot)?);
            }
        }
        if let Some(hook) = hook {
            let _ = unsafe { UnhookWinEvent(hook) };
        }
        let best = scores.iter().max_by(|a, b| a.2.unwrap_or(0.0).total_cmp(&b.2.unwrap_or(0.0)));
        p.line(format!("- Windows keeps {} closer, scores {:?}", best.map_or("neither", |best| best.0), scores));
        restore(p);
        Ok(())
    }

    fn restore(p: &mut Probe) {
        let Ok(desktop) = platform::current() else { return };
        for (screen, shown) in p.wallpapers.clone() {
            match shown {
                Some(path) => {
                    let result = desktop.set_wallpaper(&screen, &path);
                    p.line(format!("- restored `{}` to {} {:?}", screen.id, path.display(), result.map_err(|error| error.to_string())));
                }
                None => p.line(format!("- `{}` had no picture file, left as it is", screen.id)),
            }
        }
        pump(Duration::from_millis(500));
    }

    fn filetime(time: FILETIME) -> u64 {
        (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
    }

    fn cpu(child: &Child) -> Option<Duration> {
        let handle = HANDLE(child.as_raw_handle());
        let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
        Some(Duration::from_nanos((filetime(kernel) + filetime(user)) * 100))
    }

    fn memory(child: &Child) -> Option<(usize, usize)> {
        let handle = HANDLE(child.as_raw_handle());
        let mut counters = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
        unsafe { GetProcessMemoryInfo(handle, &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS, counters.cb) }.ok()?;
        Some((counters.WorkingSetSize, counters.PrivateUsage))
    }

    fn threads(process: u32) -> Option<usize> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.ok()?;
        let mut entry = THREADENTRY32 { dwSize: size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut count = 0;
        let mut more = unsafe { Thread32First(snapshot, &mut entry) }.is_ok();
        while more {
            if entry.th32OwnerProcessID == process {
                count += 1;
            }
            more = unsafe { Thread32Next(snapshot, &mut entry) }.is_ok();
        }
        let _ = unsafe { CloseHandle(snapshot) };
        Some(count)
    }

    fn running(program: &str) -> Vec<u32> {
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return Vec::new() };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = Vec::new();
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
        while more {
            let length = entry.szExeFile.iter().position(|unit| *unit == 0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case(program) {
                found.push(entry.th32ProcessID);
            }
            more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
        }
        let _ = unsafe { CloseHandle(snapshot) };
        found
    }

    fn rendered(home: &Path) -> bool {
        let Ok(bytes) = fs::read(home.join("state.json")) else { return false };
        let Ok(state) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return false };
        state["screens"].as_object().is_some_and(|screens| !screens.is_empty() && screens.values().all(|screen| !screen["fingerprint"].is_null()))
    }

    fn twin() -> Result<PathBuf> {
        let exe = std::env::current_exe()?;
        let folder = exe.parent().and_then(Path::parent).context("no build folder")?;
        let twin = folder.join("scoochw.exe");
        if !twin.is_file() {
            bail!("{} is missing, so build with cargo build --release -p scooch --bins --examples", twin.display());
        }
        Ok(twin)
    }

    fn end_to_end(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = desktop.screens()?.first().context("no screen")?.clone();
        let twin = twin()?;
        if let Some(picture) = p.pictures.first().cloned() {
            desktop.set_wallpaper(&screen, &picture)?;
            p.line(format!("- wallpaper set to {}", picture.display()));
        }
        fs::write(p.home.join("state.json"), br#"{"version":1,"on":true}"#)?;
        let log = fs::File::create(p.out.join("scoochw.log"))?;
        let child = Command::new(&twin).arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).spawn()?;
        let process = child.id();
        p.line(format!("- started {} as process {process}", twin.display()));
        p.daemon = Some(child);
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(60), || rendered(&home));
        p.line(format!("- first render after {:?} ms", first.map(|time| time.as_millis())));
        let view = shell::view()?;
        let (name, folder) = PROBES[0];
        let pidl = pidl_of(&view, name, folder)?;
        let spot = p.target.unwrap_or(target(&view)?);
        let mut times = Vec::new();
        for round in 0..5 {
            let before = desktop.wallpaper(&screen)?.unwrap_or_default();
            let at = POINT { x: spot.x + if round % 2 == 0 { 0 } else { 160 }, y: spot.y + 40 * round };
            place(&view, &pidl, at)?;
            let moved = Instant::now();
            let changed = wait_until(Duration::from_secs(10), || desktop.wallpaper(&screen).ok().flatten().is_some_and(|now| now != before));
            let latency = changed.map(|_| moved.elapsed().as_millis());
            p.line(format!("- move {} to {},{} new wallpaper after {:?} ms", round + 1, at.x, at.y, latency));
            times.extend(latency);
            pump(Duration::from_millis(500));
        }
        times.sort_unstable();
        p.line(format!("- median move to new wallpaper {:?} ms", times.get(times.len() / 2)));
        park_cursor();
        pump(Duration::from_secs(3));
        let Some(child) = p.daemon.as_ref() else { bail!("the daemon handle is gone") };
        let (cpu_before, quiet_from) = (cpu(child), epoch());
        pump(Duration::from_secs(60));
        let Some(child) = p.daemon.as_ref() else { bail!("the daemon handle is gone") };
        let (cpu_after, quiet_to) = (cpu(child), epoch());
        let held = memory(child);
        p.line(format!("- CPU time before {:?} after {:?} over 60 s idle", cpu_before, cpu_after));
        if let (Some(before), Some(after)) = (cpu_before, cpu_after) {
            p.line(format!("- CPU used while idle {} ms", after.saturating_sub(before).as_millis()));
        }
        p.line(format!("- working set and private bytes {:?}", held));
        p.line(format!("- threads {:?}", threads(process)));
        let stopped = scooch::poke::stop(0);
        let mut code = None;
        if let Some(child) = p.daemon.as_mut() {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(5) {
                if let Ok(Some(status)) = child.try_wait() {
                    code = Some(status);
                    break;
                }
                pump(Duration::from_millis(50));
            }
        }
        p.line(format!("- stop event sent {stopped}, process ended {:?}", code));
        if code.is_some() {
            p.daemon = None;
        }
        let trace = fs::read_to_string(p.out.join("scoochw.log")).unwrap_or_default();
        let quiet: Vec<&str> = trace.lines().filter(|line| line.split_whitespace().next().and_then(|stamp| stamp.parse::<f64>().ok()).is_some_and(|stamp| stamp >= quiet_from && stamp <= quiet_to)).collect();
        p.line(format!("- trace lines during the idle minute {}", quiet.len()));
        for line in quiet.iter().take(20) {
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
            let removed = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            if path.exists() {
                p.line(format!("- could not delete {} {:?}", path.display(), removed.err()));
            }
        }
        if let Ok(view) = shell::view() {
            if let Some(flags) = p.flags {
                let mask = (FWF_AUTOARRANGE.0 | FWF_SNAPTOGRID.0) as u32;
                let result = unsafe { view.folder.SetCurrentFolderFlags(mask, flags & mask) };
                p.line(format!("- folder flags restored to {flags:#x} {result:?}"));
            }
            if let Some((mode, icon)) = p.mode {
                let result = unsafe { view.folder.SetViewModeAndIconSize(mode, icon) };
                p.line(format!("- view mode {} and icon size {icon} restored {result:?}", mode.0));
            }
        }
        restore(p);
        for id in p.notifications.drain(..).filter(|id| *id != 0) {
            let _ = unsafe { SHChangeNotifyDeregister(id) };
        }
        if !p.window.is_invalid() {
            let _ = unsafe { DestroyWindow(p.window) };
        }
        let removed = fs::remove_dir_all(&p.home);
        p.line(format!("- temp folder removed {:?}", removed.map_err(|error| error.to_string())));
        let left = running("scoochw.exe");
        p.line(format!("- scoochw.exe processes left {}", left.len()));
        if !left.is_empty() {
            return Err(anyhow!("scoochw.exe is still running as {left:?}"));
        }
        Ok(())
    }
}
