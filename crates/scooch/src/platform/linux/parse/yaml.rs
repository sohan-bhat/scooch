#[derive(Clone, Debug, PartialEq)]
pub enum Yaml {
    Map(Vec<(String, Yaml)>),
    List(Vec<Yaml>),
    Text(String),
    Empty,
}

impl Yaml {
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(entries) => entries.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Yaml::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<i64> {
        self.text()?.trim().parse().ok()
    }

    pub fn list(&self) -> &[Yaml] {
        match self {
            Yaml::List(items) => items,
            _ => &[],
        }
    }

    pub fn entries(&self) -> &[(String, Yaml)] {
        match self {
            Yaml::Map(entries) => entries,
            _ => &[],
        }
    }
}

struct Line {
    indent: usize,
    text: String,
}

pub fn parse(source: &str) -> Yaml {
    let mut lines: Vec<Line> = Vec::new();
    for raw in join_quoted(source).lines() {
        let trimmed = raw.trim_start_matches(' ');
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" || trimmed == "..." || trimmed.starts_with("%YAML") {
            continue;
        }
        let text = trimmed.strip_prefix("--- ").unwrap_or(trimmed).trim_end().to_string();
        lines.push(Line { indent: raw.len() - trimmed.len(), text });
    }
    let mut index = 0;
    match lines.first().map(|line| line.indent) {
        Some(indent) => block(&mut lines, &mut index, indent),
        None => Yaml::Empty,
    }
}

fn join_quoted(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut quote: Option<char> = None;
    let mut line_start = true;
    let mut comment = false;
    while let Some(letter) = chars.next() {
        if comment {
            if letter == '\n' {
                comment = false;
                line_start = true;
                out.push(letter);
            }
            continue;
        }
        match quote {
            None => {
                if letter == '#' && (line_start || out.ends_with(' ')) {
                    comment = true;
                    continue;
                }
                if letter == '"' || letter == '\'' {
                    let previous = out.trim_end_matches(' ').chars().last();
                    if previous.is_none_or(|before| matches!(before, ':' | '-' | '?' | '\n' | '[' | ',' | '{')) || line_start {
                        quote = Some(letter);
                    }
                }
                line_start = letter == '\n' || (line_start && letter == ' ');
                out.push(letter);
            }
            Some(open) => {
                if open == '"' && letter == '\\' {
                    match chars.peek() {
                        Some('\n') => {
                            chars.next();
                            while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
                                chars.next();
                            }
                            continue;
                        }
                        Some(_) => {
                            out.push(letter);
                            if let Some(next) = chars.next() {
                                out.push(next);
                            }
                            continue;
                        }
                        None => {}
                    }
                }
                if open == '\'' && letter == '\'' && chars.peek() == Some(&'\'') {
                    out.push(letter);
                    out.push('\'');
                    chars.next();
                    continue;
                }
                if letter == '\n' {
                    while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
                        chars.next();
                    }
                    if chars.peek() == Some(&'\n') {
                        out.push_str(if open == '"' { "\\n" } else { "\n" });
                        chars.next();
                        while chars.peek() == Some(&' ') || chars.peek() == Some(&'\t') {
                            chars.next();
                        }
                    } else {
                        out.push(' ');
                    }
                    continue;
                }
                if letter == open {
                    quote = None;
                }
                out.push(letter);
            }
        }
    }
    out
}

fn block(lines: &mut Vec<Line>, index: &mut usize, indent: usize) -> Yaml {
    let Some(first) = lines.get(*index) else { return Yaml::Empty };
    if first.text == "-" || first.text.starts_with("- ") {
        return sequence(lines, index, indent);
    }
    if first.text.starts_with("? ") || split_key(&first.text).is_some() {
        return mapping(lines, index, indent);
    }
    *index += 1;
    scalar(&lines[*index - 1].text)
}

