//! Studio maker M2 on Pumpkin: glyph images (`:ruby:`) in studio text, and canvas dialogs (`"layout": "canvas"`).
//! The layout itself is shared with the app's tests (`studio_format::canvas`); this file reads the pack's
//! `glyphs.json`, fills each text with the player's placeholders and turns the result into a dialog body.
//!
//! The canvas is one message body as wide as it draws, with the pictures' text shadow off (like Paper).
//!
//! What the game can't do (the same on Paper):
//! - A click on body text doesn't carry the dialog's inputs, so `<input:...>` stays empty for canvas buttons.
//!
//! A pressed button's picture shows until the next studio tick (half a second) here; Paper puts it back after 3 ticks.

use pumpkin_plugin_api::java_dialog::DialogBody;
use pumpkin_plugin_api::java_dialogs::DialogMessage;
use pumpkin_plugin_api::text::TextComponent;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;
use studio_format::canvas;
use studio_format::glyphs::Glyphs;
use studio_format::mini;

/// Click ids: `monolab:dialog/<id>/c<element>`, next to the grid buttons' `monolab:dialog/<id>/<button>`.
pub const CLICK_MARK: char = 'c';

struct Loaded {
    file: PathBuf,
    stamp: Option<(SystemTime, u64)>,
    table: Arc<Glyphs>,
}

fn state() -> &'static Mutex<Option<Loaded>> {
    static STATE: OnceLock<Mutex<Option<Loaded>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

/// Rereads `<studio>/pack/glyphs.json` when it changed. Cheap otherwise; call it from the studio's poll.
pub fn load(root: &Path) {
    let file = root.join("pack").join("glyphs.json");
    let stamp = std::fs::metadata(&file).ok().and_then(|meta| Some((meta.modified().ok()?, meta.len())));
    let mut held = state().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if held.as_ref().is_some_and(|one| one.file == file && one.stamp == stamp) {
        return;
    }
    let table = stamp.and_then(|_| std::fs::read_to_string(&file).ok()).map(|text| Glyphs::parse(&text)).unwrap_or_default();
    *held = Some(Loaded { file, stamp, table: Arc::new(table) });
}

/// The glyph table as last loaded (empty before the pack is built).
pub fn glyphs() -> Arc<Glyphs> {
    state().lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().map(|one| one.table.clone()).unwrap_or_default()
}

/// `:ruby:` → the glyph, for any studio text before its placeholders are filled.
pub fn swap(text: &str) -> String {
    glyphs().swap(text)
}

/// What a `font` file got wrong when the pack was built (`glyphs.json`'s problems), for its status.
pub fn font_problem(id: &str) -> Option<String> {
    glyphs().problems.get(id).cloned()
}

/// What is wrong with a dialog's canvas (and its starting variables) for Pumpkin.
pub fn problem(data: &Map<String, Value>) -> Option<String> {
    if let Some(text) = canvas::variables_problem(data) {
        return Some(text);
    }
    let canvas = canvas::canvas_of(data)?;
    let table = glyphs();
    let known = (!table.by_name.is_empty()).then_some(table.as_ref());
    canvas::problem(canvas, known)
}

/// What a click on the dialog's canvas does to the dialog (`close`, `stay` or `refresh`), if it is a canvas.
pub fn after(data: &Map<String, Value>) -> Option<&'static str> {
    canvas::canvas_of(data).map(canvas::after)
}

/// The canvas as the dialog's one body part, or `None` when the dialog isn't a canvas (or can't be drawn).
///
/// - `fill`: MiniMessage with this player's placeholders filled in (glyphs are swapped here).
/// - `allowed`: whether this player may use a button or area (its `permission`).
/// - `hidden`: elements whose `if` doesn't hold for this player; `pressed`: a button just clicked.
pub fn body(id: &str, data: &Map<String, Value>, mut fill: impl FnMut(&str) -> String, allowed: impl Fn(&Map<String, Value>) -> bool, hidden: HashSet<usize>, pressed: Option<usize>) -> Option<DialogBody> {
    let canvas = canvas::canvas_of(data)?;
    let table = glyphs();
    let options = canvas::Options { shadow_off: true, hidden, pressed };
    let drawn = canvas::render_with(
        canvas,
        &table,
        |text| {
            let filled = table.swap(&fill(text));
            (mini::to_json(&filled), mini::plain(&filled))
        },
        |at, one| allowed(one).then(|| json!({ "action": "custom", "id": format!("{}:dialog/{id}/{CLICK_MARK}{at}", super::play::TAG) })),
        &options,
    )
    .ok()?;
    let contents = TextComponent::from_json(&drawn.content.to_string()).ok()?;
    Some(DialogBody::Message(DialogMessage { contents, width: Some(drawn.width.clamp(1, 1024) as u32) }))
}

/// The element a `c<at>` click names (its index and object), for its `actions`, `permission` and `if`.
pub fn clicked(data: &Map<String, Value>, at: &str) -> Option<(usize, Map<String, Value>)> {
    let index: usize = at.strip_prefix(CLICK_MARK)?.parse().ok()?;
    let canvas = canvas::canvas_of(data)?;
    let one = canvas::elements(canvas).into_iter().nth(index)?;
    matches!(one.get("type").and_then(Value::as_str), Some("button" | "area")).then_some((index, one))
}

/// Every canvas element with its index and `if` conditions, for working out which to hide.
pub fn conditions(data: &Map<String, Value>) -> Vec<(usize, Value)> {
    let Some(canvas) = canvas::canvas_of(data) else { return Vec::new() };
    canvas::elements(canvas).into_iter().enumerate().take(canvas::MOST_ELEMENTS).filter_map(|(at, one)| one.get("if").filter(|value| value.as_array().is_some_and(|list| !list.is_empty())).map(|value| (at, value.clone()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_name_their_element() {
        let data: Map<String, Value> = serde_json::from_str(r#"{"layout":"canvas","canvas":{"elements":[{"type":"image","glyph":"bg"},{"type":"button","glyph":"buy","actions":[{"type":"message","text":"hi"}]}]}}"#).unwrap();
        assert!(clicked(&data, "c1").is_some_and(|(at, one)| at == 1 && one.contains_key("actions")));
        assert!(clicked(&data, "c0").is_none());
        assert!(clicked(&data, "1").is_none());
        assert!(clicked(&data, "c9").is_none());
    }

    #[test]
    fn wide_canvases_fit() {
        let data: Map<String, Value> = serde_json::from_str(r#"{"layout":"canvas","canvas":{"width":250,"elements":[]}}"#).unwrap();
        assert_eq!(problem(&data), None);
        let fits: Map<String, Value> = serde_json::from_str(r#"{"layout":"canvas","canvas":{"width":180,"elements":[]}}"#).unwrap();
        assert_eq!(problem(&fits), None);
        let list: Map<String, Value> = serde_json::from_str(r#"{"title":"x"}"#).unwrap();
        assert_eq!(problem(&list), None);
    }
}
