//! `/admin`: a small page with usage numbers for the people in `MONOLAB_ADMIN_USERS`. Browser sign-in
//! with Discord through the same `/auth/callback/discord` the apps use (told apart by the OAuth state),
//! then a 12-hour cookie whose token is stored only as a hash. Section 4 of `docs/updates-and-stats.md`.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::oauth::Identity;
use super::pages;
use super::store::ADMIN_SESSION_SECS;
use super::util::{self, now, sha256};
use super::Shared;

const COOKIE: &str = "mnlab_admin";
/// Browser sign-ins that may wait for Discord at once (each also limited per address).
const WAITING_MOST: usize = 1_000;

fn page(status: StatusCode, body: String) -> Response {
    let mut response = (status, Html(body)).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty() && value.len() <= 128)
}

fn set_cookie(response: &mut Response, value: &str, max_age: u64) {
    let text = format!("{COOKIE}={value}; Path=/admin; Max-Age={max_age}; HttpOnly; Secure; SameSite=Lax");
    if let Ok(value) = HeaderValue::from_str(&text) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

pub async fn admin_page(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let now = now();
    let user = match cookie_token(&headers).map(|token| shared.store.admin_session(&sha256(&token), now)) {
        Some(Ok(user)) => user,
        Some(Err(error)) => {
            eprintln!("[accounts] admin session: {error}");
            return page(StatusCode::INTERNAL_SERVER_ERROR, pages::failed("server"));
        }
        None => None,
    };
    let Some(user) = user else {
        return page(StatusCode::OK, pages::admin_login());
    };
    if !shared.is_admin(&user) {
        return page(StatusCode::FORBIDDEN, pages::admin_forbidden());
    }
    let stats = match shared.store.stats(now) {
        Ok(stats) => stats,
        Err(error) => {
            eprintln!("[accounts] stats: {error}");
            return page(StatusCode::INTERNAL_SERVER_ERROR, pages::failed("server"));
        }
    };
    let (connected, anonymous) = shared.apps.connected();
    let (addon, fork) = shared.apps.updates();
    page(StatusCode::OK, pages::admin(&pages::AdminView { handle: &user.handle, connected, anonymous, addon, fork, stats: &stats }))
}

#[derive(Deserialize)]
pub struct LoginQuery {
    /// Pretend sign-in only.
    subject: Option<String>,
}

pub async fn admin_login(State(shared): State<Arc<Shared>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Query(query): Query<LoginQuery>) -> Response {
    if !shared.limits.allow(&format!("admin:{}", super::http::client_ip(&headers, peer)), 10, 60) {
        return page(StatusCode::TOO_MANY_REQUESTS, pages::failed("slow"));
    }
    let Some(discord) = &shared.discord else {
        if shared.fake_sign_in {
            let subject = query.subject.unwrap_or_else(|| "1".into());
            return signed_in(&shared, "fake", Identity { name: String::new(), subject, avatar: None });
        }
        return page(StatusCode::SERVICE_UNAVAILABLE, pages::failed("server"));
    };
    let now = now();
    shared.sweep(now);
    let state = util::new_secret();
    let verifier = util::new_secret();
    {
        let mut waiting = shared.web_logins.lock().expect("web logins poisoned");
        if waiting.len() >= WAITING_MOST {
            return page(StatusCode::TOO_MANY_REQUESTS, pages::failed("slow"));
        }
        waiting.insert(state.clone(), (verifier.clone(), now));
    }
    Redirect::to(&discord.authorize_url(&state, &verifier, &shared.redirect_url("discord"))).into_response()
}

/// A Discord callback for a browser sign-in to `/admin`, when `state` is one of ours. None: it's an app's.
pub async fn web_callback(shared: &Arc<Shared>, state: &str, code: Option<String>, error: Option<String>) -> Option<Response> {
    let (verifier, _) = shared.web_logins.lock().expect("web logins poisoned").remove(state)?;
    let discord = shared.discord.as_ref()?;
    let (Some(code), None) = (code, error) else {
        return Some(page(StatusCode::OK, pages::failed("denied")));
    };
    Some(match discord.identify(&shared.http, &code, &verifier, &shared.redirect_url("discord")).await {
        Ok(identity) => signed_in(shared, "discord", identity),
        Err(error) => {
            eprintln!("[accounts] discord (admin): {error}");
            page(StatusCode::BAD_GATEWAY, pages::failed("server"))
        }
    })
}

/// Discord said who this is: an admin gets a session cookie, anyone else the 403 page. Nothing is
/// stored for people who aren't admins.
fn signed_in(shared: &Shared, provider: &str, identity: Identity) -> Response {
    let user = match shared.store.user_by_identity(provider, &identity.subject) {
        Ok(Some((user, false))) => Some(user),
        Ok(_) => None,
        Err(error) => {
            eprintln!("[accounts] admin sign-in: {error}");
            return page(StatusCode::INTERNAL_SERVER_ERROR, pages::failed("server"));
        }
    };
    let Some(user) = user.filter(|user| shared.is_admin(user)) else {
        return page(StatusCode::FORBIDDEN, pages::admin_forbidden());
    };
    let token = util::new_secret();
    if let Err(error) = shared.store.open_admin_session(&sha256(&token), &user.id, now()) {
        eprintln!("[accounts] admin session: {error}");
        return page(StatusCode::INTERNAL_SERVER_ERROR, pages::failed("server"));
    }
    let mut response = Redirect::to("/admin").into_response();
    set_cookie(&mut response, &token, ADMIN_SESSION_SECS);
    response
}

pub async fn admin_logout(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie_token(&headers) {
        let _ = shared.store.close_admin_session(&sha256(&token));
    }
    let mut response = Redirect::to("/admin").into_response();
    set_cookie(&mut response, "", 0);
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `monolab-relay --notify-updates` asks here to read the release list now. Only from this machine:
/// a call through Caddy carries `X-Forwarded-For` (and Caddy answers 404 for this path anyway).
pub async fn notify_updates(State(shared): State<Arc<Shared>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> StatusCode {
    if !peer.ip().is_loopback() || headers.contains_key("x-forwarded-for") {
        return StatusCode::NOT_FOUND;
    }
    shared.updates_now.notify_one();
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_is_found() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("a=1; mnlab_admin=abc ; b=2"));
        assert_eq!(cookie_token(&headers).as_deref(), Some("abc"));
        headers.insert(header::COOKIE, HeaderValue::from_static("mnlab_admin="));
        assert_eq!(cookie_token(&headers), None);
    }
}
