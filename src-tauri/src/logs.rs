
use flate2::read::GzDecoder;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read},
    path::Path,
};

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    pub file_name: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub compressed: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub time: String,
    pub kind: String,
    pub player: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DailySummary {
    pub date: String,
    pub file_name: String,
    pub joins: u32,
    pub chats: u32,
    pub deaths: u32,
    pub errors: u32,
    pub unique_players: u32,
    pub busiest_player: String,
}

pub fn list_files(instance_path: &Path) -> Vec<LogFile> {
    let folder = instance_path.join("logs");
    let Ok(entries) = fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut files = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".log") && !name.ends_with(".log.gz") {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            Some(LogFile {
                compressed: name.ends_with(".gz"),
                file_name: name,
                size_bytes: metadata.len(),
                modified_at: metadata
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_secs(),
            })
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        (right.file_name == "latest.log")
            .cmp(&(left.file_name == "latest.log"))
            .then_with(|| right.modified_at.cmp(&left.modified_at))
    });
    files
}

pub fn safe_log_path(instance_path: &Path, file_name: &str) -> Result<std::path::PathBuf, String> {
    let name = file_name.trim();
    if name.is_empty()
        || name.contains(['/', '\\'])
        || (!name.ends_with(".log") && !name.ends_with(".log.gz"))
    {
        return Err("The log file name is malformed.".into());
    }
    let path = crate::paths::resolve_in_server(instance_path, &format!("logs/{name}"))
        .map_err(|error| match error {
            crate::paths::PathError::Missing => "Log file not found.".to_string(),
            _ => "The log file name is malformed.".to_string(),
        })?;
    if !path.is_file() {
        return Err("Log file not found.".into());
    }
    Ok(path)
}

pub fn read_lines(path: &Path, limit: usize) -> Result<Vec<String>, String> {
    let file = fs::File::open(path).map_err(|error| format!("Could not open the log: {error}"))?;
    let reader: Box<dyn Read> = if path
        .to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".gz")
    {
        Box::new(GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut lines = std::collections::VecDeque::with_capacity(limit.min(4096));
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|error| format!("Problem while reading the log: {error}"))?;
        lines.push_back(line);
        if lines.len() > limit {
            lines.pop_front();
        }
    }
    Ok(lines.into())
}

fn time_of(line: &str) -> String {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('[') {
        let first = trimmed.split_whitespace().next().unwrap_or_default();
        if first.len() == 8 && first.matches(':').count() == 2 {
            return first.to_string();
        }
        return String::new();
    }
    let inside = &trimmed[1..];
    let Some(end) = inside.find(']') else {
        return String::new();
    };
    let head = &inside[..end];
    let stamp = head.split_whitespace().next().unwrap_or(head);
    if stamp.len() == 8 && stamp.matches(':').count() == 2 {
        stamp.to_string()
    } else {
        String::new()
    }
}

fn body_of(line: &str) -> &str {
    if let Some(body) = crate::pumpkin::body_of(line) {
        return body;
    }
    line.rsplit_once("]: ").map(|(_, rest)| rest).unwrap_or(line)
}

pub fn classify(line: &str) -> Option<TimelineEvent> {
    let body = body_of(line).trim();
    let time = time_of(line);
    let lower = body.to_ascii_lowercase();

    if let Some(rest) = body.strip_prefix('<') {
        if let Some((name, text)) = rest.split_once("> ") {
            return Some(TimelineEvent {
                time,
                kind: "Chat".into(),
                player: name.to_string(),
                text: text.to_string(),
            });
        }
    }
    if let Some(name) = body.strip_suffix(" joined the game") {
        return Some(TimelineEvent {
            time,
            kind: "Join".into(),
            player: name.to_string(),
            text: "joined the server.".into(),
        });
    }
    if let Some(name) = body.strip_suffix(" left the game") {
        return Some(TimelineEvent {
            time,
            kind: "Leave".into(),
            player: name.to_string(),
            text: "left the server.".into(),
        });
    }
    if let Some((name, rest)) = body.split_once(" issued server command: ") {
        return Some(TimelineEvent {
            time,
            kind: "Command".into(),
            player: name.to_string(),
            text: rest.to_string(),
        });
    }
    const DEATHS: &[&str] = &[
        " was slain by",
        " was shot by",
        " was blown up by",
        " drowned",
        " fell from a high place",
        " burned to death",
        " was killed by",
        " died",
        " starved to death",
        " hit the ground too hard",
        " went up in flames",
        " tried to swim in lava",
        " was pricked to death",
        " suffocated in a wall",
        " was squashed by",
    ];
    if let Some(found) = DEATHS.iter().find(|needle| lower.contains(*needle)) {
        let name = body.split_whitespace().next().unwrap_or_default();
        if !name.is_empty() && !name.contains('<') {
            return Some(TimelineEvent {
                time,
                kind: "Death".into(),
                player: name.to_string(),
                text: body.trim_start_matches(name).trim().to_string(),
                });
        }
        let _ = found;
    }
    let whole = line.to_ascii_lowercase();
    if whole.contains("[error]")
        || whole.contains("/error]")
        || whole.contains("/fatal]")
        || whole.contains(" severe]")
        || lower.contains("exception")
        || lower.contains("caused by:")
    {
        return Some(TimelineEvent {
            time,
            kind: "Error".into(),
            player: String::new(),
            text: body.chars().take(200).collect(),
        });
    }
    None
}

