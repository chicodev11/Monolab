//! Monolink connections between devices, with iroh: QUIC that punches through home routers and
//! falls back to an iroh relay when it can't. The iroh key is the device key, so an iroh endpoint id
//! hashes to the same device id the account server knows.
//!
//! ALPN `/2`: the first stream is the control stream, where the teammate shows a role grant from the
//! account server (`Hello`), the host checks it and answers `Welcome`, and later grants replace it
//! (`Renew`, or `RenewNow` from the host when the team changed). Every other stream is one request,
//! answered with what the connection's grant allows. Section 6 of `docs/monolink.md`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use std::sync::Arc;

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, RelayMap, RelayMode, RelayUrl, SecretKey};
use relay_proto::grant::Grant;
use relay_proto::DeviceId;
use serde_json::{json, Value};
use tauri::AppHandle;

/// The protocol name both sides agree on (teams and role grants).
pub const ALPN_TEAM: &[u8] = b"monolab/monolink/2";
/// A control frame (a grant is well under this).
const MAX_FRAME: usize = 64 * 1024;
/// How long a teammate has to send a newer grant after `RenewNow`.
const RENEW_WITHIN: Duration = Duration::from_secs(10);
/// A teammate fetches a new grant this long before the old one runs out.
const RENEW_BEFORE: u64 = 3 * 60;
/// A request (an upload piece is the largest).
const MAX_REQUEST: usize = 16 * 1024 * 1024;
/// An answer (a file handed out for download can be 64 MiB as base64).
const MAX_ANSWER: usize = 100 * 1024 * 1024;
const CONNECT_WAIT: Duration = Duration::from_secs(8);

#[derive(Default)]
struct State {
    endpoint: Option<Endpoint>,
    /// Host side: teammates connected with a grant.
    team_conns: Vec<TeamConn>,
    /// Host side: the newest version seen of each team; older grants are refused.
    seen: HashMap<String, u64>,
    /// Guest side: connections to hosts, by team and host.
    team_links: HashMap<(String, DeviceId), Connection>,
    next: u64,
}

struct TeamConn {
    id: u64,
    team: String,
    grant: Arc<Mutex<Grant>>,
    connection: Connection,
    control: Arc<tokio::sync::Mutex<SendStream>>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

fn endpoint() -> Option<Endpoint> {
    state().lock().unwrap_or_else(|poison| poison.into_inner()).endpoint.clone()
}

/// The device id an iroh endpoint id stands for.
pub fn device_of(id: &iroh::EndpointId) -> DeviceId {
    DeviceId::of(id.as_bytes())
}

/// The iroh relay that carries a connection when no direct path works: Monolink's own, at the account
/// server's address (`relay/deploy/iroh-relay.toml`, only devices of an account may use it).
/// `MONOLAB_IROH_RELAY` names another, `n0` for n0's public relays.
fn relay_choice() -> String {
    std::env::var("MONOLAB_IROH_RELAY")
        .ok()
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty())
        .unwrap_or_else(crate::monolink_account::server)
}

/// A local test account server (plain http) has no relay next to it, so it keeps n0's.
fn relay_mode_for(choice: &str) -> RelayMode {
    if choice == "n0" || !choice.starts_with("https://") {
        return RelayMode::Default;
    }
    match choice.parse::<RelayUrl>() {
        Ok(url) => RelayMode::Custom(RelayMap::from(url)),
        Err(_) => RelayMode::Default,
    }
}

/// Opens this device's iroh endpoint and starts serving teammates. Runs while Monolink is on.
pub fn start(app: &AppHandle) {
    if endpoint().is_some() {
        return;
    }
    let Ok(key) = crate::device::device_key(app) else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let built = Endpoint::builder(presets::N0)
            .relay_mode(relay_mode_for(&relay_choice()))
            .secret_key(SecretKey::from_bytes(&key.to_bytes()))
            .alpns(vec![ALPN_TEAM.to_vec()])
            // Addresses go to teammates through the account server, not to a public directory.
            .clear_address_lookup()
            .bind()
            .await;
        let endpoint = match built {
            Ok(endpoint) => endpoint,
            Err(error) => {
                eprintln!("monolink direct: could not open the endpoint: {error}");
                return;
            }
        };
        {
            let mut held = state().lock().unwrap_or_else(|poison| poison.into_inner());
            if held.endpoint.is_some() {
                return;
            }
            held.endpoint = Some(endpoint.clone());
        }
        while let Some(incoming) = endpoint.accept().await {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let Ok(connection) = incoming.await else { return };
                if connection.alpn() == ALPN_TEAM {
                    serve_team(app, connection).await;
                }
            });
        }
    });
}

