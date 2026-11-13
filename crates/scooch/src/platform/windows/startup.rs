use super::layout;
use anyhow::{bail, Context, Result};
use std::ffi::{c_void, OsString};
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use ::windows::core::{w, HSTRING, PCWSTR, PWSTR};
use ::windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE};
use ::windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use ::windows::Win32::System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ};
use ::windows::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, OpenProcess, OpenProcessToken, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, STARTUPINFOW,
};
use ::windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

pub const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
pub const VALUE: PCWSTR = w!("Scooch");
pub const APPROVED: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
pub const ADMIN: &str = "Run scooch from a normal window, not as administrator.";
const VERSION: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");

pub fn custom_home() -> bool {
    std::env::var_os("SCOOCH_HOME").is_some_and(|value| !value.is_empty())
}

pub fn program(exe: &Path) -> PathBuf {
    let twin = layout::twin(exe);
    if twin.is_file() {
        twin
    } else {
        exe.to_path_buf()
    }
}

pub fn install(exe: &Path) -> Result<()> {
    if refused() {
        bail!(ADMIN);
    }
    let program = program(exe);
    if !custom_home() {
        write(&layout::command_line(&program))?;
        clear_approval()?;
    }
    launch(&program)
}

pub fn remove() -> Result<()> {
    if custom_home() {
        return Ok(());
    }
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, VALUE) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => clear_approval(),
        _ => bail!("Scooch could not remove its login entry, so delete Scooch under Startup apps in Settings."),
    }
}

pub fn clear_approval() -> Result<()> {
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVED, VALUE) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        _ => bail!("Scooch could not clear its Startup apps switch, so turn Scooch on under Startup apps in Settings."),
    }
}

pub fn approval() -> Option<Vec<u8>> {
    read_raw(HKEY_CURRENT_USER, APPROVED, VALUE, RRF_RT_REG_BINARY)
}

pub fn elevation(process: HANDLE) -> Option<bool> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    let read = unsafe { GetTokenInformation(token, TokenElevation, Some(&mut elevation as *mut TOKEN_ELEVATION as *mut c_void), size_of::<TOKEN_ELEVATION>() as u32, &mut returned) };
    let _ = unsafe { CloseHandle(token) };
    read.ok()?;
    Some(elevation.TokenIsElevated != 0)
}

pub fn own_elevation() -> bool {
    elevation(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

pub fn shell_elevation() -> Option<bool> {
    let shell = unsafe { GetShellWindow() };
    if shell.is_invalid() {
        return None;
    }
    let mut process = 0u32;
    if unsafe { GetWindowThreadProcessId(shell, Some(&mut process)) } == 0 || process == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.ok()?;
    let elevated = elevation(handle);
    let _ = unsafe { CloseHandle(handle) };
    elevated
}

pub fn refused() -> bool {
    let own = own_elevation();
    own && layout::refuse(own, shell_elevation())
}

fn write(data: &str) -> Result<()> {
    let wide: Vec<u16> = data.encode_utf16().chain(Some(0)).collect();
    let bytes = (wide.len() * size_of::<u16>()) as u32;
    let written = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, RUN, VALUE, REG_SZ.0, Some(wide.as_ptr() as *const c_void), bytes) };
    if written != ERROR_SUCCESS {
        bail!("Scooch could not add its login entry, so check that your account can change startup apps.");
    }
    Ok(())
}

fn read_raw(root: HKEY, key: PCWSTR, value: PCWSTR, kind: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
    let mut size = 0u32;
    if unsafe { RegGetValueW(root, key, value, kind, None, None, Some(&mut size)) } != ERROR_SUCCESS {
        return None;
    }
    let mut buffer = vec![0u8; (size as usize).max(2)];
    let mut size = buffer.len() as u32;
    if unsafe { RegGetValueW(root, key, value, kind, None, Some(buffer.as_mut_ptr() as *mut c_void), Some(&mut size)) } != ERROR_SUCCESS {
        return None;
    }
    buffer.truncate(size as usize);
    Some(buffer)
}

pub fn read(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<String> {
    let bytes = read_raw(root, key, value, RRF_RT_REG_SZ)?;
    let buffer: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    if length == 0 {
        return None;
    }
    Some(OsString::from_wide(&buffer[..length]).to_string_lossy().into_owned())
}

pub fn entry() -> Option<String> {
    read(HKEY_CURRENT_USER, RUN, VALUE)
}

pub fn build() -> Option<u32> {
    read(HKEY_LOCAL_MACHINE, VERSION, w!("CurrentBuildNumber"))?.trim().parse().ok()
}

pub fn launch(program: &Path) -> Result<()> {
    let plain = layout::plain(program);
    let application = HSTRING::from(plain.as_path());
    let mut line: Vec<u16> = layout::command_line(&plain).encode_utf16().chain(Some(0)).collect();
    let folder = plain.parent().filter(|folder| !folder.as_os_str().is_empty()).map(HSTRING::from);
    let folder = folder.as_ref().map_or(PCWSTR::null(), |folder| PCWSTR(folder.as_ptr()));
    let startup = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut process = PROCESS_INFORMATION::default();
    let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    unsafe { CreateProcessW(&application, Some(PWSTR(line.as_mut_ptr())), None, None, false, flags, None, folder, &startup, &mut process) }
        .with_context(|| format!("Scooch could not start {}, so run scooch again.", plain.display()))?;
    let _ = unsafe { CloseHandle(process.hThread) };
    let _ = unsafe { CloseHandle(process.hProcess) };
    Ok(())
}
