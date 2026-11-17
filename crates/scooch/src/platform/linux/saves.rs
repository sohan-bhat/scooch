use std::fmt::Display;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const REFUSED: &str = "Scooch could not listen for icon saves on the session bus, so it waits for the desktop's own notice.";

pub struct Saves {
    last: Mutex<Option<Instant>>,
}

impl Saves {
    pub const fn new() -> Self {
        Self { last: Mutex::new(None) }
    }

    pub fn heard(&self) {
        *self.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
    }

    pub fn last(&self) -> Option<Instant> {
        *self.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for Saves {
    fn default() -> Self {
        Self::new()
    }
}

pub static SAVES: Saves = Saves::new();

pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub fn hear<I: Iterator<Item = bool>, E: Display>(opened: Result<I, E>, saves: &Saves, wake: &Waker, log: &mut dyn FnMut(String)) {
    let calls = match opened {
        Ok(calls) => calls,
        Err(error) => {
            log(format!("{REFUSED} {error}"));
            return;
        }
    };
    for wanted in calls {
        if wanted {
            saves.heard();
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn counter() -> (Arc<AtomicUsize>, Waker) {
        let count = Arc::new(AtomicUsize::new(0));
        let counted = count.clone();
        (count, Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }))
    }

    #[test]
    fn a_refused_monitor_logs_once_and_hears_nothing() {
        let saves = Saves::new();
        let (woken, wake) = counter();
        let mut lines = Vec::new();
        hear(Err::<std::iter::Empty<bool>, _>("org.freedesktop.DBus.Error.AccessDenied"), &saves, &wake, &mut |line| lines.push(line));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with(REFUSED));
        assert_eq!((woken.load(Ordering::SeqCst), saves.last()), (0, None));
    }

    #[test]
    fn each_wanted_call_wakes_after_its_time_is_kept() {
        let saves = Saves::new();
        let (woken, wake) = counter();
        let mut lines = Vec::new();
        let before = Instant::now();
        hear(Ok::<_, String>([true, false, true].into_iter()), &saves, &wake, &mut |line| lines.push(line));
        assert_eq!(woken.load(Ordering::SeqCst), 2);
        assert!(lines.is_empty());
        assert!(saves.last().is_some_and(|at| at >= before));
    }
}
