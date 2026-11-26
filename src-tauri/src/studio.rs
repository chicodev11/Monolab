//! Studio: in-server features (scoreboard, tab list, MOTD...) the Monolab add-on runs from JSON files.
//! One resource = one file, `<root>/<kind>/<id>.json` (`plugins/Monolab/studio` on Paper,
//! `plugins/data/monolab/studio` on Pumpkin). The app reads, checks and writes those files; the add-on rereads
//! them within two seconds and reports what loaded in `<root>/status.json`. The checks are the shared
//! `studio-format` crate's, which the Pumpkin add-on uses too and the Paper add-on's `Studio.check` matches.

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub use studio_format::{valid_id, valid_kind, KINDS, PUMPKIN_KINDS, SCHEMA};

/// Where the Paper add-on (`plugins/Monolab`) keeps the studio.
pub const PAPER_ROOT: &str = "plugins/Monolab/studio";
/// The Pumpkin add-on may only write in its own data folder.
pub const PUMPKIN_ROOT: &str = "plugins/data/monolab/studio";

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioResource {
    pub kind: String,
    pub id: String,
    pub enabled: bool,
    /// What the add-on said when it loaded the file; empty when it loaded fine or hasn't read it yet.
    pub error: String,
    /// The add-on has read this file (it is in status.json).
    pub seen: bool,
    pub modified: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioOverview {
    /// The server runs an add-on with a studio (Paper family or Pumpkin).
    pub supported: bool,
    /// The studio folder in the server folder.
    pub root: String,
    /// The kinds this server's add-on runs (all on Paper, the looks on Pumpkin).
    pub kinds: Vec<String>,
    /// The add-on has written status.json at least once.
    pub has_status: bool,
    /// The add-on's `studio` switch.
    pub on: bool,
    pub addon_version: String,
    /// When the add-on last wrote status.json, in ms.
    pub status_at: f64,
    pub resources: Vec<StudioResource>,
    /// Kinds turned off one by one in the add-on's config.yml (`studio-<kind>: false`).
    pub off: Vec<String>,
    /// Installed plugins that do the same job as some kinds.
    pub overlaps: Vec<StudioOverlap>,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioOverlap {
    /// The plugin's usual name, e.g. `TAB`.
    pub plugin: String,
    /// The jar it was found by.
    pub file: String,
    pub kinds: Vec<String>,
}

/// Plugins known to do what a studio kind does. Both running at once fight (two scoreboards, two chat formats).
const OVERLAPS: &[(&str, &[&str])] = &[
    ("TAB", &["tab", "scoreboard"]),
    ("FeatherBoard", &["scoreboard"]),
    ("AnimatedScoreboard", &["scoreboard"]),
    ("SimpleScore", &["scoreboard"]),
    ("ChatControl", &["chat", "message", "announcement"]),
    ("ChatControlRed", &["chat", "message", "announcement"]),
    ("VentureChat", &["chat"]),
    ("DeluxeChat", &["chat"]),
    ("LPC", &["chat"]),
    ("EssentialsChat", &["chat"]),
    ("ServerListPlus", &["motd"]),
    ("MiniMOTD", &["motd"]),
    ("AdvancedServerList", &["motd"]),
    ("DeluxeMenus", &["menu"]),
    ("ChestCommands", &["menu"]),
    ("CommandPanels", &["menu"]),
    ("AutoBroadcast", &["announcement"]),
    ("MyCommand", &["command"]),
    ("DecentHolograms", &["hologram"]),
    ("HolographicDisplays", &["hologram"]),
    ("FancyHolograms", &["hologram"]),
    ("Citizens", &["npc"]),
    ("FancyNpcs", &["npc"]),
    ("ZNPCsPlus", &["npc"]),
    ("Multiverse-Core", &["world"]),
];

/// Which known plugins are in `plugins/`: each jar's file name (`TAB v4.1.jar`, `DeluxeMenus-1.14.0.jar`) and,
/// when it could be read, the name its `plugin.yml` gives (so `tab-bukkit.jar` is still TAB).
/// A file name counts when the jar starts with the plugin's name and the next character isn't a letter or digit,
/// so `TabList.jar` isn't TAB; a `plugin.yml` name must be the same (any case). Turned-off jars
/// (`.jar.disabled` and the like) don't count.
pub fn overlaps(jars: &[(String, Option<String>)]) -> Vec<StudioOverlap> {
    let mut found: Vec<StudioOverlap> = Vec::new();
    for (name, declared) in jars {
        let lower = name.to_ascii_lowercase();
        let Some(stem) = lower.strip_suffix(".jar") else { continue };
        let by_file = OVERLAPS
            .iter()
            .filter(|(plugin, _)| {
                let key = plugin.to_ascii_lowercase();
                stem.strip_prefix(&key).is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric()))
            })
            .max_by_key(|(plugin, _)| plugin.len());
        let by_yml = || {
            let declared = declared.as_deref()?.trim();
            OVERLAPS.iter().find(|(plugin, _)| plugin.eq_ignore_ascii_case(declared))
        };
        let best = by_yml().or(by_file);
        if let Some((plugin, kinds)) = best {
            if !found.iter().any(|one| one.plugin == *plugin) {
                found.push(StudioOverlap {
                    plugin: plugin.to_string(),
                    file: name.clone(),
                    kinds: kinds.iter().map(|one| one.to_string()).collect(),
                });
            }
        }
    }
    found
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioFile {
    pub text: String,
    /// What the file looked like when read, to spot changes made elsewhere before saving over them.
    pub hash: String,
}

pub fn path_of(root: &str, kind: &str, id: &str) -> Result<String, String> {
    if !valid_kind(kind) {
        return Err("Unknown studio kind.".into());
    }
    if !valid_id(id) {
        return Err("Names may only use a-z, 0-9, - and _ (up to 48).".into());
    }
    Ok(format!("{root}/{kind}/{id}.json"))
}

/// The custom world file (`"type": "custom"`, `"main": true`, on) that gives the server's main world its generator.
/// The first by id when there are more.
pub fn main_world(files: &[(String, serde_json::Map<String, Value>)]) -> Option<&str> {
    files
        .iter()
        .find(|(_, data)| {
            studio_format::enabled(data)
                && data.get("type").and_then(Value::as_str) == Some("custom")
                && data.get("main").and_then(Value::as_bool) == Some(true)
        })
        .map(|(id, _)| id.as_str())
}

/// `bukkit.yml` text with `worlds.<level>.generator` set to `Monolab:<id>`, or (`None`) with Monolab's taken out.
/// Another plugin's generator is only replaced when one of ours is asked for. Written by line, as Bukkit does
/// (two spaces a level), so the rest of the file and its comments stay as they are.
pub fn bukkit_generator(text: &str, level: &str, id: Option<&str>) -> String {
    let wanted = id.map(|id| format!("Monolab:{id}"));
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let Some(worlds) = lines.iter().position(|line| line.trim_end() == "worlds:") else {
        let Some(wanted) = wanted else { return text.to_string() };
        let mut out = text.trim_end_matches('\n').to_string();
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("worlds:\n  {level}:\n    generator: {wanted}\n"));
        return out;
    };
    let inside = |line: &str| line.is_empty() || line.starts_with(' ') || line.starts_with('#');
    let end = lines[worlds + 1..].iter().position(|line| !inside(line)).map_or(lines.len(), |at| worlds + 1 + at);
    let head = format!("  {level}:");
    let level_at = (worlds + 1..end).find(|&at| lines[at].trim_end() == head);
    let generator_at = level_at.and_then(|level_at| {
        let level_end = (level_at + 1..end).find(|&at| !lines[at].is_empty() && !lines[at].starts_with("    ")).unwrap_or(end);
        (level_at + 1..level_end).find(|&at| lines[at].starts_with("    generator:"))
    });
    match (wanted, level_at, generator_at) {
        (Some(wanted), _, Some(at)) => lines[at] = format!("    generator: {wanted}"),
        (Some(wanted), Some(at), None) => lines.insert(at + 1, format!("    generator: {wanted}")),
        (Some(wanted), None, None) => {
            lines.insert(worlds + 1, head);
            lines.insert(worlds + 2, format!("    generator: {wanted}"));
        }
        (None, _, Some(at)) => {
            let value = lines[at].trim_start().trim_start_matches("generator:").trim().trim_matches(['\'', '"']).to_string();
            if !value.starts_with("Monolab:") {
                return text.to_string();
            }
            lines.remove(at);
        }
        (None, _, None) => return text.to_string(),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub fn hash(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The add-on's checks, so a broken file is caught before it is saved. `None` = fine.
pub fn check(id: &str, text: &str) -> Option<String> {
    studio_format::check(id, text).err()
}

/// A starting file for a new resource of `kind`.
pub fn template(kind: &str, id: &str) -> String {
    let body = match kind {
        "scoreboard" => json!({
            "title": "<b>My server</b>",
            "lines": ["", "Online: <online>/<max>", "TPS: <tps>"],
            "show": { "worlds": [], "permission": "" },
            "refreshTicks": 20,
        }),
        "tab" => json!({ "header": "<b>My server</b>", "footer": "Online: <online>/<max>", "playerName": "<player>" }),
        "motd" => json!({ "lines": ["<b>My server</b>", "Come and play"] }),
        "chat" => json!({ "format": "<player>: <message>" }),
        "message" => json!({ "join": "<player> joined", "leave": "<player> left", "firstJoin": "Welcome, <player>!" }),
        "announcement" => json!({ "everySeconds": 300, "messages": ["Tip: be nice"], "random": false }),
        "bossbar" => json!({
            "title": "<gold><b>Welcome, <player></b>",
            "color": "yellow",
            "style": "progress",
            "progress": 1.0,
            "show": { "worlds": [], "permission": "" },
            "refreshTicks": 20,
        }),
        "hologram" => json!({
            "world": "world",
            "x": 0.5,
            "y": 80.0,
            "z": 0.5,
            "lines": ["<gold><b>Welcome!</b>", "<gray>Online: <online>/<max>"],
            "billboard": "center",
            "shadow": true,
            "background": true,
            "seeThrough": false,
            "scale": 1.0,
        }),
        "npc" => json!({
            "name": "<yellow><b>Guide",
            "entity": "villager",
            "profession": "librarian",
            "world": "world",
            "x": 0.5,
            "y": 64.0,
            "z": 0.5,
            "yaw": 0.0,
            "lookAtPlayers": true,
            "glow": false,
            "baby": false,
            "cooldownSeconds": 1,
            "if": [],
            "actions": [{ "type": "message", "text": "<yellow>Guide:</yellow> Welcome, <player>!" }],
            "deny": [],
        }),
        "dialog" => json!({
            "title": "<b>Welcome",
            "body": ["Glad you're here, <player>.", "Pick a kit to start with."],
            "inputs": [{ "type": "choice", "key": "kit", "label": "Kit", "options": ["warrior", "archer"], "initial": "warrior" }],
            "buttons": [
                { "label": "<green>Start", "tooltip": "", "actions": [{ "type": "message", "text": "You picked <input:kit>." }] },
                { "label": "Later", "tooltip": "", "actions": [] },
            ],
            "columns": 2,
            "canClose": true,
            "permission": "",
        }),
        "loot" => json!({
            "drops": { "from": "none", "targets": [], "replace": false },
            "rolls": 1,
            "rollsMax": 2,
            "entries": [
                { "item": "diamond", "min": 1, "max": 2, "weight": 1 },
                { "item": "iron_ingot", "min": 2, "max": 5, "weight": 4 },
                { "item": "", "weight": 5 },
            ],
            "show": { "worlds": [], "permission": "" },
        }),
        "advancement" => json!({
            "title": "<gold>First steps",
            "description": "Join the server",
            "icon": "oak_sapling",
            "frame": "task",
            "parent": "",
            "background": "stone",
            "toast": true,
            "announce": true,
            "hidden": false,
            "rewards": [],
        }),
        "font" => json!({ "image": "", "height": 8, "ascent": 7, "name": id.replace('-', "_") }),
        "world" => json!({
            "name": id,
            "environment": "normal",
            "type": "flat",
            "seed": "",
            "structures": false,
            "layers": [{ "block": "bedrock", "height": 1 }, { "block": "dirt", "height": 2 }, { "block": "grass_block", "height": 1 }],
            "biome": "plains",
            "pvp": true,
            "gamerules": { "doDaylightCycle": false, "doMobSpawning": false },
        }),
        "command" => json!({
            "name": id,
            "description": "A command made in the studio",
            "aliases": [],
            "permission": "",
            "cooldownSeconds": 0,
            "actions": [{ "type": "message", "text": "<green>Hello, <player>!" }],
        }),
        "menu" => json!({
            "title": "<b>Menu",
            "rows": 3,
            "command": "",
            "permission": "",
            "fill": "gray_stained_glass_pane",
            "slots": [{
                "slot": 13,
                "item": { "material": "diamond", "name": "<aqua>Click me", "lore": ["Gives you a diamond"] },
                "actions": [{ "type": "give", "item": "diamond", "amount": 1 }, { "type": "message", "text": "Here you go!" }],
                "close": true,
            }],
        }),
        "item" => json!({
            "material": "diamond_sword",
            "name": "<gold>Hero sword",
            "lore": ["A sword for heroes"],
            "enchantments": { "sharpness": 5 },
            "unbreakable": false,
            "glow": false,
            // M1: "look": { "texture": "<studio texture>" } or { "model": "<studio model>" }, and components
            // (maxStack, rarity, durability, food, tool, attack, equip, cooldown, hideTooltip). See
            // crates/studio-format/src/item.rs.
            "rarity": "rare",
            "attack": { "damage": 8, "speed": 1.6 },
            "useActions": [],
            "eatActions": [],
            "hitActions": [],
            "breakActions": [],
        }),
        "recipe" => json!({
            "type": "shaped",
            "shape": ["DDD", " S ", " S "],
            "keys": { "D": "diamond", "S": "stick" },
            "result": { "item": "diamond_pickaxe", "amount": 1 },
        }),
        "rule" => json!({
            "when": { "event": "join" },
            "if": [],
            "then": [{ "type": "message", "text": "Welcome back, <player>!" }],
            "else": [],
            "cooldownSeconds": 0,
        }),
        // M4: a Blockbench model from the studio's assets (see crates/studio-format/src/model.rs).
        "model" => json!({ "model": "", "scale": 1, "animations": { "idle": "" }, "clickActions": [] }),
        // M3: a block that borrows a vanilla block state, and a piece of furniture (block.rs, furniture.rs).
        "block" => json!({
            "shape": "cube",
            "look": { "texture": "" },
            "hardness": 1.5,
            "tool": "pickaxe",
            "sounds": "stone",
            "drops": { "self": true, "loot": "", "exp": 0 },
            "placeActions": [],
            "useActions": [],
            "breakActions": [],
        }),
        "furniture" => json!({
            "look": { "texture": "" },
            "turns": 4,
            "solid": true,
            "cells": [{ "x": 0, "y": 0, "z": 0 }],
            "drops": true,
            "useActions": [],
            "breakActions": [],
        }),
        // M5: a custom mob with stats, a look, drops, natural spawning and skills (crates/studio-format/src/mob.rs).
        "mob" => json!({
            "base": "zombie",
            "name": "",
            "health": 20,
            "damage": 3,
            "equipment": {},
            "model": "",
            "loot": "",
            "vanillaDrops": false,
            "exp": 5,
            "ai": { "attack": true, "wander": true, "look": true, "burn": false, "pickup": false },
            "spawn": { "enabled": false, "worlds": [], "biomes": [], "lightMin": 0, "lightMax": 7, "max": 10, "every": 30, "chance": 50, "radius": 32 },
            "skills": [],
        }),
        "enchant" => json!({
            "name": "Lifesteal",
            "maxLevel": 3,
            "items": ["sword", "axe"],
            "chance": 100,
            "hitActions": [{ "type": "heal", "amount": 1 }],
            "mineActions": [],
            "hurtActions": [],
        }),
        "structure" => json!({
            "source": "",
            "size": [0, 0, 0],
            "blocks": 0,
            "rotation": "0",
            "centered": true,
            "lift": 0,
        }),
        "quest" => json!({
            "name": "Lost sheep",
            "description": "The shepherd lost his flock.",
            "repeatable": false,
            "advancement": "",
            "steps": [
                { "type": "talk", "target": "shepherd", "text": "Talk to the shepherd" },
                { "type": "collect", "target": "white_wool", "amount": 5, "text": "" },
            ],
            "startActions": [],
            "rewards": [{ "type": "give", "item": "emerald", "amount": 3 }],
        }),
        "function" => json!({
            "description": "Welcome a player",
            "actions": [
                { "type": "message", "text": "<green>Welcome, <player>!" },
                { "type": "sound", "sound": "entity.player.levelup", "volume": 1, "pitch": 1 },
            ],
        }),
        "shop" => json!({
            "title": "General store",
            "currency": "variable",
            "variable": "coins",
            "moneyName": "coins",
            "rows": 0,
            "goods": [
                { "item": "bread", "amount": 4, "buy": 10, "sell": 0 },
                { "item": "diamond", "amount": 1, "buy": 200, "sell": 80 },
            ],
        }),
        "trade" => json!({
            "title": "Blacksmith",
            "restockMinutes": 0,
            "offers": [{ "buy": "emerald", "buyAmount": 5, "sell": "iron_sword", "sellAmount": 1, "uses": 0 }],
        }),
        "cosmetic" => json!({
            "slot": "hat",
            "item": "golden_helmet",
            "scale": 1,
            "up": 0,
            "back": 0,
            "permission": "",
        }),
        "biome" => json!({
            "sky": "#7ba4ff",
            "fog": "#c0d8ff",
            "water": "#3f76e4",
            "waterFog": "#050533",
            "grass": "",
            "foliage": "",
            "particle": "",
            "particleChance": 0.01,
            "music": "",
            "ambient": "",
            "temperature": 0.8,
            "downfall": 0.4,
            "rain": true,
            "like": "plains",
        }),
        "painting" => json!({
            "texture": "",
            "width": 2,
            "height": 1,
            "title": "Sunset",
            "author": "",
        }),
        "song" => json!({
            "name": "Boss theme",
            "sound": "boss_theme",
            "seconds": 120,
            "comparator": 1,
        }),
        _ => json!({}),
    };
    // schema, id and enabled first, then the kind's own fields (serde_json alone would sort them).
    let mut text = format!("{{\n  \"schema\": {SCHEMA},\n  \"id\": {},\n  \"enabled\": true", json!(id));
    for (key, one) in body.as_object().into_iter().flatten() {
        let pretty = serde_json::to_string_pretty(one).unwrap_or_default().replace('\n', "\n  ");
        text.push_str(&format!(",\n  {}: {pretty}", json!(key)));
    }
    text.push_str("\n}\n");
    text
}

#[derive(Default)]
struct Status {
    on: bool,
    addon: String,
    at: f64,
    /// (kind, id, enabled, error) per file the add-on read.
    rows: Vec<(String, String, bool, String)>,
}

fn read_status(text: &str) -> Status {
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let rows = value
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            let text = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
            (text("kind"), text("id"), row.get("enabled").and_then(Value::as_bool).unwrap_or(false), text("error"))
        })
        .collect();
    Status {
        on: value.get("on").and_then(Value::as_bool).unwrap_or(false),
        addon: value.get("addon").and_then(Value::as_str).unwrap_or_default().to_string(),
        at: value.get("at").and_then(Value::as_f64).unwrap_or(0.0),
        rows,
    }
}

/// The first line of the studio's `AGENTS.md`. The app rewrites a file that starts with it when the text
/// changes, and leaves a file without it alone (somebody wrote their own).
pub const GUIDE_MARK: &str = "<!-- Written by Monolab. Edit freely: remove this line and Monolab stops updating the file. -->";

/// `studio/AGENTS.md`: how the studio files are written, for an AI agent (Claude Code, Codex, the app's own) that is
/// asked to make or change them. Agents read the `AGENTS.md` nearest the files they touch. The format stays JSON
/// (the editor, both add-ons and Rewind read it); this is what makes it easy to write by hand.
pub fn guide(root: &str, kinds: &[&str]) -> String {
    let mut out = format!(
        "{GUIDE_MARK}\n# Monolab studio files\n\n\
Every file here is one thing on the server: `{root}/<kind>/<id>.json`. The Monolab add-on reads them within a second of\n\
a save (no restart) and writes what went wrong to `status.json` in this folder. **Read `status.json` after you\n\
write a file**: a row with `\"error\"` for your file means it was not applied.\n\n\
## Rules\n\n\
- `<id>` (the file name) is a-z, 0-9, `-` and `_`, up to 48. Kinds are the folder names below; no other folders.\n\
- Every file starts with `\"schema\": {SCHEMA}`, `\"id\"` (the file name) and `\"enabled\"` (true or false).\n\
- Keep the field names of the examples. Unknown fields are kept but do nothing. A file is at most 256 KB.\n\
- Text is MiniMessage: `<red>`, `<b>`, `<gradient:#ff0000:#0000ff>`, `<#33aaff>`. Placeholders: `<player>`, `<online>`,\n\
  `<max>`, `<tps>`, `<world>`, `<ping>`, `<var:name>` (server variable), `<pvar:name>` (the player's), and PlaceholderAPI's\n\
  `%...%` when it is installed.\n\
- Times are in ticks (20 = 1 second) where the field name says `Ticks`.\n\
- Don't write `status.json`, `live.json`, `live/` or `data/`: the add-on keeps those. `structures/` holds block lists the app makes from `.schem` and `.nbt` files; leave them alone.\n\n\
## Actions and conditions\n\n\
`then`, `actions`, `else` and a button's `actions` are lists of steps run in order. A step is `{{\"type\": ...}}`:\n\n\
| type | fields |\n|---|---|\n\
| message, broadcast, actionbar | `text` |\n\
| title | `title`, `subtitle`, `stayTicks` |\n\
| sound | `sound` (`entity.experience_orb.pickup`), `volume`, `pitch` |\n\
| command | `command` (no `/`), `as`: `console` or `player` |\n\
| teleport | `world`, `x`, `y`, `z`, `yaw`, `pitch` |\n\
| give | `item` (`diamond`, or `studio:<item id>`), `amount` |\n\
| menu / dialog / loot / advancement | the id of that studio file, in a field named like the type |\n\
| enchant | `enchant` (a studio enchantment id), `level` (0 takes it off): on the item in the player's main hand |\n\
| cosmetic | `cosmetic` (a studio cosmetic id), `mode`: `toggle` (default), `wear` or `remove` |\n\
| emote | `model` (a studio model id), `animation`: the model rides the player and plays it once |\n\
| trade | `trade` (a studio trade list id): opens its trade screen (Paper) |\n\
| shop | `shop` (a studio shop id): opens its shop screen |\n\
| money | `op` (`give`, `take`, `set`), `amount`: Vault money (Paper with a Vault economy plugin) |\n\
| function | `function` (a studio function id): runs that function's `actions` here, with the same player and placeholders |\n\
| quest | `quest` (a studio quest id), `mode`: `start` (default), `abandon` or `complete` |\n\
| structure | `structure` (a studio structure id), `at`: `player` (default) or `spot` (`world`, `x`, `y`, `z`), `rotation` (`0`, `90`, `180`, `270`, `random`; empty = the file's) |\n\
| close | (closes the open menu) |\n\
| set | `variable`, `scope` (`player` or `server`), `op` (`set`, `add`, `subtract`), `value` |\n\
| wait | `ticks` |\n\
| take | `item`, `amount`: takes items from the player |\n\
| xp | `op` (`give`, `take`, `set`), `amount`, `levels` (true = levels, false = points) |\n\
| gamemode | `mode` (`survival`, `creative`, `adventure`, `spectator`) |\n\
| kick | `text` (the reason) |\n\
| food, health | `amount` (20 = full) |\n\
| bossbar | `op` (`show`, `hide`), `text`, `color`, `progress` (0 to 1), `seconds` (0 = until hidden) |\n\
| block | `block`, `world` (empty = the player's), `x`, `y`, `z` (numbers, or `~`/`~2` from the player) |\n\
| fill | `block`, `world`, `x1` `y1` `z1` to `x2` `y2` `z2` |\n\
| region | `op` (`copy`: the box `x1`..`z2` into `name`; `paste`: `name` at `x` `y` `z`, `rotate` 0/90/180/270, `air`), `world` |\n\
| set_permission | `op` (`give`, `take`), `permission`, `seconds` (0 = for good; LuckPerms when installed, else until they leave) |\n\
| set_group | `op` (`add`, `remove`), `group`: a LuckPerms group (Paper) |\n\
| manage_world | `op` (`load`, `unload`, `spawn`, `time`, `weather`, `difficulty`, `border`, `rule`), `world`, `value`, `rule` |\n\
| cancel | (stops what the player did, in rules whose event can be stopped) |\n\
| stop | (runs nothing after it) |\n\
| if | `if` (conditions), `then`, `else` (steps) |\n\
| repeat | `times`, `actions` |\n\
| each_player | `actions`: run once for every online player, as that player |\n\n\
Conditions (`if`) are steps too: `permission` (`permission`), `world` (`worlds`), `variable` (`variable`, `scope`, `op`\n\
one of `=` `!=` `>` `<` `>=` `<=`, `value`), `chance` (`percent`), `online` (`op`, `value`), `holding` (`item`),\n\
`advancement` (`advancement`), `quest` (`quest`, `state`: `active`, `done` or `new`), `money` (`amount`: has at least that\n\
much Vault money, Paper), `has_item` (`item`, `amount`), `gamemode` (`modes`), `health` and `level` (`op`, `value`), `sneaking`,\n\
`time` (`is`: `day` or `night`), `weather` (`is`: `clear`, `rain` or `thunder`), `group` (`group`, LuckPerms, Paper). All must hold.\n\n\
A rule's `when.event` is one of: `join`, `first_join`, `quit`, `chat`, `death`, `respawn`, `break`, `place`, `world_change`, `command`, `timer`, `schedule`,\n\
`server_start`, `click`, `click_entity`, `kill`, `hurt`, `hit`, `eat`, `craft`, `fish`, `sneak`, `drop`, `pickup`, `level_up`,\n\
`teleport`, `gamemode`.\n\n\
**What a player typed** in a dialog is `<input:key>`. In a `command` it only goes in when it is letters, digits, `_`, `.`\n\
and `-` (otherwise the command does not run, so nobody can type `@a` into it). `<input:key|raw>` puts it in as typed,\n\
for a plugin's last free-text argument; never use `|raw` with `say`, `tell`, `msg` or `execute`.\n\n\
## Kinds\n\nEach example is what the app's **New** button writes.\n"
    );
    for kind in kinds {
        out.push_str(&format!("\n### {kind}\n\n```json\n{}```\n", template(kind, &format!("example-{kind}"))));
    }
    out.push_str(
        "\nA dialog body line is text or `{\"item\": \"diamond\", \"count\": 1, \"text\": \"...\"}`. A dialog input of type `text` takes\n\
`lines` (more than 1 = a text box) and `maxLength`; a `choice` option is text or `{\"id\": \"...\", \"label\": \"...\"}`. A button\n\
takes `permission` (hidden without it) and `url` (opens a link, no actions). `\"exit\": {\"label\", \"actions\"}` is the\n\
button under the others that Esc presses too.\n",
    );
    out
}

/// Whether to write the guide: there is none yet, or ours is out of date (never over somebody else's file).
pub fn guide_update(now: Option<&str>, next: &str) -> bool {
    match now {
        None => true,
        Some(now) => now.starts_with(GUIDE_MARK) && now != next,
    }
}

/// Lists files (kind, id, modified secs) against what the add-on reported.
pub fn overview(root: Option<&str>, kinds: &[&str], status: Option<&str>, files: Vec<(String, String, u64)>) -> StudioOverview {
    let Status { on, addon: addon_version, at: status_at, rows } = status.map(read_status).unwrap_or_default();
    let mut resources: Vec<StudioResource> = files
        .into_iter()
        .map(|(kind, id, modified)| {
            let row = rows.iter().find(|(one, name, _, _)| *one == kind && *name == id);
            StudioResource {
                enabled: row.map(|one| one.2).unwrap_or(true),
                error: row.map(|one| one.3.clone()).unwrap_or_default(),
                seen: row.is_some(),
                modified: modified as f64 * 1000.0,
                kind,
                id,
            }
        })
        .collect();
    resources
        .sort_by(|a, b| (KINDS.iter().position(|one| *one == a.kind), &a.id).cmp(&(KINDS.iter().position(|one| *one == b.kind), &b.id)));
    StudioOverview {
        supported: root.is_some(),
        root: root.unwrap_or_default().to_string(),
        kinds: kinds.iter().map(|one| one.to_string()).collect(),
        has_status: status.is_some(),
        on,
        addon_version,
        status_at,
        resources,
        off: Vec::new(),
        overlaps: Vec::new(),
    }
}

/// A top-down look at a custom world's generator, `size`×`size` spots, row by row.
#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorldgenPreview {
    pub size: u32,
    pub step: u32,
    pub sea: i32,
    /// Ground height of each spot.
    pub heights: Vec<i32>,
    /// Index into `blocks` of the block seen from above.
    pub tops: Vec<u16>,
    pub blocks: Vec<String>,
    /// Index into `biome_names` of each spot's biome (empty name: the server's own, on vanilla ground).
    pub biomes: Vec<u16>,
    pub biome_names: Vec<String>,
    /// `"mode": "vanilla"`: only the biomes are known.
    pub vanilla: bool,
    pub problems: Vec<String>,
}