fn sequence(lines: &mut Vec<Line>, index: &mut usize, indent: usize) -> Yaml {
    let mut items = Vec::new();
    while let Some(line) = lines.get(*index) {
        if line.indent != indent || !(line.text == "-" || line.text.starts_with("- ")) {
            break;
        }
        let rest = line.text.get(2..).unwrap_or("").trim_start().to_string();
        if rest.is_empty() {
            *index += 1;
            match lines.get(*index) {
                Some(next) if next.indent > indent => {
                    let child = next.indent;
                    items.push(block(lines, index, child));
                }
                _ => items.push(Yaml::Empty),
            }
            continue;
        }
        let shift = line.text.len() - rest.len();
        lines[*index] = Line { indent: indent + shift, text: rest };
        let child = indent + shift;
        items.push(block(lines, index, child));
    }
    Yaml::List(items)
}

fn mapping(lines: &mut Vec<Line>, index: &mut usize, indent: usize) -> Yaml {
    let mut entries = Vec::new();
    while let Some(line) = lines.get(*index) {
        if line.indent != indent {
            break;
        }
        let (key, value) = if let Some(rest) = line.text.strip_prefix("? ") {
            let key = scalar(rest).text().unwrap_or_default().to_string();
            *index += 1;
            let value = match lines.get(*index) {
                Some(next) if next.indent == indent && (next.text == ":" || next.text.starts_with(": ")) => next.text.get(2..).unwrap_or("").trim().to_string(),
                _ => {
                    entries.push((key, Yaml::Empty));
                    continue;
                }
            };
            if split_key(&value).is_some() || value.starts_with("- ") {
                lines[*index] = Line { indent: indent + 2, text: value };
                entries.push((key, block(lines, index, indent + 2)));
                continue;
            }
            (key, value)
        } else if let Some((key, value)) = split_key(&line.text) {
            (key, value)
        } else {
            break;
        };
        *index += 1;
        if !value.is_empty() {
            entries.push((key, scalar(&value)));
            continue;
        }
        match lines.get(*index) {
            Some(next) if next.indent > indent => {
                let child = next.indent;
                entries.push((key, block(lines, index, child)));
            }
            Some(next) if next.indent == indent && (next.text == "-" || next.text.starts_with("- ")) => entries.push((key, sequence(lines, index, indent))),
            _ => entries.push((key, Yaml::Empty)),
        }
    }
    Yaml::Map(entries)
}

fn split_key(text: &str) -> Option<(String, String)> {
    let mut chars = text.char_indices().peekable();
    let mut quote = None;
    while let Some((at, letter)) = chars.next() {
        match quote {
            Some(open) => {
                if open == '"' && letter == '\\' {
                    chars.next();
                } else if letter == open {
                    quote = None;
                }
            }
            None => {
                if (letter == '"' || letter == '\'') && at == 0 {
                    quote = Some(letter);
                } else if letter == ':' && chars.peek().is_none_or(|(_, next)| *next == ' ') {
                    let key = scalar(text[..at].trim()).text().unwrap_or_default().to_string();
                    return Some((key, text[at + 1..].trim().to_string()));
                }
            }
        }
    }
    None
}

pub fn scalar(text: &str) -> Yaml {
    let text = text.trim();
    if text.is_empty() || text == "~" || text == "null" {
        return Yaml::Empty;
    }
    if let Some(inner) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return Yaml::Text(unescape(inner));
    }
    if let Some(inner) = text.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')) {
        return Yaml::Text(inner.replace("''", "'"));
    }
    let plain = match text.find(" #") {
        Some(at) => &text[..at],
        None => text,
    };
    Yaml::Text(plain.trim_end().to_string())
}