/// This device's address, published to the account server for teammates.
pub fn address() -> Result<Value, String> {
    let endpoint = endpoint().ok_or("Direct connections aren't running on the host.")?;
    serde_json::to_value(endpoint.addr()).map_err(|error| error.to_string())
}

fn held() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|poison| poison.into_inner())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0)
}

/// Writes one control frame: length (u32, big end) then JSON.
async fn write_frame(send: &mut SendStream, value: &Value) -> Result<(), String> {
    let body = value.to_string().into_bytes();
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&body);
    send.write_all(&frame).await.map_err(|error| error.to_string())
}

async fn read_frame(recv: &mut RecvStream) -> Option<Value> {
    let mut size = [0u8; 4];
    recv.read_exact(&mut size).await.ok()?;
    let size = u32::from_be_bytes(size) as usize;
    if size > MAX_FRAME {
        return None;
    }
    let mut body = vec![0u8; size];
    recv.read_exact(&mut body).await.ok()?;
    serde_json::from_slice(&body).ok()
}

/// Host side: whether a grant (text) lets `peer` in here now. Gives the grant or a reason code.
fn admit(app: &AppHandle, text: &str, peer: &DeviceId) -> Result<Grant, String> {
    let me = endpoint().map(|one| device_of(&one.id())).ok_or("stopped")?;
    let grant = relay_proto::grant::open(text, &crate::monolink_team::trusted_keys()).map_err(|refused| refused.code().to_string())?;
    let seen = held().seen.get(&grant.team).copied().unwrap_or(0);
    grant.check(peer, &me, unix_now(), seen).map_err(|refused| refused.code().to_string())?;
    if crate::monolink_team::reach(app, &grant).is_empty() {
        return Err("not_shared".into());
    }
    Ok(grant)
}

