//! What the Players tab's info window shows about one player: play time and other numbers from the
//! world's statistics file, join and leave times from the log, and the files that hold their data.

use serde::Serialize;

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStat {
    /// The statistic's id without "minecraft:", e.g. "deaths". The app names it.
    pub key: String,
    pub value: f64,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSession {
    /// "HH:MM:SS" as the log wrote it.
    pub joined: String,
    /// Empty while they are still on, or when the log ended first.
    pub left: String,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerDataFile {
    /// Path inside the server folder, with forward slashes.
    pub path: String,
    /// "data", "stats", "advancements", or the plugin's folder name.
    pub kind: String,
    pub size: u64,
    pub modified_at: u64,
}

#[derive(Clone, Debug, Serialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayerInfo {
    pub uuid: String,
    pub play_seconds: Option<f64>,
    pub stats: Vec<PlayerStat>,
    pub advancements: Option<u32>,
    pub sessions: Vec<PlayerSession>,
    pub chats: u32,
    pub commands: u32,
    pub files: Vec<PlayerDataFile>,
    /// Newest change to any of their files: when the server last saved them.
    pub last_saved_at: u64,
}

/// Custom statistics worth showing, in this order.
const SHOWN: [&str; 10] = [
    "deaths",
    "mob_kills",
    "player_kills",
    "damage_dealt",
    "damage_taken",
    "jump",
    "walk_one_cm",
    "sprint_one_cm",
    "fly_one_cm",
    "leave_game",
];

/// Play time and the shown numbers from a stats JSON file. Totals of blocks mined and items crafted
/// come along as "mined" and "crafted".
pub fn read_stats(text: &str) -> (Option<f64>, Vec<PlayerStat>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return (None, Vec::new()) };
    let stats = &value["stats"];
    let custom = &stats["minecraft:custom"];
    let number = |key: &str| custom.get(format!("minecraft:{key}")).and_then(|one| one.as_f64());
    // 1.17 renamed play_one_minute (which counted ticks all along) to play_time.
    let play = number("play_time").or_else(|| number("play_one_minute")).map(|ticks| ticks / 20.0);
    let mut out: Vec<PlayerStat> = SHOWN.iter().filter_map(|key| number(key).map(|value| PlayerStat { key: (*key).into(), value })).collect();
    let total = |group: &str| stats.get(format!("minecraft:{group}")).and_then(|one| one.as_object()).map(|all| all.values().filter_map(|one| one.as_f64()).sum::<f64>());
    if let Some(value) = total("mined").filter(|one| *one > 0.0) {
        out.push(PlayerStat { key: "mined".into(), value });
    }
    if let Some(value) = total("crafted").filter(|one| *one > 0.0) {
        out.push(PlayerStat { key: "crafted".into(), value });
    }
    (play, out)
}

/// How many advancements (not recipes) are done in an advancements JSON file.
pub fn count_advancements(text: &str) -> Option<u32> {
    let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let all = value.as_object()?;
    Some(
        all.iter()
            .filter(|(key, one)| !key.contains(":recipes/") && one.get("done").and_then(|done| done.as_bool()) == Some(true))
            .count() as u32,
    )
}

fn time_of(line: &str) -> String {
    line.strip_prefix('[').and_then(|rest| rest.get(..8)).filter(|time| time.as_bytes().get(2) == Some(&b':')).unwrap_or("").to_string()
}

/// Joins, leaves, chat lines and commands of one player in a log.
pub fn read_log(text: &str, name: &str) -> (Vec<PlayerSession>, u32, u32) {
    let mut sessions: Vec<PlayerSession> = Vec::new();
    let (mut chats, mut commands) = (0, 0);
    let joined = format!("]: {name} joined the game");
    let left = format!("]: {name} left the game");
    let chat = format!("]: <{name}> ");
    let issued = format!("]: {name} issued server command: ");
    for line in text.lines() {
        if line.ends_with(&joined) {
            sessions.push(PlayerSession { joined: time_of(line), left: String::new() });
        } else if line.ends_with(&left) {
            match sessions.last_mut() {
                Some(open) if open.left.is_empty() => open.left = time_of(line),
                _ => sessions.push(PlayerSession { joined: String::new(), left: time_of(line) }),
            }
        } else if line.contains(&chat) {
            chats += 1;
        } else if line.contains(&issued) {
            commands += 1;
        }
    }
    (sessions, chats, commands)
}

/// Whether a plugin file name is about this player (with or without the dashes in the UUID, or by name).
pub fn names_player(file_name: &str, uuid: &str, name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(stem, _)| stem);
    let uuid = uuid.to_ascii_lowercase();
    stem == uuid || stem == uuid.replace('-', "") || (!name.is_empty() && stem == name.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_play_time_and_numbers() {
        let text = r#"{"stats":{"minecraft:custom":{"minecraft:play_time":72000,"minecraft:deaths":3,"minecraft:jump":10},
            "minecraft:mined":{"minecraft:stone":5,"minecraft:dirt":2}},"DataVersion":4000}"#;
        let (play, stats) = read_stats(text);
        assert_eq!(play, Some(3600.0));
        assert_eq!(stats[0], PlayerStat { key: "deaths".into(), value: 3.0 });
        assert!(stats.contains(&PlayerStat { key: "mined".into(), value: 7.0 }));
        let old = r#"{"stats":{"minecraft:custom":{"minecraft:play_one_minute":40}}}"#;
        assert_eq!(read_stats(old).0, Some(2.0));
        assert_eq!(read_stats("not json").0, None);
    }

    #[test]
    fn counts_advancements_without_recipes() {
        let text = r#"{"minecraft:story/root":{"done":true},"minecraft:recipes/misc/x":{"done":true},"minecraft:story/mine_stone":{"done":false}}"#;
        assert_eq!(count_advancements(text), Some(1));
    }

    #[test]
    fn reads_sessions_chat_and_commands() {
        let log = "[12:05:10] [Server thread/INFO]: mina_k joined the game\n\
                   [12:06:30] [Async Chat Thread - #0/INFO]: <mina_k> hi\n\
                   [12:06:40] [Server thread/INFO]: mina_k issued server command: /home\n\
                   [12:07:00] [Server thread/INFO]: mina_k left the game\n\
                   [12:09:00] [Server thread/INFO]: mina_k joined the game\n\
                   [12:09:01] [Server thread/INFO]: other joined the game";
        let (sessions, chats, commands) = read_log(log, "mina_k");
        assert_eq!(sessions, vec![
            PlayerSession { joined: "12:05:10".into(), left: "12:07:00".into() },
            PlayerSession { joined: "12:09:00".into(), left: String::new() },
        ]);
        assert_eq!((chats, commands), (1, 1));
    }

    #[test]
    fn matches_plugin_files_by_uuid_or_name() {
        let uuid = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
        assert!(names_player("069a79f4-44e9-4726-a5be-fca90e38aaf5.yml", uuid, "Notch"));
        assert!(names_player("069A79F444E94726A5BEFCA90E38AAF5.json", uuid, "Notch"));
        assert!(names_player("notch.yml", uuid, "Notch"));
        assert!(!names_player("config.yml", uuid, "Notch"));
    }
}
