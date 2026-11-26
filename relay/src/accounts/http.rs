//! The account server's HTTP side: sign-in, handles, teams, members, invites, activity, the
//! WebSocket upgrade and the few web pages. JSON in and out; errors are `{"error": "<code>"}` and the
//! app turns codes into words. Section 4 of `docs/monolink.md`.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, Query, State, WebSocketUpgrade};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use relay_proto::DeviceId;
use serde::Deserialize;
use serde_json::{Value, json};

use super::store::{Activity, CrashReport, HANDLE_COOLDOWN, Invite, Joined, User};
use super::util::{self, DAY, Role, check_handle, hex, now, sha256};
use super::{Login, LoginResult, Shared, may_remove, may_set_role, pages};

pub fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route("/admin", get(super::admin::admin_page))
        .route("/admin/login", get(super::admin::admin_login))
        .route("/admin/logout", post(super::admin::admin_logout))
        .route("/v2/admin/notify-updates", post(super::admin::notify_updates))
        .route("/v2/app", get(app_ws))
        .route("/invite/{code}", get(invite_page))
        .route("/auth/callback/{provider}", get(auth_callback))
        .route("/auth/done", get(auth_done))
        .route("/auth/{provider}", get(auth_start))
        .route("/v2/auth/begin", post(auth_begin))
        .route("/v2/auth/poll", post(auth_poll))
        .route("/v2/auth/finish", post(auth_finish))
        .route("/v2/auth/logout", post(logout))
        .route("/v2/handles/{handle}", get(handle_check))
        .route("/v2/me", get(me_get).patch(me_patch).delete(me_delete))
        .route("/v2/me/devices", get(devices_get))
        .route("/v2/me/devices/{id}", axum::routing::delete(device_delete))
        .route("/v2/me/nodes", post(node_code))
        .route("/v2/nodes/pair", post(node_pair))
        .route("/v2/teams", post(team_create))
        .route("/v2/teams/{team}", get(team_get).patch(team_patch).delete(team_delete))
        .route("/v2/teams/{team}/transfer", post(team_transfer))
        .route("/v2/teams/{team}/members", post(member_invite))
        .route("/v2/teams/{team}/members/{user}", patch(member_patch).delete(member_delete))
        .route("/v2/me/invites", get(my_invites))
        .route("/v2/me/invites/{team}", post(my_invite_answer))
        .route("/v2/teams/{team}/invites", get(invites_list).post(invite_create))
        .route("/v2/teams/{team}/invites/{id}", axum::routing::delete(invite_delete))
        .route("/v2/teams/{team}/requests", get(requests_list))
        .route("/v2/teams/{team}/requests/{user}", post(request_answer))
        .route("/v2/teams/{team}/activity", get(activity_list).post(activity_post))
        .route("/v2/teams/{team}/servers", post(server_attach))
        .route("/v2/teams/{team}/servers/{id}", axum::routing::delete(server_detach))
        .route("/v2/me/addr", axum::routing::put(addr_put))
        .route("/v2/grants", post(grants))
        .route("/v2/keys", get(keys))
        .route("/v2/invites/{code}", get(invite_preview))
        .route("/v2/invites/{code}/accept", post(invite_accept))
        .route("/v2/ws", get(ws))
        .route("/v2/relay-access", post(relay_access))
        .route("/v2/crash", post(crash_report))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not_found") })
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::map_response(security_headers))
        .with_state(shared)
}

/// Headers for every answer; web pages also get a policy that allows only their own inline styles
/// (no scripts, no outside requests, no framing).
async fn security_headers(mut response: Response) -> Response {
    let html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    let headers = response.headers_mut();
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    // Invite codes and sign-in ids are in some addresses: never pass them on.
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    if html {
        headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; img-src 'self' data:; form-action 'self'; base-uri 'none'; frame-ancestors 'none'"),
        );
    }
    response
}

// ── errors and helpers ─────────────────────────────────────────────────────

pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    extra: Option<Value>,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code, extra: None }
    }
    fn bad(code: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code)
    }
    fn forbidden(code: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, code)
    }
    fn conflict(code: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, code)
    }
    fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found")
    }
    fn slow() -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "slow_down")
    }
    fn with(mut self, extra: Value) -> Self {
        self.extra = Some(extra);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "error": self.code });
        if let (Some(Value::Object(extra)), Value::Object(map)) = (self.extra, &mut body) {
            map.extend(extra);
        }
        (self.status, Json(body)).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        eprintln!("[accounts] database: {error}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "server")
    }
}

type Answer = Result<Json<Value>, ApiError>;

/// The caller's address. Behind Caddy (we listen on loopback) the first X-Forwarded-For entry is the
/// real one; Caddy replaces whatever the client sent there.
pub(crate) fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> String {
    if peer.ip().is_loopback()
        && let Some(first) =
            headers.get("x-forwarded-for").and_then(|value| value.to_str().ok()).and_then(|forwarded| forwarded.split(',').next())
    {
        return first.trim().to_string();
    }
    peer.ip().to_string()
}

pub fn user_json(user: &User) -> Value {
    json!({
        "id": user.id,
        "handle": user.handle,
        "number": user.number,
        "avatar": user.avatar,
        "handle_change_after": user.handle_changed_at + HANDLE_COOLDOWN,
    })
}

pub fn activity_json(shared: &Shared, item: &Activity) -> Value {
    let actor = item.actor.as_deref().and_then(|id| shared.store.user(id).ok().flatten()).map(|user| user.handle);
    json!({
        "id": item.id,
        "actor": actor,
        "kind": item.kind,
        "data": serde_json::from_str::<Value>(&item.data).unwrap_or(Value::Null),
        "at": item.at,
    })
}

fn invite_json(shared: &Shared, invite: &Invite) -> Value {
    let by = shared.store.user(&invite.created_by).ok().flatten().map(|user| user.handle);
    json!({
        "id": hex(&invite.code_hash),
        "prefix": invite.prefix,
        "role": invite.role.as_str(),
        "created_by": by,
        "created_at": invite.created_at,
        "expires": invite.expires,
        "max_uses": invite.max_uses,
        "uses": invite.uses,
        "needs_approval": invite.needs_approval,
    })
}

/// A team name: trimmed, 1–40 characters, no control characters.
fn team_name(text: &str) -> Result<String, ApiError> {
    let name = text.trim();
    let count = name.chars().count();
    if count == 0 || count > 40 || name.chars().any(char::is_control) {
        return Err(ApiError::bad("bad_name"));
    }
    Ok(name.to_string())
}

/// Closes a device's live connections and tells teammates of anyone now offline.
fn kick(shared: &Shared, device: &str) {
    for person in shared.hub.kick_device(device) {
        let teammates = shared.store.teammates(&person).unwrap_or_default();
        shared.hub.send(&teammates, &json!({ "type": "presence", "user": person, "online": false }));
    }
}

