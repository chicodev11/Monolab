//! Monolink central server: Discord sign-in, unique handles, teams, roles, invite links,
//! who is online, and a short activity log. HTTP + one WebSocket, meant to sit behind Caddy on
//! `mnlab.cc`. Server files, console text and commands never come here. See `docs/monolink.md`.

mod admin;
pub mod app;
mod http;
pub use http::node_pair_message;
mod live;
pub mod oauth;
mod pages;
pub mod store;
pub mod util;

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use serde_json::json;
use tokio::task::JoinHandle;

use store::Store;
use util::{Limiter, Role};

pub struct Config {
    pub listen: SocketAddr,
    pub data: PathBuf,
    /// Where people reach this server, like `https://mnlab.cc`. Used for links and the OAuth callback.
    pub public_url: String,
    pub discord: Option<oauth::Discord>,
    /// A pretend sign-in at `/auth/fake`, for tests and trying things out on this machine.
    pub fake_sign_in: bool,
    /// The release list to read every 5 minutes for update notices (`app::UPDATES_URL`); None reads nothing.
    pub updates_url: Option<String>,
    /// Who may open `/admin`: account ids or permanent 8-digit numbers (`MONOLAB_ADMIN_USERS`).
    pub admins: Vec<String>,
}

pub struct Accounts {
    pub addr: SocketAddr,
    pub store: Arc<Store>,
    tasks: Vec<JoinHandle<()>>,
}

impl Accounts {
    pub fn stop(self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// The number of the key that signs grants now. Apps carry the public keys they trust (by number);
/// a new key goes out in an app update first.
pub const GRANT_KID: u8 = 1;
const GRANT_KEY_FILE: &str = "grant-key.bin";

/// The key that signs role grants, made the first time (only this server's user may read it).
pub fn load_or_create_grant_key(data: &std::path::Path) -> io::Result<SigningKey> {
    let path = data.join(GRANT_KEY_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let secret: [u8; 32] = bytes.try_into().map_err(|_| io::Error::other(format!("{} is damaged", path.display())))?;
            Ok(SigningKey::from_bytes(&secret))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(data)?;
            let secret = util::random::<32>();
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            use std::io::Write;
            options.open(&path)?.write_all(&secret)?;
            Ok(SigningKey::from_bytes(&secret))
        }
        Err(error) => Err(error),
    }
}

/// How long a sign-in may wait in the browser, and a new person has to pick a handle.
const LOGIN_SECS: u64 = 10 * 60;
const SIGNUP_SECS: u64 = 30 * 60;

#[derive(Clone)]
enum LoginResult {
    SignedIn { token: String, user: store::User },
    NeedsHandle { ticket: String, suggested: String },
    Failed(&'static str),
}

struct Login {
    device: String,
    provider: &'static str,
    poll_hash: Vec<u8>,
    state: String,
    verifier: String,
    created: u64,
    result: Option<LoginResult>,
    /// The port the signing-in app listens on at 127.0.0.1. After Discord, the browser is sent there
    /// with `proof`, and polling only gives the session with it: a sign-in link someone else opens in
    /// their browser sends the proof to their PC, not to the app that started it.
    port: u16,
    proof: String,
    /// Who signed in at Discord, kept until the app comes back with `proof`.
    identity: Option<oauth::Identity>,
}

struct Signup {
    device: String,
    provider: &'static str,
    subject: String,
    avatar: Option<String>,
    created: u64,
}

pub(crate) struct Shared {
    store: Arc<Store>,
    public_url: String,
    discord: Option<oauth::Discord>,
    fake_sign_in: bool,
    http: reqwest::Client,
    logins: Mutex<HashMap<String, Login>>,
    signups: Mutex<HashMap<Vec<u8>, Signup>>,
    hub: live::Hub,
    limits: Limiter,
    grant_key: SigningKey,
    /// Where each host device can be reached over iroh, as it last said (JSON), and when. Kept in
    /// memory only and handed only to teammates who get a grant for that host.
    addrs: Mutex<HashMap<String, (String, u64)>>,
    /// One-time codes for pairing a monolab-node machine: code hash → (user, until). Memory only.
    node_codes: Mutex<HashMap<Vec<u8>, (String, u64)>>,
    /// Open `/v2/app` connections and the release list numbers. Memory only.
    apps: app::AppHub,
    /// Wakes the release list reader early (`monolab-relay --notify-updates`).
    updates_now: tokio::sync::Notify,
    admins: Vec<String>,
    /// Browser sign-ins to `/admin` waiting for Discord: state → (PKCE verifier, started). Memory only.
    web_logins: Mutex<HashMap<String, (String, u64)>>,
}

pub async fn start(config: Config) -> io::Result<Accounts> {
    // reqwest is built without a crypto provider; this relay uses ring everywhere.
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::fs::create_dir_all(&config.data)?;
    let store = Arc::new(Store::open(&config.data.join("accounts.db")).map_err(io::Error::other)?);
    let grant_key = load_or_create_grant_key(&config.data)?;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(io::Error::other)?;
    let shared = Arc::new(Shared {
        store: store.clone(),
        public_url: config.public_url.trim_end_matches('/').to_string(),
        discord: config.discord,
        fake_sign_in: config.fake_sign_in,
        http,
        logins: Mutex::new(HashMap::new()),
        signups: Mutex::new(HashMap::new()),
        hub: live::Hub::default(),
        limits: Limiter::default(),
        grant_key,
        addrs: Mutex::new(HashMap::new()),
        node_codes: Mutex::new(HashMap::new()),
        apps: app::AppHub::default(),
        updates_now: tokio::sync::Notify::new(),
        admins: config.admins.iter().map(|one| one.trim().to_string()).filter(|one| !one.is_empty()).collect(),
        web_logins: Mutex::new(HashMap::new()),
    });
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    let addr = listener.local_addr()?;
    let app = http::router(shared.clone());
    let mut tasks = vec![tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await;
    })];
    if let Some(url) = config.updates_url {
        tasks.push(tokio::spawn(app::watch_updates(shared.clone(), url)));
    }
    // Old daily usage rows and ended admin sign-ins go once a day (and once at start).
    let pruner = shared.clone();
    tasks.push(tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(util::DAY));
        loop {
            every.tick().await;
            if let Err(error) = pruner.store.prune_usage(util::now()) {
                eprintln!("[accounts] pruning usage counts: {error}");
            }
        }
    }));
    Ok(Accounts { addr, store, tasks })
}

