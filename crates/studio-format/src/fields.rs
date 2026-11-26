//! Reading resource fields the way the Paper add-on's `StudioText` does: wrong types fall back quietly.

use serde_json::{Map, Value};

pub type Data = Map<String, Value>;

pub fn text(data: &Data, key: &str, fallback: &str) -> String {
    data.get(key).and_then(Value::as_str).unwrap_or(fallback).to_string()
}

/// A field that may be text, a number or true/false (a variable's value), as text, like the Paper add-on's
/// `StudioText.scalar`: whole numbers without a `.0`.
pub fn scalar(data: &Data, key: &str, fallback: &str) -> String {
    match data.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => match number.as_f64() {
            Some(value) if value.fract() == 0.0 && value.abs() < 1e15 => format!("{}", value as i64),
            _ => number.to_string(),
        },
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => fallback.to_string(),
    }
}

/// The key holds text (an empty text counts: it means "show nothing").
pub fn has(data: &Data, key: &str) -> bool {
    data.get(key).is_some_and(Value::is_string)
}

pub fn number(data: &Data, key: &str, fallback: i64, least: i64, most: i64) -> i64 {
    let read = data.get(key).and_then(Value::as_f64).map(|one| one.round() as i64).unwrap_or(fallback);
    read.clamp(least, most)
}

/// A list of texts (anything else in it becomes an empty line), or one text as a list of one.
pub fn lines(data: &Data, key: &str) -> Vec<String> {
    match data.get(key) {
        Some(Value::Array(list)) => list.iter().map(|one| one.as_str().unwrap_or_default().to_string()).collect(),
        Some(Value::String(one)) => vec![one.clone()],
        _ => Vec::new(),
    }
}

/// A value a player typed (a dialog input) may go into a command as `<input:key>` only when it is a name,
/// a number or an id: letters, digits, `_`, `.` and `-`, up to 64. `<input:key|raw>` puts it in as typed.
pub fn safe_input(value: &str) -> bool {
    value.len() <= 64 && value.bytes().all(|one| one.is_ascii_alphanumeric() || matches!(one, b'_' | b'.' | b'-'))
}

/// A command's text with the typed inputs (`input:*` in `extra`) checked and `<input:key|raw>` filled by
/// `fill` around the other placeholders. `None` = an `<input:key>` isn't [`safe_input`], so nothing runs.
pub fn command_line(text: &str, extra: &[(String, String)], fill: impl FnOnce(&str) -> String) -> Option<String> {
    let mut out = text.to_string();
    let mut raw = Vec::new();
    for (key, value) in extra.iter().filter(|(key, _)| key.starts_with("input:")) {
        if out.contains(&format!("<{key}>")) && !safe_input(value) {
            return None;
        }
        let tag = format!("<{key}|raw>");
        if out.contains(&tag) {
            let mark = format!("\u{0}{}\u{0}", raw.len());
            out = out.replace(&tag, &mark);
            raw.push((mark, value.clone()));
        }
    }
    let mut out = fill(&out);
    for (mark, value) in raw {
        out = out.replace(&mark, &value);
    }
    Some(out)
}

/// Who a resource shows for: `show.worlds` (empty = every world) and `show.permission` (empty = everyone).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Show {
    pub worlds: Vec<String>,
    pub permission: String,
}

pub fn show(data: &Data) -> Show {
    let Some(Value::Object(show)) = data.get("show") else { return Show::default() };
    Show {
        worlds: lines(show, "worlds").into_iter().filter(|one| !one.trim().is_empty()).collect(),
        permission: text(show, "permission", "").trim().to_string(),
    }
}

impl Show {
    /// `has_permission` is only asked when a permission is set.
    pub fn allows(&self, world: &str, has_permission: impl FnOnce(&str) -> bool) -> bool {
        (self.worlds.is_empty() || self.worlds.iter().any(|one| one == world))
            && (self.permission.is_empty() || has_permission(&self.permission))
    }
}

/// One of `allowed` (case doesn't matter), else `fallback`.
pub fn choice(data: &Data, key: &str, allowed: &[&str], fallback: &'static str) -> String {
    let read = text(data, key, fallback).trim().to_ascii_lowercase();
    if allowed.contains(&read.as_str()) {
        read
    } else {
        fallback.to_string()
    }
}

