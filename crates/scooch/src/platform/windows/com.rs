use anyhow::{bail, Result};
use std::cell::Cell;
use std::ffi::{c_void, OsString};
use std::os::windows::ffi::OsStringExt;
use std::sync::Once;
use ::windows::core::PWSTR;
use ::windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use ::windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED};
use ::windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use ::windows::Win32::UI::Shell::Common::ITEMIDLIST;

static AWARE: Once = Once::new();

thread_local! {
    static JOINED: Cell<bool> = const { Cell::new(false) };
}

pub fn aware() {
    AWARE.call_once(|| {
        let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    });
}

pub fn enter(single: bool) -> Result<()> {
    aware();
    if JOINED.with(Cell::get) {
        return Ok(());
    }
    let mode = if single { COINIT_APARTMENTTHREADED } else { COINIT_MULTITHREADED };
    let result = unsafe { CoInitializeEx(None, mode) };
    if result.is_err() && result != RPC_E_CHANGED_MODE {
        bail!("Scooch could not start the Windows component system, so restart it with scooch.");
    }
    JOINED.with(|joined| joined.set(true));
    Ok(())
}

pub fn ready() -> Result<()> {
    enter(false)
}

pub fn take(text: PWSTR) -> OsString {
    if text.is_null() {
        return OsString::new();
    }
    let owned = OsString::from_wide(unsafe { text.as_wide() });
    unsafe { CoTaskMemFree(Some(text.0 as *const c_void)) };
    owned
}

pub struct Pidl(pub *mut ITEMIDLIST);

impl Drop for Pidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CoTaskMemFree(Some(self.0 as *const c_void)) };
        }
    }
}