impl Shared {
    /// Whether this person may open `/admin`: their account id or permanent number is in
    /// `MONOLAB_ADMIN_USERS`. Handles aren't accepted: they can change hands.
    fn is_admin(&self, user: &store::User) -> bool {
        self.admins.iter().any(|one| *one == user.id || one.parse::<u64>().is_ok_and(|number| number == user.number && number != 0))
    }

    fn redirect_url(&self, provider: &str) -> String {
        format!("{}/auth/callback/{provider}", self.public_url)
    }

    fn invite_url(&self, code: &str) -> String {
        format!("{}/invite/{code}", self.public_url)
    }

    /// Drops sign-ins and sign-ups nobody finished.
    fn sweep(&self, now: u64) {
        self.logins.lock().expect("logins poisoned").retain(|_, login| now < login.created + LOGIN_SECS);
        self.signups.lock().expect("signups poisoned").retain(|_, signup| now < signup.created + SIGNUP_SECS);
        self.web_logins.lock().expect("web logins poisoned").retain(|_, (_, created)| now < *created + LOGIN_SECS);
    }

    /// The provider said who this is: sign them in, or ask the app for a handle.
    fn identified(&self, login_id: &str, identity: oauth::Identity, now: u64) -> LoginResult {
        let Some((device, provider)) = self.logins.lock().expect("logins poisoned").get(login_id).map(|one| (one.device.clone(), one.provider))
        else {
            return LoginResult::Failed("expired");
        };
        let result = match self.store.user_by_identity(provider, &identity.subject) {
            Err(_) => LoginResult::Failed("server"),
            Ok(Some((_, true))) => LoginResult::Failed("disabled"),
            Ok(Some((user, false))) => {
                let _ = self.store.set_avatar(&user.id, identity.avatar.as_deref());
                match self.new_session(&user.id, &device, now) {
                    Ok(Some(token)) => LoginResult::SignedIn { token, user: store::User { avatar: identity.avatar, ..user } },
                    Ok(None) => LoginResult::Failed("too_many_devices"),
                    Err(_) => LoginResult::Failed("server"),
                }
            }
            Ok(None) => {
                let ticket = util::new_secret();
                self.signups.lock().expect("signups poisoned").insert(
                    util::sha256(&ticket),
                    Signup { device, provider, subject: identity.subject, avatar: identity.avatar, created: now },
                );
                LoginResult::NeedsHandle { ticket, suggested: util::suggest_handle(&identity.name) }
            }
        };
        if let Some(login) = self.logins.lock().expect("logins poisoned").get_mut(login_id) {
            login.result = Some(result.clone());
        }
        result
    }

