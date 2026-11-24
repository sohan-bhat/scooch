use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const NAME: &str = "icon.lock";
const STEP: Duration = Duration::from_millis(50);

pub struct Held {
    _file: File,
}

pub fn path(folder: &Path) -> PathBuf {
    folder.join(NAME)
}

pub fn take(folder: &Path) -> Option<Held> {
    let _ = std::fs::create_dir_all(folder);
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(path(folder)).ok()?;
    file.try_lock().ok()?;
    Some(Held { _file: file })
}

pub fn wait(folder: &Path, patience: Duration) -> Option<Held> {
    let deadline = Instant::now() + patience;
    loop {
        if let Some(held) = take(folder) {
            return Some(held);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(STEP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("scooch-app-lock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn one_holder_at_a_time_and_the_next_after_it_ends() {
        let folder = folder("one");
        let first = take(&folder).expect("the first start gets the lock");
        assert!(take(&folder).is_none(), "a second start got the lock while the first held it");
        drop(first);
        assert!(take(&folder).is_some(), "the lock stayed taken after its holder ended");
    }

    #[test]
    fn a_waiting_start_gets_the_lock_once_it_is_free_and_gives_up_after_its_patience() {
        let folder = folder("wait");
        let first = take(&folder).unwrap();
        let started = Instant::now();
        assert!(wait(&folder, Duration::from_millis(300)).is_none());
        assert!(started.elapsed() >= Duration::from_millis(300));
        let freed = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(first);
        });
        let started = Instant::now();
        assert!(wait(&folder, Duration::from_secs(5)).is_some());
        assert!(started.elapsed() < Duration::from_secs(2));
        freed.join().unwrap();
    }
}
