
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;

use base64::Engine;
use image::{ImageFormat, RgbaImage};
use tauri::{AppHandle, Manager};
use zip::ZipArchive;

const SIZE: u32 = 64;

const TOP: f32 = 1.0;
const LEFT: f32 = 0.80;
const RIGHT: f32 = 0.62;

static DRAWN: Mutex<Option<(String, HashMap<String, String>)>> = Mutex::new(None);

fn cache_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("client-jars");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

pub(crate) fn client_jar(app: &AppHandle, version: &str) -> Result<PathBuf, String> {
    let dir = cache_dir(app).ok_or("App folder not found.")?;
    let path = dir.join(format!("client-{version}.jar"));
    if path.is_file() {
        return Ok(path);
    }
    let client = crate::addons::client()?;
    let get = |url: &str| -> Result<serde_json::Value, String> {
        client
            .get(url)
            .send()
            .map_err(|error| error.to_string())?
            .json::<serde_json::Value>()
            .map_err(|error| error.to_string())
    };
    let manifest = get("https://launchermeta.mojang.com/mc/game/version_manifest_v2.json")?;
    let entry = manifest
        .get("versions")
        .and_then(|value| value.as_array())
        .ok_or("Could not read the version list.")?
        .iter()
        .find(|one| one.get("id").and_then(|value| value.as_str()) == Some(version))
        .ok_or("This version is not in Mojang's list.")?;
    let url = entry
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or("The version has no URL.")?;
    let detail = get(url)?;
    let download = detail
        .pointer("/downloads/client")
        .ok_or("This version has no client file.")?;
    let jar_url = download
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or("The client has no URL.")?;
    let want = download
        .get("sha1")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    let staging = dir.join(format!(".client-{version}.part"));
    crate::download_file(&client, jar_url, &staging)?;
    if !want.is_empty() {
        if let Err(error) = crate::hashes::verify_sha1(&staging, &want) {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
    }
    fs::rename(&staging, &path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn read<R: std::io::Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let mut item = zip.by_name(name).ok()?;
    let mut out = Vec::new();
    item.read_to_end(&mut out).ok()?;
    Some(out)
}

fn model_textures<R: std::io::Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    id: &str,
) -> HashMap<String, String> {
    let mut found: HashMap<String, String> = HashMap::new();
    let mut next = Some(format!("block/{id}"));
    for _ in 0..5 {
        let Some(name) = next.take() else { break };
        let path = format!("assets/minecraft/models/{name}.json");
        let Some(raw) = read(zip, &path) else { break };
        let Ok(model) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            break;
        };
        if let Some(map) = model.get("textures").and_then(|one| one.as_object()) {
            for (key, value) in map {
                found.entry(key.clone()).or_insert_with(|| {
                    value.as_str().unwrap_or_default().to_string()
                });
            }
        }
        next = model
            .get("parent")
            .and_then(|one| one.as_str())
            .map(|one| one.trim_start_matches("minecraft:").to_string());
    }
    found
}

fn resolve(map: &HashMap<String, String>, key: &str) -> Option<String> {
    let mut at = map.get(key)?.clone();
    for _ in 0..5 {
        let Some(rest) = at.strip_prefix('#') else {
            return Some(at.trim_start_matches("minecraft:").to_string());
        };
        at = map.get(rest)?.clone();
    }
    None
}

fn face<R: std::io::Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Option<RgbaImage> {
    let raw = read(zip, &format!("assets/minecraft/textures/{name}.png"))?;
    let full = image::load_from_memory(&raw).ok()?.to_rgba8();
    let width = full.width();
    if full.height() > width {
        return Some(image::imageops::crop_imm(&full, 0, 0, width, width).to_image());
    }
    Some(full)
}

fn shade(pixel: image::Rgba<u8>, amount: f32) -> image::Rgba<u8> {
    let go = |one: u8| (f32::from(one) * amount).round().clamp(0.0, 255.0) as u8;
    image::Rgba([go(pixel[0]), go(pixel[1]), go(pixel[2]), pixel[3]])
}

