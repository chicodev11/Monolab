//! Studio maker M2: glyph images. The app builds `studio/pack/glyphs.json` with the resource pack
//! (`src-tauri/src/studio_pack_fonts.rs`): each `font` file's short name, the character it got and how wide it
//! draws. The add-ons read it to turn `:ruby:` in studio text into that character, and to lay out canvas dialogs.
//! Paper keeps its own copy of these rules (`StudioGlyphs.java`); keep the two in step.

use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The pack's font with every glyph at its own size (for text).
pub const GLYPH_FONT: &str = "monolab:glyph";
/// Spaces of ±1, 2, 4 … 256 pixels, for moving the pen in a canvas.
pub const SPACE_FONT: &str = "monolab:space";
/// `monolab:canvas_0` … `canvas_8`: the glyphs again, each drawn 0–8 pixels lower in a 9-pixel line.
pub const CANVAS_FONT: &str = "monolab:canvas_";
/// `monolab:text_1` … `text_8`: the game's own letters, 1–8 pixels lower.
pub const TEXT_FONT: &str = "monolab:text_";
/// Space characters inside [`SPACE_FONT`]: +2^k is `SPACE_UP + k`, -2^k is `SPACE_DOWN + k`.
pub const SPACE_UP: u32 = 0xE000;
pub const SPACE_DOWN: u32 = 0xE100;
pub const SPACE_POWERS: u32 = 8;
/// Names in `:name:`: a-z, 0-9 and `_`, up to 32.
pub const MOST_NAME: usize = 32;

/// One glyph image.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    /// The `font` file it came from.
    pub id: String,
    /// The character (one, from the private use area).
    pub text: String,
    /// How far it moves the pen, in GUI pixels (its drawn width + 1).
    pub advance: i32,
    pub width: i32,
    pub height: i32,
    /// It is in the canvas fonts (a picture too tall to pad isn't).
    pub canvas: bool,
}

/// `glyphs.json`, read. Empty when there is none yet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glyphs {
    pub by_name: BTreeMap<String, Glyph>,
    /// The see-through 1×9 glyph that makes a canvas line's space clickable: (character, advance).
    pub hit: Option<(String, i32)>,
    /// What went wrong with a font file, by file id (the add-ons show it as that file's error).
    pub problems: BTreeMap<String, String>,
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MOST_NAME && name.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'_')
}

/// A font file's `:name:`: its `name`, else its id with `-` as `_`.
pub fn name_of(id: &str, data: &serde_json::Map<String, Value>) -> String {
    match data.get("name").and_then(Value::as_str).map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => id.replace('-', "_"),
    }
}

impl Glyphs {
    pub fn parse(text: &str) -> Self {
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else { return Self::default() };
        let mut out = Self::default();
        if let Some(Value::Object(glyphs)) = root.get("glyphs") {
            for (name, one) in glyphs {
                let Some(one) = one.as_object() else { continue };
                let Some(text) = one.get("char").and_then(Value::as_str) else { continue };
                if !valid_name(name) || text.chars().count() != 1 {
                    continue;
                }
                let number = |key: &str| one.get(key).and_then(Value::as_i64).unwrap_or(0).clamp(0, 1024) as i32;
                out.by_name.insert(
                    name.clone(),
                    Glyph {
                        id: one.get("id").and_then(Value::as_str).unwrap_or(name).to_string(),
                        text: text.to_string(),
                        advance: number("advance"),
                        width: number("width"),
                        height: number("height"),
                        canvas: one.get("canvas").and_then(Value::as_bool) != Some(false),
                    },
                );
            }
        }
        if let Some(Value::Object(hit)) = root.get("hit") {
            if let Some(text) = hit.get("char").and_then(Value::as_str) {
                out.hit = Some((text.to_string(), hit.get("advance").and_then(Value::as_i64).unwrap_or(2) as i32));
            }
        }
        if let Some(Value::Object(problems)) = root.get("problems") {
            for (id, text) in problems {
                if let Some(text) = text.as_str() {
                    out.problems.insert(id.clone(), text.to_string());
                }
            }
        }
        out
    }

    pub fn get(&self, name: &str) -> Option<&Glyph> {
        self.by_name.get(name.trim_matches(':'))
    }

    /// A menu's title with its `background` glyph drawn under the whole screen: the pen goes back to the screen's
    /// left edge (the title starts 8 pixels in), draws the glyph, and comes back so the title sits where it was.
    /// The glyph's `ascent` in its font file decides how high it sits. An unknown name leaves the title alone.
    pub fn menu_title(&self, background: &str, title: &str) -> String {
        let name = background.trim();
        match self.get(name) {
            Some(glyph) if !name.is_empty() => menu_title_with(glyph, title),
            _ => title.to_string(),
        }
    }

