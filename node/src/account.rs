//! The node as a device of its owner's Monolab account: pairing with a one-time code, and telling the
//! account server where it can be reached (`PUT /v2/me/addr`) so teammates' grants carry its address.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

/// What the node signs to pair. Must match the account server's `node_pair_message`.
pub fn pair_message(code: &str) -> String {
    format!("monolab-node-pair-v1\n{code}")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn session_path(data: &Path) -> PathBuf {
    data.join("session.token")
}

/// The session the node got when it was paired, if it was.
pub fn session(data: &Path) -> Option<String> {
    std::fs::read_to_string(session_path(data)).ok().map(|text| text.trim().to_string()).filter(|text| !text.is_empty())
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("monolab-node/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())
}

/// `monolab-node pair <code>`: the owner made the code in the app (Team → Nodes). The node proves it holds
/// its key by signing the code, gets a session of its own and keeps it in `<data>/session.token` (0600).
pub fn pair(accounts: &str, data: &Path, key: &[u8; 32], code: &str, name: &str) -> Result<String, String> {
    let code = code.trim().to_ascii_uppercase();
    if code.len() != 10 || !code.chars().all(|one| one.is_ascii_alphanumeric()) {
        return Err("The code is the 10 letters and digits the app showed.".into());
    }
    let signing = SigningKey::from_bytes(key);
    let body = json!({
        "code": code,
        "public_key": hex(&signing.verifying_key().to_bytes()),
        "signature": hex(&signing.sign(pair_message(&code).as_bytes()).to_bytes()),
        "name": name,
    });
    let response = client()?.post(format!("{}/v2/nodes/pair", accounts.trim_end_matches('/'))).json(&body).send().map_err(|error| format!("Could not reach the account server: {error}"))?;
    let status = response.status();
    let answer: Value = response.json().unwrap_or(Value::Null);
    if !status.is_success() {
        let why = answer["error"].as_str().unwrap_or("refused");
        return Err(match why {
            "expired" => "That code has expired or was already used. Make a new one in the app.".into(),
            "device_revoked" => "This node was removed from the account. Remove device.key to pair it as a new node.".into(),
            "too_many_devices" => "The account has too many devices. Remove one in the app first.".into(),
            other => format!("The account server said no ({other})."),
        });
    }
    let token = answer["token"].as_str().ok_or("The account server didn't give a session.")?;
    std::fs::create_dir_all(data).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(session_path(data))
        .map_err(|error| format!("Could not save the session: {error}"))?;
    file.write_all(token.as_bytes()).map_err(|error| error.to_string())?;
    Ok(answer["device"].as_str().unwrap_or_default().to_string())
}

/// Tells the account server where this node is (its iroh address as JSON). A 401 means the node was
/// removed from the account; the caller stops trying.
pub fn send_addr(accounts: &str, token: &str, addr: &Value) -> Result<(), String> {
    let response = client()?
        .put(format!("{}/v2/me/addr", accounts.trim_end_matches('/')))
        .bearer_auth(token)
        .json(&json!({ "addr": addr }))
        .send()
        .map_err(|error| error.to_string())?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err("unpaired".into()),
        other => Err(format!("account server answered {other}")),
    }
}

/// The versions of the teams this node's owner is in (`GET /v2/me`), to learn of a team change without waiting
/// for a newer grant (F7). A 401 means the node was removed.
pub fn team_versions(accounts: &str, token: &str) -> Result<Vec<(String, u64)>, String> {
    let response = client()?
        .get(format!("{}/v2/me", accounts.trim_end_matches('/')))
        .bearer_auth(token)
        .send()
        .map_err(|error| error.to_string())?;
    match response.status().as_u16() {
        200..=299 => Ok(versions_of(&response.json::<Value>().map_err(|error| error.to_string())?)),
        401 => Err("unpaired".into()),
        other => Err(format!("account server answered {other}")),
    }
}

/// One team as its members see it (`GET /v2/teams/{team}`): its servers carry `instance_id` and `host_device`.
pub fn team(accounts: &str, token: &str, team: &str) -> Result<Value, String> {
    let response = client()?
        .get(format!("{}/v2/teams/{team}", accounts.trim_end_matches('/')))
        .bearer_auth(token)
        .send()
        .map_err(|error| error.to_string())?;
    match response.status().as_u16() {
        200..=299 => response.json::<Value>().map_err(|error| error.to_string()),
        401 => Err("unpaired".into()),
        other => Err(format!("account server answered {other}")),
    }
}

/// Tells a team what happened on one of its servers this node hosts (`POST /v2/teams/{team}/activity`).
pub fn post_activity(accounts: &str, token: &str, team: &str, server: &str, kind: &str, data: &Value) -> Result<(), String> {
    let response = client()?
        .post(format!("{}/v2/teams/{team}/activity", accounts.trim_end_matches('/')))
        .bearer_auth(token)
        .json(&json!({ "server": server, "kind": kind, "data": data }))
        .send()
        .map_err(|error| error.to_string())?;
    match response.status().as_u16() {
        200..=299 => Ok(()),
        401 => Err("unpaired".into()),
        other => Err(format!("account server answered {other}")),
    }
}

/// `(team, version)` from a `/v2/me` answer.
pub fn versions_of(me: &Value) -> Vec<(String, u64)> {
    me["teams"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|one| Some((one["id"].as_str()?.to_string(), one["version"].as_u64()?)))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_team_versions() {
        let me = serde_json::json!({ "teams": [{ "id": "T1", "version": 4 }, { "id": "T2" }, { "version": 2 }] });
        assert_eq!(super::versions_of(&me), [("T1".to_string(), 4)]);
    }

    #[test]
    fn signs_what_the_account_server_checks() {
        assert_eq!(super::pair_message("ABCDEFGHJK"), "monolab-node-pair-v1\nABCDEFGHJK");
    }
}