/// Draws a custom world's generator for the editor. `generator` is the world file's `"generator"` block as JSON.
pub fn worldgen_preview(generator: &str, seed: &str, world: &str, x: i32, z: i32, size: u32, step: u32) -> Result<WorldgenPreview, String> {
    let value: serde_json::Value = serde_json::from_str(generator).map_err(|error| format!("Invalid JSON: {error}"))?;
    let data = value.as_object().cloned().unwrap_or_default();
    let size = size.clamp(8, 256);
    let step = step.clamp(1, 64);
    let made = studio_format::worldgen::Generator::new(&data, studio_format::worldgen::seed_of(seed, world));
    let mut blocks: Vec<String> = Vec::new();
    let mut biome_names: Vec<String> = Vec::new();
    let mut heights = Vec::new();
    let mut tops = Vec::new();
    let mut biomes = Vec::new();
    let index_of = |list: &mut Vec<String>, name: String| match list.iter().position(|one| *one == name) {
        Some(index) => index as u16,
        None => {
            list.push(name);
            (list.len() - 1) as u16
        }
    };
    for (height, top, biome) in studio_format::worldgen::preview(&made, x, z, size, step) {
        heights.push(height);
        tops.push(index_of(&mut blocks, top));
        biomes.push(index_of(&mut biome_names, biome));
    }
    Ok(WorldgenPreview {
        size,
        step,
        sea: made.sea,
        heights,
        tops,
        blocks,
        biomes,
        biome_names,
        vanilla: made.vanilla().is_some(),
        problems: studio_format::worldgen::problems(&data),
    })
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioSaved {
    pub hash: String,
    /// The running server took it at once over the live line; otherwise it reads it within two seconds.
    pub applied: bool,
}

/// Where the add-on's live line listens, from `studio/live.json`. The port is on the server machine's own
/// 127.0.0.1: here for a local server, through an SSH tunnel for an SSH one.
pub struct Live {
    pub port: u16,
    pub token: String,
}

pub fn read_live(live_json: &str) -> Result<Live, String> {
    let live: Value = serde_json::from_str(live_json).map_err(|_| "live.json is unreadable.".to_string())?;
    let port = live.get("port").and_then(Value::as_u64).filter(|one| (1..=65535).contains(one)).ok_or("live.json has no port.")? as u16;
    let token = live.get("token").and_then(Value::as_str).ok_or("live.json has no token.")?.to_string();
    Ok(Live { port, token })
}

/// A live line: a socket to a server on this PC, or an SSH tunnel to one elsewhere.
pub trait Line: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Line for T {}

/// The live line of a server on this PC.
pub async fn local_line(port: u16) -> Result<tokio::net::TcpStream, String> {
    match tokio::time::timeout(std::time::Duration::from_millis(800), tokio::net::TcpStream::connect(("127.0.0.1", port))).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err("The add-on didn't answer.".into()),
    }
}

