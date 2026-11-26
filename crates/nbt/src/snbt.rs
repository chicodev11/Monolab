//! Stringified NBT, the text form used by commands and for editing values.

use crate::tag::{Compound, Kind, List, Tag};

fn plain_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')
}

pub fn quote(text: &str) -> String {
    let mark = if text.contains('"') && !text.contains('\'') {
        '\''
    } else {
        '"'
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(mark);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == mark => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(mark);
    out
}

/// A compound key as SNBT writes it: bare when it can be, quoted otherwise.
pub fn key(text: &str) -> String {
    if !text.is_empty() && text.chars().all(plain_char) {
        text.to_string()
    } else {
        quote(text)
    }
}

fn float(v: f64) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e16 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

fn float32(v: f32) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e7 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// The value alone, as it would be typed: `5b`, `"text"`, `1.5d`.
pub fn scalar(tag: &Tag) -> Option<String> {
    Some(match tag {
        Tag::Byte(v) => format!("{v}b"),
        Tag::Short(v) => format!("{v}s"),
        Tag::Int(v) => format!("{v}"),
        Tag::Long(v) => format!("{v}L"),
        Tag::Float(v) => format!("{}f", float32(*v)),
        Tag::Double(v) => format!("{}d", float(*v)),
        Tag::String(text) => quote(text),
        _ => return None,
    })
}

/// The value as a user edits it in a field: no suffix, no quotes.
pub fn edit_text(tag: &Tag) -> String {
    match tag {
        Tag::Byte(v) => v.to_string(),
        Tag::Short(v) => v.to_string(),
        Tag::Int(v) => v.to_string(),
        Tag::Long(v) => v.to_string(),
        Tag::Float(v) => float32(*v),
        Tag::Double(v) => float(*v),
        Tag::String(text) => text.clone(),
        other => to_snbt(other, false),
    }
}

/// Reads what a user typed into a field for a value of `kind`.
pub fn parse_as(kind: Kind, text: &str) -> Result<Tag, String> {
    let t = text.trim();
    let int = |what: &str| format!("\"{t}\" is not a whole number that fits a {what}.");
    let bool_byte = || match t {
        "true" => Some(1),
        "false" => Some(0),
        _ => None,
    };
    Ok(match kind {
        Kind::Byte => Tag::Byte(match bool_byte() {
            Some(v) => v,
            None => strip(t, 'b')
                .parse()
                .map_err(|_| int("Byte (-128 to 127)"))?,
        }),
        Kind::Short => Tag::Short(strip(t, 's').parse().map_err(|_| int("Short"))?),
        Kind::Int => Tag::Int(t.parse().map_err(|_| int("Int"))?),
        Kind::Long => Tag::Long(strip(t, 'l').parse().map_err(|_| int("Long"))?),
        Kind::Float => Tag::Float(
            strip(t, 'f')
                .parse()
                .map_err(|_| format!("\"{t}\" is not a number."))?,
        ),
        Kind::Double => Tag::Double(
            strip(t, 'd')
                .parse()
                .map_err(|_| format!("\"{t}\" is not a number."))?,
        ),
        // Strings are taken exactly as typed, spaces included.
        Kind::String => Tag::String(text.to_string()),
        _ => {
            let tag = parse(t)?;
            if tag.kind() != kind {
                return Err(format!(
                    "Expected a {}, got a {}.",
                    kind.name(),
                    tag.kind().name()
                ));
            }
            tag
        }
    })
}

fn strip(text: &str, suffix: char) -> &str {
    text.strip_suffix(suffix)
        .or_else(|| text.strip_suffix(suffix.to_ascii_uppercase()))
        .unwrap_or(text)
}

pub fn to_snbt(tag: &Tag, pretty: bool) -> String {
    let mut out = String::new();
    write(&mut out, tag, pretty, 0);
    out
}

pub fn compound_to_snbt(map: &Compound, pretty: bool) -> String {
    let mut out = String::new();
    write_compound(&mut out, map, pretty, 0);
    out
}

fn newline(out: &mut String, pretty: bool, depth: usize) {
    if pretty {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
    }
}

fn write_compound(out: &mut String, map: &Compound, pretty: bool, depth: usize) {
    if map.is_empty() {
        out.push_str("{}");
        return;
    }
    out.push('{');
    for (i, (name, tag)) in map.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        newline(out, pretty, depth + 1);
        out.push_str(&key(name));
        out.push_str(if pretty { ": " } else { ":" });
        write(out, tag, pretty, depth + 1);
    }
    newline(out, pretty, depth);
    out.push('}');
}