    /// `:ruby:` → the glyph in MiniMessage, white and plain so the text around it doesn't tint it. Unknown names
    /// and anything inside a `<tag>` stay as they are. `\:` keeps a colon.
    pub fn swap(&self, text: &str) -> String {
        if self.by_name.is_empty() || !text.contains(':') {
            return text.to_string();
        }
        let letters: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        while at < letters.len() {
            let letter = letters[at];
            if letter == '\\' && letters.get(at + 1) == Some(&':') {
                out.push(':');
                at += 2;
                continue;
            }
            if letter == '\\' && matches!(letters.get(at + 1), Some('<') | Some('\\')) {
                out.push(letter);
                out.push(letters[at + 1]);
                at += 2;
                continue;
            }
            if letter == '<' {
                // Copy the tag whole, so `<gradient:red:blue>` never meets a glyph named red.
                match letters[at..].iter().position(|one| *one == '>') {
                    Some(end) => {
                        out.extend(&letters[at..=at + end]);
                        at += end + 1;
                    }
                    None => {
                        out.extend(&letters[at..]);
                        at = letters.len();
                    }
                }
                continue;
            }
            if letter == ':' {
                let name: String = letters[at + 1..].iter().take_while(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || **one == '_').take(MOST_NAME + 1).collect();
                let closes = letters.get(at + 1 + name.chars().count()) == Some(&':');
                if closes {
                    if let Some(glyph) = self.by_name.get(&name) {
                        out.push_str(&glyph_mini(&glyph.text));
                        at += name.chars().count() + 2;
                        continue;
                    }
                }
            }
            out.push(letter);
            at += 1;
        }
        out
    }
}

/// One glyph character as MiniMessage. `</font>` closes the tags opened after it too.
pub fn glyph_mini(text: &str) -> String {
    format!("<font:{GLYPH_FONT}><white><!bold><!italic><!underlined><!strikethrough><!obfuscated>{text}</font>")
}

/// How far in a menu's title starts from the screen's left edge.
pub const MENU_TITLE_LEFT: i32 = 8;

fn menu_title_with(glyph: &Glyph, title: &str) -> String {
    let space = |pixels: i32| format!("<font:{SPACE_FONT}>{}</font>", spaces(pixels));
    format!("{}{}{}{title}", space(-MENU_TITLE_LEFT), glyph_mini(&glyph.text), space(MENU_TITLE_LEFT - glyph.advance))
}

/// The pen moves `pixels` (left when below 0) in [`SPACE_FONT`].
pub fn spaces(pixels: i32) -> String {
    let mut out = String::new();
    let base = if pixels >= 0 { SPACE_UP } else { SPACE_DOWN };
    let mut left = pixels.unsigned_abs();
    let largest = 1u32 << SPACE_POWERS;
    while left >= largest * 2 {
        out.push(char::from_u32(base + SPACE_POWERS).unwrap_or(' '));
        left -= largest;
    }
    for power in (0..=SPACE_POWERS).rev() {
        if left & (1 << power) != 0 {
            out.push(char::from_u32(base + power).unwrap_or(' '));
        }
    }
    out
}

/// The game's own letter widths (`minecraft:default`, from Minecraft 26.3's client), in GUI pixels with the 1-pixel
/// gap. From the user's DialogUI (`vanilla_widths.txt`: start, end, width, `b` bitmap or `u` unifont).
pub struct Widths {
    starts: Vec<u32>,
    ends: Vec<u32>,
    widths: Vec<u8>,
    bitmap: Vec<bool>,
}

/// Letters the table doesn't know (emoji...).
const UNKNOWN_WIDTH: i32 = 8;