/// Host side: one teammate's connection.
async fn serve_team(app: AppHandle, connection: Connection) {
    let peer = device_of(&connection.remote_id());
    if !crate::monolink_account::is_signed_in(&app) {
        connection.close(1u32.into(), b"host signed out");
        return;
    }
    let Ok(Ok((mut send, mut recv))) = tokio::time::timeout(CONNECT_WAIT, connection.accept_bi()).await else {
        connection.close(1u32.into(), b"no hello");
        return;
    };
    let hello = tokio::time::timeout(CONNECT_WAIT, read_frame(&mut recv)).await.ok().flatten();
    let text = hello.as_ref().filter(|one| one["type"] == "hello").and_then(|one| one["grant"].as_str()).unwrap_or_default().to_string();
    let grant = match admit(&app, &text, &peer) {
        Ok(grant) => grant,
        Err(code) => {
            let _ = write_frame(&mut send, &json!({ "type": "denied", "reason": code })).await;
            let _ = send.finish();
            // Let the denial arrive before closing.
            tokio::time::sleep(Duration::from_millis(300)).await;
            connection.close(2u32.into(), b"denied");
            return;
        }
    };
    let welcome = crate::monolink_team::welcome(&app, &grant);
    if write_frame(&mut send, &json!({ "type": "welcome", "info": welcome })).await.is_err() {
        return;
    }
    let team = grant.team.clone();
    let user = grant.user.clone();
    let grant = Arc::new(Mutex::new(grant));
    let control = Arc::new(tokio::sync::Mutex::new(send));
    let id = {
        let mut held = held();
        held.next += 1;
        let id = held.next;
        held.team_conns.push(TeamConn {
            id,
            team: team.clone(),
            grant: grant.clone(),
            connection: connection.clone(),
            control: control.clone(),
        });
        id
    };

    // Control stream: newer grants for the same person and team.
    {
        let (app, connection, grant, control) = (app.clone(), connection.clone(), grant.clone(), control.clone());
        tauri::async_runtime::spawn(async move {
            while let Some(frame) = read_frame(&mut recv).await {
                if frame["type"] != "renew" {
                    continue;
                }
                let text = frame["grant"].as_str().unwrap_or_default();
                match admit(&app, text, &peer) {
                    Ok(newer) if newer.team == team && newer.user == user => {
                        let info = crate::monolink_team::welcome(&app, &newer);
                        *grant.lock().unwrap_or_else(|poison| poison.into_inner()) = newer;
                        let _ = write_frame(&mut *control.lock().await, &json!({ "type": "renewed", "info": info })).await;
                    }
                    other => {
                        let reason = other.err().unwrap_or_else(|| "wrong_team".into());
                        let _ = write_frame(&mut *control.lock().await, &json!({ "type": "denied", "reason": reason })).await;
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        connection.close(2u32.into(), b"denied");
                        return;
                    }
                }
            }
        });
    }
    // Grants run out: close when the current one has, unless a newer one came.
    {
        let (connection, grant) = (connection.clone(), grant.clone());
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                if connection.close_reason().is_some() {
                    return;
                }
                let exp = grant.lock().unwrap_or_else(|poison| poison.into_inner()).exp;
                if unix_now() >= exp {
                    connection.close(3u32.into(), b"expired");
                    return;
                }
            }
        });
    }

    while let Ok((mut send, mut recv)) = connection.accept_bi().await {
        // Monolink only works while this host is signed in.
        if !crate::monolink_account::is_signed_in(&app) {
            connection.close(1u32.into(), b"host signed out");
            break;
        }
        let (app, grant) = (app.clone(), grant.clone());
        tauri::async_runtime::spawn(async move {
            let answer = match recv.read_to_end(MAX_REQUEST).await {
                Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(request) => {
                        let command = request.get("cmd").and_then(Value::as_str).unwrap_or_default().to_string();
                        let body = request.get("body").cloned().unwrap_or(Value::Null);
                        // The grant as it is now: a renewal that lowered the role applies to the next request.
                        let grant = grant.lock().unwrap_or_else(|poison| poison.into_inner()).clone();
                        let ran =
                            tauri::async_runtime::spawn_blocking(move || crate::monolink_team::serve(&app, &peer, &grant, &command, &body))
                                .await;
                        match ran {
                            Ok(Ok(value)) => json!({ "value": value }),
                            Ok(Err(error)) => json!({ "error": error }),
                            Err(error) => json!({ "error": error.to_string() }),
                        }
                    }
                    Err(_) => json!({ "error": "The request was not valid." }),
                },
                Err(_) => return,
            };
            if send.write_all(answer.to_string().as_bytes()).await.is_ok() {
                let _ = send.finish();
            }
        });
    }
    held().team_conns.retain(|one| one.id != id);
}

/// Host side: the account server says a team changed (members, roles, servers, devices). Grants made
/// before this version are asked to renew, and dropped if no newer one comes within ten seconds.
pub fn team_changed(team: &str, version: u64) {
    type Stale = (Connection, Arc<Mutex<Grant>>, Arc<tokio::sync::Mutex<SendStream>>);
    let stale: Vec<Stale> = {
        let mut held = held();
        let seen = held.seen.entry(team.to_string()).or_insert(0);
        *seen = (*seen).max(version);
        held.team_conns
            .iter()
            .filter(|one| one.team == team && one.grant.lock().unwrap_or_else(|poison| poison.into_inner()).team_version < version)
            .map(|one| (one.connection.clone(), one.grant.clone(), one.control.clone()))
            .collect()
    };
    for (connection, grant, control) in stale {
        tauri::async_runtime::spawn(async move {
            let _ = write_frame(&mut *control.lock().await, &json!({ "type": "renew_now" })).await;
            tokio::time::sleep(RENEW_WITHIN).await;
            if grant.lock().unwrap_or_else(|poison| poison.into_inner()).team_version < version {
                connection.close(4u32.into(), b"team changed");
            }
        });
    }
}