async fn send<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, message: Value) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    writer
        .write_all(
            format!(
                "{message}
"
            )
            .as_bytes(),
        )
        .await
        .map_err(|error| error.to_string())?;
    writer.flush().await.map_err(|error| error.to_string())
}

/// Asks the add-on to reread now, over its live line (a local socket or an SSH tunnel). Returns status.json as JSON text.
pub async fn reload_on<S>(stream: S, token: &str) -> Result<String, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(reader).lines();
    let talk = async {
        // The reload goes only after the welcome: a refused hello closes the line, and unread data there would turn
        // the refusal into a connection reset.
        send(&mut writer, json!({ "type": "hello", "token": token })).await?;
        let mut welcomed = false;
        for _ in 0..32 {
            let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? else { break };
            let message: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
            match message.get("type").and_then(Value::as_str) {
                Some("welcome") if !welcomed => {
                    welcomed = true;
                    send(&mut writer, json!({ "type": "reload" })).await?;
                }
                Some("status") => return Ok(message.get("status").cloned().unwrap_or(Value::Null).to_string()),
                Some("error") => return Err(message.get("message").and_then(Value::as_str).unwrap_or("The add-on said no.").to_string()),
                // Player events and the like.
                _ => {}
            }
        }
        Err("The add-on didn't answer.".to_string())
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), talk).await.unwrap_or_else(|_| Err("The add-on didn't answer.".into()))
}

