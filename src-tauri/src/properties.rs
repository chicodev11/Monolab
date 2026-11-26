
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ValueKind {
    Toggle,
    Number,
    Choice,
    Text,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PropertyInfo {
    pub key: String,
    pub value: String,
    pub kind: ValueKind,
    pub options: Vec<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub common: bool,
}

struct Spec {
    key: &'static str,
    kind: ValueKind,
    options: &'static [&'static str],
    min: Option<i64>,
    max: Option<i64>,
    common: bool,
}

const fn spec(
    key: &'static str,
    kind: ValueKind,
    options: &'static [&'static str],
    min: Option<i64>,
    max: Option<i64>,
    common: bool,
) -> Spec {
    Spec {
        key,
        kind,
        options,
        min,
        max,
        common,
    }
}

const DIFFICULTIES: &[&str] = &["peaceful", "easy", "normal", "hard"];
const GAMEMODES: &[&str] = &["survival", "creative", "adventure", "spectator"];
const LEVEL_TYPES: &[&str] = &[
    "minecraft:normal",
    "minecraft:flat",
    "minecraft:large_biomes",
    "minecraft:amplified",
    "minecraft:single_biome_surface",
];

static SPECS: &[Spec] = &[
    spec("motd", ValueKind::Text, &[], None, None, true),
    spec("server-port", ValueKind::Number, &[], Some(1), Some(65535), true),
    spec("max-players", ValueKind::Number, &[], Some(1), Some(10_000), true),
    spec("difficulty", ValueKind::Choice, DIFFICULTIES, None, None, true),
    spec("gamemode", ValueKind::Choice, GAMEMODES, None, None, true),
    spec("hardcore", ValueKind::Toggle, &[], None, None, true),
    spec("pvp", ValueKind::Toggle, &[], None, None, true),
    spec("online-mode", ValueKind::Toggle, &[], None, None, true),
    spec("white-list", ValueKind::Toggle, &[], None, None, true),
    spec("enforce-whitelist", ValueKind::Toggle, &[], None, None, false),
    spec("view-distance", ValueKind::Number, &[], Some(2), Some(32), true),
    spec("simulation-distance", ValueKind::Number, &[], Some(2), Some(32), true),
    spec("level-name", ValueKind::Text, &[], None, None, true),
    spec("level-seed", ValueKind::Text, &[], None, None, true),
    spec("level-type", ValueKind::Choice, LEVEL_TYPES, None, None, false),
    spec("spawn-protection", ValueKind::Number, &[], Some(0), Some(1000), true),
    spec("allow-nether", ValueKind::Toggle, &[], None, None, false),
    spec("allow-flight", ValueKind::Toggle, &[], None, None, false),
    spec("enable-command-block", ValueKind::Toggle, &[], None, None, false),
    spec("force-gamemode", ValueKind::Toggle, &[], None, None, false),
    spec("spawn-monsters", ValueKind::Toggle, &[], None, None, false),
    spec("player-idle-timeout", ValueKind::Number, &[], Some(0), Some(1440), false),
    spec("max-world-size", ValueKind::Number, &[], Some(1), Some(29_999_984), false),
    spec("enable-rcon", ValueKind::Toggle, &[], None, None, true),
    spec("rcon.port", ValueKind::Number, &[], Some(1), Some(65535), false),
    spec("rcon.password", ValueKind::Text, &[], None, None, false),
    spec("enable-query", ValueKind::Toggle, &[], None, None, false),
    spec("query.port", ValueKind::Number, &[], Some(1), Some(65535), false),
    spec("server-ip", ValueKind::Text, &[], None, None, false),
    spec("resource-pack", ValueKind::Text, &[], None, None, false),
    spec("require-resource-pack", ValueKind::Toggle, &[], None, None, false),
    spec("enforce-secure-profile", ValueKind::Toggle, &[], None, None, false),
    spec("network-compression-threshold", ValueKind::Number, &[], Some(-1), Some(1_000_000), false),
    spec("max-tick-time", ValueKind::Number, &[], Some(-1), Some(600_000), false),
    spec("sync-chunk-writes", ValueKind::Toggle, &[], None, None, false),
    spec("hide-online-players", ValueKind::Toggle, &[], None, None, false),
    spec("generate-structures", ValueKind::Toggle, &[], None, None, false),
    spec("op-permission-level", ValueKind::Number, &[], Some(1), Some(4), false),
    spec("function-permission-level", ValueKind::Number, &[], Some(1), Some(4), false),
    spec("entity-broadcast-range-percentage", ValueKind::Number, &[], Some(10), Some(1000), false),
];

fn find(key: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|spec| spec.key == key)
}

pub fn describe(key: &str, value: &str) -> PropertyInfo {
    match find(key) {
        Some(spec) => PropertyInfo {
            key: key.to_string(),
            value: value.to_string(),
            kind: spec.kind,
            options: spec.options.iter().map(|item| item.to_string()).collect(),
            min: spec.min,
            max: spec.max,
            common: spec.common,
        },
        None => PropertyInfo {
            key: key.to_string(),
            value: value.to_string(),
            kind: if value == "true" || value == "false" {
                ValueKind::Toggle
            } else {
                ValueKind::Text
            },
            options: Vec::new(),
            min: None,
            max: None,
            common: false,
        },
    }
}

pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(one) = chars.next() {
        if one != '\\' {
            out.push(one);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('f') => out.push('\u{000C}'),
            Some('u') => {
                let hex: String = chars.clone().take(4).collect();
                match u32::from_str_radix(&hex, 16)
                    .ok()
                    .filter(|_| hex.len() == 4)
                    .and_then(char::from_u32)
                {
                    Some(found) => {
                        out.push(found);
                        for _ in 0..4 {
                            chars.next();
                        }
                    }
                    None => out.push('u'),
                }
            }
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for one in value.chars() {
        match one {
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            ':' => out.push_str(r"\:"),
            '=' => out.push_str(r"\="),
            other => out.push(other),
        }
    }
    out
}

pub fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                return None;
            }
            let (key, value) = line.split_once('=').or_else(|| line.split_once(':'))?;
            Some((key.trim().to_string(), unescape(value.trim())))
        })
        .collect()
}

pub fn needs_check(before: &[(String, String)], key: &str, value: &str) -> bool {
    !before
        .iter()
        .any(|(old_key, old_value)| old_key == key && old_value == value)
}

pub fn validate(key: &str, value: &str) -> Result<(), String> {
    let Some(spec) = find(key) else {
        return Ok(());
    };
    match spec.kind {
        ValueKind::Toggle => {
            if value == "true" || value == "false" {
                Ok(())
            } else {
                Err(format!("{key} must be true or false."))
            }
        }
        ValueKind::Number => {
            let Ok(number) = value.trim().parse::<i64>() else {
                return Err(format!("{key} must be a number. Current value: {value}"));
            };
            if let Some(min) = spec.min {
                if number < min {
                    return Err(format!("{key} must be at least {min}."));
                }
            }
            if let Some(max) = spec.max {
                if number > max {
                    return Err(format!("{key} must be at most {max}."));
                }
            }
            Ok(())
        }
        ValueKind::Choice => {
            if spec.options.contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "{key} must be one of {}.",
                    spec.options.join(", ")
                ))
            }
        }
        ValueKind::Text => {
            if value.contains(['\n', '\r']) {
                Err(format!("{key} cannot contain line breaks."))
            } else {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{describe, escape, needs_check, parse, unescape, validate, ValueKind};

    #[test]
    fn accepts_the_level_type_a_real_server_wrote() {
        let raw = r"minecraft\:normal";
        assert!(validate("level-type", raw).is_err());
        assert_eq!(unescape(raw), "minecraft:normal");
        assert!(validate("level-type", &unescape(raw)).is_ok());
    }

    #[test]
    fn puts_the_backslashes_back_when_saving() {
        for line in [
            r"minecraft\:normal",
            r"https\://example.com/pack.zip",
            r"hunter2",
            r"tab\there",
            r"back\\slash",
            r"a\=b",
        ] {
            assert_eq!(escape(&unescape(line)), line, "{line}");
        }
    }

    #[test]
    fn only_checks_what_you_changed() {
        let before = parse("level-type=minecraft\\:normal\nold=default\nserver-port=25565\n");
        assert_eq!(before[0].1, "minecraft:normal");
        assert!(!needs_check(&before, "level-type", "minecraft:normal"));
        assert!(!needs_check(&before, "old", "default"));
        assert!(needs_check(&before, "server-port", "25566"));
    }

    #[test]
    fn unescapes_what_java_writes() {
        assert_eq!(unescape(concat!("\\", "u00a7", "aHello")), "§aHello");
        assert_eq!(unescape("Our server"), "Our server");
        assert_eq!(unescape(r"a\nb"), "a\nb");
        assert_eq!(unescape(r"\u12"), "u12");
        assert_eq!(unescape("end\\"), "end\\");
        assert_eq!(escape("Our server"), "Our server");
    }

    #[test]
    #[ignore]
    fn saves_a_real_file_untouched() {
        let root = std::path::Path::new(r"C:\Users\alex\AppData\Roaming\Monolab\servers");
        let mut checked = 0usize;
        for entry in std::fs::read_dir(root).expect("could not open the server folder") {
            let Ok(entry) = entry else { continue };
            let path = entry.path().join("server.properties");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            checked += 1;
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let (key, value) = (key.trim(), unescape(value.trim()));
                if let Err(reason) = validate(key, &value) {
                    panic!("{}: {reason}", path.display());
                }
                assert_eq!(escape(&value), line.split_once('=').unwrap().1.trim(), "{key}");
            }
        }
        println!("server.properties of {checked} servers can be saved unchanged.");
        assert!(checked > 0, "Nothing to show.");
    }

    #[test]
    fn knows_the_shape_of_known_keys() {
        let info = describe("view-distance", "10");
        assert_eq!(info.kind, ValueKind::Number);
        assert_eq!(info.min, Some(2));
        assert!(info.common);
    }

    #[test]
    fn guesses_toggles_for_unknown_keys() {
        assert_eq!(describe("some-mod-option", "true").kind, ValueKind::Toggle);
        assert_eq!(describe("some-mod-option", "banana").kind, ValueKind::Text);
    }

    #[test]
    fn rejects_text_in_a_number_field() {
        let error = validate("view-distance", "abc").unwrap_err();
        assert!(error.contains("number"));
    }

    #[test]
    fn rejects_numbers_outside_the_allowed_range() {
        assert!(validate("view-distance", "1").is_err());
        assert!(validate("view-distance", "33").is_err());
        assert!(validate("view-distance", "12").is_ok());
    }

    #[test]
    fn rejects_unknown_choices() {
        assert!(validate("difficulty", "impossible").is_err());
        assert!(validate("difficulty", "hard").is_ok());
    }

    #[test]
    fn only_accepts_true_or_false_for_toggles() {
        assert!(validate("pvp", "yes").is_err());
        assert!(validate("pvp", "false").is_ok());
    }

    #[test]
    fn leaves_unknown_keys_alone() {
        assert!(validate("some-mod-option", "anything at all").is_ok());
    }
}