/// Closes every Monolink connection, host and guest side (signed out).
pub fn close_all() {
    let mut held = held();
    for conn in held.team_conns.drain(..) {
        conn.connection.close(1u32.into(), b"host signed out");
    }
    for (_, connection) in held.team_links.drain() {
        connection.close(0u32.into(), b"signed out");
    }
}

/// Closes everything for a team, host and guest side (server taken out, team left or deleted).
pub fn close_team(team: &str) {
    let mut held = held();
    for conn in held.team_conns.iter().filter(|one| one.team == team) {
        conn.connection.close(5u32.into(), b"not shared");
    }
    held.team_conns.retain(|one| one.team != team);
    for ((one, _), connection) in held.team_links.iter() {
        if one == team {
            connection.close(0u32.into(), b"left");
        }
    }
    held.team_links.retain(|(one, _), _| one != team);
}

/// Guest side: a request to a teammate's host. iroh's own relay carries it when a direct path can't
/// be found.
pub async fn call_team(app: &AppHandle, team: &str, host: DeviceId, command: &str, body: &Value, wait: Duration) -> Result<Value, String> {
    let connection = team_link(app, team, host).await?;
    let request = json!({ "cmd": command, "body": body }).to_string();
    let Ok(Ok((mut send, mut recv))) = tokio::time::timeout(CONNECT_WAIT, connection.open_bi()).await else {
        held().team_links.remove(&(team.to_string(), host));
        return Err("Lost the connection to the host. Try again.".into());
    };
    if send.write_all(request.as_bytes()).await.is_err() || send.finish().is_err() {
        held().team_links.remove(&(team.to_string(), host));
        return Err("Lost the connection to the host. Try again.".into());
    }
    match tokio::time::timeout(wait, recv.read_to_end(MAX_ANSWER)).await {
        Ok(Ok(bytes)) => {
            let answer: Value = serde_json::from_slice(&bytes).map_err(|_| "The host's answer was not valid.".to_string())?;
            match answer.get("error").and_then(Value::as_str) {
                Some(error) => Err(error.to_string()),
                None => Ok(answer.get("value").cloned().unwrap_or(Value::Null)),
            }
        }
        Ok(Err(_)) => {
            held().team_links.remove(&(team.to_string(), host));
            Err(close_text(&connection))
        }
        Err(_) => Err("The host didn't answer in time.".into()),
    }
}

/// Words for why the host closed a team connection.
fn close_text(connection: &Connection) -> String {
    let reason = connection.close_reason().map(|one| one.to_string()).unwrap_or_default();
    if reason.contains("team changed") || reason.contains("denied") {
        "Your access to this server changed. Open it again.".into()
    } else if reason.contains("not shared") {
        "This server isn't shared with the team any more.".into()
    } else {
        "Lost the connection to the host. Try again.".into()
    }
}

fn denied_text(reason: &str) -> String {
    match reason {
        "not_shared" => "This server isn't shared with the team any more.".into(),
        "unknown_key" | "bad_signature" => "The host couldn't check your access. One of you may need to update Monolab.".into(),
        "clock" => "Your computer's clock is off. Set it to the right time and try again.".into(),
        "stopped" => "The host's Monolink isn't running.".into(),
        _ => format!("The host didn't let you in ({reason})."),
    }
}

/// Guest side: the open connection to a host for a team, or a new one (grant shown and accepted).
async fn team_link(app: &AppHandle, team: &str, host: DeviceId) -> Result<Connection, String> {
    let key = (team.to_string(), host);
    if let Some(open) = held().team_links.get(&key).filter(|one| one.close_reason().is_none()) {
        return Ok(open.clone());
    }
    start(app);
    let mut last = String::new();
    // A second try with a fresh grant, for a team that changed since the last one was fetched.
    for fresh in [false, true] {
        match open_team_link(app, team, host, fresh).await {
            Ok(connection) => {
                held().team_links.insert(key, connection.clone());
                return Ok(connection);
            }
            Err((error, retry)) => {
                last = error;
                if !retry {
                    break;
                }
            }
        }
    }
    Err(last)
}

