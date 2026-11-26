//! Monolink v2 over iroh (ALPN `monolab/monolink/2`), host side, with the app's rules
//! (`src-tauri/src/direct.rs`, `serve_team`): the first stream is the control stream, where the
//! teammate shows a grant (`hello`), the node checks it and answers `welcome` or `denied`; newer
//! grants replace it (`renew` → `renewed`); the connection closes when the grant runs out. Every
//! other stream is one request, answered with what the connection's grant allows *now*.
//!
//! The node learns that a team changed two ways: a grant with a newer team version, and the team versions it
//! reads from the account server every 30 seconds (`main.rs`, F7). Either raises the version this node
//! accepts, and older connections of that team get `renew_now` and ten seconds to show a newer grant.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream};
use relay_proto::DeviceId;
use relay_proto::grant::Grant;
use serde_json::{Value, json};

use crate::serve::{Node, admit, serve, unix_now, welcome};

pub const ALPN_TEAM: &[u8] = b"monolab/monolink/2";
/// A control frame (a grant is well under this).
const MAX_FRAME: usize = 64 * 1024;
/// A request (the app's limit; the node's own requests are far smaller).
const MAX_REQUEST: usize = 16 * 1024 * 1024;
const HELLO_WAIT: Duration = Duration::from_secs(8);
/// How long a teammate has to send a newer grant after `renew_now`.
const RENEW_WITHIN: Duration = Duration::from_secs(10);

type Control = Arc<tokio::sync::Mutex<SendStream>>;

struct TeamConn {
    id: u64,
    team: String,
    grant: Arc<Mutex<Grant>>,
    connection: Connection,
    control: Control,
}

#[derive(Default)]
struct State {
    conns: Vec<TeamConn>,
    /// The newest version seen of each team; older grants are refused.
    seen: HashMap<String, u64>,
    next: u64,
}

pub struct Server {
    pub node: Arc<Node>,
    state: Mutex<State>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub fn device_of(id: &iroh::EndpointId) -> DeviceId {
    DeviceId::of(id.as_bytes())
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

impl Server {
    pub fn new(node: Node) -> Arc<Server> {
        Arc::new(Server { node: Arc::new(node), state: Mutex::new(State::default()) })
    }

    /// Checks a grant and, when it is newer than any seen for its team, makes it the floor and asks
    /// older connections of that team to renew.
    fn admit(&self, text: &str, peer: &DeviceId) -> Result<Grant, String> {
        let seen = |team: &str| locked(&self.state).seen.get(team).copied().unwrap_or(0);
        // The team isn't known before opening, so open once to learn it, then check against its floor.
        let first = admit(&self.node, text, peer, unix_now(), 0)?;
        let grant = admit(&self.node, text, peer, unix_now(), seen(&first.team))?;
        self.raise(&grant.team, grant.team_version);
        Ok(grant)
    }

    pub fn raise(&self, team: &str, version: u64) {
        type Stale = (Connection, Arc<Mutex<Grant>>, Control);
        let stale: Vec<Stale> = {
            let mut state = locked(&self.state);
            let seen = state.seen.entry(team.to_string()).or_insert(0);
            if version <= *seen {
                return;
            }
            *seen = version;
            state
                .conns
                .iter()
                .filter(|one| one.team == team && locked(&one.grant).team_version < version)
                .map(|one| (one.connection.clone(), one.grant.clone(), one.control.clone()))
                .collect()
        };
        for (connection, grant, control) in stale {
            tokio::spawn(async move {
                let _ = write_frame(&mut *control.lock().await, &json!({ "type": "renew_now" })).await;
                tokio::time::sleep(RENEW_WITHIN).await;
                if locked(&grant).team_version < version {
                    connection.close(4u32.into(), b"team changed");
                }
            });
        }
    }

    /// One teammate's connection, from the hello to the close.
    pub async fn serve_team(self: Arc<Self>, connection: Connection) {
        let peer = device_of(&connection.remote_id());
        let Ok(Ok((mut send, mut recv))) = tokio::time::timeout(HELLO_WAIT, connection.accept_bi()).await else {
            connection.close(1u32.into(), b"no hello");
            return;
        };
        let hello = tokio::time::timeout(HELLO_WAIT, read_frame(&mut recv)).await.ok().flatten();
        let text =
            hello.as_ref().filter(|one| one["type"] == "hello").and_then(|one| one["grant"].as_str()).unwrap_or_default().to_string();
        let grant = match self.admit(&text, &peer) {
            Ok(grant) => grant,
            Err(code) => {
                self.node.log.record("(unknown)", "team", &peer.text(), "__hello", &json!({}), &Err(format!("denied: {code}")));
                let _ = write_frame(&mut send, &json!({ "type": "denied", "reason": code })).await;
                let _ = send.finish();
                // Let the denial arrive before closing.
                tokio::time::sleep(Duration::from_millis(300)).await;
                connection.close(2u32.into(), b"denied");
                return;
            }
        };
        if write_frame(&mut send, &json!({ "type": "welcome", "info": welcome(&self.node.config, &grant) })).await.is_err() {
            return;
        }
        let (team, user) = (grant.team.clone(), grant.user.clone());
        let grant = Arc::new(Mutex::new(grant));
        let control: Control = Arc::new(tokio::sync::Mutex::new(send));
        let id = {
            let mut state = locked(&self.state);
            state.next += 1;
            let id = state.next;
            state.conns.push(TeamConn {
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
            let (server, connection, grant, control) = (self.clone(), connection.clone(), grant.clone(), control.clone());
            tokio::spawn(async move {
                while let Some(frame) = read_frame(&mut recv).await {
                    if frame["type"] != "renew" {
                        continue;
                    }
                    let text = frame["grant"].as_str().unwrap_or_default();
                    match server.admit(text, &peer) {
                        Ok(newer) if newer.team == team && newer.user == user => {
                            let info = welcome(&server.node.config, &newer);
                            *locked(&grant) = newer;
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
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    if connection.close_reason().is_some() {
                        return;
                    }
                    if unix_now() >= locked(&grant).exp {
                        connection.close(3u32.into(), b"expired");
                        return;
                    }
                }
            });
        }

        while let Ok((mut send, mut recv)) = connection.accept_bi().await {
            let (node, grant) = (self.node.clone(), grant.clone());
            tokio::spawn(async move {
                let answer = match recv.read_to_end(MAX_REQUEST).await {
                    Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                        Ok(request) => {
                            let command = request.get("cmd").and_then(Value::as_str).unwrap_or_default().to_string();
                            let body = request.get("body").cloned().unwrap_or(Value::Null);
                            // The grant as it is now: a renewal that lowered the role applies to the next request.
                            let grant = locked(&grant).clone();
                            let ran = tokio::task::spawn_blocking(move || {
                                // Checked again per request: a grant that ran out between checks answers nothing.
                                if unix_now() >= grant.exp {
                                    return Err("Your access ran out. Open the server again.".to_string());
                                }
                                serve(&node, &peer, &grant, &command, &body)
                            })
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
        locked(&self.state).conns.retain(|one| one.id != id);
    }
}
