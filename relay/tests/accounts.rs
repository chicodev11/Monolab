//! Monolink account server, end to end on this machine with the pretend sign-in:
//! sign up, pick handles, make a team, invite with approval, change roles, see each other online.

use std::time::Duration;

use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

struct Temp(std::path::PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("monolab-accounts-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Temp(dir)
}

struct Server {
    base: String,
    http: reqwest::Client,
    _accounts: relay::accounts::Accounts,
}

async fn server(dir: &Temp) -> Server {
    server_with(dir, vec![]).await
}

async fn server_with(dir: &Temp, admins: Vec<String>) -> Server {
    let config = relay::accounts::Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        data: dir.0.clone(),
        public_url: "http://127.0.0.1".into(),
        discord: None,
        fake_sign_in: true,
        updates_url: None,
        admins,
    };
    let accounts = relay::accounts::start(config).await.unwrap();
    Server { base: format!("http://{}", accounts.addr), http: reqwest::Client::new(), _accounts: accounts }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Server {
    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
        let url = format!("{}{path}", self.base);
        let mut request = match method {
            "GET" => self.http.get(url),
            "POST" => self.http.post(url),
            "PATCH" => self.http.patch(url),
            "DELETE" => self.http.delete(url),
            "PUT" => self.http.put(url),
            _ => unreachable!(),
        };
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// Starts a sign-in from a device and finishes the pretend Discord step. Gives what polling says.
    async fn sign_in(&self, seed: u8, subject: &str, name: &str) -> Value {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let at = now();
        let signature = key.sign(format!("monolab-login-v1\nfake\n{at}").as_bytes());
        let (status, begin) = self
            .call(
                "POST",
                "/v2/auth/begin",
                None,
                Some(json!({
                    "provider": "fake",
                    "public_key": hex(&key.verifying_key().to_bytes()),
                    "device_name": format!("PC {seed}"),
                    "at": at,
                    "signature": hex(&signature.to_bytes()),
                    "port": 50123,
                })),
            )
            .await;
        assert_eq!(status, 200, "{begin}");
        let poll = json!({ "login_id": begin["login_id"], "poll_secret": begin["poll_secret"] });
        let (_, waiting) = self.call("POST", "/v2/auth/poll", None, Some(poll.clone())).await;
        assert_eq!(waiting["state"], "waiting");
        // The browser step: the returned url points at the public address, so take its path.
        let url = begin["url"].as_str().unwrap();
        let path = &url[url.find("/auth/").unwrap()..];
        let browser = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let page = browser.get(format!("{}{path}&subject={subject}&name={name}", self.base)).send().await.unwrap();
        // Sent to the app that started it, on 127.0.0.1, with the proof.
        assert_eq!(page.status().as_u16(), 303);
        let back = page.headers()["location"].to_str().unwrap().to_string();
        assert!(back.starts_with("http://127.0.0.1:50123/monolab-login?login="), "{back}");
        let proof = back.split("proof=").nth(1).unwrap().to_string();
        // Without the proof (someone else's browser finished the sign-in), nothing is handed out.
        let (_, still) = self.call("POST", "/v2/auth/poll", None, Some(poll.clone())).await;
        assert_eq!(still["state"], "waiting");
        let wrong = json!({ "login_id": begin["login_id"], "poll_secret": begin["poll_secret"], "proof": "0".repeat(proof.len()) });
        let (_, still) = self.call("POST", "/v2/auth/poll", None, Some(wrong)).await;
        assert_eq!(still["state"], "waiting");
        // The same link can't be finished twice.
        let twice = browser.get(format!("{}{path}&subject=999&name=intruder", self.base)).send().await.unwrap();
        assert_eq!(twice.status().as_u16(), 200, "an expired page, not another redirect");
        let poll = json!({ "login_id": begin["login_id"], "poll_secret": begin["poll_secret"], "proof": proof });
        let (_, result) = self.call("POST", "/v2/auth/poll", None, Some(poll.clone())).await;
        // Handed out once.
        let (_, again) = self.call("POST", "/v2/auth/poll", None, Some(poll)).await;
        assert_eq!(again["reason"], "expired");
        result
    }

    /// A new person: sign in, then pick a handle. Gives the session token and user id.
    async fn sign_up(&self, seed: u8, subject: &str, handle: &str) -> (String, String) {
        let result = self.sign_in(seed, subject, handle).await;
        assert_eq!(result["state"], "needs_handle", "{result}");
        let (status, done) =
            self.call("POST", "/v2/auth/finish", None, Some(json!({ "ticket": result["ticket"], "handle": handle }))).await;
        assert_eq!(status, 200, "{done}");
        assert_eq!(done["user"]["handle"], handle.to_lowercase());
        (done["token"].as_str().unwrap().to_string(), done["user"]["id"].as_str().unwrap().to_string())
    }

    async fn live(&self, token: &str) -> Live {
        let url = format!("{}/v2/ws", self.base.replace("http://", "ws://"));
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        socket.send(Message::Text(json!({ "type": "auth", "token": token }).to_string().into())).await.unwrap();
        let mut live = Live { socket };
        assert_eq!(live.next().await["type"], "ready");
        live
    }
}

struct Live {
    socket: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
}

impl Live {
    async fn next(&mut self) -> Value {
        loop {
            let message = tokio::time::timeout(Duration::from_secs(5), self.socket.next()).await.expect("server answered in time");
            match message {
                Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                other => panic!("connection ended: {other:?}"),
            }
        }
    }

    /// Skips messages until one of `kind` arrives.
    async fn until(&mut self, kind: &str) -> Value {
        loop {
            let message = self.next().await;
            if message["type"] == kind {
                return message;
            }
        }
    }
}

#[tokio::test]
async fn team_invite_and_roles() {
    let dir = temp("flow");
    let server = server(&dir).await;

    let (alice, alice_id) = server.sign_up(1, "100", "Alice").await;
    let (_, check) = server.call("GET", "/v2/handles/ALICE", None, None).await;
    assert_eq!(check["reason"], "taken");
    let (_, check) = server.call("GET", "/v2/handles/admin", None, None).await;
    assert_eq!(check["reason"], "reserved");
    let (bob, bob_id) = server.sign_up(2, "200", "bob").await;

    // Signing in again with the same Discord account goes straight in.
    let again = server.sign_in(3, "100", "whatever").await;
    assert_eq!(again["state"], "signed_in");
    assert_eq!(again["user"]["handle"], "alice");

    let (status, team) = server.call("POST", "/v2/teams", Some(&alice), Some(json!({ "name": "  Build Crew " }))).await;
    assert_eq!(status, 200, "{team}");
    assert_eq!(team["name"], "Build Crew");
    assert_eq!(team["role"], "owner");
    let team_id = team["id"].as_str().unwrap().to_string();

    // Not in the team: looks like it doesn't exist.
    let (status, _) = server.call("GET", &format!("/v2/teams/{team_id}"), Some(&bob), None).await;
    assert_eq!(status, 404);

    let (status, bad) = server
        .call(
            "POST",
            &format!("/v2/teams/{team_id}/invites"),
            Some(&alice),
            Some(json!({ "role": "developer", "expires_in": 31 * 86_400 })),
        )
        .await;
    assert_eq!((status, bad["error"].as_str()), (400, Some("bad_expiry")));
    let (status, bad) = server.call("POST", &format!("/v2/teams/{team_id}/invites"), Some(&alice), Some(json!({ "role": "owner" }))).await;
    assert_eq!((status, bad["error"].as_str()), (400, Some("bad_role")));

    let (status, invite) =
        server.call("POST", &format!("/v2/teams/{team_id}/invites"), Some(&alice), Some(json!({ "role": "developer" }))).await;
    assert_eq!(status, 200, "{invite}");
    assert_eq!(invite["needs_approval"], true);
    let code = invite["code"].as_str().unwrap().to_string();
    assert!(invite["url"].as_str().unwrap().ends_with(&format!("/invite/{code}")));

    let mut alice_live = server.live(&alice).await;

    // The whole link works as well as the bare code, in any case.
    let link = format!("https://mnlab.cc/invite/{}", code.to_uppercase());
    let (_, preview) = server.call("GET", &format!("/v2/invites/{}", urlencode(&link)), None, None).await;
    assert_eq!(preview["valid"], true, "{preview}");
    assert_eq!(preview["team"]["name"], "Build Crew");
    assert_eq!(preview["by"], "alice");
    assert_eq!(preview["role"], "developer");

    let page = server.http.get(format!("{}/invite/{code}", server.base)).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    let html = page.text().await.unwrap();
    assert!(html.contains("Build Crew") && html.contains(&format!("monolab://invite/{code}")));

    let (_, joined) = server.call("POST", &format!("/v2/invites/{code}/accept"), Some(&bob), None).await;
    assert_eq!(joined["state"], "waiting");
    let notice = alice_live.until("notice").await;
    assert_eq!(notice["kind"], "join_request");
    assert_eq!(notice["data"]["user"], "bob");

    // Still waiting: no second request, no second notice.
    let (_, joined) = server.call("POST", &format!("/v2/invites/{code}/accept"), Some(&bob), None).await;
    assert_eq!(joined["state"], "waiting");

    let (_, requests) = server.call("GET", &format!("/v2/teams/{team_id}/requests"), Some(&alice), None).await;
    assert_eq!(requests["requests"][0]["user"]["handle"], "bob");
    let (status, _) =
        server.call("POST", &format!("/v2/teams/{team_id}/requests/{bob_id}"), Some(&alice), Some(json!({ "accept": true }))).await;
    assert_eq!(status, 204);
    let changed = alice_live.until("team_changed").await;
    assert_eq!(changed["team"], team_id.as_str());

    let mut bob_live = server.live(&bob).await;
    let presence = alice_live.until("presence").await;
    assert_eq!((presence["user"].as_str(), presence["online"].as_bool()), (Some(bob_id.as_str()), Some(true)));

    let (_, team) = server.call("GET", &format!("/v2/teams/{team_id}"), Some(&bob), None).await;
    assert_eq!(team["role"], "developer");
    assert_eq!(team["members"].as_array().unwrap().len(), 2);
    assert_eq!(team["members"][0]["online"], true);

    // A developer runs nothing in the team.
    let (status, _) = server.call("POST", &format!("/v2/teams/{team_id}/invites"), Some(&bob), Some(json!({ "role": "viewer" }))).await;
    assert_eq!(status, 403);
    let (status, _) = server.call("DELETE", &format!("/v2/teams/{team_id}/members/{alice_id}"), Some(&bob), None).await;
    assert_eq!(status, 403);

    let (status, _) =
        server.call("PATCH", &format!("/v2/teams/{team_id}/members/{bob_id}"), Some(&alice), Some(json!({ "role": "viewer" }))).await;
    assert_eq!(status, 200);
    assert_eq!(bob_live.until("team_changed").await["team"], team_id.as_str());
    let notice = bob_live.until("notice").await;
    assert_eq!((notice["kind"].as_str(), notice["data"]["role"].as_str()), (Some("role_changed"), Some("viewer")));

    let (status, bad) =
        server.call("PATCH", &format!("/v2/teams/{team_id}/members/{alice_id}"), Some(&alice), Some(json!({ "role": "admin" }))).await;
    assert_eq!((status, bad["error"].as_str()), (403, Some("self")));
    let (status, bad) = server.call("POST", &format!("/v2/teams/{team_id}/transfer"), Some(&alice), Some(json!({ "user": bob_id }))).await;
    assert_eq!((status, bad["error"].as_str()), (409, Some("not_admin")));

    let (_, activity) = server.call("GET", &format!("/v2/teams/{team_id}/activity"), Some(&bob), None).await;
    let kinds: Vec<&str> = activity["items"].as_array().unwrap().iter().map(|item| item["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["role_changed", "member_joined", "team_created"]);
    assert_eq!(activity["items"][0]["actor"], "alice");

    let (status, bad) = server.call("DELETE", &format!("/v2/teams/{team_id}/members/{alice_id}"), Some(&alice), None).await;
    assert_eq!((status, bad["error"].as_str()), (409, Some("owner_cannot_leave")));
    let (status, _) = server.call("DELETE", &format!("/v2/teams/{team_id}/members/{bob_id}"), Some(&bob), None).await;
    assert_eq!(status, 204);
    let (_, team) = server.call("GET", &format!("/v2/teams/{team_id}"), Some(&alice), None).await;
    assert_eq!(team["members"].as_array().unwrap().len(), 1);
    // Bob hears that the team changed one last time, so his app drops it.
    assert_eq!(bob_live.until("team_changed").await["team"], team_id.as_str());
}

#[tokio::test]
async fn invites_handles_and_devices() {
    let dir = temp("rules");
    let server = server(&dir).await;
    let (alice, _) = server.sign_up(1, "100", "alice").await;
    let (bob, _) = server.sign_up(2, "200", "bob").await;

    // A new handle can't be changed for 30 days.
    let (status, bad) = server.call("PATCH", "/v2/me", Some(&alice), Some(json!({ "handle": "alice2" }))).await;
    assert_eq!((status, bad["error"].as_str()), (409, Some("too_soon")));
    assert!(bad["next"].as_u64().unwrap() > now() + 29 * 86_400);

    let (_, team) = server.call("POST", "/v2/teams", Some(&alice), Some(json!({ "name": "Open" }))).await;
    let team_id = team["id"].as_str().unwrap();
    let (_, invite) = server
        .call(
            "POST",
            &format!("/v2/teams/{team_id}/invites"),
            Some(&alice),
            Some(json!({ "role": "viewer", "max_uses": 1, "needs_approval": false })),
        )
        .await;
    let code = invite["code"].as_str().unwrap();
    let (_, joined) = server.call("POST", &format!("/v2/invites/{code}/accept"), Some(&bob), None).await;
    assert_eq!(joined["state"], "joined");
    let (_, preview) = server.call("GET", &format!("/v2/invites/{code}"), None, None).await;
    assert_eq!((preview["valid"].as_bool(), preview["reason"].as_str()), (Some(false), Some("used_up")));

    let (_, second) = server.call("POST", &format!("/v2/teams/{team_id}/invites"), Some(&alice), Some(json!({ "role": "viewer" }))).await;
    let (_, list) = server.call("GET", &format!("/v2/teams/{team_id}/invites"), Some(&alice), None).await;
    assert_eq!(list["invites"].as_array().unwrap().len(), 1);
    assert!(list["invites"][0].get("code").is_none());
    let (status, _) =
        server.call("DELETE", &format!("/v2/teams/{team_id}/invites/{}", second["id"].as_str().unwrap()), Some(&alice), None).await;
    assert_eq!(status, 204);
    let (_, preview) = server.call("GET", &format!("/v2/invites/{}", second["code"].as_str().unwrap()), None, None).await;
    assert_eq!(preview["reason"], "revoked");
    let (_, preview) = server.call("GET", "/v2/invites/not-a-code", None, None).await;
    assert_eq!(preview["reason"], "not_found");

    // A second device for Alice, then taken away: its session stops working at once.
    let other = server.sign_in(9, "100", "alice").await;
    let other_token = other["token"].as_str().unwrap();
    let (_, devices) = server.call("GET", "/v2/me/devices", Some(&alice), None).await;
    let devices = devices["devices"].as_array().unwrap().clone();
    assert_eq!(devices.len(), 2);
    let gone = devices.iter().find(|device| device["this"] == false).unwrap()["id"].as_str().unwrap().to_string();
    let (status, _) = server.call("DELETE", &format!("/v2/me/devices/{gone}"), Some(&alice), None).await;
    assert_eq!(status, 204);
    let (status, _) = server.call("GET", "/v2/me", Some(other_token), None).await;
    assert_eq!(status, 401);

    // Owners can't delete their account; others can, and their handle stays reserved.
    let (status, bad) = server.call("DELETE", "/v2/me", Some(&alice), None).await;
    assert_eq!((status, bad["error"].as_str()), (409, Some("owns_team")));
    let (status, _) = server.call("DELETE", "/v2/me", Some(&bob), None).await;
    assert_eq!(status, 204);
    let (_, check) = server.call("GET", "/v2/handles/bob", None, None).await;
    assert_eq!(check["reason"], "taken");
    let (status, _) = server.call("POST", "/v2/auth/logout", Some(&alice), None).await;
    assert_eq!(status, 204);
    let (status, _) = server.call("GET", "/v2/me", Some(&alice), None).await;
    assert_eq!(status, 401);
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn servers_and_grants() {
    let dir = temp("grants");
    let server = server(&dir).await;
    let (alice, _) = server.sign_up(11, "1100", "alice").await;
    let (bob, bob_id) = server.sign_up(12, "1200", "bob").await;
    let alice_device = relay_proto::DeviceId::of(&SigningKey::from_bytes(&[11; 32]).verifying_key().to_bytes());
    let bob_device = relay_proto::DeviceId::of(&SigningKey::from_bytes(&[12; 32]).verifying_key().to_bytes());

    let (_, team) = server.call("POST", "/v2/teams", Some(&alice), Some(json!({ "name": "Crew" }))).await;
    let team_id = team["id"].as_str().unwrap().to_string();
    let (_, invite) = server
        .call("POST", &format!("/v2/teams/{team_id}/invites"), Some(&alice), Some(json!({ "role": "developer", "needs_approval": false })))
        .await;
    let (status, joined) = server.call("POST", &format!("/v2/invites/{}/accept", invite["code"].as_str().unwrap()), Some(&bob), None).await;
    assert_eq!(status, 200, "{joined}");

    // A developer can't attach servers; the owner attaches one from the device that runs it.
    let (status, _) =
        server.call("POST", &format!("/v2/teams/{team_id}/servers"), Some(&bob), Some(json!({ "instance_id": "x", "name": "X" }))).await;
    assert_eq!(status, 403);
    let (status, team) = server
        .call(
            "POST",
            &format!("/v2/teams/{team_id}/servers"),
            Some(&alice),
            Some(json!({ "instance_id": "survival-1", "name": "Survival" })),
        )
        .await;
    assert_eq!(status, 200, "{team}");
    assert_eq!(team["servers"][0]["host"], "alice");
    assert_eq!(team["servers"][0]["mine"], true);
    let server_id = team["servers"][0]["id"].as_str().unwrap().to_string();
    let version = team["version"].as_u64().unwrap();
    // The same server can't go in twice.
    let (status, _) = server
        .call("POST", &format!("/v2/teams/{team_id}/servers"), Some(&alice), Some(json!({ "instance_id": "survival-1", "name": "Again" })))
        .await;
    assert_eq!(status, 409);

    // The host says where it is; bob gets a grant for alice's device with that address.
    let (status, _) = server.call("PUT", "/v2/me/addr", Some(&alice), Some(json!({ "addr": { "id": "abc", "addrs": [] } }))).await;
    assert_eq!(status, 204);
    let (status, got) = server.call("POST", "/v2/grants", Some(&bob), Some(json!({ "team": team_id }))).await;
    assert_eq!(status, 200, "{got}");
    assert_eq!(got["grants"].as_array().unwrap().len(), 1);
    let one = &got["grants"][0];
    assert_eq!(one["host"], "alice");
    assert_eq!(one["addr"]["id"], "abc");
    let (_, keys) = server.call("GET", "/v2/keys", None, None).await;
    let public: [u8; 32] = (0..32)
        .map(|at| u8::from_str_radix(&keys["keys"][0]["public_key"].as_str().unwrap()[at * 2..at * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let grant = relay_proto::grant::open(one["grant"].as_str().unwrap(), &[(1, public)]).unwrap();
    assert_eq!(grant.handle, "bob");
    assert_eq!(grant.user, bob_id);
    assert_eq!(grant.role, relay_proto::grant::Role::Developer);
    assert_eq!(grant.device, bob_device);
    assert_eq!(grant.host, alice_device);
    assert_eq!(grant.servers, vec!["survival-1".to_string()]);
    assert_eq!(grant.team_version, version);
    assert_eq!(grant.check(&bob_device, &alice_device, now(), version), Ok(()));
    // The host gets no grant for its own servers.
    let (_, own) = server.call("POST", "/v2/grants", Some(&alice), Some(json!({ "team": team_id }))).await;
    assert_eq!(own["grants"].as_array().unwrap().len(), 0);

    // Only the host device may report what happened on its server.
    let (status, _) = server
        .call("POST", &format!("/v2/teams/{team_id}/activity"), Some(&bob), Some(json!({ "server": server_id, "kind": "server_started" })))
        .await;
    assert_eq!(status, 403);
    let (status, _) = server
        .call("POST", &format!("/v2/teams/{team_id}/activity"), Some(&alice), Some(json!({ "server": server_id, "kind": "rm_rf" })))
        .await;
    assert_eq!(status, 400);
    let (status, _) = server
        .call(
            "POST",
            &format!("/v2/teams/{team_id}/activity"),
            Some(&alice),
            Some(json!({ "server": server_id, "kind": "file_changed", "data": { "path": "server.properties" } })),
        )
        .await;
    assert_eq!(status, 204);
    let (_, activity) = server.call("GET", &format!("/v2/teams/{team_id}/activity"), Some(&bob), None).await;
    assert_eq!(activity["items"][0]["kind"], "file_changed");
    assert_eq!(activity["items"][0]["data"]["server"], "Survival");

    // Lowering bob's role raises the version, so the old grant is now outdated at the host.
    let (_, team) =
        server.call("PATCH", &format!("/v2/teams/{team_id}/members/{bob_id}"), Some(&alice), Some(json!({ "role": "viewer" }))).await;
    let newer = team["version"].as_u64().unwrap();
    assert!(newer > version);
    assert_eq!(grant.check(&bob_device, &alice_device, now(), newer), Err(relay_proto::grant::Refused::Outdated));

    // Detaching leaves nothing to grant.
    let (status, _) = server.call("DELETE", &format!("/v2/teams/{team_id}/servers/{server_id}"), Some(&bob), None).await;
    assert_eq!(status, 403);
    let (status, _) = server.call("DELETE", &format!("/v2/teams/{team_id}/servers/{server_id}"), Some(&alice), None).await;
    assert_eq!(status, 204);
    let (_, got) = server.call("POST", "/v2/grants", Some(&bob), Some(json!({ "team": team_id }))).await;
    assert_eq!(got["grants"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn nodes_pair_and_host_servers() {
    let dir = temp("nodes");
    let server = server(&dir).await;
    let (alice, _) = server.sign_up(21, "2100", "alice").await;
    let (bob, _) = server.sign_up(22, "2200", "bob").await;
    let (_, team) = server.call("POST", "/v2/teams", Some(&alice), Some(json!({ "name": "Crew" }))).await;
    let team_id = team["id"].as_str().unwrap().to_string();
    let (_, invite) = server
        .call("POST", &format!("/v2/teams/{team_id}/invites"), Some(&alice), Some(json!({ "role": "developer", "needs_approval": false })))
        .await;
    server.call("POST", &format!("/v2/invites/{}/accept", invite["code"].as_str().unwrap()), Some(&bob), None).await;

    // Alice gets a code; the node signs it with its own key.
    let (status, made) = server.call("POST", "/v2/me/nodes", Some(&alice), None).await;
    assert_eq!(status, 200, "{made}");
    let code = made["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 10);
    let node_key = SigningKey::from_bytes(&[77; 32]);
    let public = hex(&node_key.verifying_key().to_bytes());
    let node_device = relay_proto::DeviceId::of(&node_key.verifying_key().to_bytes());
    let sign = |text: &str| hex(&node_key.sign(relay::accounts::node_pair_message(text).as_bytes()).to_bytes());
    // A wrong signature, then a code nobody made, are refused.
    let (status, _) = server
        .call(
            "POST",
            "/v2/nodes/pair",
            None,
            Some(json!({ "code": code, "public_key": public, "signature": sign("OTHER"), "name": "box" })),
        )
        .await;
    assert_eq!(status, 400);
    let (status, _) = server
        .call(
            "POST",
            "/v2/nodes/pair",
            None,
            Some(json!({ "code": "ZZZZZZZZZZ", "public_key": public, "signature": sign("ZZZZZZZZZZ"), "name": "box" })),
        )
        .await;
    assert_eq!(status, 400);
    let (status, paired) = server
        .call(
            "POST",
            "/v2/nodes/pair",
            None,
            Some(json!({ "code": code.to_lowercase(), "public_key": public, "signature": sign(&code), "name": "box" })),
        )
        .await;
    assert_eq!(status, 200, "{paired}");
    assert_eq!(paired["device"], node_device.text());
    let node = paired["token"].as_str().unwrap().to_string();
    // The code works once.
    let (status, _) = server
        .call("POST", "/v2/nodes/pair", None, Some(json!({ "code": code, "public_key": public, "signature": sign(&code), "name": "box" })))
        .await;
    assert_eq!(status, 400);
    let (_, devices) = server.call("GET", "/v2/me/devices", Some(&alice), None).await;
    assert!(devices["devices"].as_array().unwrap().iter().any(|one| one["id"] == node_device.text() && one["kind"] == "node"), "{devices}");

    // Alice attaches a server that lives on the node; bob can't name alice's node.
    let (status, _) = server
        .call(
            "POST",
            &format!("/v2/teams/{team_id}/servers"),
            Some(&alice),
            Some(json!({ "instance_id": "survival", "name": "Survival", "host": "not-a-node" })),
        )
        .await;
    assert_eq!(status, 403);
    let (status, team) = server
        .call(
            "POST",
            &format!("/v2/teams/{team_id}/servers"),
            Some(&alice),
            Some(json!({ "instance_id": "survival", "name": "Survival", "host": node_device.text() })),
        )
        .await;
    assert_eq!(status, 200, "{team}");
    let server_id = team["servers"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(team["servers"][0]["node"], true);
    assert_eq!(team["servers"][0]["host"], "alice");
    assert_eq!(team["servers"][0]["host_online"], false, "no address yet");

    // The node says where it is with its own session, and bob's grant is for the node.
    let (status, _) = server.call("PUT", "/v2/me/addr", Some(&node), Some(json!({ "addr": { "id": "node-addr", "addrs": [] } }))).await;
    assert_eq!(status, 204);
    let (_, got) = server.call("POST", "/v2/grants", Some(&bob), Some(json!({ "team": team_id }))).await;
    let one = &got["grants"][0];
    assert_eq!(one["addr"]["id"], "node-addr", "{got}");
    let (_, keys) = server.call("GET", "/v2/keys", None, None).await;
    let key: [u8; 32] = (0..32)
        .map(|at| u8::from_str_radix(&keys["keys"][0]["public_key"].as_str().unwrap()[at * 2..at * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let grant = relay_proto::grant::open(one["grant"].as_str().unwrap(), &[(1, key)]).unwrap();
    assert_eq!(grant.host, node_device);
    assert_eq!(grant.servers, vec!["survival".to_string()]);
    // The node reports activity on its server.
    let (status, _) = server
        .call("POST", &format!("/v2/teams/{team_id}/activity"), Some(&node), Some(json!({ "server": server_id, "kind": "server_started" })))
        .await;
    assert_eq!(status, 204);

    // The iroh relay asks whether an endpoint may use it: the node may, a stranger may not, and a call
    // that came through Caddy (X-Forwarded-For) is not answered.
    let access = |id: String, forwarded: bool| {
        let mut request = server.http.post(format!("{}/v2/relay-access", server.base)).header("X-Iroh-Endpoint-Id", id);
        if forwarded {
            request = request.header("X-Forwarded-For", "203.0.113.9");
        }
        async move {
            let answer = request.send().await.unwrap();
            (answer.status().as_u16(), answer.text().await.unwrap())
        }
    };
    assert_eq!(access(public.clone(), false).await, (200, "true".to_string()));
    assert_eq!(access(hex(&[5; 32]), false).await.0, 403);
    assert_eq!(access("not hex".into(), false).await.0, 403);
    assert_eq!(access(public.clone(), true).await.0, 404);

    // Revoking the node drops its servers and its session.
    let (status, _) = server.call("DELETE", &format!("/v2/me/devices/{}", node_device.text()), Some(&alice), None).await;
    assert_eq!(status, 204);
    assert_eq!(access(public.clone(), false).await.0, 403, "a removed node may not use the relay");
    let (_, got) = server.call("POST", "/v2/grants", Some(&bob), Some(json!({ "team": team_id }))).await;
    assert_eq!(got["grants"].as_array().unwrap().len(), 0);
    let (status, _) = server.call("PUT", "/v2/me/addr", Some(&node), Some(json!({ "addr": { "id": "x" } }))).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn numbers_and_person_invites() {
    let dir = temp("numbers");
    let server = server(&dir).await;
    let (alice, _) = server.sign_up(1, "100", "alice").await;
    let (bob, bob_id) = server.sign_up(2, "200", "bob").await;
    let (carol, _) = server.sign_up(3, "300", "carol").await;

    // Everyone gets a permanent 8-digit number, all different.
    let mut numbers = Vec::new();
    for token in [&alice, &bob, &carol] {
        let (_, me) = server.call("GET", "/v2/me", Some(token), None).await;
        let number = me["user"]["number"].as_u64().unwrap();
        assert!((10_000_000..100_000_000).contains(&number), "{number}");
        numbers.push(number);
    }
    numbers.dedup();
    assert_eq!(numbers.len(), 3);

    let (_, team) = server.call("POST", "/v2/teams", Some(&alice), Some(json!({ "name": "Build" }))).await;
    let team_id = team["id"].as_str().unwrap();
    let members_path = format!("/v2/teams/{team_id}/members");
    let invite = |who: Value| server.call("POST", &members_path, Some(&alice), Some(json!({ "who": who, "role": "developer" })));

    // By handle (with or without @) and by number.
    let (status, sent) = invite(json!("@bob")).await;
    assert_eq!(status, 200, "{sent}");
    assert_eq!(sent["user"]["id"], bob_id.as_str());
    let (status, _) = invite(json!(numbers[2].to_string())).await;
    assert_eq!(status, 200);
    let (status, bad) = invite(json!("nobody")).await;
    assert_eq!((status, bad["error"].as_str()), (404, Some("no_such_user")));

    // Bob sees it and joins; Carol declines.
    let (_, mine) = server.call("GET", "/v2/me/invites", Some(&bob), None).await;
    assert_eq!(mine["invites"][0]["name"], "Build");
    assert_eq!(mine["invites"][0]["by"], "alice");
    let (_, answer) = server.call("POST", &format!("/v2/me/invites/{team_id}"), Some(&bob), Some(json!({ "accept": true }))).await;
    assert_eq!(answer["state"], "joined");
    let (_, answer) = server.call("POST", &format!("/v2/me/invites/{team_id}"), Some(&carol), Some(json!({ "accept": false }))).await;
    assert_eq!(answer["state"], "declined");
    let (_, full) = server.call("GET", &format!("/v2/teams/{team_id}"), Some(&alice), None).await;
    let members: Vec<&str> = full["members"].as_array().unwrap().iter().map(|one| one["handle"].as_str().unwrap()).collect();
    assert_eq!(members, ["alice", "bob"]);
    let (status, _) = server.call("POST", &format!("/v2/me/invites/{team_id}"), Some(&carol), Some(json!({ "accept": true }))).await;
    assert_eq!(status, 404);

    // Already in: refused. Developers can't invite.
    let (status, bad) = invite(json!("bob")).await;
    assert_eq!((status, bad["error"].as_str()), (409, Some("already_member")));
    let (status, _) = server.call("POST", &members_path, Some(&bob), Some(json!({ "who": "carol", "role": "viewer" }))).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn crash_reports_are_checked_limited_and_stored() {
    let dir = temp("crash");
    let server = server(&dir).await;
    let report = json!({ "version": "2.0.0", "os": "windows", "kind": "panic", "message": "boom", "detail": "at main.rs:1" });
    let (status, answer) = server.call("POST", "/v2/crash", None, Some(report.clone())).await;
    assert_eq!(status, 200, "{answer}");
    assert!(answer["id"].as_i64().unwrap() > 0);

    let (status, answer) =
        server.call("POST", "/v2/crash", None, Some(json!({ "version": "2.0.0", "os": "windows", "kind": "other", "message": "x" }))).await;
    assert_eq!((status, answer["error"].as_str()), (400, Some("bad_crash")));
    let big = json!({ "version": "2.0.0", "os": "windows", "kind": "ui", "message": "m".repeat(1001) });
    let (status, answer) = server.call("POST", "/v2/crash", None, Some(big)).await;
    assert_eq!((status, answer["error"].as_str()), (400, Some("crash_too_big")));
    // The router caps a body at 16 KB.
    let huge = json!({ "version": "2.0.0", "os": "windows", "kind": "ui", "message": "m", "detail": "d".repeat(20_000) });
    let (status, _) = server.call("POST", "/v2/crash", None, Some(huge)).await;
    assert_eq!(status, 413);

    for _ in 0..2 {
        assert_eq!(server.call("POST", "/v2/crash", None, Some(report.clone())).await.0, 200);
    }
    let (status, answer) = server.call("POST", "/v2/crash", None, Some(report.clone())).await;
    assert_eq!((status, answer["error"].as_str()), (429, Some("slow_down")));

    let db = rusqlite::Connection::open(dir.0.join("accounts.db")).unwrap();
    let stored: i64 = db.query_row("SELECT COUNT(*) FROM crash_reports WHERE message = 'boom'", [], |row| row.get(0)).unwrap();
    assert_eq!(stored, 3);
}

#[tokio::test]
async fn app_connection_hello_updates_and_counts() {
    let dir = temp("app");
    let server = server(&dir).await;
    let url = format!("{}/v2/app", server.base.replace("http://", "ws://"));
    let install = "0123456789abcdef0123456789abcdef";

    let (socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let mut app = Live { socket };
    let hello = json!({ "type": "hello", "app": "1.0.2", "addon": 41, "fork": 0, "os": "windows-x86_64", "install": install });
    app.socket.send(Message::Text(hello.to_string().into())).await.unwrap();
    assert_eq!(app.next().await, json!({ "type": "updates", "addon": 0, "fork": 0 }));
    app.socket.send(Message::Text(json!({ "type": "ping" }).to_string().into())).await.unwrap();
    assert_eq!(app.next().await["type"], "pong");

    // With usage counts off nothing is stored, but the app still gets notices.
    let (socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let mut quiet = Live { socket };
    let hello = json!({ "type": "hello", "app": "1.0.2", "addon": 41, "fork": 0, "os": "linux-x86_64", "install": null });
    quiet.socket.send(Message::Text(hello.to_string().into())).await.unwrap();
    assert_eq!(quiet.next().await["type"], "updates");

    // A bad install id is refused.
    let (socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let mut bad = Live { socket };
    let hello = json!({ "type": "hello", "app": "1.0.2", "os": "windows-x86_64", "install": "ABC" });
    bad.socket.send(Message::Text(hello.to_string().into())).await.unwrap();
    assert_eq!(bad.next().await, json!({ "type": "error", "code": "bad_hello" }));

    let stats = server._accounts.store.stats(now()).unwrap();
    assert_eq!((stats.today, stats.installs), (1, 1));
    assert_eq!(stats.by_os, vec![("windows-x86_64".to_string(), 1)]);

    // Only from this machine, without X-Forwarded-For (as Caddy adds).
    let through_caddy =
        server.http.post(format!("{}/v2/admin/notify-updates", server.base)).header("x-forwarded-for", "203.0.113.9").send().await.unwrap();
    assert_eq!(through_caddy.status().as_u16(), 404);
    let local = server.http.post(format!("{}/v2/admin/notify-updates", server.base)).send().await.unwrap();
    assert_eq!(local.status().as_u16(), 204);
}

#[tokio::test]
async fn admin_page_needs_an_admin() {
    let dir = temp("admin");
    let id = {
        let first = server(&dir).await;
        let (_, id) = first.sign_up(31, "301", "boss").await;
        first.sign_up(32, "302", "someone").await;
        id
    };
    let server = server_with(&dir, vec![id]).await;
    let browser = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();

    let page = browser.get(format!("{}/admin", server.base)).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    assert_eq!(page.headers()["x-frame-options"], "DENY");
    assert!(page.headers()["content-security-policy"].to_str().unwrap().contains("default-src 'none'"));
    assert!(page.text().await.unwrap().contains("/admin/login"));

    // Someone who isn't an admin gets 403 and no cookie.
    let other = browser.get(format!("{}/admin/login?subject=302", server.base)).send().await.unwrap();
    assert_eq!(other.status().as_u16(), 403);
    assert!(other.headers().get("set-cookie").is_none());

    let login = browser.get(format!("{}/admin/login?subject=301", server.base)).send().await.unwrap();
    assert_eq!(login.status().as_u16(), 303);
    let cookie = login.headers()["set-cookie"].to_str().unwrap().to_string();
    for part in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/admin", "Max-Age=43200"] {
        assert!(cookie.contains(part), "{cookie}");
    }
    let pair = cookie.split(';').next().unwrap().to_string();
    let page = browser.get(format!("{}/admin", server.base)).header("cookie", &pair).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    assert_eq!(page.headers()["cache-control"], "no-store");
    let text = page.text().await.unwrap();
    assert!(text.contains("Usage") && text.contains("@boss") && text.contains("Signed-in devices"), "{text}");

    let out = browser.post(format!("{}/admin/logout", server.base)).header("cookie", &pair).send().await.unwrap();
    assert_eq!(out.status().as_u16(), 303);
    let page = browser.get(format!("{}/admin", server.base)).header("cookie", &pair).send().await.unwrap();
    assert!(page.text().await.unwrap().contains("/admin/login"));
}
