use notify::event::{AccessKind, AccessMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

const QUIET: Duration = Duration::from_millis(120);

pub fn changes(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

pub fn follow(folder: &Path, changed: impl Fn() + Send + 'static) -> notify::Result<RecommendedWatcher> {
    let _ = std::fs::create_dir_all(folder);
    let (send, receive) = mpsc::channel::<()>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| changes(&event.kind)) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn opening_and_reading_are_no_change_and_every_write_is_one() {
        assert!(!changes(&EventKind::Access(AccessKind::Open(AccessMode::Any))));
        assert!(!changes(&EventKind::Access(AccessKind::Read)));
        assert!(!changes(&EventKind::Access(AccessKind::Close(AccessMode::Read))));
        assert!(changes(&EventKind::Access(AccessKind::Close(AccessMode::Write))));
        assert!(changes(&EventKind::Modify(ModifyKind::Data(DataChange::Any))));
        assert!(changes(&EventKind::Create(CreateKind::File)));
        assert!(changes(&EventKind::Remove(RemoveKind::File)));
    }

    #[test]
    fn a_follower_that_reads_the_folder_does_not_wake_itself() {
        let folder = std::env::temp_dir().join(format!("scooch-app-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let state = folder.join("state.json");
        std::fs::write(&state, b"{}").unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let woke = Arc::new(AtomicUsize::new(0));
        let (count, read) = (woke.clone(), state.clone());
        let _watcher = follow(&folder, move || {
            count.fetch_add(1, Ordering::SeqCst);
            let _ = std::fs::read(&read);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::read(&state);
        std::thread::sleep(Duration::from_millis(800));
        let after_read = woke.load(Ordering::SeqCst);
        std::fs::write(&state, b"{\"on\": true}").unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        let after_write = woke.load(Ordering::SeqCst);
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(after_read, 0);
        assert!((1..=2).contains(&after_write), "{after_write}");
    }
}