impl Widths {
    pub fn vanilla() -> &'static Widths {
        static TABLE: OnceLock<Widths> = OnceLock::new();
        TABLE.get_or_init(|| Widths::parse(include_str!("vanilla_widths.txt")))
    }

    pub fn parse(text: &str) -> Self {
        let mut out = Widths { starts: Vec::new(), ends: Vec::new(), widths: Vec::new(), bitmap: Vec::new() };
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split(' ').collect();
            if parts.len() < 4 {
                continue;
            }
            let (Ok(start), Ok(end), Ok(width)) = (u32::from_str_radix(parts[0], 16), u32::from_str_radix(parts[1], 16), parts[2].parse::<u8>()) else { continue };
            out.starts.push(start);
            out.ends.push(end);
            out.widths.push(width);
            out.bitmap.push(parts[3] == "b");
        }
        out
    }

    fn find(&self, code: u32) -> Option<usize> {
        let at = match self.starts.binary_search(&code) {
            Ok(at) => at,
            Err(0) => return None,
            Err(at) => at - 1,
        };
        (code <= self.ends[at]).then_some(at)
    }

    pub fn width(&self, letter: char) -> i32 {
        self.find(letter as u32).map_or(UNKNOWN_WIDTH, |at| i32::from(self.widths[at]))
    }

    /// The whole text's advance (not bold; bold adds 1 per letter).
    pub fn text(&self, text: &str) -> i32 {
        text.chars().map(|one| self.width(one)).sum()
    }

    /// Bitmap letters can be moved down 1 pixel at a time (the `text_N` fonts); unifont ones (Korean...) can't.
    pub fn all_bitmap(&self, text: &str) -> bool {
        text.chars().all(|one| self.find(one as u32).is_some_and(|at| self.bitmap[at]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Glyphs {
        Glyphs::parse(r#"{"schema":1,"glyphs":{"ruby":{"id":"ruby","char":"","advance":9,"width":8,"height":8},"red":{"char":"","advance":5}},"hit":{"char":"","advance":2},"problems":{"broken":"No picture."}}"#)
    }

    #[test]
    fn reads_the_table() {
        let glyphs = table();
        assert_eq!(glyphs.get("ruby").map(|one| one.advance), Some(9));
        assert_eq!(glyphs.get(":ruby:").map(|one| one.text.as_str()), Some("\u{e000}"));
        assert_eq!(glyphs.hit, Some(("\u{f8ff}".to_string(), 2)));
        assert_eq!(glyphs.problems.get("broken").map(String::as_str), Some("No picture."));
        assert_eq!(Glyphs::parse("nope"), Glyphs::default());
    }

    #[test]
    fn menu_backgrounds_go_under_the_title() {
        let mut table = Glyphs::default();
        table.by_name.insert("menu_bg".into(), Glyph { id: "menu-bg".into(), text: "\u{E201}".into(), advance: 177, width: 176, height: 222, canvas: false });
        let title = table.menu_title("menu_bg", "<gold>Shop");
        assert!(title.starts_with(&format!("<font:{SPACE_FONT}>{}</font>", spaces(-8))), "{title}");
        assert!(title.contains(&glyph_mini("\u{E201}")));
        assert!(title.ends_with(&format!("<font:{SPACE_FONT}>{}</font><gold>Shop", spaces(8 - 177))), "{title}");
        assert_eq!(table.menu_title("nope", "Shop"), "Shop");
        assert_eq!(table.menu_title("", "Shop"), "Shop");
    }

    #[test]
    fn swaps_names_outside_tags() {
        let glyphs = table();
        assert_eq!(glyphs.swap("Hi :ruby:!"), format!("Hi {}!", glyph_mini("\u{e000}")));
        assert_eq!(glyphs.swap("<gradient:red:blue>x</gradient>"), "<gradient:red:blue>x</gradient>");
        assert_eq!(glyphs.swap(":nope: 12:30 \\:ruby:"), ":nope: 12:30 :ruby:");
        assert_eq!(glyphs.swap("::ruby::"), format!(":{}:", glyph_mini("\u{e000}")));
        assert_eq!(Glyphs::default().swap(":ruby:"), ":ruby:");
    }

    #[test]
    fn spaces_add_up() {
        let advance = |text: &str| -> i32 {
            text.chars()
                .map(|one| {
                    let code = one as u32;
                    if code >= SPACE_DOWN { -(1 << (code - SPACE_DOWN)) } else { 1 << (code - SPACE_UP) }
                })
                .sum()
        };
        for pixels in [0, 1, 13, 255, 256, 511, 600, 1000, -1, -300, -700] {
            assert_eq!(advance(&spaces(pixels)), pixels, "{pixels}");
        }
        assert_eq!(spaces(13).chars().count(), 3);
    }

    #[test]
    fn game_widths() {
        let widths = Widths::vanilla();
        assert_eq!(widths.width(' '), 4);
        assert_eq!(widths.width('i'), 2);
        assert_eq!(widths.text("Shop"), 6 + 6 + 6 + 6);
        assert!(widths.all_bitmap("Shop!"));
        assert!(!widths.all_bitmap("상점"));
        assert_eq!(widths.width('상'), 8);
    }
}