/// Stays on the live line and hands every message the add-on sends (player joins, rule traces) to `heard`,
/// until `stop` is set or the line closes. Returns why it ended.
pub async fn watch_on<S>(stream: S, token: &str, stop: &std::sync::atomic::AtomicBool, heard: impl Fn(Value)) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use std::sync::atomic::Ordering;
    use tokio::io::AsyncBufReadExt;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(reader).lines();
    send(&mut writer, json!({ "type": "hello", "token": token })).await?;
    while !stop.load(Ordering::Relaxed) {
        // Wake up now and then to notice `stop`, and ping so a dead line shows. `next_line` loses nothing when cut short.
        match tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line()).await {
            Ok(Ok(None)) => return Ok(()),
            Ok(Ok(Some(line))) => {
                let message: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
                match message.get("type").and_then(Value::as_str) {
                    Some("error") => {
                        return Err(message.get("message").and_then(Value::as_str).unwrap_or("The add-on said no.").to_string())
                    }
                    Some("player") | Some("trace") => heard(message),
                    _ => {}
                }
            }
            Ok(Err(error)) => return Err(error.to_string()),
            Err(_) => send(&mut writer, json!({ "type": "ping" })).await?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block<T>(work: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(work)
    }

    /// What `studio_save` does for a server on this PC.
    fn live_reload(live_json: &str) -> Result<String, String> {
        block(async {
            let live = read_live(live_json)?;
            reload_on(local_line(live.port).await?, &live.token).await
        })
    }

    fn watch(live_json: &str, stop: &std::sync::atomic::AtomicBool, heard: impl Fn(Value)) -> Result<(), String> {
        block(async {
            let live = read_live(live_json)?;
            watch_on(local_line(live.port).await?, &live.token, stop, heard).await
        })
    }

    /// The SSH path hands over a tunnel instead of a socket; any byte stream works the same.
    #[test]
    fn reloads_over_any_stream() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        block(async {
            let (near, far) = tokio::io::duplex(4096);
            let addon = tokio::spawn(async move {
                let (reader, mut writer) = tokio::io::split(far);
                let mut lines = tokio::io::BufReader::new(reader).lines();
                assert!(lines.next_line().await.unwrap().unwrap().contains("\"token\":\"t\""));
                writer
                    .write_all(
                        b"{\"type\":\"welcome\"}
",
                    )
                    .await
                    .unwrap();
                assert_eq!(lines.next_line().await.unwrap().unwrap(), r#"{"type":"reload"}"#);
                writer
                    .write_all(
                        b"{\"type\":\"status\",\"status\":{\"on\":true}}
",
                    )
                    .await
                    .unwrap();
            });
            assert_eq!(reload_on(near, "t").await.unwrap(), r#"{"on":true}"#);
            addon.await.unwrap();
        });
    }

    /// Against the real add-on code: `STUDIO_LIVE=<path to live.json> cargo test real_live -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the add-on's live line running"]
    fn real_live() {
        let file = std::env::var("STUDIO_LIVE").expect("STUDIO_LIVE");
        let live = std::fs::read_to_string(&file).unwrap();
        let status = live_reload(&live).expect("reload");
        eprintln!("status {status}");
        assert!(status.contains("\"resources\""));
        let wrong = live.replace("\"token\":\"", "\"token\":\"x");
        assert_eq!(live_reload(&wrong).unwrap_err(), "Wrong token.");
    }

    /// A fake add-on: checks the hello, sends a player event first, then the status.
    #[test]
    fn reloads_over_the_live_line() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut out = stream.try_clone().unwrap();
                let mut lines = BufReader::new(stream).lines();
                let hello: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
                if hello["token"] != "secret" {
                    writeln!(out, r#"{{"type":"error","message":"Wrong token."}}"#).unwrap();
                    continue;
                }
                writeln!(out, r#"{{"type":"welcome","addon":"38"}}"#).unwrap();
                assert_eq!(lines.next().unwrap().unwrap(), r#"{"type":"reload"}"#);
                writeln!(out, r#"{{"type":"player","event":"join","name":"Steve","online":1}}"#).unwrap();
                writeln!(out, r#"{{"type":"status","status":{{"schema":1,"on":true,"resources":[]}}}}"#).unwrap();
            }
        });
        let status = live_reload(&format!(r#"{{"port":{port},"token":"secret"}}"#)).unwrap();
        assert!(status.contains("\"on\":true"));
        assert_eq!(live_reload(&format!(r#"{{"port":{port},"token":"nope"}}"#)).unwrap_err(), "Wrong token.");
        server.join().unwrap();
        assert!(live_reload("{}").is_err());
        assert!(live_reload("not json").is_err());
    }

    /// A fake add-on that sends a join and a trace, then closes: both reach `heard`, pongs and welcomes don't.
    #[test]
    fn watches_the_live_line() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut out = stream.try_clone().unwrap();
            let mut lines = BufReader::new(stream).lines();
            let hello: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            assert_eq!(hello["token"], "t");
            writeln!(out, r#"{{"type":"welcome","addon":"39"}}"#).unwrap();
            writeln!(out, r#"{{"type":"player","event":"join","name":"Steve","online":1}}"#).unwrap();
            writeln!(out, r#"{{"type":"trace","rule":"hi","event":"join","player":"Steve","passed":true,"ran":["message"],"at":1}}"#)
                .unwrap();
        });
        let heard = std::sync::Mutex::new(Vec::new());
        let stop = std::sync::atomic::AtomicBool::new(false);
        watch(&format!(r#"{{"port":{port},"token":"t"}}"#), &stop, |one| heard.lock().unwrap().push(one)).unwrap();
        server.join().unwrap();
        let heard = heard.into_inner().unwrap();
        assert_eq!(heard.len(), 2);
        assert_eq!(heard[0]["type"], "player");
        assert_eq!(heard[1]["rule"], "hi");
    }

    #[test]
    fn finds_plugins_that_do_the_same_job() {
        let names: Vec<(String, Option<String>)> = [
            ("TAB v4.1.2.jar", None),
            ("TabList.jar", Some("TabList")),
            ("DeluxeMenus-1.14.0.jar", None),
            ("ChatControlRed-10.jar", None),
            ("FeatherBoard.jar.disabled", Some("FeatherBoard")),
            ("config.yml", None),
            // Named differently on disk, found by what its plugin.yml says.
            ("hologram-plugin-2.8.jar", Some("DecentHolograms")),
            ("whatever.jar", Some("SomethingElse")),
        ]
        .iter()
        .map(|(file, yml)| (file.to_string(), yml.map(str::to_string)))
        .collect();
        let found = overlaps(&names);
        let plugins: Vec<&str> = found.iter().map(|one| one.plugin.as_str()).collect();
        assert_eq!(plugins, ["TAB", "DeluxeMenus", "ChatControlRed", "DecentHolograms"]);
        assert_eq!(found[3].file, "hologram-plugin-2.8.jar");
        assert_eq!(found[0].kinds, ["tab", "scoreboard"]);
        assert_eq!(found[0].file, "TAB v4.1.2.jar");
        assert!(OVERLAPS.iter().all(|(_, kinds)| kinds.iter().all(|kind| valid_kind(kind))));
    }

    #[test]
    fn names_stay_inside_the_studio_folder() {
        assert_eq!(path_of(PAPER_ROOT, "motd", "default").unwrap(), "plugins/Monolab/studio/motd/default.json");
        assert_eq!(path_of(PUMPKIN_ROOT, "tab", "a").unwrap(), "plugins/data/monolab/studio/tab/a.json");
        assert!(path_of(PAPER_ROOT, "motd", "../x").is_err());
        assert!(path_of(PAPER_ROOT, "motd", "Big").is_err());
        assert!(path_of(PAPER_ROOT, "weird", "a").is_err());
        assert!(path_of(PAPER_ROOT, "tab", &"a".repeat(49)).is_err());
    }

    #[test]
    fn checks_like_the_addon() {
        assert_eq!(check("a", r#"{"schema":1,"id":"a","enabled":true}"#), None);
        assert_eq!(check("a", r#"{"schema":1}"#), None);
        assert!(check("a", "{").unwrap().starts_with("Invalid JSON"));
        assert!(check("a", "[]").unwrap().contains("object"));
        assert!(check("a", "{}").unwrap().contains("schema"));
        assert!(check("a", r#"{"schema":2}"#).unwrap().contains("newer"));
        assert!(check("a", r#"{"schema":1.5}"#).unwrap().contains("whole"));
        assert!(check("a", r#"{"schema":1,"id":"b"}"#).unwrap().contains("file name"));
        assert!(check("a", r#"{"schema":1,"enabled":"yes"}"#).unwrap().contains("enabled"));
    }

    #[test]
    fn templates_pass_their_own_checks() {
        for kind in KINDS {
            let text = template(kind, "new-one");
            assert_eq!(check("new-one", &text), None, "{kind}");
            assert!(text.starts_with("{\n  \"schema\": 1,\n  \"id\": \"new-one\",\n  \"enabled\": true,\n"), "{kind}: {text}");
        }
    }

    #[test]
    fn matches_files_to_the_addon_report() {
        let status = r#"{"schema":1,"addon":"38","at":5,"on":true,"resources":[
            {"kind":"motd","id":"default","enabled":true},
            {"kind":"tab","id":"bad","enabled":false,"error":"Invalid JSON: x"}]}"#;
        let files = vec![
            ("tab".to_string(), "bad".to_string(), 2),
            ("motd".to_string(), "default".to_string(), 1),
            ("scoreboard".to_string(), "fresh".to_string(), 3),
        ];
        let seen = overview(Some(PAPER_ROOT), &KINDS, Some(status), files);
        assert!(seen.has_status && seen.on && seen.addon_version == "38");
        let order: Vec<&str> = seen.resources.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(order, ["fresh", "bad", "default"]);
        assert!(!seen.resources[0].seen && seen.resources[0].error.is_empty());
        assert_eq!(seen.resources[1].error, "Invalid JSON: x");
        assert!(seen.resources[2].seen && seen.resources[2].enabled);
        let none = overview(Some(PAPER_ROOT), &KINDS, None, Vec::new());
        assert!(!none.has_status && !none.on);
    }

    #[test]
    fn the_guide_shows_every_kind_and_keeps_hand_written_ones() {
        let text = guide(PAPER_ROOT, &KINDS);
        assert!(text.starts_with(GUIDE_MARK));
        for kind in KINDS {
            assert!(text.contains(&format!("### {kind}\n")), "{kind}");
        }
        assert!(text.contains("plugins/Monolab/studio/<kind>/<id>.json"));
        assert!(guide_update(None, &text));
        assert!(!guide_update(Some(&text), &text));
        assert!(guide_update(Some(&format!("{GUIDE_MARK}\nold")), &text));
        assert!(!guide_update(Some("# Our own notes"), &text));
    }

    #[test]
    fn hashes_are_stable() {
        assert_eq!(hash("a"), hash("a"));
        assert_ne!(hash("a"), hash("b"));
        assert_eq!(hash("").len(), 64);
    }
}

#[cfg(test)]
mod main_world_tests {
    use super::*;

    #[test]
    fn bukkit_generator_sets_and_takes_back() {
        assert_eq!(bukkit_generator("", "world", Some("spiral")), "worlds:\n  world:\n    generator: Monolab:spiral\n");
        let base = "settings:\n  allow-end: true\naliases: now-in-commands.yml\n";
        let set = bukkit_generator(base, "world", Some("spiral"));
        assert_eq!(set, format!("{base}worlds:\n  world:\n    generator: Monolab:spiral\n"));
        assert_eq!(bukkit_generator(&set, "world", Some("isles")), format!("{base}worlds:\n  world:\n    generator: Monolab:isles\n"));
        assert_eq!(bukkit_generator(&set, "world", None), format!("{base}worlds:\n  world:\n"));
        let other = "worlds:\n  world:\n    generator: Terra:DEFAULT\n  lobby:\n    generator: VoidGen\nticks-per:\n  autosave: 6000\n";
        assert_eq!(bukkit_generator(other, "world", None), other);
        assert_eq!(
            bukkit_generator(other, "lobby", Some("a")),
            "worlds:\n  world:\n    generator: Terra:DEFAULT\n  lobby:\n    generator: Monolab:a\nticks-per:\n  autosave: 6000\n"
        );
        assert_eq!(bukkit_generator(other, "survival", Some("a")), "worlds:\n  survival:\n    generator: Monolab:a\n  world:\n    generator: Terra:DEFAULT\n  lobby:\n    generator: VoidGen\nticks-per:\n  autosave: 6000\n");
        assert_eq!(bukkit_generator("worlds:\n  world:\n    generator: 'Monolab:x'\n", "world", None), "worlds:\n  world:\n");
        assert_eq!(bukkit_generator(base, "world", None), base);
    }

    #[test]
    fn main_world_picks_the_first_marked_custom_file() {
        let file = |id: &str, data: Value| (id.to_string(), data.as_object().unwrap().clone());
        let files = vec![
            file("a", json!({ "type": "flat", "main": true })),
            file("b", json!({ "type": "custom", "main": true, "enabled": false })),
            file("c", json!({ "type": "custom" })),
            file("d", json!({ "type": "custom", "main": true })),
            file("e", json!({ "type": "custom", "main": true })),
        ];
        assert_eq!(main_world(&files), Some("d"));
        assert_eq!(main_world(&files[..3]), None);
    }
}