/// One connection attempt. The error says whether a fresh grant might help.
async fn open_team_link(app: &AppHandle, team: &str, host: DeviceId, fresh: bool) -> Result<Connection, (String, bool)> {
    // The endpoint may still be opening right after start.
    let mut opened = endpoint();
    for _ in 0..30 {
        if opened.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        opened = endpoint();
    }
    let endpoint = opened.ok_or(("Direct connections couldn't start on this PC.".to_string(), false))?;
    let pass = crate::monolink_account::grant_for(app, team, &host, fresh).await.map_err(|error| (error, false))?;
    let address = pass.addr.ok_or(("The host is offline.".to_string(), !fresh))?;
    let address: EndpointAddr = serde_json::from_value(address).map_err(|_| ("The host's address was not valid.".to_string(), !fresh))?;
    if device_of(&address.id) != host {
        return Err(("The host's address was not valid.".into(), false));
    }
    let connection = tokio::time::timeout(CONNECT_WAIT, endpoint.connect(address, ALPN_TEAM))
        .await
        .map_err(|_| ("Couldn't reach the host.".to_string(), !fresh))?
        .map_err(|_| ("Couldn't reach the host.".to_string(), !fresh))?;
    let (mut send, mut recv) = connection.open_bi().await.map_err(|error| (error.to_string(), false))?;
    write_frame(&mut send, &json!({ "type": "hello", "grant": pass.grant, "app": env!("CARGO_PKG_VERSION") }))
        .await
        .map_err(|error| (error, false))?;
    let answer = tokio::time::timeout(CONNECT_WAIT, read_frame(&mut recv))
        .await
        .ok()
        .flatten()
        .ok_or(("The host didn't answer.".to_string(), false))?;
    if answer["type"] != "welcome" {
        let reason = answer["reason"].as_str().unwrap_or("denied").to_string();
        connection.close(0u32.into(), b"denied");
        let retry = !fresh && matches!(reason.as_str(), "outdated" | "expired" | "not_shared");
        return Err((denied_text(&reason), retry));
    }
    // Keeps the grant fresh on this connection: before it runs out, and when the host asks.
    let (app, team, mine) = (app.clone(), team.to_string(), connection.clone());
    let mut exp = pass.exp;
    tauri::async_runtime::spawn(async move {
        loop {
            let until = exp.saturating_sub(RENEW_BEFORE).saturating_sub(unix_now());
            let renew = tokio::select! {
                frame = read_frame(&mut recv) => match frame {
                    None => break,
                    Some(frame) if frame["type"] == "renew_now" => true,
                    Some(frame) if frame["type"] == "denied" => break,
                    Some(_) => false,
                },
                _ = tokio::time::sleep(Duration::from_secs(until.max(1))) => true,
            };
            if !renew {
                continue;
            }
            match crate::monolink_account::grant_for(&app, &team, &host, true).await {
                Ok(next) => {
                    exp = next.exp;
                    if write_frame(&mut send, &json!({ "type": "renew", "grant": next.grant })).await.is_err() {
                        break;
                    }
                }
                // Taken out of the team, or the server detached: the host closes it.
                Err(_) => tokio::time::sleep(Duration::from_secs(5)).await,
            }
        }
        held().team_links.retain(|_, one| one.stable_id() != mine.stable_id());
    });
    Ok(connection)
}

/// How this device reaches a team host right now: "direct", "relayed" (iroh relay) or "none".
pub fn team_route(team: &str, host: &DeviceId) -> &'static str {
    let held = held();
    let Some(connection) = held.team_links.get(&(team.to_string(), *host)).filter(|one| one.close_reason().is_none()) else {
        return "none";
    };
    match connection.paths().iter().find(|one| one.is_selected()) {
        Some(path) if path.is_ip() => "direct",
        _ => "relayed",
    }
}

#[cfg(test)]
mod relay_tests {
    use super::*;

    #[test]
    fn uses_our_relay_unless_told_or_testing_locally() {
        let RelayMode::Custom(map) = relay_mode_for("https://mnlab.cc") else { panic!("our relay") };
        assert_eq!(map.len(), 1);
        assert!(matches!(relay_mode_for("n0"), RelayMode::Default));
        assert!(matches!(relay_mode_for("http://127.0.0.1:8080"), RelayMode::Default));
    }
}
