use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const RETRIES: [u64; 3] = [1, 5, 30];

pub const GNOME_KEYS: [&str; 3] = ["/org/gnome/desktop/background/", "/org/gnome/desktop/interface/color-scheme", "/org/gnome/shell/extensions/ding/"];
pub const CINNAMON_KEYS: [&str; 2] = ["/org/cinnamon/desktop/background/", "/org/nemo/desktop/"];
pub const MATE_KEYS: [&str; 3] = ["/org/mate/desktop/background/", "/org/mate/caja/desktop/", "/org/mate/caja/icon-view/"];
pub const GVFS_RULE: &str = "type='signal',interface='org.gtk.vfs.Metadata',member='AttributeChanged'";
pub const DCONF_RULE: &str = "type='signal',interface='ca.desrt.dconf.Writer',member='Notify'";
pub const XFCONF_RULE: &str = "type='signal',interface='org.xfce.Xfconf',member='PropertyChanged'";
pub const DING_RULE: &str = "type='signal',interface='org.gtk.Actions',member='Changed',path='/com/rastersoft/dingextension/control'";

pub fn desktop_entry(name: &str) -> bool {
    !name.starts_with('.') || name == ".hidden"
}

pub fn metadata(tree: &str, path: &str, desktop_in_home: Option<&str>) -> bool {
    let tree_name = Path::new(tree).file_name().and_then(|name| name.to_str()).unwrap_or(tree);
    if tree_name != "home" {
        return path == "/";
    }
    if path == "/" {
        return true;
    }
    let Some(desktop) = desktop_in_home else { return false };
    let Some(rest) = path.strip_prefix(desktop).and_then(|rest| rest.strip_prefix('/')) else { return false };
    !rest.is_empty() && !rest.contains('/')
}

pub fn dconf(prefix: &str, changes: &[String], watched: &[&str]) -> bool {
    let hit = |full: &str| watched.iter().any(|key| full.starts_with(key) || key.starts_with(full));
    if changes.is_empty() {
        return hit(prefix);
    }
    changes.iter().any(|change| hit(&format!("{prefix}{change}")))
}

pub fn xfconf(channel: &str, property: &str) -> bool {
    channel == super::xfce::CHANNEL && (property.starts_with("/backdrop/") || property.starts_with("/desktop-icons/"))
}

pub fn wallpaper_property(channel: &str, property: &str) -> bool {
    channel == super::xfce::CHANNEL && property.starts_with("/backdrop/")
}

pub fn kde_file(name: &str) -> bool {
    name == super::kde::APPLETS || name == super::kde::SHELL
}

pub fn desktop_metadata(name: &str) -> bool {
    name == super::nemo::METADATA_FILE
}

pub fn xfce_file(name: &str) -> bool {
    name == super::xfce::YAML_FILE || (name.starts_with("icons.screen") && name.ends_with(".rc"))
}

pub fn retry_delay(failures: usize) -> Option<Duration> {
    RETRIES.get(failures).map(|seconds| Duration::from_secs(*seconds))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    pub folder: PathBuf,
    pub waiting: Option<OsString>,
}

pub fn nearest(target: &Path, exists: impl Fn(&Path) -> bool) -> Option<Spot> {
    if exists(target) {
        return Some(Spot { folder: target.to_path_buf(), waiting: None });
    }
    let mut child = target;
    let mut parent = target.parent()?;
    loop {
        if exists(parent) {
            return Some(Spot { folder: parent.to_path_buf(), waiting: child.file_name().map(OsString::from) });
        }
        child = parent;
        parent = parent.parent()?;
    }
}

