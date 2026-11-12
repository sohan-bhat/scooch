use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub name: Vec<String>,
    pub entries: BTreeMap<String, String>,
}

impl Group {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn number(&self, key: &str) -> Option<i64> {
        self.get(key)?.trim().parse().ok()
    }

    pub fn joined(&self) -> String {
        self.name.join("][")
    }
}

pub fn parse(text: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.trim_end().ends_with(']') {
            let inner = &line.trim_end()[1..line.trim_end().len() - 1];
            let name: Vec<String> = inner.split("][").map(str::to_string).collect();
            match groups.iter().position(|group| group.name == name) {
                Some(index) => {
                    let group = groups.remove(index);
                    groups.push(group);
                }
                None => groups.push(Group { name, entries: BTreeMap::new() }),
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim_end();
        let key = match key.find('[') {
            Some(start) if key[start..].starts_with("[$") => &key[..start],
            Some(_) => continue,
            None => key,
        };
        let Some(group) = groups.last_mut() else { continue };
        group.entries.insert(key.to_string(), unescape(value.trim_start()));
    }
    groups
}

pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(letter) = chars.next() {
        if letter != '\\' {
            out.push(letter);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('x') => {
                let high = chars.next().and_then(|digit| digit.to_digit(16));
                let low = chars.next().and_then(|digit| digit.to_digit(16));
                match (high, low) {
                    (Some(high), Some(low)) => out.push(char::from_u32(high * 16 + low).unwrap_or('?')),
                    _ => out.push('?'),
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub fn find<'a>(groups: &'a [Group], name: &[&str]) -> Option<&'a Group> {
    groups.iter().find(|group| group.name.len() == name.len() && group.name.iter().zip(name).all(|(a, b)| a == b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPLETS: &str = "[ActionPlugins][0]\nRightButton;NoModifier=org.kde.contextmenu\n\n[Containments][1]\nactivityId=8f2b\nformfactor=0\nlastScreen=0\nplugin=org.kde.plasma.folder\nwallpaperplugin=org.kde.image\n\n[Containments][1][General]\npositions={\"1920x1080\":[\"1\"\\,\"8\"\\,\"desktop:/a.txt\"\\,\"0\"\\,\"2\"]}\nsortMode=-1\n\n[Containments][1][Wallpaper][org.kde.image][General]\nImage=file:///usr/share/wallpapers/Next/\nname[de]=Hallo\nPreviewImage[$e]=$HOME/x.png\n";

    #[test]
    fn nested_groups_split_into_parts() {
        let groups = parse(APPLETS);
        let containment = find(&groups, &["Containments", "1"]).unwrap();
        assert_eq!(containment.get("plugin"), Some("org.kde.plasma.folder"));
        assert_eq!(containment.number("lastScreen"), Some(0));
        let general = find(&groups, &["Containments", "1", "General"]).unwrap();
        assert_eq!(general.get("positions"), Some("{\"1920x1080\":[\"1\"\\,\"8\"\\,\"desktop:/a.txt\"\\,\"0\"\\,\"2\"]}"));
        let wallpaper = find(&groups, &["Containments", "1", "Wallpaper", "org.kde.image", "General"]).unwrap();
        assert_eq!(wallpaper.get("Image"), Some("file:///usr/share/wallpapers/Next/"));
        assert_eq!(wallpaper.get("name"), None);
        assert_eq!(wallpaper.get("PreviewImage"), Some("$HOME/x.png"));
    }

    #[test]
    fn escapes_follow_kconfig() {
        assert_eq!(unescape("\\sa\\tb\\\\c\\x41"), " a\tb\\cA");
        assert_eq!(unescape("a\\,b"), "a\\,b");
    }

    #[test]
    fn xfce_rc_groups_are_paths() {
        let groups = parse("[xfdesktop-version-4.10.3+]\n4.10.3+=true\n\n[/home/ada/Desktop/notes.txt]\nrow=2\ncol=0\n\n[Trash]\nrow=1\ncol=0\n");
        let notes = find(&groups, &["/home/ada/Desktop/notes.txt"]).unwrap();
        assert_eq!((notes.number("row"), notes.number("col")), (Some(2), Some(0)));
        assert_eq!(groups.len(), 3);
    }
}
