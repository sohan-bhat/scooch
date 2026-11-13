use super::{com, shell, sta, startup, wakes, Wake};
use anyhow::{bail, Context, Result};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ::windows::core::{w, PCWSTR};
use ::windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WAIT_OBJECT_0, WPARAM, ERROR_SUCCESS};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::System::Registry::{RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME};
use ::windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use ::windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
use ::windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use ::windows::Win32::UI::Shell::Common::ITEMIDLIST;
use ::windows::Win32::UI::Shell::{
    SHChangeNotification_Lock, SHChangeNotification_Unlock, SHChangeNotifyDeregister, SHChangeNotifyEntry, SHChangeNotifyRegister, SHGetSpecialFolderLocation, CSIDL_DESKTOP, SHCNE_CREATE, SHCNE_DELETE,
    SHCNE_MKDIR, SHCNE_RENAMEFOLDER, SHCNE_RENAMEITEM, SHCNE_RMDIR, SHCNE_UPDATEDIR, SHCNRF_InterruptLevel, SHCNRF_NewDelivery, SHCNRF_ShellLevel,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowThreadProcessId, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, TranslateMessage, MSG,
    WINDOW_EX_STYLE, WINDOW_STYLE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_APP, WM_CLOSE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ENDSESSION, WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_SETTINGCHANGE,
    WM_WTSSESSION_CHANGE, WNDCLASSEXW,
};

type Shared = Arc<dyn Fn() + Send + Sync>;

pub const CLASS: PCWSTR = w!("ScoochWatch");
pub const CHANGED: u32 = WM_APP + 1;
pub const REATTACH: u32 = WM_APP + 2;
pub const FOUND: u32 = WM_APP + 3;
const DESKTOPS: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops");
const PAUSE: Duration = Duration::from_secs(1);

const _: () = assert!(wakes::SPI_ICONHORIZONTALSPACING == ::windows::Win32::UI::WindowsAndMessaging::SPI_ICONHORIZONTALSPACING.0);
const _: () = assert!(wakes::SPI_SETDESKWALLPAPER == ::windows::Win32::UI::WindowsAndMessaging::SPI_SETDESKWALLPAPER.0);
const _: () = assert!(wakes::SPI_SETICONTITLELOGFONT == ::windows::Win32::UI::WindowsAndMessaging::SPI_SETICONTITLELOGFONT.0);
const _: () = assert!(wakes::SPI_SETICONMETRICS == ::windows::Win32::UI::WindowsAndMessaging::SPI_SETICONMETRICS.0);
const _: () = assert!(wakes::SPI_SETWORKAREA == ::windows::Win32::UI::WindowsAndMessaging::SPI_SETWORKAREA.0);
const _: () = assert!(wakes::EVENT_SYSTEM_CAPTUREEND == ::windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_CAPTUREEND);
const _: () = assert!(wakes::EVENT_OBJECT_CREATE == ::windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_CREATE);
const _: () = assert!(wakes::EVENT_OBJECT_DESTROY == ::windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_DESTROY);
const _: () = assert!(wakes::EVENT_OBJECT_SHOW == ::windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_SHOW);
const _: () = assert!(wakes::EVENT_OBJECT_HIDE == ::windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_HIDE);
const _: () = assert!(wakes::OBJID_WINDOW == ::windows::Win32::UI::WindowsAndMessaging::OBJID_WINDOW.0);
const _: () = assert!(wakes::EVENT_OBJECT_REORDER == ::windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_REORDER);
const _: () = assert!(wakes::EVENT_OBJECT_LOCATIONCHANGE == ::windows::Win32::UI::WindowsAndMessaging::EVENT_OBJECT_LOCATIONCHANGE);
const _: () = assert!(wakes::OBJID_CLIENT == ::windows::Win32::UI::WindowsAndMessaging::OBJID_CLIENT.0);
const _: () = assert!(wakes::WTS_SESSION_LOGON as u32 == ::windows::Win32::UI::WindowsAndMessaging::WTS_SESSION_LOGON);
const _: () = assert!(wakes::WTS_SESSION_UNLOCK as u32 == ::windows::Win32::UI::WindowsAndMessaging::WTS_SESSION_UNLOCK);
const _: () = assert!(wakes::PBT_APMRESUMEAUTOMATIC as u32 == ::windows::Win32::UI::WindowsAndMessaging::PBT_APMRESUMEAUTOMATIC);

