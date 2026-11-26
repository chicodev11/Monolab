//! Backup copies to S3-compatible storage (AWS S3, Backblaze B2, Cloudflare R2, MinIO). One signed PUT per
//! backup (AWS Signature V4, path-style addresses), so files above 5 GB are refused. Keys are sealed in
//! `cloud-backup.json`; the screen only sees the endpoint, bucket and whether a secret is set.

use crate::data_root::MonolabDirs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, path::PathBuf, time::Duration};
use tauri::{AppHandle, Manager};

const MOST_BYTES: u64 = 5 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    key_id: String,
    secret: String,
    on: bool,
    #[serde(default)]
    last: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CloudInfo {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub key_id: String,
    pub has_secret: bool,
    pub on: bool,
    /// What happened to the last copy ("" before the first).
    pub last: String,
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join("cloud-backup.json"))
}

fn load(app: &AppHandle) -> Saved {
    path(app).ok().and_then(|path| crate::store::read_secret(&path).ok().flatten()).unwrap_or_default()
}

fn keep(app: &AppHandle, saved: &Saved) -> Result<(), String> {
    let text = serde_json::to_string_pretty(saved).map_err(|error| error.to_string())?;
    crate::store::write_secret(&path(app)?, &text).map_err(|error| format!("Could not save the cloud settings: {error}"))
}

pub fn info(app: &AppHandle) -> CloudInfo {
    let one = load(app);
    CloudInfo {
        endpoint: one.endpoint,
        region: one.region,
        bucket: one.bucket,
        prefix: one.prefix,
        key_id: one.key_id,
        has_secret: !one.secret.is_empty(),
        on: one.on,
        last: one.last,
    }
}

/// `secret`: `None` keeps the saved one.
pub fn save(app: &AppHandle, next: CloudInfo, secret: Option<String>) -> Result<CloudInfo, String> {
    let mut saved = load(app);
    let endpoint = next.endpoint.trim().trim_end_matches('/').to_string();
    if !endpoint.is_empty() && !endpoint.starts_with("https://") {
        return Err("The endpoint must start with https://.".into());
    }
    let bucket = next.bucket.trim().to_string();
    if !bucket.is_empty() && !bucket.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'-' || one == b'.') {
        return Err("Bucket names use a-z, 0-9, - and . only.".into());
    }
    saved.endpoint = endpoint;
    saved.region = if next.region.trim().is_empty() { "us-east-1".into() } else { next.region.trim().into() };
    saved.bucket = bucket;
    saved.prefix = next.prefix.trim().trim_matches('/').to_string();
    saved.key_id = next.key_id.trim().to_string();
    if let Some(secret) = secret {
        saved.secret = secret.trim().to_string();
    }
    saved.on = next.on && !saved.endpoint.is_empty() && !saved.bucket.is_empty() && !saved.key_id.is_empty() && !saved.secret.is_empty();
    keep(app, &saved)?;
    Ok(info(app))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|one| format!("{one:02x}")).collect()
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner: Vec<u8> = block.iter().map(|one| one ^ 0x36).collect();
    let outer: Vec<u8> = block.iter().map(|one| one ^ 0x5c).collect();
    let first = Sha256::new().chain_update(&inner).chain_update(data).finalize();
    Sha256::new().chain_update(&outer).chain_update(first).finalize().to_vec()
}

/// S3's path encoding: everything but unreserved characters and `/`.
fn encode(path: &str) -> String {
    path.bytes()
        .map(|one| {
            if one.is_ascii_alphanumeric() || matches!(one, b'-' | b'_' | b'.' | b'~' | b'/') {
                (one as char).to_string()
            } else {
                format!("%{one:02X}")
            }
        })
        .collect()
}

/// The `Authorization` header for a PUT of a body whose SHA-256 is `payload`, at `stamp` (`YYYYMMDDTHHMMSSZ`).
pub fn authorization(key_id: &str, secret: &str, region: &str, host: &str, path: &str, payload: &str, stamp: &str) -> String {
    let date = &stamp[..8];
    let headers = format!("host:{host}\nx-amz-content-sha256:{payload}\nx-amz-date:{stamp}\n");
    let signed = "host;x-amz-content-sha256;x-amz-date";
    let request = format!("PUT\n{}\n\n{headers}\n{signed}\n{payload}", encode(path));
    let scope = format!("{date}/{region}/s3/aws4_request");
    let to_sign = format!("AWS4-HMAC-SHA256\n{stamp}\n{scope}\n{}", hex(&Sha256::digest(request.as_bytes())));
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    for part in [region, "s3", "aws4_request"] {
        key = hmac(&key, part.as_bytes());
    }
    format!("AWS4-HMAC-SHA256 Credential={key_id}/{scope}, SignedHeaders={signed}, Signature={}", hex(&hmac(&key, to_sign.as_bytes())))
}

