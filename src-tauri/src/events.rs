
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const ADDON_VERSION: u32 = 27;

#[cfg(test)]
const GRADLE_PROPERTIES: &str = include_str!("../../gradle.properties");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddonKind {
    Paper,
    NeoForge,
    Fabric,
    Pumpkin,
}

pub fn addon_kind(core: &str) -> Option<AddonKind> {
    match core {
        "Paper" | "Purpur" | "Folia" => Some(AddonKind::Paper),
        "NeoForge" => Some(AddonKind::NeoForge),
        "Fabric" => Some(AddonKind::Fabric),
        "Pumpkin" => Some(AddonKind::Pumpkin),
        _ => None,
    }
}

pub const PUMPKIN_PLUGIN_NAME: &str = "monolab";

pub const PREFIX: &str = "Monolab-";

pub fn is_our_addon(name: &str) -> bool {
    name.starts_with(PREFIX)
}

pub fn event_folder(server: &Path) -> PathBuf {
    if server.join(crate::pumpkin::CONFIG).is_file() {
        server.join("plugins").join("data").join(PUMPKIN_PLUGIN_NAME)
    } else if server.join("mods").is_dir() && !server.join("plugins").is_dir() {
        server.join("config").join("monolab")
    } else {
        server.join("plugins").join("Monolab")
    }
}

pub fn write_timezone(server: &Path) -> Result<(), String> {
    let folder = event_folder(server);
    fs::create_dir_all(&folder)
        .map_err(|error| format!("Could not create the record folder: {error}"))?;
    let minutes = chrono::Local::now().offset().local_minus_utc() / 60;
    fs::write(
        folder.join("timezone.json"),
        format!("{{\"utcOffsetMinutes\":{minutes}}}
"),
    )
    .map_err(|error| format!("Could not save timezone.json: {error}"))
}

pub fn addon_folder(server: &Path, kind: AddonKind) -> PathBuf {
    match kind {
        AddonKind::Paper | AddonKind::Pumpkin => server.join("plugins"),
        AddonKind::NeoForge | AddonKind::Fabric => server.join("mods"),
    }
}

const TARGETS_JSON: &str = include_str!("../../addon/neoforge/targets.json");

#[derive(serde::Deserialize, specta::Type)]
struct TargetFile {
    targets: Vec<ModTarget>,
}

#[derive(Clone, Debug, serde::Deserialize, specta::Type)]
pub struct ModTarget {
    pub id: String,
    pub games: Vec<String>,
}

fn mod_targets() -> &'static [ModTarget] {
    static PARSED: std::sync::OnceLock<Vec<ModTarget>> = std::sync::OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str::<TargetFile>(TARGETS_JSON)
            .expect("Could not read addon/neoforge/targets.json")
            .targets
    })
}

pub fn mod_target(game_version: &str) -> Option<&'static ModTarget> {
    mod_targets()
        .iter()
        .find(|target| target.games.iter().any(|one| one == game_version))
}

const FABRIC_TARGETS_JSON: &str = include_str!("../../addon/fabric/targets.json");

#[derive(serde::Deserialize, specta::Type)]
struct FabricTargetFile {
    targets: Vec<FabricTarget>,
}

#[derive(Clone, Debug, serde::Deserialize, specta::Type)]
pub struct FabricTarget {
    pub id: String,
}

fn fabric_targets() -> &'static [FabricTarget] {
    static PARSED: std::sync::OnceLock<Vec<FabricTarget>> = std::sync::OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str::<FabricTargetFile>(FABRIC_TARGETS_JSON)
            .expect("Could not read addon/fabric/targets.json")
            .targets
    })
}

pub fn fabric_target(game_version: &str) -> Option<&'static FabricTarget> {
    fabric_targets()
        .iter()
        .find(|target| target.id == game_version)
}

const PUMPKIN_TARGETS_JSON: &str = include_str!("../../addon/pumpkin/targets.json");