fn invert(a: (f32, f32), b: (f32, f32), point: (f32, f32)) -> Option<(f32, f32)> {
    let det = a.0 * b.1 - a.1 * b.0;
    if det.abs() < f32::EPSILON {
        return None;
    }
    let u = (point.0 * b.1 - point.1 * b.0) / det;
    let v = (a.0 * point.1 - a.1 * point.0) / det;
    Some((u, v))
}

fn cube(top: &RgbaImage, left: &RgbaImage, right: &RgbaImage) -> RgbaImage {
    let s = SIZE as f32;
    let mut out = RgbaImage::new(SIZE, SIZE);
    #[allow(clippy::type_complexity)]
    let faces: [((f32, f32), (f32, f32), (f32, f32), &RgbaImage, f32); 3] = [
        ((0.0, s / 4.0), (s / 2.0, -s / 4.0), (s / 2.0, s / 4.0), top, TOP),
        ((0.0, s / 4.0), (s / 2.0, s / 4.0), (0.0, s / 2.0), left, LEFT),
        ((s / 2.0, s / 2.0), (s / 2.0, -s / 4.0), (0.0, s / 2.0), right, RIGHT),
    ];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let point = (x as f32 + 0.5, y as f32 + 0.5);
            for (origin, along, down, texture, amount) in &faces {
                let moved = (point.0 - origin.0, point.1 - origin.1);
                let Some((u, v)) = invert(*along, *down, moved) else {
                    continue;
                };
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let tx = ((u * texture.width() as f32) as u32).min(texture.width() - 1);
                let ty = ((v * texture.height() as f32) as u32).min(texture.height() - 1);
                let pixel = *texture.get_pixel(tx, ty);
                if pixel[3] == 0 {
                    continue;
                }
                out.put_pixel(x, y, shade(pixel, *amount));
                break;
            }
        }
    }
    out
}

fn flat(texture: &RgbaImage) -> RgbaImage {
    image::imageops::resize(
        texture,
        SIZE,
        SIZE,
        image::imageops::FilterType::Nearest,
    )
}

fn to_data_url(image: &RgbaImage) -> Option<String> {
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    ))
}

fn draw<R: std::io::Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    id: &str,
) -> Option<RgbaImage> {
    if let Some(texture) = face(zip, &format!("item/{id}")) {
        return Some(flat(&texture));
    }
    let map = model_textures(zip, id);
    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|key| resolve(&map, key))
    };
    let top = pick(&["up", "top", "end", "all", "texture"]);
    let side = pick(&["side", "north", "all", "texture"]);
    let top = top.or_else(|| {
        ["_top", ""].iter().find_map(|tail| {
            let name = format!("block/{id}{tail}");
            face(zip, &name).is_some().then_some(name)
        })
    })?;
    let side = side.unwrap_or_else(|| top.clone());
    let top_image = face(zip, &top)?;
    let side_image = face(zip, &side)?;
    Some(cube(&top_image, &side_image, &side_image))
}

pub fn icons(
    app: &AppHandle,
    version: &str,
    ids: &[String],
) -> Result<HashMap<String, String>, String> {
    let mut held = DRAWN.lock().unwrap_or_else(|one| one.into_inner());
    if held.as_ref().map(|(at, _)| at.as_str()) != Some(version) {
        *held = Some((version.to_string(), HashMap::new()));
    }
    let (_, cache) = held.as_mut().expect("Just added");

    let missing: Vec<String> = ids
        .iter()
        .map(|one| one.trim_start_matches("minecraft:").to_string())
        .filter(|one| !cache.contains_key(one))
        .collect();
    if !missing.is_empty() {
        let path = client_jar(app, version)?;
        let file = fs::File::open(&path).map_err(|error| error.to_string())?;
        let mut zip = ZipArchive::new(file).map_err(|error| error.to_string())?;
        for id in missing {
            let drawn = draw(&mut zip, &id).and_then(|one| to_data_url(&one));
            cache.insert(id, drawn.unwrap_or_default());
        }
    }
    Ok(ids
        .iter()
        .filter_map(|one| {
            let key = one.trim_start_matches("minecraft:").to_string();
            let found = cache.get(&key)?;
            (!found.is_empty()).then(|| (one.clone(), found.clone()))
        })
        .collect())
}

