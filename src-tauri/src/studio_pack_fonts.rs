//! Studio maker M2 (`docs/studio-maker.md`): glyph images. A `font` file (`studio/font/<id>.json`) turns a texture
//! asset into one character of the pack's own font, so `:ruby:` can be used in any studio text:
//!
//! ```json
//! { "schema": 1, "image": "ruby", "height": 8, "ascent": 7, "name": "ruby" }
//! ```
//!
//! What goes into the pack (all in our own `monolab:` fonts, so they stack with other packs' fonts and never take
//! their characters):
//!
//! - `monolab:glyph`: every glyph at its `height` and `ascent`, for text.
//! - `monolab:canvas_0` … `canvas_8`: every glyph again at the same size, 0–8 pixels down a 9-pixel line, for
//!   canvas dialogs. Pictures under 8 high are padded at the bottom (the game refuses an ascent above the height).
//! - `monolab:space`: ±1, 2, 4 … 256-pixel spaces that move the pen.
//! - `monolab:text_1` … `text_8`: the game's letters 1–8 pixels down (same game textures, so the pack doesn't grow).
//!
//! Beside the zip, `studio/pack/glyphs.json` tells the add-ons each name's character and width.
//! Ported from the user's DialogUI (`PackBuilder.java`, `Spaces.java`); `studio_format::glyphs` reads the table.

use crate::studio_pack::{asset_name, Ids, Source, NAMESPACE};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Cursor;
use studio_format::glyphs::{self as table, SPACE_DOWN, SPACE_POWERS, SPACE_UP};

/// The first glyph character, and the see-through one that makes canvas click areas work (never a glyph's).
pub const FIRST_GLYPH: u32 = 0xE000;
pub const HIT: u32 = 0xF8FF;
pub const LINE: i64 = 9;
pub const BASE_ASCENT: i64 = 7;
/// A glyph's picture fits the game's 256-pixel font sheet (with its 1-pixel gap).
pub const MOST_PICTURE: u32 = 255;
/// Where `glyphs.json` goes in the list `font_files` returns (next to the zip, not in it).
pub const TABLE_FILE: &str = "glyphs.json";
/// The game's bitmap providers for `minecraft:default` (26.3), copied from DialogUI.
const VANILLA: &str = include_str!("studio_vanilla_fonts.json");

fn png(picture: &image::RgbaImage) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    // An in-memory PNG of a picture we made can't fail to encode.
    let _ = picture.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// The rightmost column with any color, plus one (how the game measures a glyph).
fn drawn_width(picture: &image::RgbaImage) -> u32 {
    (0..picture.width()).rev().find(|x| (0..picture.height()).any(|y| picture.get_pixel(*x, y)[3] != 0)).map_or(0, |x| x + 1)
}

fn number(data: &Map<String, Value>, key: &str) -> Option<i64> {
    data.get(key).and_then(Value::as_f64).map(|one| one.round() as i64)
}

fn json_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec_pretty(value).unwrap_or_default()
}

fn bitmap(file: &str, height: i64, ascent: i64, text: &str) -> Value {
    json!({ "type": "bitmap", "file": file, "height": height, "ascent": ascent, "chars": [text] })
}