thread_local! {
    static WAKE: RefCell<Option<Shared>> = const { RefCell::new(None) };
    static LIST: Cell<isize> = const { Cell::new(0) };
    static VIEW: Cell<isize> = const { Cell::new(0) };
    static PENDING: Cell<bool> = const { Cell::new(false) };
    static TASKBAR: Cell<u32> = const { Cell::new(0) };
    static NOTIFY: Cell<u32> = const { Cell::new(0) };
    static HOOKS: RefCell<Vec<HWINEVENTHOOK>> = const { RefCell::new(Vec::new()) };
}

static RETRY: Mutex<wakes::Retry> = Mutex::new(wakes::Retry::new());
static ATTACHED: AtomicBool = AtomicBool::new(false);

fn wake(cause: &str) {
    if crate::daemon::trace() {
        eprintln!("{} wake {cause}", crate::daemon::stamp());
    }
    WAKE.with(|slot| {
        if let Some(wake) = slot.borrow().as_ref() {
            wake();
        }
    });
}

pub fn run(wake: Wake) -> Result<()> {
    if startup::refused() {
        bail!(startup::ADMIN);
    }
    com::enter(true)?;
    let wake: Shared = Arc::from(wake);
    WAKE.with(|slot| *slot.borrow_mut() = Some(wake.clone()));
    let window = window()?;
    TASKBAR.with(|taskbar| taskbar.set(unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }));
    let sessions = unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) }.is_ok();
    desktops(wake);
    register(window);
    request(window);
    retry(window);
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
    detach();
    if sessions {
        let _ = unsafe { WTSUnRegisterSessionNotification(window) };
    }
    let _ = unsafe { DestroyWindow(window) };
    WAKE.with(|slot| *slot.borrow_mut() = None);
    Ok(())
}

fn window() -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }.context("Scooch could not find its own program, so reinstall it.")?;
    let class = WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: instance.into(), lpszClassName: CLASS, ..Default::default() };
    unsafe { RegisterClassExW(&class) };
    unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("Scooch"), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, Some(instance.into()), None) }
        .context("Scooch could not open its hidden window, so restart it with scooch.")
}

fn request(window: HWND) {
    if PENDING.with(|pending| pending.replace(true)) {
        return;
    }
    let target = window.0 as isize;
    let sent = sta::post(move || {
        let (list, host) = shell::view().map_or((0, 0), |view| (view.list.0 as isize, view.host.0 as isize));
        let _ = unsafe { PostMessageW(Some(HWND(target as *mut c_void)), FOUND, WPARAM(list as usize), LPARAM(host)) };
    });
    if !sent {
        PENDING.with(|pending| pending.set(false));
    }
}

fn attach(window: HWND, list: isize, host: isize) -> bool {
    replace_hooks(Vec::new());
    LIST.with(|slot| slot.set(0));
    VIEW.with(|slot| slot.set(0));
    let list_window = HWND(list as *mut c_void);
    let hooks = if list == 0 { Vec::new() } else { hook(list_window) };
    let attached = !hooks.is_empty();
    if attached {
        LIST.with(|slot| slot.set(list));
        VIEW.with(|slot| slot.set(host));
        replace_hooks(hooks);
        register(window);
    }
    ATTACHED.store(attached, Ordering::SeqCst);
    attached
}

fn replace_hooks(hooks: Vec<HWINEVENTHOOK>) {
    let old = HOOKS.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), hooks));
    for hook in old {
        let _ = unsafe { UnhookWinEvent(hook) };
    }
}

fn detach() {
    replace_hooks(Vec::new());
    LIST.with(|list| list.set(0));
    VIEW.with(|host| host.set(0));
    let old = NOTIFY.with(|notify| notify.replace(0));
    if old != 0 {
        let _ = unsafe { SHChangeNotifyDeregister(old) };
    }
}

pub fn hook(list: HWND) -> Vec<HWINEVENTHOOK> {
    let mut process = 0u32;
    let thread = unsafe { GetWindowThreadProcessId(list, Some(&mut process)) };
    if thread == 0 {
        return Vec::new();
    }
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    wakes::HOOKS.iter().map(|(low, high)| unsafe { SetWinEventHook(*low, *high, None, Some(event), process, thread, flags) }).filter(|hook| !hook.is_invalid()).collect()
}