pub fn timeline(lines: &[String]) -> Vec<TimelineEvent> {
    lines.iter().filter_map(|line| classify(line)).collect()
}

fn date_of(file_name: &str) -> String {
    let parts = file_name.split('-').collect::<Vec<_>>();
    if parts.len() >= 3 {
        let candidate = format!("{}-{}-{}", parts[0], parts[1], parts[2]);
        if candidate.len() == 10 && candidate.chars().filter(char::is_ascii_digit).count() == 8 {
            return candidate;
        }
    }
    if file_name == "latest.log" {
        return "Today".into();
    }
    file_name.to_string()
}

pub fn summarize(file_name: &str, lines: &[String]) -> DailySummary {
    let mut joins = 0;
    let mut chats = 0;
    let mut deaths = 0;
    let mut errors = 0;
    let mut players: HashSet<String> = HashSet::new();
    let mut chat_counts: HashMap<String, u32> = HashMap::new();

    for event in timeline(lines) {
        match event.kind.as_str() {
            "Join" => {
                joins += 1;
                players.insert(event.player);
            }
            "Chat" => {
                chats += 1;
                *chat_counts.entry(event.player.clone()).or_default() += 1;
                players.insert(event.player);
            }
            "Death" => {
                deaths += 1;
                players.insert(event.player);
            }
            "Error" => errors += 1,
            _ => {}
        }
    }
    let busiest_player = chat_counts
        .into_iter()
        .max_by_key(|(name, count)| (*count, std::cmp::Reverse(name.clone())))
        .map(|(name, _)| name)
        .unwrap_or_default();

    DailySummary {
        date: date_of(file_name),
        file_name: file_name.to_string(),
        joins,
        chats,
        deaths,
        errors,
        unique_players: players.len() as u32,
        busiest_player,
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Masked {
    pub ips: u32,
    pub paths: u32,
    pub secrets: u32,
}

pub fn mask(text: &str) -> (String, Masked) {
    use regex::Regex;
    use std::sync::OnceLock;
    static IPV4: OnceLock<Regex> = OnceLock::new();
    static IPV6: OnceLock<Regex> = OnceLock::new();
    static HOME: OnceLock<Regex> = OnceLock::new();
    static SECRET: OnceLock<Regex> = OnceLock::new();
    let ipv4 = IPV4.get_or_init(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b([+\-.]?)").expect("ipv4"));
    let ipv6 = IPV6.get_or_init(|| {
        Regex::new(r"(?i)\b(?:[0-9a-f]{0,4}:){2,7}[0-9a-f]{0,4}\b").expect("ipv6")
    });
    let home = HOME.get_or_init(|| {
        Regex::new(r"(?i)([A-Z]:\\Users\\|/home/|/Users/)([^\\/\s\]\)]+)").expect("home")
    });
    let secret = SECRET.get_or_init(|| {
        Regex::new(r"(?i)\b(token|password|passwd|secret|api[_-]?key|webhook)(\s*[=:]\s*)([^\s,;\]\)]+)").expect("secret")
    });

    let mut count = Masked::default();
    let text = secret.replace_all(text, |caps: &regex::Captures| {
        count.secrets += 1;
        format!("{}{}[REDACTED]", &caps[1], &caps[2])
    });
    let text = home.replace_all(&text, |caps: &regex::Captures| {
        count.paths += 1;
        format!("{}[USER]", &caps[1])
    });
    let text = ipv4.replace_all(&text, |caps: &regex::Captures| {
        let whole = &caps[0];
        let tail = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let address = &whole[..whole.len() - tail.len()];
        if !tail.is_empty()
            || address.starts_with("127.")
            || address == "0.0.0.0"
            || address.split('.').any(|part| part.parse::<u16>().map(|n| n > 255).unwrap_or(true))
        {
            return whole.to_string();
        }
        count.ips += 1;
        "[IP]".to_string()
    });
    let text = ipv6.replace_all(&text, |caps: &regex::Captures| {
        let whole = &caps[0];
        let looks_like_address =
            whole.contains("::") || whole.chars().any(|letter| letter.is_ascii_alphabetic());
        if whole == "::1" || whole == "::" || !looks_like_address {
            return whole.to_string();
        }
        count.ips += 1;
        "[IP]".to_string()
    });
    (text.into_owned(), count)
}

#[cfg(test)]
mod tests {
    use super::{classify, date_of, mask, summarize, time_of};

    #[test]
    fn hides_what_should_not_leave_the_house() {
        let text = [
            "[12:00:01] [Server thread/INFO]: Steve[/203.0.113.7:51234] logged in with entity id 12",
            "[12:00:02] [Server thread/INFO]: Alex[/2001:db8:85a3::8a2e:370:7334:51235] logged in",
            "[12:00:03] [Server thread/INFO]: Starting Minecraft server on *:25565",
            r"[12:00:04] [Server thread/WARN]: at C:\Users\alex\AppData\Roaming\Monolab\servers\server_1\plugins\x.jar",
            "[12:00:05] [Server thread/INFO]: [Discord] webhook=https://discord.com/api/webhooks/1/abc token=xyz.123 password: hunter2",
            "[12:00:06] [Server thread/INFO]: Loading Paper 26.2-123 for Minecraft 26.2 on 127.0.0.1",
            "[12:00:07] [ServerMain/INFO]: [bootstrap] Running Java 25 (OpenJDK 64-Bit Server VM 25.0.4.1+1-LTS)",
        ]
        .join("\n");
        let (out, count) = mask(&text);
        assert!(out.contains("Steve[/[IP]:51234]"), "{out}");
        assert!(out.contains("Alex[/[IP]"), "{out}");
        assert!(out.contains("*:25565"), "ports stay: {out}");
        assert!(out.contains(r"C:\Users\[USER]\AppData"), "{out}");
        assert!(out.contains("webhook=[REDACTED]") && out.contains("token=[REDACTED]") && out.contains("password: [REDACTED]"), "{out}");
        assert!(out.contains("26.2-123") && out.contains("127.0.0.1") && out.contains("25.0.4.1+1-LTS"), "version numbers and own address stay: {out}");
        assert_eq!((count.ips, count.paths, count.secrets), (2, 1, 3));
    }

    #[test]
    fn reads_the_timestamp_from_the_line_head() {
        assert_eq!(time_of("[13:45:22] [Server thread/INFO]: hi"), "13:45:22");
        assert_eq!(time_of("no brackets here"), "");
    }

    #[test]
    fn understands_joins_and_leaves() {
        let join = classify("[13:45:22] [Server thread/INFO]: Steve joined the game").unwrap();
        assert_eq!(join.kind, "Join");
        assert_eq!(join.player, "Steve");
        let leave = classify("[13:50:00] [Server thread/INFO]: Steve left the game").unwrap();
        assert_eq!(leave.kind, "Leave");
    }

    #[test]
    fn understands_chat_and_commands() {
        let chat = classify("[13:46:00] [Server thread/INFO]: <Alex> hello there").unwrap();
        assert_eq!(chat.kind, "Chat");
        assert_eq!(chat.player, "Alex");
        assert_eq!(chat.text, "hello there");
        let command =
            classify("[13:47:00] [Server thread/INFO]: Alex issued server command: /gamemode creative")
                .unwrap();
        assert_eq!(command.kind, "Command");
        assert_eq!(command.text, "/gamemode creative");
    }

    #[test]
    fn understands_deaths_and_errors() {
        let death =
            classify("[13:48:00] [Server thread/INFO]: Steve was slain by Zombie").unwrap();
        assert_eq!(death.kind, "Death");
        assert_eq!(death.player, "Steve");
        let error =
            classify("[13:49:00] [Server thread/ERROR]: java.lang.NullPointerException").unwrap();
        assert_eq!(error.kind, "Error");
    }

    #[test]
    fn ignores_ordinary_lines() {
        assert!(classify("[13:45:00] [Server thread/INFO]: Preparing spawn area: 42%").is_none());
        let pumpkin = classify("[INFO] 5657 joined the game").unwrap();
        assert_eq!(pumpkin.player, "5657");
        assert_eq!(pumpkin.kind, "Join");
        let pumpkin = classify("19:33:00  INFO <5657> hello").unwrap();
        assert_eq!(pumpkin.time, "19:33:00");
        assert_eq!(pumpkin.player, "5657");
    }

    #[test]
    fn pulls_the_date_out_of_a_rotated_file_name() {
        assert_eq!(date_of("2026-08-21-1.log.gz"), "2026-08-21");
        assert_eq!(date_of("latest.log"), "Today");
    }

    #[test]
    fn counts_a_day_at_a_glance() {
        let lines = vec![
            "[13:45:22] [Server thread/INFO]: Steve joined the game".to_string(),
            "[13:46:00] [Server thread/INFO]: <Steve> hello".to_string(),
            "[13:46:10] [Server thread/INFO]: <Steve> anyone there".to_string(),
            "[13:47:00] [Server thread/INFO]: Alex joined the game".to_string(),
            "[13:48:00] [Server thread/INFO]: Steve was slain by Zombie".to_string(),
            "[13:49:00] [Server thread/ERROR]: something exploded".to_string(),
        ];
        let summary = summarize("2026-08-21-1.log.gz", &lines);
        assert_eq!(summary.date, "2026-08-21");
        assert_eq!(summary.joins, 2);
        assert_eq!(summary.chats, 2);
        assert_eq!(summary.deaths, 1);
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.unique_players, 2);
        assert_eq!(summary.busiest_player, "Steve");
    }
}
