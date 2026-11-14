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
    use scooch::platform::windows::layout::{self, Cell, Label, SPACING};
    use scooch::platform::windows::{com, picture, shell, startup, wakes};
    use scooch::platform::{self, Screen};
    use scooch_core::{Options, Rect, RgbImage, Scene, Scenery};
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
    use ::windows::core::{w, HSTRING, PCWSTR};
    use ::windows::Win32::Foundation::{CloseHandle, COLORREF, ERROR_SUCCESS, FILETIME, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_OBJECT_0, WPARAM};
    use ::windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, MapWindowPoints, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, CAPTUREBLT, CreateSolidBrush, UpdateWindow, DIB_RGB_COLORS, HDC, HMONITOR, LOGFONTW, MONITORINFO, SRCCOPY, ChangeDisplaySettingsExW, EnumDisplaySettingsW, CDS_TYPE, DEVMODEW, DISP_CHANGE_SUCCESSFUL, DM_PELSHEIGHT, DM_PELSWIDTH,
        ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
    };
    use ::windows::core::Interface;
    use ::windows::Win32::System::Com::{CoCreateInstance, IServiceProvider, CLSCTX_ALL, CLSCTX_INPROC_SERVER};
    use ::windows::Win32::System::Variant::VARIANT;
    use ::windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next, PROCESSENTRY32W, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32};
    use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use ::windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use ::windows::Win32::System::Registry::{RegCloseKey, RegDeleteKeyValueW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_BINARY, REG_DWORD, RRF_RT_REG_DWORD};
    use ::windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount, GetProcessTimes, OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, PROCESS_VM_READ};
    use scooch::live::{Command as LiveCommand, Helper, Hints, Reply};
    use scooch::platform::windows::layer::{self, Method};
    use ::windows::Wdk::Foundation::{NtQueryObject, OBJECT_INFORMATION_CLASS};
    use ::windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
    use ::windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK, TreeScope_Children};
    use ::windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForSystem, GetDpiForWindow, SystemParametersInfoForDpi, MDT_EFFECTIVE_DPI};
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
        MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_CONTROL, VK_SHIFT,
    };
    use ::windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use ::windows::Win32::UI::Shell::{
        IDesktopWallpaper, IFolderView2, IShellBrowser, IShellDispatch, Shell, SVSI_SELECT, IShellItem, IShellItemArray, IShellWindows, SHCreateItemFromParsingName, SHCreateShellItemArrayFromShellItem, ShellWindows, FWF_NOICONS,
        SID_STopLevelBrowser, SVGIO_ALLVIEW, SWC_DESKTOP, SWFO_NEEDDISPATCH,
        SHChangeNotification_Lock, SHChangeNotification_Unlock, SHChangeNotifyDeregister, SHChangeNotifyEntry, SHChangeNotifyRegister, SHGetSpecialFolderLocation, CSIDL_DESKTOP, CSIDL_DESKTOPDIRECTORY, FOLDERVIEWMODE,
        FVM_ICON, FWF_AUTOARRANGE, FWF_SNAPTOGRID, SHCNE_ALLEVENTS, SHCNE_CREATE, SHCNE_DELETE, SHCNE_MKDIR, SHCNE_RENAMEFOLDER, SHCNE_RENAMEITEM, SHCNE_RMDIR, SHCNE_UPDATEDIR, SHCNRF_InterruptLevel,
        SHCNRF_NewDelivery, SHCNRF_ShellLevel, SVSI_POSITIONITEM,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetShellWindow, GetWindowLongW, GetWindowRect, GetWindowTextW, IsIconic, IsWindowVisible, ShowWindow, GWL_STYLE, SW_MINIMIZE, WM_CLOSE, WS_CAPTION, PostMessageW, SYSTEM_PARAMETERS_INFO_ACTION, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER, WM_COMMAND,
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClassNameW, GetClientRect, GetSystemMetrics, GetWindowThreadProcessId, MsgWaitForMultipleObjects, PeekMessageW, RegisterClassExW,
        SetCursorPos, SystemParametersInfoW, TranslateMessage, EVENT_MAX, EVENT_MIN, MSG, PM_REMOVE, QS_ALLINPUT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        SPI_GETICONTITLELOGFONT, SPI_GETWORKAREA, SPI_GETCLIENTAREAANIMATION, SPI_SETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_SETTINGCHANGE, WNDCLASSEXW, SW_HIDE, SW_SHOWNOACTIVATE,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    const ROOT_NOTE: u32 = WM_APP + 11;
    const TOGGLE_ICONS: usize = 0x7402;
    const FOLDER_NOTE: u32 = WM_APP + 12;
    const SETTLE: Duration = Duration::from_millis(1500);
    const FILE_WAIT: Duration = Duration::from_secs(5);
    const SPARE: f32 = 10.0;
    const PROBES: [(&str, bool); 7] = [
        ("scooch-probe-a.txt", false),
        ("scooch-probe-twentyeight.txt", false),
        ("scooch probe with spaces that wraps.txt", false),
        ("scooch-probe-a-name-long-enough-that-the-desktop-has-to-cut-it-after-two-lines.txt", false),
        ("scooch-probe-folder", true),
        ("sc.txt", false),
        ("short name.txt", false),
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

    #[derive(Clone, Copy, Debug, Default)]
    struct Desk {
        list: isize,
        view: isize,
    }

    impl Desk {
        fn of(view: &shell::View) -> Self {
            Desk { list: view.list.0 as isize, view: view.host.0 as isize }
        }
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
        failed: Vec<String>,
        made: Vec<PathBuf>,
        flags: Option<u32>,
        mode: Option<(FOLDERVIEWMODE, i32)>,
        wallpapers: Vec<(Screen, Option<PathBuf>)>,
        pictures: Vec<PathBuf>,
        daemon: Option<Child>,
        target: Option<POINT>,
        shell: Option<IShellDispatch>,
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
    }

    pub fn main() -> ExitCode {
        START.get_or_init(Instant::now);
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).is_some_and(|first| first == "--hold") {
            return hold(&args[2..]);
        }
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
            failed: Vec::new(),
            made: Vec::new(),
            flags: None,
            mode: None,
            wallpapers: Vec::new(),
            pictures: Vec::new(),
            daemon: None,
            target: None,
            shell: None,
        };
        probe.section("Setup", setup);
        probe.section("1 Session facts", facts);
        probe.section("2 Read", read);
        probe.section("3 Geometry truth", geometry);
        probe.section("4 Move and events", events);
        probe.section("5 Wallpaper", wallpaper);
        probe.section("5b Solid color and slideshow", modes);
        probe.section("6 End to end", end_to_end);
        probe.section("6b Daemon glide", daemon_glide);
        probe.section("9 Live layer", live);
        probe.section("8 Display scaling", scaling);
        probe.section("7 Cleanup", cleanup);
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
        probe.save();
        if failed.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    fn setup(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        com::enter(true)?;
        p.window = window()?;
        let root = notify(p.window, CSIDL_DESKTOP, ROOT_NOTE);
        let folder = notify(p.window, CSIDL_DESKTOPDIRECTORY, FOLDER_NOTE);
        p.notifications = vec![root, folder];
        p.line(format!("- SCOOCH_HOME {}", p.home.display()));
        p.line(format!("- Desktop folder {}", p.desktop.display()));
        p.line(format!("- adapter {}", desktop.name()));
        p.line(format!("- shell notification ids, desktop root {root}, desktop folder {folder}"));
        uncover(p);
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

    fn steps(single: bool) -> Vec<String> {
        let outcome = std::thread::spawn(move || -> Vec<String> {
            let mut log = Vec::new();
            let step = |log: &mut Vec<String>, name: &str, result: std::result::Result<String, ::windows::core::Error>| -> bool {
                let ok = result.is_ok();
                log.push(format!("{name} {}", result.unwrap_or_else(|error| format!("failed {:#010x} {}", error.code().0, error.message()))));
                ok
            };
            if let Err(error) = com::enter(single) {
                log.push(format!("joining COM failed {error:#}"));
                return log;
            }
            let windows: ::windows::core::Result<IShellWindows> = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) };
            let Ok(windows) = windows else {
                step(&mut log, "CoCreateInstance ShellWindows", windows.map(|_| String::new()));
                return log;
            };
            log.push("CoCreateInstance ShellWindows ok".into());
            let place = shell::desktop_place();
            let root = VARIANT::default();
            let mut handle = 0i32;
            let dispatch = unsafe { windows.FindWindowSW(&place, &root, SWC_DESKTOP, &mut handle, SWFO_NEEDDISPATCH) };
            let Ok(dispatch) = dispatch else {
                step(&mut log, "FindWindowSW", dispatch.map(|_| String::new()));
                return log;
            };
            log.push(format!("FindWindowSW ok hwnd {handle:#x}"));
            let provider = dispatch.cast::<IServiceProvider>();
            let Ok(provider) = provider else {
                step(&mut log, "cast to IServiceProvider", provider.map(|_| String::new()));
                return log;
            };
            log.push("cast to IServiceProvider ok".into());
            let browser: ::windows::core::Result<IShellBrowser> = unsafe { provider.QueryService(&SID_STopLevelBrowser) };
            let Ok(browser) = browser else {
                step(&mut log, "QueryService SID_STopLevelBrowser", browser.map(|_| String::new()));
                return log;
            };
            log.push("QueryService SID_STopLevelBrowser ok".into());
            let view = unsafe { browser.QueryActiveShellView() };
            let Ok(view) = view else {
                step(&mut log, "QueryActiveShellView", view.map(|_| String::new()));
                return log;
            };
            log.push("QueryActiveShellView ok".into());
            let host = unsafe { view.GetWindow() };
            step(&mut log, "IShellView GetWindow", host.map(|host| format!("ok {:#x}", host.0 as isize)));
            let folder = view.cast::<IFolderView2>();
            let Ok(folder) = folder else {
                step(&mut log, "cast to IFolderView2", folder.map(|_| String::new()));
                return log;
            };
            log.push("cast to IFolderView2 ok".into());
            let count = unsafe { folder.ItemCount(SVGIO_ALLVIEW) };
            step(&mut log, "ItemCount", count.map(|count| format!("ok {count}")));
            log
        })
        .join();
        outcome.unwrap_or_else(|_| vec!["the thread stopped".into()])
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
                    if !p.wallpapers.iter().any(|(known, _)| known.id == screen.id) {
                        p.wallpapers.push((screen, shown.ok().flatten()));
                    }
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
                let font = shell::Font::open(&view);
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
        for (label, single) in [("multithreaded", false), ("single threaded", true)] {
            p.line(format!("- each view step from a fresh {label} apartment"));
            for line in steps(single) {
                p.line(format!("  - {line}"));
            }
        }
        let mut key = HKEY::default();
        let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops"), None, KEY_READ, &mut key) } == ERROR_SUCCESS;
        if opened {
            let _ = unsafe { RegCloseKey(key) };
        }
        p.line(format!("- VirtualDesktops key present {opened}"));
        p.line(format!("- login entry {:?}", startup::entry()));
        p.line(format!("- elevated {}, Explorer elevated {:?}, refused {}", startup::own_elevation(), startup::shell_elevation(), startup::refused()));
        approval_round_trip(p);
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

    fn approval_round_trip(p: &mut Probe) {
        let before = startup::approval();
        p.line(format!("- Startup apps switch before {before:?}"));
        if before.is_some() {
            p.line("- a real Startup apps switch exists, so the round trip is skipped".to_string());
            return;
        }
        let disabled = [3u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let written = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, startup::APPROVED, startup::VALUE, REG_BINARY.0, Some(disabled.as_ptr() as *const c_void), disabled.len() as u32) };
        let read = startup::approval();
        let seen = read.as_deref().is_some_and(layout::startup_disabled);
        let cleared = startup::clear_approval();
        p.line(format!("- wrote a disabled switch {written:?}, read back {read:?}, seen as disabled {seen}, cleared {:?}, left {:?}", cleared.map_err(|error| error.to_string()), startup::approval()));
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
        p.flags.get_or_insert(flags);
        if p.mode.is_none() {
            p.mode = shell::mode(&view).ok();
        }
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
        let font = shell::Font::open(&view)?;
        let cell = shell::cell(&view, font.line)?;
        p.line(format!("- cell width {} icon {} line {} wrap {}", cell.width, cell.icon, cell.line, cell.wrap(&SPACING)));
        let items = shell::items(&view)?;
        let rects = shell::rects(&items, &cell, &font, &SPACING);
        p.line(String::new());
        p.line("| item | position | label | adapter rect |".to_string());
        p.line("|---|---|---|---|".to_string());
        for (item, rect) in items.iter().zip(&rects) {
            let label = font.measure(&item.name, cell.wrap(&SPACING).round() as i32, item.unfolded);
            let open = if item.unfolded { " unfolded" } else { "" };
            p.line(format!("| {} | {},{} | {}x{}{open} | {} |", item.name, item.at.x, item.at.y, label.width, label.height, show(rect)));
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
        let copied = unsafe { BitBlt(memory, 0, 0, width, height, Some(screen), area.left, area.top, SRCCOPY | CAPTUREBLT) };
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

    fn changed_box(before: &RgbImage, after: &RgbImage, origin: (i32, i32), clip: RECT) -> Option<Rect> {
        if before.dimensions() != after.dimensions() {
            return None;
        }
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (x, y, pixel) in after.enumerate_pixels() {
            let (screen_x, screen_y) = (origin.0 + x as i32, origin.1 + y as i32);
            if screen_x < clip.left || screen_x >= clip.right || screen_y < clip.top || screen_y >= clip.bottom {
                continue;
            }
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
        let font = shell::Font::open(&view)?;
        let cell = shell::cell(&view, font.line)?;
        let wrap = cell.wrap(&SPACING).round() as i32;
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        let spot = target(&view)?;
        p.target = Some(spot);
        let mut spot_screen = [spot];
        unsafe { MapWindowPoints(Some(view.list), None, &mut spot_screen) };
        let reach = (cell.width.max(cell.icon) * 2.0) as i32 + 40;
        let area = RECT { left: spot_screen[0].x - reach, top: spot_screen[0].y - reach / 2, right: spot_screen[0].x + reach, bottom: spot_screen[0].y + reach * 2 };
        let tall = shell::spacing(&view).map_or(cell.width as i32 * 2, |spacing| spacing.y);
        let middle = spot_screen[0].x + (cell.icon / 2.0) as i32;
        let clip = RECT { left: middle - cell.width as i32, top: spot_screen[0].y - cell.line as i32, right: middle + cell.width as i32, bottom: spot_screen[0].y + tall + cell.line as i32 };
        let dpi = unsafe { GetDpiForWindow(view.list) };
        p.line(format!("- target spot {},{} in list coordinates, capture area {}, pixels judged inside {}, list DPI {dpi}, icon box {}", spot.x, spot.y, show(&rect_of(area)), show(&rect_of(clip)), cell.icon));
        let focused: Vec<String> = shell::items(&view)?.into_iter().filter(|item| item.unfolded).map(|item| item.name).collect();
        p.line(format!("- desktop list has keyboard focus {}, unfolded items {focused:?}", shell::list_focused(&view)));
        uncover(p);
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
                let pixels = changed_box(&before, &after, (area.left, area.top), clip);
                let unfolded = shell::items(&view)?.into_iter().find(|item| item.name == shown || item.name == *name).is_some_and(|item| item.unfolded);
                let state = unsafe { view.folder.GetSelectionState(pidl.0) }.map(|state| format!("{state:#x}")).unwrap_or_default();
                p.raw(format!("{shown} selection state {state}, list has keyboard focus {}, unfolded {unfolded}", shell::list_focused(&view)));
                let _ = fs::write(p.out.join(format!("before-{index}.png")), scooch_core::encode_png(&before)?);
                let _ = fs::write(p.out.join(format!("after-{index}.png")), scooch_core::encode_png(&after)?);
                place(&view, &pidl, home)?;
                pump(Duration::from_millis(400));
                let label = font.measure(&shown, wrap, unfolded);
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
                "| {} | {:.0},{:.0} | {:.0}x{:.0}{} | {} | {} | {} | {} | {} |",
                measure.name,
                measure.at.0,
                measure.at.1,
                measure.label.width,
                measure.label.height,
                if measure.label.unfolded { " unfolded" } else { "" },
                show(&measure.adapter),
                measure.automation.as_ref().map_or("none".into(), show),
                measure.pixels.as_ref().map_or("none".into(), show),
                to(&measure.automation),
                to(&measure.pixels)
            ));
        }
        solve(p, &measured, &cell, dpi);
        Ok(())
    }

    fn rect_area(rect: Option<Rect>) -> RECT {
        rect.map_or(RECT::default(), |rect| RECT { left: rect.x as i32, top: rect.y as i32, right: (rect.x + rect.w) as i32, bottom: (rect.y + rect.h) as i32 })
    }

    fn solve(p: &mut Probe, measured: &[Measure], cell: &Cell, dpi: u32) {
        let limit = SPARE * dpi.max(96) as f32 / 96.0;
        let mut negative = Vec::new();
        p.line(String::new());
        p.line(format!("- judged against drawn pixels, sideways against the drawn pixels or the {} px icon box whichever is wider, allowed spare 0 to {limit} px", cell.icon));
        for measure in measured {
            let Some(drawn) = measure.pixels else {
                p.line(format!("- {} has no drawn pixels to judge", measure.name));
                p.failed.push(format!("{} at {dpi} DPI has no drawn pixels", measure.name));
                continue;
            };
            let raw = margins(&measure.adapter, &drawn);
            let left = drawn.x.min(measure.at.0);
            let right = (drawn.x + drawn.w).max(measure.at.0 + cell.icon);
            let edges = [left - measure.adapter.x, raw[1], measure.adapter.x + measure.adapter.w - right, raw[3]];
            let verdict = if raw.iter().any(|edge| *edge < 0.0) {
                negative.push((measure, drawn));
                "misses drawn pixels"
            } else if edges.iter().any(|edge| *edge > limit) {
                "spares too much"
            } else {
                "fits"
            };
            p.line(format!("- {} {verdict}, judged {edges:?}, raw {raw:?}", measure.name));
            if verdict != "fits" {
                p.failed.push(format!("{} at {dpi} DPI {verdict} {edges:?}", measure.name));
            }
        }
        if negative.is_empty() {
            p.line(format!("- no rectangle misses a drawn pixel, so the constants stay `{SPACING:?}`"));
            return;
        }
        let mut above = Vec::new();
        let mut tail = Vec::new();
        let mut one_line = Vec::new();
        for (measure, drawn) in &negative {
            let lines = layout::lines(cell, measure.label);
            above.push(measure.at.1 - drawn.y);
            tail.push(drawn.y + drawn.h - (measure.at.1 + cell.icon + lines * cell.line));
            let anchor = measure.at.0 + cell.icon / 2.0;
            let width = 2.0 * (anchor - drawn.x).max(drawn.x + drawn.w - anchor);
            one_line.push(width - cell.icon.max(measure.label.width.min(cell.cap(&SPACING))));
        }
        let most = |values: &[f32]| values.iter().copied().fold(None, |best: Option<f32>, value| Some(best.map_or(value, |best| best.max(value))));
        p.line(format!(
            "- corrected constants from the misses, top at least {:?} px above the reported point, {:?} px below the label lines, {:?} px of sides",
            most(&above).map(f32::ceil),
            most(&tail).map(f32::ceil),
            most(&one_line).map(f32::ceil)
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
        drag_to(from, POINT { x: from.x + distance, y: from.y })
    }

    fn drag_to(from: POINT, to: POINT) -> bool {
        let mut sent = mouse(from.x, from.y, MOUSE_EVENT_FLAGS(0));
        pump(Duration::from_millis(150));
        sent &= mouse(from.x, from.y, MOUSEEVENTF_LEFTDOWN);
        pump(Duration::from_millis(150));
        for step in 1..=15 {
            sent &= mouse(from.x + (to.x - from.x) * step / 15, from.y + (to.y - from.y) * step / 15, MOUSE_EVENT_FLAGS(0));
            pump(Duration::from_millis(40));
        }
        sent &= mouse(to.x, to.y, MOUSEEVENTF_LEFTUP);
        sent
    }

    fn key(code: VIRTUAL_KEY, up: bool) -> INPUT {
        let flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: code, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    fn chord(codes: &[VIRTUAL_KEY]) -> bool {
        let mut inputs: Vec<INPUT> = codes.iter().map(|code| key(*code, false)).collect();
        inputs.extend(codes.iter().rev().map(|code| key(*code, true)));
        let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
        sent as usize == inputs.len()
    }

    fn click(at: POINT, right: bool) -> bool {
        let (down, up) = if right { (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP) } else { (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP) };
        let mut sent = mouse(at.x, at.y, MOUSE_EVENT_FLAGS(0));
        pump(Duration::from_millis(100));
        sent &= mouse(at.x, at.y, down);
        pump(Duration::from_millis(80));
        sent &= mouse(at.x, at.y, up);
        sent
    }

    fn control_wheel(at: POINT, notches: i32) -> bool {
        let mut sent = mouse(at.x, at.y, MOUSE_EVENT_FLAGS(0));
        pump(Duration::from_millis(150));
        let down = [key(VK_CONTROL, false)];
        sent &= unsafe { SendInput(&down, size_of::<INPUT>() as i32) } == 1;
        pump(Duration::from_millis(80));
        for _ in 0..notches.abs() {
            let wheel = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: (120 * notches.signum()) as u32, dwFlags: MOUSEEVENTF_WHEEL, time: 0, dwExtraInfo: 0 } } };
            sent &= unsafe { SendInput(&[wheel], size_of::<INPUT>() as i32) } == 1;
            pump(Duration::from_millis(120));
        }
        let up = [key(VK_CONTROL, true)];
        sent &= unsafe { SendInput(&up, size_of::<INPUT>() as i32) } == 1;
        sent
    }

    fn bag_icon_size() -> Option<u32> {
        let mut value = 0u32;
        let mut size = size_of::<u32>() as u32;
        let read = unsafe { RegGetValueW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\Shell\Bags\1\Desktop"), w!("IconSize"), RRF_RT_REG_DWORD, None, Some(&mut value as *mut u32 as *mut c_void), Some(&mut size)) };
        (read == ERROR_SUCCESS).then_some(value)
    }

    fn empty_spot() -> POINT {
        work_area().map_or(POINT { x: 700, y: 500 }, |area| POINT { x: area.left + (area.right - area.left) * 70 / 100, y: area.top + (area.bottom - area.top) * 70 / 100 })
    }

    fn uncover(p: &mut Probe) {
        let shell: ::windows::core::Result<IShellDispatch> = unsafe { CoCreateInstance(&Shell, None, CLSCTX_ALL) };
        match shell {
            Ok(shell) => {
                let minimized = unsafe { shell.MinimizeAll() };
                pump(Duration::from_millis(1500));
                p.line(format!("- minimized every window so the desktop shows {minimized:?}"));
                p.shell = Some(shell);
            }
            Err(error) => p.line(format!("- could not reach Shell.Application {error}")),
        }
        for (class, title, done) in clear_windows(p.window) {
            p.line(format!("- {done} a window in the way, class `{class}` titled `{title}`"));
        }
    }

    fn clear_windows(own: HWND) -> Vec<(String, String, &'static str)> {
        unsafe extern "system" fn each(window: HWND, data: LPARAM) -> ::windows::core::BOOL {
            let found = unsafe { &mut *(data.0 as *mut Vec<isize>) };
            found.push(window.0 as isize);
            true.into()
        }
        let mut windows: Vec<isize> = Vec::new();
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut windows as *mut Vec<isize> as isize)) };
        let mut cleared = Vec::new();
        let keep = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd", "ScoochProbe", "ScoochWatch"];
        for raw in windows {
            let window = HWND(raw as *mut c_void);
            if window == own || !unsafe { IsWindowVisible(window) }.as_bool() || unsafe { IsIconic(window) }.as_bool() {
                continue;
            }
            let class = class_name(window);
            if keep.contains(&class.as_str()) {
                continue;
            }
            let mut frame = RECT::default();
            if unsafe { GetWindowRect(window, &mut frame) }.is_err() || frame.right - frame.left < 40 || frame.bottom - frame.top < 40 {
                continue;
            }
            let mut text = [0u16; 128];
            let length = unsafe { GetWindowTextW(window, &mut text) }.clamp(0, 128) as usize;
            let title = String::from_utf16_lossy(&text[..length]);
            if class == "#32770" {
                let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
                cleared.push((class, title, "closed"));
            } else if unsafe { GetWindowLongW(window, GWL_STYLE) } as u32 & WS_CAPTION.0 == WS_CAPTION.0 {
                let _ = unsafe { ShowWindow(window, SW_MINIMIZE) };
                cleared.push((class, title, "minimized"));
            }
        }
        if !cleared.is_empty() {
            pump(Duration::from_millis(800));
        }
        cleared
    }

    fn rule_names() -> [&'static str; 2] {
        ["capture end on the desktop thread", "create, destroy, show or hide of the list or its view"]
    }

    fn adapter_rule(seen: &Seen, desk: Desk) -> Option<usize> {
        let on_list = seen.window == desk.list;
        let on_view = seen.window == desk.view;
        if !wakes::event(seen.kind, on_list, on_view, seen.object, seen.child) {
            return None;
        }
        wakes::HOOKS.iter().position(|(low, high)| (*low..=*high).contains(&seen.kind))
    }

    fn candidate(seen: &Seen, desk: Desk) -> Vec<String> {
        let mut found = vec![format!("{:#06x} any window", seen.kind)];
        if seen.window == desk.view {
            found.push(format!("{:#06x} view object {}", seen.kind, seen.object));
        }
        if seen.window == desk.list {
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

    fn summarize(p: &mut Probe, action: &Action, desk: Desk) {
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for seen in &action.seen {
            let target = if seen.window == desk.list {
                "list"
            } else if seen.window == desk.view {
                "view"
            } else {
                seen.class.as_str()
            };
            *kinds.entry(format!("{:#06x} {} obj {} {}", seen.kind, target, seen.object, if seen.child > 0 { "item" } else { "self" })).or_default() += 1;
        }
        let rules: Vec<&str> = (0..wakes::HOOKS.len()).filter(|rule| action.seen.iter().any(|seen| adapter_rule(seen, desk) == Some(*rule))).map(|rule| rule_names()[rule]).collect();
        let shell_hits = action.notes.iter().filter(|note| note.1 == "root" && (note.2 as u32) & ADAPTER_EVENTS != 0).count();
        let settings: Vec<String> = action.settings.iter().map(|(_, value)| format!("{value:#x}")).collect();
        p.line(format!("  - events {}", if kinds.is_empty() { "none".into() } else { kinds.iter().map(|(kind, count)| format!("{kind} x{count}")).collect::<Vec<_>>().join(", ") }));
        p.line(format!("  - adapter hooks {}", if rules.is_empty() { "silent".into() } else { rules.join(", ") }));
        p.line(format!("  - adapter shell notification {}", if shell_hits > 0 { format!("fired {shell_hits} times") } else { "silent".into() }));
        if !settings.is_empty() {
            p.line(format!("  - setting changes {}", settings.join(" ")));
        }
    }

    fn toggle_icons(view: &shell::View, show: bool) -> Result<String> {
        let before = shell::visible(view);
        let mask = FWF_NOICONS.0 as u32;
        let flag = unsafe { view.folder.SetCurrentFolderFlags(mask, if show { 0 } else { mask }) };
        pump(Duration::from_millis(600));
        let after_flag = shell::visible(view);
        let mut detail = format!("visible {before} then {after_flag} through FWF_NOICONS {flag:?}");
        if after_flag != show {
            let posted = unsafe { PostMessageW(Some(view.host), WM_COMMAND, WPARAM(TOGGLE_ICONS), LPARAM(0)) };
            pump(Duration::from_millis(600));
            detail.push_str(&format!(", then {} through the menu command {posted:?}", shell::visible(view)));
        }
        Ok(detail)
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
        uncover(p);
        let view = shell::view()?;
        let desk = Desk::of(&view);
        let hook = whole_hook(view.list).context("could not hook the desktop thread")?;
        let spot = p.target.unwrap_or(target(&view)?);
        let (name, folder) = PROBES[0];
        let font = shell::Font::open(&view)?;
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
        actions.push(act(p, "change the icon size through IFolderView2", |_| {
            unsafe { view.folder.SetViewModeAndIconSize(FVM_ICON, other) }?;
            pump(Duration::from_millis(300));
            Ok(format!("from {} to {other}, Bags IconSize {:?}", original.1, bag_icon_size()))
        }));
        actions.push(act(p, "put the icon size back through IFolderView2", |_| {
            unsafe { view.folder.SetViewModeAndIconSize(original.0, original.1) }?;
            pump(Duration::from_millis(300));
            Ok(format!("to {}, Bags IconSize {:?}", original.1, bag_icon_size()))
        }));
        actions.push(act(p, "make icons smaller with Ctrl and the wheel", |_| {
            let before = (shell::mode(&view).map(|mode| mode.1).ok(), bag_icon_size());
            let sent = control_wheel(empty_spot(), -1);
            pump(Duration::from_millis(600));
            Ok(format!("sent {sent}, view size and Bags IconSize {before:?} then {:?}", (shell::mode(&view).map(|mode| mode.1).ok(), bag_icon_size())))
        }));
        actions.push(act(p, "make icons larger with Ctrl and the wheel", |_| {
            let before = (shell::mode(&view).map(|mode| mode.1).ok(), bag_icon_size());
            let sent = control_wheel(empty_spot(), 1);
            pump(Duration::from_millis(600));
            Ok(format!("sent {sent}, view size and Bags IconSize {before:?} then {:?}", (shell::mode(&view).map(|mode| mode.1).ok(), bag_icon_size())))
        }));
        let _ = unsafe { view.folder.SetViewModeAndIconSize(original.0, original.1) };
        pump(Duration::from_millis(600));
        let _ = click(empty_spot(), false);
        pump(Duration::from_millis(600));
        actions.push(act(p, "make icons small with Ctrl Shift 4 on the focused desktop", |_| {
            let before = shell::mode(&view).map(|mode| mode.1).ok();
            let sent = chord(&[VK_CONTROL, VK_SHIFT, VIRTUAL_KEY(0x34)]);
            pump(Duration::from_millis(600));
            Ok(format!("sent {sent}, view size {before:?} then {:?}", shell::mode(&view).map(|mode| mode.1).ok()))
        }));
        actions.push(act(p, "make icons medium with Ctrl Shift 3 on the focused desktop", |_| {
            let before = shell::mode(&view).map(|mode| mode.1).ok();
            let sent = chord(&[VK_CONTROL, VK_SHIFT, VIRTUAL_KEY(0x33)]);
            pump(Duration::from_millis(600));
            Ok(format!("sent {sent}, view size {before:?} then {:?}", shell::mode(&view).map(|mode| mode.1).ok()))
        }));
        let _ = unsafe { view.folder.SetViewModeAndIconSize(original.0, original.1) };
        pump(Duration::from_millis(600));
        actions.push(act(p, "hide desktop icons", |_| toggle_icons(&view, false)));
        actions.push(act(p, "show desktop icons", |_| toggle_icons(&view, true)));
        if !shell::visible(&view) {
            let again = toggle_icons(&view, true);
            p.line(format!("- icons were still hidden, showing them again gave {:?}", again.map_err(|error| error.to_string())));
        }
        for action in &actions {
            p.line(format!("- **{}**", action.name));
            summarize(p, action, desk);
        }
        park_cursor();
        pump(SETTLE);
        let rest = act(p, "rest for 20 s", |_| {
            pump(Duration::from_secs(20));
            Ok("no input".into())
        });
        summarize(p, &rest, desk);
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
        summarize(p, &moving, desk);
        let _ = unsafe { UnhookWinEvent(hook) };
        smallest(p, &actions, &[&rest, &moving], desk);
        Ok(())
    }

    fn smallest(p: &mut Probe, actions: &[Action], quiet: &[&Action], desk: Desk) {
        let noisy: std::collections::BTreeSet<String> = quiet.iter().flat_map(|action| action.seen.iter().flat_map(|seen| candidate(seen, desk))).collect();
        let mut covers: BTreeMap<String, std::collections::BTreeSet<usize>> = BTreeMap::new();
        for (index, action) in actions.iter().enumerate() {
            for seen in &action.seen {
                for rule in candidate(seen, desk) {
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
        let adapter_noise: Vec<&str> = (0..wakes::HOOKS.len()).filter(|rule| quiet.iter().any(|action| action.seen.iter().any(|seen| adapter_rule(seen, desk) == Some(*rule)))).map(|rule| rule_names()[rule]).collect();
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

    fn subject_picture(width: u32, height: u32) -> RgbImage {
        let mut bytes = Vec::with_capacity(width as usize * height as usize * 3);
        let radius = width.min(height) as f32 * 0.18;
        for y in 0..height {
            for x in 0..width {
                let dx = x as f32 - width as f32 / 2.0;
                let dy = y as f32 - height as f32 / 2.0;
                let inside = (dx * dx + dy * dy).sqrt() < radius;
                let grain = ((x * 7 + y * 13) % 11) as u8;
                let pixel = if inside { [245 - grain, 225 - grain, 185] } else { [18 + grain, 26 + grain, 46 + grain] };
                bytes.extend_from_slice(&pixel);
            }
        }
        RgbImage::from_raw(width, height, bytes).unwrap_or_default()
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
        let desk = view.as_ref().map(Desk::of).unwrap_or_default();
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
            summarize(p, &action, desk);
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
        let classic = act(p, "set the JPEG the way Settings does, through SystemParametersInfo", |_| {
            spi_wallpaper(&jpeg_path.to_string_lossy())?;
            Ok("with SPIF_UPDATEINIFILE and SPIF_SENDCHANGE".into())
        });
        summarize(p, &classic, desk);
        let id = (screen.id != layout::SPAN).then(|| screen.id.clone());
        p.line(format!("  - GetWallpaper for the screen {}", raw_wallpaper(&object, id.as_deref())));
        p.line(format!("  - GetWallpaper with no screen {}", raw_wallpaper(&object, None)));
        p.line(format!("  - Control Panel Desktop WallPaper {:?}", startup::read(HKEY_CURRENT_USER, w!(r"Control Panel\Desktop"), w!("WallPaper"))));
        p.line(format!("  - adapter reads {:?}", desktop.wallpaper(&screen).map_err(|error| error.to_string())));
        p.line(format!("  - Themes folder {:?}", picture::themes()));
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
                None => {
                    let result = spi_wallpaper("");
                    p.line(format!("- `{}` had no picture file, set back to a solid color {:?}", screen.id, result.map_err(|error| error.to_string())));
                }
            }
        }
        pump(Duration::from_millis(500));
    }

    fn spi_wallpaper(path: &str) -> Result<()> {
        let mut wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(wide.as_mut_ptr() as *mut c_void), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) }?;
        Ok(())
    }

    fn raw_wallpaper(object: &IDesktopWallpaper, id: Option<&str>) -> String {
        let id = id.map(HSTRING::from);
        let pointer = id.as_ref().map_or(PCWSTR::null(), |id| PCWSTR(id.as_ptr()));
        match unsafe { object.GetWallpaper(pointer) } {
            Ok(text) => format!("{:?}", com::take(text)),
            Err(error) => format!("failed with {error}"),
        }
    }

    fn report_mode(p: &mut Probe, object: &IDesktopWallpaper, screen: &Screen, label: &str) {
        let id = (screen.id != layout::SPAN).then(|| screen.id.clone());
        p.line(format!("- **{label}**"));
        p.line(format!("  - GetWallpaper for the screen {}", raw_wallpaper(object, id.as_deref())));
        p.line(format!("  - GetWallpaper with no screen {}", raw_wallpaper(object, None)));
        p.line(format!("  - GetStatus {:?}", unsafe { object.GetStatus() }.map(|state| state.0)));
        p.line(format!("  - GetBackgroundColor {:?}", unsafe { object.GetBackgroundColor() }.map(|color| format!("{:#08x}", color.0))));
        p.line(format!("  - GetPosition {:?}", unsafe { object.GetPosition() }.map(|position| position.0)));
        if let Ok(desktop) = platform::current() {
            p.line(format!("  - adapter reads {:?}", desktop.wallpaper(screen).map_err(|error| error.to_string())));
        }
    }

    fn modes(p: &mut Probe) -> Result<()> {
        let object = picture::object()?;
        let desktop = platform::current()?;
        let screen = desktop.screens()?.first().context("no screen")?.clone();
        let color = unsafe { object.GetBackgroundColor() };
        report_mode(p, &object, &screen, "as found");
        let solid = act(p, "switch to a solid color", |_| {
            spi_wallpaper("")?;
            Ok("through SystemParametersInfo with an empty path".into())
        });
        p.line(format!("  - setting changes {}", solid.settings.len()));
        report_mode(p, &object, &screen, "solid color");
        let slides = p.home.join("slides");
        fs::create_dir_all(&slides)?;
        let (width, height) = (screen.frame.w.max(64.0) as u32, screen.frame.h.max(64.0) as u32);
        let first = detailed(width, height);
        let second = RgbImage::from_raw(width, height, first.as_raw().iter().map(|value| 255 - value).collect()).context("no second slide")?;
        fs::write(slides.join("one.jpg"), jpeg(&first)?)?;
        fs::write(slides.join("two.jpg"), jpeg(&second)?)?;
        let started = act(p, "start a two picture slideshow", |_| {
            let folder: IShellItem = unsafe { SHCreateItemFromParsingName(&HSTRING::from(slides.as_path()), None) }?;
            let items: IShellItemArray = unsafe { SHCreateShellItemArrayFromShellItem(&folder) }?;
            unsafe { object.SetSlideshow(&items) }?;
            Ok(format!("from {}", slides.display()))
        });
        p.line(format!("  - setting changes {}", started.settings.len()));
        pump(Duration::from_secs(2));
        report_mode(p, &object, &screen, "slideshow");
        if let Ok(color) = color {
            let result = unsafe { object.SetBackgroundColor(color) };
            p.line(format!("- background color put back {result:?}"));
        }
        restore(p);
        report_mode(p, &object, &screen, "restored");
        Ok(())
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

    fn explorer_ready(old: u32) -> bool {
        let shell = unsafe { GetShellWindow() };
        if shell.is_invalid() {
            return false;
        }
        let mut process = 0u32;
        unsafe { GetWindowThreadProcessId(shell, Some(&mut process)) };
        process != 0 && process != old && shell::view().is_ok_and(|view| shell::visible(&view))
    }

    fn restart_explorer(p: &mut Probe) -> Result<u128> {
        let shell = unsafe { GetShellWindow() };
        if shell.is_invalid() {
            bail!("no shell window to restart");
        }
        let mut process = 0u32;
        unsafe { GetWindowThreadProcessId(shell, Some(&mut process)) };
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, false, process) }?;
        let ended = unsafe { TerminateProcess(handle, 1) };
        let _ = unsafe { CloseHandle(handle) };
        ended?;
        let started = Instant::now();
        let mut back = wait_until(Duration::from_secs(15), || explorer_ready(process));
        if back.is_none() {
            p.line("- Explorer did not come back by itself, so the probe starts it".to_string());
            let _ = Command::new("explorer.exe").spawn();
            back = wait_until(Duration::from_secs(20), || explorer_ready(process));
        }
        let took = back.map(|_| started.elapsed().as_millis()).context("Explorer did not come back")?;
        p.line(format!("- Explorer process {process} ended and a new desktop was ready after {took} ms"));
        pump(Duration::from_secs(2));
        uncover(p);
        Ok(took)
    }

    fn reattach(p: &mut Probe, trace: &Path, read: &mut u64, screen: &Screen) -> Result<()> {
        let desktop = platform::current()?;
        let _ = new_lines(trace, read);
        restart_explorer(p)?;
        let attached = wait_until(Duration::from_secs(20), || fs::read_to_string(trace).is_ok_and(|text| text.get(*read as usize..).is_some_and(|fresh| fresh.contains(" wake explorer"))));
        p.line(format!("- daemon attached to the new desktop after {:?} ms", attached.map(|time| time.as_millis())));
        if attached.is_none() {
            p.failed.push("the daemon did not attach after Explorer restarted".into());
        }
        for line in new_lines(trace, read) {
            p.line(format!("  - daemon `{}`", line.chars().take(160).collect::<String>()));
        }
        let view = shell::view()?;
        let font = shell::Font::open(&view)?;
        let cell = shell::cell(&view, font.line)?;
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        let shown = shown_name(PROBES[0].0, PROBES[0].1);
        let rect = automation_rects(&automation, view.list)?.into_iter().find(|(seen, _)| *seen == shown).map(|(_, rect)| rect).context("no UI Automation rect for the probe item after the restart")?;
        let from = POINT { x: (rect.x + rect.w / 2.0) as i32, y: (rect.y + cell.icon / 2.0) as i32 };
        let center = POINT { x: (screen.frame.x + screen.frame.w / 2.0) as i32, y: (screen.frame.y + screen.frame.h / 2.0) as i32 };
        let to = if (from.x - center.x).abs() < 60 && (from.y - center.y).abs() < 60 {
            POINT { x: (screen.frame.x + screen.frame.w * 0.8) as i32, y: (screen.frame.y + screen.frame.h * 0.2) as i32 }
        } else {
            center
        };
        let before = desktop.wallpaper(screen)?.unwrap_or_default();
        let sent = drag_to(from, to);
        let dropped = Instant::now();
        let changed = wait_until(Duration::from_secs(10), || desktop.wallpaper(screen).ok().flatten().is_some_and(|now| now != before));
        let latency = changed.map(|_| dropped.elapsed().as_millis());
        pump(Duration::from_millis(300));
        let wakes: Vec<String> = new_lines(trace, read).into_iter().filter(|line| line.contains(" wake ") || line.contains(" set ")).collect();
        p.line(format!("- drag after the restart from {},{} to {},{} sent {sent}, new wallpaper after {:?} ms", from.x, from.y, to.x, to.y, latency));
        for line in &wakes {
            p.line(format!("  - daemon `{}`", line.chars().take(160).collect::<String>()));
        }
        if !wakes.iter().any(|line| line.contains(" wake ")) {
            p.failed.push("the drag after the Explorer restart woke nothing".into());
        }
        if latency.is_none() {
            p.failed.push("the drag after the Explorer restart brought no new wallpaper".into());
        }
        park_cursor();
        Ok(())
    }

    fn override_dpi() -> (i32, ::windows::core::Result<()>) {
        let mut relative = 0i32;
        let read = unsafe { SystemParametersInfoW(SYSTEM_PARAMETERS_INFO_ACTION(0x009E), 0, Some(&mut relative as *mut i32 as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
        (relative, read)
    }

    fn set_override(relative: i32) -> ::windows::core::Result<()> {
        unsafe { SystemParametersInfoW(SYSTEM_PARAMETERS_INFO_ACTION(0x009F), relative as u32, None, SPIF_UPDATEINIFILE) }
    }

    fn primary_dpi() -> u32 {
        display_monitors().first().map_or(0, |(_, dpi)| *dpi)
    }

    fn list_dpi() -> u32 {
        shell::view().map_or(0, |view| unsafe { GetDpiForWindow(view.list) })
    }

    fn try_override(p: &mut Probe, relative: i32, wanted: u32) -> bool {
        let result = set_override(relative);
        pump(Duration::from_secs(3));
        let (monitor, list) = (primary_dpi(), list_dpi());
        p.line(format!("- SystemParametersInfo 0x009F with step {relative} gave {result:?}, monitor DPI {monitor}, desktop list DPI {list}, wanted {wanted}"));
        monitor == wanted && list == wanted
    }

    fn current_mode() -> Option<(u32, u32)> {
        let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool().then_some((mode.dmPelsWidth, mode.dmPelsHeight))
    }

    fn largest_mode() -> Option<DEVMODEW> {
        let mut best: Option<DEVMODEW> = None;
        for index in 0..512u32 {
            let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
            if !unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_DISPLAY_SETTINGS_MODE(index), &mut mode) }.as_bool() {
                break;
            }
            if best.as_ref().is_none_or(|known| mode.dmPelsWidth * mode.dmPelsHeight > known.dmPelsWidth * known.dmPelsHeight) {
                best = Some(mode);
            }
        }
        best
    }

    fn desktop_dword(name: PCWSTR) -> Option<u32> {
        let mut value = 0u32;
        let mut size = size_of::<u32>() as u32;
        let read = unsafe { RegGetValueW(HKEY_CURRENT_USER, w!(r"Control Panel\Desktop"), name, RRF_RT_REG_DWORD, None, Some(&mut value as *mut u32 as *mut c_void), Some(&mut size)) };
        (read == ERROR_SUCCESS).then_some(value)
    }

    fn set_desktop_dword(name: PCWSTR, value: Option<u32>) -> String {
        let result = match value {
            Some(value) => unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, w!(r"Control Panel\Desktop"), name, REG_DWORD.0, Some(&value as *const u32 as *const c_void), size_of::<u32>() as u32) },
            None => unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, w!(r"Control Panel\Desktop"), name) },
        };
        format!("{result:?}")
    }

    fn scaled_sections(p: &mut Probe) {
        let dpi = list_dpi();
        p.line(format!("- the desktop now runs at {dpi} DPI, so sections 1, 2 and 3 run again"));
        p.section(&format!("1 Session facts at {dpi} DPI"), facts);
        p.section(&format!("2 Read at {dpi} DPI"), read);
        p.section(&format!("3 Geometry truth at {dpi} DPI"), geometry);
        p.section(&format!("9 Live layer at {dpi} DPI"), live_scaled);
        p.line(String::new());
        p.line("## 8 Display scaling, putting it back".to_string());
        p.line(String::new());
    }

    fn scaling(p: &mut Probe) -> Result<()> {
        let (relative, read) = override_dpi();
        let start = current_mode();
        p.line(format!("- monitor DPI {}, desktop list DPI {}, system DPI {}, logical DPI override step {relative} read {read:?}, display mode {start:?}", primary_dpi(), list_dpi(), unsafe { GetDpiForSystem() }));
        let mut scaled = try_override(p, relative + 2, 144);
        let mut resized = false;
        if !scaled {
            if let (Some(mode), Some((width, height))) = (largest_mode(), start) {
                if mode.dmPelsWidth * mode.dmPelsHeight > width * height {
                    let mut wanted = mode;
                    wanted.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT;
                    let changed = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), Some(&wanted), None, CDS_TYPE(0), None) };
                    pump(Duration::from_secs(3));
                    resized = changed == DISP_CHANGE_SUCCESSFUL;
                    p.line(format!("- changing the display to {}x{} gave {:?}, mode now {:?}", mode.dmPelsWidth, mode.dmPelsHeight, changed.0, current_mode()));
                    if resized {
                        scaled = try_override(p, relative + 2, 144);
                    }
                }
            }
        }
        if !scaled {
            scaled = try_override(p, relative + 1, 120);
        }
        if scaled {
            scaled_sections(p);
        }
        let back = set_override(relative);
        pump(Duration::from_secs(2));
        p.line(format!("- override put back to step {relative} {back:?}, monitor DPI {}, desktop list DPI {}", primary_dpi(), list_dpi()));
        if resized {
            let reset = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None) };
            pump(Duration::from_secs(3));
            p.line(format!("- display mode reset {:?}, mode now {:?}", reset.0, current_mode()));
        }
        if scaled {
            return Ok(());
        }
        let pixels = desktop_dword(w!("LogPixels"));
        let scaling_flag = desktop_dword(w!("Win8DpiScaling"));
        p.line(format!("- LogPixels was {pixels:?} and Win8DpiScaling {scaling_flag:?}"));
        let wrote = (set_desktop_dword(w!("LogPixels"), Some(144)), set_desktop_dword(w!("Win8DpiScaling"), Some(1)));
        p.line(format!("- wrote LogPixels 144 and Win8DpiScaling 1 {wrote:?}"));
        let restarted = restart_explorer(p);
        p.line(format!("- after an Explorer restart {restarted:?}, monitor DPI {}, desktop list DPI {}, system DPI {}", primary_dpi(), list_dpi(), unsafe { GetDpiForSystem() }));
        let took = list_dpi() == 144;
        if took {
            scaled_sections(p);
        }
        let restored = (set_desktop_dword(w!("LogPixels"), pixels), set_desktop_dword(w!("Win8DpiScaling"), scaling_flag));
        p.line(format!("- put LogPixels and Win8DpiScaling back {restored:?}"));
        let again = restart_explorer(p);
        p.line(format!("- after restarting Explorer again {again:?}, desktop list DPI {}", list_dpi()));
        if !took {
            p.line("- no method changed the display scale on this runner".to_string());
        }
        Ok(())
    }

    fn screen_rect(screen: &Screen) -> RECT {
        RECT { left: screen.frame.x as i32, top: screen.frame.y as i32, right: (screen.frame.x + screen.frame.w) as i32, bottom: (screen.frame.y + screen.frame.h) as i32 }
    }

    fn mean_gap(shot: &RgbImage, target: &RgbImage) -> f32 {
        if shot.dimensions() != target.dimensions() {
            return f32::MAX;
        }
        let (width, height) = (shot.width(), shot.height());
        let mut total = 0u64;
        let mut count = 0u64;
        for y in (height / 10..height * 8 / 10).step_by(7) {
            for x in (width / 4..width * 9 / 10).step_by(7) {
                let (a, b) = (shot.get_pixel(x, y).0, target.get_pixel(x, y).0);
                total += a.iter().zip(b.iter()).map(|(p, q)| u64::from(p.abs_diff(*q))).sum::<u64>();
                count += 3;
            }
        }
        total as f32 / count.max(1) as f32
    }

    fn fade(p: &mut Probe, label: &str, area: RECT, old: &RgbImage, new: &RgbImage, set: impl FnOnce() -> Result<()>) {
        let mut samples = Vec::new();
        let started = Instant::now();
        let result = set();
        let returned = started.elapsed().as_millis();
        while started.elapsed() < Duration::from_millis(2500) {
            if let Ok(shot) = capture(area) {
                samples.push((started.elapsed().as_millis(), mean_gap(&shot, old), mean_gap(&shot, new)));
            }
        }
        let first = samples.iter().find(|(_, to_old, _)| *to_old > 3.0).map(|(at, _, _)| *at);
        let settled = samples.iter().position(|(_, _, to_new)| *to_new < 3.0).map(|index| samples[index].0);
        p.line(format!(
            "- {label} set returned {:?} after {returned} ms, screen first changed at {first:?} ms, showed the new picture at {settled:?} ms, fade {:?} ms over {} captures",
            result.map_err(|error| error.to_string()),
            settled.zip(first).map(|(settled, first)| settled.saturating_sub(first)),
            samples.len()
        ));
    }

    fn own_private() -> usize {
        let process = unsafe { GetCurrentProcess() };
        let mut counters = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
        let _ = unsafe { GetProcessMemoryInfo(process, &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS, counters.cb) };
        counters.PrivateUsage
    }

    fn process_numbers(pid: u32) -> Option<(usize, u32)> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }.ok()?;
        let mut counters = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
        let read = unsafe { GetProcessMemoryInfo(handle, &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS, counters.cb) }.is_ok();
        let mut handles = 0u32;
        let counted = unsafe { GetProcessHandleCount(handle, &mut handles) }.is_ok();
        let _ = unsafe { CloseHandle(handle) };
        (read && counted).then_some((counters.PrivateUsage, handles))
    }

    fn process_gone(pid: u32) -> bool {
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else { return true };
        let ended = unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
        let _ = unsafe { CloseHandle(handle) };
        ended
    }

    fn gone_within(pid: u32, limit: Duration) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if process_gone(pid) {
                return Some(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        process_gone(pid).then(|| started.elapsed())
    }

    fn type_names() -> BTreeMap<u16, String> {
        let mut words = vec![0u64; 1 << 13];
        for _ in 0..8 {
            let mut needed = 0u32;
            let status = unsafe { NtQueryObject(None, OBJECT_INFORMATION_CLASS(3), Some(words.as_mut_ptr() as *mut c_void), (words.len() * 8) as u32, Some(&mut needed)) };
            if status.is_ok() {
                break;
            }
            let wanted = (needed as usize / 8 + 1).max(words.len() * 2);
            words = vec![0u64; wanted];
        }
        let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        let read16 = |at: usize| bytes.get(at..at + 2).map_or(0, |pair| u16::from_le_bytes([pair[0], pair[1]]));
        let count = bytes.get(0..4).map_or(0, |four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]));
        let mut names = BTreeMap::new();
        let mut at = 8usize;
        for _ in 0..count {
            if at + 0x68 > bytes.len() {
                break;
            }
            let (length, maximum) = (read16(at) as usize, read16(at + 2) as usize);
            let index = bytes[at + 0x5a];
            let units: Vec<u16> = (0..length / 2).map(|unit| read16(at + 0x68 + unit * 2)).collect();
            names.insert(u16::from(index), String::from_utf16_lossy(&units));
            at = (at + 0x68 + maximum + 7) & !7;
        }
        names
    }

    fn handle_types(pid: u32) -> Option<BTreeMap<String, usize>> {
        let mut words = vec![0u64; 1 << 17];
        let mut read = false;
        for _ in 0..8 {
            let mut needed = 0u32;
            let status = unsafe { NtQuerySystemInformation(SYSTEM_INFORMATION_CLASS(64), words.as_mut_ptr() as *mut c_void, (words.len() * 8) as u32, &mut needed) };
            if status.is_ok() {
                read = true;
                break;
            }
            let wanted = (needed as usize / 8 + 8192).max(words.len() * 2);
            if wanted > 1 << 26 {
                break;
            }
            words = vec![0u64; wanted];
        }
        if !read {
            return None;
        }
        let names = type_names();
        let count = words[0] as usize;
        let mut kinds = BTreeMap::new();
        for entry in 0..count {
            let base = 2 + entry * 5;
            let Some(fields) = words.get(base..base + 5) else { break };
            if fields[1] as u32 == pid {
                let kind = (fields[3] >> 48) as u16;
                *kinds.entry(names.get(&kind).cloned().unwrap_or_else(|| format!("type {kind}"))).or_insert(0usize) += 1;
            }
        }
        Some(kinds)
    }

    fn icon_rects() -> Result<Vec<Rect>> {
        let view = shell::view()?;
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        Ok(automation_rects(&automation, view.list)?.into_iter().map(|(_, rect)| rect).collect())
    }

    fn settle(area: RECT, limit: Duration) -> Result<RgbImage> {
        let started = Instant::now();
        let bottom = work_area().map_or(area.bottom, |work| work.bottom.min(area.bottom)) - area.top;
        let mut last = capture(area)?;
        while started.elapsed() < limit {
            pump(Duration::from_millis(300));
            let shot = capture(area)?;
            let rows = (bottom.max(0) as usize * shot.width() as usize * 3).min(shot.as_raw().len());
            let still = shot.dimensions() == last.dimensions() && shot.as_raw()[..rows] == last.as_raw()[..rows];
            last = shot;
            if still {
                break;
            }
        }
        Ok(last)
    }

    fn shell_process() -> u32 {
        let shell = unsafe { GetShellWindow() };
        let mut process = 0u32;
        if !shell.is_invalid() {
            unsafe { GetWindowThreadProcessId(shell, Some(&mut process)) };
        }
        process
    }

    fn press(view: &shell::View, pidl: &com::Pidl, at: POINT) -> (bool, Option<u128>) {
        let selected = || unsafe { view.folder.GetSelectionState(pidl.0) }.is_ok_and(|state| state & SVSI_SELECT.0 as u32 != 0);
        let mut sent = mouse(at.x, at.y, MOUSE_EVENT_FLAGS(0));
        pump(Duration::from_millis(100));
        let pressed = Instant::now();
        sent &= mouse(at.x, at.y, MOUSEEVENTF_LEFTDOWN);
        let (mut took, mut up) = (None, false);
        while pressed.elapsed() < Duration::from_secs(1) {
            if selected() {
                took = Some(pressed.elapsed().as_millis());
                break;
            }
            if !up && pressed.elapsed() >= Duration::from_millis(80) {
                sent &= mouse(at.x, at.y, MOUSEEVENTF_LEFTUP);
                up = true;
            }
            pump(Duration::from_millis(4));
        }
        if !up {
            sent &= mouse(at.x, at.y, MOUSEEVENTF_LEFTUP);
        }
        (sent, took)
    }

    const ANSWER: Duration = Duration::from_secs(3);
    const GLIDE_MS: u32 = 450;

    fn bare() -> Hints {
        Hints { present: Some(Method::Bare.hint().into()), scale: None }
    }

    struct Kit {
        picture: PathBuf,
        size: [u32; 2],
        pair: (scooch_core::Bundle, scooch_core::Bundle),
        canvas: scooch_core::Canvas,
        stills: Vec<(PathBuf, RgbImage)>,
        tables: Vec<PathBuf>,
        frames: Vec<(f32, RgbImage)>,
        mirrored_frames: Vec<(f32, RgbImage)>,
        scenic_frames: Vec<(f32, RgbImage)>,
        other: (PathBuf, RgbImage),
        detail: (PathBuf, RgbImage),
        icons: Vec<Rect>,
        subject: [(f32, f32); 2],
        patch: RECT,
        spot: RECT,
    }

    fn save_png(path: &Path, image: &RgbImage) -> Result<()> {
        fs::write(path, scooch_core::encode_png(image)?)?;
        Ok(())
    }

    fn subject_at(bundle: &scooch_core::Bundle, analysis: &scooch_core::Analysis) -> Option<(f32, f32)> {
        let s = analysis.subject;
        let (u, v) = (f64::from(s.x + s.w / 2.0), f64::from(s.y + s.h / 2.0));
        let nearest = |map: &[f64], value: f64| map.iter().enumerate().min_by(|x, y| (x.1 - value).abs().total_cmp(&(y.1 - value).abs())).map(|(index, found)| (index, (found - value).abs()));
        let (x, dx) = nearest(&bundle.xmap, u)?;
        let (y, dy) = nearest(&bundle.ymap, v)?;
        (dx < 0.01 && dy < 0.01).then_some((x as f32 + 0.5, y as f32 + 0.5))
    }

    fn inside(icons: &[Rect], x: f32, y: f32, margin: f32) -> bool {
        icons.iter().any(|rect| x >= rect.x - margin && x < rect.x + rect.w + margin && y >= rect.y - margin && y < rect.y + rect.h + margin)
    }

    fn block_gap(a: &RgbImage, b: &RgbImage, left: u32, top: u32, size: u32) -> f32 {
        let mut total = 0u64;
        for y in top..(top + size).min(a.height()) {
            for x in left..(left + size).min(a.width()) {
                let (p, q) = (a.get_pixel(x, y).0, b.get_pixel(x, y).0);
                total += p.iter().zip(q.iter()).map(|(m, n)| u64::from(m.abs_diff(*n))).sum::<u64>();
            }
        }
        total as f32 / (size * size * 3) as f32
    }

    fn best_block(a: &RgbImage, b: &RgbImage, icons: &[Rect], size: u32) -> (u32, u32, f32) {
        let mut best = (a.width() / 2, a.height() / 2, -1.0f32);
        for top in (0..a.height().saturating_sub(size)).step_by(size as usize) {
            for left in (0..a.width().saturating_sub(size + 40)).step_by(size as usize) {
                let corners = [(left, top), (left + size, top), (left, top + size), (left + size, top + size)];
                if corners.iter().any(|(x, y)| inside(icons, *x as f32, *y as f32, 8.0)) {
                    continue;
                }
                let gap = block_gap(a, b, left, top, size);
                if gap > best.2 {
                    best = (left, top, gap);
                }
            }
        }
        best
    }

    struct Found {
        scene: Scene,
        placement: scooch_core::Placement,
        at: (f32, f32),
        moved: f32,
        tries: usize,
    }

    fn moving_pair(analysis: &scooch_core::Analysis, first: &Scene, placed: &scooch_core::Placement, from: (f32, f32), options: &Options, image: &RgbImage) -> Option<Found> {
        let mut order: Vec<usize> = (0..first.icons.len()).collect();
        let centre = |rect: &Rect| (rect.x + rect.w / 2.0 - from.0).hypot(rect.y + rect.h / 2.0 - from.1);
        order.sort_by(|x, y| centre(&first.icons[*x]).total_cmp(&centre(&first.icons[*y])));
        let offsets = [(0.0, 0.0), (-90.0, 0.0), (90.0, 0.0), (0.0, -90.0), (0.0, 90.0), (-160.0, -60.0), (160.0, 60.0)];
        let mut best: Option<Found> = None;
        let mut tries = 0usize;
        for index in order.into_iter().take(6) {
            for (dx, dy) in offsets {
                tries += 1;
                let mut icons = first.icons.clone();
                let rect = icons[index];
                icons[index].x = (from.0 + dx - rect.w / 2.0).clamp(0.0, first.width as f32 - rect.w);
                icons[index].y = (from.1 + dy - rect.h / 2.0).clamp(0.0, first.height as f32 - rect.h);
                let scene = Scene { width: first.width, height: first.height, icons };
                let placement = scooch_core::solve(analysis, &scene, options, Some(placed));
                let bundle = scooch_core::bundle(image, analysis, &placement, &scene, options);
                let Some(at) = subject_at(&bundle, analysis) else { continue };
                let moved = (at.0 - from.0).hypot(at.1 - from.1);
                if best.as_ref().is_none_or(|found| moved > found.moved) {
                    best = Some(Found { scene, placement, at, moved, tries });
                }
                if moved >= 80.0 {
                    return best.map(|found| Found { tries, ..found });
                }
                if tries >= 30 {
                    return best.map(|found| Found { tries, ..found });
                }
            }
        }
        best.map(|found| Found { tries, ..found })
    }

    fn kit(p: &mut Probe, screen: &Screen) -> Result<Kit> {
        let (width, height) = (screen.frame.w as u32, screen.frame.h as u32);
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/photos/balloon-sky.jpg");
        let picture_path = p.home.join("glide-picture.jpg");
        fs::copy(&source, &picture_path).with_context(|| format!("no test photo at {}", source.display()))?;
        let image = picture::decode(&picture_path)?;
        let analysis = scooch_core::analyze(&image);
        let options = Options { scenery: Scenery::Auto, ..Options::default() };
        let icons: Vec<Rect> = icon_rects()?.into_iter().map(|rect| Rect { x: rect.x - screen.frame.x, y: rect.y - screen.frame.y, ..rect }).collect();
        let first = Scene { width, height, icons: icons.clone() };
        let placed_a = scooch_core::solve(&analysis, &first, &options, None);
        let a = scooch_core::bundle(&image, &analysis, &placed_a, &first, &options);
        let from = subject_at(&a, &analysis).context("the subject is not on screen in A")?;
        let found = moving_pair(&analysis, &first, &placed_a, from, &options, &image).context("no icon move gave a placement")?;
        p.line(format!("- searched {} icon moves, the subject goes from {:.0},{:.0} to {:.0},{:.0}, {:.0} px", found.tries, from.0, from.1, found.at.0, found.at.1, found.moved));
        if found.moved < 80.0 {
            p.failed.push(format!("no icon move made the subject travel 80 px, the best was {:.0} px", found.moved));
        }
        let centre = |rect: &Rect| (rect.x + rect.w / 2.0 - width as f32 / 2.0).hypot(rect.y + rect.h / 2.0 - height as f32 / 2.0);
        let index = icons.iter().enumerate().min_by(|(_, x), (_, y)| centre(x).total_cmp(&centre(y))).map(|(index, _)| index).context("no icons to move")?;
        let mut scenic_icons = icons.clone();
        let shift = if scenic_icons[index].x > width as f32 / 2.0 { -320.0 } else { 320.0 };
        scenic_icons[index].x = (scenic_icons[index].x + shift).clamp(0.0, width as f32 - scenic_icons[index].w);
        let scenic_scene = Scene { width, height, icons: scenic_icons };
        let placed_s = scooch_core::solve(&analysis, &scenic_scene, &options, Some(&placed_a));
        let mut placed_m = placed_a.clone();
        placed_m.fx.iter_mut().for_each(|value| *value = 1.0 - *value);
        placed_m.mirrored = !placed_a.mirrored;
        let b = scooch_core::bundle(&image, &analysis, &found.placement, &found.scene, &options);
        let mirrored = scooch_core::bundle(&image, &analysis, &placed_m, &first, &options);
        let scenic = scooch_core::bundle(&image, &analysis, &placed_s, &scenic_scene, &options);
        let mut stills = Vec::new();
        let mut tables = Vec::new();
        for (name, placement, scene, bundle) in [("a", &placed_a, &first, &a), ("b", &found.placement, &found.scene, &b), ("mirrored", &placed_m, &first, &mirrored), ("scenic", &placed_s, &scenic_scene, &scenic)] {
            let still = scooch_core::render_with(&image, &analysis, placement, scene, &options);
            let path = p.home.join(format!("glide-{name}.png"));
            save_png(&path, &still)?;
            stills.push((path, still));
            let table = p.home.join(format!("glide-{name}.tables"));
            fs::write(&table, bundle.tables_to_bytes())?;
            tables.push(table);
        }
        let frames: Vec<(f32, RgbImage)> = (0..=40).map(|step| step as f32 / 40.0).map(|s| (s, scooch_core::frame(&a, &b, s))).collect();
        let mirrored_frames: Vec<(f32, RgbImage)> = (0..=10).map(|step| step as f32 / 10.0).map(|s| (s, scooch_core::frame(&a, &mirrored, s))).collect();
        let scenic_frames: Vec<(f32, RgbImage)> = (0..=10).map(|step| step as f32 / 10.0).map(|s| (s, scooch_core::frame(&a, &scenic, s))).collect();
        let other_image = subject_picture(width, height);
        let other_path = p.home.join("glide-other.png");
        save_png(&other_path, &other_image)?;
        let detail_image = detailed(width, height);
        let detail_path = p.home.join("glide-detail.png");
        save_png(&detail_path, &detail_image)?;
        let (left, top, moving) = best_block(&frames[0].1, &frames[6].1, &icons, 24);
        let (spot_left, spot_top, apart) = best_block(&stills[0].1, &other_image, &icons, 16);
        let origin = (screen.frame.x as i32, screen.frame.y as i32);
        p.line(format!(
            "- test photo {} decoded to {}x{}, compatible {}, mirrored compatible {}, scenery pair compatible {}, canvas {}x{}, tables {} bytes each, motion patch at {left},{top} differs by {moving:.1}, other still patch at {spot_left},{spot_top} differs by {apart:.1}",
            source.display(),
            image.width(),
            image.height(),
            scooch_core::compatible(&a, &b),
            scooch_core::compatible(&a, &mirrored),
            scooch_core::compatible(&a, &scenic),
            a.canvas_width,
            a.canvas_height,
            fs::metadata(&tables[0]).map(|meta| meta.len()).unwrap_or(0)
        ));
        let canvas = scooch_core::canvas(&image, width, height);
        Ok(Kit {
            picture: picture_path,
            size: [width, height],
            subject: [from, found.at],
            pair: (a, b),
            canvas,
            stills,
            tables,
            frames,
            mirrored_frames,
            scenic_frames,
            other: (other_path, other_image),
            detail: (detail_path, detail_image),
            icons,
            patch: RECT { left: origin.0 + left as i32, top: origin.1 + top as i32, right: origin.0 + (left + 24) as i32, bottom: origin.1 + (top + 24) as i32 },
            spot: RECT { left: origin.0 + spot_left as i32, top: origin.1 + spot_top as i32, right: origin.0 + (spot_left + 16) as i32, bottom: origin.1 + (spot_top + 16) as i32 },
        })
    }

    fn outside(width: u32, height: u32, icons: &[Rect], step: u32) -> Vec<usize> {
        let bottom = work_area().map_or(height, |area| (area.bottom.max(0) as u32).min(height));
        let mut keep = Vec::new();
        for y in (0..bottom).step_by(step as usize) {
            for x in (0..width.saturating_sub(40)).step_by(step as usize) {
                if !inside(icons, x as f32, y as f32, 8.0) {
                    keep.push((y * width + x) as usize);
                }
            }
        }
        keep
    }

    fn difference(a: &RgbImage, b: &RgbImage, keep: &[usize]) -> (f32, u8, u8) {
        if a.dimensions() != b.dimensions() {
            return (f32::MAX, 255, 255);
        }
        let (x, y) = (a.as_raw(), b.as_raw());
        let mut spread = [0usize; 256];
        let mut total = 0u64;
        for index in keep {
            let at = index * 3;
            let gaps = [x[at].abs_diff(y[at]), x[at + 1].abs_diff(y[at + 1]), x[at + 2].abs_diff(y[at + 2])];
            total += gaps.iter().map(|gap| u64::from(*gap)).sum::<u64>();
            spread[*gaps.iter().max().unwrap_or(&0) as usize] += 1;
        }
        let count = keep.len().max(1);
        let mut seen = 0usize;
        let high = spread.iter().position(|n| {
            seen += n;
            seen * 100 >= count * 99
        });
        (total as f32 / (count * 3) as f32, high.unwrap_or(0) as u8, spread.iter().rposition(|n| *n > 0).unwrap_or(0) as u8)
    }

    fn strong(baseline: &RgbImage, still: &RgbImage, icons: &[Rect]) -> Vec<Vec<usize>> {
        let width = baseline.width();
        let mut mask = vec![Vec::new(); icons.len()];
        for (index, (p, q)) in baseline.as_raw().as_chunks::<3>().0.iter().zip(still.as_raw().as_chunks::<3>().0.iter()).enumerate() {
            if !(0..3).any(|c| p[c].abs_diff(q[c]) > 60) {
                continue;
            }
            let (x, y) = (index as u32 % width, index as u32 / width);
            if let Some(icon) = icons.iter().position(|rect| inside(std::slice::from_ref(rect), x as f32, y as f32, 0.0)) {
                mask[icon].push(index);
            }
        }
        mask
    }

    fn total(mask: &[Vec<usize>]) -> usize {
        mask.iter().map(Vec::len).sum()
    }

    fn covered(shot: &RgbImage, baseline: &RgbImage, expected: &[&RgbImage], mask: &[Vec<usize>]) -> (usize, usize) {
        let (s, b) = (shot.as_raw(), baseline.as_raw());
        let (mut hidden, mut redrawn) = (0usize, 0usize);
        for icon in mask {
            let (mut gone, mut changed, mut kept) = (0usize, 0usize, 0usize);
            for index in icon {
                let at = index * 3;
                if !(0..3).any(|c| s[at + c].abs_diff(b[at + c]) > 40) {
                    kept += 1;
                } else if expected.iter().any(|image| {
                    let e = image.as_raw();
                    (0..3).all(|c| e.get(at + c).is_some_and(|value| s[at + c].abs_diff(*value) <= 16))
                }) {
                    gone += 1;
                } else {
                    changed += 1;
                }
            }
            redrawn += changed;
            if (kept + changed) * 4 >= icon.len() {
                redrawn += gone;
            } else {
                hidden += gone;
            }
        }
        (hidden, redrawn)
    }

    fn best_frame(shot: &RgbImage, frames: &[(f32, RgbImage)], keep: &[usize]) -> (f32, f32) {
        frames.iter().map(|(s, frame)| (*s, difference(shot, frame, keep).0)).min_by(|x, y| x.1.total_cmp(&y.1)).unwrap_or((0.0, f32::MAX))
    }

    fn tracked(shot: &RgbImage, template: &RgbImage, from: (f32, f32), to: (f32, f32)) -> f32 {
        let size = template.width() as i32;
        let steps = 48;
        let mut best = (0.0f32, f64::MAX);
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let (cx, cy) = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            let (left, top) = (cx as i32 - size / 2, cy as i32 - size / 2);
            if left < 0 || top < 0 || left + size > shot.width() as i32 || top + size > shot.height() as i32 {
                continue;
            }
            let mut total = 0u64;
            for y in 0..size {
                for x in 0..size {
                    let (p, q) = (shot.get_pixel((left + x) as u32, (top + y) as u32).0, template.get_pixel(x as u32, y as u32).0);
                    total += p.iter().zip(q.iter()).map(|(m, n)| u64::from(m.abs_diff(*n))).sum::<u64>();
                }
            }
            if (total as f64) < best.1 {
                best = (t, total as f64);
            }
        }
        best.0
    }

    fn template(still: &RgbImage, at: (f32, f32), size: u32) -> RgbImage {
        let left = (at.0 as i32 - size as i32 / 2).clamp(0, still.width() as i32 - size as i32) as u32;
        let top = (at.1 as i32 - size as i32 / 2).clamp(0, still.height() as i32 - size as i32) as u32;
        RgbImage::from_fn(size, size, |x, y| *still.get_pixel(left + x, top + y))
    }

    fn strip(shots: &[(u128, RgbImage)], from: u128, to: u128) -> Option<RgbImage> {
        let picked: Vec<&RgbImage> = (0..9)
            .filter_map(|k| {
                let at = from + (to.saturating_sub(from)) * k / 8;
                shots.iter().min_by_key(|(t, _)| t.abs_diff(at)).map(|(_, shot)| shot)
            })
            .collect();
        let first = picked.first()?;
        let (w, h) = (first.width() / 4, first.height() / 4);
        let mut out = RgbImage::new(w * 9, h);
        for (k, shot) in picked.iter().enumerate() {
            for y in 0..h {
                for x in 0..w {
                    out.put_pixel(k as u32 * w + x, y, *shot.get_pixel(x * 4, y * 4));
                }
            }
        }
        Some(out)
    }

    fn shows(area: RECT, still: &RgbImage, keep: &[usize], limit: Duration) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if capture(area).is_ok_and(|shot| difference(&shot, still, keep).0 < 1.5) {
                return Some(started.elapsed());
            }
            pump(Duration::from_millis(30));
        }
        None
    }

    fn patch_of(image: &RgbImage, area: RECT, patch: RECT) -> RgbImage {
        let (left, top) = ((patch.left - area.left) as u32, (patch.top - area.top) as u32);
        let (w, h) = ((patch.right - patch.left) as u32, (patch.bottom - patch.top) as u32);
        RgbImage::from_fn(w, h, |x, y| *image.get_pixel((left + x).min(image.width() - 1), (top + y).min(image.height() - 1)))
    }

    fn patch_until(patch: RECT, wanted: &RgbImage, close: bool, limit: Duration) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < limit {
            if let Ok(shot) = capture(patch) {
                let gap = mean_gap_all(&shot, wanted);
                if (gap < 6.0) == close {
                    return Some(started.elapsed());
                }
            }
            std::thread::yield_now();
        }
        None
    }

    fn mean_gap_all(a: &RgbImage, b: &RgbImage) -> f32 {
        if a.dimensions() != b.dimensions() {
            return f32::MAX;
        }
        let total: u64 = a.as_raw().iter().zip(b.as_raw()).map(|(p, q)| u64::from(p.abs_diff(*q))).sum();
        total as f32 / a.as_raw().len().max(1) as f32
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Ending {
        During,
        AtOnce,
        Never,
    }

    struct Pass<'a> {
        label: &'a str,
        name: &'a str,
        tables: (&'a Path, &'a Path),
        track: bool,
        old: &'a (PathBuf, RgbImage),
        new: &'a (PathBuf, RgbImage),
        frames: &'a [(f32, RgbImage)],
        ending: Ending,
        ms: u32,
        strip: bool,
        scale: Option<f32>,
    }

    const HOLD: u128 = 700;

    fn region(old: &RgbImage, new: &RgbImage, keep: &[usize]) -> Vec<usize> {
        if old.dimensions() != new.dimensions() {
            return Vec::new();
        }
        let (x, y) = (old.as_raw(), new.as_raw());
        keep.iter().copied().filter(|index| (0..3).any(|c| x[index * 3 + c].abs_diff(y[index * 3 + c]) > 24)).collect()
    }

    fn region_gap(shot: &RgbImage, still: &RgbImage, region: &[usize]) -> f32 {
        if region.is_empty() || shot.dimensions() != still.dimensions() {
            return 0.0;
        }
        let (x, y) = (shot.as_raw(), still.as_raw());
        let total: u64 = region.iter().map(|index| (0..3).map(|c| u64::from(x[index * 3 + c].abs_diff(y[index * 3 + c]))).sum::<u64>()).sum();
        total as f32 / (region.len() * 3) as f32
    }

    fn side(shot: &RgbImage, old: &RgbImage, new: &RgbImage, region: &[usize]) -> char {
        if region.is_empty() {
            return '=';
        }
        let (a, b) = (region_gap(shot, old, region), region_gap(shot, new, region));
        if a < 3.0 && a * 3.0 < b {
            'A'
        } else if b < 3.0 && b * 3.0 < a {
            'B'
        } else {
            '~'
        }
    }

    fn equal_until(area: RECT, still: &RgbImage, other: &RgbImage, keep: &[usize], limit: Duration) -> (Option<Duration>, (f32, u8, u8), Option<RgbImage>) {
        let started = Instant::now();
        let apart = region(still, other, keep);
        let mut last = (f32::MAX, 255, 255);
        let mut shot = None;
        while started.elapsed() < limit {
            if let Ok(taken) = capture(area) {
                last = difference(&taken, still, keep);
                let near = region_gap(&taken, still, &apart) < 2.0;
                shot = Some(taken);
                if last.1 <= 2 && near {
                    return (Some(started.elapsed()), last, None);
                }
            }
            pump(Duration::from_millis(15));
        }
        (None, last, shot)
    }

    fn evidence(p: &mut Probe, name: &str, shot: &RgbImage, reference: &RgbImage, keep: &[usize], marks: &[(&str, Option<i128>)]) -> String {
        let width = shot.width() as usize;
        let (s, r) = (shot.as_raw(), reference.as_raw());
        let mut shades = vec![0u8; s.len()];
        let (mut count, mut left, mut top, mut right, mut bottom, mut sx, mut sy) = (0usize, usize::MAX, usize::MAX, 0usize, 0usize, 0f64, 0f64);
        for index in keep {
            let at = index * 3;
            let gap = (0..3).map(|c| s[at + c].abs_diff(r[at + c])).max().unwrap_or(0);
            let (x, y) = (index % width, index / width);
            shades[at..at + 3].fill(gap.saturating_mul(4));
            if gap > 8 {
                count += 1;
                (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
                sx += x as f64;
                sy += y as f64;
            }
        }
        let _ = save_png(&p.out.join(format!("{name}-capture.png")), shot);
        let _ = save_png(&p.out.join(format!("{name}-still.png")), reference);
        if let Some(diff) = RgbImage::from_raw(shot.width(), shot.height(), shades) {
            let _ = save_png(&p.out.join(format!("{name}-diff.png")), &diff);
        }
        let when: Vec<String> = marks.iter().map(|(label, at)| format!("{label} {}", at.map_or("never".to_string(), |at| format!("{at:+} ms")))).collect();
        if count == 0 {
            return format!("saved {name}-capture.png, no pixel off by more than 8, taken {}", when.join(", "));
        }
        format!(
            "saved {name}-capture.png, {name}-still.png and {name}-diff.png, {count} pixels off by more than 8 between {left},{top} and {right},{bottom}, centred at {:.0},{:.0}, taken {}",
            sx / count as f64,
            sy / count as f64,
            when.join(", ")
        )
    }

    fn glide_run(p: &mut Probe, helper: &mut Helper, id: u32, screen: &Screen, kit: &Kit, run: &Pass) -> Result<()> {
        let desktop = platform::current()?;
        let area = screen_rect(screen);
        let (width, height) = (kit.size[0], kit.size[1]);
        let keep = outside(width, height, &kit.icons, 2);
        let wide = outside(width, height, &kit.icons, 1);
        p.line(format!("- **{}**, {} ms, scale {}", run.label, run.ms, run.scale.map_or("chosen by the helper".to_string(), |scale| format!("pinned at {scale}"))));
        helper.warm(Hints { scale: run.scale, ..bare() }, None, None, ANSWER)?;
        let asked_old = Instant::now();
        desktop.set_wallpaper(screen, &run.old.0)?;
        let (landed, landed_gap, missed) = equal_until(area, &run.old.1, &run.new.1, &wide, Duration::from_secs(6));
        park_cursor();
        let baseline = settle(area, Duration::from_secs(3))?;
        let mask = strong(&baseline, &run.old.1, &kit.icons);
        let system = difference(&baseline, &run.old.1, &wide);
        p.line(format!(
            "  - the old still was set and a capture equalled it after {:?} ms of {} ms waited (last gap mean {:.2} 99th {} largest {}), the system against the file mean {:.2} 99th {} largest {}",
            landed.map(|d| d.as_millis()),
            asked_old.elapsed().as_millis(),
            landed_gap.0,
            landed_gap.1,
            landed_gap.2,
            system.0,
            system.1,
            system.2
        ));
        if landed.is_none() {
            p.failed.push(format!("{}: the old still never showed exactly before open", run.label));
            if let Some(shot) = missed {
                let told = evidence(p, &format!("{}-before", run.name), &shot, &run.old.1, &wide, &[]);
                p.line(format!("  - {told}"));
            }
        }
        let asked = Instant::now();
        helper.ask_open(id, screen, &run.old.0)?;
        let mut opening: Vec<(u128, RgbImage)> = Vec::new();
        let mut shown: Option<(u64, u128)> = None;
        while asked.elapsed() < Duration::from_secs(4) {
            if let Ok(shot) = capture(area) {
                opening.push((asked.elapsed().as_millis(), shot));
            }
            match shown {
                Some((_, at)) if asked.elapsed().as_millis() > at + 150 => break,
                Some(_) => {}
                None => match helper.next(Duration::from_millis(1)) {
                    Some(Reply::Shown { id: seen, ms }) if seen == id => shown = Some((ms, asked.elapsed().as_millis())),
                    Some(Reply::Failed { why, .. }) => bail!("the helper could not open the layer {why}"),
                    _ => {}
                },
            }
        }
        let (shown_ms, shown_at) = shown.context("the helper never said the layer was shown")?;
        let opened: Vec<(f32, u8, u8)> = opening.iter().map(|(_, shot)| difference(shot, &baseline, &wide)).collect();
        let worst_open = opened.iter().enumerate().max_by_key(|(_, d)| d.1).map(|(index, d)| (index, *d));
        let mut line = format!("  - open answered shown after {shown_ms} ms by the helper and {shown_at} ms by the probe, {} captures during the open against the plain desktop", opening.len());
        if let Some((index, d)) = worst_open {
            line += &format!(": worst mean {:.2} 99th {} largest {}", d.0, d.1, d.2);
            if d.1 > 2 {
                let at = opening[index].0 as i128;
                line += &format!(", {}", evidence(p, &format!("{}-open", run.name), &opening[index].1, &baseline, &wide, &[("after open was sent", Some(at)), ("after shown", Some(at - shown_at as i128))]));
            }
            if d.1 > 8 {
                p.failed.push(format!("{}: opening the layer changed the screen", run.label));
            }
        }
        p.line(line);
        let started = Instant::now();
        helper.ask_glide(id, run.tables.0, run.tables.1, run.ms)?;
        if run.ending == Ending::AtOnce {
            helper.ask_settle(id, &run.new.0, None)?;
        }
        let mut shots: Vec<(u128, RgbImage)> = Vec::new();
        let (mut set_at, mut glided, mut settled, mut failed, mut closed_at, mut full_at) = (None, None, None, None, None, None);
        let limit = Duration::from_millis(u64::from(run.ms)) + if run.ending == Ending::Never { Duration::from_secs(9) } else { Duration::from_secs(6) };
        while started.elapsed() < limit {
            let at = started.elapsed().as_millis();
            if set_at.is_none() && at >= 150 {
                let _ = desktop.set_wallpaper(screen, &run.new.0);
                set_at = Some(started.elapsed().as_millis());
                if run.ending == Ending::During {
                    helper.ask_settle(id, &run.new.0, None)?;
                }
            }
            if let Ok(shot) = capture(area) {
                if settled.is_some() && full_at.is_none() {
                    let d = difference(&shot, &run.new.1, &wide);
                    if d.1 <= 2 && d.0 < 0.3 {
                        full_at = Some(at);
                    }
                }
                shots.push((at, shot));
            }
            while let Some(reply) = helper.next(Duration::ZERO) {
                let now = started.elapsed().as_millis();
                match reply {
                    Reply::Glided { id: seen, frames, slow_frames, first_frame_ms, uploads, scale } if seen == id => glided = Some((now, frames, slow_frames, first_frame_ms, uploads, scale)),
                    Reply::Settled { id: seen } if seen == id => settled = Some(now),
                    Reply::Failed { why, .. } => failed = Some((now, why)),
                    _ => {}
                }
            }
            if closed_at.is_none() {
                let ready = match run.ending {
                    Ending::Never => failed.as_ref().map(|(at, _)| *at),
                    _ => settled.zip(set_at).map(|(done, set)| done.max(set + HOLD)),
                };
                if ready.is_some_and(|ready| started.elapsed().as_millis() >= ready) {
                    if run.ending != Ending::Never {
                        helper.close(id, ANSWER)?;
                    }
                    closed_at = Some(started.elapsed().as_millis());
                }
            }
            if closed_at.is_some_and(|closed| started.elapsed().as_millis() >= closed + 1000) {
                break;
            }
            drain();
        }
        if closed_at.is_none() {
            let _ = helper.close(id, ANSWER);
        }
        pump(Duration::from_millis(300));
        let after = capture(area)?;
        let left = layer::ours_left();
        let marks = |at: u128| -> Vec<(&'static str, Option<i128>)> {
            vec![
                ("from the wallpaper set", set_at.map(|set| at as i128 - set as i128)),
                ("from settled", settled.map(|done| at as i128 - done as i128)),
                ("from close", closed_at.map(|closed| at as i128 - closed as i128)),
            ]
        };
        let glide_end = glided.map(|g| g.0).unwrap_or(u128::from(run.ms) + 200);
        let moving: Vec<&(u128, RgbImage)> = shots.iter().filter(|(at, _)| *at <= glide_end).collect();
        let matches: Vec<(u128, f32, f32)> = moving
            .iter()
            .map(|(at, shot)| {
                let (s, gap) = best_frame(shot, run.frames, &keep);
                (*at, s, gap)
            })
            .collect();
        let lead = 80 + glided.map_or(0, |g| u128::from(g.3));
        let worst_after_lead = matches.iter().filter(|(at, ..)| *at > lead).map(|m| m.2).fold(0.0f32, f32::max);
        let step = 1.0 / (run.frames.len().max(2) - 1) as f32;
        let backwards = matches.windows(2).filter(|pair| pair[1].1 + step * 1.01 < pair[0].1).count();
        let distinct = {
            let mut seen: Vec<i32> = matches.iter().map(|m| (m.1 / step).round() as i32).collect();
            seen.dedup();
            seen.len()
        };
        let mut progress = Vec::new();
        if run.track && run.ending != Ending::Never {
            let pattern = template(&run.old.1, kit.subject[0], 40);
            for (_, shot) in &moving {
                progress.push(tracked(shot, &pattern, kit.subject[0], kit.subject[1]));
            }
        }
        let slipped = progress.windows(2).filter(|pair| pair[1] + 0.03 < pair[0]).count();
        let places = {
            let mut seen: Vec<i32> = progress.iter().map(|t| (t * 48.0).round() as i32).collect();
            seen.dedup();
            seen.len()
        };
        let mut worst_cover = (0usize, 0usize, 0usize);
        for (index, (_, shot)) in shots.iter().enumerate() {
            let (s, _) = best_frame(shot, run.frames, &keep);
            let nearest = run.frames.iter().min_by(|x, y| (x.0 - s).abs().total_cmp(&(y.0 - s).abs())).map(|(_, frame)| frame);
            let mut expected = vec![&run.old.1, &run.new.1];
            expected.extend(nearest);
            let (hidden, redrawn) = covered(shot, &baseline, &expected, &mask);
            if hidden > worst_cover.0 {
                worst_cover.0 = hidden;
                worst_cover.2 = index;
            }
            worst_cover.1 = worst_cover.1.max(redrawn);
        }
        let from_settle = settled.or(failed.as_ref().map(|f| f.0)).unwrap_or(u128::MAX);
        let held: Vec<(usize, (f32, u8, u8))> = shots.iter().enumerate().filter(|(_, (at, _))| *at >= from_settle).map(|(index, (_, shot))| (index, difference(shot, &run.new.1, &wide))).collect();
        let after_gap = difference(&after, &run.new.1, &wide);
        let worst_held = held.iter().max_by_key(|(_, d)| d.1).copied();
        let first_after_settled = held.first().map(|(_, d)| *d);
        let apart = region(&run.old.1, &run.new.1, &keep);
        let sides: Vec<char> = shots.iter().map(|(_, shot)| side(shot, &run.old.1, &run.new.1, &apart)).collect();
        let appeared = sides.iter().position(|class| *class == 'B');
        let returned: Vec<usize> = appeared.map_or(Vec::new(), |first| (first..sides.len()).filter(|index| sides[*index] == 'A').collect());
        let after_close: String = shots.iter().zip(&sides).filter(|((at, _), _)| closed_at.is_some_and(|closed| *at >= closed)).map(|(_, class)| *class).collect();
        p.line(format!(
            "  - glided {:?}, settled at {settled:?} ms, first capture equal to the new still at {full_at:?} ms, failure {failed:?}, new wallpaper set at {set_at:?} ms, closed at {closed_at:?} ms, {} captures at {:.1} per second",
            glided.map(|(at, frames, slow, first, uploads, scale)| format!("at {at} ms with {frames} frames, {slow} slow, first moving frame {first} ms after glide was sent, pictures uploaded {uploads}, drawn at scale {scale}")),
            shots.len(),
            shots.len() as f32 / started.elapsed().as_secs_f32().max(0.001)
        ));
        p.line(format!(
            "  - {} captures during the motion matched to the CPU frame, {distinct} distinct s, worst mean {:.2}, worst after the {lead} ms lead in {worst_after_lead:.2}, steps back in s {backwards}: {}",
            matches.len(),
            matches.iter().map(|m| m.2).fold(0.0f32, f32::max),
            matches.iter().map(|(at, s, gap)| format!("{at} {s:.3} {gap:.1}")).collect::<Vec<_>>().join(", ")
        ));
        if !progress.is_empty() {
            p.line(format!("  - subject progress from A to B in {places} distinct places, steps back {slipped}: {:?}", progress.iter().map(|t| (t * 100.0).round() / 100.0).collect::<Vec<_>>()));
        }
        let mut line = format!("  - icon pixels covered by the layer in the worst capture {} of {}, icon pixels the desktop drew differently but not covered {}", worst_cover.0, total(&mask), worst_cover.1);
        if worst_cover.0 > 0 {
            let (at, shot) = &shots[worst_cover.2];
            line += &format!(", {}", evidence(p, &format!("{}-covered", run.name), shot, &baseline, &(0..(width * height) as usize).collect::<Vec<_>>(), &marks(*at)));
        }
        p.line(line);
        let mut line = format!(
            "  - against the new still: first capture after settled {:?}, after close {:.2} 99th {} largest {}",
            first_after_settled.map(|d| format!("mean {:.2} 99th {} largest {}", d.0, d.1, d.2)),
            after_gap.0,
            after_gap.1,
            after_gap.2
        );
        if let Some((index, d)) = worst_held {
            line += &format!(", worst from settled to a second after close mean {:.2} 99th {} largest {}", d.0, d.1, d.2);
            if d.1 > 2 {
                let (at, shot) = &shots[index];
                line += &format!(", {}", evidence(p, &format!("{}-handover", run.name), shot, &run.new.1, &wide, &marks(*at)));
            }
            if d.1 > 8 {
                p.failed.push(format!("{}: the hand over to the new wallpaper showed a difference", run.label));
            }
        }
        p.line(line);
        let mut line = format!("  - {} pixels differ between the two stills, old picture seen again after the new one {} times, captures after close {after_close}, windows of ours left {left}", apart.len(), returned.len());
        if let Some(index) = returned.first() {
            let (at, shot) = &shots[*index];
            line += &format!(", {}", evidence(p, &format!("{}-returned", run.name), shot, &run.new.1, &keep, &marks(*at)));
            p.failed.push(format!("{}: the old picture came back after the new one", run.label));
        }
        p.line(line);
        if run.strip {
            if let Some(image) = strip(&shots, 0, glide_end) {
                let _ = save_png(&p.out.join(format!("{}-strip.png", run.name)), &image);
            }
        }
        if worst_cover.0 * 100 > total(&mask).max(1) {
            p.failed.push(format!("{}: icons were covered in a capture", run.label));
        }
        if backwards > 0 || slipped > 0 {
            p.failed.push(format!("{}: the motion stepped back", run.label));
        }
        if run.track && run.ending != Ending::Never && run.ms >= 1000 && places < 8 {
            p.failed.push(format!("{}: the subject was seen in only {places} places", run.label));
        }
        if left > 0 {
            p.failed.push(format!("{}: a layer window was left", run.label));
        }
        if glided.is_none() {
            p.failed.push(format!("{}: the helper never answered glided", run.label));
        }
        if run.ending == Ending::Never && failed.is_none() {
            p.failed.push(format!("{}: the helper did not close the layer by itself", run.label));
        }
        if run.ending != Ending::Never && settled.is_none() {
            p.failed.push(format!("{}: the helper never answered settled", run.label));
        }
        if run.ending != Ending::Never && settled.is_some() && first_after_settled.is_some_and(|d| d.1 > 2) {
            p.failed.push(format!("{}: the first capture after settled was not the new still", run.label));
        }
        Ok(())
    }

    fn wallpaper_window() -> Option<HWND> {
        match layer::layout().ok()?.0 {
            layer::Layout::InProgman { wallpaper, .. } | layer::Layout::InWorkerW { wallpaper, .. } => Some(HWND(wallpaper as *mut c_void)),
            layer::Layout::ProgmanOnly { progman, .. } => Some(HWND(progman as *mut c_void)),
        }
    }

    fn window_pixels(window: HWND, width: i32, height: i32) -> Option<RgbImage> {
        let source = unsafe { GetDC(Some(window)) };
        if source.is_invalid() {
            return None;
        }
        let memory = unsafe { CreateCompatibleDC(Some(source)) };
        let bitmap = unsafe { CreateCompatibleBitmap(source, width, height) };
        let old = unsafe { SelectObject(memory, bitmap.into()) };
        let copied = unsafe { BitBlt(memory, 0, 0, width, height, Some(source), 0, 0, SRCCOPY) };
        unsafe { SelectObject(memory, old) };
        let header = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() };
        let mut info = BITMAPINFO { bmiHeader: header, ..Default::default() };
        let mut data = vec![0u8; width as usize * height as usize * 4];
        let lines = unsafe { GetDIBits(memory, bitmap, 0, height as u32, Some(data.as_mut_ptr() as *mut c_void), &mut info, DIB_RGB_COLORS) };
        let _ = unsafe { DeleteObject(bitmap.into()) };
        let _ = unsafe { DeleteDC(memory) };
        unsafe { ReleaseDC(Some(window), source) };
        if copied.is_err() || lines != height {
            return None;
        }
        let rgb = data.as_chunks::<4>().0.iter().flat_map(|pixel| [pixel[2], pixel[1], pixel[0]]).collect();
        RgbImage::from_raw(width as u32, height as u32, rgb)
    }

    fn classify(shot: &RgbImage, old: &RgbImage, new: &RgbImage, keep: &[usize]) -> char {
        side(shot, old, new, &region(old, new, keep))
    }

    fn handover_question(p: &mut Probe, helper: &mut Helper, id: &mut u32, screen: &Screen, kit: &Kit) -> Result<()> {
        let desktop = platform::current()?;
        let area = screen_rect(screen);
        let (width, height) = (kit.size[0], kit.size[1]);
        let keep = outside(width, height, &kit.icons, 3);
        let (old, new) = (&kit.stills[0], &kit.stills[1]);
        p.line("- **what the screen shows when the layer closes after the system wallpaper changed under it**".to_string());
        for delay in [0u64, 150, 300, 600, 1200, 2500] {
            desktop.set_wallpaper(screen, &old.0)?;
            let (ready, _, _) = equal_until(area, &old.1, &new.1, &keep, Duration::from_secs(6));
            *id += 1;
            helper.open(*id, screen, &old.0, ANSWER)?;
            pump(Duration::from_millis(200));
            let window = wallpaper_window();
            let set = Instant::now();
            desktop.set_wallpaper(screen, &new.0)?;
            let mut painted = None;
            let mut under = String::new();
            while set.elapsed() < Duration::from_millis(delay) {
                if let Some(window) = window {
                    if let Some(shot) = window_pixels(window, width as i32, height as i32) {
                        let class = classify(&shot, &old.1, &new.1, &keep);
                        if !under.ends_with(class) {
                            under.push(class);
                        }
                        if class == 'B' && painted.is_none() {
                            painted = Some(set.elapsed().as_millis());
                        }
                    }
                }
                pump(Duration::from_millis(10));
            }
            let closing = Instant::now();
            helper.close(*id, ANSWER)?;
            let mut seen: Vec<(u128, char)> = Vec::new();
            while closing.elapsed() < Duration::from_millis(600) {
                if let Ok(shot) = capture(area) {
                    seen.push((closing.elapsed().as_millis(), classify(&shot, &old.1, &new.1, &keep)));
                }
            }
            let later = Instant::now();
            let mut settled_on = None;
            while later.elapsed() < Duration::from_secs(3) {
                if let Ok(shot) = capture(area) {
                    if classify(&shot, &old.1, &new.1, &keep) == 'B' {
                        settled_on = Some(600 + later.elapsed().as_millis());
                        break;
                    }
                }
                pump(Duration::from_millis(20));
            }
            if painted.is_none() {
                if let Some(window) = window {
                    let probe = Instant::now();
                    while probe.elapsed() < Duration::from_secs(2) {
                        if window_pixels(window, width as i32, height as i32).is_some_and(|shot| classify(&shot, &old.1, &new.1, &keep) == 'B') {
                            painted = Some(delay as u128 + 600 + probe.elapsed().as_millis());
                            break;
                        }
                        pump(Duration::from_millis(10));
                    }
                }
            }
            let olds = seen.iter().filter(|(_, class)| *class == 'A').count();
            p.line(format!(
                "  - closed {delay} ms after the set call (old still ready {:?} ms): first 600 ms after close {}, the new picture on screen at {settled_on:?} ms after close, the wallpaper window's own pixels went {under} and showed the new picture {painted:?} ms after the set call, old picture captures after close {olds}",
                ready.map(|d| d.as_millis()),
                seen.iter().map(|(at, class)| format!("{at}{class}")).collect::<Vec<_>>().join(" ")
            ));
        }
        Ok(())
    }

    fn shader_time(p: &mut Probe, kit: &Kit) -> Result<()> {
        let gpu = scooch_live::Gpu::new(false).map_err(anyhow::Error::msg)?;
        let picture = gpu.picture(&kit.canvas);
        let glide = gpu.glide(&picture, &kit.pair.0, &kit.pair.1).map_err(anyhow::Error::msg)?;
        let (width, height) = (kit.size[0], kit.size[1]);
        let mut lines = Vec::new();
        for s in [0.0f32, 0.5, 1.0] {
            let mut times: Vec<u128> = (0..6).filter_map(|_| gpu.time(width, height, &[(scooch_live::Look::Glide(&glide, s), 1.0)]).ok()).map(|d| d.as_millis()).collect();
            times.sort_unstable();
            lines.push(format!("s {s}: {times:?} ms"));
        }
        p.line(format!("- one frame of the real shader at {width}x{height} on {} ({}), drawn offscreen and waited for: {}", gpu.name, gpu.kind, lines.join(", ")));
        Ok(())
    }

    fn color_check(p: &mut Probe, helper: &mut Helper, id: u32, screen: &Screen, still: &(PathBuf, RgbImage), label: &str) -> Result<()> {
        let desktop = platform::current()?;
        let area = screen_rect(screen);
        let icons: Vec<Rect> = icon_rects().unwrap_or_default().into_iter().map(|rect| Rect { x: rect.x - screen.frame.x, y: rect.y - screen.frame.y, ..rect }).collect();
        let wide = outside(area.right as u32 - area.left as u32, area.bottom as u32 - area.top as u32, &icons, 1);
        if still.1.dimensions() != ((area.right - area.left) as u32, (area.bottom - area.top) as u32) {
            p.line(format!("- {label}: the still is {:?} and the screen {}x{}, so it is skipped", still.1.dimensions(), area.right - area.left, area.bottom - area.top));
            return Ok(());
        }
        desktop.set_wallpaper(screen, &still.0)?;
        let landed = shows(area, &still.1, &wide, Duration::from_secs(4));
        park_cursor();
        let plain = settle(area, Duration::from_secs(3))?;
        helper.open(id, screen, &still.0, ANSWER)?;
        pump(Duration::from_millis(300));
        let layered = capture(area)?;
        helper.close(id, ANSWER)?;
        let system = difference(&plain, &still.1, &wide);
        let ours = difference(&layered, &still.1, &wide);
        let between = difference(&layered, &plain, &wide);
        p.line(format!(
            "- {label}: wallpaper on screen after {:?} ms, the system against the file mean {:.2} 99th {} largest {}, the layer against the file mean {:.2} 99th {} largest {}, the layer against the plain desktop mean {:.2} 99th {} largest {}, target largest at most 2 {}",
            landed.map(|d| d.as_millis()),
            system.0,
            system.1,
            system.2,
            ours.0,
            ours.1,
            ours.2,
            between.0,
            between.1,
            between.2,
            if between.2 <= 2 { "met" } else { "not met" }
        ));
        if between.1 > 8 {
            p.failed.push(format!("{label}: the layer does not match the system's wallpaper"));
        }
        Ok(())
    }

    fn timing(p: &mut Probe, helper: &mut Helper, id: &mut u32, screen: &Screen, kit: &Kit) -> Result<()> {
        let area = screen_rect(screen);
        let desktop = platform::current()?;
        desktop.set_wallpaper(screen, &kit.stills[0].0)?;
        pump(Duration::from_millis(1500));
        let still_patch = patch_of(&kit.stills[0].1, area, kit.patch);
        let other_patch = patch_of(&kit.other.1, area, kit.spot);
        let (mut shown, mut told, mut moving, mut firsts, mut visible) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for _ in 0..5 {
            *id += 1;
            let asked = Instant::now();
            helper.ask_open(*id, screen, &kit.stills[0].0)?;
            told.push(u128::from(helper.shown(*id, ANSWER)?));
            shown.push(asked.elapsed().as_millis());
            pump(Duration::from_millis(100));
            let sent = Instant::now();
            helper.ask_glide(*id, &kit.tables[0], &kit.tables[1], GLIDE_MS)?;
            let first = patch_until(kit.patch, &still_patch, false, Duration::from_secs(2));
            moving.push(first.map(|d| d.as_millis().min(sent.elapsed().as_millis())));
            let glided = helper.glided(*id, ANSWER)?;
            firsts.push((glided.first_frame_ms, glided.frames, glided.slow_frames));
            helper.close(*id, ANSWER)?;
            pump(Duration::from_millis(200));
            *id += 1;
            let asked = Instant::now();
            helper.ask_open(*id, screen, &kit.other.0)?;
            let seen = patch_until(kit.spot, &other_patch, true, Duration::from_secs(2));
            helper.shown(*id, ANSWER)?;
            visible.push(seen.map(|d| d.as_millis().min(asked.elapsed().as_millis())));
            helper.close(*id, ANSWER)?;
            pump(Duration::from_millis(200));
        }
        p.line(format!("- opens after the first, open sent to shown {shown:?} ms by the probe and {told:?} ms by the helper"));
        p.line(format!("- glide sent to the first moving frame captured on screen {moving:?} ms, the helper's first frame, frames and slow frames {firsts:?}"));
        p.line(format!("- open of a still that differs from the wallpaper, sent to first captured on screen {visible:?} ms"));
        let rates: Vec<f32> = firsts.iter().map(|(_, frames, _)| *frames as f32 / (GLIDE_MS as f32 / 1000.0)).collect();
        p.line(format!("- frames per second during a {GLIDE_MS} ms glide {rates:?}, the display runs at {:?} Hz", display_rate()));
        Ok(())
    }

    fn display_rate() -> Option<u32> {
        let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut mode) }.as_bool().then_some(mode.dmDisplayFrequency)
    }

    unsafe extern "system" fn flash_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn plain_window_latency(p: &mut Probe) -> Result<()> {
        let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
        let brush = unsafe { CreateSolidBrush(COLORREF(0x00FF00FF)) };
        let class = WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(flash_procedure), hInstance: instance.into(), lpszClassName: w!("ScoochProbeFlash"), hbrBackground: brush, ..Default::default() };
        unsafe { RegisterClassExW(&class) };
        let window = unsafe { CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("ScoochProbeFlash"), w!("flash"), WS_POPUP, 300, 300, 48, 48, None, None, Some(instance.into()), None) }?;
        let spot = RECT { left: 316, top: 316, right: 332, bottom: 332 };
        let magenta = |shot: &RgbImage| shot.get_pixel(8, 8).0 == [255, 0, 255];
        let mut times = Vec::new();
        for _ in 0..6 {
            let shown = Instant::now();
            let _ = unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
            let _ = unsafe { UpdateWindow(window) };
            let mut seen = None;
            while shown.elapsed() < Duration::from_secs(1) {
                drain();
                if capture(spot).is_ok_and(|shot| magenta(&shot)) {
                    seen = Some(shown.elapsed().as_millis());
                    break;
                }
            }
            times.push(seen);
            let _ = unsafe { ShowWindow(window, SW_HIDE) };
            pump(Duration::from_millis(200));
        }
        let _ = unsafe { DestroyWindow(window) };
        let _ = unsafe { DeleteObject(brush.into()) };
        p.line(format!("- a plain topmost window painted by GDI, shown to first captured {times:?} ms"));
        Ok(())
    }

    fn glides(p: &mut Probe, helper: &mut Helper, id: &mut u32, screen: &Screen, kit: &Kit) -> Result<()> {
        let pid = helper.pid;
        let mut marks = Vec::new();
        let mut uploads = Vec::new();
        let (mut first, mut last) = (None, None);
        for round in 1..=20u32 {
            *id += 1;
            helper.open(*id, screen, &kit.stills[0].0, ANSWER)?;
            helper.ask_glide(*id, &kit.tables[0], &kit.tables[1], GLIDE_MS)?;
            helper.ask_settle(*id, &kit.stills[1].0, None)?;
            uploads.push(helper.glided(*id, ANSWER)?.uploads);
            helper.settled(*id, ANSWER)?;
            helper.close(*id, ANSWER)?;
            drain();
            if [1, 20].contains(&round) {
                std::thread::sleep(Duration::from_millis(300));
                marks.push((round, process_numbers(pid)));
                if round == 1 {
                    first = handle_types(pid);
                } else {
                    last = handle_types(pid);
                }
            }
        }
        uploads.sort_unstable();
        uploads.dedup();
        p.line(format!(
            "- twenty glides in one helper, {}, pictures uploaded {uploads:?}",
            marks.iter().map(|(round, numbers)| format!("after {round}: {}", numbers.map_or("unreadable".to_string(), |(private, handles)| format!("{} MB private, {handles} handles", private / 1_000_000)))).collect::<Vec<_>>().join(", ")
        ));
        if let (Some(first), Some(last)) = (&first, &last) {
            let mut kinds: Vec<String> = first.keys().chain(last.keys()).cloned().collect();
            kinds.sort();
            kinds.dedup();
            let moved: Vec<String> = kinds.iter().filter_map(|kind| {
                let (before, after) = (first.get(kind).copied().unwrap_or(0), last.get(kind).copied().unwrap_or(0));
                (before != after).then(|| format!("{kind} {before} to {after}"))
            }).collect();
            p.line(format!("  - handle types that changed between glide 1 and glide 20 {}", if moved.is_empty() { "none".to_string() } else { moved.join(", ") }));
        }
        let at = |round: u32| marks.iter().find(|(seen, _)| *seen == round).and_then(|(_, numbers)| *numbers);
        if let (Some(one), Some(twenty)) = (at(1), at(20)) {
            if twenty.0 > one.0 + 15_000_000 {
                p.failed.push(format!("helper private bytes grew {} MB over twenty glides", (twenty.0 - one.0) / 1_000_000));
            }
            if twenty.1 > one.1 + 4 {
                p.failed.push(format!("helper handles grew from {} to {} over twenty glides", one.1, twenty.1));
            }
        }
        if uploads != vec![1] {
            p.failed.push(format!("the picture was uploaded more than once {uploads:?}"));
        }
        Ok(())
    }

    fn hold(args: &[String]) -> ExitCode {
        let run = || -> Result<()> {
            let [program, still, x, y, w, h, present] = args else { bail!("hold needs a program, a still, a rectangle and a presentation") };
            let frame = Rect { x: x.parse()?, y: y.parse()?, w: w.parse()?, h: h.parse()? };
            let screen = Screen { id: "held".into(), frame, scale: 1.0 };
            let mut helper = Helper::spawn(Path::new(program), Duration::from_secs(5))?;
            helper.warm(Hints { present: Some(present.clone()), scale: None }, None, None, Duration::from_secs(20))?;
            helper.open(1, &screen, Path::new(still), ANSWER)?;
            println!("shown {}", helper.pid);
            let _ = std::io::stdout().flush();
            loop {
                let _ = helper.ask_settle(1, Path::new(still), Some(1));
                let _ = helper.settled(1, ANSWER);
                std::thread::sleep(Duration::from_secs(1));
            }
        };
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                println!("failed {error:#}");
                ExitCode::FAILURE
            }
        }
    }

    fn other_on_screen(kit: &Kit, area: RECT, limit: Duration) -> Option<Duration> {
        patch_until(kit.spot, &patch_of(&kit.other.1, area, kit.spot), true, limit)
    }

    fn other_gone(kit: &Kit, area: RECT, limit: Duration) -> Option<Duration> {
        patch_until(kit.spot, &patch_of(&kit.other.1, area, kit.spot), false, limit)
    }

    fn parent_check(p: &mut Probe, program: &Path, screen: &Screen, kit: &Kit) -> Result<()> {
        let area = screen_rect(screen);
        let frame = screen.frame;
        let mut holder = Command::new(std::env::current_exe()?)
            .arg("--hold")
            .arg(program)
            .arg(&kit.other.0)
            .args([frame.x, frame.y, frame.w, frame.h].map(|value| value.to_string()))
            .arg(Method::Bare.hint())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?;
        let output = holder.stdout.take().context("the holder has no output")?;
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for text in std::io::BufRead::lines(std::io::BufReader::new(output)).map_while(Result::ok) {
                if sender.send(text).is_err() {
                    break;
                }
            }
        });
        let said = lines.recv_timeout(Duration::from_secs(40)).unwrap_or_default();
        let Some(pid) = said.strip_prefix("shown ").and_then(|pid| pid.trim().parse::<u32>().ok()) else {
            let _ = holder.kill();
            let _ = holder.wait();
            bail!("the holding process said {said:?}");
        };
        let before = other_on_screen(kit, area, Duration::from_secs(3));
        holder.kill()?;
        let _ = holder.wait();
        let off = other_gone(kit, area, Duration::from_secs(2));
        let ended = gone_within(pid, Duration::from_secs(3));
        p.line(format!("- the process that started the helper was ended after a capture saw its layer {:?} ms after shown, gone from the screen after {:?} ms, helper process gone after {:?} ms", before.map(|d| d.as_millis()), off.map(|off| off.as_millis()), ended.map(|ended| ended.as_millis())));
        if before.is_none() || off.is_none_or(|off| off > Duration::from_millis(500)) || ended.is_none() {
            p.failed.push("ending the daemon side did not take the layer and the helper away within 500 ms".into());
        }
        Ok(())
    }

    fn hang_up_check(p: &mut Probe, program: &Path, screen: &Screen, kit: &Kit) -> Result<()> {
        let area = screen_rect(screen);
        let mut helper = Helper::spawn(program, Duration::from_secs(5))?;
        helper.warm(bare(), None, None, Duration::from_secs(20))?;
        helper.open(1, screen, &kit.other.0, ANSWER)?;
        let before = other_on_screen(kit, area, Duration::from_secs(2));
        helper.hang_up();
        let off = other_gone(kit, area, Duration::from_secs(2));
        let ended = helper.wait_exit(Duration::from_secs(3));
        p.line(format!("- the pipe to the helper was closed with the layer on screen after {:?} ms, gone from the screen after {:?} ms, helper exited after {:?} ms", before.map(|d| d.as_millis()), off.map(|off| off.as_millis()), ended.map(|ended| ended.as_millis())));
        if before.is_none() || off.is_none_or(|off| off > Duration::from_millis(500)) || ended.is_none() {
            p.failed.push("closing the pipe did not take the layer and the helper away within 500 ms".into());
        }
        Ok(())
    }

    fn helper_kill_check(p: &mut Probe, program: &Path, screen: &Screen, kit: &Kit) -> Result<()> {
        let area = screen_rect(screen);
        let shell_before = shell_process();
        let mut helper = Helper::spawn(program, Duration::from_secs(5))?;
        helper.warm(bare(), None, None, Duration::from_secs(20))?;
        helper.open(1, screen, &kit.other.0, ANSWER)?;
        let before = other_on_screen(kit, area, Duration::from_secs(2));
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, false, helper.pid) }?;
        let ended = unsafe { TerminateProcess(handle, 9) };
        let _ = unsafe { CloseHandle(handle) };
        ended?;
        let off = other_gone(kit, area, Duration::from_secs(2));
        let cleared = wait_until(Duration::from_secs(2), || layer::ours_left() == 0);
        let explorer = shell_process() == shell_before && shell::view().is_ok();
        let asked = Instant::now();
        let answer = helper.close(1, Duration::from_millis(400));
        let took = asked.elapsed().as_millis();
        p.line(format!(
            "- the helper was ended with its layer on screen after {:?} ms, gone from the screen after {:?} ms, windows gone after {:?} ms, Explorer unchanged {explorer}, close answered {:?} after {took} ms",
            before.map(|d| d.as_millis()),
            off.map(|off| off.as_millis()),
            cleared.map(|cleared| cleared.as_millis()),
            answer.map_err(|error| error.to_string())
        ));
        if before.is_none() || off.is_none() || cleared.is_none() || !explorer || took > 450 {
            p.failed.push("ending the helper did not leave a clean desktop and a quick failure".into());
        }
        Ok(())
    }

    fn idle_check(p: &mut Probe, program: &Path, screen: &Screen, kit: &Kit) -> Result<()> {
        let mut helper = Helper::spawn(program, Duration::from_secs(5))?;
        helper.warm(bare(), None, None, Duration::from_secs(20))?;
        helper.open(1, screen, &kit.stills[0].0, ANSWER)?;
        helper.close(1, ANSWER)?;
        let last = Instant::now();
        let pid = helper.pid;
        let ended = wait_until(Duration::from_secs(45), || helper.exited()).map(|_| last.elapsed());
        let left = layer::ours_left();
        p.line(format!("- a helper left alone exited {:?} ms after its last command, process gone {}, windows left {left}", ended.map(|ended| ended.as_millis()), process_gone(pid)));
        if ended.is_none_or(|ended| ended < Duration::from_secs(29) || ended > Duration::from_secs(36)) || left > 0 {
            p.failed.push("a helper left alone did not exit by itself after about 30 seconds".into());
        }
        Ok(())
    }

    fn input_check(p: &mut Probe, helper: &mut Helper, id: &mut u32, screen: &Screen, kit: &Kit) -> Result<()> {
        let view = shell::view()?;
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        let (name, folder) = PROBES[5];
        let shown = shown_name(name, folder);
        let rect = automation_rects(&automation, view.list)?.into_iter().find(|(seen, _)| *seen == shown).map(|(_, rect)| rect).context("no rect for the click target")?;
        let pidl = pidl_of(&view, name, folder)?;
        let target = POINT { x: (rect.x + rect.w / 2.0) as i32, y: (rect.y + 20.0) as i32 };
        click(empty_spot(), false);
        pump(Duration::from_millis(400));
        let (sent, bare_click) = press(&view, &pidl, target);
        p.line(format!("- with no layer, a click on `{shown}` sent {sent} selected it {bare_click:?} ms after the button went down"));
        click(empty_spot(), false);
        pump(Duration::from_millis(400));
        *id += 1;
        helper.open(*id, screen, &kit.stills[0].0, ANSWER)?;
        helper.ask_glide(*id, &kit.tables[0], &kit.tables[1], 4000)?;
        pump(Duration::from_millis(300));
        let (sent, took) = press(&view, &pidl, target);
        p.line(format!("- during a glide, a click on `{shown}` sent {sent} selected it {took:?} ms after the button went down"));
        if took.is_none_or(|took| took > 200) {
            p.failed.push(format!("a click on an icon during a glide selected it after {took:?} ms"));
        }
        let (moved_name, moved_folder) = PROBES[6];
        let moved = shown_name(moved_name, moved_folder);
        let moved_pidl = pidl_of(&view, moved_name, moved_folder)?;
        let (_, before) = position(&view, &moved_pidl)?;
        let rect = automation_rects(&automation, view.list)?.into_iter().find(|(seen, _)| *seen == moved).map(|(_, rect)| rect).context("no rect for the drag target")?;
        let from = POINT { x: (rect.x + rect.w / 2.0) as i32, y: (rect.y + 20.0) as i32 };
        let to = POINT { x: (screen.frame.x + screen.frame.w * 0.7) as i32, y: (screen.frame.y + screen.frame.h * 0.7) as i32 };
        let dragged = drag_to(from, to);
        pump(Duration::from_millis(600));
        let (_, after) = position(&view, &moved_pidl)?;
        p.line(format!("- during a glide, a drag of `{moved}` sent {dragged} moved it from {},{} to {},{}", before.x, before.y, after.x, after.y));
        if before.x == after.x && before.y == after.y {
            p.failed.push("a drag during a glide did not move the icon".into());
        }
        let glided = helper.glided(*id, Duration::from_secs(6));
        helper.close(*id, ANSWER)?;
        p.line(format!("  - the helper glided {:?} during the click and the drag", glided.map_err(|error| error.to_string())));
        park_cursor();
        Ok(())
    }

    fn live(p: &mut Probe) -> Result<()> {
        uncover(p);
        let desktop = platform::current()?;
        let screen = desktop.screens()?.first().context("no screen")?.clone();
        let area = screen_rect(&screen);
        p.line("- Progman and WorkerW windows before".to_string());
        for line in layer::tree() {
            p.line(format!("  - `{line}`"));
        }
        let (layout, notes) = layer::layout()?;
        for note in notes {
            p.line(format!("- {note}"));
        }
        p.line(format!("- layout {}", layout.describe()));
        p.line("- Progman and WorkerW windows after".to_string());
        for line in layer::tree() {
            p.line(format!("  - `{line}`"));
        }
        let original = desktop.wallpaper(&screen).ok().flatten();
        p.line(format!("- wallpaper before {original:?}"));
        let (width, height) = (screen.frame.w as u32, screen.frame.h as u32);
        let field = subject_picture(width, height);
        let other = RgbImage::from_raw(width, height, field.as_raw().iter().map(|value| value / 2 + 60).collect()).context("no second picture")?;
        let field_path = p.home.join("layer-field.png");
        let other_path = p.home.join("layer-other.png");
        fs::write(&field_path, scooch_core::encode_png(&field)?)?;
        fs::write(&other_path, scooch_core::encode_png(&other)?)?;
        let previous = capture(area)?;
        fade(p, "with no layer, the adapter", area, &previous, &field, || desktop.set_wallpaper(&screen, &field_path));
        fade(p, "with no layer, the adapter", area, &field, &other, || desktop.set_wallpaper(&screen, &other_path));
        fade(p, "with no layer, SystemParametersInfo", area, &other, &field, || spi_wallpaper(&field_path.to_string_lossy()));
        plain_window_latency(p)?;
        let made = Instant::now();
        let kit = kit(p, &screen)?;
        p.line(format!("- the probe built the pictures, bundles, stills, tables and CPU frames in {} ms", made.elapsed().as_millis()));
        let program = twin()?;
        let helpers_before = running("scoochw.exe").len();
        let own_before = own_private();
        p.line(format!("- probe private bytes before any helper {own_before}"));
        let spawned = Instant::now();
        let mut helper = Helper::spawn(&program, Duration::from_secs(10))?;
        let hello = spawned.elapsed().as_millis();
        let ready = helper.warm(bare(), Some(&kit.picture), Some(kit.size), Duration::from_secs(60))?;
        let warm_took = spawned.elapsed().as_millis();
        p.line(format!(
            "- helper {} said hello {hello} ms after spawn, device, shaders and picture ready on {} after {} ms, spawn to ready {warm_took} ms cold, pictures uploaded {}, once warm {:?} private bytes and handles",
            helper.pid,
            ready.adapter,
            ready.ms,
            ready.uploads,
            process_numbers(helper.pid)
        ));
        let mut id = 0u32;
        id += 1;
        color_check(p, &mut helper, id, &screen, &kit.stills[0], "color at the first scale, the glide still")?;
        id += 1;
        color_check(p, &mut helper, id, &screen, &kit.detail, "scale at the first scale, a fine detail still")?;
        let slow = 3000;
        let runs = [
            Pass { label: "glide from A to B, settle sent with the new wallpaper at 150 ms", name: "glide", tables: (&kit.tables[0], &kit.tables[1]), track: true, old: &kit.stills[0], new: &kit.stills[1], frames: &kit.frames, ending: Ending::During, ms: slow, strip: true, scale: Some(1.0) },
            Pass { label: "the same glide drawn at half scale", name: "half", tables: (&kit.tables[0], &kit.tables[1]), track: true, old: &kit.stills[0], new: &kit.stills[1], frames: &kit.frames, ending: Ending::During, ms: slow, strip: true, scale: Some(0.5) },
            Pass { label: "cross-fade to a mirrored B", name: "mirrored", tables: (&kit.tables[0], &kit.tables[2]), track: false, old: &kit.stills[0], new: &kit.stills[2], frames: &kit.mirrored_frames, ending: Ending::During, ms: slow, strip: true, scale: None },
            Pass { label: "glide with settle sent together with the glide", name: "early", tables: (&kit.tables[0], &kit.tables[1]), track: true, old: &kit.stills[0], new: &kit.stills[1], frames: &kit.frames, ending: Ending::AtOnce, ms: slow, strip: false, scale: None },
            Pass { label: "glide with no settle, left to the helper's five second rule", name: "unsettled", tables: (&kit.tables[0], &kit.tables[1]), track: true, old: &kit.stills[0], new: &kit.stills[1], frames: &kit.frames, ending: Ending::Never, ms: slow, strip: false, scale: None },
            Pass { label: "a pair where only the scenery changes", name: "scenery", tables: (&kit.tables[0], &kit.tables[3]), track: false, old: &kit.stills[0], new: &kit.stills[3], frames: &kit.scenic_frames, ending: Ending::During, ms: slow, strip: true, scale: None },
            Pass { label: "a 450 ms glide for timing only", name: "quick", tables: (&kit.tables[0], &kit.tables[1]), track: false, old: &kit.stills[0], new: &kit.stills[1], frames: &kit.frames, ending: Ending::During, ms: GLIDE_MS, strip: false, scale: None },
        ];
        for run in &runs {
            id += 1;
            if let Err(error) = glide_run(p, &mut helper, id, &screen, &kit, run) {
                p.failed.push(format!("{} stopped with {error:#}", run.label));
                p.line(format!("  - stopped with {error:#}"));
            }
            pump(Duration::from_millis(500));
        }
        if let Err(error) = handover_question(p, &mut helper, &mut id, &screen, &kit) {
            p.line(format!("  - stopped with {error:#}"));
        }
        timing(p, &mut helper, &mut id, &screen, &kit)?;
        glides(p, &mut helper, &mut id, &screen, &kit)?;
        input_check(p, &mut helper, &mut id, &screen, &kit)?;
        let pid = helper.pid;
        drop(helper);
        p.line(format!("- after quit the helper process is gone {}", gone_within(pid, Duration::from_secs(2)).is_some()));
        desktop.set_wallpaper(&screen, &kit.stills[0].0)?;
        pump(Duration::from_millis(1500));
        hang_up_check(p, &program, &screen, &kit)?;
        parent_check(p, &program, &screen, &kit)?;
        helper_kill_check(p, &program, &screen, &kit)?;
        idle_check(p, &program, &screen, &kit)?;
        let mut helper = Helper::spawn(&program, Duration::from_secs(10))?;
        helper.warm(bare(), Some(&kit.picture), Some(kit.size), Duration::from_secs(60))?;
        restart_explorer(p)?;
        helper.send(&LiveCommand::Hello { version: scooch::live::VERSION })?;
        p.line("- **after an Explorer restart, with the same helper**".to_string());
        for line in layer::tree() {
            p.line(format!("  - `{line}`"));
        }
        id += 1;
        let after_restart = Pass { label: "glide after the restart", name: "restart", tables: (&kit.tables[0], &kit.tables[1]), track: true, old: &kit.stills[0], new: &kit.stills[1], frames: &kit.frames, ending: Ending::During, ms: 3000, strip: true, scale: None };
        if let Err(error) = glide_run(p, &mut helper, id, &screen, &kit, &after_restart) {
            p.failed.push(format!("the helper could not glide after an Explorer restart {error:#}"));
        }
        p.line("- Progman and WorkerW windows after the helper's open".to_string());
        for line in layer::tree() {
            p.line(format!("  - `{line}`"));
        }
        drop(helper);
        pump(Duration::from_millis(500));
        if let Err(error) = shader_time(p, &kit) {
            p.line(format!("- the shader could not be timed in the probe {error:#}"));
        }
        let helpers_after = running("scoochw.exe").len();
        let own_after = own_private();
        p.line(format!("- probe private bytes after every helper ran and exited {own_after}, change {} bytes, scoochw processes before {helpers_before} after {helpers_after}", own_after as i64 - own_before as i64));
        if helpers_after > helpers_before {
            p.failed.push("a helper process remained".into());
        }
        if let Some(path) = original {
            let _ = desktop.set_wallpaper(&screen, &path);
        }
        Ok(())
    }

    fn live_scaled(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = desktop.screens()?.first().context("no screen")?.clone();
        let (width, height) = (screen.frame.w as u32, screen.frame.h as u32);
        let detail = detailed(width, height);
        let path = p.home.join(format!("scaled-detail-{width}x{height}.png"));
        save_png(&path, &detail)?;
        let original = desktop.wallpaper(&screen).ok().flatten();
        let mut helper = Helper::spawn(&twin()?, Duration::from_secs(10))?;
        helper.warm(bare(), None, None, Duration::from_secs(60))?;
        color_check(p, &mut helper, 1, &screen, &(path, detail), &format!("scale at {} DPI, a fine detail still", list_dpi()))?;
        drop(helper);
        if let Some(path) = original {
            let _ = desktop.set_wallpaper(&screen, &path);
        }
        Ok(())
    }

    fn helper_children(parent: u32) -> Vec<u32> {
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return Vec::new() };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = Vec::new();
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
        while more {
            let length = entry.szExeFile.iter().position(|unit| *unit == 0).unwrap_or(entry.szExeFile.len());
            if entry.th32ParentProcessID == parent && String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case("scoochw.exe") {
                found.push(entry.th32ProcessID);
            }
            more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
        }
        let _ = unsafe { CloseHandle(snapshot) };
        found
    }

    fn log_lines(path: &Path) -> Vec<String> {
        fs::read_to_string(path).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn stamp_of(line: &str) -> Option<f64> {
        line.split_whitespace().next().and_then(|stamp| stamp.parse::<f64>().ok())
    }

    fn animation() -> Option<bool> {
        let mut enabled = ::windows::core::BOOL(1);
        unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut enabled as *mut ::windows::core::BOOL as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.ok().map(|_| enabled.as_bool())
    }

    fn set_animation(on: bool) -> bool {
        unsafe { SystemParametersInfoW(SPI_SETCLIENTAREAANIMATION, 0, Some(usize::from(on) as *mut c_void), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) }.is_ok()
    }

    struct Watched {
        dropped: f64,
        shots: Vec<(f64, RgbImage)>,
        log: Vec<String>,
        old: Option<PathBuf>,
        kept: Option<PathBuf>,
        new: Option<PathBuf>,
        sent: bool,
    }

    fn watch_change(log: &Path, screen: &Screen, act: impl FnOnce() -> bool, capture_screen: bool) -> Watched {
        let desktop = platform::current().ok();
        let area = screen_rect(screen);
        let old = desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten());
        let kept = old.as_ref().and_then(|path| {
            static KEPT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let number = KEPT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let extension = path.extension().map_or("png".to_string(), |extension| extension.to_string_lossy().into_owned());
            let kept = std::env::temp_dir().join(format!("scooch-judged-{}-{number}.{extension}", std::process::id()));
            fs::copy(path, &kept).ok().map(|_| kept)
        });
        let start = log_lines(log).len();
        let sent = act();
        let dropped = epoch();
        let started = Instant::now();
        let mut shots = Vec::new();
        let mut done: Option<Instant> = None;
        let mut count = 0usize;
        while started.elapsed() < Duration::from_secs(12) {
            if capture_screen {
                if let Ok(shot) = capture(area) {
                    shots.push((epoch(), shot));
                }
            } else {
                pump(Duration::from_millis(30));
            }
            count += 1;
            if done.is_none() && count.is_multiple_of(3) && log_lines(log).iter().skip(start).any(|line| line.contains(" tick ") && !line.contains("Unchanged)]")) {
                done = Some(Instant::now());
            }
            if done.is_some_and(|done| done.elapsed() > Duration::from_secs(1)) {
                break;
            }
            drain();
        }
        let log: Vec<String> = log_lines(log).into_iter().skip(start).filter(|line| line.contains(" glide ") || line.contains(" tick ") || line.contains(" wake ") || line.contains(" set ")).collect();
        let set = log.iter().rev().find_map(|line| line.split_once(' ').and_then(|(_, rest)| rest.strip_prefix("set ")).map(|path| PathBuf::from(path.trim())));
        let new = set.or_else(|| desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten()));
        Watched { dropped, shots, log, old, kept, new, sent }
    }

    fn logged(watched: &Watched, word: &str) -> Option<f64> {
        let word = word.trim_start();
        watched.log.iter().find(|line| line.split_once(' ').is_some_and(|(_, rest)| rest.starts_with(word))).and_then(|line| stamp_of(line))
    }

    fn tables_file(home: &Path, output: &Path) -> Option<Vec<u8>> {
        let name = output.file_name()?.to_string_lossy().into_owned();
        let found = fs::read_dir(home.join("cache").join("tables")).ok()?.flatten().find(|file| file.file_name().to_string_lossy().starts_with(&format!("{name}.")))?;
        fs::read(found.path()).ok()
    }

    struct Judged<'a> {
        source: &'a Path,
        old_tables: Option<&'a [u8]>,
        before: &'a RgbImage,
        still_icons: &'a [Rect],
        earlier_ready: Option<f64>,
    }

    fn judge_daemon_glide(p: &mut Probe, watched: &Watched, screen: &Screen, given: &Judged, label: &str, strip_name: Option<&str>) -> bool {
        let Judged { source, old_tables, before, still_icons, earlier_ready } = *given;
        let (Some(old_path), Some(new_path)) = (&watched.kept, &watched.new) else {
            p.line(format!("- {label}: the wallpaper before {:?}, kept as {:?}, or after {:?} is unknown", watched.old, watched.kept, watched.new));
            return false;
        };
        let (old, new) = (picture::decode(old_path), picture::decode(new_path));
        let (Ok(old), Ok(new)) = (old, new) else {
            p.line(format!("- {label}: the two stills could not be read, old kept as {} and new {}", old_path.display(), new_path.display()));
            return false;
        };
        let (width, height) = old.dimensions();
        let icons_now: Vec<Rect> = icon_rects().unwrap_or_default().into_iter().map(|rect| Rect { x: rect.x - screen.frame.x, y: rect.y - screen.frame.y, ..rect }).collect();
        let mut avoid = icons_now.clone();
        avoid.extend(still_icons.iter().copied());
        let keep = outside(width, height, &avoid, 2);
        let apart = region(&old, &new, &keep);
        let new_tables = tables_file(&p.home, new_path);
        let frames: Vec<(f32, RgbImage)> = match (old_tables, new_tables.as_deref(), picture::decode(source)) {
            (Some(a), Some(b), Ok(image)) => {
                let canvas = scooch_core::canvas(&image, width, height);
                match (scooch_core::Bundle::tables_from_bytes(a, canvas.clone()), scooch_core::Bundle::tables_from_bytes(b, canvas)) {
                    (Ok(a), Ok(b)) => (0..=20).map(|step| step as f32 / 20.0).map(|s| (s, scooch_core::frame(&a, &b, s))).collect(),
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        let classes: Vec<char> = watched.shots.iter().map(|(_, shot)| side(shot, &old, &new, &apart)).collect();
        let open = logged(watched, " glide open");
        let settled = logged(watched, " glide settled");
        let closed = logged(watched, " glide closed");
        let settle = logged(watched, " glide settle ");
        let glided = logged(watched, " glide glided");
        let ready = logged(watched, " glide ready").or(earlier_ready);
        let tick = watched.log.iter().find(|line| line.contains(" tick ")).cloned();
        let set = logged(watched, " set ");
        let wake = logged(watched, " wake ");
        let fitted: Vec<Option<(f32, f32)>> = watched.shots.iter().map(|(_, shot)| (!frames.is_empty()).then(|| best_frame(shot, &frames, &keep))).collect();
        let window: Vec<f32> = watched.shots.iter().enumerate().filter(|(_, (at, _))| open.is_some_and(|open| *at >= open) && settled.is_none_or(|settled| *at <= settled)).filter_map(|(index, _)| fitted[index].map(|(s, _)| s)).collect();
        let steady = window.windows(2).all(|pair| pair[1] >= pair[0] - 0.1);
        let distinct = {
            let mut seen: Vec<i32> = window.iter().map(|s| (s * 20.0).round() as i32).collect();
            seen.dedup();
            seen.len()
        };
        let appeared = classes.iter().position(|class| *class == 'B');
        let returned = appeared.map_or(0, |first| classes[first..].iter().filter(|class| **class == 'A').count());
        let first_move = classes.iter().position(|class| *class != 'A' && *class != '=').map(|index| (watched.shots[index].0 - watched.dropped) * 1000.0);
        let mask = strong(before, &old, still_icons);
        let mut hidden = 0usize;
        for (index, (_, shot)) in watched.shots.iter().enumerate() {
            let nearest = fitted[index].and_then(|(s, _)| frames.iter().min_by(|x, y| (x.0 - s).abs().total_cmp(&(y.0 - s).abs())).map(|(_, frame)| frame));
            let mut expected = vec![&old, &new];
            expected.extend(nearest);
            hidden = hidden.max(covered(shot, before, &expected, &mask).0);
        }
        let wide = outside(width, height, &avoid, 1);
        let hand_over: Option<(f32, u8, u8)> = settled.map(|settled| {
            watched.shots.iter().filter(|(at, _)| *at > settled && closed.is_none_or(|closed| *at <= closed + 1.0)).map(|(_, shot)| difference(shot, &new, &wide)).fold((0.0f32, 0u8, 0u8), |w, d| (w.0.max(d.0), w.1.max(d.1), w.2.max(d.2)))
        });
        let since = |time: Option<f64>| time.map(|time| ((time - watched.dropped) * 1000.0).round());
        let order: Vec<(&str, Option<f64>)> = vec![("ready", ready), ("open and glide", open), ("settle after the set", settle), ("glided", glided), ("settled", settled), ("closed", closed)];
        let in_order = match (open, settle, settled, closed) {
            (Some(open), Some(settle), Some(settled), Some(closed)) => open <= settle && settled >= settle && closed >= settled && closed + 0.02 >= settle + HOLD as f64 / 1000.0,
            _ => false,
        };
        let counts = watched.log.iter().find(|line| line.contains(" glide glided")).and_then(|line| line.split(" glide glided ").nth(1)).map(str::to_string).unwrap_or_default();
        let laps = tick.as_deref().map(|line| line.split(" ms ").nth(1).and_then(|rest| rest.split("] ").nth(1)).unwrap_or("").to_string()).unwrap_or_default();
        let tick_ms = tick.as_deref().and_then(|line| line.split(" tick ").nth(1)).and_then(|rest| rest.split_whitespace().next()).and_then(|ms| ms.parse::<f64>().ok());
        let tick_start = tick.as_deref().and_then(stamp_of).zip(tick_ms).map(|(end, ms)| end - ms / 1000.0);
        p.line(format!(
            "- {label}: drag sent {}, {} captures over {:.0} ms, {} pixels differ between the two stills, classes {}",
            watched.sent,
            watched.shots.len(),
            watched.shots.last().map_or(0.0, |(at, _)| (at - watched.dropped) * 1000.0),
            apart.len(),
            classes.iter().collect::<String>()
        ));
        p.line(format!("  - daemon times after the drop {:?}, open before the set, settle after it, close after settled and {HOLD} ms after the set {in_order}, glided `{counts}`", order.iter().map(|(name, at)| format!("{name} {:?}", since(*at))).collect::<Vec<_>>()));
        p.line(format!(
            "  - drop to first moving frame on screen {:?} ms, wake {:?} ms, tick started {:?} ms, wallpaper set {:?} ms, settled {:?} ms, closed {:?} ms, tick laps `{laps}`",
            first_move.map(f64::round),
            since(wake),
            since(tick_start),
            since(set),
            since(settled),
            since(closed)
        ));
        p.line(format!("  - {} captures fitted to {} CPU frames during the glide, {distinct} distinct s, steady {steady}: {:?}", window.len(), frames.len(), window.iter().map(|s| (s * 100.0).round() / 100.0).collect::<Vec<_>>()));
        p.line(format!("  - icon pixels covered {hidden} of {}, from settled to a second after close against the new still {hand_over:?}, old picture after the new {returned}", total(&mask)));
        p.line(format!("  - daemon log {:?}", watched.log.iter().map(|line| line.chars().take(200).collect::<String>()).collect::<Vec<_>>()));
        if let Some(name) = strip_name {
            let picked: Vec<(f64, RgbImage)> = watched.shots.iter().filter(|(at, _)| open.is_none_or(|open| *at >= open - 0.05) && settled.is_none_or(|settled| *at <= settled + 0.1)).map(|(at, shot)| (*at, shot.clone())).collect();
            let timed: Vec<(u128, RgbImage)> = picked.into_iter().map(|(at, shot)| (((at - watched.dropped) * 1000.0).max(0.0) as u128, shot)).collect();
            if let (Some(first), Some(last)) = (timed.first().map(|(at, _)| *at), timed.last().map(|(at, _)| *at)) {
                if let Some(image) = strip(&timed, first, last) {
                    let _ = save_png(&p.out.join(format!("{name}.png")), &image);
                    p.line(format!("  - nine frame strip {name}.png"));
                }
            }
        }
        returned == 0 && steady && distinct >= 3 && hidden * 100 <= total(&mask).max(1) && hand_over.is_some_and(|d| d.1 <= 2) && in_order
    }

    fn icons_except(screen: &Screen, moving: &str) -> Vec<Rect> {
        let Ok(view) = shell::view() else { return Vec::new() };
        let automation: ::windows::core::Result<IUIAutomation> = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
        let Ok(automation) = automation else { return Vec::new() };
        automation_rects(&automation, view.list).unwrap_or_default().into_iter().filter(|(seen, _)| seen != moving).map(|(_, rect)| Rect { x: rect.x - screen.frame.x, y: rect.y - screen.frame.y, ..rect }).collect()
    }

    fn icon_spot(name: &str, folder: bool) -> Option<(f32, f32)> {
        let view = shell::view().ok()?;
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let shown = shown_name(name, folder);
        automation_rects(&automation, view.list).ok()?.into_iter().find(|(seen, _)| *seen == shown).map(|(_, rect)| (rect.x + rect.w / 2.0, rect.y))
    }

    fn drag_icon(name: &str, folder: bool, to: POINT) -> bool {
        let Ok(view) = shell::view() else { return false };
        let automation: ::windows::core::Result<IUIAutomation> = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
        let Ok(automation) = automation else { return false };
        let shown = shown_name(name, folder);
        let Some(rect) = automation_rects(&automation, view.list).ok().and_then(|rects| rects.into_iter().find(|(seen, _)| *seen == shown).map(|(_, rect)| rect)) else { return false };
        let from = POINT { x: (rect.x + rect.w / 2.0) as i32, y: (rect.y + 20.0) as i32 };
        let sent = drag_to(from, to);
        park_cursor();
        sent
    }

    fn start_glide_daemon(p: &mut Probe, log: &Path, forced: bool) -> Result<Child> {
        let twin = twin()?;
        let file = fs::File::create(log)?;
        let mut command = Command::new(&twin);
        command.arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(file);
        if forced {
            command.env("SCOOCH_GLIDE_ANYWAY", "1");
        } else {
            command.env_remove("SCOOCH_GLIDE_ANYWAY");
        }
        Ok(command.spawn()?)
    }

    fn quiet_log(log: &Path, quiet: Duration, limit: Duration) -> bool {
        let started = Instant::now();
        let mut count = log_lines(log).len();
        let mut since = Instant::now();
        while started.elapsed() < limit {
            pump(Duration::from_millis(100));
            let now = log_lines(log).len();
            if now != count {
                count = now;
                since = Instant::now();
            } else if since.elapsed() >= quiet {
                return true;
            }
        }
        false
    }

    fn stop_glide_daemon(child: &mut Child) -> Option<std::process::ExitStatus> {
        let _ = scooch::poke::stop(0);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(status)) = child.try_wait() {
                return Some(status);
            }
            pump(Duration::from_millis(50));
        }
        let _ = child.kill();
        child.wait().ok()
    }

    fn cover_window(screen: &Screen) -> Result<HWND> {
        let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
        let brush = unsafe { CreateSolidBrush(COLORREF(0x00202020)) };
        let class = WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(flash_procedure), hInstance: instance.into(), lpszClassName: w!("ScoochProbeCover"), hbrBackground: brush, ..Default::default() };
        unsafe { RegisterClassExW(&class) };
        let frame = screen.frame;
        let window = unsafe { CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("ScoochProbeCover"), w!("cover"), WS_POPUP, frame.x as i32, frame.y as i32, frame.w as i32, frame.h as i32, None, None, Some(instance.into()), None) }?;
        let _ = unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
        let _ = unsafe { UpdateWindow(window) };
        pump(Duration::from_millis(400));
        Ok(window)
    }

    fn daemon_glide(p: &mut Probe) -> Result<()> {
        let found = animation();
        let result = daemon_glide_steps(p);
        if let Some(found) = found {
            let put = set_animation(found);
            let now = animation();
            p.line(format!("- the animation setting put back as found, before {found} now {now:?} {put}"));
            if now != Some(found) {
                p.failed.push("the animation setting was not put back as the probe found it".into());
            }
        }
        result
    }

    fn daemon_glide_steps(p: &mut Probe) -> Result<()> {
        uncover(p);
        let turned = set_animation(true);
        p.line(format!("- animations turned on for the glide {turned}, read back {:?}", animation()));
        let desktop = platform::current()?;
        let screen = desktop.screens()?.first().context("no screen")?.clone();
        let area = screen_rect(&screen);
        let (width, height) = (screen.frame.w as u32, screen.frame.h as u32);
        let source = p.home.join("daemon-glide.jpg");
        fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/photos/balloon-sky.jpg"), &source)?;
        desktop.set_wallpaper(&screen, &source)?;
        pump(Duration::from_millis(800));
        fs::write(p.home.join("state.json"), br#"{"version":1,"on":true,"glide":true,"options":{"scenery":"auto"}}"#)?;
        let log = p.out.join("glide-daemon.log");
        let mut child = start_glide_daemon(p, &log, true)?;
        let daemon = child.id();
        p.line(format!("- started the daemon {daemon} with SCOOCH_GLIDE_ANYWAY=1 and SCOOCH_TRACE=1 on the runner's software adapter, wallpaper source {}", source.display()));
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(60), || rendered(&home));
        let motion = wait_until(Duration::from_secs(15), || log_lines(&log).iter().any(|line| line.contains("glide reduce motion off")));
        p.line(format!("- first render after {:?} ms, the daemon logged reduce motion off after {:?} ms, last reading `{}`", first.map(|time| time.as_millis()), motion.map(|time| time.as_millis()), log_lines(&log).iter().rev().find(|line| line.contains(" glide reduce motion ")).cloned().unwrap_or_default()));
        if motion.is_none() {
            p.failed.push("the daemon never logged reduce motion as off after the probe turned animations on".into());
            stop_glide_daemon(&mut child);
            return Ok(());
        }
        let (name, folder) = PROBES[0];
        let centre = POINT { x: (screen.frame.x + screen.frame.w * 0.5) as i32, y: (screen.frame.y + screen.frame.h * 0.5) as i32 };
        let aside = POINT { x: (screen.frame.x + screen.frame.w * 0.62) as i32, y: (screen.frame.y + screen.frame.h * 0.32) as i32 };
        let lower = POINT { x: (screen.frame.x + screen.frame.w * 0.42) as i32, y: (screen.frame.y + screen.frame.h * 0.62) as i32 };
        let mut setup = None;
        for (step, target) in [(1, lower), (2, aside)] {
            let calm = quiet_log(&log, Duration::from_millis(1000), Duration::from_secs(10));
            let from = icon_spot(name, folder);
            let watched = watch_change(&log, &screen, || drag_icon(name, folder, target), false);
            p.line(format!("- setup drag {step} from {from:?} to {},{} after a quiet log {calm}, icon now at {:?}: new wallpaper {}, log {:?}", target.x, target.y, icon_spot(name, folder), watched.new != watched.old, watched.log.iter().map(|line| line.chars().take(200).collect::<String>()).collect::<Vec<_>>()));
            setup = Some(watched);
        }
        let setup = setup.context("no setup drag")?;
        let wrote_tables = setup.log.iter().any(|line| line.contains(" tick ") && line.rsplit("] ").next().is_some_and(|laps| laps.split(", ").any(|lap| lap.starts_with("tables "))));
        let ticks: Vec<String> = setup.log.iter().filter(|line| line.contains(" tick ")).map(|line| line.chars().take(300).collect()).collect();
        p.line(format!("- the last setup drag's tick lines {ticks:?}"));
        p.line(format!("- the last setup drag rendered with a tables entry {wrote_tables}"));
        if !wrote_tables {
            p.failed.push("the setup drags wrote no tables, so the timed drag could not glide".into());
        }
        quiet_log(&log, Duration::from_millis(1000), Duration::from_secs(10));
        let still_icons = icons_except(&screen, &shown_name(name, folder));
        let old_tables = desktop.wallpaper(&screen).ok().flatten().and_then(|old| tables_file(&p.home, &old));
        park_cursor();
        let before = settle(area, Duration::from_secs(3))?;
        let from = icon_spot(name, folder);
        let watched = watch_change(&log, &screen, || drag_icon(name, folder, centre), true);
        p.line(format!("- the timed drag from {from:?} to {},{}, icon now at {:?}", centre.x, centre.y, icon_spot(name, folder)));
        let earlier_ready = logged(&setup, " glide ready");
        let given = Judged { source: &source, old_tables: old_tables.as_deref(), before: &before, still_icons: &still_icons, earlier_ready };
        let good = judge_daemon_glide(p, &watched, &screen, &given, "glide on, a real drag onto the subject", Some("daemon-glide-strip"));
        pump(Duration::from_millis(1000));
        let left = layer::ours_left();
        p.line(format!("- helper processes under the daemon {:?}, layer windows left {left}", helper_children(daemon)));
        if !good || left > 0 {
            p.failed.push("the daemon's glide did not move steadily under the icons and hand over cleanly".into());
        }
        let scooch = twin()?.with_file_name("scooch.exe");
        let toggle = |home: &Path| Command::new(&scooch).arg("glide").env("SCOOCH_HOME", home).output().map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string()).unwrap_or_default();
        p.line(format!("- scooch glide said {:?}", toggle(&p.home)));
        pump(Duration::from_millis(800));
        let off = watch_change(&log, &screen, || drag_icon(name, folder, lower), false);
        let off_time = logged(&off, " set ").map(|set| ((set - off.dropped) * 1000.0).round());
        let off_glided = off.log.iter().any(|line| line.contains(" glide open"));
        p.line(format!("- glide off, a drag: new wallpaper {}, set {:?} ms after the drop, glided {off_glided}, log {:?}", off.new != off.old, off_time, off.log.iter().map(|line| line.chars().take(160).collect::<String>()).collect::<Vec<_>>()));
        p.line(format!("- scooch glide said {:?}", toggle(&p.home)));
        if off_glided || off.new == off.old {
            p.failed.push("with glide off the daemon did not take today's path".into());
        }
        pump(Duration::from_millis(800));
        let wrote = set_animation(false);
        p.line(format!("- animations turned off {wrote}, read back {:?}", animation()));
        let reduced = watch_change(&log, &screen, || drag_icon(name, folder, aside), false);
        let reduced_glided = reduced.log.iter().any(|line| line.contains(" glide open"));
        let said = reduced.log.iter().any(|line| line.contains("glide skipped, reduce motion"));
        let read = log_lines(&log).iter().rev().find(|line| line.contains(" glide reduce motion ")).cloned();
        p.line(format!("- animations off, a drag: new wallpaper {}, glided {reduced_glided}, logged skipped reduce motion {said}, last reading `{}`, log {:?}", reduced.new != reduced.old, read.unwrap_or_default(), reduced.log.iter().map(|line| line.chars().take(160).collect::<String>()).collect::<Vec<_>>()));
        if reduced_glided || !said {
            p.failed.push("with animations off the daemon did not take today's path".into());
        }
        let _ = set_animation(true);
        let motion_back = wait_until(Duration::from_secs(10), || {
            let _ = scooch::poke::send(daemon);
            pump(Duration::from_millis(300));
            log_lines(&log).iter().rev().find(|line| line.contains(" glide reduce motion ")).is_some_and(|line| line.ends_with(" off"))
        });
        p.line(format!("- animations on again, the daemon read it after {:?} ms", motion_back.map(|time| time.as_millis())));
        quiet_log(&log, Duration::from_millis(1000), Duration::from_secs(10));
        let program_move = |target: POINT| {
            move || {
                let Ok(view) = shell::view() else { return false };
                let Ok(pidl) = pidl_of(&view, name, folder) else { return false };
                let mut client = [target];
                unsafe { MapWindowPoints(None, Some(view.list), &mut client) };
                let placed = place(&view, &pidl, client[0]).is_ok();
                let _ = scooch::poke::send(daemon);
                placed
            }
        };
        let tick_of = |watched: &Watched| watched.log.iter().find(|line| line.contains(" tick ")).cloned().unwrap_or_default();
        let open_move = watch_change(&log, &screen, program_move(POINT { x: lower.x - 30, y: lower.y - 20 }), false);
        p.line(format!("- a program move uncovered, then a wake: new wallpaper {}, glided {}, tick `{}`", open_move.new != open_move.old, open_move.log.iter().any(|line| line.contains(" glide open")), tick_of(&open_move)));
        quiet_log(&log, Duration::from_millis(1000), Duration::from_secs(10));
        let cover = cover_window(&screen)?;
        let covered_desktop = layer::covered(&screen);
        let shut = watch_change(&log, &screen, program_move(POINT { x: aside.x - 30, y: aside.y + 20 }), false);
        let _ = unsafe { DestroyWindow(cover) };
        pump(Duration::from_millis(400));
        let shut_glided = shut.log.iter().any(|line| line.contains(" glide open"));
        let hidden = shut.log.iter().any(|line| line.contains("glide skipped, hidden"));
        p.line(format!("- the same move while a window covers the whole desktop, covered seen {covered_desktop}: new wallpaper {}, glided {shut_glided}, logged skipped hidden {hidden}, tick `{}`, log {:?}", shut.new != shut.old, tick_of(&shut), shut.log.iter().map(|line| line.chars().take(200).collect::<String>()).collect::<Vec<_>>()));
        if shut.new == shut.old || !hidden {
            p.failed.push("a covered desktop did not skip the glide as hidden and set the wallpaper".into());
        }
        pump(Duration::from_millis(1500));
        let start = log_lines(&log).len();
        let shown_before = desktop.wallpaper(&screen).ok().flatten();
        let sent = drag_icon(name, folder, centre);
        let opened = wait_until(Duration::from_secs(6), || log_lines(&log).iter().skip(start).any(|line| line.contains(" glide open")));
        let killed = helper_children(daemon);
        for pid in &killed {
            if let Ok(handle) = unsafe { OpenProcess(PROCESS_TERMINATE, false, *pid) } {
                let _ = unsafe { TerminateProcess(handle, 9) };
                let _ = unsafe { CloseHandle(handle) };
            }
        }
        let _ = wait_until(Duration::from_secs(8), || log_lines(&log).iter().skip(start).any(|line| line.contains(" tick ")));
        pump(Duration::from_millis(1200));
        let now_shown = desktop.wallpaper(&screen).ok().flatten();
        let recorded = fs::read(p.home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()).and_then(|state| state["screens"].as_object().and_then(|screens| screens.values().next().and_then(|entry| entry["output"].as_str().map(PathBuf::from))));
        let matches = now_shown.as_ref().and_then(|path| picture::decode(path).ok()).and_then(|file| {
            let icons: Vec<Rect> = icon_rects().unwrap_or_default().into_iter().map(|rect| Rect { x: rect.x - screen.frame.x, y: rect.y - screen.frame.y, ..rect }).collect();
            capture(area).ok().map(|shot| difference(&shot, &file, &outside(width, height, &icons, 2)))
        });
        let alive = matches!(child.try_wait(), Ok(None));
        let kill_log: Vec<String> = log_lines(&log).into_iter().skip(start).filter(|line| line.contains(" glide ")).map(|line| line.chars().take(180).collect()).collect();
        p.line(format!(
            "- helper killed mid glide, drag sent {sent}, open logged after {:?} ms, killed {killed:?}, wallpaper changed {}, it is the recorded output {}, screen against the file {matches:?}, daemon alive {alive}, log {kill_log:?}",
            opened.map(|time| time.as_millis()),
            now_shown != shown_before,
            now_shown == recorded
        ));
        let mut tries = 0usize;
        let mut glided_again = false;
        for target in [aside, lower, centre] {
            quiet_log(&log, Duration::from_millis(1000), Duration::from_secs(10));
            tries += 1;
            let again = watch_change(&log, &screen, || drag_icon(name, folder, target), false);
            glided_again = again.log.iter().any(|line| line.contains(" glide settled"));
            let reasons: Vec<String> = again.log.iter().filter(|line| line.contains("glide skipped")).map(|line| line.chars().take(200).collect()).collect();
            p.line(format!("- drag {tries} after the kill glides {glided_again}, skip reasons {reasons:?}, log {:?}", again.log.iter().map(|line| line.chars().take(200).collect::<String>()).collect::<Vec<_>>()));
            if glided_again {
                break;
            }
        }
        p.line(format!("- it took {tries} drags to glide again after the helper was killed"));
        if killed.is_empty() || now_shown != recorded || !matches.is_some_and(|d| d.1 <= 2) || !alive || !glided_again || tries > 2 {
            p.failed.push(format!("killing the helper mid glide was not handled cleanly, {tries} drags to glide again"));
        }
        let last = Instant::now();
        let gone = wait_until(Duration::from_secs(40), || helper_children(daemon).is_empty());
        pump(Duration::from_secs(35).saturating_sub(last.elapsed()));
        let still = helper_children(daemon);
        p.line(format!("- no helper {:?} s after the last drag, 35 s after it helper processes {still:?}", gone.map(|_| last.elapsed().as_secs())));
        if !still.is_empty() {
            p.failed.push("a helper was still running 35 s after the last drag".into());
        }
        let (cpu_before, quiet_from) = (cpu(&child), epoch());
        pump(Duration::from_secs(60));
        let (cpu_after, quiet_to) = (cpu(&child), epoch());
        let quiet = log_lines(&log).iter().filter(|line| stamp_of(line).is_some_and(|stamp| stamp >= quiet_from && stamp <= quiet_to)).count();
        p.line(format!(
            "- the daemon with glide on, idle for a minute after its last glide: CPU used {:?} ms, working set and private bytes {:?}, threads {:?}, trace lines {quiet}",
            cpu_before.zip(cpu_after).map(|(before, after)| after.saturating_sub(before).as_millis()),
            memory(&child),
            threads(daemon)
        ));
        let stopped = stop_glide_daemon(&mut child);
        p.line(format!("- daemon stopped {stopped:?}"));
        pump(Duration::from_millis(800));
        let plain_log = p.out.join("glide-daemon-software.log");
        let mut plain = start_glide_daemon(p, &plain_log, false)?;
        let plain_pid = plain.id();
        let _ = wait_until(Duration::from_secs(30), || log_lines(&plain_log).iter().any(|line| line.contains(" tick ")));
        let first_try = watch_change(&plain_log, &screen, || drag_icon(name, folder, lower), false);
        pump(Duration::from_millis(1500));
        let second_try = watch_change(&plain_log, &screen, || drag_icon(name, folder, centre), false);
        pump(Duration::from_secs(3));
        let helpers = helper_children(plain_pid);
        let lines: Vec<String> = log_lines(&plain_log).into_iter().filter(|line| line.contains(" glide ")).map(|line| line.chars().take(180).collect()).collect();
        let skipped = lines.iter().any(|line| line.contains("glide skipped, software adapter"));
        let opened_plain = lines.iter().any(|line| line.contains(" glide open"));
        p.line(format!("- without SCOOCH_GLIDE_ANYWAY: new wallpapers {} and {}, glided {opened_plain}, logged skipped software adapter {skipped}, helper processes 3 s later {helpers:?}, log {lines:?}", first_try.new != first_try.old, second_try.new != second_try.old));
        if !skipped || opened_plain || !helpers.is_empty() || second_try.new == second_try.old {
            p.failed.push("without the override the daemon did not refuse the software adapter cleanly".into());
        }
        let stopped = stop_glide_daemon(&mut plain);
        p.line(format!("- daemon stopped {stopped:?}"));
        Ok(())
    }

    fn new_lines(path: &Path, read: &mut u64) -> Vec<String> {
        let Ok(bytes) = fs::read(path) else { return Vec::new() };
        let start = (*read as usize).min(bytes.len());
        *read = bytes.len() as u64;
        String::from_utf8_lossy(&bytes[start..]).lines().map(str::to_string).collect()
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

    fn encode_times(p: &mut Probe) {
        let scene = scooch_core::Scene { width: 3840, height: 2160, icons: Vec::new() };
        let source = p.wallpapers.iter().find_map(|(_, shown)| shown.clone()).and_then(|path| scooch_core::open(&path).ok());
        let photo = source.map(|image| {
            let placement = scooch_core::fill(image.width(), image.height(), &scene);
            scooch_core::render(&image, &placement, &scene, &scooch_core::Options::default())
        });
        let noise = detailed(3840, 2160);
        for (label, picture) in [("the default wallpaper", photo.as_ref()), ("the fine detail test picture", Some(&noise))] {
            let Some(picture) = picture else {
                p.line(format!("- no {label} to encode"));
                continue;
            };
            let started = Instant::now();
            let png = scooch_core::encode_png(picture).map(|bytes| bytes.len());
            let png_time = started.elapsed().as_millis();
            let started = Instant::now();
            let jpeg_size = jpeg(picture).map(|bytes| bytes.len());
            let jpeg_time = started.elapsed().as_millis();
            p.line(format!(
                "- 3840x2160 {label} PNG {png_time} ms {:?} bytes, JPEG quality 95 {jpeg_time} ms {:?} bytes",
                png.map_err(|error| error.to_string()),
                jpeg_size.map_err(|error| error.to_string())
            ));
        }
    }

    fn end_to_end(p: &mut Probe) -> Result<()> {
        encode_times(p);
        uncover(p);
        let desktop = platform::current()?;
        let screen = desktop.screens()?.first().context("no screen")?.clone();
        let twin = twin()?;
        let picture = p.home.join("probe-subject.jpg");
        fs::write(&picture, jpeg(&subject_picture(screen.frame.w as u32, screen.frame.h as u32))?)?;
        desktop.set_wallpaper(&screen, &picture)?;
        p.line(format!("- wallpaper set to a picture with one bright subject in the middle {}", picture.display()));
        let _ = click(empty_spot(), false);
        park_cursor();
        pump(Duration::from_millis(600));
        fs::write(p.home.join("state.json"), br#"{"version":1,"on":true}"#)?;
        let log = fs::File::create(p.out.join("scoochw.log"))?;
        let child = Command::new(&twin).arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).spawn()?;
        let process = child.id();
        p.line(format!("- started {} as process {process}", twin.display()));
        p.daemon = Some(child);
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(60), || rendered(&home));
        p.line(format!("- first render after {:?} ms", first.map(|time| time.as_millis())));
        if first.is_none() {
            p.failed.push("the daemon never rendered".into());
        }
        let view = shell::view()?;
        let (name, folder) = PROBES[0];
        let shown = shown_name(name, folder);
        let font = shell::Font::open(&view)?;
        let cell = shell::cell(&view, font.line)?;
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
        let subject = POINT { x: (screen.frame.x + screen.frame.w / 2.0) as i32, y: (screen.frame.y + screen.frame.h / 2.0) as i32 };
        let calm = POINT { x: (screen.frame.x + screen.frame.w * 0.8) as i32, y: (screen.frame.y + screen.frame.h * 0.2) as i32 };
        let mut times = Vec::new();
        let trace = p.out.join("scoochw.log");
        let mut read = fs::metadata(&trace).map_or(0, |meta| meta.len());
        for round in 0..6 {
            let before = desktop.wallpaper(&screen)?.unwrap_or_default();
            let Some(rect) = automation_rects(&automation, view.list)?.into_iter().find(|(seen, _)| *seen == shown).map(|(_, rect)| rect) else {
                p.line(format!("- drag {} found no UI Automation rect for the item", round + 1));
                break;
            };
            let from = POINT { x: (rect.x + rect.w / 2.0) as i32, y: (rect.y + cell.icon / 2.0) as i32 };
            let to = if round % 2 == 0 { subject } else { calm };
            let sent = drag_to(from, to);
            let dropped = Instant::now();
            let changed = wait_until(Duration::from_secs(10), || desktop.wallpaper(&screen).ok().flatten().is_some_and(|now| now != before));
            let latency = changed.map(|_| dropped.elapsed().as_millis());
            pump(Duration::from_millis(300));
            let landed = automation_rects(&automation, view.list)?.into_iter().find(|(seen, _)| *seen == shown).map(|(_, rect)| (rect.x + rect.w / 2.0, rect.y));
            let wakes = new_lines(&trace, &mut read).into_iter().filter(|line| line.contains(" wake ") || line.contains(" set ")).collect::<Vec<_>>();
            p.line(format!("- drag {} from {},{} to {},{} sent {sent}, item now at {:?}, new wallpaper after {:?} ms", round + 1, from.x, from.y, to.x, to.y, landed, latency));
            for line in &wakes {
                p.line(format!("  - daemon `{}`", line.chars().take(160).collect::<String>()));
            }
            if !wakes.iter().any(|line| line.contains(" wake ")) {
                p.failed.push(format!("drag {} woke nothing", round + 1));
            }
            match latency {
                Some(_) => times.extend(latency),
                None => p.failed.push(format!("drag {} brought no new wallpaper", round + 1)),
            }
            park_cursor();
            pump(Duration::from_millis(800));
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
        reattach(p, &trace, &mut read, &screen)?;
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
            if !shell::visible(&view) {
                let shown = toggle_icons(&view, true);
                p.line(format!("- desktop icons shown again {:?}", shown.map_err(|error| error.to_string())));
            }
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
        if let Some(shell) = p.shell.take() {
            let undone = unsafe { shell.UndoMinimizeALL() };
            p.line(format!("- windows restored {undone:?}"));
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