#[derive(serde::Deserialize, specta::Type)]
struct PumpkinTargetFile {
    targets: Vec<PumpkinTarget>,
}

#[derive(Clone, Debug, serde::Deserialize, specta::Type)]
pub struct PumpkinTarget {
    pub id: String,
    pub games: Vec<String>,
}

fn pumpkin_targets() -> &'static [PumpkinTarget] {
    static PARSED: std::sync::OnceLock<Vec<PumpkinTarget>> = std::sync::OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str::<PumpkinTargetFile>(PUMPKIN_TARGETS_JSON)
            .expect("Could not read addon/pumpkin/targets.json")
            .targets
    })
}

pub fn pumpkin_target(version: &str) -> Option<&'static PumpkinTarget> {
    pumpkin_targets()
        .iter()
        .find(|target| target.games.iter().any(|one| one == version))
}

pub fn supported_games(kind: AddonKind) -> Vec<String> {
    match kind {
        AddonKind::Paper => Vec::new(),
        AddonKind::NeoForge => mod_targets()
            .iter()
            .flat_map(|target| target.games.iter().cloned())
            .collect(),
        AddonKind::Fabric => fabric_targets()
            .iter()
            .map(|target| target.id.clone())
            .collect(),
        AddonKind::Pumpkin => pumpkin_targets()
            .iter()
            .flat_map(|target| target.games.iter().cloned())
            .collect(),
    }
}

pub fn jar_name(kind: AddonKind, game_version: &str, version: u32) -> Option<String> {
    match kind {
        AddonKind::Paper => Some(format!("Monolab-{version}.jar")),
        AddonKind::NeoForge => mod_target(game_version)
            .map(|target| format!("Monolab-mod-{}-{version}.jar", target.id)),
        AddonKind::Fabric => fabric_target(game_version)
            .map(|target| format!("Monolab-fabric-{}-{version}.jar", target.id)),
        AddonKind::Pumpkin => pumpkin_target(game_version)
            .map(|target| format!("Monolab-pumpkin-{}-{version}.wasm", target.id)),
    }
}

pub fn installed_addon(server: &Path, kind: AddonKind) -> Option<(String, u32)> {
    let entries = fs::read_dir(addon_folder(server, kind)).ok()?;
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            parse_jar_version(kind, &name).map(|version| (name, version))
        })
        .max_by_key(|(_, version)| *version)
}

pub fn addon_version_in_name(kind: AddonKind, file_name: &str) -> Option<u32> {
    parse_jar_version(kind, file_name)
}

fn parse_jar_version(kind: AddonKind, file_name: &str) -> Option<u32> {
    let file_name = file_name.strip_prefix(PREFIX)?;
    if kind == AddonKind::Pumpkin {
        let rest = file_name.strip_prefix("pumpkin-")?;
        let digits = rest.split_once('-')?.1.strip_suffix(".wasm")?;
        return digits.parse().ok();
    }
    let rest = match kind {
        AddonKind::Paper => file_name,
        AddonKind::NeoForge => {
            let rest = file_name.strip_prefix("mod-")?;
            rest.split_once('-')?.1
        }
        AddonKind::Fabric => {
            let rest = file_name.strip_prefix("fabric-")?;
            rest.split_once('-')?.1
        }
        AddonKind::Pumpkin => unreachable!("handled above"),
    };
    let digits = rest.strip_suffix(".jar")?;
    digits.parse().ok()
}

pub fn event_days(server: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(event_folder(server)) else {
        return Vec::new();
    };
    let mut days: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let rest = name.strip_prefix("events-")?;
            Some(rest.strip_suffix(".jsonl")?.to_string())
        })
        .collect();
    days.sort();
    days.reverse();
    days
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GameEvent {
    pub at: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mspt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub players: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunks: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placed: Option<BTreeMap<String, u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broken: Option<BTreeMap<String, u64>>,
}