/// The pack files for the studio's glyph images, and `glyphs.json` (path [`TABLE_FILE`]). Other paths start with
/// `assets/` and go into the zip. `fonts` are the `font` files (id, checked data); `sources` are the pack's
/// sources, of which only textures are read. Characters come from `ids` (kind `glyph`), so they never move.
pub fn font_files(fonts: &[(String, Map<String, Value>)], sources: &[Source], ids: &mut Ids) -> Vec<(String, Vec<u8>)> {
    let textures: BTreeMap<&str, &[u8]> = sources
        .iter()
        .filter(|(folder, file, _)| folder == "textures" && file.ends_with(".png"))
        .filter_map(|(folder, file, bytes)| asset_name(folder, file).map(|name| (name, bytes.as_slice())))
        .collect();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut glyph_font: Vec<Value> = Vec::new();
    let mut canvas_fonts: Vec<Vec<Value>> = (0..LINE).map(|_| Vec::new()).collect();
    let mut glyphs = Map::new();
    let mut problems = Map::new();
    let mut sorted: Vec<&(String, Map<String, Value>)> = fonts.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    for (id, data) in sorted {
        if !studio_format::enabled(data) {
            continue;
        }
        let name = table::name_of(id, data);
        if !table::valid_name(&name) {
            problems.insert(id.clone(), json!("The name (used as :name:) may only use a-z, 0-9 and _, up to 32."));
            continue;
        }
        if glyphs.contains_key(&name) {
            problems.insert(id.clone(), json!(format!("Another glyph is already called :{name}:.")));
            continue;
        }
        let image = data.get("image").and_then(Value::as_str).unwrap_or_default().trim().trim_end_matches(".png").to_string();
        let Some(bytes) = textures.get(image.as_str()) else {
            problems.insert(id.clone(), json!(if image.is_empty() { "Pick a picture from the studio's textures.".to_string() } else { format!("The picture {image} isn't in the studio's textures.") }));
            continue;
        };
        let Ok(picture) = image::load_from_memory(bytes).map(|one| one.to_rgba8()) else {
            problems.insert(id.clone(), json!(format!("{image}.png couldn't be read as a picture.")));
            continue;
        };
        let (w, h) = (picture.width(), picture.height());
        if w == 0 || h == 0 || w > MOST_PICTURE || h > MOST_PICTURE {
            problems.insert(id.clone(), json!(format!("Glyph pictures can be at most {MOST_PICTURE}×{MOST_PICTURE} ({image} is {w}×{h}).")));
            continue;
        }
        let height = number(data, "height").unwrap_or(i64::from(h)).clamp(1, 256);
        let ascent = number(data, "ascent").unwrap_or(BASE_ASCENT.min(height)).clamp(-256, height);
        let code = ids.number("glyph", id, FIRST_GLYPH);
        let Some(letter) = char::from_u32(code).filter(|_| code < HIT) else {
            problems.insert(id.clone(), json!("There are too many glyphs (about 6,400 at most)."));
            continue;
        };
        let text = letter.to_string();
        let scale = height as f64 / f64::from(h);
        let advance = (0.5 + f64::from(drawn_width(&picture)) * scale) as i64 + 1;
        let file = format!("{NAMESPACE}:studio/{image}.png");
        glyph_font.push(bitmap(&file, height, ascent, &text));

        // The canvas fonts need a height of 8 or more. A smaller one is padded below with see-through rows, a whole
        // number of picture heights so the scale stays the same.
        let mut rows: i64 = 1;
        while height * rows < BASE_ASCENT + 1 {
            rows += 1;
        }
        let canvas = u64::from(h) * rows as u64 <= u64::from(MOST_PICTURE);
        if canvas {
            let canvas_file = if rows == 1 {
                file.clone()
            } else {
                let mut padded = image::RgbaImage::new(w, h * rows as u32);
                image::imageops::overlay(&mut padded, &picture, 0, 0);
                let path = format!("font/pad/{image}_{rows}");
                files.insert(format!("assets/{NAMESPACE}/textures/{path}.png"), png(&padded));
                format!("{NAMESPACE}:{path}.png")
            };
            for (offset, providers) in canvas_fonts.iter_mut().enumerate() {
                providers.push(bitmap(&canvas_file, height * rows, BASE_ASCENT - offset as i64, &text));
            }
        }
        glyphs.insert(
            name,
            json!({
                "id": id,
                "char": text,
                "advance": advance,
                "width": (f64::from(w) * scale).round() as i64,
                "height": height,
                "ascent": ascent,
                "canvas": canvas,
            }),
        );
    }

    // The see-through 1×9 glyph (alpha 1 of 255: the game draws nothing under 0.1) at the ends of click lines.
    let mut hit = image::RgbaImage::new(1, LINE as u32);
    for y in 0..LINE as u32 {
        hit.put_pixel(0, y, image::Rgba([255, 255, 255, 1]));
    }
    files.insert(format!("assets/{NAMESPACE}/textures/font/hit.png"), png(&hit));
    let hit_text = char::from_u32(HIT).map(String::from).unwrap_or_default();
    canvas_fonts[0].push(bitmap(&format!("{NAMESPACE}:font/hit.png"), LINE, BASE_ASCENT, &hit_text));

    files.insert(format!("assets/{NAMESPACE}/font/glyph.json"), json_bytes(&json!({ "providers": glyph_font })));
    for (offset, providers) in canvas_fonts.into_iter().enumerate() {
        files.insert(format!("assets/{NAMESPACE}/font/canvas_{offset}.json"), json_bytes(&json!({ "providers": providers })));
    }
    files.insert(format!("assets/{NAMESPACE}/font/space.json"), json_bytes(&space_font()));
    for (offset, font) in text_fonts() {
        files.insert(format!("assets/{NAMESPACE}/font/text_{offset}.json"), json_bytes(&font));
    }
    let table = json!({
        "schema": 1,
        "font": table::GLYPH_FONT,
        "space": table::SPACE_FONT,
        "canvas": table::CANVAS_FONT,
        "text": table::TEXT_FONT,
        "hit": { "char": hit_text, "advance": 2, "width": 1 },
        "glyphs": glyphs,
        "problems": problems,
    });
    files.insert(TABLE_FILE.to_string(), json_bytes(&table));
    files.into_iter().collect()
}

