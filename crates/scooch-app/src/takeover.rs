use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ANSWER: Duration = Duration::from_secs(3);

fn places() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![home.join(".local/bin"), home.join("bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/opt/homebrew/bin")]
}

fn same(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn answers_as_scooch(path: &Path) -> bool {
    let Ok(mut child) = Command::new(path).arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return false };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return false,
            Ok(None) if started.elapsed() >= ANSWER => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
        }
    }
    let mut said = String::new();
    child.stdout.take().is_some_and(|mut out| out.read_to_string(&mut said).is_ok()) && said.starts_with("scooch ")
}

fn linked(stale: &Path, target: &Path) -> std::io::Result<()> {
    let staged = stale.with_file_name(".scooch.link");
    let _ = std::fs::remove_file(&staged);
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &staged)?;
    std::fs::rename(&staged, stale)
}

pub fn stale(program: &Path) -> Vec<PathBuf> {
    places()
        .into_iter()
        .map(|folder| folder.join("scooch"))
        .filter(|path| std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file()))
        .filter(|path| !same(path, program))
        .filter(|path| answers_as_scooch(path))
        .collect()
}

pub fn take_over(program: &Path, link: bool) -> Vec<String> {
    let mut done = Vec::new();
    for path in stale(program) {
        let result = if link { linked(&path, program) } else { std::fs::remove_file(&path) };
        if result.is_ok() {
            done.push(format!("{} {}", if link { "linked" } else { "removed" }, path.display()));
        }
    }
    done
}
