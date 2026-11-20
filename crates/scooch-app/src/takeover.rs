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

pub fn answers_as_scooch(path: &Path) -> bool {
    let mut command = Command::new(path);
    #[cfg(target_os = "linux")]
    crate::appimage::apply(&mut command);
    let Ok(mut child) = command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return false };
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
    let staged = stale.with_file_name(format!(".scooch.link.{}", std::process::id()));
    let _ = std::fs::remove_file(&staged);
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &staged)?;
    std::fs::rename(&staged, stale)
}

fn copies() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![home.join(".local/bin/scooch"), scooch::facade::folder().join("bin").join("scooch")]
}

pub fn was_ours(target: &Path, copies: &[PathBuf]) -> bool {
    let text = target.to_string_lossy();
    text.ends_with(".app/Contents/MacOS/scooch") || copies.iter().any(|copy| copy == target)
}

fn dangling_ours(path: &Path, copies: &[PathBuf]) -> bool {
    let link = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    link && !path.exists() && std::fs::read_link(path).is_ok_and(|target| was_ours(&target, copies))
}

pub fn stale(program: &Path) -> Vec<PathBuf> {
    let copies = copies();
    places()
        .into_iter()
        .map(|folder| folder.join("scooch"))
        .filter(|path| {
            if dangling_ours(path, &copies) {
                return path != program;
            }
            std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file()) && !same(path, program) && answers_as_scooch(path)
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_to_a_gone_bundle_or_copy_count_as_ours() {
        let copies = vec![PathBuf::from("/home/ada/.local/bin/scooch"), PathBuf::from("/home/ada/.local/share/scooch/bin/scooch")];
        assert!(was_ours(Path::new("/Applications/Old/Scooch.app/Contents/MacOS/scooch"), &copies));
        assert!(was_ours(Path::new("/home/ada/.local/bin/scooch"), &copies));
        assert!(was_ours(Path::new("/home/ada/.local/share/scooch/bin/scooch"), &copies));
        assert!(!was_ours(Path::new("/opt/other/scooch"), &copies));
        let folder = std::env::temp_dir().join(format!("scooch-takeover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let gone = folder.join("Gone.app/Contents/MacOS/scooch");
        let link = folder.join("scooch");
        std::os::unix::fs::symlink(&gone, &link).unwrap();
        assert!(dangling_ours(&link, &copies));
        let other = folder.join("other");
        std::os::unix::fs::symlink(folder.join("missing"), &other).unwrap();
        assert!(!dangling_ours(&other, &copies));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
