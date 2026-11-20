use std::path::{Path, PathBuf};

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

pub fn script(exe: &Path, file: &Path) -> Option<String> {
    let (exe, file) = (exe.to_str()?, file.to_str()?);
    let plain = |text: &str| text.starts_with('/') && !text.chars().any(|letter| matches!(letter, '\'' | '"' | '`' | '$' | '\\' | '%') || letter.is_control());
    (plain(exe) && plain(file)).then(|| format!("if [ -x '{exe}' ]; then exec '{exe}' watch; else rm -f '{file}'; fi"))
}

pub fn exec(exe: &Path, file: &Path) -> String {
    match script(exe, file) {
        Some(script) => escape_value(&format!("sh -c {}", quote(&script))),
        None => escape_value(&format!("{} watch", quote(&exe.to_string_lossy()))),
    }
}

pub fn text(exe: &Path, file: &Path) -> String {
    format!("[Desktop Entry]\nType=Application\nName=Scooch\nExec={}\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n", exec(exe, file))
}

pub fn program(text: &str) -> Option<PathBuf> {
    let value = text.lines().find_map(|line| line.strip_prefix("Exec="))?.trim();
    if let Some(rest) = value.strip_prefix("sh -c ") {
        let start = rest.find("exec '")? + "exec '".len();
        let length = rest[start..].find("' watch")?;
        return Some(PathBuf::from(&rest[start..start + length]));
    }
    let rest = value.strip_prefix('"')?;
    let mut found = String::new();
    let mut letters = rest.replace("\\\\", "\\").replace("%%", "%").chars().collect::<Vec<_>>().into_iter();
    while let Some(letter) = letters.next() {
        match letter {
            '"' => return Some(PathBuf::from(found)),
            '\\' => found.extend(letters.next()),
            other => found.push(other),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "/home/ada/.config/autostart/scooch.desktop";

    #[test]
    fn the_entry_starts_the_program_when_it_is_there_and_removes_itself_when_it_is_not() {
        let entry = text(Path::new("/home/ada/.local/bin/scooch"), Path::new(FILE));
        assert_eq!(entry, "[Desktop Entry]\nType=Application\nName=Scooch\nExec=sh -c \"if [ -x '/home/ada/.local/bin/scooch' ]; then exec '/home/ada/.local/bin/scooch' watch; else rm -f '/home/ada/.config/autostart/scooch.desktop'; fi\"\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n");
        assert_eq!(program(&entry), Some(PathBuf::from("/home/ada/.local/bin/scooch")));
        assert!(entry.contains("/home/ada/.local/bin/scooch"), "the program path stays readable for prerm and the app");
    }

    #[cfg(unix)]
    #[test]
    fn the_script_runs_the_program_or_removes_the_entry() {
        let root = crate::state::scratch("entry-script");
        let file = root.join("scooch.desktop");
        let program = root.join("scooch");
        let marker = root.join("ran");
        std::fs::write(&program, format!("#!/bin/sh\necho \"$1\" > '{}'\n", marker.display())).unwrap();
        std::fs::set_permissions(&program, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        std::fs::write(&file, text(&program, &file)).unwrap();
        let run = || std::process::Command::new("/bin/sh").arg("-c").arg(script(&program, &file).unwrap()).status().unwrap();
        run();
        assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "watch");
        assert!(file.is_file());
        std::fs::remove_file(&program).unwrap();
        run();
        assert!(!file.exists(), "the entry stayed with its program gone");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_the_script_cannot_hold_falls_back_to_the_plain_entry() {
        assert_eq!(exec(Path::new("/home/ada/My Apps/scooch"), Path::new(FILE)), "sh -c \"if [ -x '/home/ada/My Apps/scooch' ]; then exec '/home/ada/My Apps/scooch' watch; else rm -f '/home/ada/.config/autostart/scooch.desktop'; fi\"");
        assert_eq!(exec(Path::new("/opt/a\"b$c"), Path::new(FILE)), "\"/opt/a\\\\\"b\\\\$c\" watch");
        assert_eq!(exec(Path::new("/opt/100%/scooch"), Path::new(FILE)), "\"/opt/100%%/scooch\" watch");
        assert_eq!(exec(Path::new("/opt/it's/scooch"), Path::new(FILE)), "\"/opt/it's/scooch\" watch");
        assert_eq!(program(&text(Path::new("/opt/100%/scooch"), Path::new(FILE))), Some(PathBuf::from("/opt/100%/scooch")));
        assert_eq!(program(&text(Path::new("/opt/a\"b"), Path::new(FILE))), Some(PathBuf::from("/opt/a\"b")));
        assert_eq!(program("[Desktop Entry]\nName=Scooch\n"), None);
    }
}
