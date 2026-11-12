use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const BATCH: usize = 200;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub uri: String,
    pub attributes: BTreeMap<String, String>,
}

pub fn parse_info(output: &str) -> Vec<Info> {
    let mut found: Vec<Info> = Vec::new();
    let mut in_attributes = false;
    for line in output.lines() {
        if let Some(uri) = line.strip_prefix("uri: ") {
            found.push(Info { uri: uri.to_string(), attributes: BTreeMap::new() });
            in_attributes = false;
            continue;
        }
        let Some(current) = found.last_mut() else { continue };
        if line == "attributes:" {
            in_attributes = true;
            continue;
        }
        if !in_attributes {
            continue;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            in_attributes = false;
            continue;
        };
        if let Some((key, value)) = rest.split_once(": ") {
            current.attributes.insert(key.to_string(), value.to_string());
        } else if let Some(key) = rest.strip_suffix(':') {
            current.attributes.insert(key.to_string(), String::new());
        }
    }
    found
}

pub fn decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = match rest.find('/') {
        Some(0) => rest,
        Some(start) if rest[..start].eq_ignore_ascii_case("localhost") => &rest[start..],
        _ => return None,
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    Some(bytes::to_path(decode(rest)))
}

pub fn path_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for byte in bytes::of_path(path).iter() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@');
        if keep {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub mod bytes {
    use super::Cow;
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    pub fn to_path(bytes: Vec<u8>) -> PathBuf {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }

    #[cfg(not(unix))]
    pub fn to_path(bytes: Vec<u8>) -> PathBuf {
        PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
    }

    #[cfg(unix)]
    pub fn of_path(path: &Path) -> Cow<'_, [u8]> {
        use std::os::unix::ffi::OsStrExt;
        Cow::Borrowed(path.as_os_str().as_bytes())
    }

    #[cfg(not(unix))]
    pub fn of_path(path: &Path) -> Cow<'_, [u8]> {
        match path.to_string_lossy() {
            Cow::Borrowed(text) => Cow::Borrowed(text.as_bytes()),
            Cow::Owned(text) => Cow::Owned(text.into_bytes()),
        }
    }
}

pub fn location(text: &str) -> Option<PathBuf> {
    if text.starts_with('/') {
        return Some(PathBuf::from(text));
    }
    uri_path(text)
}

pub fn point(value: &str) -> Option<(f64, f64)> {
    let mut parts = value.split(',');
    let x = parts.next()?.trim().parse::<f64>().ok()?;
    let y = parts.next()?.trim().parse::<f64>().ok()?;
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = "uri: file:///home/ada/Desktop/notes.txt\nlocal path: /home/ada/Desktop/notes.txt\nunix mount: /dev/sda2 /home ext4 rw,relatime\nattributes:\n  metadata::nautilus-icon-position: 130,48\n  metadata::trusted: true\nuri: file:///home/ada/Desktop/a%20b%C3%A9.txt\nlocal path: /home/ada/Desktop/a bé.txt\nattributes:\n";

    #[test]
    fn several_files_come_back_in_order_with_their_metadata() {
        let found = parse_info(TWO);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].uri, "file:///home/ada/Desktop/notes.txt");
        assert_eq!(found[0].attributes["metadata::nautilus-icon-position"], "130,48");
        assert_eq!(found[0].attributes["metadata::trusted"], "true");
        assert!(found[1].attributes.is_empty());
        assert_eq!(uri_path(&found[1].uri), Some(PathBuf::from("/home/ada/Desktop/a bé.txt")));
    }

    #[test]
    fn an_empty_value_is_kept_as_empty() {
        let found = parse_info("uri: file:///d/x\nattributes:\n  metadata::nautilus-icon-position: \n");
        assert_eq!(found[0].attributes.get("metadata::nautilus-icon-position").map(String::as_str), Some(""));
        assert_eq!(point(""), None);
    }

    #[test]
    fn lines_before_any_uri_and_error_text_are_ignored() {
        let found = parse_info("gio: file:///gone: No such file\nattributes:\n  metadata::x: 1\n");
        assert!(found.is_empty());
    }

    #[test]
    fn points_need_two_numbers() {
        assert_eq!(point("130,48"), Some((130.0, 48.0)));
        assert_eq!(point("12.5, -3"), Some((12.5, -3.0)));
        assert_eq!(point("130"), None);
        assert_eq!(point("a,b"), None);
    }

    #[test]
    fn paths_and_uris_round_trip() {
        for text in ["/home/ada/Desktop/a b.txt", "/tmp/100%/x#y?.png", "/p/é.jpg", "/p/[x]"] {
            let path = PathBuf::from(text);
            assert_eq!(uri_path(&path_uri(&path)), Some(path.clone()), "{}", path_uri(&path));
        }
        assert_eq!(path_uri(Path::new("/a b/c")), "file:///a%20b/c");
        assert_eq!(uri_path("file://localhost/x/y"), Some(PathBuf::from("/x/y")));
        assert_eq!(uri_path("trash:///"), None);
        assert_eq!(uri_path("file://server/x"), None);
        assert_eq!(location("/x/y.png"), Some(PathBuf::from("/x/y.png")));
        assert_eq!(decode("%zz%4"), b"%zz%4".to_vec());
    }
}
