use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

const QUIET: Duration = Duration::from_millis(120);

pub fn follow(folder: &Path, changed: impl Fn() + Send + 'static) -> notify::Result<RecommendedWatcher> {
    let _ = std::fs::create_dir_all(folder);
    let (send, receive) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok() {
            let _ = send.send(());
        }
    })?;
    watcher.watch(folder, RecursiveMode::NonRecursive)?;
    std::thread::Builder::new()
        .name("scooch-app-watch".into())
        .spawn(move || {
            while receive.recv().is_ok() {
                while receive.recv_timeout(QUIET).is_ok() {}
                changed();
            }
        })
        .map_err(|error| notify::Error::generic(&error.to_string()))?;
    Ok(watcher)
}
