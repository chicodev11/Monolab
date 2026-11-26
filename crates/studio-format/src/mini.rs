//! MiniMessage → Minecraft JSON text, for servers without Adventure (Pumpkin). Covers what the studio uses and
//! the app's preview (`src/lib/miniMessage.ts`) draws: colors, hex, `color:`, decorations and `!` to turn one
//! off, reset, newline, gradient and rainbow. Unknown tags stay as text, as in MiniMessage.

use serde_json::{json, Map, Value};

/// Minecraft's 16 named text colors.
const NAMED: [(&str, &str); 18] = [
    ("black", "#000000"),
    ("dark_blue", "#0000aa"),
    ("dark_green", "#00aa00"),
    ("dark_aqua", "#00aaaa"),
    ("dark_red", "#aa0000"),
    ("dark_purple", "#aa00aa"),
    ("gold", "#ffaa00"),
    ("gray", "#aaaaaa"),
    ("grey", "#aaaaaa"),
    ("dark_gray", "#555555"),
    ("dark_grey", "#555555"),
    ("blue", "#5555ff"),
    ("green", "#55ff55"),
    ("aqua", "#55ffff"),
    ("red", "#ff5555"),
    ("light_purple", "#ff55ff"),
    ("yellow", "#ffff55"),
    ("white", "#ffffff"),
];

const DECORATIONS: [(&str, usize); 11] = [
    ("b", 0),
    ("bold", 0),
    ("i", 1),
    ("italic", 1),
    ("em", 1),
    ("u", 2),
    ("underlined", 2),
    ("st", 3),
    ("strikethrough", 3),
    ("obf", 4),
    ("obfuscated", 4),
];
const DECORATION_KEYS: [&str; 5] = ["bold", "italic", "underlined", "strikethrough", "obfuscated"];

#[derive(Clone, Default, Debug, PartialEq)]
struct Style {
    color: Option<String>,
    /// Each decoration set on or off; unset ones follow the parent.
    decorations: [Option<bool>; 5],
    /// `<font:monolab:glyph>`, which glyph images (`:ruby:`) use.
    font: Option<String>,
}

/// `<font:name>` or `<font:namespace:name>` → `namespace:name`, when it is a valid resource name.
fn font_of(args: &[&str]) -> Option<String> {
    let joined = match args {
        [name] => format!("minecraft:{name}"),
        [space, name] => format!("{space}:{name}"),
        _ => return None,
    };
    let (space, name) = joined.split_once(':')?;
    let fine = |part: &str, slash: bool| {
        !part.is_empty()
            && part
                .bytes()
                .all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || matches!(one, b'_' | b'-' | b'.') || (slash && one == b'/'))
    };
    (fine(space, false) && fine(name, true)).then_some(joined)
}

#[derive(Clone, Debug, PartialEq)]
enum Item {
    Run(String, Style),
    Newline,
}

struct Frame {
    tag: String,
    style: Style,
    /// A gradient (stops) or rainbow (no stops) recolors each letter of what it wraps when it closes.
    paint: Option<(Option<Vec<String>>, usize)>,
}

/// Puts `\` before `<` and `\` so a value can't add tags.
pub fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('<', "\\<")
}