pub(crate) struct MapPalette {
    pub json: String,
    pub grass: Option<Vec<u8>>,
    pub foliage: Option<Vec<u8>>,
}

const MAP_KEYS: [&str; 11] = ["top", "end", "up", "all", "texture", "cross", "plant", "crop", "layer0", "particle", "side"];

fn split_id(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("minecraft", id))
}

fn json_at<R: std::io::Read + std::io::Seek>(zip: &mut ZipArchive<R>, path: &str) -> Option<serde_json::Value> {
    serde_json::from_slice(&read(zip, path)?).ok()
}

fn map_model<R: std::io::Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    id: &str,
    textures: &mut HashMap<String, serde_json::Value>,
    tinted: &mut bool,
    depth: u32,
) {
    if depth > 12 {
        return;
    }
    let (namespace, path) = split_id(id);
    let Some(model) = json_at(zip, &format!("assets/{namespace}/models/{path}.json")) else {
        return;
    };
    if let Some(parent) = model.get("parent").and_then(|one| one.as_str()) {
        map_model(zip, parent, textures, tinted, depth + 1);
    }
    if let Some(own) = model.get("textures").and_then(|one| one.as_object()) {
        for (key, value) in own {
            textures.insert(key.clone(), value.clone());
        }
    }
    if let Some(elements) = model.get("elements").and_then(|one| one.as_array()) {
        *tinted = elements
            .iter()
            .any(|element| element.pointer("/faces/up/tintindex").is_some_and(|one| !one.is_null()));
    }
}

fn texture_ref(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    let value = if value.is_object() { value.get("sprite").or_else(|| value.get("texture"))? } else { value };
    value.as_str().map(str::to_string)
}

fn average_color<R: std::io::Read + std::io::Seek>(zip: &mut ZipArchive<R>, id: &str) -> Option<u32> {
    let (namespace, path) = split_id(id);
    let raw = read(zip, &format!("assets/{namespace}/textures/{path}.png"))?;
    let image = image::load_from_memory(&raw).ok()?.to_rgba8();
    let side = image.width().min(image.height());
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for y in 0..side {
        for x in 0..image.width() {
            let pixel = image.get_pixel(x, y);
            if pixel[3] < 30 {
                continue;
            }
            r += u64::from(pixel[0]);
            g += u64::from(pixel[1]);
            b += u64::from(pixel[2]);
            n += 1;
        }
    }
    (n > 0).then(|| ((r / n) as u32) << 16 | ((g / n) as u32) << 8 | (b / n) as u32)
}

