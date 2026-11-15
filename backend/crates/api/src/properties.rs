//! Java `.properties` parsing and serialisation.
//!
//! The control plane exposes `server.properties` as an ordered list of entries
//! so comments and blank lines survive a round-trip through the UI. Values use
//! the Java escaping rules (`\t`, `\n`, `\r`, `\f`, `\\`, `\uXXXX`, and
//! backslash-continued physical lines).
//!
//! `rcon.password` is never sent to the browser. It is redacted on reads and
//! restored from disk on writes, so saving the form cannot blank it.

use serde::{Deserialize, Serialize};

/// Keys whose values must not leave the control plane.
pub const REDACTED_KEYS: [&str; 1] = ["rcon.password"];

pub const MAX_ENTRIES: usize = 2_000;
const MAX_KEY_CHARS: usize = 256;

/// One logical line of a properties file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PropertyEntry {
    /// A `#` or `!` comment line, preserved verbatim.
    Comment { text: String },
    /// An empty line, preserved so section spacing survives.
    Blank,
    /// A `key=value` pair, with both sides decoded.
    Property { key: String, value: String },
}

impl PropertyEntry {
    pub fn property(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Property {
            key: key.into(),
            value: value.into(),
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            Self::Property { key, .. } => Some(key),
            _ => None,
        }
    }
}

/// Parse a properties document into ordered entries.
pub fn parse(input: &str) -> Vec<PropertyEntry> {
    let mut entries = Vec::new();
    let mut lines = input.split('\n').peekable();

    while let Some(raw) = lines.next() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim_start();

        if line.trim().is_empty() {
            entries.push(PropertyEntry::Blank);
            continue;
        }
        if trimmed.starts_with('#') || trimmed.starts_with('!') {
            entries.push(PropertyEntry::Comment {
                text: line.to_string(),
            });
            continue;
        }

        // A logical line continues while it ends with an odd number of
        // backslashes. The final backslash is removed and the continuation's
        // leading whitespace is ignored (Java semantics).
        let mut logical = line.to_string();
        while ends_with_odd_backslashes(&logical) {
            logical.pop();
            let Some(next) = lines.next() else {
                break;
            };
            let next = next.strip_suffix('\r').unwrap_or(next);
            logical.push_str(next.trim_start());
        }

        entries.push(parse_pair(&logical));
    }

    entries
}

/// Serialise entries back to a properties document.
pub fn serialise(entries: &[PropertyEntry]) -> String {
    let lines: Vec<String> = entries
        .iter()
        .map(|entry| match entry {
            PropertyEntry::Comment { text } => sanitise_comment(text),
            PropertyEntry::Blank => String::new(),
            PropertyEntry::Property { key, value } => {
                format!("{}={}", escape(key, true), escape(value, false))
            }
        })
        .collect();

    lines.join("\n")
}

/// Validate user-supplied entries before they are persisted.
pub fn validate(entries: &[PropertyEntry]) -> Result<(), String> {
    if entries.len() > MAX_ENTRIES {
        return Err(format!("at most {MAX_ENTRIES} entries are allowed"));
    }

    for entry in entries {
        let PropertyEntry::Property { key, value } = entry else {
            continue;
        };
        if key.trim().is_empty() {
            return Err("property keys must not be empty".into());
        }
        if key.chars().count() > MAX_KEY_CHARS {
            return Err(format!(
                "property keys must be at most {MAX_KEY_CHARS} characters"
            ));
        }
        if key.chars().any(char::is_control) || value.chars().any(char::is_control) {
            return Err("property keys and values must not contain control characters".into());
        }
        // A multi-line value would be written as an escaped sequence anyway,
        // but rejecting it keeps the contract simple for the UI.
        if key.contains(['\n', '\r', '=']) {
            return Err("property keys must not contain newlines or '='".into());
        }
    }

    Ok(())
}

/// Find the first value for `key`.
pub fn get<'a>(entries: &'a [PropertyEntry], key: &str) -> Option<&'a str> {
    entries.iter().find_map(|entry| match entry {
        PropertyEntry::Property {
            key: entry_key,
            value,
        } if entry_key == key => Some(value.as_str()),
        _ => None,
    })
}

/// Replace redacted values with empty strings before sending to the browser.
pub fn redact(entries: &mut [PropertyEntry]) {
    for entry in entries {
        if let PropertyEntry::Property { key, value } = entry {
            if is_redacted(key) {
                value.clear();
            }
        }
    }
}

/// Restore redacted keys from the current on-disk document so a save never
/// wipes a secret the user could not see.
pub fn restore_redacted(incoming: &mut [PropertyEntry], current: &[PropertyEntry]) {
    for entry in incoming {
        let PropertyEntry::Property { key, value } = entry else {
            continue;
        };
        if !is_redacted(key) {
            continue;
        }
        if let Some(existing) = get(current, key) {
            *value = existing.to_string();
        }
    }
}

pub fn is_redacted(key: &str) -> bool {
    REDACTED_KEYS.contains(&key)
}

fn parse_pair(line: &str) -> PropertyEntry {
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;

    skip_whitespace(&chars, &mut index);
    let key_start = index;
    let key_end = scan_key(&chars, key_start);
    let key = unescape(&chars[key_start..key_end]);

    index = key_end;
    skip_whitespace(&chars, &mut index);
    if index < chars.len() && (chars[index] == '=' || chars[index] == ':') {
        index += 1;
    }
    skip_whitespace(&chars, &mut index);

    let value = unescape(&chars[index..]);
    PropertyEntry::Property { key, value }
}

fn skip_whitespace(chars: &[char], index: &mut usize) {
    while *index < chars.len() && chars[*index].is_whitespace() {
        *index += 1;
    }
}