pub fn desktop_in_home(desktop: &Path, home: &Path) -> Option<String> {
    let rest = desktop.strip_prefix(home).ok()?;
    let text = rest.to_str()?;
    Some(if text.is_empty() { String::new() } else { format!("/{text}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_desktop_files_do_not_wake() {
        assert!(desktop_entry("notes.txt"));
        assert!(desktop_entry(".hidden"));
        assert!(!desktop_entry(".goutputstream-ABC"));
    }

    #[test]
    fn metadata_changes_wake_only_for_the_desktop_and_home() {
        let desktop = Some("/Desktop");
        let tree = "/home/ada/.local/share/gvfs-metadata/home";
        assert!(metadata(tree, "/Desktop/a.txt", desktop));
        assert!(metadata(tree, "/", desktop));
        assert!(!metadata(tree, "/Desktop", desktop));
        assert!(!metadata(tree, "/Desktop/folder/inner.txt", desktop));
        assert!(!metadata(tree, "/Documents/a.txt", desktop));
        assert!(metadata("/home/ada/.local/share/gvfs-metadata/uuid-1234", "/", desktop));
        assert!(!metadata("/home/ada/.local/share/gvfs-metadata/uuid-1234", "/x", desktop));
    }

    #[test]
    fn dconf_changes_match_by_prefix() {
        let keys = GNOME_KEYS;
        assert!(dconf("/org/gnome/desktop/background/", &["picture-uri".into(), "picture-uri-dark".into()], &keys));
        assert!(dconf("/org/gnome/desktop/background/picture-uri", &["".into()], &keys));
        assert!(dconf("/org/gnome/desktop/interface/color-scheme", &[], &keys));
        assert!(dconf("/org/gnome/", &["desktop/background/picture-options".into()], &keys));
        assert!(!dconf("/org/gnome/desktop/interface/", &["clock-format".into()], &keys));
        assert!(!dconf("/org/gnome/terminal/", &["x".into()], &keys));
    }

    #[test]
    fn xfconf_changes_wake_for_the_desktop_channel() {
        assert!(xfconf("xfce4-desktop", "/backdrop/screen0/monitorVirtual-1/workspace0/last-image"));
        assert!(xfconf("xfce4-desktop", "/desktop-icons/icon-size"));
        assert!(!xfconf("xfce4-panel", "/backdrop/x"));
        assert!(wallpaper_property("xfce4-desktop", "/backdrop/x") && !wallpaper_property("xfce4-desktop", "/desktop-icons/x"));
    }

    #[test]
    fn config_files_are_matched_by_name() {
        assert!(kde_file("plasma-org.kde.plasma.desktop-appletsrc"));
        assert!(!kde_file("plasma-org.kde.plasma.desktop-appletsrc.lock"));
        assert!(xfce_file("icons.screen0.yaml"));
        assert!(xfce_file("icons.screen0-1904x1064.rc"));
        assert!(xfce_file("icons.screen.latest.rc"));
        assert!(!xfce_file("icons.screen0.yaml.new"));
        assert!(desktop_metadata("desktop-metadata"));
        assert!(!desktop_metadata("desktop-metadata.LT1Q2"));
    }

    #[test]
    fn retries_wait_one_five_and_thirty_seconds_then_stop() {
        let waits: Vec<Option<Duration>> = (0..5).map(retry_delay).collect();
        assert_eq!(waits, vec![Some(Duration::from_secs(1)), Some(Duration::from_secs(5)), Some(Duration::from_secs(30)), None, None]);
    }

    #[test]
    fn a_missing_folder_is_watched_through_its_nearest_parent() {
        let present = [PathBuf::from("/home/ada"), PathBuf::from("/home"), PathBuf::from("/")];
        let exists = |path: &Path| present.iter().any(|known| known == path);
        assert_eq!(nearest(Path::new("/home/ada"), exists), Some(Spot { folder: PathBuf::from("/home/ada"), waiting: None }));
        assert_eq!(nearest(Path::new("/home/ada/.config/nemo"), exists), Some(Spot { folder: PathBuf::from("/home/ada"), waiting: Some(".config".into()) }));
        assert_eq!(nearest(Path::new("/home/ada/Desktop"), exists), Some(Spot { folder: PathBuf::from("/home/ada"), waiting: Some("Desktop".into()) }));
        assert_eq!(nearest(Path::new("relative"), |_| false), None);
    }

    #[test]
    fn the_desktop_is_named_inside_home() {
        assert_eq!(desktop_in_home(Path::new("/home/ada/Desktop"), Path::new("/home/ada")), Some("/Desktop".into()));
        assert_eq!(desktop_in_home(Path::new("/srv/desk"), Path::new("/home/ada")), None);
    }
}