fn block_entry<R: std::io::Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    colors: &mut HashMap<String, Option<u32>>,
    path: &str,
    block: &str,
) -> Option<serde_json::Value> {
    let state = json_at(zip, path)?;
    let first = |value: &serde_json::Value| -> Option<String> {
        let value = value.as_array().map_or(Some(value), |list| list.first())?;
        value.get("model")?.as_str().map(str::to_string)
    };
    let variants = state.get("variants").and_then(|one| one.as_object()).filter(|one| !one.is_empty());
    let model = match (variants, state.get("multipart").and_then(|one| one.as_array())) {
        (Some(variants), _) => first(variants.values().next()?)?,
        (None, Some(parts)) => first(parts.first()?.get("apply")?)?,
        _ => return None,
    };
    let mut textures = HashMap::new();
    let mut tinted = false;
    map_model(zip, &model, &mut textures, &mut tinted, 0);
    let mut color = None;
    for key in MAP_KEYS {
        let mut reference = texture_ref(textures.get(key));
        for _ in 0..8 {
            match reference.as_deref().and_then(|one| one.strip_prefix('#')) {
                Some(rest) => reference = texture_ref(textures.get(rest)),
                None => break,
            }
        }
        let Some(reference) = reference.filter(|one| !one.starts_with('#')) else {
            continue;
        };
        let found = match colors.get(&reference) {
            Some(known) => *known,
            None => {
                let found = average_color(zip, &reference);
                colors.insert(reference, found);
                found
            }
        };
        if found.is_some() {
            color = found;
            break;
        }
    }
    let color = color?;
    let (namespace, name) = split_id(block);
    let tint = if namespace == "minecraft" {
        match name {
            "grass_block" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "sugar_cane" | "vine" | "bush" => "grass",
            "birch_leaves" => "fixed:80a755",
            "spruce_leaves" => "fixed:619961",
            "mangrove_leaves" => "fixed:92c648",
            "water" | "bubble_column" => "water",
            "lily_pad" => "fixed:208030",
            _ if name.ends_with("_leaves") && !["cherry", "azalea", "pale_oak"].iter().any(|one| name.contains(one)) => "foliage",
            _ => "none",
        }
    } else if tinted {
        if name.contains("leaves") || name.contains("leaf") {
            "foliage"
        } else if name.contains("water") {
            "water"
        } else {
            "grass"
        }
    } else {
        "none"
    };
    Some(serde_json::json!([format!("{color:06x}"), tint]))
}

fn biome_color(value: Option<&serde_json::Value>, otherwise: &str) -> String {
    match value {
        Some(serde_json::Value::Number(number)) => format!("{:06x}", number.as_i64().unwrap_or(0) & 0xFF_FF_FF),
        Some(serde_json::Value::String(text)) if text.starts_with('#') && text.len() >= 7 => text[1..7].to_lowercase(),
        _ => otherwise.to_string(),
    }
}

fn biome_entry<R: std::io::Read + std::io::Seek>(zip: &mut ZipArchive<R>, path: &str) -> Option<serde_json::Value> {
    let biome = json_at(zip, path)?;
    let effects = biome.get("effects");
    let effect = |key: &str| effects.and_then(|one| one.get(key));
    Some(serde_json::json!([
        biome.get("temperature").and_then(|one| one.as_f64()).unwrap_or(0.8),
        biome.get("downfall").and_then(|one| one.as_f64()).unwrap_or(0.4),
        biome_color(effect("water_color"), "3f76e4"),
        biome_color(effect("grass_color"), ""),
        biome_color(effect("foliage_color"), ""),
        effect("grass_color_modifier").and_then(|one| one.as_str()).unwrap_or(""),
    ]))
}