/// Fills `<name>` placeholders with escaped values. Other tags stay for the MiniMessage reader.
pub fn fill(text: &str, values: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('>');
        let name = close.map(|end| &after[..end]).filter(|name| {
            !name.is_empty()
                && name.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || matches!(one, b'_' | b':' | b'-'))
        });
        match name.and_then(|name| values.iter().find(|(key, _)| *key == name)) {
            Some((name, value)) => {
                out.push_str(&escape(value));
                rest = &after[name.len() + 1..];
            }
            None => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn color_of(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if lower.len() == 7 && lower.starts_with('#') && lower[1..].bytes().all(|one| one.is_ascii_hexdigit()) {
        return Some(lower);
    }
    NAMED.iter().find(|(one, _)| *one == lower).map(|(_, hex)| hex.to_string())
}

fn rgb(color: &str) -> [f64; 3] {
    [1, 3, 5].map(|at| u8::from_str_radix(&color[at..at + 2], 16).unwrap_or(0) as f64)
}

fn hex(parts: [f64; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", parts[0].round() as u8, parts[1].round() as u8, parts[2].round() as u8)
}

fn mix(stops: &[String], at: f64) -> String {
    if stops.len() == 1 {
        return stops[0].clone();
    }
    let spot = at.clamp(0.0, 1.0) * (stops.len() - 1) as f64;
    let from = (spot.floor() as usize).min(stops.len() - 2);
    let part = spot - from as f64;
    let (a, b) = (rgb(&stops[from]), rgb(&stops[from + 1]));
    hex([0, 1, 2].map(|one| a[one] + (b[one] - a[one]) * part))
}

fn rainbow(at: f64) -> String {
    let hue = at * 360.0;
    let k = |n: f64| (n + hue / 60.0) % 6.0;
    let f = |n: f64| 1.0 - k(n).min(4.0 - k(n)).clamp(0.0, 1.0);
    hex([f(5.0) * 255.0, f(3.0) * 255.0, f(1.0) * 255.0])
}

/// Reads MiniMessage into styled runs and line breaks.
fn parse(input: &str) -> Vec<Item> {
    let letters: Vec<char> = input.chars().collect();
    let mut items: Vec<Item> = Vec::new();
    let mut stack: Vec<Frame> = vec![Frame { tag: String::new(), style: Style::default(), paint: None }];
    let mut plain = String::new();

    fn push(items: &mut Vec<Item>, stack: &[Frame], plain: &mut String) {
        if !plain.is_empty() {
            items.push(Item::Run(std::mem::take(plain), stack[stack.len() - 1].style.clone()));
        }
    }
    fn close(items: &mut Vec<Item>, stack: &mut Vec<Frame>, at: usize) {
        let frames = stack.split_off(at);
        for frame in frames.into_iter().rev() {
            let Some((stops, first)) = frame.paint else { continue };
            let tail = items.split_off(first.min(items.len()));
            let count: usize = tail.iter().map(|one| if let Item::Run(text, _) = one { text.chars().count() } else { 0 }).sum();
            let mut index = 0usize;
            for one in tail {
                match one {
                    Item::Newline => items.push(Item::Newline),
                    Item::Run(text, style) => {
                        for letter in text.chars() {
                            let spot = if count <= 1 { 0.0 } else { index as f64 / (count - 1) as f64 };
                            index += 1;
                            let color = match &stops {
                                Some(stops) => mix(stops, spot),
                                None => rainbow(spot),
                            };
                            items.push(Item::Run(letter.to_string(), Style { color: Some(color), ..style.clone() }));
                        }
                    }
                }
            }
        }
    }

    let mut at = 0;
    while at < letters.len() {
        let letter = letters[at];
        if letter == '\\' && matches!(letters.get(at + 1), Some('<') | Some('\\')) {
            plain.push(letters[at + 1]);
            at += 2;
            continue;
        }
        if letter != '<' {
            plain.push(letter);
            at += 1;
            continue;
        }
        let Some(end) = letters[at..].iter().position(|one| *one == '>').map(|found| at + found) else {
            plain.extend(&letters[at..]);
            break;
        };
        let body: String = letters[at + 1..end].iter().collect();
        let handled = (|| {
            let closing = body.starts_with('/');
            let raw = if closing { &body[1..] } else { &body[..] };
            let negate = raw.starts_with('!');
            let raw = if negate { &raw[1..] } else { raw };
            let mut parts = raw.split(':');
            let name = parts.next().unwrap_or_default().to_ascii_lowercase();
            let args: Vec<&str> = parts.collect();
            let simple = name.strip_prefix('#').unwrap_or(&name);
            if simple.is_empty() || !simple.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'_') {
                return false;
            }
            if closing {
                let found = stack.iter().rposition(|one| one.tag == name);
                if let Some(found) = found.filter(|found| *found > 0) {
                    push(&mut items, &stack, &mut plain);
                    close(&mut items, &mut stack, found);
                }
                return true;
            }
            if name == "newline" || name == "br" {
                push(&mut items, &stack, &mut plain);
                items.push(Item::Newline);
                return true;
            }
            if name == "reset" {
                push(&mut items, &stack, &mut plain);
                close(&mut items, &mut stack, 1);
                return true;
            }
            let color = if matches!(name.as_str(), "color" | "colour" | "c") {
                args.first().and_then(|one| color_of(one))
            } else {
                color_of(&name)
            };
            let decoration = DECORATIONS.iter().find(|(one, _)| *one == name).map(|(_, slot)| *slot);
            let font = if name == "font" { font_of(&args) } else { None };
            if color.is_none() && decoration.is_none() && font.is_none() && name != "gradient" && name != "rainbow" {
                return false;
            }
            push(&mut items, &stack, &mut plain);
            let mut style = stack[stack.len() - 1].style.clone();
            if let Some(color) = color {
                style.color = Some(color);
            }
            if font.is_some() {
                style.font = font;
            }
            if let Some(slot) = decoration {
                style.decorations[slot] = Some(!negate);
            }
            let paint = match name.as_str() {
                "gradient" => {
                    let stops: Vec<String> = args.iter().filter_map(|one| color_of(one)).collect();
                    Some((Some(if stops.is_empty() { vec!["#ffffff".into(), "#000000".into()] } else { stops }), items.len()))
                }
                "rainbow" => Some((None, items.len())),
                _ => None,
            };
            stack.push(Frame { tag: name, style, paint });
            true
        })();
        if handled {
            at = end + 1;
        } else {
            plain.push('<');
            at += 1;
        }
    }
    push(&mut items, &stack, &mut plain);
    close(&mut items, &mut stack, 1);
    items
}

/// MiniMessage as a Minecraft JSON text component: `{"text":"","extra":[...]}`, lines joined with `\n`.
pub fn to_json(input: &str) -> Value {
    let mut extra: Vec<Value> = Vec::new();
    for item in parse(input) {
        match item {
            Item::Newline => extra.push(json!({ "text": "\n" })),
            Item::Run(text, style) => {
                let mut part = Map::new();
                part.insert("text".into(), Value::String(text));
                if let Some(color) = style.color {
                    part.insert("color".into(), Value::String(color));
                }
                for (slot, value) in style.decorations.iter().enumerate() {
                    if let Some(value) = value {
                        part.insert(DECORATION_KEYS[slot].into(), Value::Bool(*value));
                    }
                }
                if let Some(font) = style.font {
                    part.insert("font".into(), Value::String(font));
                }
                extra.push(Value::Object(part));
            }
        }
    }
    if extra.is_empty() {
        return json!({ "text": "" });
    }
    json!({ "text": "", "extra": extra })
}

/// The plain text a player would read.
pub fn plain(input: &str) -> String {
    parse(input)
        .into_iter()
        .map(|item| match item {
            Item::Run(text, _) => text,
            Item::Newline => "\n".into(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_decorations() {
        let got = to_json("<red>Hi <b>there</b></red> <#00ff00>x");
        let extra = got["extra"].as_array().unwrap();
        assert_eq!(extra[0], json!({ "text": "Hi ", "color": "#ff5555" }));
        assert_eq!(extra[1], json!({ "text": "there", "color": "#ff5555", "bold": true }));
        assert_eq!(extra[2], json!({ "text": " " }));
        assert_eq!(extra[3], json!({ "text": "x", "color": "#00ff00" }));
        assert_eq!(to_json("<b>a<!b>b")["extra"][1], json!({ "text": "b", "bold": false }));
        assert_eq!(to_json("<color:gold>g")["extra"][0]["color"], "#ffaa00");
    }

    #[test]
    fn newlines_reset_and_unknown_tags() {
        assert_eq!(plain("one<newline>two<br>three"), "one\ntwo\nthree");
        assert_eq!(to_json("<red>a<reset>b")["extra"][1], json!({ "text": "b" }));
        // An unmatched closing tag is dropped, as in the app preview; an unknown opening tag stays.
        assert_eq!(plain("<hover:x>keep</nope> 1 < 2"), "<hover:x>keep 1 < 2");
        assert_eq!(plain("\\<red>literal"), "<red>literal");
        assert_eq!(to_json(""), json!({ "text": "" }));
    }

    #[test]
    fn gradients_color_each_letter() {
        let got = to_json("<gradient:#ffffff:#000000>abc</gradient>");
        let colors: Vec<&str> = got["extra"].as_array().unwrap().iter().map(|one| one["color"].as_str().unwrap()).collect();
        assert_eq!(colors, ["#ffffff", "#808080", "#000000"]);
        let rainbow = to_json("<rainbow>ab</rainbow>");
        assert_eq!(rainbow["extra"][0]["color"], "#ff0000");
        assert_eq!(plain("<gradient:red:blue><b>x</b>y</gradient>"), "xy");
        assert_eq!(to_json("<gradient:red:blue><b>x</b>y</gradient>")["extra"][0]["bold"], true);
    }

    #[test]
    fn fonts_and_glyphs() {
        assert_eq!(to_json("<font:monolab:glyph>x</font>y")["extra"][0], json!({ "text": "x", "font": "monolab:glyph" }));
        assert_eq!(to_json("<font:uniform>x")["extra"][0]["font"], "minecraft:uniform");
        assert_eq!(plain("<font:Bad Name>x"), "<font:Bad Name>x");
        let glyphs = crate::glyphs::Glyphs::parse(r#"{"glyphs":{"ruby":{"char":"","advance":9}}}"#);
        let got = to_json(&glyphs.swap("<red><b>Gem :ruby: here"));
        let extra = got["extra"].as_array().unwrap();
        let gem = extra.iter().find(|one| one["text"] == "\u{e000}").unwrap();
        assert_eq!(gem["font"], "monolab:glyph");
        assert_eq!(gem["color"], "#ffffff");
        assert_eq!(gem["bold"], false);
        // Text after the glyph is back to red and bold.
        assert_eq!(extra.last().unwrap(), &json!({ "text": " here", "color": "#ff5555", "bold": true }));
    }

    #[test]
    fn placeholders_are_escaped() {
        let values = [("player", "<red>Steve".to_string()), ("online", "3".to_string())];
        let filled = fill("<gold><player> (<online>) <unknown>", &values);
        assert_eq!(filled, "<gold>\\<red>Steve (3) <unknown>");
        assert_eq!(plain(&filled), "<red>Steve (3) <unknown>");
    }
}
