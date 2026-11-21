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

pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let Some(ready) = app.try_state::<Ready>() else { return };
    let taken = ready.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some((update, bytes)) = taken {
        let _ = update.install(bytes);
    }
}