fn write_array<T: ToString>(out: &mut String, mark: &str, items: &[T], suffix: &str) {
    out.push('[');
    out.push_str(mark);
    out.push(';');
    for (i, item) in items.iter().enumerate() {
        out.push_str(if i == 0 { "" } else { "," });
        out.push_str(&item.to_string());
        out.push_str(suffix);
    }
    out.push(']');
}

fn write(out: &mut String, tag: &Tag, pretty: bool, depth: usize) {
    match tag {
        Tag::Compound(map) => write_compound(out, map, pretty, depth),
        Tag::List(list) => {
            if list.items.is_empty() {
                out.push_str("[]");
                return;
            }
            // Short lists of numbers stay on one line even when pretty.
            let flat = !pretty
                || (list.items.len() <= 8
                    && !list.kind.is_container()
                    && list.kind != Kind::String);
            out.push('[');
            for (i, item) in list.items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if flat && pretty {
                        out.push(' ');
                    }
                }
                if !flat {
                    newline(out, pretty, depth + 1);
                }
                write(out, item, pretty, depth + 1);
            }
            if !flat {
                newline(out, pretty, depth);
            }
            out.push(']');
        }
        Tag::ByteArray(v) => write_array(out, "B", v, "b"),
        Tag::IntArray(v) => write_array(out, "I", v, ""),
        Tag::LongArray(v) => write_array(out, "L", v, "L"),
        other => out.push_str(&scalar(other).expect("scalar")),
    }
}

/// Parses SNBT as written by commands, e.g. `{Count:1b,id:"minecraft:stone"}`.
pub fn parse(text: &str) -> Result<Tag, String> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        at: 0,
    };
    let tag = parser.value(0)?;
    parser.space();
    if parser.at < parser.chars.len() {
        return Err(parser.fail("Unexpected text after the value"));
    }
    Ok(tag)
}

