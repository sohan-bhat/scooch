use std::collections::HashSet;
use std::sync::Mutex;

static WAITING: Mutex<Option<HashSet<i32>>> = Mutex::new(None);

fn parent_of(status: &str) -> Option<i32> {
    status.lines().find_map(|line| line.strip_prefix("PPid:")).and_then(|value| value.trim().parse().ok())
}

fn runs_watch(pid: i32) -> bool {
    std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|bytes| bytes.split(|byte| *byte == 0).any(|part| part == b"watch"))
}

fn own_daemons() -> Vec<i32> {
    let me = std::process::id() as i32;
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok()))
        .filter(|pid| std::fs::read_to_string(format!("/proc/{pid}/status")).ok().and_then(|status| parent_of(&status)) == Some(me))
        .filter(|pid| runs_watch(*pid))
        .collect()
}

pub fn reap_daemons() {
    for pid in own_daemons() {
        let mut waiting = WAITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !waiting.get_or_insert_with(HashSet::new).insert(pid) {
            continue;
        }
        drop(waiting);
        let _ = std::thread::Builder::new().name("scooch-app-reap".into()).spawn(move || {
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
            if let Some(waiting) = WAITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).as_mut() {
                waiting.remove(&pid);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parent_is_read_from_the_status_file() {
        assert_eq!(parent_of("Name:\tscooch\nPPid:\t4242\nUid:\t1000\n"), Some(4242));
        assert_eq!(parent_of("Name:\tscooch\n"), None);
    }
}
