use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const LABEL: &str = "me.sohanbhat.scooch";

pub fn plist_path() -> Result<PathBuf> {
    Ok(super::real_home().context("Scooch could not find your home folder.")?.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

pub fn plist(exe: &Path, data: &Path, home: Option<&Path>) -> String {
    let environment = match home {
        Some(home) => format!("\t<key>EnvironmentVariables</key>\n\t<dict>\n\t\t<key>SCOOCH_HOME</key>\n\t\t<string>{}</string>\n\t</dict>\n", escape(&home.to_string_lossy())),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
		<string>watch</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>ProcessType</key>
	<string>Standard</string>
{environment}	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#,
        exe = escape(&exe.to_string_lossy()),
        log = escape(&data.join("watch.log").to_string_lossy()),
    )
}

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub trait Launchctl {
    fn run(&mut self, arguments: &[&str]) -> bool;
    fn pause(&mut self, length: Duration);
}

struct System;

impl Launchctl for System {
    fn run(&mut self, arguments: &[&str]) -> bool {
        launchctl(arguments)
    }

    fn pause(&mut self, length: Duration) {
        std::thread::sleep(length);
    }
}

const GONE_PATIENCE: Duration = Duration::from_secs(3);
const GONE_POLL: Duration = Duration::from_millis(100);
const RETRY_AFTER: Duration = Duration::from_secs(1);

pub fn unload(control: &mut dyn Launchctl, target: &str) -> bool {
    if !control.run(&["print", target]) {
        return true;
    }
    control.run(&["bootout", target]);
    let mut waited = Duration::ZERO;
    while waited < GONE_PATIENCE {
        if !control.run(&["print", target]) {
            return true;
        }
        control.pause(GONE_POLL);
        waited += GONE_POLL;
    }
    !control.run(&["print", target])
}

pub fn load(control: &mut dyn Launchctl, domain: &str, target: &str, plist: &Path) -> Result<()> {
    unload(control, target);
    let file = plist.to_string_lossy();
    if control.run(&["bootstrap", domain, &file]) {
        return Ok(());
    }
    control.pause(RETRY_AFTER);
    if control.run(&["bootstrap", domain, &file]) {
        return Ok(());
    }
    if !control.run(&["print", target]) {
        let _ = std::fs::remove_file(plist);
    }
    bail!("macOS did not accept the Scooch login item, so run scooch doctor to see why.");
}

pub fn install(exe: &Path, data: &Path) -> Result<()> {
    let path = plist_path()?;
    unload(&mut System, &target());
    std::fs::create_dir_all(path.parent().context("Scooch could not find the LaunchAgents folder.")?)?;
    std::fs::create_dir_all(data)?;
    let home = std::env::var_os("SCOOCH_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    std::fs::write(&path, plist(exe, data, home.as_deref())).with_context(|| format!("Scooch could not write {}, so check that the folder is writable.", path.display()))?;
    load(&mut System, &domain(), &target(), &path)
}

pub fn remove() -> Result<()> {
    let path = plist_path()?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("Scooch could not remove {}, so delete it by hand.", path.display()))?;
    }
    unload(&mut System, &target());
    Ok(())
}

fn target() -> String {
    format!("{}/{LABEL}", domain())
}

pub fn installed() -> bool {
    plist_path().is_ok_and(|path| path.exists())
}

pub fn loaded() -> bool {
    launchctl(&["print", &target()])
}

fn domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl(arguments: &[&str]) -> bool {
    Command::new("/bin/launchctl").args(arguments).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_item_runs_watch_in_the_background() {
        let text = plist(Path::new("/opt/bin/scooch"), Path::new("/data"), None);
        assert!(text.contains("<string>/opt/bin/scooch</string>\n\t\t<string>watch</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(text.contains("<key>SuccessfulExit</key>\n\t\t<false/>"));
        assert!(text.contains("<key>ProcessType</key>\n\t<string>Standard</string>"));
        assert!(text.contains(&format!("<string>{LABEL}</string>")));
        assert!(!text.contains("SCOOCH_HOME"));
    }

    #[test]
    fn a_custom_data_folder_is_passed_on() {
        let text = plist(Path::new("/opt/bin/scooch"), Path::new("/data"), Some(Path::new("/scratch/state")));
        assert!(text.contains("<key>SCOOCH_HOME</key>\n\t\t<string>/scratch/state</string>"));
    }

    struct Script {
        loaded: Vec<bool>,
        bootstraps: Vec<bool>,
        calls: Vec<String>,
        paused: Duration,
    }

    impl Launchctl for Script {
        fn run(&mut self, arguments: &[&str]) -> bool {
            self.calls.push(arguments[0].to_string());
            match arguments[0] {
                "print" => if self.loaded.len() > 1 { self.loaded.remove(0) } else { self.loaded[0] },
                "bootstrap" => self.bootstraps.remove(0),
                _ => true,
            }
        }

        fn pause(&mut self, length: Duration) {
            self.paused += length;
        }
    }

    fn script(loaded: &[bool], bootstraps: &[bool]) -> Script {
        Script { loaded: loaded.to_vec(), bootstraps: bootstraps.to_vec(), calls: Vec::new(), paused: Duration::ZERO }
    }

    #[test]
    fn a_new_job_waits_until_the_old_one_is_gone() {
        let mut control = script(&[true, true, true, false], &[true]);
        let folder = crate::state::scratch("waitgone");
        load(&mut control, "gui/501", "gui/501/x", &folder.join("x.plist")).unwrap();
        assert_eq!(control.calls, ["print", "bootout", "print", "print", "print", "bootstrap"]);
        assert_eq!(control.paused, Duration::from_millis(200));
    }

    #[test]
    fn waiting_for_the_old_job_stops_after_three_seconds() {
        let mut control = script(&[true], &[]);
        assert!(!unload(&mut control, "gui/501/x"));
        assert!(control.paused >= Duration::from_secs(3) && control.paused < Duration::from_millis(3200));
    }

    #[test]
    fn a_failed_bootstrap_is_tried_once_more() {
        let folder = crate::state::scratch("retry");
        let plist = folder.join("x.plist");
        std::fs::write(&plist, b"x").unwrap();
        let mut control = script(&[false], &[false, true]);
        load(&mut control, "gui/501", "gui/501/x", &plist).unwrap();
        assert_eq!(control.calls, ["print", "bootstrap", "bootstrap"]);
        assert_eq!(control.paused, Duration::from_secs(1));
        assert!(plist.exists());
    }

    #[test]
    fn the_plist_is_removed_only_when_the_job_is_not_loaded() {
        let folder = crate::state::scratch("keepplist");
        let plist = folder.join("x.plist");
        std::fs::write(&plist, b"x").unwrap();
        let mut loaded = script(&[false, true], &[false, false]);
        assert!(load(&mut loaded, "gui/501", "gui/501/x", &plist).is_err());
        assert!(plist.exists());
        let mut gone = script(&[false], &[false, false]);
        assert!(load(&mut gone, "gui/501", "gui/501/x", &plist).is_err());
        assert!(!plist.exists());
    }

    #[test]
    fn the_file_passes_plutil() {
        let folder = crate::state::scratch("plist");
        let file = folder.join("agent.plist");
        std::fs::write(&file, plist(Path::new("/Users/a&b/scooch"), &folder, Some(&folder))).unwrap();
        assert!(Command::new("/usr/bin/plutil").arg("-lint").arg(&file).stdout(Stdio::null()).status().unwrap().success());
    }

    #[test]
    fn odd_paths_stay_valid_xml() {
        let text = plist(Path::new("/Users/a&b/<scooch>"), Path::new("/data"), None);
        assert!(text.contains("/Users/a&amp;b/&lt;scooch&gt;"));
    }
}
