pub fn string(text: &str) -> Option<String> {
    let text = text.trim();
    let text = text.strip_prefix("@s ").unwrap_or(text);
    let mut chars = text.chars();
    let quote = chars.next().filter(|first| *first == '\'' || *first == '"')?;
    let mut out = String::new();
    while let Some(letter) = chars.next() {
        match letter {
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'f' => out.push('\u{c}'),
                'b' => out.push('\u{8}'),
                'v' => out.push('\u{b}'),
                'a' => out.push('\u{7}'),
                'u' => out.push(unicode(&mut chars, 4)?),
                'U' => out.push(unicode(&mut chars, 8)?),
                other => out.push(other),
            },
            end if end == quote => return chars.as_str().trim().is_empty().then_some(out),
            other => out.push(other),
        }
    }
    None
}

fn unicode(chars: &mut std::str::Chars, digits: usize) -> Option<char> {
    let mut value = 0u32;
    for _ in 0..digits {
        value = value * 16 + chars.next()?.to_digit(16)?;
    }
    char::from_u32(value)
}

pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for letter in text.chars() {
        match letter {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

pub fn boolean(text: &str) -> Option<bool> {
    match text.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub fn strings(text: &str) -> Vec<String> {
    let text = text.trim();
    let text = text.strip_prefix("@as ").unwrap_or(text);
    let Some(inner) = text.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) else { return Vec::new() };
    let mut found = Vec::new();
    let mut rest = inner.trim();
    while !rest.is_empty() {
        let Some(quote) = rest.chars().next().filter(|first| *first == '\'' || *first == '"') else { break };
        let mut end = None;
        let mut escaped = false;
        for (index, letter) in rest.char_indices().skip(1) {
            if escaped {
                escaped = false;
            } else if letter == '\\' {
                escaped = true;
            } else if letter == quote {
                end = Some(index);
                break;
            }
        }
        let Some(end) = end else { break };
        if let Some(value) = string(&rest[..=end]) {
            found.push(value);
        }
        rest = rest[end + 1..].trim_start().trim_start_matches(',').trim_start();
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsettings_strings_unquote() {
        assert_eq!(string("'file:///usr/share/backgrounds/warty-final-ubuntu.png'\n"), Some("file:///usr/share/backgrounds/warty-final-ubuntu.png".into()));
        assert_eq!(string("\"it's here\""), Some("it's here".into()));
        assert_eq!(string("'a\\'b\\\\c'"), Some("a'b\\c".into()));
        assert_eq!(string("'caf\\u00e9'"), Some("café".into()));
        assert_eq!(string("@s 'x'"), Some("x".into()));
        assert_eq!(string("zoom"), None);
        assert_eq!(string("'open"), None);
        assert_eq!(string("'a' 'b'"), None);
    }

    #[test]
    fn quoting_round_trips() {
        for text in ["file:///home/a/Pictures/it's me.jpg", "back\\slash", "tab\there", "plain"] {
            assert_eq!(string(&quote(text)), Some(text.to_string()), "{}", quote(text));
        }
        assert_eq!(quote("it's"), "'it\\'s'");
    }

    #[test]
    fn lists_and_booleans_read_back() {
        assert_eq!(strings("['ding@rastersoft.com', 'ubuntu-dock@ubuntu.com']"), vec!["ding@rastersoft.com".to_string(), "ubuntu-dock@ubuntu.com".to_string()]);
        assert_eq!(strings("@as []"), Vec::<String>::new());
        assert_eq!(strings("['a,b', \"c'd\"]"), vec!["a,b".to_string(), "c'd".to_string()]);
        assert_eq!(boolean("true\n"), Some(true));
        assert_eq!(boolean("1"), None);
    }
}
