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

}