/// A number from 0 to 1 (boss bar progress); anything else gives `fallback`.
pub fn fraction(data: &Data, key: &str, fallback: f64) -> f64 {
    data.get(key).and_then(Value::as_f64).filter(|one| one.is_finite()).unwrap_or(fallback).clamp(0.0, 1.0)
}

pub fn priority(data: &Data) -> i64 {
    number(data, "priority", 0, -1000, 1000)
}

/// The one with the highest `priority` that `shows`; on a tie the first wins (the list is sorted by name).
pub fn pick<T>(items: &[T], data_of: impl Fn(&T) -> &Data, shows: impl Fn(&Data) -> bool) -> Option<&T> {
    let mut best: Option<(&T, i64)> = None;
    for one in items {
        let data = data_of(one);
        if !shows(data) {
            continue;
        }
        let rank = priority(data);
        if best.is_none_or(|(_, top)| rank > top) {
            best = Some((one, rank));
        }
    }
    best.map(|(one, _)| one)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data(value: Value) -> Data {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn typed_inputs_in_commands() {
        let extra = vec![("input:name".to_string(), "Steve_1".to_string()), ("input:why".to_string(), "@a <player> hi".to_string())];
        let fill = |text: &str| text.replace("<player>", "alex").replace("<input:name>", "Steve_1");
        assert_eq!(command_line("ban <input:name> by <player>", &extra, fill).as_deref(), Some("ban Steve_1 by alex"));
        assert_eq!(command_line("say <input:why>", &extra, fill), None);
        // Raw text goes in last, so a placeholder typed into it stays as typed.
        assert_eq!(command_line("report <input:why|raw>", &extra, fill).as_deref(), Some("report @a <player> hi"));
        assert!(safe_input("") && safe_input("1.5") && !safe_input("a b") && !safe_input(&"a".repeat(65)));
    }

    #[test]
    fn reads_like_the_java_addon() {
        let one = data(
            json!({ "title": "Hi", "n": 12.6, "lines": ["a", 3, "b"], "single": "x", "show": { "worlds": ["world", " "], "permission": "vip" } }),
        );
        assert_eq!(text(&one, "title", "?"), "Hi");
        assert_eq!(text(&one, "n", "?"), "?");
        assert!(has(&one, "title") && !has(&one, "n"));
        assert_eq!(number(&one, "n", 1, 0, 10), 10);
        assert_eq!(number(&one, "missing", 5, 0, 10), 5);
        assert_eq!(lines(&one, "lines"), ["a", "", "b"]);
        assert_eq!(lines(&one, "single"), ["x"]);
        let rules = show(&one);
        assert_eq!(rules.worlds, ["world"]);
        assert!(rules.allows("world", |node| node == "vip"));
        assert!(!rules.allows("world_nether", |_| true));
        assert!(!rules.allows("world", |_| false));
        assert!(show(&data(json!({}))).allows("anywhere", |_| false));
        let bar = data(json!({ "color": "RED", "style": "wavy", "progress": 1.7, "half": 0.5 }));
        assert_eq!(choice(&bar, "color", &crate::BOSSBAR_COLORS, "purple"), "red");
        assert_eq!(choice(&bar, "style", &crate::BOSSBAR_STYLES, "progress"), "progress");
        assert_eq!(fraction(&bar, "progress", 1.0), 1.0);
        assert_eq!(fraction(&bar, "half", 1.0), 0.5);
        assert_eq!(fraction(&bar, "missing", 0.25), 0.25);
    }

    #[test]
    fn picks_the_highest_priority_first() {
        let all = [data(json!({ "id": "a" })), data(json!({ "id": "b", "priority": 5 })), data(json!({ "id": "c", "priority": 5 }))];
        let got = pick(&all, |one| one, |_| true).unwrap();
        assert_eq!(got["id"], "b");
        let got = pick(&all, |one| one, |one| one["id"] != "b").unwrap();
        assert_eq!(got["id"], "c");
        assert!(pick(&all, |one| one, |_| false).is_none());
    }
}