fn file_sha256(file: &Path) -> Result<String, String> {
    let mut reader = fs::File::open(file).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn upload(saved: &Saved, file: &Path, server: &str) -> Result<String, String> {
    let size = fs::metadata(file).map_err(|error| error.to_string())?.len();
    if size > MOST_BYTES {
        return Err("This backup is over 5 GB, more than one upload can carry. It stays on this PC only.".into());
    }
    let name = file.file_name().and_then(|one| one.to_str()).ok_or("The backup has no file name.")?;
    let server: String =
        server.chars().map(|one| if one.is_ascii_alphanumeric() || one == '-' || one == '_' { one } else { '-' }).collect();
    let key = [saved.prefix.as_str(), server.as_str(), name].iter().filter(|one| !one.is_empty()).copied().collect::<Vec<_>>().join("/");
    let host = saved.endpoint.trim_start_matches("https://").split('/').next().unwrap_or_default().to_string();
    let path = format!("/{}/{key}", saved.bucket);
    let payload = file_sha256(file)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let auth = authorization(&saved.key_id, &saved.secret, &saved.region, &host, &path, &payload, &stamp);
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(60 * 60)).build().map_err(|error| error.to_string())?;
    let body = reqwest::blocking::Body::sized(fs::File::open(file).map_err(|error| error.to_string())?, size);
    let answer = client
        .put(format!("https://{host}{}", encode(&path)))
        .header("x-amz-date", &stamp)
        .header("x-amz-content-sha256", &payload)
        .header("authorization", auth)
        .body(body)
        .send()
        .map_err(|error| format!("Could not reach the storage: {error}"))?;
    if answer.status().is_success() {
        Ok(key)
    } else {
        let status = answer.status();
        let text = answer.text().unwrap_or_default();
        let code = text.split("<Code>").nth(1).and_then(|rest| rest.split('<').next()).unwrap_or_default();
        Err(format!("The storage refused the upload ({status}{}).", if code.is_empty() { String::new() } else { format!(", {code}") }))
    }
}

/// Copies a finished backup in the background when cloud copies are on, and remembers how it went.
pub fn copy_after_backup(app: &AppHandle, file: PathBuf, server: &str) {
    let saved = load(app);
    if !saved.on {
        return;
    }
    let (app, server) = (app.clone(), server.to_string());
    std::thread::spawn(move || {
        let result = upload(&saved, &file, &server);
        let mut now = load(&app);
        now.last = match result {
            Ok(key) => format!("Copied {key}"),
            Err(error) => error,
        };
        let _ = keep(&app, &now);
    });
}

/// A one-off copy of a small test file, to check the settings.
pub fn test(app: &AppHandle) -> Result<String, String> {
    let saved = load(app);
    if saved.endpoint.is_empty() || saved.bucket.is_empty() || saved.key_id.is_empty() || saved.secret.is_empty() {
        return Err("Fill in the endpoint, bucket and both keys first.".into());
    }
    let file = std::env::temp_dir().join("monolab-cloud-test.txt");
    fs::write(&file, "Monolab can write here.\n").map_err(|error| error.to_string())?;
    let done = upload(&saved, &file, "test");
    let _ = fs::remove_file(&file);
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc4231() {
        // RFC 4231 test case 2.
        assert_eq!(
            hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn signs_like_aws() {
        // AWS's documented signing key example (sig v4 "Examples of how to derive a signing key").
        let mut key = hmac(b"AWS4wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", b"20120215");
        for part in ["us-east-1", "iam", "aws4_request"] {
            key = hmac(&key, part.as_bytes());
        }
        assert_eq!(hex(&key), "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d");
        let header =
            authorization("AKID", "secret", "us-west-004", "s3.example.com", "/bucket/a b.zip", &"0".repeat(64), "20261121T010203Z");
        assert!(header.starts_with("AWS4-HMAC-SHA256 Credential=AKID/20261121/us-west-004/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature="));
        assert_eq!(encode("/bucket/a b.zip"), "/bucket/a%20b.zip");
    }
}