fn teams_changed_for(shared: &Shared, user: &str) {
    for (team, _) in shared.store.teams_of(user).unwrap_or_default() {
        shared.team_changed(&team.id, &[]);
    }
}

// ── who is calling ─────────────────────────────────────────────────────────

pub struct Me {
    user: User,
    device: String,
    token_hash: Vec<u8>,
}

impl FromRequestParts<Arc<Shared>> for Me {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, shared: &Arc<Shared>) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(ApiError::new(StatusCode::UNAUTHORIZED, "signed_out"))?;
        let token_hash = sha256(token.trim());
        let (user, device) = shared.store.session(&token_hash, now())?.ok_or(ApiError::new(StatusCode::UNAUTHORIZED, "signed_out"))?;
        Ok(Me { user, device, token_hash })
    }
}

/// The caller's role in a team, or 404 when they aren't in it (so team ids don't leak).
fn role_in(shared: &Shared, team: &str, me: &Me) -> Result<Role, ApiError> {
    shared.store.role(team, &me.user.id)?.ok_or_else(ApiError::not_found)
}

fn manager_in(shared: &Shared, team: &str, me: &Me) -> Result<Role, ApiError> {
    let role = role_in(shared, team, me)?;
    if !role.manages() {
        return Err(ApiError::forbidden("forbidden"));
    }
    Ok(role)
}

// ── sign-in ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Begin {
    provider: String,
    public_key: String,
    device_name: String,
    at: u64,
    signature: String,
    /// Where the app listens on 127.0.0.1 for the end of the sign-in (see `Login::port`).
    port: u16,
}

/// What a device signs to start a sign-in: proves the device key is really there, so a sign-in can't be
/// started for someone else's device.
pub fn login_message(provider: &str, at: u64) -> String {
    format!("monolab-login-v1\n{provider}\n{at}")
}

async fn auth_begin(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Begin>,
) -> Answer {
    if !shared.limits.allow(&format!("begin:{}", client_ip(&headers, peer)), 10, 60) {
        return Err(ApiError::slow());
    }
    let provider: &'static str = match body.provider.as_str() {
        "discord" if shared.discord.is_some() => "discord",
        "fake" if shared.fake_sign_in => "fake",
        _ => return Err(ApiError::bad("bad_provider")),
    };
    if body.port < 1024 {
        return Err(ApiError::bad("bad_port"));
    }
    let now = now();
    if body.at.abs_diff(now) > 300 {
        return Err(ApiError::bad("clock"));
    }
    let key: [u8; 32] = util::unhex(&body.public_key).and_then(|bytes| bytes.try_into().ok()).ok_or(ApiError::bad("bad_key"))?;
    let signature: [u8; 64] = util::unhex(&body.signature).and_then(|bytes| bytes.try_into().ok()).ok_or(ApiError::bad("bad_signature"))?;
    let verifying = VerifyingKey::from_bytes(&key).map_err(|_| ApiError::bad("bad_key"))?;
    verifying
        .verify(login_message(provider, body.at).as_bytes(), &Signature::from_bytes(&signature))
        .map_err(|_| ApiError::bad("bad_signature"))?;
    let device = DeviceId::of(&key).text();
    if !shared.store.device_seen(&device, &key, &relay_proto::clean_name(&body.device_name), now)? {
        return Err(ApiError::forbidden("device_revoked"));
    }
    shared.sweep(now);
    let login_id = util::new_secret();
    let poll_secret = util::new_secret();
    shared.logins.lock().expect("logins poisoned").insert(
        login_id.clone(),
        Login {
            device,
            provider,
            poll_hash: sha256(&poll_secret),
            state: util::new_secret(),
            verifier: util::new_secret(),
            created: now,
            result: None,
            port: body.port,
            proof: util::new_secret(),
            identity: None,
        },
    );
    let url = format!("{}/auth/{provider}?login={login_id}", shared.public_url);
    Ok(Json(json!({ "login_id": login_id, "poll_secret": poll_secret, "url": url })))
}

#[derive(Deserialize)]
struct StartQuery {
    login: String,
    /// Pretend sign-in only.
    subject: Option<String>,
    name: Option<String>,
}

async fn auth_start(State(shared): State<Arc<Shared>>, Path(provider): Path<String>, Query(query): Query<StartQuery>) -> Response {
    let found = shared
        .logins
        .lock()
        .expect("logins poisoned")
        .get(&query.login)
        .filter(|login| login.provider == provider && login.result.is_none())
        .map(|login| (login.state.clone(), login.verifier.clone()));
    let Some((state, verifier)) = found else {
        return Html(pages::failed("expired")).into_response();
    };
    match provider.as_str() {
        "discord" => match &shared.discord {
            Some(discord) => Redirect::to(&discord.authorize_url(&state, &verifier, &shared.redirect_url("discord"))).into_response(),
            None => Html(pages::failed("server")).into_response(),
        },
        "fake" if shared.fake_sign_in => {
            let subject = query.subject.unwrap_or_else(|| "1".into());
            let name = query.name.unwrap_or_else(|| format!("user{subject}"));
            let identity = super::oauth::Identity { subject, name, avatar: None };
            back_to_app(&shared, &query.login, identity)
        }
        _ => Html(pages::failed("server")).into_response(),
    }
}

/// Discord said who this is: keep it, and send the browser to the app that started the sign-in with
/// the proof it needs to collect the session. The app then sends the browser on to `/auth/done`.
fn back_to_app(shared: &Shared, login_id: &str, identity: super::oauth::Identity) -> Response {
    let mut logins = shared.logins.lock().expect("logins poisoned");
    let Some(login) = logins.get_mut(login_id).filter(|login| login.result.is_none() && login.identity.is_none()) else {
        return Html(pages::failed("expired")).into_response();
    };
    login.identity = Some(identity);
    Redirect::to(&format!("http://127.0.0.1:{}/monolab-login?login={login_id}&proof={}", login.port, login.proof)).into_response()
}

