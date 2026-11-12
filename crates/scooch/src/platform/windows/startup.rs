use super::layout;
use anyhow::{bail, Context, Result};
use std::ffi::{c_void, OsString};
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use ::windows::core::{w, HSTRING, PCWSTR, PWSTR};
use ::windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use ::windows::Win32::System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ, RRF_RT_REG_SZ};
use ::windows::Win32::System::Threading::{CreateProcessW, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS, PROCESS_INFORMATION, STARTUPINFOW};

pub const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
pub const VALUE: PCWSTR = w!("Scooch");
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
    let program = program(exe);
    if !custom_home() {
        write(&layout::command_line(&program))?;
    }
    launch(&program)
}

pub fn remove() -> Result<()> {
    if custom_home() {
        return Ok(());
    }
    match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, VALUE) } {
        ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
        _ => bail!("Scooch could not remove its login entry, so delete Scooch under Startup apps in Settings."),
    }
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

pub fn read(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut size = 0u32;
    if unsafe { RegGetValueW(root, key, value, RRF_RT_REG_SZ, None, None, Some(&mut size)) } != ERROR_SUCCESS || size < 2 {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let mut size = (buffer.len() * size_of::<u16>()) as u32;
    if unsafe { RegGetValueW(root, key, value, RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr() as *mut c_void), Some(&mut size)) } != ERROR_SUCCESS {
        return None;
    }
    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
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