/// Index of the first unescaped `=`, `:`, or whitespace character.
fn scan_key(chars: &[char], start: usize) -> usize {
    let mut escaped = false;
    for (offset, character) in chars[start..].iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '=' | ':' => return start + offset,
            character if character.is_whitespace() => return start + offset,
            _ => {}
        }
    }
    chars.len()
}

fn ends_with_odd_backslashes(line: &str) -> bool {
    line.chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count()
        % 2
        == 1
}

fn unescape(chars: &[char]) -> String {
    let mut out = String::with_capacity(chars.len());
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != '\\' {
            out.push(chars[index]);
            index += 1;
            continue;
        }

        index += 1;
        let Some(escaped) = chars.get(index).copied() else {
            out.push('\\');
            break;
        };

        match escaped {
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            'f' => out.push('\u{000C}'),
            'u' => {
                if let Some((character, consumed)) = decode_unicode(&chars[index + 1..]) {
                    out.push(character);
                    index += consumed;
                } else {
                    out.push('u');
                }
            }
            other => out.push(other),
        }
        index += 1;
    }

    out
}

/// Decode the first 4 hex digits; returns the character and digits consumed.
fn decode_unicode(chars: &[char]) -> Option<(char, usize)> {
    let digits: String = chars.iter().take(4).collect();
    if digits.len() < 4 {
        return None;
    }
    let value = u32::from_str_radix(&digits, 16).ok()?;
    char::from_u32(value).map(|character| (character, 4))
}

fn escape(input: &str, is_key: bool) -> String {
    let mut out = String::with_capacity(input.len());
    let mut leading = true;

    for (index, character) in input.chars().enumerate() {
        let must_escape = match character {
            '\\' => true,
            '\t' => true,
            '\n' => true,
            '\r' => true,
            '\u{000C}' => true,
            ' ' | '=' | ':' if is_key => true,
            ' ' if leading => true,
            '#' | '!' if is_key && index == 0 => true,
            character if !character.is_ascii_graphic() && character != ' ' => true,
            _ => false,
        };

        if !is_key && !character.is_whitespace() {
            leading = false;
        }

        if must_escape {
            match character {
                '\t' => out.push_str("\\t"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\u{000C}' => out.push_str("\\f"),
                '\\' => out.push_str("\\\\"),
                ' ' | '=' | ':' | '#' | '!' => {
                    out.push('\\');
                    out.push(character);
                }
                character => push_unicode(&mut out, character),
            }
        } else {
            out.push(character);
        }
    }

    out
}

fn push_unicode(out: &mut String, character: char) {
    let mut buffer = [0_u16; 2];
    for unit in character.encode_utf16(&mut buffer) {
        out.push_str(&format!("\\u{unit:04X}"));
    }
}

/// Comments must stay single-line; strip line breaks from user input.
fn sanitise_comment(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
#Minecraft server properties
#Sat Nov 14 00:00:00 UTC 2026
motd=Hello \\u00A7aWorld
pvp=true
difficulty=normal

rcon.password=super-secret
server-port=25565
";

    #[test]
    fn round_trips_comments_and_blanks() {
        let entries = parse(SAMPLE);
        assert!(matches!(entries[0], PropertyEntry::Comment { .. }));
        assert_eq!(entries[2], PropertyEntry::property("motd", "Hello §aWorld"));
        assert_eq!(entries[3], PropertyEntry::property("pvp", "true"));
        assert!(entries.contains(&PropertyEntry::Blank));

        let serialised = serialise(&entries);
        let reparsed = parse(&serialised);
        assert_eq!(entries, reparsed);
    }

    #[test]
    fn decodes_escapes_and_continuations() {
        let entries = parse("motd=line one\\\n    line two\npath=C\\:\\\\temp\n");
        assert_eq!(
            entries[0],
            PropertyEntry::property("motd", "line oneline two")
        );
        assert_eq!(entries[1], PropertyEntry::property("path", "C:\\temp"));
    }

    #[test]
    fn handles_colon_separator_and_whitespace() {
        let entries = parse("a: 1\nb 2\nc\n");
        assert_eq!(entries[0], PropertyEntry::property("a", "1"));
        assert_eq!(entries[1], PropertyEntry::property("b", "2"));
        assert_eq!(entries[2], PropertyEntry::property("c", ""));
    }

    #[test]
    fn redacts_and_restores_rcon_password() {
        let mut entries = parse(SAMPLE);
        redact(&mut entries);
        assert_eq!(get(&entries, "rcon.password"), Some(""));

        let mut incoming = parse("pvp=false\nrcon.password=\n");
        restore_redacted(&mut incoming, &parse(SAMPLE));
        assert_eq!(get(&incoming, "rcon.password"), Some("super-secret"));
        assert_eq!(get(&incoming, "pvp"), Some("false"));
    }

    #[test]
    fn rejects_invalid_entries() {
        assert!(validate(&[PropertyEntry::property("", "x")]).is_err());
        assert!(validate(&[PropertyEntry::property("a\0b", "x")]).is_err());
        assert!(validate(&[PropertyEntry::property("a=b", "x")]).is_err());
        assert!(validate(&[PropertyEntry::property("motd", "line\nbreak")]).is_err());
        assert!(validate(&[PropertyEntry::property("motd", "ok")]).is_ok());
    }

    #[test]
    fn escapes_values_that_look_like_specials() {
        let entries = vec![
            PropertyEntry::property("motd", "  leading spaces"),
            PropertyEntry::property("icon", "#not-a-comment"),
            PropertyEntry::property("unicode", "café"),
        ];
        let serialised = serialise(&entries);
        assert!(serialised.starts_with("motd=\\ \\ "));
        assert!(serialised.contains("unicode=caf\\u00E9"));
        assert_eq!(parse(&serialised), entries);
    }
}
