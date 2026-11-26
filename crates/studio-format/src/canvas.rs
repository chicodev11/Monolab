//! Studio maker M2: canvas dialogs (`"layout": "canvas"`). Pictures, text and click areas at pixel spots, drawn
//! into one dialog body text with glyphs and spaces, the way the user's DialogUI does it (`UiCanvas.java`):
//!
//! - The body is text in 9-pixel lines, so `y` splits into the line (`y / 9`) and the spot in it (`y % 9`); the
//!   spot picks one of the `canvas_0` … `canvas_8` fonts, the same glyphs with lower ascents.
//! - `x` is reached with space characters (`monolab:space`), left or right.
//! - Every line is padded to the same width, and lines are centered, so they all start at the same `x`.
//! - A click on a glyph runs its click event: a picture's whole box (to the pixel), or for an area, spaces as
//!   wide as it across every 9-pixel line it touches (with a see-through glyph at both ends of the line, because
//!   the game only looks for clicks inside the drawn glyphs' box).
//! - Overlaps: on one line, later elements draw over earlier ones; a lower line draws over the lines above it.
//!
//! - `if` (conditions, like an action's `if`) hides an element for players it doesn't hold for; a button's
//!   `pressed` glyph shows for a moment after a click while the dialog stays open; the canvas's `after` says what
//!   a click does to the dialog (`close`, `stay`, or `refresh`: draw it again after the actions).
//!
//! The Pumpkin add-on draws with this; Paper has the same rules in `StudioCanvas.java` and the app's editor in
//! `canvasModel.ts`. Keep the three in step.

use crate::glyphs::{self, Glyph, Glyphs, Widths};
use serde_json::{json, Map, Value};

pub const LINE: i32 = 9;
/// The body text's padding on each side.
pub const BODY_PADDING: i32 = 4;
/// Dialog body widths go up to 1024.
pub const MOST_BODY: i32 = 1024;
/// Extra body width so a letter a little wider than the table never wraps a line.
pub const WRAP_SLACK: i32 = 32;
pub const MOST_ELEMENTS: usize = 400;
pub const TYPES: [&str; 4] = ["image", "text", "button", "area"];
pub const ALIGNS: [&str; 3] = ["left", "center", "right"];
/// What a click on the canvas does to the dialog: close it, leave it open, or draw it again after the actions
/// (new variables, conditions...).
pub const AFTERS: [&str; 3] = ["close", "stay", "refresh"];
/// Actions that take the player somewhere else, so a canvas isn't drawn again after them.
pub const LEAVING: [&str; 5] = ["dialog", "menu", "close", "shop", "trade"];
/// Most variables a dialog can start.
pub const MOST_VARIABLES: usize = 64;

/// How to draw a canvas for one player.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Write `"shadow_color": 0` so pictures get no shadow (servers whose text type can say so).
    pub shadow_off: bool,
    /// Elements whose `if` doesn't hold for this player: left out, with no click.
    pub hidden: std::collections::HashSet<usize>,
    /// A button drawn with its `pressed` glyph (just clicked).
    pub pressed: Option<usize>,
}

/// What a click does to the dialog: `after`, or the older `"close": false` (stay). Unknown values close.
pub fn after(canvas: &Map<String, Value>) -> &'static str {
    match canvas.get("after").and_then(Value::as_str) {
        Some("stay") => "stay",
        Some("refresh") => "refresh",
        Some("close") => "close",
        _ if canvas.get("close").and_then(Value::as_bool) == Some(false) => "stay",
        _ => "close",
    }
}