fn read_counts(value: Option<&serde_json::Value>) -> Option<BTreeMap<String, u64>> {
    let found: BTreeMap<String, u64> = value?
        .as_object()?
        .iter()
        .filter_map(|(block, count)| Some((block.clone(), count.as_u64()?)))
        .collect();
    (!found.is_empty()).then_some(found)
}

pub fn parse_line(line: &str) -> Option<GameEvent> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(|found| found.as_str())
            .filter(|found| !found.is_empty())
            .map(str::to_owned)
    };
    let number = |key: &str| value.get(key).and_then(|found| found.as_f64());
    Some(GameEvent {
        at: value.get("at").and_then(|found| found.as_u64())?,
        kind: text("kind")?,
        player: text("player"),
        world: text("world"),
        x: number("x"),
        y: number("y"),
        z: number("z"),
        tps: number("tps"),
        mspt: number("mspt"),
        memory_mib: value.get("memoryMib").and_then(|found| found.as_u64()),
        players: value.get("players").and_then(|found| found.as_u64()),
        chunks: value.get("chunks").and_then(|found| found.as_u64()),
        entities: value.get("entities").and_then(|found| found.as_u64()),
        from: value.get("from").and_then(|found| found.as_u64()),
        placed: read_counts(value.get("placed")),
        broken: read_counts(value.get("broken")),
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LivePlayer {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub health: i64,
    pub food: i64,
    pub ping: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveState {
    pub at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mspt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_memory_mib: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunks: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<u64>,
    pub entity_kinds: Vec<EntityKind>,
    pub hotspots: Vec<Hotspot>,
    pub worlds: Vec<LiveWorld>,
    pub players: Vec<LivePlayer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EntityKind {
    pub r#type: String,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Hotspot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
    pub x: i64,
    pub z: i64,
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveWorld {
    pub name: String,
    pub chunks: u64,
    pub entities: u64,
}

const LIVE_STALE_MS: u64 = 15_000;

fn stale(at_ms: u64, limit_ms: u64) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0);
    now > at_ms && now - at_ms > limit_ms
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandNode {
    pub kind: String,
    pub name: Option<String>,
    pub children: Vec<usize>,
    pub redirect: Option<usize>,
    pub run: bool,
    pub from: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerCommand {
    pub name: String,
    pub usage: Option<String>,
    pub from: Option<String>,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandBook {
    pub nodes: Vec<CommandNode>,
    pub flat: Vec<ServerCommand>,
}

pub fn read_command_book(server: &Path) -> CommandBook {
    let Ok(text) = fs::read_to_string(event_folder(server).join("commands.json")) else {
        return CommandBook::default();
    };
    command_book_from_text(&text)
}

pub fn command_book_from_text(text: &str) -> CommandBook {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return CommandBook::default();
    };
    CommandBook {
        nodes: read_nodes(&value),
        flat: read_flat(&value),
    }
}

fn read_nodes(value: &serde_json::Value) -> Vec<CommandNode> {
    value
        .get("nodes")
        .and_then(|found| found.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|one| {
                    Some(CommandNode {
                        kind: one.get("kind")?.as_str()?.to_string(),
                        name: one
                            .get("name")
                            .and_then(|found| found.as_str())
                            .map(str::to_owned),
                        children: one
                            .get("children")
                            .and_then(|found| found.as_array())
                            .map(|kids| {
                                kids.iter()
                                    .filter_map(|kid| kid.as_str()?.parse().ok())
                                    .collect()
                            })
                            .unwrap_or_default(),
                        redirect: one
                            .get("redirect")
                            .and_then(|found| found.as_u64())
                            .map(|at| at as usize),
                        run: one
                            .get("run")
                            .and_then(|found| found.as_bool())
                            .unwrap_or(false),
                        from: one
                            .get("from")
                            .and_then(|found| found.as_str())
                            .map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_flat(value: &serde_json::Value) -> Vec<ServerCommand> {
    value
        .get("commands")
        .and_then(|found| found.as_array())
        .into_iter()
        .flatten()
        .filter_map(|one| {
            Some(ServerCommand {
                name: one.get("name")?.as_str()?.to_string(),
                usage: one
                    .get("usage")
                    .and_then(|found| found.as_str())
                    .map(str::to_owned),
                from: one
                    .get("from")
                    .and_then(|found| found.as_str())
                    .map(str::to_owned),
                args: one
                    .get("args")
                    .and_then(|found| found.as_array())
                    .map(|list| {
                        list.iter()
                            .filter_map(|item| item.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}

pub fn read_live(server: &Path) -> Option<LiveState> {
    let text = fs::read_to_string(event_folder(server).join("live.json")).ok()?;
    live_from_text(&text)
}

pub fn live_from_text(text: &str) -> Option<LiveState> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let players = value
        .get("players")?
        .as_array()?
        .iter()
        .filter_map(|one| {
            let number = |key: &str| one.get(key).and_then(|found| found.as_f64());
            let whole = |key: &str| one.get(key).and_then(|found| found.as_i64()).unwrap_or(0);
            Some(LivePlayer {
                name: one.get("name")?.as_str()?.to_string(),
                world: one
                    .get("world")
                    .and_then(|found| found.as_str())
                    .map(str::to_owned),
                x: number("x").unwrap_or(0.0),
                y: number("y").unwrap_or(0.0),
                z: number("z").unwrap_or(0.0),
                health: whole("health"),
                food: whole("food"),
                ping: whole("ping"),
            })
        })
        .collect();
    let whole = |key: &str| value.get(key).and_then(|found| found.as_u64());
    let at = value.get("at").and_then(|found| found.as_u64())?;
    if stale(at, LIVE_STALE_MS) {
        return None;
    }
    Some(LiveState {
        at,
        tps: value.get("tps").and_then(|found| found.as_f64()),
        mspt: value.get("mspt").and_then(|found| found.as_f64()),
        memory_mib: whole("memoryMib"),
        max_memory_mib: whole("maxMemoryMib"),
        chunks: whole("chunks"),
        entities: whole("entities"),
        entity_kinds: read_entity_kinds(value.get("entityKinds")),
        hotspots: read_hotspots(value.get("hotspots")),
        worlds: read_live_worlds(value.get("worlds")),
        players,
    })
}

fn read_entity_kinds(value: Option<&serde_json::Value>) -> Vec<EntityKind> {
    value
        .and_then(|found| found.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|one| {
                    Some(EntityKind {
                        r#type: one.get("type")?.as_str()?.to_string(),
                        count: one.get("count")?.as_u64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_hotspots(value: Option<&serde_json::Value>) -> Vec<Hotspot> {
    value
        .and_then(|found| found.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|one| {
                    Some(Hotspot {
                        world: one.get("world").and_then(|found| found.as_str()).map(str::to_owned),
                        x: one.get("x")?.as_i64()?,
                        z: one.get("z")?.as_i64()?,
                        count: one.get("count")?.as_u64()?,
                        top: one.get("top").and_then(|found| found.as_str()).map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_live_worlds(value: Option<&serde_json::Value>) -> Vec<LiveWorld> {
    value
        .and_then(|found| found.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|one| {
                    Some(LiveWorld {
                        name: one.get("name")?.as_str()?.to_string(),
                        chunks: one.get("chunks")?.as_u64()?,
                        entities: one.get("entities")?.as_u64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub kinds: Vec<String>,
    pub player: String,
    pub limit: usize,
}

fn matches_kind(kinds: &[String], event: &GameEvent) -> bool {
    kinds.contains(&event.kind)
}

pub fn read_day(server: &Path, day: &str, filter: &Filter) -> Vec<GameEvent> {
    if !day.chars().all(|letter| letter.is_ascii_digit() || letter == '-') {
        return Vec::new();
    }
    let path = event_folder(server).join(format!("events-{day}.jsonl"));
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let wanted = filter.player.to_lowercase();
    let mut found: Vec<GameEvent> = text
        .lines()
        .rev()
        .filter_map(parse_line)
        .filter(|event| filter.kinds.is_empty() || matches_kind(&filter.kinds, event))
        .filter(|event| {
            wanted.is_empty()
                || event
                    .player
                    .as_deref()
                    .is_some_and(|name| name.to_lowercase().contains(&wanted))
        })
        .take(if filter.limit == 0 { 500 } else { filter.limit })
        .collect();
    found.shrink_to_fit();
    found
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_command_tree_the_addon_writes() {
        let text = r#"{"at":1,"format":2,"nodes":[
            {"kind":"root","children":["1"]},
            {"kind":"literal","name":"difficulty","children":["2"],"from":"MyPlugin"},
            {"kind":"literal","name":"peaceful","run":true},
            {"kind":"literal","name":"execute","redirect":0}
        ]}"#;
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let book = super::CommandBook {
            nodes: super::read_nodes(&value),
            flat: super::read_flat(&value),
        };
        assert_eq!(book.nodes.len(), 4);
        assert_eq!(book.nodes[0].children, vec![1]);
        assert_eq!(book.nodes[1].name.as_deref(), Some("difficulty"));
        assert_eq!(book.nodes[1].from.as_deref(), Some("MyPlugin"));
        assert!(book.nodes[2].run);
        assert_eq!(book.nodes[3].redirect, Some(0));
        let old = serde_json::from_str::<serde_json::Value>(
            r#"{"at":1,"commands":[{"name":"say","args":["hi"]}]}"#,
        )
        .unwrap();
        assert!(super::read_nodes(&old).is_empty());
        assert_eq!(super::read_flat(&old).len(), 1);
    }

    use super::*;

    #[test]
    fn addon_version_matches_gradle() {
        let gradle = GRADLE_PROPERTIES
            .lines()
            .filter_map(|line| line.trim().strip_prefix("seAddonVersion"))
            .filter_map(|rest| rest.trim().strip_prefix('='))
            .map(|value| value.trim().to_string())
            .next()
            .expect("seAddonVersion is missing from gradle.properties");
        assert_eq!(
            gradle,
            ADDON_VERSION.to_string(),
            "seAddonVersion in gradle.properties differs from ADDON_VERSION in events.rs"
        );
        let pumpkin = include_str!("../../addon/pumpkin/src/lib.rs");
        assert!(
            pumpkin.contains(&format!("const VERSION: &str = \"{gradle}\";")),
            "VERSION in addon/pumpkin/src/lib.rs differs from gradle.properties"
        );
    }

    #[test]
    fn forgets_a_live_file_that_stopped_updating() {
        let root = std::env::temp_dir().join("monolab-live-stale-test");
        std::fs::remove_dir_all(&root).ok();
        let folder = event_folder(&root);
        std::fs::create_dir_all(&folder).unwrap();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let write = |at: u64| {
            std::fs::write(
                folder.join("live.json"),
                format!(
                    r#"{{"at":{at},"tps":20.0,"players":[{{"name":"alex","world":"world","x":1,"y":64,"z":2}}]}}"#
                ),
            )
            .unwrap();
        };

        write(now);
        let live = read_live(&root).expect("something just written should be read");
        assert_eq!(live.players.len(), 1);

        write(now - 8_000);
        assert!(
            read_live(&root).is_some(),
            "dropping data a few seconds old would make the screen flicker during lag"
        );

        write(now - 60_000);
        assert!(
            read_live(&root).is_none(),
            "stale data counts as missing"
        );

        write(now + 30_000);
        assert!(
            read_live(&root).is_some(),
            "treating a future time as stale would drop good values"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reads_a_join_line() {
        let event = parse_line(
            r#"{"at":1755846000123,"kind":"join","player":"alex","world":"world","x":1.5,"y":64,"z":2}"#,
        )
        .expect("should be readable");
        assert_eq!(event.kind, "join");
        assert_eq!(event.player.as_deref(), Some("alex"));
        assert_eq!(event.x, Some(1.5));
    }

    #[test]
    fn reads_a_block_tally() {
        let event = parse_line(
            r#"{"at":9,"kind":"blocks","player":"a","from":3,"placed":{"STONE":412},"broken":{"DIRT":88,"GRAVEL":2}}"#,
        )
        .expect("should be readable");
        assert_eq!(event.from, Some(3));
        assert_eq!(event.placed.as_ref().unwrap()["STONE"], 412);
        assert_eq!(event.broken.as_ref().unwrap().len(), 2);
        let only_placed =
            parse_line(r#"{"at":9,"kind":"blocks","player":"a","placed":{"STONE":1}}"#).unwrap();
        assert!(only_placed.broken.is_none());
    }

    #[test]
    fn keeps_a_kind_it_does_not_know() {
        let event = parse_line("{\"at\":1,\"kind\":\"mined\",\"player\":\"a\"}").expect("should be readable");
        assert_eq!(event.kind, "mined");
    }

    #[test]
    fn throws_away_only_the_broken_line() {
        assert!(parse_line(r#"{"at":1,"kind":"jo"#).is_none());
        assert!(parse_line("").is_none());
        assert!(parse_line(r#"{"kind":"join"}"#).is_none());
        assert!(parse_line(r#"{"at":1}"#).is_none());
    }

    #[test]
    fn reads_the_version_out_of_the_jar_name() {
        use AddonKind::{Fabric, NeoForge, Paper};
        assert_eq!(parse_jar_version(Paper, "Monolab-1.jar"), Some(1));
        assert_eq!(parse_jar_version(Paper, "Monolab-12.jar"), Some(12));
        assert_eq!(parse_jar_version(NeoForge, "Monolab-mod-mc1_21_1-5.jar"), Some(5));
        assert_eq!(parse_jar_version(NeoForge, "Monolab-mod-mc26_2-12.jar"), Some(12));
        assert_eq!(parse_jar_version(Paper, "Monolab-mod-mc1_21_1-5.jar"), None);
        assert_eq!(
            parse_jar_version(Fabric, "Monolab-fabric-1.21.1-6.jar"),
            Some(6)
        );
        assert_eq!(parse_jar_version(Fabric, "Monolab-mod-mc1_21_1-6.jar"), None);
        assert_eq!(parse_jar_version(NeoForge, "Monolab-fabric-1.21.1-6.jar"), None);
        assert_eq!(parse_jar_version(Paper, "Monolab-fabric-1.21.1-6.jar"), None);
        assert_eq!(parse_jar_version(NeoForge, "Monolab-5.jar"), None);
        assert_eq!(parse_jar_version(Paper, "Monolab-1.jar.disabled"), None);
        assert_eq!(parse_jar_version(Paper, "EssentialsX.jar"), None);
        assert_eq!(parse_jar_version(Paper, "Monolab.jar"), None);
    }

    #[test]
    fn reads_a_day_from_disk() {
        let server = std::env::temp_dir().join(format!(
            "monolab-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(event_folder(&server)).unwrap();
        fs::write(
            event_folder(&server).join("events-2026-08-23.jsonl"),
            concat!(
                r#"{"at":1,"kind":"join","player":"alex"}"#,
                "
",
                r#"{"at":2,"kind":"death","player":"friend","by":"ZOMBIE"}"#,
                "
",
                r#"{"at":3,"kind":"quit","player":"alex"}"#,
                "
{\"at\":4,\"kind\":\"jo",
            ),
        )
        .unwrap();

        let all = read_day(&server, "2026-08-23", &Filter::default());
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].at, 3);
        assert_eq!(all[2].at, 1);

        let only_death = read_day(
            &server,
            "2026-08-23",
            &Filter {
                kinds: vec!["death".into()],
                ..Filter::default()
            },
        );
        assert_eq!(only_death.len(), 1);

        let mine = read_day(
            &server,
            "2026-08-23",
            &Filter {
                player: "ALEX".into(),
                ..Filter::default()
            },
        );
        assert_eq!(mine.len(), 2);

        assert_eq!(event_days(&server), vec!["2026-08-23".to_string()]);
        assert!(read_day(&server, "../../secret", &Filter::default()).is_empty());
        fs::remove_dir_all(&server).ok();
    }

    #[test]
    fn reads_who_is_online() {
        let server = std::env::temp_dir().join(format!(
            "monolab-live-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(event_folder(&server)).unwrap();
        assert!(read_live(&server).is_none());

        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        fs::write(
            event_folder(&server).join("live.json"),
            format!(
                r#"{{"at":{at},"tps":19.98,"players":[{{"name":"alex","world":"world","x":1.5,"y":64.0,"z":2.5,"health":20,"food":18,"ping":31}}]}}"#
            ),
        )
        .unwrap();
        let live = read_live(&server).expect("should be readable");
        assert_eq!(live.at, at);
        assert_eq!(live.tps, Some(19.98));
        assert_eq!(live.memory_mib, None);
        assert_eq!(live.players[0].name, "alex");
        assert_eq!(live.players[0].food, 18);
        assert!(live.entity_kinds.is_empty() && live.hotspots.is_empty() && live.worlds.is_empty());

        fs::write(
            event_folder(&server).join("live.json"),
            format!(
                r#"{{"at":{at},"entities":3,"entityKinds":[{{"type":"minecraft:zombie","count":2}},{{"type":"minecraft:item"}}],"hotspots":[{{"world":"world","x":12,"z":-4,"count":120,"top":"minecraft:zombie"}}],"worlds":[{{"name":"world","chunks":600,"entities":3}}],"players":[]}}"#
            ),
        )
        .unwrap();
        let live = read_live(&server).expect("should be readable");
        assert_eq!(
            live.entity_kinds,
            vec![EntityKind { r#type: "minecraft:zombie".into(), count: 2 }]
        );
        assert_eq!(live.hotspots[0].x, 12);
        assert_eq!(live.hotspots[0].z, -4);
        assert_eq!(live.hotspots[0].top.as_deref(), Some("minecraft:zombie"));
        assert_eq!(live.worlds[0].chunks, 600);

        fs::write(
            event_folder(&server).join("live.json"),
            format!(r#"{{"at":{at},"players":[]}}"#),
        )
        .unwrap();
        let empty = read_live(&server).expect("should be readable");
        assert!(empty.players.is_empty());
        assert_eq!(empty.tps, None);

        fs::write(event_folder(&server).join("live.json"), r#"{"at":9,"pla"#).unwrap();
        assert!(read_live(&server).is_none());
        fs::remove_dir_all(&server).ok();
    }

    #[test]
    fn only_paper_and_its_forks() {
        assert_eq!(addon_kind("Paper"), Some(AddonKind::Paper));
        assert_eq!(addon_kind("Purpur"), Some(AddonKind::Paper));
        assert_eq!(addon_kind("NeoForge"), Some(AddonKind::NeoForge));
        assert_eq!(addon_kind("Fabric"), Some(AddonKind::Fabric));
        assert_eq!(addon_kind("Folia"), Some(AddonKind::Paper));
        assert_eq!(addon_kind("Forge"), None);
        assert_eq!(addon_kind("Spigot"), None);
        assert_eq!(addon_kind("Vanilla"), None);
        assert_eq!(addon_kind("Pumpkin"), Some(AddonKind::Pumpkin));
        assert_eq!(
            jar_name(AddonKind::Paper, "26.2", 5).as_deref(),
            Some("Monolab-5.jar")
        );
        assert_eq!(
            jar_name(AddonKind::Paper, "1.21.1", 5).as_deref(),
            Some("Monolab-5.jar")
        );
        assert_eq!(
            jar_name(AddonKind::NeoForge, "1.21.1", 5).as_deref(),
            Some("Monolab-mod-mc1_21_1-5.jar")
        );
        assert_eq!(
            jar_name(AddonKind::NeoForge, "26.2", 5).as_deref(),
            Some("Monolab-mod-mc26_2-5.jar")
        );
        assert_eq!(
            jar_name(AddonKind::NeoForge, "1.21.4", 5).as_deref(),
            Some("Monolab-mod-mc1_21_4-5.jar")
        );
        assert_eq!(
            jar_name(AddonKind::NeoForge, "1.21.8", 5).as_deref(),
            Some("Monolab-mod-mc1_21_8-5.jar")
        );
        assert_eq!(jar_name(AddonKind::NeoForge, "1.20.1", 5), None);
        assert_eq!(
            jar_name(AddonKind::Fabric, "1.21.1", 6).as_deref(),
            Some("Monolab-fabric-1.21.1-6.jar")
        );
        assert_eq!(
            jar_name(AddonKind::Fabric, "1.21.11", 6).as_deref(),
            Some("Monolab-fabric-1.21.11-6.jar")
        );
        assert_eq!(
            jar_name(AddonKind::Fabric, "26.2", 6).as_deref(),
            Some("Monolab-fabric-26.2-6.jar")
        );
        assert_eq!(jar_name(AddonKind::Fabric, "1.20.6", 6), None);
        assert_eq!(jar_name(AddonKind::Fabric, "26.3", 6), None);
        assert!(addon_folder(Path::new("s"), AddonKind::Paper).ends_with("plugins"));
        assert!(addon_folder(Path::new("s"), AddonKind::NeoForge).ends_with("mods"));
        assert!(addon_folder(Path::new("s"), AddonKind::Fabric).ends_with("mods"));
        assert_eq!(
            jar_name(AddonKind::Pumpkin, "0.1.0-dev+26.2-26.45", 12).as_deref(),
            Some("Monolab-pumpkin-26.45-12.wasm")
        );
        assert_eq!(jar_name(AddonKind::Pumpkin, "nightly", 12), None);
        assert!(addon_folder(Path::new("s"), AddonKind::Pumpkin).ends_with("plugins"));
    }

    #[test]
    fn keeps_pumpkin_records_where_the_sandbox_puts_them() {
        let server = std::env::temp_dir().join(format!(
            "se-pumpkin-addon-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let plugins = server.join("plugins");
        fs::create_dir_all(&plugins).unwrap();
        assert_eq!(event_folder(&server), server.join("plugins").join("Monolab"));
        fs::write(server.join("pumpkin.toml"), "seed = \"1\"
").unwrap();
        assert_eq!(
            event_folder(&server),
            server.join("plugins").join("data").join("monolab")
        );
        fs::write(plugins.join("Monolab-pumpkin-26.45-11.wasm.deactivated"), b"").unwrap();
        assert_eq!(installed_addon(&server, AddonKind::Pumpkin), None);
        fs::write(plugins.join("Monolab-pumpkin-26.45-12.wasm"), b"").unwrap();
        assert_eq!(
            installed_addon(&server, AddonKind::Pumpkin),
            Some(("Monolab-pumpkin-26.45-12.wasm".to_string(), 12))
        );
        assert_eq!(installed_addon(&server, AddonKind::Paper), None);
        write_timezone(&server).unwrap();
        let text = fs::read_to_string(event_folder(&server).join("timezone.json")).unwrap();
        let expected = chrono::Local::now().offset().local_minus_utc() / 60;
        assert_eq!(text.trim(), format!("{{\"utcOffsetMinutes\":{expected}}}"));
        let _ = fs::remove_dir_all(&server);
    }
}