    /// A new session token for a device, or None when the person has too many devices.
    fn new_session(&self, user: &str, device: &str, now: u64) -> rusqlite::Result<Option<String>> {
        let token = util::new_secret();
        Ok(self.store.open_session(&util::sha256(&token), user, device, now)?.then_some(token))
    }

    /// A team's members, roles or name changed: raise its version and tell everyone in it,
    /// plus anyone in `also` (someone who just left or was removed).
    fn team_changed(&self, team: &str, also: &[String]) {
        let Ok(version) = self.store.bump(team) else { return };
        let mut people: Vec<String> = self.store.members(team).unwrap_or_default().into_iter().map(|one| one.user.id).collect();
        people.extend(also.iter().cloned());
        self.hub.send(&people, &json!({ "type": "team_changed", "team": team, "version": version }));
    }

    fn notice(&self, people: &[String], kind: &str, data: serde_json::Value) {
        self.hub.send(people, &json!({ "type": "notice", "kind": kind, "data": data }));
    }

    /// Writes one activity row and pushes it to the team.
    fn record(&self, team: &str, actor: Option<&str>, kind: &str, data: serde_json::Value, now: u64) {
        self.record_for(team, None, actor, kind, data, now);
    }

    fn record_for(&self, team: &str, server: Option<&str>, actor: Option<&str>, kind: &str, data: serde_json::Value, now: u64) {
        let Ok(item) = self.store.record_for(team, server, actor, kind, &data, now) else { return };
        let people: Vec<String> = self.store.members(team).unwrap_or_default().into_iter().map(|one| one.user.id).collect();
        self.hub.send(&people, &json!({ "type": "activity", "team": team, "item": http::activity_json(self, &item) }));
    }

    fn managers(&self, team: &str) -> Vec<String> {
        self.store.members(team).unwrap_or_default().into_iter().filter(|one| one.role.manages()).map(|one| one.user.id).collect()
    }
}

/// Whether `actor` may give `target` (currently `current`) the role `new`. See section 8 of the design.
pub fn may_set_role(actor: Role, current: Role, new: Role, same_person: bool) -> Result<(), &'static str> {
    if !actor.manages() {
        return Err("forbidden");
    }
    if same_person {
        return Err("self");
    }
    if current == Role::Owner {
        return Err("owner");
    }
    if new == Role::Owner {
        return Err("use_transfer");
    }
    if actor == Role::Admin && current == Role::Admin {
        return Err("peer");
    }
    if new > actor {
        return Err("above_you");
    }
    Ok(())
}

/// Whether `actor` may remove someone whose role is `target`. Leaving yourself is handled apart.
pub fn may_remove(actor: Role, target: Role) -> Result<(), &'static str> {
    if !actor.manages() {
        return Err("forbidden");
    }
    if target == Role::Owner {
        return Err("owner");
    }
    if actor == Role::Admin && target == Role::Admin {
        return Err("peer");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use Role::*;

    #[test]
    fn role_rules() {
        assert_eq!(may_set_role(Owner, Developer, Admin, false), Ok(()));
        assert_eq!(may_set_role(Owner, Admin, Viewer, false), Ok(()));
        assert_eq!(may_set_role(Admin, Developer, Admin, false), Ok(()));
        assert_eq!(may_set_role(Admin, Admin, Viewer, false), Err("peer"));
        assert_eq!(may_set_role(Admin, Owner, Viewer, false), Err("owner"));
        assert_eq!(may_set_role(Owner, Admin, Owner, false), Err("use_transfer"));
        assert_eq!(may_set_role(Developer, Viewer, Developer, false), Err("forbidden"));
        assert_eq!(may_set_role(Admin, Viewer, Viewer, true), Err("self"));
        assert_eq!(may_remove(Owner, Admin), Ok(()));
        assert_eq!(may_remove(Admin, Admin), Err("peer"));
        assert_eq!(may_remove(Admin, Owner), Err("owner"));
        assert_eq!(may_remove(Viewer, Viewer), Err("forbidden"));
    }
}
