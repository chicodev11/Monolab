
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

const WAIT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelHost {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub role: PanelRole,
    #[serde(default)]
    pub hidden: Vec<String>,
    #[serde(default)]
    pub kind: PanelKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PanelKind {
    #[default]
    Hydrodactyl,
    Pterodactyl,
    Pelican,
}

impl PanelKind {
    pub fn fallback_daemon(self) -> &'static str {
        match self {
            PanelKind::Hydrodactyl => "elytra",
            _ => "",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PanelRole {
    #[default]
    User,
    Admin,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelHostView {
    #[serde(flatten)]
    pub host: PanelHost,
    pub has_key: bool,
    pub has_app_key: bool,
}

fn app_slot(id: &str) -> String {
    format!("{id}#app")
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelServer {
    pub identifier: String,
    pub name: String,
    pub node: String,
    pub daemon: String,
    pub memory_limit_mib: u32,
    pub port: u16,
    pub suspended: bool,
    #[serde(default)]
    pub owner: bool,
}

fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(dir)
}

fn hosts_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(folder(app)?.join("panel-hosts.json"))
}

fn secrets_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(folder(app)?.join("panel-secrets.json"))
}

pub fn load_hosts(app: &AppHandle) -> Result<Vec<PanelHost>, String> {
    let text = match std::fs::read_to_string(hosts_path(app)?) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Cannot read panel-hosts.json: {error}")),
    };
    serde_json::from_str(&text).map_err(|error| format!("Cannot read panel-hosts.json: {error}"))
}

fn save_hosts(app: &AppHandle, hosts: &[PanelHost]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(hosts).map_err(|error| error.to_string())?;
    std::fs::write(hosts_path(app)?, text)
        .map_err(|error| format!("Cannot write panel-hosts.json: {error}"))
}

fn load_secrets(app: &AppHandle) -> HashMap<String, String> {
    secrets_path(app)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_secrets(app: &AppHandle, secrets: &HashMap<String, String>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(secrets).map_err(|error| error.to_string())?;
    std::fs::write(secrets_path(app)?, text)
        .map_err(|error| format!("Cannot write panel-secrets.json: {error}"))
}

pub fn list_hosts(app: &AppHandle) -> Result<Vec<PanelHostView>, String> {
    let secrets = load_secrets(app);
    Ok(load_hosts(app)?
        .into_iter()
        .map(|host| PanelHostView {
            has_key: secrets.get(&host.id).is_some_and(|one| !one.is_empty()),
            has_app_key: secrets.get(&app_slot(&host.id)).is_some_and(|one| !one.is_empty()),
            host,
        })
        .collect())
}

pub fn host(app: &AppHandle, id: &str) -> Result<PanelHost, String> {
    load_hosts(app)?
        .into_iter()
        .find(|one| one.id == id)
        .ok_or_else(|| "Cannot find that registered panel.".to_string())
}

pub fn tidy_url(given: &str) -> Result<String, String> {
    let one = given.trim().trim_end_matches('/');
    if one.is_empty() {
        return Err("Enter the panel address.".into());
    }
    let bad = || format!("The panel address is malformed: {given}");
    let full = if one.starts_with("https://") || one.starts_with("http://") {
        one.to_string()
    } else {
        let probe = reqwest::Url::parse(&format!("http://{one}")).map_err(|_| bad())?;
        let home = probe.host_str().is_some_and(crate::net::is_private_host);
        format!("{}://{one}", if home { "http" } else { "https" })
    };
    let parsed = reqwest::Url::parse(&full).map_err(|_| bad())?;
    if parsed.scheme() == "http" && !parsed.host_str().is_some_and(crate::net::is_private_host) {
        return Err("Panels outside your network require https. The API key travels in every request header, so plain http would leak it to the internet. (Home addresses like 192.168.… may use http.)".into());
    }
    Ok(full)
}

pub fn save_host(
    app: &AppHandle,
    mut one: PanelHost,
    key: Option<String>,
    app_key: Option<String>,
) -> Result<(), String> {
    one.url = tidy_url(&one.url)?;
    if one.name.trim().is_empty() {
        one.name = one
            .url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("Panel")
            .to_string();
    }
    let have = load_secrets(app);
    let will_have = |slot: &str, given: &Option<String>| match given {
        Some(given) => !given.trim().is_empty(),
        None => have.get(slot).is_some_and(|one| !one.is_empty()),
    };
    if one.role == PanelRole::Admin && !will_have(&app_slot(&one.id), &app_key) {
        return Err("Admins need an Application API key.".into());
    }
    if one.role == PanelRole::User && !will_have(&one.id, &key) {
        return Err("Users need an account API key.".into());
    }
    let mut hosts = load_hosts(app)?;
    match hosts.iter_mut().find(|have| have.id == one.id) {
        Some(have) => {
            one.hidden = have.hidden.clone();
            *have = one.clone()
        }
        None => hosts.push(one.clone()),
    }
    save_hosts(app, &hosts)?;
    let mut secrets = load_secrets(app);
    for (slot, given) in [(one.id.clone(), key), (app_slot(&one.id), app_key)] {
        if let Some(given) = given {
            if given.trim().is_empty() {
                secrets.remove(&slot);
            } else {
                secrets.insert(slot, given.trim().to_string());
            }
        }
    }
    if one.role == PanelRole::User {
        secrets.remove(&app_slot(&one.id));
    }
    save_secrets(app, &secrets)?;
    Ok(())
}

pub fn hide_server(app: &AppHandle, host_id: &str, ident: &str) -> Result<(), String> {
    let mut hosts = load_hosts(app)?;
    if let Some(host) = hosts.iter_mut().find(|one| one.id == host_id) {
        if !host.hidden.iter().any(|one| one == ident) {
            host.hidden.push(ident.to_string());
        }
    }
    save_hosts(app, &hosts)
}

pub fn unhide_server(app: &AppHandle, host_id: &str, ident: &str) -> Result<(), String> {
    let mut hosts = load_hosts(app)?;
    if let Some(host) = hosts.iter_mut().find(|one| one.id == host_id) {
        host.hidden.retain(|one| one != ident);
    }
    save_hosts(app, &hosts)
}

pub fn delete_host(app: &AppHandle, id: &str) -> Result<(), String> {
    let mut hosts = load_hosts(app)?;
    hosts.retain(|one| one.id != id);
    save_hosts(app, &hosts)?;
    let mut secrets = load_secrets(app);
    let had = secrets.remove(id).is_some() | secrets.remove(&app_slot(id)).is_some();
    if had {
        save_secrets(app, &secrets)?;
    }
    Ok(())
}

pub struct Api {
    url: String,
    key: Option<String>,
    app_key: Option<String>,
    role: PanelRole,
    kind: PanelKind,
}

#[derive(Clone)]
enum Payload {
    None,
    Json(Value),
    Text(String),
}

pub struct Account {
    pub id: u32,
    pub username: String,
    pub admin: bool,
}

impl Api {
    pub fn new(url: &str, key: Option<&str>, app_key: Option<&str>, role: PanelRole) -> Result<Self, String> {
        let tidy = |one: Option<&str>| one.map(str::trim).filter(|one| !one.is_empty()).map(str::to_string);
        let (key, app_key) = (tidy(key), tidy(app_key));
        match role {
            PanelRole::User if key.is_none() => return Err("Enter the account API key.".into()),
            PanelRole::Admin if app_key.is_none() => return Err("Enter the Application API key.".into()),
            _ => {}
        }
        Ok(Self { url: tidy_url(url)?, key, app_key, role, kind: PanelKind::default() })
    }

    pub fn with_kind(mut self, kind: PanelKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn kind(&self) -> PanelKind {
        self.kind
    }

    /// Hydrodactyl puts the daemon in client paths and says which one in `meta.daemonType`;
    /// Pterodactyl and Pelican have neither, so the page title tells those apart.
    pub async fn detect_kind(&self, servers: &[PanelServer]) -> PanelKind {
        if let Some(first) = servers.first().filter(|_| self.key.is_some()) {
            if let Ok(got) = self.get(&format!("/servers/{}", first.identifier)).await {
                if got.pointer("/meta/daemonType").and_then(|one| one.as_str()).is_some_and(|one| !one.is_empty()) {
                    return PanelKind::Hydrodactyl;
                }
            }
        }
        let page = match self.client() {
            Ok(client) => match client.get(&self.url).send().await {
                Ok(answer) => answer.text().await.unwrap_or_default().to_lowercase(),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        };
        if page.contains("hydrodactyl") || page.contains("pyrodactyl") {
            PanelKind::Hydrodactyl
        } else if page.contains("pelican") {
            PanelKind::Pelican
        } else {
            PanelKind::Pterodactyl
        }
    }

    pub async fn check_application(&self) -> Result<(), String> {
        self.app_get("/servers?per_page=1").await.map(|_| ())
    }

    pub fn has_account_key(&self) -> bool {
        self.key.is_some()
    }

    fn client_long(&self) -> Result<reqwest::Client, String> {
        crate::net::panel_builder()
            .connect_timeout(WAIT)
            .timeout(Duration::from_secs(30 * 60))
            .build()
            .map_err(|error| format!("Cannot talk to the panel: {error}"))
    }

    fn client(&self) -> Result<reqwest::Client, String> {
        crate::net::panel_builder()
            .timeout(WAIT)
            .build()
            .map_err(|error| format!("Cannot talk to the panel: {error}"))
    }

    async fn send(&self, method: reqwest::Method, at: &str, body: Option<Value>) -> Result<Value, String> {
        let text = self
            .raw(method, at, &[], body.map(Payload::Json).unwrap_or(Payload::None))
            .await?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|_| "The panel's answer is not JSON. Check the address is the panel's.".to_string())
    }

    async fn raw(
        &self,
        method: reqwest::Method,
        at: &str,
        query: &[(&str, &str)],
        body: Payload,
    ) -> Result<String, String> {
        let application = at.starts_with("/api/application");
        let key = if application { self.app_key.as_ref() } else { self.key.as_ref() }.ok_or(if application {
            "No Application API key. Register the panel again as an admin."
        } else {
            "No account API key. Start, stop and console need an account key — add one in the panel registration."
        })?;
        let path = at;
        let at = format!("{}{at}", self.url);
        let mut keys = vec![key.clone()];
        if application {
            if let Some(account) = self.key.as_ref().filter(|one| *one != key) {
                keys.push(account.clone());
            }
        }
        let mut last = String::new();
        for (turn, key) in keys.iter().enumerate() {
            let mut ask = self
                .client()?
                .request(method.clone(), &at)
                .bearer_auth(key)
                .header("Accept", "application/json");
            if !query.is_empty() {
                ask = ask.query(query);
            }
            ask = match body.clone() {
                Payload::None => ask,
                Payload::Json(body) => ask.json(&body),
                Payload::Text(text) => ask.header("Content-Type", "text/plain").body(text),
            };
            let answer = ask
                .send()
                .await
                .map_err(|error| format!("Could not reach the panel: {}", brief(&error)))?;
            let code = answer.status();
            let text = answer.text().await.unwrap_or_default();
            if code.is_success() {
                return Ok(text);
            }
            last = format!("{} [{}]", explain(code, &text, application), path.split('?').next().unwrap_or(path));
            let retry = application && (code.as_u16() == 401 || code.as_u16() == 403) && turn + 1 < keys.len();
            if !retry {
                break;
            }
        }
        Err(last)
    }

    pub async fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let method = match method.to_ascii_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "PATCH" => reqwest::Method::PATCH,
            "DELETE" => reqwest::Method::DELETE,
            _ => return Err("That request cannot be sent.".into()),
        };
        if !path_is_safe(path) {
            return Err("That is not a panel API path.".into());
        }
        if path.starts_with("/api/application") {
            self.admin_only()?;
        }
        self.send(method, path, body).await
    }

    pub async fn my_servers(&self) -> Result<Vec<PanelServer>, String> {
        let mut out = Vec::new();
        for page in 1..=20 {
            let got = self.get(&format!("?include=allocations&per_page=100&page={page}")).await?;
            let rows = got.get("data").and_then(|one| one.as_array()).cloned().unwrap_or_default();
            out.extend(rows.iter().filter_map(read_server));
            let pages = got
                .pointer("/meta/pagination/total_pages")
                .and_then(|one| one.as_u64())
                .unwrap_or(1);
            if page as u64 >= pages || rows.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    pub async fn market_installed(&self, ident: &str) -> Vec<MarketInstall> {
        let Ok(got) = self.get(&format!("/servers/{ident}/marketplace/installed")).await else {
            return Vec::new();
        };
        got.get("installs")
            .and_then(|one| one.as_array())
            .map(|list| list.iter().filter_map(|one| serde_json::from_value(one.clone()).ok()).collect())
            .unwrap_or_default()
    }

    pub async fn market_forget(&self, ident: &str, record: &MarketInstall) -> Result<(), String> {
        let body = serde_json::json!({ "type": record.kind, "source": record.source, "project_id": record.project_id });
        self.send(reqwest::Method::DELETE, &format!("/api/client/servers/{ident}/marketplace/installed"), Some(body))
            .await
            .map(|_| ())
    }

    pub async fn market_record(&self, ident: &str, record: &MarketInstall) -> Result<(), String> {
        let body = serde_json::to_value(record).map_err(|error| error.to_string())?;
        self.send(reqwest::Method::POST, &format!("/api/client/servers/{ident}/marketplace/installed"), Some(body))
            .await
            .map(|_| ())
    }

    pub async fn egg_name(&self, ident: &str) -> Option<String> {
        let server = self.get(&format!("/servers/{ident}")).await.ok()?;
        let uuid = server.pointer("/attributes/egg")?.as_str()?.to_string();
        let nests = self.get("/nests?include=eggs").await.ok()?;
        nests.get("data")?.as_array()?.iter().find_map(|nest| {
            nest.pointer("/attributes/relationships/eggs/data")?.as_array()?.iter().find_map(|egg| {
                let at = egg.get("attributes")?;
                (at.get("uuid")?.as_str()? == uuid).then(|| at.get("name")?.as_str().map(str::to_string))?
            })
        })
    }

    pub async fn get(&self, path: &str) -> Result<Value, String> {
        self.send(reqwest::Method::GET, &format!("/api/client{path}"), None).await
    }

    fn admin_only(&self) -> Result<(), String> {
        if self.role != PanelRole::Admin {
            return Err("This panel is registered as a user. Register it as an admin to create servers.".into());
        }
        Ok(())
    }

    async fn app_get(&self, path: &str) -> Result<Value, String> {
        self.admin_only()?;
        self.send(reqwest::Method::GET, &format!("/api/application{path}"), None).await
    }

    async fn app_list(&self, path: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        let join = if path.contains('?') { '&' } else { '?' };
        for page in 1..=50 {
            let got = self.app_get(&format!("{path}{join}per_page=100&page={page}")).await?;
            let rows = got.get("data").and_then(|one| one.as_array()).cloned().unwrap_or_default();
            out.extend(rows.iter().filter_map(|row| row.get("attributes").cloned()));
            let pages = got
                .pointer("/meta/pagination/total_pages")
                .and_then(|one| one.as_u64())
                .unwrap_or(1);
            if page as u64 >= pages || rows.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    pub async fn account(&self) -> Result<Account, String> {
        let got = self.get("/account").await?;
        let at = got.get("attributes").unwrap_or(&got);
        Ok(Account {
            id: at.get("id").and_then(|one| one.as_u64()).unwrap_or(0) as u32,
            username: at.get("username").and_then(|one| one.as_str()).unwrap_or("").to_string(),
            admin: at.get("admin").and_then(|one| one.as_bool()).unwrap_or(false),
        })
    }

    pub async fn servers(&self) -> Result<Vec<PanelServer>, String> {
        if self.role == PanelRole::Admin && self.key.is_none() {
            return Ok(self
                .app_list("/servers?include=allocations")
                .await?
                .iter()
                .filter_map(read_app_server)
                .collect());
        }
        let scope = if self.role == PanelRole::Admin { "&type=admin-all" } else { "" };
        let mut out = Vec::new();
        for page in 1..=20 {
            let got = self
                .get(&format!("?include=allocations&per_page=100&page={page}{scope}"))
                .await?;
            let rows = got.get("data").and_then(|one| one.as_array()).cloned().unwrap_or_default();
            for row in &rows {
                if let Some(one) = read_server(row) {
                    out.push(one);
                }
            }
            let pages = got
                .pointer("/meta/pagination/total_pages")
                .and_then(|one| one.as_u64())
                .unwrap_or(1);
            if page as u64 >= pages || rows.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    pub async fn daemon(&self, ident: &str) -> Result<String, String> {
        let got = self.get(&format!("/servers/{ident}")).await?;
        Ok(got
            .pointer("/meta/daemonType")
            .and_then(|one| one.as_str())
            .filter(|one| !one.is_empty())
            .unwrap_or(self.kind.fallback_daemon())
            .to_string())
    }

    pub async fn permissions(&self, ident: &str) -> Result<Vec<String>, String> {
        let got = self.get(&format!("/servers/{ident}")).await?;
        Ok(got
            .pointer("/meta/user_permissions")
            .and_then(|one| one.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|one| one.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn create_options(&self) -> Result<PanelCreateOptions, String> {
        self.admin_only()?;
        let me = if self.key.is_some() { self.account().await.map(|one| one.id).unwrap_or(0) } else { 0 };
        let users = self
            .app_list("/users")
            .await?
            .iter()
            .map(|at| PanelUser {
                id: num(at, "id"),
                username: text(at, "username"),
                email: text(at, "email"),
            })
            .collect();
        let nodes = self
            .app_list("/nodes")
            .await?
            .iter()
            .map(|at| PanelNode { id: num(at, "id"), name: text(at, "name") })
            .collect();
        let mut eggs = Vec::new();
        for nest in self.app_list("/nests").await? {
            let nest_name = text(&nest, "name");
            let nest_id = num(&nest, "id");
            for egg in self.app_list(&format!("/nests/{nest_id}/eggs?include=variables")).await? {
                eggs.push(read_egg(&nest_name, &egg));
            }
        }
        Ok(PanelCreateOptions { me, users, nodes, eggs })
    }

    pub async fn free_allocations(&self, node: u32) -> Result<Vec<PanelAllocation>, String> {
        Ok(self
            .app_list(&format!("/nodes/{node}/allocations"))
            .await?
            .iter()
            .filter(|at| !at.get("assigned").and_then(|one| one.as_bool()).unwrap_or(false))
            .map(|at| PanelAllocation {
                id: num(at, "id"),
                ip: text(at, "ip"),
                alias: text(at, "alias"),
                port: num(at, "port") as u16,
            })
            .collect())
    }

    pub async fn create_server(&self, new: &PanelNewServer) -> Result<String, String> {
        self.admin_only()?;
        if new.name.trim().is_empty() {
            return Err("Please enter a server name.".into());
        }
        let body = create_body(new);
        let got = self
            .send(reqwest::Method::POST, "/api/application/servers", Some(body))
            .await?;
        Ok(got
            .pointer("/attributes/identifier")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string())
    }
}

#[derive(Clone, Debug)]
pub struct Target {
    pub ident: String,
    pub daemon: String,
}

impl Target {
    fn at(&self, path: &str) -> String {
        if self.daemon.is_empty() {
            format!("/api/client/servers/{}{path}", self.ident)
        } else {
            format!("/api/client/servers/{}/{}{path}", self.daemon, self.ident)
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelStats {
    pub state: String,
    pub cpu: f64,
    pub memory_bytes: f64,
    pub memory_limit_bytes: f64,
    pub disk_bytes: f64,
    pub rx_bytes: f64,
    pub tx_bytes: f64,
    pub uptime_ms: f64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelBackup {
    pub uuid: String,
    pub name: String,
    pub bytes: f64,
    pub created_at: u64,
    pub completed: bool,
    pub successful: bool,
    pub locked: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelVariable {
    pub name: String,
    pub description: String,
    pub env: String,
    pub value: String,
    pub default_value: String,
    pub editable: bool,
    pub rules: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelStartup {
    pub command: String,
    pub docker_image: String,
    pub variables: Vec<PanelVariable>,
}

fn iso_seconds(value: Option<&Value>) -> u64 {
    value
        .and_then(|one| one.as_str())
        .and_then(|one| chrono::DateTime::parse_from_rfc3339(one).ok())
        .map(|one| one.timestamp().max(0) as u64)
        .unwrap_or(0)
}

fn float(at: &Value, pointer: &str) -> f64 {
    at.pointer(pointer).and_then(|one| one.as_f64()).unwrap_or(0.0)
}

pub fn panel_path(relative: &str) -> Result<String, String> {
    let trimmed = relative.trim().trim_matches(['/', '\\']).replace('\\', "/");
    if trimmed.is_empty() {
        return Ok("/".into());
    }
    crate::paths::check_relative(&trimmed).map_err(|error| error.to_string())?;
    Ok(format!("/{trimmed}"))
}

pub fn split_path(relative: &str) -> Result<(String, String), String> {
    let full = panel_path(relative)?;
    let (dir, name) = full.rsplit_once('/').unwrap_or(("", &full));
    if name.is_empty() {
        return Err("The server folder itself cannot be changed.".into());
    }
    Ok((if dir.is_empty() { "/".into() } else { dir.to_string() }, name.to_string()))
}

impl Api {
    pub async fn power(&self, target: &Target, signal: &str) -> Result<(), String> {
        self.send(reqwest::Method::POST, &target.at("/power"), Some(serde_json::json!({ "signal": signal })))
            .await
            .map(|_| ())
    }

    pub async fn command(&self, target: &Target, line: &str) -> Result<(), String> {
        self.send(reqwest::Method::POST, &target.at("/command"), Some(serde_json::json!({ "command": line })))
            .await
            .map(|_| ())
    }

    pub async fn stats(&self, target: &Target) -> Result<PanelStats, String> {
        let got = self.send(reqwest::Method::GET, &target.at("/resources"), None).await?;
        let at = got.get("attributes").unwrap_or(&got);
        Ok(PanelStats {
            state: at.get("current_state").and_then(|one| one.as_str()).unwrap_or("offline").to_string(),
            cpu: float(at, "/resources/cpu_absolute"),
            memory_bytes: float(at, "/resources/memory_bytes"),
            memory_limit_bytes: float(at, "/resources/memory_limit_bytes"),
            disk_bytes: float(at, "/resources/disk_bytes"),
            rx_bytes: float(at, "/resources/network_rx_bytes"),
            tx_bytes: float(at, "/resources/network_tx_bytes"),
            uptime_ms: float(at, "/resources/uptime"),
        })
    }

    pub async fn socket(&self, target: &Target) -> Result<(String, String), String> {
        let got = self.send(reqwest::Method::GET, &target.at("/websocket"), None).await?;
        let socket = got.pointer("/data/socket").and_then(|one| one.as_str()).unwrap_or("");
        let token = got.pointer("/data/token").and_then(|one| one.as_str()).unwrap_or("");
        if socket.is_empty() || token.is_empty() {
            return Err("The panel gave no console address. Check you have console permission on this server.".into());
        }
        Ok((socket.to_string(), token.to_string()))
    }

    pub async fn list(&self, target: &Target, relative: &str) -> Result<Vec<crate::remote::FileRow>, String> {
        let dir = panel_path(relative)?;
        let text = self
            .raw(reqwest::Method::GET, &target.at("/files/list"), &[("directory", &dir)], Payload::None)
            .await?;
        let got: Value = serde_json::from_str(&text).map_err(|_| "Cannot read the panel's file list.".to_string())?;
        let mut rows: Vec<crate::remote::FileRow> = got
            .get("data")
            .and_then(|one| one.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|row| row.get("attributes"))
                    .map(|at| crate::remote::FileRow {
                        name: text_of(at, "name"),
                        directory: !at.get("is_file").and_then(|one| one.as_bool()).unwrap_or(true),
                        size_bytes: at.get("size").and_then(|one| one.as_u64()).unwrap_or(0),
                        modified_at: iso_seconds(at.get("modified_at")),
                    })
                    .filter(|one| !one.name.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        rows.sort_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(rows)
    }

    pub async fn exists(&self, target: &Target, relative: &str) -> Result<Option<bool>, String> {
        let (dir, name) = split_path(relative)?;
        let rows = match self.list(target, &dir).await {
            Ok(rows) => rows,
            Err(error) if error.contains("(404") => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(rows.iter().find(|one| one.name == name).map(|one| one.directory))
    }

    pub async fn read_text(&self, target: &Target, relative: &str) -> Result<String, String> {
        let file = panel_path(relative)?;
        let text = self
            .raw(reqwest::Method::GET, &target.at("/files/contents"), &[("file", &file)], Payload::None)
            .await?;
        if text.len() > 4 * 1024 * 1024 {
            return Err("This file is too large to open in the app (over 4 MB).".into());
        }
        Ok(text)
    }

    pub async fn write_text(&self, target: &Target, relative: &str, text: &str) -> Result<(), String> {
        let file = panel_path(relative)?;
        self.raw(reqwest::Method::POST, &target.at("/files/write"), &[("file", &file)], Payload::Text(text.to_string()))
            .await
            .map(|_| ())
    }

    pub async fn download_url(&self, target: &Target, relative: &str) -> Result<String, String> {
        let file = panel_path(relative)?;
        let text = self
            .raw(reqwest::Method::GET, &target.at("/files/download"), &[("file", &file)], Payload::None)
            .await?;
        let got: Value = serde_json::from_str(&text).map_err(|_| "The panel gave no download address.".to_string())?;
        got.pointer("/attributes/url")
            .and_then(|one| one.as_str())
            .map(str::to_string)
            .ok_or("The panel gave no download address.".into())
    }

    pub async fn read_bytes(&self, target: &Target, relative: &str, limit: u64) -> Result<Vec<u8>, String> {
        let url = self.download_url(target, relative).await?;
        let mut answer = self
            .client()?
            .get(&url)
            .send()
            .await
            .map_err(|error| format!("Could not download the file from the daemon: {}", brief(&error)))?;
        if !answer.status().is_success() {
            return Err(format!("The daemon did not send the file ({}).", answer.status()));
        }
        let mut out = Vec::new();
        while let Some(chunk) = answer.chunk().await.map_err(|error| format!("The download was cut off: {}", brief(&error)))? {
            out.extend_from_slice(&chunk);
            if out.len() as u64 > limit {
                return Err(format!("The file is too large (over {} MB).", limit / 1024 / 1024));
            }
        }
        Ok(out)
    }

    pub async fn read_head(&self, target: &Target, relative: &str, limit: usize) -> Result<Vec<u8>, String> {
        let url = self.download_url(target, relative).await?;
        let mut answer = self
            .client()?
            .get(&url)
            .send()
            .await
            .map_err(|error| format!("Could not download the file from the daemon: {}", brief(&error)))?;
        if !answer.status().is_success() {
            return Err(format!("The daemon did not send the file ({}).", answer.status()));
        }
        let mut out = Vec::new();
        while let Some(chunk) = answer.chunk().await.map_err(|error| format!("The download was cut off: {}", brief(&error)))? {
            out.extend_from_slice(&chunk);
            if out.len() >= limit {
                out.truncate(limit);
                break;
            }
        }
        Ok(out)
    }

    pub async fn download_to<F: FnMut(u64, u64)>(
        &self,
        target: &Target,
        relative: &str,
        destination: &std::path::Path,
        mut tick: F,
    ) -> Result<u64, String> {
        use std::io::Write;
        let url = self.download_url(target, relative).await?;
        let mut answer = self
            .client_long()?
            .get(&url)
            .send()
            .await
            .map_err(|error| format!("Could not download the file from the daemon: {}", brief(&error)))?;
        if !answer.status().is_success() {
            return Err(format!("The daemon did not send the file ({}).", answer.status()));
        }
        let total = answer.content_length().unwrap_or(0);
        let part = destination.with_extension("part");
        let mut out = std::fs::File::create(&part).map_err(|error| format!("Cannot create the file to download into: {error}"))?;
        let mut sent = 0u64;
        while let Some(chunk) = answer.chunk().await.map_err(|error| format!("The download was cut off: {}", brief(&error)))? {
            out.write_all(&chunk).map_err(|error| error.to_string())?;
            sent += chunk.len() as u64;
            tick(sent, total);
        }
        drop(out);
        std::fs::rename(&part, destination).map_err(|error| format!("Cannot move the downloaded file: {error}"))?;
        Ok(sent)
    }

    pub async fn upload(&self, target: &Target, relative_dir: &str, name: &str, bytes: Vec<u8>) -> Result<(), String> {
        let dir = panel_path(relative_dir)?;
        let got = self.send(reqwest::Method::GET, &target.at("/files/upload"), None).await?;
        let url = got
            .pointer("/attributes/url")
            .and_then(|one| one.as_str())
            .ok_or("The panel gave no upload address.")?;
        let part = reqwest::multipart::Part::bytes(bytes).file_name(name.to_string());
        let form = reqwest::multipart::Form::new().part("files", part);
        let answer = self
            .client_long()?
            .post(url)
            .query(&[("directory", dir.as_str())])
            .multipart(form)
            .send()
            .await
            .map_err(|error| format!("Could not upload to the daemon: {}", brief(&error)))?;
        if !answer.status().is_success() {
            let code = answer.status();
            let body = answer.text().await.unwrap_or_default();
            return Err(format!("The daemon did not accept the upload ({code}) {}", body.chars().take(160).collect::<String>()));
        }
        Ok(())
    }

    pub async fn rename(&self, target: &Target, from: &str, to: &str) -> Result<(), String> {
        let from = panel_path(from)?;
        let to = panel_path(to)?;
        self.send(
            reqwest::Method::PUT,
            &target.at("/files/rename"),
            Some(serde_json::json!({ "root": "/", "files": [{ "from": from.trim_start_matches('/'), "to": to.trim_start_matches('/') }] })),
        )
        .await
        .map(|_| ())
    }

    pub async fn delete(&self, target: &Target, relatives: &[String]) -> Result<(), String> {
        let mut by_dir: HashMap<String, Vec<String>> = HashMap::new();
        for one in relatives {
            let (dir, name) = split_path(one)?;
            by_dir.entry(dir).or_default().push(name);
        }
        for (dir, names) in by_dir {
            self.send(
                reqwest::Method::POST,
                &target.at("/files/delete"),
                Some(serde_json::json!({ "root": dir, "files": names })),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn make_dir(&self, target: &Target, relative_dir: &str, name: &str) -> Result<(), String> {
        let dir = panel_path(relative_dir)?;
        self.send(
            reqwest::Method::POST,
            &target.at("/files/create-folder"),
            Some(serde_json::json!({ "root": dir, "name": name })),
        )
        .await
        .map(|_| ())
    }

    pub async fn compress(&self, target: &Target, relative_dir: &str, names: &[String]) -> Result<String, String> {
        let dir = panel_path(relative_dir)?;
        let got = self
            .send(
                reqwest::Method::POST,
                &target.at("/files/compress"),
                Some(serde_json::json!({ "root": dir, "files": names })),
            )
            .await?;
        Ok(got.pointer("/attributes/name").and_then(|one| one.as_str()).unwrap_or("").to_string())
    }

    pub async fn decompress(&self, target: &Target, relative: &str) -> Result<(), String> {
        let (dir, name) = split_path(relative)?;
        self.send(
            reqwest::Method::POST,
            &target.at("/files/decompress"),
            Some(serde_json::json!({ "root": dir, "file": name })),
        )
        .await
        .map(|_| ())
    }

    pub async fn backups(&self, target: &Target) -> Result<Vec<PanelBackup>, String> {
        let mut out = Vec::new();
        for page in 1..=20 {
            let text = self
                .raw(
                    reqwest::Method::GET,
                    &target.at("/backups"),
                    &[("per_page", "50"), ("page", &page.to_string())],
                    Payload::None,
                )
                .await?;
            let got: Value = serde_json::from_str(&text).map_err(|_| "Cannot read the panel's backup list.".to_string())?;
            let rows = got.get("data").and_then(|one| one.as_array()).cloned().unwrap_or_default();
            for at in rows.iter().filter_map(|row| row.get("attributes")) {
                out.push(PanelBackup {
                    uuid: text_of(at, "uuid"),
                    name: text_of(at, "name"),
                    bytes: at.get("bytes").and_then(|one| one.as_f64()).unwrap_or(0.0),
                    created_at: iso_seconds(at.get("created_at")),
                    completed: at.get("completed_at").is_some_and(|one| !one.is_null()),
                    successful: at.get("is_successful").and_then(|one| one.as_bool()).unwrap_or(false),
                    locked: at.get("is_locked").and_then(|one| one.as_bool()).unwrap_or(false),
                });
            }
            let pages = got.pointer("/meta/pagination/total_pages").and_then(|one| one.as_u64()).unwrap_or(1);
            if page as u64 >= pages || rows.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    pub async fn create_backup(&self, target: &Target, name: &str) -> Result<(), String> {
        self.send(reqwest::Method::POST, &target.at("/backups"), Some(serde_json::json!({ "name": name })))
            .await
            .map(|_| ())
    }

    pub async fn restore_backup(&self, target: &Target, uuid: &str) -> Result<(), String> {
        self.send(
            reqwest::Method::POST,
            &target.at(&format!("/backups/{uuid}/restore")),
            Some(serde_json::json!({ "truncate": false, "truncate_directory": false })),
        )
        .await
        .map(|_| ())
    }

    pub async fn delete_backup(&self, target: &Target, uuid: &str) -> Result<(), String> {
        self.send(reqwest::Method::DELETE, &target.at(&format!("/backups/{uuid}")), None)
            .await
            .map(|_| ())
    }

    pub async fn startup(&self, target: &Target) -> Result<PanelStartup, String> {
        let got = self.send(reqwest::Method::GET, &target.at("/startup"), None).await?;
        let variables = got
            .get("data")
            .and_then(|one| one.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|row| row.get("attributes"))
                    .map(|at| PanelVariable {
                        name: text_of(at, "name"),
                        description: text_of(at, "description"),
                        env: text_of(at, "env_variable"),
                        value: plain(at.get("server_value")),
                        default_value: plain(at.get("default_value")),
                        editable: at.get("is_editable").and_then(|one| one.as_bool()).unwrap_or(false),
                        rules: text_of(at, "rules"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(PanelStartup {
            command: got.pointer("/meta/startup_command").and_then(|one| one.as_str()).unwrap_or("").to_string(),
            docker_image: got
                .pointer("/meta/docker_image")
                .and_then(|one| one.as_str())
                .unwrap_or("")
                .to_string(),
            variables,
        })
    }

    pub async fn set_variable(&self, target: &Target, env: &str, value: &str) -> Result<(), String> {
        self.send(
            reqwest::Method::PUT,
            &target.at("/startup/variable"),
            Some(serde_json::json!({ "key": env, "value": value })),
        )
        .await
        .map(|_| ())
    }
}

fn text_of(at: &Value, key: &str) -> String {
    at.get(key).and_then(|one| one.as_str()).unwrap_or("").to_string()
}

fn plain(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(one)) => one.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn create_body(new: &PanelNewServer) -> Value {
    serde_json::json!({
        "name": new.name.trim(),
        "user": new.owner,
        "egg": new.egg,
        "docker_image": new.docker_image,
        "startup": new.startup,
        "environment": new.environment,
        "limits": {
            "memory": new.memory_mib,
            "swap": 0,
            "disk": new.disk_mib,
            "io": 500,
            "cpu": new.cpu,
        },
        "feature_limits": { "databases": 0, "allocations": 1, "backups": 3 },
        "allocation": { "default": new.allocation },
        "start_on_completion": false,
    })
}

fn num(at: &Value, key: &str) -> u32 {
    at.get(key).and_then(|one| one.as_u64()).unwrap_or(0) as u32
}

fn text(at: &Value, key: &str) -> String {
    at.get(key).and_then(|one| one.as_str()).unwrap_or("").to_string()
}

fn read_egg(nest: &str, at: &Value) -> PanelEgg {
    let images = at
        .get("docker_images")
        .and_then(|one| one.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(label, image)| {
                    image.as_str().map(|image| PanelImage { label: label.clone(), image: image.to_string() })
                })
                .collect::<Vec<_>>()
        })
        .filter(|list| !list.is_empty())
        .unwrap_or_else(|| {
            let one = text(at, "docker_image");
            if one.is_empty() { Vec::new() } else { vec![PanelImage { label: one.clone(), image: one }] }
        });
    let variables = at
        .pointer("/relationships/variables/data")
        .and_then(|one| one.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|row| row.get("attributes"))
                .map(|var| PanelEggVariable {
                    name: text(var, "name"),
                    env: text(var, "env_variable"),
                    default_value: var
                        .get("default_value")
                        .map(|one| match one {
                            Value::String(one) => one.clone(),
                            Value::Null => String::new(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default(),
                    description: text(var, "description"),
                })
                .filter(|var| !var.env.is_empty())
                .collect()
        })
        .unwrap_or_default();
    PanelEgg {
        id: num(at, "id"),
        nest: nest.to_string(),
        name: text(at, "name"),
        images,
        startup: text(at, "startup"),
        variables,
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelUser {
    pub id: u32,
    pub username: String,
    pub email: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelNode {
    pub id: u32,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelImage {
    pub label: String,
    pub image: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelEggVariable {
    pub name: String,
    pub env: String,
    pub default_value: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelEgg {
    pub id: u32,
    pub nest: String,
    pub name: String,
    pub images: Vec<PanelImage>,
    pub startup: String,
    pub variables: Vec<PanelEggVariable>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelCreateOptions {
    pub me: u32,
    pub users: Vec<PanelUser>,
    pub nodes: Vec<PanelNode>,
    pub eggs: Vec<PanelEgg>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelAllocation {
    pub id: u32,
    pub ip: String,
    pub alias: String,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelNewServer {
    pub name: String,
    pub owner: u32,
    pub egg: u32,
    pub docker_image: String,
    pub startup: String,
    pub environment: HashMap<String, String>,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub cpu: u32,
    pub allocation: u32,
}

pub fn saved_keys(app: &AppHandle, id: &str) -> (Option<String>, Option<String>) {
    let secrets = load_secrets(app);
    (secrets.get(id).cloned(), secrets.get(&app_slot(id)).cloned())
}

pub fn api(app: &AppHandle, host_id: &str) -> Result<Api, String> {
    let one = host(app, host_id)?;
    let secrets = load_secrets(app);
    Api::new(
        &one.url,
        secrets.get(host_id).map(String::as_str),
        secrets.get(&app_slot(host_id)).map(String::as_str),
        one.role,
    )
    .map(|api| api.with_kind(one.kind))
    .map_err(|_| "No panel API key. Register it again in Manage sessions.".to_string())
}

fn read_app_server(at: &Value) -> Option<PanelServer> {
    let identifier = at.get("identifier").and_then(|one| one.as_str())?.to_string();
    let default = at.get("allocation").and_then(|one| one.as_u64());
    let port = at
        .pointer("/relationships/allocations/data")
        .and_then(|one| one.as_array())
        .and_then(|list| {
            list.iter()
                .find(|one| one.pointer("/attributes/id").and_then(|one| one.as_u64()) == default)
                .or_else(|| list.first())
        })
        .and_then(|one| one.pointer("/attributes/port"))
        .and_then(|one| one.as_u64())
        .unwrap_or(0) as u16;
    Some(PanelServer {
        identifier,
        name: text(at, "name"),
        node: at.get("node").map(|one| one.to_string()).unwrap_or_default(),
        daemon: String::new(),
        memory_limit_mib: at.pointer("/limits/memory").and_then(|one| one.as_u64()).unwrap_or(0) as u32,
        port,
        suspended: at.get("suspended").and_then(|one| one.as_bool()).unwrap_or(false),
        owner: false,
    })
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
pub struct MarketInstall {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_title: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub version_name: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
}

pub fn path_is_safe(path: &str) -> bool {
    (path.starts_with("/api/client") || path.starts_with("/api/application"))
        && !path.contains("://")
        && !path.contains("..")
        && !path.contains('\\')
        && !path.chars().any(|one| one.is_whitespace() || one.is_control())
}

fn read_server(row: &Value) -> Option<PanelServer> {
    let at = row.pointer("/attributes")?;
    let identifier = at.get("identifier").and_then(|one| one.as_str())?.to_string();
    let port = at
        .pointer("/relationships/allocations/data")
        .and_then(|one| one.as_array())
        .and_then(|list| {
            list.iter()
                .find(|one| {
                    one.pointer("/attributes/is_default")
                        .and_then(|one| one.as_bool())
                        .unwrap_or(false)
                })
                .or_else(|| list.first())
        })
        .and_then(|one| one.pointer("/attributes/port"))
        .and_then(|one| one.as_u64())
        .unwrap_or(0) as u16;
    Some(PanelServer {
        identifier,
        name: at
            .get("name")
            .and_then(|one| one.as_str())
            .unwrap_or("Unnamed server")
            .to_string(),
        node: at.get("node").and_then(|one| one.as_str()).unwrap_or("").to_string(),
        daemon: String::new(),
        memory_limit_mib: at
            .pointer("/limits/memory")
            .and_then(|one| one.as_u64())
            .unwrap_or(0) as u32,
        port,
        suspended: at
            .get("is_suspended")
            .and_then(|one| one.as_bool())
            .unwrap_or(false),
        owner: at.get("server_owner").and_then(|one| one.as_bool()).unwrap_or(false),
    })
}

fn explain(code: reqwest::StatusCode, body: &str, application: bool) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|one| {
            one.pointer("/errors/0/detail")
                .or_else(|| one.pointer("/errors/0/code"))
                .and_then(|one| one.as_str().map(str::to_string))
        })
        .unwrap_or_default();
    if detail.contains("auto-allocation is not enabled") {
        return "Automatic port allocation is off in the panel. Turn on Automatic Allocation under Settings > Advanced in the admin area, or create ports under Hydrodactyl > Nodes and assign them.".into();
    }
    if detail.contains("No database host") {
        return "The panel has no database host. Register a MySQL host under Databases in the admin area first.".into();
    }
    let head = match code.as_u16() {
        401 | 403 if application => "The panel rejected the Application key. Check it was made under Application API in the admin area with read access to servers, nodes, users, eggs and allocations and write access to servers.",
        401 | 403 => "The panel rejected the account key. Check it was made under API Credentials in your account (Application keys do not work here) and that you have access to that server.",
        404 => "The panel could not find that. Check the address and that the server still exists.",
        409 => "The panel says it cannot do that now. The server is off or already in that state.",
        422 => "The panel rejected the values.",
        429 => "The panel says there were too many requests. Try again shortly.",
        502..=504 => "The panel could not reach the daemon. The node may be down.",
        _ => "The panel returned an error.",
    };
    if detail.is_empty() {
        format!("{head} ({code})")
    } else {
        format!("{head} ({code} · {detail})")
    }
}

fn brief(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        return "timed out".into();
    }
    if error.is_connect() {
        return "could not connect".into();
    }
    error
        .source()
        .map(|one| one.to_string())
        .unwrap_or_else(|| error.to_string())
}

use std::error::Error as _;

#[cfg(test)]
mod tests {
    #[test]
    fn puts_the_daemon_in_paths_only_for_hydrodactyl() {
        let hydro = super::Target { ident: "abc".into(), daemon: "wings".into() };
        assert_eq!(hydro.at("/power"), "/api/client/servers/wings/abc/power");
        let ptero = super::Target { ident: "abc".into(), daemon: String::new() };
        assert_eq!(ptero.at("/power"), "/api/client/servers/abc/power");
        assert_eq!(super::PanelKind::Hydrodactyl.fallback_daemon(), "elytra");
        assert_eq!(super::PanelKind::Pelican.fallback_daemon(), "");
    }

    use super::*;

    #[test]
    fn keeps_https_and_refuses_plain_http_outside() {
        assert_eq!(tidy_url(" panel.example.com/ ").unwrap(), "https://panel.example.com");
        assert_eq!(tidy_url("https://panel.example.com").unwrap(), "https://panel.example.com");
        assert!(tidy_url("http://panel.example.com").is_err());
        assert!(tidy_url("http://8.8.8.8").is_err());
        assert!(tidy_url("  ").is_err());
    }

    #[test]
    fn takes_plain_http_inside_the_house() {
        assert_eq!(tidy_url("http://192.168.0.20").unwrap(), "http://192.168.0.20");
        assert_eq!(tidy_url("http://192.168.0.20:8080/").unwrap(), "http://192.168.0.20:8080");
        assert_eq!(tidy_url("192.168.0.20").unwrap(), "http://192.168.0.20");
        assert_eq!(tidy_url("panel.lan").unwrap(), "http://panel.lan");
        assert_eq!(tidy_url("https://192.168.0.20").unwrap(), "https://192.168.0.20");
    }

    #[test]
    fn builds_what_store_server_request_wants() {
        let new = PanelNewServer {
            name: " New server ".into(),
            owner: 1,
            egg: 3,
            docker_image: "ghcr.io/pterodactyl/yolks:java_21".into(),
            startup: "java -jar {{SERVER_JARFILE}}".into(),
            environment: HashMap::from([("SERVER_JARFILE".to_string(), "server.jar".to_string())]),
            memory_mib: 4096,
            disk_mib: 10240,
            cpu: 0,
            allocation: 7,
        };
        let body = create_body(&new);
        assert_eq!(body["name"], "New server");
        assert_eq!(body["allocation"]["default"], 7);
        assert_eq!(body["limits"]["io"], 500);
        assert_eq!(body["environment"]["SERVER_JARFILE"], "server.jar");
    }

    #[test]
    fn reads_an_egg_with_its_images_and_variables() {
        let egg = serde_json::json!({
            "id": 3, "name": "Paper", "startup": "java -jar x",
            "docker_images": { "Java 21": "yolks:java_21", "Java 17": "yolks:java_17" },
            "relationships": { "variables": { "data": [
                { "attributes": { "name": "version", "env_variable": "MINECRAFT_VERSION", "default_value": "latest", "description": "" } },
                { "attributes": { "name": "build", "env_variable": "BUILD_NUMBER", "default_value": null } }
            ] } }
        });
        let one = read_egg("Minecraft", &egg);
        assert_eq!(one.images.len(), 2);
        assert!(one.images.iter().any(|img| img.image == "yolks:java_21" && img.label == "Java 21"));
        assert_eq!(one.variables[0].default_value, "latest");
        assert_eq!(one.variables[1].default_value, "");
    }

    #[test]
    fn reads_a_server_row_and_picks_the_default_port() {
        let row = serde_json::json!({
            "attributes": {
                "identifier": "d3aac109",
                "name": "Our server",
                "node": "seoul-1",
                "is_suspended": false,
                "limits": { "memory": 4096 },
                "relationships": { "allocations": { "data": [
                    { "attributes": { "port": 25580, "is_default": false } },
                    { "attributes": { "port": 25565, "is_default": true } }
                ] } }
            }
        });
        let one = read_server(&row).expect("one line");
        assert_eq!(one.identifier, "d3aac109");
        assert_eq!(one.memory_limit_mib, 4096);
        assert_eq!(one.port, 25565);
    }

    #[test]
    fn throws_away_rows_it_cannot_read() {
        assert!(read_server(&serde_json::json!({ "attributes": { "name": "no id" } })).is_none());
        assert!(read_server(&serde_json::json!({})).is_none());
    }

    #[test]
    fn says_what_the_panel_complained_about() {
        let said = explain(
            reqwest::StatusCode::UNAUTHORIZED,
            "{\"errors\":[{\"code\":\"InvalidKey\",\"detail\":\"wrong key\"}]}",
            false,
        );
        assert!(explain(reqwest::StatusCode::FORBIDDEN, "", true).contains("Application"));
        assert!(said.contains("account key"), "{said}");
        assert!(said.contains("wrong key"), "{said}");
        let html = explain(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "<html>…tens of KB…</html>", false);
        assert!(!html.contains("html"), "{html}");
    }
}
