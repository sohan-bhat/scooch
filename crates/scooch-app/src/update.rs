use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

struct Ready(Mutex<Option<(Update, Vec<u8>)>>);

pub fn fetch<R: Runtime>(app: &AppHandle<R>) {
    app.manage(Ready(Mutex::new(None)));
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(updater) = handle.updater() else { return };
        let Ok(Some(update)) = updater.check().await else { return };
        let Ok(bytes) = update.download(|_, _| {}, || {}).await else { return };
        if let Some(ready) = handle.try_state::<Ready>() {
            *ready.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((update, bytes));
        }
    });
}

pub fn waiting<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<Ready>().is_some_and(|ready| ready.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some())
}

pub fn install<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Some(ready) = app.try_state::<Ready>() else { return false };
    let taken = ready.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    taken.is_some_and(|(update, bytes)| update.install(bytes).is_ok())
}