pub fn parse_compound(text: &str) -> Result<Compound, String> {
    match parse(text)? {
        Tag::Compound(map) => Ok(map),
        other => Err(format!(
            "Expected a compound, got a {}.",
            other.kind().name()
        )),
    }
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn fail(&self, what: &str) -> String {
        format!("{what} at character {}.", self.at + 1)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        self.space();
        if self.peek() == Some(c) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.fail(&format!("Expected '{c}'")))
        }
    }

    fn quoted(&mut self) -> Result<String, String> {
        let mark = self.peek().expect("quote");
        self.at += 1;
        let mut out = String::new();
        loop {
            let c = self
                .peek()
                .ok_or_else(|| self.fail("A quote is never closed"))?;
            self.at += 1;
            match c {
                '\\' => {
                    let e = self.peek().ok_or_else(|| self.fail("Unfinished escape"))?;
                    self.at += 1;
                    out.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        other => other,
                    });
                }
                c if c == mark => return Ok(out),
                c => out.push(c),
            }
        }
    }

    fn word(&mut self) -> String {
        let start = self.at;
        while self.peek().is_some_and(plain_char) {
            self.at += 1;
        }
        self.chars[start..self.at].iter().collect()
    }

    fn key(&mut self) -> Result<String, String> {
        self.space();
        match self.peek() {
            Some('"' | '\'') => self.quoted(),
            _ => {
                let word = self.word();
                if word.is_empty() {
                    Err(self.fail("Expected a key"))
                } else {
                    Ok(word)
                }
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Tag, String> {
        if depth > 512 {
            return Err(self.fail("Nested too deeply"));
        }
        self.space();
        match self.peek() {
            None => Err(self.fail("Expected a value")),
            Some('{') => {
                self.at += 1;
                let mut map = Compound::new();
                self.space();
                if self.peek() == Some('}') {
                    self.at += 1;
                    return Ok(Tag::Compound(map));
                }
                loop {
                    let name = self.key()?;
                    self.expect(':')?;
                    let tag = self.value(depth + 1)?;
                    map.insert(name, tag);
                    self.space();
                    match self.peek() {
                        Some(',') => self.at += 1,
                        Some('}') => {
                            self.at += 1;
                            return Ok(Tag::Compound(map));
                        }
                        _ => return Err(self.fail("Expected ',' or '}'")),
                    }
                }
            }
            Some('[') => {
                self.at += 1;
                self.space();
                let marker = (self.peek(), self.chars.get(self.at + 1).copied());
                if let (Some(m @ ('B' | 'I' | 'L')), Some(';')) = marker {
                    self.at += 2;
                    return self.array(m);
                }
                let mut items = Vec::new();
                if self.peek() == Some(']') {
                    self.at += 1;
                    return Ok(Tag::List(List::empty()));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space();
                    match self.peek() {
                        Some(',') => self.at += 1,
                        Some(']') => {
                            self.at += 1;
                            return List::of(items).map(Tag::List).map_err(|e| self.fail(&e));
                        }
                        _ => return Err(self.fail("Expected ',' or ']'")),
                    }
                }
            }
            Some('"' | '\'') => Ok(Tag::String(self.quoted()?)),
            Some(_) => {
                let word = self.word();
                if word.is_empty() {
                    return Err(self.fail("Unexpected character"));
                }
                if let Some(tag) = number(&word) {
                    return Ok(tag);
                }
                Ok(match word.as_str() {
                    "true" => Tag::Byte(1),
                    "false" => Tag::Byte(0),
                    _ => Tag::String(word),
                })
            }
        }
    }

    fn array(&mut self, mark: char) -> Result<Tag, String> {
        let mut values = Vec::new();
        self.space();
        if self.peek() == Some(']') {
            self.at += 1;
        } else {
            loop {
                self.space();
                let word = self.word();
                let n = match number(&word) {
                    Some(tag) => tag.as_i64(),
                    None => None,
                }
                .ok_or_else(|| self.fail("Expected a whole number"))?;
                values.push(n);
                self.space();
                match self.peek() {
                    Some(',') => self.at += 1,
                    Some(']') => {
                        self.at += 1;
                        break;
                    }
                    _ => return Err(self.fail("Expected ',' or ']'")),
                }
            }
        }
        let range = |lo: i64, hi: i64| values.iter().all(|v| (lo..=hi).contains(v));
        Ok(match mark {
            'B' if range(i8::MIN as i64, i8::MAX as i64) => {
                Tag::ByteArray(values.into_iter().map(|v| v as i8).collect())
            }
            'I' if range(i32::MIN as i64, i32::MAX as i64) => {
                Tag::IntArray(values.into_iter().map(|v| v as i32).collect())
            }
            'L' => Tag::LongArray(values),
            _ => return Err(self.fail("A number is too large for this array")),
        })
    }
}

/// A bare word as a number, if it is one; otherwise SNBT treats it as text.
fn number(word: &str) -> Option<Tag> {
    let last = word.chars().last()?;
    let body = &word[..word.len() - last.len_utf8()];
    let numeric = |s: &str| {
        !s.is_empty()
            && s.chars().any(|c| c.is_ascii_digit())
            && s.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
    };
    match last.to_ascii_lowercase() {
        'b' if numeric(body) => body.parse().ok().map(Tag::Byte),
        's' if numeric(body) => body.parse().ok().map(Tag::Short),
        'l' if numeric(body) => body.parse().ok().map(Tag::Long),
        'f' if numeric(body) => body.parse().ok().map(Tag::Float),
        'd' if numeric(body) => body.parse().ok().map(Tag::Double),
        _ if word
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == '+') =>
        {
            word.parse().ok().map(Tag::Int)
        }
        _ if numeric(word) && word.contains(['.', 'e', 'E']) => word.parse().ok().map(Tag::Double),
        _ => None,
    }
}
