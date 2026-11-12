use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
	<string>Background</string>
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

pub fn install(exe: &Path, data: &Path) -> Result<()> {
    let path = plist_path()?;
    remove()?;
    std::fs::create_dir_all(path.parent().context("Scooch could not find the LaunchAgents folder.")?)?;
    std::fs::create_dir_all(data)?;
    let home = std::env::var_os("SCOOCH_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    std::fs::write(&path, plist(exe, data, home.as_deref())).with_context(|| format!("Scooch could not write {}, so check that the folder is writable.", path.display()))?;
    if !launchctl(&["bootstrap", &domain(), &path.to_string_lossy()]) {
        let _ = std::fs::remove_file(&path);
        bail!("macOS did not accept the Scooch login item, so run scooch doctor to see why.");
    }
    Ok(())
}

pub fn remove() -> Result<()> {
    if loaded() {
        launchctl(&["bootout", &format!("{}/{LABEL}", domain())]);
    }
    let path = plist_path()?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("Scooch could not remove {}, so delete it by hand.", path.display()))?;
    }
    Ok(())
}

pub fn installed() -> bool {
    plist_path().is_ok_and(|path| path.exists())
}

pub fn loaded() -> bool {
    launchctl(&["print", &format!("{}/{LABEL}", domain())])
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
        assert!(text.contains("<key>ProcessType</key>\n\t<string>Background</string>"));
        assert!(text.contains(&format!("<string>{LABEL}</string>")));
        assert!(!text.contains("SCOOCH_HOME"));
    }

    #[test]
    fn a_custom_data_folder_is_passed_on() {
        let text = plist(Path::new("/opt/bin/scooch"), Path::new("/data"), Some(Path::new("/scratch/state")));
        assert!(text.contains("<key>SCOOCH_HOME</key>\n\t\t<string>/scratch/state</string>"));
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