async fn auth_done() -> Html<String> {
    Html(pages::signed_in())
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn auth_callback(State(shared): State<Arc<Shared>>, Path(provider): Path<String>, Query(query): Query<CallbackQuery>) -> Response {
    let Some(discord) = shared.discord.as_ref().filter(|_| provider == "discord") else {
        return Html(pages::failed("server")).into_response();
    };
    let state = query.state.unwrap_or_default();
    // A browser sign-in to /admin (same Discord callback, its own state).
    if !state.is_empty()
        && let Some(response) = super::admin::web_callback(&shared, &state, query.code.clone(), query.error.clone()).await
    {
        return response;
    }
    let found = shared
        .logins
        .lock()
        .expect("logins poisoned")
        .iter()
        .find(|(_, login)| !state.is_empty() && login.state == state && login.result.is_none() && login.identity.is_none())
        .map(|(id, login)| (id.clone(), login.verifier.clone()));
    let Some((login_id, verifier)) = found else {
        return Html(pages::failed("expired")).into_response();
    };
    let fail = |reason: &'static str| {
        if let Some(login) = shared.logins.lock().expect("logins poisoned").get_mut(&login_id) {
            login.result = Some(LoginResult::Failed(reason));
        }
        Html(pages::failed(reason)).into_response()
    };
    let (Some(code), None) = (query.code, query.error) else {
        return fail("denied");
    };
    match discord.identify(&shared.http, &code, &verifier, &shared.redirect_url("discord")).await {
        Ok(identity) => back_to_app(&shared, &login_id, identity),
        Err(error) => {
            eprintln!("[accounts] discord: {error}");
            fail("server")
        }
    }
}

#[derive(Deserialize)]
struct Poll {
    login_id: String,
    poll_secret: String,
    /// What the browser brought to the app's 127.0.0.1 port (see `Login::port`).
    #[serde(default)]
    proof: String,
}

async fn auth_poll(State(shared): State<Arc<Shared>>, Json(body): Json<Poll>) -> Answer {
    if !shared.limits.allow(&format!("poll:{}", body.login_id), 60, 60) {
        return Err(ApiError::slow());
    }
    let identity = {
        let mut logins = shared.logins.lock().expect("logins poisoned");
        let Some(login) = logins.get(&body.login_id).filter(|login| login.poll_hash == sha256(&body.poll_secret)) else {
            return Ok(Json(json!({ "state": "failed", "reason": "expired" })));
        };
        if let Some(LoginResult::Failed(reason)) = login.result {
            logins.remove(&body.login_id);
            return Ok(Json(json!({ "state": "failed", "reason": reason })));
        }
        // Signed in at Discord, but the proof hasn't reached this app (yet, or ever: the link was
        // opened on another PC).
        let proven = !body.proof.is_empty() && sha256(&body.proof) == sha256(&login.proof);
        match &login.identity {
            Some(identity) if proven => identity.clone(),
            _ => return Ok(Json(json!({ "state": "waiting" }))),
        }
    };
    let answer = match shared.identified(&body.login_id, identity, now()) {
        LoginResult::SignedIn { token, user } => json!({ "state": "signed_in", "token": token, "user": user_json(&user) }),
        LoginResult::NeedsHandle { ticket, suggested } => json!({ "state": "needs_handle", "ticket": ticket, "suggested": suggested }),
        LoginResult::Failed(reason) => json!({ "state": "failed", "reason": reason }),
    };
    // Each result is handed out once.
    shared.logins.lock().expect("logins poisoned").remove(&body.login_id);
    Ok(Json(answer))
}

async fn handle_check(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(handle): Path<String>,
) -> Answer {
    if !shared.limits.allow(&format!("handle:{}", client_ip(&headers, peer)), 60, 60) {
        return Err(ApiError::slow());
    }
    Ok(Json(match check_handle(&handle) {
        Err(reason) => json!({ "ok": false, "reason": reason }),
        Ok(handle) if !shared.store.handle_free(&handle, None, now())? => json!({ "ok": false, "handle": handle, "reason": "taken" }),
        Ok(handle) => json!({ "ok": true, "handle": handle }),
    }))
}

#[derive(Deserialize)]
struct Finish {
    ticket: String,
    handle: String,
}

async fn auth_finish(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Finish>,
) -> Answer {
    if !shared.limits.allow(&format!("finish:{}", client_ip(&headers, peer)), 30, 60) {
        return Err(ApiError::slow());
    }
    let handle = check_handle(&body.handle).map_err(ApiError::bad)?;
    let now = now();
    shared.sweep(now);
    let ticket_hash = sha256(&body.ticket);
    let signup = shared.signups.lock().expect("signups poisoned").remove(&ticket_hash).ok_or(ApiError::bad("expired"))?;
    let Some(user) = shared.store.create_user(&handle, signup.avatar.as_deref(), signup.provider, &signup.subject, now)? else {
        // Taken meanwhile: keep the ticket so they can try another.
        shared.signups.lock().expect("signups poisoned").insert(ticket_hash, signup);
        return Err(ApiError::conflict("taken"));
    };
    let token = shared.new_session(&user.id, &signup.device, now)?.ok_or(ApiError::conflict("too_many_devices"))?;
    Ok(Json(json!({ "token": token, "user": user_json(&user) })))
}

async fn logout(State(shared): State<Arc<Shared>>, me: Me) -> Result<StatusCode, ApiError> {
    shared.store.close_session(&me.token_hash)?;
    kick(&shared, &me.device);
    Ok(StatusCode::NO_CONTENT)
}

// ── me ─────────────────────────────────────────────────────────────────────

async fn me_get(State(shared): State<Arc<Shared>>, me: Me) -> Answer {
    let teams: Vec<Value> = shared
        .store
        .teams_of(&me.user.id)?
        .into_iter()
        .map(|(team, role)| json!({ "id": team.id, "name": team.name, "role": role.as_str(), "version": team.version }))
        .collect();
    Ok(Json(json!({ "user": user_json(&me.user), "device": me.device, "teams": teams })))
}

#[derive(Deserialize)]
struct MePatch {
    handle: String,
}

async fn me_patch(State(shared): State<Arc<Shared>>, me: Me, Json(body): Json<MePatch>) -> Answer {
    let handle = check_handle(&body.handle).map_err(ApiError::bad)?;
    if handle == me.user.handle {
        return Ok(Json(json!({ "user": user_json(&me.user) })));
    }
    let now = now();
    let next = me.user.handle_changed_at + HANDLE_COOLDOWN;
    if now < next {
        return Err(ApiError::conflict("too_soon").with(json!({ "next": next })));
    }
    if !shared.store.change_handle(&me.user, &handle, now)? {
        return Err(ApiError::conflict("taken"));
    }
    let user = shared.store.user(&me.user.id)?.ok_or_else(ApiError::not_found)?;
    for (team, _) in shared.store.teams_of(&user.id)? {
        shared.record(&team.id, Some(&user.id), "handle_changed", json!({ "from": me.user.handle, "to": user.handle }), now);
        shared.team_changed(&team.id, &[]);
    }
    Ok(Json(json!({ "user": user_json(&user) })))
}