fn unescape(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(letter) = chars.next() {
        if letter != '\\' {
            out.push(letter);
            continue;
        }
        let Some(code) = chars.next() else { break };
        let hex = |chars: &mut std::str::Chars, count: usize| -> Option<char> {
            let mut value = 0u32;
            for _ in 0..count {
                value = value * 16 + chars.next()?.to_digit(16)?;
            }
            char::from_u32(value)
        };
        match code {
            '0' => out.push('\0'),
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            't' | '\t' => out.push('\t'),
            'n' => out.push('\n'),
            'v' => out.push('\u{b}'),
            'f' => out.push('\u{c}'),
            'r' => out.push('\r'),
            'e' => out.push('\u{1b}'),
            ' ' => out.push(' '),
            'N' => out.push('\u{85}'),
            '_' => out.push('\u{a0}'),
            'L' => out.push('\u{2028}'),
            'P' => out.push('\u{2029}'),
            'x' => out.extend(hex(&mut chars, 2)),
            'u' => out.extend(hex(&mut chars, 4)),
            'U' => out.extend(hex(&mut chars, 8)),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_maps_and_indentless_lists_parse() {
        let doc = parse("#\n# DO NOT EDIT THIS FILE WHILE XFDESKTOP IS RUNNING\n#\nconfigs:\n- level: 0\n  monitors:\n  - id: \"abc\"\n    display_name: \"Virtual-1\"\n    geometry:\n      x: 0\n      y: 0\n      width: 1920\n      height: 1080\n  icons:\n    \"/home/ada/Desktop/a b.txt\":\n      row: 1\n      col: 2\n    \"trash:///\":\n      row: 0\n      col: 0\n      last_seen: 1700000000\n");
        let configs = doc.get("configs").unwrap().list();
        assert_eq!(configs.len(), 1);
        let config = &configs[0];
        assert_eq!(config.get("level").and_then(Yaml::number), Some(0));
        let monitor = &config.get("monitors").unwrap().list()[0];
        assert_eq!(monitor.get("display_name").and_then(Yaml::text), Some("Virtual-1"));
        assert_eq!(monitor.get("geometry").and_then(|g| g.get("width")).and_then(Yaml::number), Some(1920));
        let icons = config.get("icons").unwrap().entries();
        assert_eq!(icons[0].0, "/home/ada/Desktop/a b.txt");
        assert_eq!(icons[0].1.get("col").and_then(Yaml::number), Some(2));
        assert_eq!(icons[1].0, "trash:///");
    }

    #[test]
    fn indented_lists_and_list_of_maps_parse() {
        let doc = parse("a:\n  - x: 1\n    y: 2\n  - x: 3\nb: plain words # note\nc: 'it''s'\n");
        let items = doc.get("a").unwrap().list();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("y").and_then(Yaml::number), Some(2));
        assert_eq!(doc.get("b").and_then(Yaml::text), Some("plain words"));
        assert_eq!(doc.get("c").and_then(Yaml::text), Some("it's"));
    }

    #[test]
    fn long_keys_and_folded_quotes_join_back() {
        let doc = parse("icons:\n  ? \"/home/ada/Desktop/a very long name\\\n    \\ that wraps.txt\"\n  : row: 4\n    col: 5\n  \"/x/y z\": {}\n");
        let icons = doc.get("icons").unwrap().entries();
        assert_eq!(icons[0].0, "/home/ada/Desktop/a very long name that wraps.txt");
        assert_eq!(icons[0].1.get("row").and_then(Yaml::number), Some(4));
        assert_eq!(icons[0].1.get("col").and_then(Yaml::number), Some(5));
        assert_eq!(icons[1].0, "/x/y z");
        let folded = parse("k: \"one\n  two\"\n");
        assert_eq!(folded.get("k").and_then(Yaml::text), Some("one two"));
    }

    #[test]
    fn escapes_decode() {
        assert_eq!(scalar("\"a\\\"b\\\\c\\x41\\u00e9\""), Yaml::Text("a\"b\\cAé".into()));
        assert_eq!(scalar("~"), Yaml::Empty);
        assert_eq!(parse(""), Yaml::Empty);
    }
}