/// Builds the map's `palette.json` from a client jar, for servers whose addon can't (Pumpkin).
pub(crate) fn map_palette(jar: &std::path::Path) -> Result<MapPalette, String> {
    let file = fs::File::open(jar).map_err(|error| error.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|error| error.to_string())?;
    let names: Vec<String> = zip.file_names().filter_map(|one| one.ok().map(|name| name.into_owned())).collect();
    let mut blocks = serde_json::Map::new();
    let mut biomes = serde_json::Map::new();
    let mut colors = HashMap::new();
    for name in &names {
        if let Some(rest) = name.strip_prefix("assets/") {
            let Some((namespace, path)) = rest.split_once("/blockstates/") else { continue };
            let Some(block) = path.strip_suffix(".json") else { continue };
            let block = format!("{namespace}:{block}");
            if !blocks.contains_key(&block) {
                if let Some(entry) = block_entry(&mut zip, &mut colors, name, &block) {
                    blocks.insert(block, entry);
                }
            }
        } else if let Some(rest) = name.strip_prefix("data/") {
            let Some((namespace, path)) = rest.split_once("/worldgen/biome/") else { continue };
            let Some(biome) = path.strip_suffix(".json") else { continue };
            let biome = format!("{namespace}:{biome}");
            if !biomes.contains_key(&biome) {
                if let Some(entry) = biome_entry(&mut zip, name) {
                    biomes.insert(biome, entry);
                }
            }
        }
    }
    if blocks.is_empty() {
        return Err("The client file has no block models.".into());
    }
    let json = serde_json::json!({ "key": "client", "vanilla": true, "blocks": blocks, "biomes": biomes }).to_string();
    Ok(MapPalette {
        json,
        grass: read(&mut zip, "assets/minecraft/textures/colormap/grass.png"),
        foliage: read(&mut zip, "assets/minecraft/textures/colormap/foliage.png"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_faces_fill_the_square() {
        let solid = |shade: u8| {
            let mut one = RgbaImage::new(16, 16);
            for pixel in one.pixels_mut() {
                *pixel = image::Rgba([shade, shade, shade, 255]);
            }
            one
        };
        let drawn = cube(&solid(255), &solid(255), &solid(255));
        let filled = drawn.pixels().filter(|one| one[3] != 0).count();
        let want = (SIZE * SIZE) as usize * 3 / 4;
        assert_eq!(filled, want, "there is a gap at the seam");

        let mut shades: Vec<u8> = drawn
            .pixels()
            .filter(|one| one[3] != 0)
            .map(|one| one[0])
            .collect();
        shades.sort_unstable();
        shades.dedup();
        assert_eq!(shades.len(), 3, "expected three shades: {shades:?}");

        for (x, y) in [(0, 0), (SIZE - 1, 0), (0, SIZE - 1), (SIZE - 1, SIZE - 1)] {
            assert_eq!(drawn.get_pixel(x, y)[3], 0, "corner ({x},{y}) is filled");
        }
    }

    #[test]
    #[ignore]
    fn builds_a_map_palette_from_a_real_jar() {
        let Ok(path) = std::env::var("SE_CLIENT_JAR") else {
            return;
        };
        let palette = map_palette(std::path::Path::new(&path)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&palette.json).unwrap();
        println!("{} blocks, {} biomes", value["blocks"].as_object().unwrap().len(), value["biomes"].as_object().unwrap().len());
        println!("grass_block {} stone {} plains {}", value["blocks"]["minecraft:grass_block"], value["blocks"]["minecraft:stone"], value["biomes"]["minecraft:plains"]);
        assert_eq!(value["blocks"]["minecraft:grass_block"][1], "grass");
        assert!(palette.grass.is_some());
        if let Ok(out) = std::env::var("SE_PALETTE_OUT") {
            let out = std::path::Path::new(&out);
            std::fs::write(out.join("palette.json"), &palette.json).unwrap();
            std::fs::write(out.join("grass.png"), palette.grass.unwrap()).unwrap();
            std::fs::write(out.join("foliage.png"), palette.foliage.unwrap()).unwrap();
        }
    }

    #[test]
    #[ignore]
    fn draws_real_minecraft_items() {
        let dir = std::env::var("APPDATA").expect("APPDATA");
        let path = PathBuf::from(dir)
            .join("com.monolab.desktop/client-jars/client-1.21.4.jar");
        if !path.is_file() {
            println!("no downloaded client jar: {}", path.display());
            return;
        }
        let file = fs::File::open(&path).unwrap();
        let mut zip = ZipArchive::new(file).unwrap();
        for id in [
            "diamond_sword",
            "stone",
            "grass_block",
            "oak_log",
            "some_mod_thing",
        ] {
            let drawn = draw(&mut zip, id);
            println!(
                "{id}: {}",
                match &drawn {
                    Some(one) => format!("{}x{}", one.width(), one.height()),
                    None => "not drawn".into(),
                }
            );
            if let (Some(one), Ok(out)) = (&drawn, std::env::var("SE_ICON_OUT")) {
                let _ = std::fs::create_dir_all(&out);
                let _ = one.save(PathBuf::from(&out).join(format!("{id}.png")));
            }
        }
        assert!(draw(&mut zip, "stone").is_some(), "stone was not drawn");
        assert!(draw(&mut zip, "some_mod_thing").is_none(), "drew something that isn't there");
    }
}