fn retry(window: HWND) {
    if !RETRY.lock().is_ok_and(|mut retry| retry.begin()) {
        return;
    }
    let target = window.0 as isize;
    let spawned = std::thread::Builder::new().name("scooch-find".into()).spawn(move || loop {
        std::thread::sleep(PAUSE);
        if !RETRY.lock().is_ok_and(|mut retry| retry.next(ATTACHED.load(Ordering::SeqCst))) {
            break;
        }
        let _ = unsafe { PostMessageW(Some(HWND(target as *mut c_void)), REATTACH, WPARAM(0), LPARAM(0)) };
    });
    if spawned.is_err() {
        if let Ok(mut retry) = RETRY.lock() {
            retry.stop();
        }
    }
}

fn register(window: HWND) {
    let Ok(root) = (unsafe { SHGetSpecialFolderLocation(None, CSIDL_DESKTOP as i32) }) else { return };
    let root = com::Pidl(root);
    let entry = SHChangeNotifyEntry { pidl: root.0, fRecursive: false.into() };
    let events = SHCNE_CREATE | SHCNE_DELETE | SHCNE_MKDIR | SHCNE_RMDIR | SHCNE_RENAMEITEM | SHCNE_RENAMEFOLDER | SHCNE_UPDATEDIR;
    let sources = SHCNRF_ShellLevel | SHCNRF_InterruptLevel | SHCNRF_NewDelivery;
    let id = unsafe { SHChangeNotifyRegister(window, sources, events.0 as i32, CHANGED, 1, &entry) };
    let old = NOTIFY.with(|notify| notify.replace(id));
    if old != 0 && old != id {
        let _ = unsafe { SHChangeNotifyDeregister(old) };
    }
}

fn changed(wparam: WPARAM, lparam: LPARAM) {
    let mut pidls: *mut *mut ITEMIDLIST = std::ptr::null_mut();
    let mut kind = 0i32;
    let lock = unsafe { SHChangeNotification_Lock(HANDLE(wparam.0 as *mut c_void), lparam.0 as u32, Some(&mut pidls), Some(&mut kind)) };
    if !lock.is_invalid() {
        let _ = unsafe { SHChangeNotification_Unlock(lock) };
    }
}

fn desktops(wake: Shared) {
    let _ = std::thread::Builder::new().name("scooch-desktops".into()).spawn(move || {
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, DESKTOPS, None, KEY_NOTIFY, &mut key) } != ERROR_SUCCESS {
            return;
        }
        if let Ok(signal) = unsafe { CreateEventW(None, false, false, PCWSTR::null()) } {
            loop {
                let filter = REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME;
                if unsafe { RegNotifyChangeKeyValue(key, true, filter, Some(signal), true) } != ERROR_SUCCESS {
                    break;
                }
                if unsafe { WaitForSingleObject(signal, INFINITE) } != WAIT_OBJECT_0 {
                    break;
                }
                if !super::set_recently() {
                    wake();
                }
            }
        }
        let _ = unsafe { RegCloseKey(key) };
    });
}

unsafe extern "system" fn event(_hook: HWINEVENTHOOK, kind: u32, window: HWND, object: i32, child: i32, _thread: u32, _time: u32) {
    let handle = window.0 as isize;
    let on_list = LIST.with(|list| list.get() != 0 && list.get() == handle);
    let on_view = VIEW.with(|host| host.get() != 0 && host.get() == handle);
    if wakes::event(kind, on_list, on_view, object, child) {
        wake("list");
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_SETTINGCHANGE => {
            if wakes::setting(wparam.0, super::set_recently()) {
                wake("setting");
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            wake("display");
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            if wakes::session(wparam.0) {
                wake("session");
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wakes::power(wparam.0) {
                wake("resume");
            }
            LRESULT(1)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                unsafe { PostQuitMessage(0) };
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        CHANGED => {
            changed(wparam, lparam);
            wake("files");
            LRESULT(0)
        }
        REATTACH => {
            request(window);
            LRESULT(0)
        }
        FOUND => {
            PENDING.with(|pending| pending.set(false));
            if attach(window, wparam.0 as isize, lparam.0) {
                wake("explorer");
            }
            LRESULT(0)
        }
        other if other != 0 && other == TASKBAR.with(Cell::get) => {
            ATTACHED.store(false, Ordering::SeqCst);
            request(window);
            retry(window);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}
