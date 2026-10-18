use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChunkyProgress {
    pub world: String,
    pub state: String,
    pub percent: f64,
    pub chunks: u64,
    pub eta: String,
    pub rate: f64,
    pub at: u64,
}

fn after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(key)? + key.len();
    Some(line[at..].trim_start())
}

fn number(text: &str) -> Option<f64> {
    let digits: String = text.chars().take_while(|one| one.is_ascii_digit() || *one == '.' || *one == ',').filter(|one| *one != ',').collect();
    digits.parse().ok()
}

fn world_of(line: &str, verb: &str) -> Option<String> {
    let word = after(line, verb)?.split_whitespace().next()?;
    Some(word.trim_end_matches(['.', ',']).to_string())
}

/// Reads one Chunky console line, if it is a progress or state line.
pub fn read_line(line: &str, now: u64) -> Option<ChunkyProgress> {
    if !line.contains("Chunky") && !line.contains("Task ") {
        return None;
    }
    let states = [
        ("Task running for ", "running"),
        ("Task finished for ", "finished"),
        ("Task started for ", "running"),
        ("Task paused for ", "paused"),
        ("Task stopped for ", "paused"),
        ("Task cancelled for ", "cancelled"),
    ];
    let (verb, state) = states.iter().find(|(verb, _)| line.contains(verb))?;
    let world = world_of(line, verb)?;
    let mut out = ChunkyProgress { world, state: (*state).to_string(), at: now, ..Default::default() };
    if let Some(rest) = after(line, "Processed:") {
        out.chunks = number(rest).unwrap_or(0.0) as u64;
    }
    if let Some(open) = line.find("chunks (") {
        out.percent = number(&line[open + 8..]).unwrap_or(0.0);
    }
    if *state == "finished" {
        out.percent = 100.0;
    }
    if let Some(rest) = after(line, "ETA:") {
        out.eta = rest.split(',').next().unwrap_or("").trim().to_string();
    }
    if let Some(rest) = after(line, "Rate:") {
        out.rate = number(rest).unwrap_or(0.0);
    }
    Some(out)
}

/// The world name Chunky expects for a dimension.
pub fn world_name(core: &str, level: &str, realm: &str) -> String {
    let bukkit = matches!(core, "Paper" | "Purpur" | "Folia" | "Spigot" | "Bukkit");
    match (bukkit, realm) {
        (true, "nether") => format!("{level}_nether"),
        (true, "end") => format!("{level}_the_end"),
        (true, _) => level.to_string(),
        (false, "nether") => "minecraft:the_nether".into(),
        (false, "end") => "minecraft:the_end".into(),
        (false, _) => "minecraft:overworld".into(),
    }
}

pub fn is_chunky_jar(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("chunky") && !lower.starts_with("chunkyborder") && (lower.ends_with(".jar"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_chunky_progress_lines() {
        let line = "[12:00:01 INFO]: [Chunky] Task running for world. Processed: 2,304 chunks (14.40%), ETA: 0:00:59, Rate: 214.8 cps, Current: -13, 6";
        let one = read_line(line, 5).unwrap();
        assert_eq!(one.world, "world");
        assert_eq!(one.state, "running");
        assert_eq!(one.chunks, 2304);
        assert!((one.percent - 14.4).abs() < 0.001);
        assert_eq!(one.eta, "0:00:59");
        assert!((one.rate - 214.8).abs() < 0.001);
        let done = read_line("[Chunky] Task finished for minecraft:the_nether. Processed: 16000 chunks (100.00%), Total time: 0:01:14", 6).unwrap();
        assert_eq!(done.world, "minecraft:the_nether");
        assert_eq!(done.state, "finished");
        assert_eq!(done.percent, 100.0);
        assert_eq!(read_line("[Chunky] Task paused for world_nether.", 7).unwrap().state, "paused");
        assert_eq!(read_line("[Chunky] Task cancelled for world.", 7).unwrap().world, "world");
        assert!(read_line("[12:00:01 INFO]: Done (3.2s)!", 1).is_none());
    }

    #[test]
    fn names_worlds_per_core() {
        assert_eq!(world_name("Paper", "world", "nether"), "world_nether");
        assert_eq!(world_name("Fabric", "world", "end"), "minecraft:the_end");
        assert!(is_chunky_jar("Chunky-Bukkit-1.4.40.jar"));
        assert!(!is_chunky_jar("ChunkyBorder-1.2.jar"));
        assert!(!is_chunky_jar("Chunky-1.4.jar.disabled"));
    }
}
