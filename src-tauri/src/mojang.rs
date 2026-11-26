
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{imageops, GenericImageView, ImageFormat};
use serde::{Deserialize, Serialize};
use std::{fs, io::Cursor, path::Path, time::Duration};

const PROFILE_TTL_SECONDS: u64 = 60 * 60 * 24 * 7;

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MojangProfile {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub head: String,
    #[serde(default)]
    pub fetched_at: u64,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn client() -> Result<reqwest::blocking::Client, String> {
    crate::net::builder()
        .user_agent("Monolab/0.1.0 (local Windows desktop app)")
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())
}

fn cache_file(cache_dir: &Path, key: &str) -> std::path::PathBuf {
    let safe = key
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    cache_dir.join(format!("{safe}.json"))
}

fn read_cache(cache_dir: &Path, key: &str) -> Option<MojangProfile> {
    let text = fs::read_to_string(cache_file(cache_dir, key)).ok()?;
    let profile: MojangProfile = serde_json::from_str(&text).ok()?;
    (now().saturating_sub(profile.fetched_at) < PROFILE_TTL_SECONDS).then_some(profile)
}

fn write_cache(cache_dir: &Path, key: &str, profile: &MojangProfile) {
    let _ = fs::create_dir_all(cache_dir);
    let _ = serde_json::to_string(profile)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            fs::write(cache_file(cache_dir, key), text).map_err(|error| error.to_string())
        });
}

fn dashed_uuid(raw: &str) -> String {
    let clean = raw.replace('-', "");
    if clean.len() != 32 {
        return raw.to_string();
    }
    format!(
        "{}-{}-{}-{}-{}",
        &clean[0..8],
        &clean[8..12],
        &clean[12..16],
        &clean[16..20],
        &clean[20..32]
    )
}

fn head_icon(skin: &[u8]) -> Result<String, String> {
    let image = image::load_from_memory(skin)
        .map_err(|error| format!("Could not read the skin image: {error}"))?;
    let (width, height) = image.dimensions();
    if width < 64 || height < 32 {
        return Err("The skin image has an unexpected size.".into());
    }
    let mut head = image.view(8, 8, 8, 8).to_image();
    let overlay = image.view(40, 8, 8, 8).to_image();
    imageops::overlay(&mut head, &overlay, 0, 0);
    let scaled = imageops::resize(&head, 64, 64, imageops::FilterType::Nearest);
    let mut buffer = Cursor::new(Vec::new());
    scaled
        .write_to(&mut buffer, ImageFormat::Png)
        .map_err(|error| format!("Could not create the head icon: {error}"))?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(buffer.into_inner())
    ))
}

fn fetch_head(client: &reqwest::blocking::Client, uuid: &str) -> Option<String> {
    let session = format!(
        "https://sessionserver.mojang.com/session/minecraft/profile/{}",
        uuid.replace('-', "")
    );
    let profile: serde_json::Value = client.get(session).send().ok()?.json().ok()?;
    let encoded = profile
        .get("properties")?
        .as_array()?
        .iter()
        .find(|property| property.get("name").and_then(|n| n.as_str()) == Some("textures"))?
        .get("value")?
        .as_str()?;
    let decoded = STANDARD.decode(encoded).ok()?;
    let textures: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let skin_url = textures.pointer("/textures/SKIN/url")?.as_str()?;
    let bytes = client.get(skin_url).send().ok()?.bytes().ok()?;
    head_icon(&bytes).ok()
}

pub fn lookup_by_name(cache_dir: &Path, name: &str) -> Result<MojangProfile, String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 16 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err("Minecraft names are up to 16 letters, digits or underscores.".into());
    }
    if let Some(cached) = read_cache(cache_dir, &format!("name-{name}")) {
        return Ok(cached);
    }
    let client = client()?;
    let response = client
        .get(format!(
            "https://api.mojang.com/users/profiles/minecraft/{name}"
        ))
        .send()
        .map_err(|error| format!("Could not reach Mojang: {error}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("No Minecraft account named {name} was found."));
    }
    let value: serde_json::Value = response
        .error_for_status()
        .map_err(|error| format!("Unexpected Mojang response: {error}"))?
        .json()
        .map_err(|error| error.to_string())?;
    let raw_id = value
        .get("id")
        .and_then(|id| id.as_str())
        .ok_or("Mojang's response has no UUID.")?;
    let profile = MojangProfile {
        uuid: dashed_uuid(raw_id),
        name: value
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or(name)
            .to_string(),
        head: fetch_head(&client, raw_id).unwrap_or_default(),
        fetched_at: now(),
    };
    write_cache(cache_dir, &format!("name-{name}"), &profile);
    write_cache(cache_dir, &format!("uuid-{}", profile.uuid), &profile);
    Ok(profile)
}

pub fn head_for_uuid(cache_dir: &Path, uuid: &str) -> Option<MojangProfile> {
    let uuid = dashed_uuid(uuid.trim());
    if uuid.len() != 36 {
        return None;
    }
    if let Some(cached) = read_cache(cache_dir, &format!("uuid-{uuid}")) {
        return Some(cached);
    }
    let client = client().ok()?;
    let head = fetch_head(&client, &uuid)?;
    let profile = MojangProfile {
        uuid: uuid.clone(),
        name: String::new(),
        head,
        fetched_at: now(),
    };
    write_cache(cache_dir, &format!("uuid-{uuid}"), &profile);
    Some(profile)
}

#[cfg(test)]
mod tests {
    use super::dashed_uuid;

    #[test]
    fn adds_dashes_to_a_bare_uuid() {
        assert_eq!(
            dashed_uuid("069a79f444e94726a5befca90e38aaf5"),
            "069a79f4-44e9-4726-a5be-fca90e38aaf5"
        );
    }

    #[test]
    fn leaves_other_values_alone() {
        assert_eq!(dashed_uuid("not-a-uuid"), "not-a-uuid");
        let already = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
        assert_eq!(dashed_uuid(already), already);
    }
}
