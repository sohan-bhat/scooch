use std::path::Path;

pub const NAME: &str = "scooch.desktop";

pub fn quote(argument: &str) -> String {
    let mut out = String::from("\"");
    for letter in argument.chars() {
        if matches!(letter, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(letter);
    }
    out.push('"');
    out
}

pub fn escape_value(text: &str) -> String {
    let mut out = String::new();
    for letter in text.chars() {
        match letter {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '%' => out.push_str("%%"),
            other => out.push(other),
        }
    }
    out
}

pub fn exec(exe: &Path) -> String {
    escape_value(&format!("{} watch", quote(&exe.to_string_lossy())))
}

pub fn text(exe: &Path) -> String {
    format!("[Desktop Entry]\nType=Application\nName=Scooch\nExec={}\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n", exec(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_has_every_required_key() {
        let entry = text(Path::new("/home/ada/.local/bin/scooch"));
        assert_eq!(entry, "[Desktop Entry]\nType=Application\nName=Scooch\nExec=\"/home/ada/.local/bin/scooch\" watch\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n");
    }

    #[test]
    fn a_path_with_spaces_stays_one_argument() {
        assert_eq!(exec(Path::new("/home/ada/My Apps/scooch")), "\"/home/ada/My Apps/scooch\" watch");
    }

    #[test]
    fn reserved_letters_are_escaped_twice() {
        assert_eq!(exec(Path::new("/opt/a\"b$c")), "\"/opt/a\\\\\"b\\\\$c\" watch");
        assert_eq!(exec(Path::new("/opt/100%/scooch")), "\"/opt/100%%/scooch\" watch");
        assert_eq!(exec(Path::new("/opt/back\\slash")), "\"/opt/back\\\\\\\\slash\" watch");
    }
}