/// The leaving action types (`LEAVING`) anywhere in some actions, nested ones too.
pub fn leaving(actions: Option<&Value>) -> Vec<&'static str> {
    fn walk(value: &Value, out: &mut Vec<&'static str>) {
        match value {
            Value::Array(list) => list.iter().for_each(|one| walk(one, out)),
            Value::Object(map) => {
                if let Some(found) = map.get("type").and_then(Value::as_str).and_then(|kind| LEAVING.iter().find(|one| **one == kind)) {
                    if !out.contains(found) {
                        out.push(found);
                    }
                }
                // Conditions (an `if` step's list) can't open anything.
                map.iter().filter(|(key, _)| key.as_str() != "if").for_each(|(_, inner)| walk(inner, out));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(value) = actions {
        walk(value, &mut out);
    }
    out
}

/// The glyph a button shows just after a click, if it names one.
pub fn pressed_glyph(element: &Map<String, Value>) -> Option<String> {
    let name = text(element, "pressed");
    (text(element, "type") == "button" && glyphs::valid_name(&name)).then_some(name)
}

/// What is wrong with a dialog's starting `variables` (`{"name": value}`, set when the player has none yet).
pub fn variables_problem(dialog: &Map<String, Value>) -> Option<String> {
    let value = dialog.get("variables")?;
    let Some(vars) = value.as_object() else { return Some("\"variables\" must be an object of names and values.".into()) };
    if vars.len() > MOST_VARIABLES {
        return Some(format!("A dialog can start at most {MOST_VARIABLES} variables."));
    }
    for (name, value) in vars {
        if name.trim().is_empty() || name.len() > 64 {
            return Some("A variable's name must be 1 to 64 letters.".into());
        }
        if !(value.is_string() || value.is_number() || value.is_boolean()) {
            return Some(format!("The variable {name} needs a plain value (text or a number)."));
        }
    }
    None
}

/// A drawn canvas: the body text and the width to give the body.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    pub content: Value,
    pub width: i32,
}

fn int(data: &Map<String, Value>, key: &str, fallback: i32, least: i32, most: i32) -> i32 {
    data.get(key).and_then(Value::as_f64).map_or(fallback, |one| one.round() as i32).clamp(least, most)
}

fn text(data: &Map<String, Value>, key: &str) -> String {
    data.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// The canvas object of a dialog, if it is a canvas dialog.
pub fn canvas_of(dialog: &Map<String, Value>) -> Option<&Map<String, Value>> {
    if dialog.get("layout").and_then(Value::as_str) != Some("canvas") {
        return None;
    }
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    Some(dialog.get("canvas").and_then(Value::as_object).unwrap_or_else(|| EMPTY.get_or_init(Map::new)))
}

pub fn elements(canvas: &Map<String, Value>) -> Vec<Map<String, Value>> {
    match canvas.get("elements") {
        Some(Value::Array(list)) => list.iter().filter_map(|one| one.as_object().cloned()).collect(),
        _ => Vec::new(),
    }
}

/// What is wrong with a canvas, the way the add-ons check it before using the file. Glyph names are only
/// checked when the table is given (the add-on may start before the pack is built).
pub fn problem(canvas: &Map<String, Value>, glyphs: Option<&Glyphs>) -> Option<String> {
    let width = int(canvas, "width", 176, 1, 4096);
    if width + BODY_PADDING * 2 + WRAP_SLACK > MOST_BODY {
        return Some(format!("A canvas can be at most {} wide.", MOST_BODY - BODY_PADDING * 2 - WRAP_SLACK));
    }
    if canvas.get("after").is_some_and(|value| !value.as_str().is_some_and(|one| AFTERS.contains(&one))) {
        return Some("The canvas's \"after\" must be close, stay or refresh.".into());
    }
    let Some(Value::Array(list)) = canvas.get("elements") else { return None };
    if list.len() > MOST_ELEMENTS {
        return Some(format!("A canvas can have at most {MOST_ELEMENTS} elements."));
    }
    for (at, raw) in list.iter().enumerate() {
        let Some(one) = raw.as_object() else { return Some("Every canvas element must be an object.".into()) };
        let kind = text(one, "type");
        if !TYPES.contains(&kind.as_str()) {
            return Some(format!("Canvas element {} has no type (image, text, button or area).", at + 1));
        }
        if one.get("y").and_then(Value::as_f64).is_some_and(|y| y < 0.0) {
            return Some(format!("Canvas element {} is above the canvas (y below 0).", at + 1));
        }
        if one.get("if").is_some_and(|value| !value.is_array()) {
            return Some(format!("Canvas element {}: \"if\" must be a list of conditions.", at + 1));
        }
        let pressed = text(one, "pressed");
        if kind == "button" && !pressed.is_empty() {
            if !glyphs::valid_name(&pressed) {
                return Some(format!("Canvas element {}: the pressed picture must be a glyph name.", at + 1));
            }
            if let Some(table) = glyphs {
                match table.get(&pressed) {
                    None => return Some(format!("There is no glyph :{pressed}: (make it as a glyph image, then build the pack).")),
                    Some(glyph) if !glyph.canvas => return Some(format!(":{pressed}: is too small for its picture to go in a canvas. Make its height 8 or more.")),
                    _ => {}
                }
            }
        }
        if matches!(kind.as_str(), "image" | "button") {
            let name = text(one, "glyph");
            if !glyphs::valid_name(&name) {
                return Some(format!("Canvas element {} needs a glyph picture.", at + 1));
            }
            if let Some(table) = glyphs {
                match table.get(&name) {
                    None => return Some(format!("There is no glyph :{name}: (make it as a glyph image, then build the pack).")),
                    Some(glyph) if !glyph.canvas => return Some(format!(":{name}: is too small for its picture to go in a canvas. Make its height 8 or more.")),
                    _ => {}
                }
            }
        }
        if kind == "area" && (int(one, "w", 0, 0, 4096) <= 0 || int(one, "h", 0, 0, 4096) <= 0) {
            return Some(format!("Canvas area {} needs a width and height.", at + 1));
        }
    }
    None
}

enum Element {
    Image { glyph: Glyph, x: i32, y: i32, style: Map<String, Value> },
    Text { content: Value, plain: String, x: i32, y: i32, shadow: bool, advance: i32 },
    Area { x: i32, y: i32, w: i32, h: i32, style: Map<String, Value> },
}

/// One line being built: where the pen is, and runs of the same font and style merged into one part.
#[derive(Default)]
struct Line {
    parts: Vec<Value>,
    pending: String,
    key: Option<(String, Map<String, Value>)>,
    pen: i32,
    most: i32,
}

impl Line {
    fn move_to(&mut self, x: i32, plain: &Map<String, Value>) {
        let delta = x - self.pen;
        if delta != 0 {
            self.write(glyphs::SPACE_FONT, &glyphs::spaces(delta), plain, delta);
        }
    }

    fn write(&mut self, font: &str, text: &str, style: &Map<String, Value>, advance: i32) {
        let same = self.key.as_ref().is_some_and(|(was, wore)| was == font && wore == style);
        if !same {
            self.flush();
            self.key = Some((font.to_string(), style.clone()));
        }
        self.pending.push_str(text);
        self.pen += advance;
        self.most = self.most.max(self.pen);
    }

    fn component(&mut self, value: Value, advance: i32) {
        self.flush();
        self.key = None;
        self.parts.push(value);
        self.pen += advance;
        self.most = self.most.max(self.pen);
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let Some((font, style)) = &self.key else { return };
        let mut part = style.clone();
        part.insert("text".into(), Value::String(std::mem::take(&mut self.pending)));
        part.insert("font".into(), Value::String(font.clone()));
        self.parts.push(Value::Object(part));
    }

    fn build(mut self) -> Value {
        self.flush();
        if self.parts.is_empty() {
            return json!({ "text": "" });
        }
        json!({ "text": "", "extra": self.parts })
    }
}

fn canvas_font(offset: i32) -> String {
    format!("{}{}", glyphs::CANVAS_FONT, offset.clamp(0, LINE - 1))
}

/// Draws a canvas with every element shown and none pressed.
///
/// - `text`: MiniMessage (already with placeholders) → its text component JSON and plain text.
/// - `click`: an element's index and object → its `click_event` JSON, or `None` when it isn't clickable for this
///   player (no permission...).
/// - `shadow_off`: write `"shadow_color": 0` so pictures get no shadow (servers whose text type can say so).
pub fn render(canvas: &Map<String, Value>, glyphs: &Glyphs, draw_text: impl FnMut(&str) -> (Value, String), click: impl FnMut(usize, &Map<String, Value>) -> Option<Value>, shadow_off: bool) -> Result<Rendered, String> {
    render_with(canvas, glyphs, draw_text, click, &Options { shadow_off, ..Options::default() })
}

/// Draws a canvas for one player (`Options`: hidden elements, a pressed button).
pub fn render_with(canvas: &Map<String, Value>, glyphs: &Glyphs, mut draw_text: impl FnMut(&str) -> (Value, String), mut click: impl FnMut(usize, &Map<String, Value>) -> Option<Value>, options: &Options) -> Result<Rendered, String> {
    let shadow_off = options.shadow_off;
    let widths = Widths::vanilla();
    let width = int(canvas, "width", 176, 1, MOST_BODY);
    let height = int(canvas, "height", 166, 1, 2048);
    let mut plain = Map::new();
    if shadow_off {
        plain.insert("shadow_color".into(), json!(0));
    }
    let mut found = Vec::new();
    for (at, one) in elements(canvas).iter().enumerate().take(MOST_ELEMENTS) {
        if options.hidden.contains(&at) {
            continue;
        }
        let x = int(one, "x", 0, -2048, 2048);
        let y = int(one, "y", 0, 0, 2048);
        let mut style = plain.clone();
        let tooltip = text(one, "tooltip");
        let clickable = matches!(text(one, "type").as_str(), "button" | "area");
        if clickable {
            match click(at, one) {
                Some(event) => {
                    style.insert("click_event".into(), event);
                    if !tooltip.trim().is_empty() {
                        style.insert("hover_event".into(), json!({ "action": "show_text", "value": draw_text(&tooltip).0 }));
                    }
                }
                // Not for this player: a button still shows its picture, an area is left out.
                None if text(one, "type") == "area" => continue,
                None => {}
            }
        }
        match text(one, "type").as_str() {
            "image" | "button" => {
                let pressed = (options.pressed == Some(at)).then(|| pressed_glyph(one)).flatten().and_then(|name| glyphs.get(&name)).filter(|glyph| glyph.canvas);
                let Some(glyph) = pressed.or_else(|| glyphs.get(&text(one, "glyph")).filter(|glyph| glyph.canvas)).cloned() else { continue };
                found.push(Element::Image { glyph, x, y, style });
            }
            "text" => {
                let (content, words) = draw_text(&text(one, "text"));
                // A line break would push every line under it down; the canvas keeps one line.
                let words = words.replace('\n', " ");
                let advance = widths.text(&words);
                let shown = (advance - 1).max(0);
                let left = match text_align(one) {
                    "center" => x - shown / 2,
                    "right" => x - shown,
                    _ => x,
                };
                let shadow = one.get("shadow").and_then(Value::as_bool) != Some(false);
                found.push(Element::Text { content, plain: words, x: left, y, shadow, advance });
            }
            "area" => {
                let w = int(one, "w", 0, 0, MOST_BODY);
                let h = int(one, "h", 0, 0, 2048);
                if w > 0 && h > 0 && style.contains_key("click_event") {
                    found.push(Element::Area { x, y, w, h, style });
                }
            }
            _ => {}
        }
    }

    let text_line = |plain: &str, y: i32| if widths.all_bitmap(plain) { y / LINE } else { (y as f32 / LINE as f32).round() as i32 };
    let mut count = ((height + LINE - 1) / LINE).max(1);
    for one in &found {
        let last = match one {
            Element::Image { y, .. } => y / LINE,
            Element::Text { plain, y, .. } => text_line(plain.as_str(), *y),
            Element::Area { y, h, .. } => (y + h - 1) / LINE,
        };
        count = count.max(last + 1);
    }
    let count = count as usize;
    let mut lines: Vec<Line> = (0..count).map(|_| Line::default()).collect();
    let mut reach: Vec<Option<(i32, i32)>> = vec![None; count];
    for one in found {
        match one {
            Element::Image { glyph, x, y, style } => {
                let line = &mut lines[(y / LINE) as usize];
                line.move_to(x, &plain);
                line.write(&canvas_font(y % LINE), &glyph.text, &style, glyph.advance);
            }
            Element::Text { content, plain: words, x, y, shadow, advance } => {
                let mut wrapper = Map::new();
                wrapper.insert("text".into(), json!(""));
                if widths.all_bitmap(&words) && y % LINE != 0 {
                    wrapper.insert("font".into(), json!(format!("{}{}", glyphs::TEXT_FONT, y % LINE)));
                }
                if !shadow && shadow_off {
                    wrapper.insert("shadow_color".into(), json!(0));
                }
                wrapper.insert("extra".into(), json!([content]));
                let line = &mut lines[text_line(words.as_str(), y) as usize];
                line.move_to(x, &plain);
                line.component(Value::Object(wrapper), advance);
            }
            Element::Area { x, y, w, h, style } => {
                for at in (y / LINE)..=((y + h - 1) / LINE) {
                    let line = &mut lines[at as usize];
                    line.move_to(x, &plain);
                    line.write(glyphs::SPACE_FONT, &glyphs::spaces(w), &style, w);
                    let was = reach[at as usize].unwrap_or((i32::MAX, i32::MIN));
                    reach[at as usize] = Some((was.0.min(x), was.1.max(x + w)));
                }
            }
        }
    }
    if let Some((hit, advance)) = &glyphs.hit {
        for (at, span) in reach.iter().enumerate() {
            let Some((left, right)) = span else { continue };
            let line = &mut lines[at];
            line.move_to(*left, &plain);
            line.write(&canvas_font(0), hit, &plain, *advance);
            line.move_to(right - (advance - 1), &plain);
            line.write(&canvas_font(0), hit, &plain, *advance);
        }
    }
    let line_width = lines.iter().map(|one| one.most).fold(width, i32::max);
    let body = line_width + BODY_PADDING * 2 + WRAP_SLACK;
    if body > MOST_BODY {
        return Err(format!("The canvas is too wide to show ({body} > {MOST_BODY})."));
    }
    let mut extra = Vec::new();
    for (at, mut line) in lines.into_iter().enumerate() {
        line.move_to(line_width, &plain);
        if at > 0 {
            extra.push(json!({ "text": "\n" }));
        }
        extra.push(line.build());
    }
    Ok(Rendered { content: json!({ "text": "", "extra": extra }), width: body })
}

fn text_align(one: &Map<String, Value>) -> &'static str {
    match one.get("align").and_then(Value::as_str) {
        Some("center") => "center",
        Some("right") => "right",
        _ => "left",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Glyphs {
        Glyphs::parse(r#"{"glyphs":{"bg":{"char":"","advance":177,"width":176,"height":166},"buy":{"char":"","advance":17,"width":16,"height":16},"tiny":{"char":"","advance":3,"canvas":false}},"hit":{"char":"","advance":2}}"#)
    }

    fn data(text: &str) -> Map<String, Value> {
        serde_json::from_str(text).unwrap()
    }

    fn plain(text: &str) -> (Value, String) {
        (json!({ "text": text }), text.to_string())
    }

    fn flat(value: &Value, out: &mut Vec<Value>) {
        if value.get("text").and_then(Value::as_str).is_some_and(|one| !one.is_empty()) {
            out.push(value.clone());
        }
        if let Some(Value::Array(list)) = value.get("extra") {
            for one in list {
                flat(one, out);
            }
        }
    }

    #[test]
    fn places_pictures_on_lines_and_spots() {
        let canvas = data(r#"{"width":176,"height":166,"elements":[{"type":"image","glyph":"bg","x":0,"y":0},{"type":"button","glyph":"buy","x":20,"y":40,"tooltip":"Buy","actions":[]}]}"#);
        let drawn = render(&canvas, &table(), plain, |at, _| Some(json!({ "action": "custom", "id": format!("monolab:dialog/shop/c{at}") })), true).unwrap();
        // The background moves the pen 177 (176 drawn + the gap), one past the canvas width.
        assert_eq!(drawn.width, 177 + 8 + 32);
        let lines = drawn.content["extra"].as_array().unwrap();
        // 166 / 9 rounds up to 19 lines, joined by 18 line breaks.
        assert_eq!(lines.len(), 19 * 2 - 1);
        let mut first = Vec::new();
        flat(&lines[0], &mut first);
        assert_eq!(first[0]["font"], "monolab:canvas_0");
        assert_eq!(first[0]["text"], "\u{e000}");
        assert_eq!(first[0]["shadow_color"], 0);
        // y 40 = line 4, 4 pixels down.
        let mut fifth = Vec::new();
        flat(&lines[8], &mut fifth);
        let button = fifth.iter().find(|one| one["text"] == "\u{e001}").unwrap();
        assert_eq!(button["font"], "monolab:canvas_4");
        assert_eq!(button["click_event"]["id"], "monolab:dialog/shop/c1");
        assert_eq!(button["hover_event"]["value"]["text"], "Buy");
    }

    #[test]
    fn areas_span_lines_with_hit_glyphs() {
        let canvas = data(r#"{"width":100,"height":36,"elements":[{"type":"area","x":10,"y":12,"w":30,"h":10,"actions":[]}]}"#);
        let drawn = render(&canvas, &table(), plain, |_, _| Some(json!({ "action": "custom", "id": "x" })), false).unwrap();
        let lines = drawn.content["extra"].as_array().unwrap();
        for at in [1usize, 2] {
            let mut parts = Vec::new();
            flat(&lines[at * 2], &mut parts);
            assert_eq!(parts.iter().filter(|one| one["text"] == "\u{f8ff}").count(), 2, "line {at}");
            assert!(parts.iter().any(|one| one.get("click_event").is_some()));
        }
        let mut top = Vec::new();
        flat(&lines[0], &mut top);
        assert!(top.iter().all(|one| one.get("click_event").is_none()));
    }

    #[test]
    fn text_moves_by_pixels_unless_unifont() {
        let canvas = data(r#"{"width":100,"height":27,"elements":[{"type":"text","text":"Shop","x":50,"y":4,"align":"center"},{"type":"text","text":"상점","x":0,"y":13}]}"#);
        let drawn = render(&canvas, &table(), plain, |_, _| None, true).unwrap();
        let lines = drawn.content["extra"].as_array().unwrap();
        let wrapper = lines[0]["extra"].as_array().unwrap().iter().find(|one| one.get("extra").is_some()).unwrap();
        assert_eq!(wrapper["font"], "monolab:text_4");
        // 상점 at y 13 rounds to line 1 (y 9) and keeps the default font.
        let korean = lines[2]["extra"].as_array().unwrap().iter().find(|one| one.get("extra").is_some()).unwrap();
        assert!(korean.get("font").is_none());
    }

    #[test]
    fn problems() {
        let glyphs = table();
        assert_eq!(problem(&data(r#"{"elements":[{"type":"image","glyph":"bg"}]}"#), Some(&glyphs)), None);
        assert!(problem(&data(r#"{"elements":[{"type":"image","glyph":"nope"}]}"#), Some(&glyphs)).unwrap().contains(":nope:"));
        assert!(problem(&data(r#"{"elements":[{"type":"image","glyph":"nope"}]}"#), None).is_none());
        assert!(problem(&data(r#"{"elements":[{"type":"button","glyph":"tiny"}]}"#), Some(&glyphs)).unwrap().contains("height 8"));
        assert!(problem(&data(r#"{"elements":[{"type":"wobble"}]}"#), None).is_some());
        assert!(problem(&data(r#"{"elements":[{"type":"area","w":0,"h":4}]}"#), None).is_some());
        assert!(problem(&data(r#"{"width":2000}"#), None).is_some());
        assert!(problem(&data(r#"{"elements":[{"type":"text","y":-3}]}"#), None).is_some());
    }

    #[test]
    fn hidden_and_pressed_elements() {
        let canvas = data(r#"{"width":100,"height":18,"elements":[{"type":"button","glyph":"buy","pressed":"bg","x":0,"y":0,"actions":[]},{"type":"area","x":0,"y":9,"w":20,"h":9,"actions":[]}]}"#);
        let mut clicks = Vec::new();
        let options = Options { shadow_off: true, hidden: [1].into_iter().collect(), pressed: Some(0) };
        let drawn = render_with(&canvas, &table(), plain, |at, _| {
            clicks.push(at);
            Some(json!({ "action": "custom", "id": "x" }))
        }, &options)
        .unwrap();
        // The hidden area gets no click and isn't drawn; the button shows its pressed picture.
        assert_eq!(clicks, vec![0]);
        let mut parts = Vec::new();
        flat(&drawn.content, &mut parts);
        assert!(parts.iter().any(|one| one["text"] == "\u{e000}"));
        assert!(!parts.iter().any(|one| one["text"] == "\u{e001}"));
        assert!(!parts.iter().any(|one| one["text"] == "\u{f8ff}"));
        // Not pressed: the usual picture.
        let normal = render(&canvas, &table(), plain, |_, _| Some(json!({ "action": "custom", "id": "x" })), true).unwrap();
        let mut parts = Vec::new();
        flat(&normal.content, &mut parts);
        assert!(parts.iter().any(|one| one["text"] == "\u{e001}"));
    }

    #[test]
    fn after_and_leaving() {
        assert_eq!(after(&data(r#"{}"#)), "close");
        assert_eq!(after(&data(r#"{"close":false}"#)), "stay");
        assert_eq!(after(&data(r#"{"close":false,"after":"refresh"}"#)), "refresh");
        assert_eq!(after(&data(r#"{"after":"wobble"}"#)), "close");
        let steps: Value = serde_json::from_str(r#"[{"type":"message"},{"type":"if","if":[{"type":"quest"}],"then":[{"type":"menu","menu":"x"}]}]"#).unwrap();
        assert_eq!(leaving(Some(&steps)), vec!["menu"]);
        assert!(leaving(Some(&json!([{ "type": "set" }]))).is_empty());
        assert!(leaving(None).is_empty());
        assert_eq!(pressed_glyph(&data(r#"{"type":"button","pressed":"buy"}"#)).as_deref(), Some("buy"));
        assert_eq!(pressed_glyph(&data(r#"{"type":"image","pressed":"buy"}"#)), None);
    }

    #[test]
    fn new_keys_are_checked() {
        let glyphs = table();
        assert!(problem(&data(r#"{"after":"later"}"#), None).is_some());
        assert_eq!(problem(&data(r#"{"after":"refresh"}"#), None), None);
        assert!(problem(&data(r#"{"elements":[{"type":"text","if":"yes"}]}"#), None).is_some());
        assert_eq!(problem(&data(r#"{"elements":[{"type":"text","if":[]}]}"#), None), None);
        assert!(problem(&data(r#"{"elements":[{"type":"button","glyph":"buy","pressed":"nope"}]}"#), Some(&glyphs)).unwrap().contains(":nope:"));
        assert!(problem(&data(r#"{"elements":[{"type":"button","glyph":"buy","pressed":"Bad Name"}]}"#), None).is_some());
        assert_eq!(problem(&data(r#"{"elements":[{"type":"button","glyph":"buy","pressed":"bg"}]}"#), Some(&glyphs)), None);
        assert_eq!(variables_problem(&data(r#"{}"#)), None);
        assert_eq!(variables_problem(&data(r#"{"variables":{"tab":"0","gold":5}}"#)), None);
        assert!(variables_problem(&data(r#"{"variables":["tab"]}"#)).is_some());
        assert!(variables_problem(&data(r#"{"variables":{"tab":{"a":1}}}"#)).is_some());
    }

    #[test]
    fn a_canvas_too_wide_is_refused() {
        let canvas = data(r#"{"width":100,"height":9,"elements":[{"type":"image","glyph":"bg","x":900,"y":0}]}"#);
        assert!(render(&canvas, &table(), plain, |_, _| None, true).is_err());
        let dialog = data(r#"{"layout":"canvas"}"#);
        assert!(canvas_of(&dialog).is_some_and(|one| one.is_empty()));
        assert!(canvas_of(&data(r#"{"layout":"list"}"#)).is_none());
    }
}