async fn me_delete(State(shared): State<Arc<Shared>>, me: Me) -> Result<StatusCode, ApiError> {
    let now = now();
    let teams = shared.store.teams_of(&me.user.id)?;
    if teams.iter().any(|(_, role)| *role == Role::Owner) {
        return Err(ApiError::conflict("owns_team"));
    }
    let devices = shared.store.devices_of(&me.user.id)?;
    for (team, _) in &teams {
        shared.record(&team.id, None, "member_left", json!({ "user": me.user.handle }), now);
    }
    if !shared.store.delete_user(&me.user, now)? {
        return Err(ApiError::conflict("owns_team"));
    }
    for device in devices {
        kick(&shared, &device.id);
    }
    for (team, _) in teams {
        shared.team_changed(&team.id, &[]);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn devices_get(State(shared): State<Arc<Shared>>, me: Me) -> Answer {
    let devices: Vec<Value> = shared
        .store
        .devices_of(&me.user.id)?
        .into_iter()
        .map(|device| {
            json!({
                "id": device.id,
                "name": device.name,
                "created_at": device.created_at,
                "last_seen": device.last_seen,
                "kind": device.kind,
                "this": device.id == me.device,
            })
        })
        .collect();
    Ok(Json(json!({ "devices": devices })))
}

async fn device_delete(State(shared): State<Arc<Shared>>, me: Me, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    if !shared.store.revoke_device(&me.user.id, &id, now())? {
        return Err(ApiError::not_found());
    }
    kick(&shared, &id);
    // Hosts must stop trusting grants made for that device.
    teams_changed_for(&shared, &me.user.id);
    Ok(StatusCode::NO_CONTENT)
}

// ── monolab-node pairing ───────────────────────────────────────────────────

/// How long a node pairing code works.
const NODE_CODE_SECS: u64 = 10 * 60;

/// What a node signs to pair: proves its key is really on the machine that types the code.
pub fn node_pair_message(code: &str) -> String {
    format!("monolab-node-pair-v1\n{code}")
}

/// A one-time code the owner types on their Linux machine (`monolab-node pair <code>`) to add it as a node.
async fn node_code(State(shared): State<Arc<Shared>>, me: Me) -> Answer {
    if !shared.limits.allow(&format!("node-code:{}", me.user.id), 10, 60) {
        return Err(ApiError::slow());
    }
    // 10 letters and digits people can read out (no 0/O, 1/I/L).
    const LETTERS: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let secret = util::new_secret();
    let code: String = secret.bytes().take(10).map(|byte| LETTERS[byte as usize % LETTERS.len()] as char).collect();
    let now = now();
    let mut codes = shared.node_codes.lock().expect("node codes poisoned");
    codes.retain(|_, (_, until)| now < *until);
    codes.insert(sha256(&code), (me.user.id.clone(), now + NODE_CODE_SECS));
    Ok(Json(json!({ "code": code, "expires": now + NODE_CODE_SECS })))
}

#[derive(Deserialize)]
struct NodePair {
    code: String,
    public_key: String,
    signature: String,
    name: String,
}

/// A node turns in its code: its key becomes a device of the code's owner, of kind `node`, with a session
/// of its own (stretched each time it's used, like an app's).
async fn node_pair(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<NodePair>,
) -> Answer {
    if !shared.limits.allow(&format!("node-pair:{}", client_ip(&headers, peer)), 10, 60) {
        return Err(ApiError::slow());
    }
    let code = body.code.trim().to_ascii_uppercase();
    let key: [u8; 32] = util::unhex(&body.public_key).and_then(|bytes| bytes.try_into().ok()).ok_or(ApiError::bad("bad_key"))?;
    let signature: [u8; 64] = util::unhex(&body.signature).and_then(|bytes| bytes.try_into().ok()).ok_or(ApiError::bad("bad_signature"))?;
    let verifying = VerifyingKey::from_bytes(&key).map_err(|_| ApiError::bad("bad_key"))?;
    verifying
        .verify(node_pair_message(&code).as_bytes(), &Signature::from_bytes(&signature))
        .map_err(|_| ApiError::bad("bad_signature"))?;
    let now = now();
    let user = {
        let mut codes = shared.node_codes.lock().expect("node codes poisoned");
        codes.retain(|_, (_, until)| now < *until);
        codes.remove(&sha256(&code)).map(|(user, _)| user).ok_or(ApiError::bad("expired"))?
    };
    let device = DeviceId::of(&key).text();
    let name = relay_proto::clean_name(&body.name);
    if !shared.store.device_seen(&device, &key, &name, now)? {
        return Err(ApiError::forbidden("device_revoked"));
    }
    let token = shared.new_session(&user, &device, now)?.ok_or(ApiError::conflict("too_many_devices"))?;
    shared.store.set_node(&device)?;
    shared.notice(std::slice::from_ref(&user), "node_paired", json!({ "device": device, "name": name }));
    Ok(Json(json!({ "token": token, "device": device })))
}

// ── teams ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

async fn team_create(State(shared): State<Arc<Shared>>, me: Me, Json(body): Json<NameBody>) -> Answer {
    let name = team_name(&body.name)?;
    let now = now();
    let team = shared.store.create_team(&me.user.id, &name, now)?.ok_or(ApiError::conflict("too_many_teams"))?;
    shared.record(&team.id, Some(&me.user.id), "team_created", json!({ "name": team.name }), now);
    team_answer(&shared, &team.id, &me)
}

fn team_answer(shared: &Shared, team_id: &str, me: &Me) -> Answer {
    let role = role_in(shared, team_id, me)?;
    let team = shared.store.team(team_id)?.ok_or_else(ApiError::not_found)?;
    let members: Vec<Value> = shared
        .store
        .members(team_id)?
        .into_iter()
        .map(|member| {
            let mut value = user_json(&member.user);
            value["role"] = json!(member.role.as_str());
            value["online"] = json!(shared.hub.is_online(&member.user.id));
            value
        })
        .collect();
    let waiting = if role.manages() { shared.store.join_requests(team_id)?.len() } else { 0 };
    let servers: Vec<Value> = shared
        .store
        .team_servers(team_id)?
        .into_iter()
        .map(|server| {
            let host = server.host_user.as_deref().and_then(|id| shared.store.user(id).ok().flatten());
            // A node has no live connection here: it counts as online while its address is fresh.
            let node = shared.store.is_node(&server.host_device).unwrap_or(false);
            let online = if node {
                let now = now();
                shared.addrs.lock().expect("addrs poisoned").get(&server.host_device).is_some_and(|(_, at)| now < *at + ADDR_FRESH)
            } else {
                server.host_user.as_deref().is_some_and(|id| shared.hub.is_online(id))
            };
            json!({
                "id": server.id,
                "name": server.name,
                "instance_id": server.instance_id,
                "host_device": server.host_device,
                "host": host.as_ref().map(|one| one.handle.clone()),
                "host_online": online,
                "node": node,
                "mine": server.host_device == me.device,
            })
        })
        .collect();
    Ok(Json(json!({
        "id": team.id,
        "name": team.name,
        "owner": team.owner,
        "version": team.version,
        "role": role.as_str(),
        "members": members,
        "servers": servers,
        "waiting": waiting,
    })))
}

async fn team_get(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>) -> Answer {
    team_answer(&shared, &team, &me)
}

async fn team_patch(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>, Json(body): Json<NameBody>) -> Answer {
    manager_in(&shared, &team, &me)?;
    let name = team_name(&body.name)?;
    let old = shared.store.team(&team)?.ok_or_else(ApiError::not_found)?;
    if old.name != name {
        shared.store.rename_team(&team, &name)?;
        shared.record(&team, Some(&me.user.id), "team_renamed", json!({ "from": old.name, "to": name }), now());
        shared.team_changed(&team, &[]);
    }
    team_answer(&shared, &team, &me)
}

async fn team_delete(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>) -> Result<StatusCode, ApiError> {
    if role_in(&shared, &team, &me)? != Role::Owner {
        return Err(ApiError::forbidden("forbidden"));
    }
    let old = shared.store.team(&team)?.ok_or_else(ApiError::not_found)?;
    let people: Vec<String> = shared.store.members(&team)?.into_iter().map(|one| one.user.id).collect();
    shared.store.delete_team(&team)?;
    shared.notice(&people, "team_deleted", json!({ "team": team, "name": old.name }));
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct UserBody {
    user: String,
}

async fn team_transfer(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>, Json(body): Json<UserBody>) -> Answer {
    if role_in(&shared, &team, &me)? != Role::Owner {
        return Err(ApiError::forbidden("forbidden"));
    }
    if shared.store.role(&team, &body.user)? != Some(Role::Admin) {
        return Err(ApiError::conflict("not_admin"));
    }
    let to = shared.store.user(&body.user)?.ok_or_else(ApiError::not_found)?;
    shared.store.transfer(&team, &me.user.id, &to.id)?;
    shared.record(&team, Some(&me.user.id), "owner_changed", json!({ "to": to.handle }), now());
    shared.team_changed(&team, &[]);
    team_answer(&shared, &team, &me)
}

#[derive(Deserialize)]
struct RoleBody {
    role: String,
}

async fn member_patch(
    State(shared): State<Arc<Shared>>,
    me: Me,
    Path((team, user)): Path<(String, String)>,
    Json(body): Json<RoleBody>,
) -> Answer {
    let actor = role_in(&shared, &team, &me)?;
    let new = Role::parse(&body.role).ok_or(ApiError::bad("bad_role"))?;
    let current = shared.store.role(&team, &user)?.ok_or_else(ApiError::not_found)?;
    may_set_role(actor, current, new, user == me.user.id).map_err(ApiError::forbidden)?;
    if new != current {
        let target = shared.store.user(&user)?.ok_or_else(ApiError::not_found)?;
        shared.store.set_role(&team, &user, new)?;
        shared.record(
            &team,
            Some(&me.user.id),
            "role_changed",
            json!({ "user": target.handle, "from": current.as_str(), "to": new.as_str() }),
            now(),
        );
        shared.team_changed(&team, &[]);
        shared.notice(std::slice::from_ref(&user), "role_changed", json!({ "team": team, "role": new.as_str() }));
    }
    team_answer(&shared, &team, &me)
}

async fn member_delete(
    State(shared): State<Arc<Shared>>,
    me: Me,
    Path((team, user)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let actor = role_in(&shared, &team, &me)?;
    let now = now();
    let name = shared.store.team(&team)?.map(|one| one.name).unwrap_or_default();
    if user == me.user.id {
        if actor == Role::Owner {
            return Err(ApiError::conflict("owner_cannot_leave"));
        }
        shared.store.remove_member(&team, &user)?;
        shared.record(&team, Some(&me.user.id), "member_left", json!({ "user": me.user.handle }), now);
        shared.team_changed(&team, &[user]);
        return Ok(StatusCode::NO_CONTENT);
    }
    let target = shared.store.role(&team, &user)?.ok_or_else(ApiError::not_found)?;
    may_remove(actor, target).map_err(ApiError::forbidden)?;
    let handle = shared.store.user(&user)?.map(|one| one.handle).unwrap_or_default();
    shared.store.remove_member(&team, &user)?;
    shared.record(&team, Some(&me.user.id), "member_removed", json!({ "user": handle }), now);
    shared.team_changed(&team, std::slice::from_ref(&user));
    shared.notice(&[user], "removed", json!({ "team": team, "name": name }));
    Ok(StatusCode::NO_CONTENT)
}

// ── invites by handle or number ─────────────────────────────────────────────

#[derive(Deserialize)]
struct PersonInvite {
    /// "@handle", "handle" or the 8-digit number.
    who: String,
    role: String,
}

async fn member_invite(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>, Json(body): Json<PersonInvite>) -> Answer {
    let actor = manager_in(&shared, &team, &me)?;
    let role = Role::parse(&body.role).filter(|role| *role != Role::Owner).ok_or(ApiError::bad("bad_role"))?;
    if role > actor {
        return Err(ApiError::forbidden("above_you"));
    }
    let who = body.who.trim().trim_start_matches('@').to_lowercase();
    let person = match who.parse::<u64>() {
        Ok(number) if who.len() == 8 => shared.store.user_by_number(number)?,
        _ => shared.store.user_by_handle(&who)?,
    }
    .ok_or(ApiError::new(StatusCode::NOT_FOUND, "no_such_user"))?;
    if shared.store.role(&team, &person.id)?.is_some() {
        return Err(ApiError::conflict("already_member"));
    }
    let now = now();
    shared.store.invite_person(&team, &person.id, role, &me.user.id, now, now + INVITE_MAX)?;
    let name = shared.store.team(&team)?.map(|one| one.name).unwrap_or_default();
    shared.record(&team, Some(&me.user.id), "person_invited", json!({ "user": person.handle, "role": role.as_str() }), now);
    shared.notice(
        std::slice::from_ref(&person.id),
        "invited",
        json!({ "team": team, "name": name, "by": me.user.handle, "role": role.as_str() }),
    );
    Ok(Json(json!({ "user": user_json(&person), "role": role.as_str() })))
}

async fn my_invites(State(shared): State<Arc<Shared>>, me: Me) -> Answer {
    let invites: Vec<Value> = shared
        .store
        .invites_for(&me.user.id, now())?
        .into_iter()
        .map(|(team, role, by)| {
            let by = shared.store.user(&by).ok().flatten().map(|one| one.handle);
            json!({ "team": team.id, "name": team.name, "role": role.as_str(), "by": by })
        })
        .collect();
    Ok(Json(json!({ "invites": invites })))
}

#[derive(Deserialize)]
struct InviteAnswer {
    accept: bool,
}

async fn my_invite_answer(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>, Json(body): Json<InviteAnswer>) -> Answer {
    let now = now();
    let role = shared.store.answer_invite(&team, &me.user.id, body.accept, now)?.ok_or_else(ApiError::not_found)?;
    let name = shared.store.team(&team)?.map(|one| one.name).unwrap_or_default();
    if body.accept {
        shared.record(&team, Some(&me.user.id), "member_joined", json!({ "user": me.user.handle, "role": role.as_str() }), now);
        shared.team_changed(&team, &[]);
    }
    Ok(Json(json!({ "state": if body.accept { "joined" } else { "declined" }, "team": team, "name": name })))
}

// ── invites ────────────────────────────────────────────────────────────────

/// Longest an invite may stay open, and how long one lasts when nobody says.
const INVITE_MAX: u64 = 30 * DAY;
const INVITE_DEFAULT: u64 = 7 * DAY;

#[derive(Deserialize)]
struct InviteBody {
    role: String,
    expires_in: Option<u64>,
    max_uses: Option<u32>,
    needs_approval: Option<bool>,
}

async fn invite_create(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>, Json(body): Json<InviteBody>) -> Answer {
    let actor = manager_in(&shared, &team, &me)?;
    let role = Role::parse(&body.role).filter(|role| *role != Role::Owner).ok_or(ApiError::bad("bad_role"))?;
    if role > actor {
        return Err(ApiError::forbidden("above_you"));
    }
    let lasts = body.expires_in.unwrap_or(INVITE_DEFAULT);
    if !(60..=INVITE_MAX).contains(&lasts) {
        return Err(ApiError::bad("bad_expiry"));
    }
    if body.max_uses.is_some_and(|most| !(1..=1000).contains(&most)) {
        return Err(ApiError::bad("bad_uses"));
    }
    let now = now();
    let code = util::new_invite_code();
    let invite = Invite {
        code_hash: sha256(&code),
        prefix: code[..4].to_string(),
        team: team.clone(),
        role,
        created_by: me.user.id.clone(),
        created_at: now,
        expires: now + lasts,
        max_uses: body.max_uses,
        uses: 0,
        needs_approval: body.needs_approval.unwrap_or(true),
        revoked: false,
    };
    if !shared.store.open_invite(&invite, now)? {
        return Err(ApiError::conflict("too_many_invites"));
    }
    let mut value = invite_json(&shared, &invite);
    value["code"] = json!(code);
    value["url"] = json!(shared.invite_url(&code));
    Ok(Json(value))
}

async fn invites_list(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>) -> Answer {
    manager_in(&shared, &team, &me)?;
    let invites: Vec<Value> = shared.store.open_invites(&team, now())?.iter().map(|invite| invite_json(&shared, invite)).collect();
    Ok(Json(json!({ "invites": invites })))
}

async fn invite_delete(
    State(shared): State<Arc<Shared>>,
    me: Me,
    Path((team, id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    manager_in(&shared, &team, &me)?;
    let hash = util::unhex(&id).ok_or_else(ApiError::not_found)?;
    if !shared.store.revoke_invite(&team, &hash, now())? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Looks up an invite from a code as typed or pasted (a whole link works too).
fn find_invite(shared: &Shared, text: &str) -> Result<Option<(String, Invite)>, ApiError> {
    let last = text.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    let Some(code) = util::normalize_code(last) else { return Ok(None) };
    Ok(shared.store.invite(&sha256(&code))?.map(|invite| (code, invite)))
}

fn preview_json(shared: &Shared, invite: &Invite) -> Result<Value, ApiError> {
    let team = shared.store.team(&invite.team)?.ok_or_else(ApiError::not_found)?;
    let members = shared.store.members(&team.id)?.len();
    let by = shared.store.user(&invite.created_by)?.map(|user| user.handle);
    Ok(json!({
        "valid": true,
        "team": { "id": team.id, "name": team.name, "members": members },
        "by": by,
        "role": invite.role.as_str(),
        "needs_approval": invite.needs_approval,
        "expires": invite.expires,
    }))
}

async fn invite_preview(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Answer {
    if !shared.limits.allow(&format!("preview:{}", client_ip(&headers, peer)), 30, 60) {
        return Err(ApiError::slow());
    }
    let Some((_, invite)) = find_invite(&shared, &code)? else {
        return Ok(Json(json!({ "valid": false, "reason": "not_found" })));
    };
    if let Some(reason) = invite.unusable(now()) {
        return Ok(Json(json!({ "valid": false, "reason": reason })));
    }
    Ok(Json(preview_json(&shared, &invite)?))
}

async fn invite_accept(State(shared): State<Arc<Shared>>, me: Me, Path(code): Path<String>) -> Answer {
    if !shared.limits.allow(&format!("accept:{}", me.user.id), 20, 3600) {
        return Err(ApiError::slow());
    }
    let now = now();
    let Some((_, invite)) = find_invite(&shared, &code)? else {
        return Err(ApiError::new(StatusCode::GONE, "not_found"));
    };
    let Some(joined) = shared.store.use_invite(&invite.code_hash, &me.user.id, now)? else {
        return Err(ApiError::new(StatusCode::GONE, invite.unusable(now).unwrap_or("not_found")));
    };
    let name = shared.store.team(&invite.team)?.map(|team| team.name).unwrap_or_default();
    Ok(Json(match joined {
        Joined::Joined(team, role) => {
            shared.record(&team, Some(&me.user.id), "member_joined", json!({ "user": me.user.handle, "role": role.as_str() }), now);
            shared.team_changed(&team, &[]);
            json!({ "state": "joined", "team": team, "name": name })
        }
        Joined::Requested(team) => {
            let managers = shared.managers(&team);
            shared.notice(&managers, "join_request", json!({ "team": team, "name": name, "user": me.user.handle }));
            json!({ "state": "waiting", "team": team, "name": name })
        }
        Joined::Waiting(team) => json!({ "state": "waiting", "team": team, "name": name }),
        Joined::Already(team) => json!({ "state": "member", "team": team, "name": name }),
    }))
}

async fn requests_list(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>) -> Answer {
    manager_in(&shared, &team, &me)?;
    let requests: Vec<Value> = shared
        .store
        .join_requests(&team)?
        .into_iter()
        .map(|request| json!({ "user": user_json(&request.user), "role": request.role.as_str(), "created_at": request.created_at }))
        .collect();
    Ok(Json(json!({ "requests": requests })))
}

#[derive(Deserialize)]
struct AnswerBody {
    accept: bool,
}

async fn request_answer(
    State(shared): State<Arc<Shared>>,
    me: Me,
    Path((team, user)): Path<(String, String)>,
    Json(body): Json<AnswerBody>,
) -> Result<StatusCode, ApiError> {
    manager_in(&shared, &team, &me)?;
    let now = now();
    let role = shared.store.answer_request(&team, &user, body.accept, &me.user.id, now)?.ok_or_else(ApiError::not_found)?;
    let name = shared.store.team(&team)?.map(|one| one.name).unwrap_or_default();
    if body.accept {
        let handle = shared.store.user(&user)?.map(|one| one.handle).unwrap_or_default();
        shared.record(&team, Some(&me.user.id), "member_joined", json!({ "user": handle, "role": role.as_str() }), now);
        shared.team_changed(&team, &[]);
        shared.notice(&[user], "joined", json!({ "team": team, "name": name }));
    } else {
        shared.notice(&[user], "rejected", json!({ "team": team, "name": name }));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ActivityQuery {
    before: Option<i64>,
}

async fn activity_list(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>, Query(query): Query<ActivityQuery>) -> Answer {
    role_in(&shared, &team, &me)?;
    let items: Vec<Value> = shared.store.activity(&team, query.before, 50)?.iter().map(|item| activity_json(&shared, item)).collect();
    Ok(Json(json!({ "items": items })))
}

/// What a host may say happened on one of its servers (section 6.5). Paths and names only, never contents.
const HOST_ACTIVITY: &[&str] = &[
    "server_started",
    "server_stopped",
    "server_crashed",
    "file_changed",
    "plugin_added",
    "studio_asset_added",
    "storage_added",
    "storage_used",
];

#[derive(Deserialize)]
struct ActivityPost {
    server: String,
    kind: String,
    #[serde(default)]
    data: Value,
}

async fn activity_post(
    State(shared): State<Arc<Shared>>,
    me: Me,
    Path(team): Path<String>,
    Json(body): Json<ActivityPost>,
) -> Result<StatusCode, ApiError> {
    role_in(&shared, &team, &me)?;
    if !HOST_ACTIVITY.contains(&body.kind.as_str()) {
        return Err(ApiError::bad("bad_kind"));
    }
    let data = if body.data.is_null() { json!({}) } else { body.data };
    if !data.is_object() || data.to_string().len() > 1024 {
        return Err(ApiError::bad("too_big"));
    }
    if !shared.limits.allow(&format!("activity:{}", me.device), 60, 60) {
        return Err(ApiError::slow());
    }
    let server = shared.store.team_servers(&team)?.into_iter().find(|one| one.id == body.server).ok_or_else(ApiError::not_found)?;
    if server.host_device != me.device {
        return Err(ApiError::forbidden("not_host"));
    }
    let mut data = data;
    data["server"] = json!(server.name);
    shared.record_for(&team, Some(&server.id), Some(&me.user.id), &body.kind, data, now());
    Ok(StatusCode::NO_CONTENT)
}

/// The iroh relay on this machine (`deploy/iroh-relay.toml`, `access.http`) asks here whether an endpoint may
/// use it: only devices of a Monolab account. Answers `true` (200) or nothing. Only for the relay itself —
/// a call that came through Caddy (from outside) carries `X-Forwarded-For` and is refused like a stranger.
async fn relay_access(State(shared): State<Arc<Shared>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    if !peer.ip().is_loopback() || headers.contains_key("x-forwarded-for") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let key = headers.get("x-iroh-endpoint-id").and_then(|one| one.to_str().ok()).and_then(public_key_of);
    let Some(key) = key else { return StatusCode::FORBIDDEN.into_response() };
    match shared.store.device_active(&DeviceId::of(&key).text()) {
        Ok(true) => (StatusCode::OK, "true").into_response(),
        _ => StatusCode::FORBIDDEN.into_response(),
    }
}

/// An endpoint id (64 hex digits) as key bytes.
fn public_key_of(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut key = [0u8; 32];
    for (at, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).ok()?;
    }
    Some(key)
}

// ── servers and grants ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AttachBody {
    instance_id: String,
    name: String,
    /// A node of the caller's to attach the server from, instead of the calling device.
    #[serde(default)]
    host: Option<String>,
}

/// Attaches one of this device's servers to the team. Only owners and admins, and only from the device
/// that runs it, so nobody can put someone else's server in a team.
async fn server_attach(State(shared): State<Arc<Shared>>, me: Me, Path(team): Path<String>, Json(body): Json<AttachBody>) -> Answer {
    manager_in(&shared, &team, &me)?;
    let instance = body.instance_id.trim();
    if instance.is_empty() || instance.len() > 128 || instance.chars().any(char::is_control) {
        return Err(ApiError::bad("bad_server"));
    }
    let name = team_name(&body.name)?;
    let now = now();
    let host = match body.host.as_deref().map(str::trim).filter(|one| !one.is_empty() && *one != me.device) {
        // Only a node this person paired (their own machine), never another device.
        Some(node) if shared.store.is_node_of(node, &me.user.id)? => node.to_string(),
        Some(_) => return Err(ApiError::forbidden("not_your_node")),
        None => me.device.clone(),
    };
    let server = shared.store.attach_server(&team, &host, instance, &name, now)?.ok_or(ApiError::conflict("already_attached"))?;
    shared.record_for(&team, Some(&server.id), Some(&me.user.id), "server_attached", json!({ "server": name }), now);
    shared.team_changed(&team, &[]);
    team_answer(&shared, &team, &me)
}

async fn server_detach(
    State(shared): State<Arc<Shared>>,
    me: Me,
    Path((team, id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let role = role_in(&shared, &team, &me)?;
    let server = shared.store.team_servers(&team)?.into_iter().find(|one| one.id == id).ok_or_else(ApiError::not_found)?;
    if !role.manages() && server.host_device != me.device {
        return Err(ApiError::forbidden("forbidden"));
    }
    shared.store.detach_server(&team, &id)?;
    shared.record_for(&team, None, Some(&me.user.id), "server_detached", json!({ "server": server.name }), now());
    shared.team_changed(&team, &[]);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AddrBody {
    addr: Value,
}

/// How long a host's iroh address is handed out after it last said it.
const ADDR_FRESH: u64 = 15 * 60;

/// A host says where it can be reached over iroh. Kept in memory; given only with grants for this host.
async fn addr_put(State(shared): State<Arc<Shared>>, me: Me, Json(body): Json<AddrBody>) -> Result<StatusCode, ApiError> {
    if !shared.limits.allow(&format!("addr:{}", me.device), 30, 60) {
        return Err(ApiError::slow());
    }
    let text = body.addr.to_string();
    if !body.addr.is_object() || text.len() > 4096 {
        return Err(ApiError::bad("bad_addr"));
    }
    let now = now();
    let mut addrs = shared.addrs.lock().expect("addrs poisoned");
    addrs.retain(|_, (_, at)| now < *at + ADDR_FRESH);
    addrs.insert(me.device.clone(), (text, now));
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct GrantsBody {
    team: String,
}

/// Role grants for this device in one team: one per host that has servers there (section 5).
async fn grants(State(shared): State<Arc<Shared>>, me: Me, Json(body): Json<GrantsBody>) -> Answer {
    let role = role_in(&shared, &body.team, &me)?;
    if !shared.limits.allow(&format!("grants:{}", me.device), 30, 60) {
        return Err(ApiError::slow());
    }
    let team = shared.store.team(&body.team)?.ok_or_else(ApiError::not_found)?;
    let device = DeviceId::parse(&me.device).ok_or(ApiError::bad("bad_device"))?;
    let now = now();
    let mut by_host: std::collections::BTreeMap<String, Vec<super::store::TeamServer>> = Default::default();
    for server in shared.store.team_servers(&team.id)? {
        if server.host_device != me.device {
            by_host.entry(server.host_device.clone()).or_default().push(server);
        }
    }
    let addrs = shared.addrs.lock().expect("addrs poisoned").clone();
    let mut out = Vec::new();
    for (host, servers) in by_host {
        let Some(host_id) = DeviceId::parse(&host) else { continue };
        let grant = relay_proto::grant::Grant {
            v: 1,
            kid: super::GRANT_KID,
            user: me.user.id.clone(),
            handle: me.user.handle.clone(),
            device,
            team: team.id.clone(),
            team_version: team.version,
            role: relay_proto::grant::Role::parse(role.as_str()).expect("same roles"),
            host: host_id,
            servers: servers.iter().map(|one| one.instance_id.clone()).collect(),
            iat: now,
            exp: now + relay_proto::grant::LIFE_SECS,
        };
        let addr = addrs.get(&host).filter(|(_, at)| now < *at + ADDR_FRESH).and_then(|(text, _)| serde_json::from_str::<Value>(text).ok());
        let host_handle = servers
            .first()
            .and_then(|one| one.host_user.as_deref())
            .and_then(|id| shared.store.user(id).ok().flatten())
            .map(|one| one.handle);
        out.push(json!({
            "host_device": host,
            "host": host_handle,
            "addr": addr,
            "grant": relay_proto::grant::sign(&grant, &shared.grant_key),
            "exp": grant.exp,
            "servers": servers.iter().map(|one| json!({ "id": one.id, "instance_id": one.instance_id, "name": one.name })).collect::<Vec<_>>(),
        }));
    }
    Ok(Json(json!({ "team": team.id, "version": team.version, "role": role.as_str(), "grants": out })))
}

/// The public keys grants are signed with. Apps trust only the keys built into them; this is so a
/// new key can be checked against an update before it's used.
async fn keys(State(shared): State<Arc<Shared>>) -> Json<Value> {
    Json(json!({ "keys": [{ "kid": super::GRANT_KID, "public_key": hex(&shared.grant_key.verifying_key().to_bytes()) }] }))
}

// ── live and pages ─────────────────────────────────────────────────────────

async fn ws(State(shared): State<Arc<Shared>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.max_message_size(4096).on_upgrade(move |socket| super::live::run(shared, socket))
}

/// `/v2/app`: update notices and anonymous usage counts for every running app, no sign-in (app.rs).
async fn app_ws(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let ip = client_ip(&headers, peer);
    if !shared.limits.allow(&format!("app:{ip}"), super::app::CONNECTS_PER_MINUTE, 60) {
        return ApiError::slow().into_response();
    }
    let Some(slot) = super::app::AppHub::reserve(&shared, &ip) else {
        return ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "too_many").into_response();
    };
    upgrade.max_message_size(1024).on_upgrade(move |socket| super::app::run(shared, socket, slot))
}

async fn invite_page(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Response {
    if !shared.limits.allow(&format!("preview:{}", client_ip(&headers, peer)), 30, 60) {
        return (StatusCode::TOO_MANY_REQUESTS, Html(pages::invite_unusable("slow"))).into_response();
    }
    let found = match find_invite(&shared, &code) {
        Ok(found) => found,
        Err(error) => return error.into_response(),
    };
    let Some((code, invite)) = found else {
        return (StatusCode::NOT_FOUND, Html(pages::invite_unusable("not_found"))).into_response();
    };
    if let Some(reason) = invite.unusable(now()) {
        return (StatusCode::GONE, Html(pages::invite_unusable(reason))).into_response();
    }
    let team = shared.store.team(&invite.team).ok().flatten();
    let members = shared.store.members(&invite.team).map(|all| all.len()).unwrap_or(0);
    let by = shared.store.user(&invite.created_by).ok().flatten().map(|user| user.handle).unwrap_or_default();
    let team_name = team.map(|one| one.name).unwrap_or_default();
    Html(pages::invite(&pages::InvitePreview {
        code: &code,
        team: &team_name,
        by: &by,
        role: invite.role.as_str(),
        members,
        needs_approval: invite.needs_approval,
    }))
    .into_response()
}

// ── crash reports ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CrashBody {
    version: String,
    os: String,
    kind: String,
    message: String,
    #[serde(default)]
    detail: String,
}

/// Field limits for a crash report (the whole body is capped at 16 KB by the router).
pub const CRASH_MESSAGE_MAX: usize = 1_000;
pub const CRASH_DETAIL_MAX: usize = 12_000;

/// Checks a report someone chose to send from the app's crash dialog. Nothing about who sent it is kept.
fn crash_from(body: CrashBody) -> Result<CrashReport, ApiError> {
    let short = |text: &str, most: usize| {
        !text.trim().is_empty() && text.chars().count() <= most && !text.chars().any(|letter| letter.is_control())
    };
    if !short(&body.version, 32) || !short(&body.os, 64) {
        return Err(ApiError::bad("bad_crash"));
    }
    if !matches!(body.kind.as_str(), "panic" | "ui") {
        return Err(ApiError::bad("bad_crash"));
    }
    if body.message.trim().is_empty() || body.message.chars().count() > CRASH_MESSAGE_MAX || body.detail.chars().count() > CRASH_DETAIL_MAX
    {
        return Err(ApiError::bad("crash_too_big"));
    }
    Ok(CrashReport {
        version: body.version.trim().into(),
        os: body.os.trim().into(),
        kind: body.kind,
        message: body.message,
        detail: body.detail,
    })
}

async fn crash_report(
    State(shared): State<Arc<Shared>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<CrashBody>,
) -> Answer {
    if !shared.limits.allow(&format!("crash:{}", client_ip(&headers, peer)), 5, 3600) {
        return Err(ApiError::slow());
    }
    let report = crash_from(body)?;
    let id = shared.store.add_crash(&report, now())?;
    Ok(Json(json!({ "ok": true, "id": id })))
}