/// `monolab:space`: +2^k at `SPACE_UP + k`, -2^k at `SPACE_DOWN + k`.
fn space_font() -> Value {
    let mut advances = Map::new();
    for power in 0..=SPACE_POWERS {
        for (base, sign) in [(SPACE_UP, 1i64), (SPACE_DOWN, -1i64)] {
            if let Some(letter) = char::from_u32(base + power) {
                advances.insert(letter.to_string(), json!(sign * (1i64 << power)));
            }
        }
    }
    json!({ "providers": [{ "type": "space", "advances": advances }] })
}

/// `monolab:text_1` … `text_8`: the game's default font with each bitmap's ascent lowered.
fn text_fonts() -> Vec<(i64, Value)> {
    let vanilla: Value = serde_json::from_str(VANILLA).unwrap_or(Value::Null);
    let providers = vanilla.get("providers").and_then(Value::as_array).cloned().unwrap_or_default();
    (1..LINE)
        .map(|offset| {
            let mut out = vec![json!({ "type": "reference", "id": "minecraft:include/space" })];
            for one in &providers {
                let mut one = one.clone();
                let ascent = one.get("ascent").and_then(Value::as_i64).unwrap_or(BASE_ASCENT);
                one["ascent"] = json!(ascent - offset);
                out.push(one);
            }
            out.push(json!({ "type": "reference", "id": "minecraft:include/unifont" }));
            (offset, json!({ "providers": out }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w`×`h` picture whose first `lit` columns have color.
    fn picture(w: u32, h: u32, lit: u32) -> Vec<u8> {
        let mut one = image::RgbaImage::new(w, h);
        for x in 0..lit {
            for y in 0..h {
                one.put_pixel(x, y, image::Rgba([200, 20, 20, 255]));
            }
        }
        png(&one)
    }

    fn font(text: &str) -> Map<String, Value> {
        serde_json::from_str(text).unwrap()
    }

    fn get(files: &[(String, Vec<u8>)], path: &str) -> Value {
        let bytes = &files.iter().find(|(one, _)| one == path).unwrap_or_else(|| panic!("no {path}")).1;
        serde_json::from_slice::<Value>(bytes).unwrap()
    }

    fn sources() -> Vec<Source> {
        vec![
            ("textures".into(), "ruby.png".into(), picture(16, 16, 12)),
            ("textures".into(), "dot.png".into(), picture(4, 4, 4)),
            ("textures".into(), "bg.png".into(), picture(176, 166, 176)),
            ("sounds".into(), "ruby.png".into(), b"OggS".to_vec()),
        ]
    }

    #[test]
    fn glyphs_get_characters_fonts_and_widths() {
        let fonts = vec![
            ("ruby".to_string(), font(r#"{"schema":1,"image":"ruby","height":8,"ascent":7}"#)),
            ("shop-bg".to_string(), font(r#"{"schema":1,"image":"bg.png"}"#)),
        ];
        let mut ids = Ids::default();
        let files = font_files(&fonts, &sources(), &mut ids);
        let table = get(&files, TABLE_FILE);
        assert_eq!(table["glyphs"]["ruby"]["char"], "\u{e000}");
        // 12 lit columns at half size → 6, plus the 1-pixel gap.
        assert_eq!(table["glyphs"]["ruby"]["advance"], 7);
        assert_eq!(table["glyphs"]["ruby"]["width"], 8);
        assert_eq!(table["glyphs"]["shop_bg"]["char"], "\u{e001}");
        assert_eq!(table["glyphs"]["shop_bg"]["height"], 166);
        assert_eq!(table["glyphs"]["shop_bg"]["advance"], 177);
        assert_eq!(table["hit"]["char"], "\u{f8ff}");

        let glyph = get(&files, "assets/monolab/font/glyph.json");
        assert_eq!(glyph["providers"][0], json!({ "type": "bitmap", "file": "monolab:studio/ruby.png", "height": 8, "ascent": 7, "chars": ["\u{e000}"] }));
        let low = get(&files, "assets/monolab/font/canvas_8.json");
        assert_eq!(low["providers"][0]["ascent"], -1);
        assert_eq!(low["providers"][1]["file"], "monolab:studio/bg.png");
        let top = get(&files, "assets/monolab/font/canvas_0.json");
        assert!(top["providers"].as_array().unwrap().iter().any(|one| one["chars"][0] == "\u{f8ff}"));
        assert!(files.iter().any(|(path, _)| path == "assets/monolab/textures/font/hit.png"));
    }

    #[test]
    fn small_glyphs_are_padded_for_canvases() {
        let fonts = vec![("dot".to_string(), font(r#"{"schema":1,"image":"dot","height":3}"#))];
        let files = font_files(&fonts, &sources(), &mut Ids::default());
        let table = get(&files, TABLE_FILE);
        // Asked ascent: none, so 7 capped at the height.
        assert_eq!(get(&files, "assets/monolab/font/glyph.json")["providers"][0]["ascent"], 3);
        assert_eq!(table["glyphs"]["dot"]["canvas"], true);
        // Height 3 → 3 picture heights (9 ≥ 8), the picture is 4 high so the padded one is 12.
        let canvas = get(&files, "assets/monolab/font/canvas_2.json");
        assert_eq!(canvas["providers"][0]["file"], "monolab:font/pad/dot_3.png");
        assert_eq!(canvas["providers"][0]["height"], 9);
        assert_eq!(canvas["providers"][0]["ascent"], 5);
        let (_, padded) = files.iter().find(|(path, _)| path == "assets/monolab/textures/font/pad/dot_3.png").unwrap();
        let padded = image::load_from_memory(padded).unwrap();
        assert_eq!((padded.width(), padded.height()), (4, 12));
    }

    #[test]
    fn problems_are_told_per_file() {
        let fonts = vec![
            ("a".to_string(), font(r#"{"schema":1,"image":"ruby","name":"gem"}"#)),
            ("b".to_string(), font(r#"{"schema":1,"image":"ruby","name":"gem"}"#)),
            ("c".to_string(), font(r#"{"schema":1,"image":"missing"}"#)),
            ("d".to_string(), font(r#"{"schema":1,"image":"ruby","name":"Bad Name"}"#)),
            ("e".to_string(), font(r#"{"schema":1}"#)),
            ("f".to_string(), font(r#"{"schema":1,"image":"ruby","enabled":false}"#)),
        ];
        let files = font_files(&fonts, &sources(), &mut Ids::default());
        let table = get(&files, TABLE_FILE);
        assert!(table["problems"]["b"].as_str().unwrap().contains(":gem:"));
        assert!(table["problems"]["c"].as_str().unwrap().contains("missing"));
        assert!(table["problems"]["d"].is_string());
        assert!(table["problems"]["e"].is_string());
        assert!(table["problems"].get("a").is_none() && table["problems"].get("f").is_none());
        assert_eq!(table["glyphs"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn characters_never_move() {
        let mut ids = Ids::default();
        let both = vec![("ruby".to_string(), font(r#"{"schema":1,"image":"ruby"}"#)), ("zap".to_string(), font(r#"{"schema":1,"image":"ruby"}"#))];
        let first = font_files(&both, &sources(), &mut ids);
        assert_eq!(get(&first, TABLE_FILE)["glyphs"]["zap"]["char"], "\u{e001}");
        let only = vec![both[1].clone()];
        let again = font_files(&only, &sources(), &mut ids);
        assert_eq!(get(&again, TABLE_FILE)["glyphs"]["zap"]["char"], "\u{e001}");
        let same = font_files(&only, &sources(), &mut ids);
        assert_eq!(again, same);
    }

    #[test]
    fn spaces_and_moved_letters() {
        let files = font_files(&[], &[], &mut Ids::default());
        let space = get(&files, "assets/monolab/font/space.json");
        let advances = &space["providers"][0]["advances"];
        assert_eq!(advances["\u{e000}"], 1);
        assert_eq!(advances["\u{e008}"], 256);
        assert_eq!(advances["\u{e103}"], -8);
        let text = get(&files, "assets/monolab/font/text_3.json");
        let providers = text["providers"].as_array().unwrap();
        assert_eq!(providers[0]["id"], "minecraft:include/space");
        assert!(providers.iter().any(|one| one["file"] == "minecraft:font/ascii.png" && one["ascent"] == 4));
        assert_eq!(providers.last().unwrap()["id"], "minecraft:include/unifont");
    }
}
