use crate::data_root::MonolabDirs;
use crate::db_tls;
use mysql_async::prelude::Queryable;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const WAIT: Duration = Duration::from_secs(12);
const QUERY_WAIT: Duration = Duration::from_secs(30);
pub const PAGE: u32 = 100;
const CELL_LIMIT: usize = 2000;

/// A MySQL or MariaDB account the app may create databases with.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbHost {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub ssh_host: String,
    /// The last connection went over the network without encryption.
    #[serde(default)]
    pub plain: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbHostView {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub ssh_host: String,
    pub has_password: bool,
    pub databases: u32,
    pub plain: bool,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbHostInput {
    pub id: Option<String>,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub ssh_host: String,
}

/// An existing database someone else made, added by its login details.
#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbLinkInput {
    pub address: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub ssh_host: String,
}

/// One server's database. Without `host_id` it was linked, so the app never drops it.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerDb {
    pub id: String,
    pub server_id: String,
    #[serde(default)]
    pub host_id: String,
    pub database: String,
    pub username: String,
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub ssh_host: String,
    pub remote: String,
    #[serde(default)]
    pub plain: bool,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TablePage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub more: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    #[serde(default)]
    hosts: Vec<DbHost>,
    #[serde(default)]
    databases: Vec<ServerDb>,
}

static WRITING: Mutex<()> = Mutex::new(());

fn book_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join("databases.json"))
}

fn secrets_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(book_path(app)?.with_file_name("database-secrets.json"))
}

fn load(app: &AppHandle) -> Result<Book, String> {
    Ok(crate::store::read(&book_path(app)?)?.unwrap_or_default())
}

fn keep(app: &AppHandle, book: &Book) -> Result<(), String> {
    let text = serde_json::to_string_pretty(book).map_err(|error| error.to_string())?;
    crate::store::write(&book_path(app)?, &text).map_err(|error| format!("Could not write databases.json: {error}"))
}

fn pins_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(book_path(app)?.with_file_name("database-pins.json"))
}

/// Certificate fingerprints trusted on first use, by `address:port`.
fn pins(app: &AppHandle) -> Result<HashMap<String, String>, String> {
    Ok(crate::store::read(&pins_path(app)?)?.unwrap_or_default())
}

fn pin_key(address: &str, port: u16) -> String {
    format!("{}:{port}", address.trim().to_ascii_lowercase())
}

fn set_pin(app: &AppHandle, key: &str, value: Option<&str>) -> Result<(), String> {
    let _guard = WRITING.lock().expect("database store poisoned");
    let mut all = pins(app)?;
    match value {
        Some(value) => all.insert(key.to_string(), value.to_string()),
        None => all.remove(key),
    };
    let text = serde_json::to_string_pretty(&all).map_err(|error| error.to_string())?;
    crate::store::write(&pins_path(app)?, &text).map_err(|error| format!("Could not write database-pins.json: {error}"))
}

fn secrets(app: &AppHandle) -> Result<HashMap<String, String>, String> {
    Ok(crate::store::read_secret(&secrets_path(app)?)?.unwrap_or_default())
}

fn keep_secrets(app: &AppHandle, all: &HashMap<String, String>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(all).map_err(|error| error.to_string())?;
    crate::store::write_secret(&secrets_path(app)?, &text).map_err(|error| format!("Could not write database-secrets.json: {error}"))
}

fn secret_of(app: &AppHandle, id: &str) -> Result<String, String> {
    secrets(app)?.get(id).cloned().ok_or("No password saved for this database.".into())
}

fn set_secret(app: &AppHandle, id: &str, value: Option<&str>) -> Result<(), String> {
    let _guard = WRITING.lock().expect("database store poisoned");
    let mut all = secrets(app)?;
    match value {
        Some(value) => all.insert(id.to_string(), value.to_string()),
        None => all.remove(id),
    };
    keep_secrets(app, &all)
}

fn random_text(length: usize) -> Result<String, String> {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::with_capacity(length);
    while out.len() < length {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| format!("Could not make a password: {error}"))?;
        for byte in bytes {
            // 248 is the largest multiple of 62 in a byte, so every letter is equally likely.
            if byte < 248 && out.len() < length {
                out.push(LETTERS[byte as usize % LETTERS.len()] as char);
            }
        }
    }
    Ok(out)
}

fn server_tag(server_id: &str) -> String {
    sha2::Sha256::digest(server_id.as_bytes()).iter().take(3).map(|byte| format!("{byte:02x}")).collect()
}

fn database_name(server_id: &str, given: &str) -> Result<String, String> {
    let given = given.trim();
    if given.is_empty() || given.len() > 32 || !given.chars().all(|one| one.is_ascii_alphanumeric() || one == '_') {
        return Err("Database names can use up to 32 English letters, numbers and underscores.".into());
    }
    Ok(format!("s{}_{given}", server_tag(server_id)))
}

fn ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// `_` and `%` are wildcards in a GRANT's database name, which would reach other databases.
fn grant_ident(name: &str) -> String {
    let slash = char::from(92);
    ident(&name.replace('_', &format!("{slash}_")).replace('%', &format!("{slash}%")))
}

fn text_literal(value: &str) -> String {
    let slash = char::from(92);
    format!("'{}'", value.replace(slash, &format!("{slash}{slash}")).replace('\'', "''"))
}

fn check_endpoint(address: &str, port: u16) -> Result<(), String> {
    if address.trim().is_empty() || address.chars().any(|one| one.is_whitespace() || one == '/' || one == '@') {
        return Err("Enter the database address as a host name or IP, without spaces.".into());
    }
    if port == 0 {
        return Err("Enter a port between 1 and 65535.".into());
    }
    Ok(())
}

/// Everything needed to log in to one database or host.
pub struct Reach {
    pub address: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: Option<String>,
    pub ssh_host: String,
}

struct Tunnel(tokio::task::JoinHandle<()>);

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Link {
    conn: mysql_async::Conn,
    plain: bool,
    _tunnel: Option<Tunnel>,
}

impl Link {
    async fn close(self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.conn.disconnect()).await;
    }
}

/// Carries a local port to the database through an SSH server, for a database only that machine can reach.
async fn open_tunnel(app: &AppHandle, reach: &Reach) -> Result<(Tunnel, u16), String> {
    let conn = crate::remote::connect(app, &reach.ssh_host).await?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| format!("Could not open a local port for the SSH tunnel: {error}"))?;
    let port = listener.local_addr().map_err(|error| error.to_string())?.port();
    let (address, far_port) = (reach.address.clone(), reach.port);
    let task = tauri::async_runtime::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (conn, address) = (conn.clone(), address.clone());
            tokio::spawn(async move {
                if let Ok(mut far) = conn.forward(&address, far_port).await {
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut far).await;
                }
            });
        }
    });
    match task {
        tauri::async_runtime::JoinHandle::Tokio(handle) => Ok((Tunnel(handle), port)),
    }
}

/// How much the TLS connection checks the database's certificate.
#[derive(Clone, Copy)]
enum Trust {
    /// A certificate from a public authority, for this address.
    Strict,
    /// Any certificate: only after its fingerprint matched the pinned one, or inside an SSH tunnel whose host
    /// key was already checked.
    Checked,
    Plain,
}

fn options(address: &str, port: u16, reach: &Reach, trust: Trust) -> mysql_async::Opts {
    let builder = mysql_async::OptsBuilder::default()
        .ip_or_hostname(address.to_string())
        .tcp_port(port)
        .prefer_socket(false)
        .user(Some(reach.username.clone()))
        .pass(Some(reach.password.clone()))
        .db_name(reach.database.clone());
    match trust {
        Trust::Strict => builder.ssl_opts(mysql_async::SslOpts::default()),
        Trust::Checked => builder.ssl_opts(mysql_async::SslOpts::default().with_danger_accept_invalid_certs(true)),
        Trust::Plain => builder,
    }
    .into()
}

/// True when the traffic never leaves this computer or the local network.
fn stays_local(address: &str) -> bool {
    let address = address.trim();
    if address.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match address.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => false,
    }
}

async fn bounded<T>(work: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::time::timeout(QUERY_WAIT, work).await.map_err(|_| "The database took too long to answer. Try again.".to_string())?
}

/// The TLS handshake failed on the certificate (self-signed, expired, another name), not on the network.
fn certificate_refused(error: &mysql_async::Error) -> bool {
    let text = format!("{error:?}");
    text.contains("InvalidCertificate") || text.contains("InvalidDnsName")
}

async fn peek_certificate(address: &str, port: u16) -> Result<db_tls::Peek, String> {
    let address = address.to_string();
    tokio::task::spawn_blocking(move || db_tls::peek(&address, port, WAIT))
        .await
        .map_err(|error| error.to_string())?
}

fn changed_certificate(now: &str) -> String {
    format!(
        "The database's certificate has changed since the app first connected (it is now SHA-256 {now}). The password was not sent. \
         If the database's certificate was renewed or reinstalled, remove this database from the app and add it again."
    )
}

async fn connect(address: &str, port: u16, reach: &Reach, trust: Trust) -> Result<mysql_async::Conn, mysql_async::Error> {
    mysql_async::Conn::new(options(address, port, reach, trust)).await
}

/// Connects with encryption when the database offers it. A certificate no public authority signed is trusted
/// on first use: `pinned` is the fingerprint kept from before, and a connection whose certificate differs
/// never sends the password. Returns the connection, whether it is encrypted, and a fingerprint to keep.
async fn dial_with(
    address: &str,
    port: u16,
    reach: &Reach,
    tunneled: bool,
    pinned: Option<&str>,
) -> Result<(mysql_async::Conn, bool, Option<String>), String> {
    // Both rustls backends are in the build, so the driver can't pick one by itself.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let no_tls = |error: &mysql_async::Error| matches!(error, mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer));
    let plain = move || async move { connect(address, port, reach, Trust::Plain).await.map(|conn| (conn, false, None)).map_err(|error| plain_error(&error)) };
    let checked = move |keep: Option<String>| async move {
        connect(address, port, reach, Trust::Checked).await.map(|conn| (conn, true, keep)).map_err(|error| plain_error(&error))
    };
    if tunneled {
        return match connect(address, port, reach, Trust::Checked).await {
            Err(error) if no_tls(&error) => plain().await,
            other => other.map(|conn| (conn, true, None)).map_err(|error| plain_error(&error)),
        };
    }
    if let Some(pinned) = pinned {
        return match peek_certificate(address, port).await? {
            db_tls::Peek::Certificate(now) if now == pinned => checked(None).await,
            db_tls::Peek::Certificate(now) => Err(changed_certificate(&now)),
            db_tls::Peek::NoEncryption => Err("This database used to encrypt the connection and now offers no encryption. The password was not sent. \
                 If its settings were changed on purpose, remove this database from the app and add it again."
                .into()),
        };
    }
    match connect(address, port, reach, Trust::Strict).await {
        Ok(conn) => Ok((conn, true, None)),
        Err(error) if no_tls(&error) => plain().await,
        Err(error) if certificate_refused(&error) => match peek_certificate(address, port).await? {
            db_tls::Peek::Certificate(seen) => checked(Some(seen)).await,
            db_tls::Peek::NoEncryption => Err(plain_error(&error)),
        },
        Err(error) => Err(plain_error(&error)),
    }
}

async fn dial(app: &AppHandle, address: &str, port: u16, reach: &Reach, tunneled: bool) -> Result<(mysql_async::Conn, bool), String> {
    let key = pin_key(address, port);
    let pinned = if tunneled { None } else { pins(app)?.get(&key).cloned() };
    let (conn, encrypted, keep) = dial_with(address, port, reach, tunneled, pinned.as_deref()).await?;
    if let Some(seen) = keep {
        set_pin(app, &key, Some(&seen))?;
    }
    Ok((conn, encrypted))
}

async fn open(app: &AppHandle, reach: &Reach) -> Result<Link, String> {
    let dial = async {
        let (tunnel, address, port) = if reach.ssh_host.is_empty() {
            (None, reach.address.clone(), reach.port)
        } else {
            let (tunnel, port) = open_tunnel(app, reach).await?;
            (Some(tunnel), "127.0.0.1".to_string(), port)
        };
        dial(app, &address, port, reach, tunnel.is_some()).await.map(|(conn, encrypted)| Link {
            conn,
            plain: !encrypted && tunnel.is_none() && !stays_local(&address),
            _tunnel: tunnel,
        })
    };
    tokio::time::timeout(WAIT, dial).await.map_err(|_| "The database did not answer in time. Check the address, port and firewall.".to_string())?
}

fn plain_error(error: &mysql_async::Error) -> String {
    match error {
        mysql_async::Error::Server(server) => match server.code {
            1045 => "The database refused this username or password.".into(),
            1044 | 1142 | 1227 | 1410 => "This database account doesn't have permission. It needs to be able to create databases and users.".into(),
            1049 => "This database doesn't exist on the host.".into(),
            1130 => "The database doesn't accept connections from this computer. Allow this address on the database host.".into(),
            1007 => "A database with this name already exists on the host.".into(),
            _ => format!("The database returned an error: {}", server.message),
        },
        mysql_async::Error::Io(_) => "Could not connect to the database. Check the address, port and firewall.".into(),
        // Windows MariaDB falls back to this for root when the password is wrong.
        mysql_async::Error::Driver(mysql_async::DriverError::UnknownAuthPlugin { name }) => {
            format!("The database refused this login. Check the username and password. If they are right, this account uses a login method the app doesn't support ({name}).")
        }
        other => format!("Could not use the database: {other}"),
    }
}

fn host_reach(app: &AppHandle, host: &DbHost) -> Result<Reach, String> {
    Ok(Reach {
        address: host.address.clone(),
        port: host.port,
        username: host.username.clone(),
        password: secret_of(app, &host.id)?,
        database: None,
        ssh_host: host.ssh_host.clone(),
    })
}

async fn version_of(app: &AppHandle, reach: &Reach) -> Result<(String, bool), String> {
    let mut link = open(app, reach).await?;
    let version: Option<String> = bounded(async { link.conn.query_first("SELECT VERSION()").await.map_err(|error| plain_error(&error)) }).await?;
    let plain = link.plain;
    link.close().await;
    Ok((version.unwrap_or_default(), plain))
}

pub fn list_hosts(app: &AppHandle) -> Result<Vec<DbHostView>, String> {
    let (book, keys) = (load(app)?, secrets(app)?);
    Ok(book
        .hosts
        .iter()
        .map(|host| DbHostView {
            id: host.id.clone(),
            name: host.name.clone(),
            address: host.address.clone(),
            port: host.port,
            username: host.username.clone(),
            ssh_host: host.ssh_host.clone(),
            plain: host.plain,
            has_password: keys.get(&host.id).is_some_and(|one| !one.is_empty()),
            databases: book.databases.iter().filter(|one| one.host_id == host.id).count() as u32,
        })
        .collect())
}

/// Checks the login first, so a saved host is one that worked. Returns the database's version.
pub async fn save_host(app: &AppHandle, input: DbHostInput) -> Result<String, String> {
    check_endpoint(&input.address, input.port)?;
    if input.username.trim().is_empty() {
        return Err("Enter the database username.".into());
    }
    let id = input.id.clone().filter(|one| !one.is_empty()).unwrap_or(format!("db-{}", random_text(12)?));
    let password = match input.password.clone().filter(|one| !one.is_empty()) {
        Some(password) => password,
        None => secret_of(app, &id).map_err(|_| "Enter the database password.".to_string())?,
    };
    let name = input.name.trim();
    let host = DbHost {
        id: id.clone(),
        name: if name.is_empty() { input.address.trim().to_string() } else { name.to_string() },
        address: input.address.trim().to_string(),
        port: input.port,
        username: input.username.trim().to_string(),
        ssh_host: input.ssh_host.clone(),
        plain: false,
    };
    let (version, plain) = version_of(
        app,
        &Reach {
            address: host.address.clone(),
            port: host.port,
            username: host.username.clone(),
            password: password.clone(),
            database: None,
            ssh_host: host.ssh_host.clone(),
        },
    )
    .await?;
    let host = DbHost { plain, ..host };
    {
        let _guard = WRITING.lock().expect("database store poisoned");
        let mut book = load(app)?;
        for one in book.databases.iter_mut().filter(|one| one.host_id == id) {
            one.plain = plain;
            one.address = host.address.clone();
            one.port = host.port;
            one.ssh_host = host.ssh_host.clone();
        }
        match book.hosts.iter_mut().find(|one| one.id == id) {
            Some(slot) => *slot = host,
            None => book.hosts.push(host),
        }
        keep(app, &book)?;
    }
    set_secret(app, &id, Some(&password))?;
    Ok(version)
}

pub async fn check_host(app: &AppHandle, id: &str) -> Result<String, String> {
    let host = load(app)?.hosts.into_iter().find(|one| one.id == id).ok_or("This database host no longer exists.")?;
    let (version, plain) = version_of(app, &host_reach(app, &host)?).await?;
    let _guard = WRITING.lock().expect("database store poisoned");
    let mut book = load(app)?;
    for slot in book.hosts.iter_mut().filter(|one| one.id == id) {
        slot.plain = plain;
    }
    for one in book.databases.iter_mut().filter(|one| one.host_id == id) {
        one.plain = plain;
    }
    keep(app, &book)?;
    Ok(version)
}

/// Forgets the host and its records. The databases themselves stay on the host.
pub fn delete_host(app: &AppHandle, id: &str) -> Result<(), String> {
    let _guard = WRITING.lock().expect("database store poisoned");
    let mut book = load(app)?;
    let mut keys = secrets(app)?;
    keys.remove(id);
    for one in book.databases.iter().filter(|one| one.host_id == id) {
        keys.remove(&one.id);
    }
    let gone = book.hosts.iter().find(|one| one.id == id).map(|one| pin_key(&one.address, one.port));
    book.hosts.retain(|one| one.id != id);
    book.databases.retain(|one| one.host_id != id);
    keep(app, &book)?;
    keep_secrets(app, &keys)?;
    drop(_guard);
    // Adding the host again trusts the certificate it shows then.
    match gone {
        Some(key) => set_pin(app, &key, None),
        None => Ok(()),
    }
}

pub fn of_server(app: &AppHandle, server_id: &str) -> Result<Vec<ServerDb>, String> {
    Ok(load(app)?.databases.into_iter().filter(|one| one.server_id == server_id).collect())
}

pub fn record(app: &AppHandle, server_id: &str, db: &str) -> Result<ServerDb, String> {
    of_server(app, server_id)?.into_iter().find(|one| one.id == db).ok_or("This database no longer exists.".into())
}

fn add_record(app: &AppHandle, record: &ServerDb, password: &str) -> Result<(), String> {
    {
        let _guard = WRITING.lock().expect("database store poisoned");
        let mut book = load(app)?;
        book.databases.push(record.clone());
        keep(app, &book)?;
    }
    set_secret(app, &record.id, Some(password))
}

async fn run_all(conn: &mut mysql_async::Conn, statements: &[String]) -> Result<(), String> {
    for one in statements {
        conn.query_drop(one).await.map_err(|error| plain_error(&error))?;
    }
    Ok(())
}

async fn make_database(conn: &mut mysql_async::Conn, database: &str, username: &str, password: &str) -> Result<(), String> {
    // Stops here if the name is taken, so the clean-up below can only drop what this call made.
    run_all(conn, &[format!("CREATE DATABASE {} CHARACTER SET utf8mb4", ident(database))]).await?;
    let account = format!("{}@'%'", text_literal(username));
    let made = run_all(
        conn,
        &[
            format!("CREATE USER {account} IDENTIFIED BY {}", text_literal(password)),
            format!("GRANT ALL PRIVILEGES ON {}.* TO {account}", grant_ident(database)),
        ],
    )
    .await;
    if made.is_err() {
        let _ = run_all(conn, &[format!("DROP DATABASE IF EXISTS {}", ident(database)), format!("DROP USER IF EXISTS {account}")]).await;
    }
    made
}

async fn change_password(conn: &mut mysql_async::Conn, username: &str, remote: &str, password: &str) -> Result<(), String> {
    run_all(conn, &[format!("ALTER USER {}@{} IDENTIFIED BY {}", text_literal(username), text_literal(remote), text_literal(password))]).await
}

async fn drop_database(conn: &mut mysql_async::Conn, database: &str, username: &str, remote: &str) -> Result<(), String> {
    run_all(
        conn,
        &[
            format!("DROP DATABASE IF EXISTS {}", ident(database)),
            format!("DROP USER IF EXISTS {}@{}", text_literal(username), text_literal(remote)),
        ],
    )
    .await
}

pub async fn create(app: &AppHandle, server_id: &str, host_id: &str, given: &str) -> Result<ServerDb, String> {
    let database = database_name(server_id, given)?;
    let host = load(app)?.hosts.into_iter().find(|one| one.id == host_id).ok_or("This database host no longer exists.")?;
    let username = format!("u{}_{}", server_tag(server_id), random_text(8)?.to_lowercase());
    let password = random_text(24)?;
    let mut link = open(app, &host_reach(app, &host)?).await?;
    let made = make_database(&mut link.conn, &database, &username, &password).await;
    link.close().await;
    made?;
    let made = ServerDb {
        id: format!("sdb-{}", random_text(12)?),
        server_id: server_id.to_string(),
        host_id: host.id,
        database,
        username,
        address: host.address,
        port: host.port,
        ssh_host: host.ssh_host,
        remote: "%".into(),
        plain: host.plain,
    };
    add_record(app, &made, &password)?;
    Ok(made)
}

/// Remembers a database made elsewhere, once its login works.
pub async fn link_existing(app: &AppHandle, server_id: &str, input: DbLinkInput) -> Result<ServerDb, String> {
    check_endpoint(&input.address, input.port)?;
    if input.database.trim().is_empty() || input.username.trim().is_empty() {
        return Err("Enter the database name and username.".into());
    }
    let made = ServerDb {
        id: format!("sdb-{}", random_text(12)?),
        server_id: server_id.to_string(),
        host_id: String::new(),
        database: input.database.trim().to_string(),
        username: input.username.trim().to_string(),
        address: input.address.trim().to_string(),
        port: input.port,
        ssh_host: input.ssh_host,
        remote: String::new(),
        plain: false,
    };
    let reach = record_reach(&made, input.password.clone());
    let link = open(app, &reach).await?;
    let made = ServerDb { plain: link.plain, ..made };
    link.close().await;
    add_record(app, &made, &input.password)?;
    Ok(made)
}

fn record_reach(one: &ServerDb, password: String) -> Reach {
    Reach {
        address: one.address.clone(),
        port: one.port,
        username: one.username.clone(),
        password,
        database: Some(one.database.clone()),
        ssh_host: one.ssh_host.clone(),
    }
}

pub fn reach_of(app: &AppHandle, server_id: &str, db: &str) -> Result<Reach, String> {
    let one = record(app, server_id, db)?;
    let password = secret_of(app, &one.id)?;
    Ok(record_reach(&one, password))
}

pub fn password_of(app: &AppHandle, server_id: &str, db: &str) -> Result<String, String> {
    secret_of(app, &record(app, server_id, db)?.id)
}

fn host_of_record(app: &AppHandle, one: &ServerDb) -> Result<DbHost, String> {
    load(app)?
        .hosts
        .into_iter()
        .find(|host| host.id == one.host_id)
        .ok_or("This database was linked by its login, so the app can't change it. Change it where it was made.".into())
}

pub async fn rotate_password(app: &AppHandle, server_id: &str, db: &str) -> Result<(), String> {
    let one = record(app, server_id, db)?;
    let host = host_of_record(app, &one)?;
    let password = random_text(24)?;
    let mut link = open(app, &host_reach(app, &host)?).await?;
    let changed = change_password(&mut link.conn, &one.username, &one.remote, &password).await;
    link.close().await;
    changed?;
    set_secret(app, &one.id, Some(&password))
}

/// Drops a database the app made; a linked one is only forgotten.
pub async fn delete(app: &AppHandle, server_id: &str, db: &str) -> Result<(), String> {
    let one = record(app, server_id, db)?;
    if !one.host_id.is_empty() {
        let host = host_of_record(app, &one)?;
        let mut link = open(app, &host_reach(app, &host)?).await?;
        let gone = drop_database(&mut link.conn, &one.database, &one.username, &one.remote).await;
        link.close().await;
        gone?;
    }
    {
        let _guard = WRITING.lock().expect("database store poisoned");
        let mut book = load(app)?;
        book.databases.retain(|other| other.id != one.id);
        keep(app, &book)?;
    }
    if one.host_id.is_empty() {
        set_pin(app, &pin_key(&one.address, one.port), None)?;
    }
    set_secret(app, &one.id, None)
}

/// Databases whose server is gone from the app and its trash, so no server tab can reach them any more.
pub fn orphans(app: &AppHandle, known: &[String]) -> Result<Vec<ServerDb>, String> {
    Ok(load(app)?.databases.into_iter().filter(|one| !known.contains(&one.server_id)).collect())
}

pub async fn delete_orphan(app: &AppHandle, known: &[String], db: &str) -> Result<(), String> {
    let one = orphans(app, known)?.into_iter().find(|one| one.id == db).ok_or("This database no longer exists.")?;
    delete(app, &one.server_id, db).await
}

pub const PLUGIN_FILES: [(&str, &str); 3] = [
    ("LuckPerms", "plugins/LuckPerms/config.yml"),
    ("LuckPerms", "plugins/luckperms/config.yml"),
    ("CoreProtect", "plugins/CoreProtect/config.yml"),
];

/// Writes one database's login into a plugin's own config, keeping the rest of the file as it is.
pub fn fill_plugin(plugin: &str, file: &str, text: &str, one: &ServerDb, password: &str) -> Result<String, String> {
    let port = one.port.to_string();
    let address = format!("{}:{port}", one.address);
    let values: Vec<(&str, &str, &str)> = match plugin {
        "LuckPerms" => vec![
            ("storage-method", "text", "MySQL"),
            ("data.address", "text", &address),
            ("data.database", "text", &one.database),
            ("data.username", "text", &one.username),
            ("data.password", "text", password),
        ],
        "CoreProtect" => vec![
            ("use-mysql", "bool", "true"),
            ("mysql-host", "text", &one.address),
            ("mysql-port", "number", &port),
            ("mysql-database", "text", &one.database),
            ("mysql-username", "text", &one.username),
            ("mysql-password", "text", password),
        ],
        _ => return Err("This plugin isn't supported.".into()),
    };
    let mut out = text.to_string();
    for (key, kind, value) in values {
        out = crate::extensions::set_at(file, &out, key, kind, value)?;
    }
    Ok(out)
}

pub fn export(app: &AppHandle) -> Result<serde_json::Value, String> {
    serde_json::to_value(load(app)?).map_err(|error| error.to_string())
}

/// Adds hosts and records that aren't here yet. Passwords never travel, so each asks for its own again.
pub fn import(app: &AppHandle, incoming: serde_json::Value) -> Result<usize, String> {
    let incoming: Book = serde_json::from_value(incoming).unwrap_or_default();
    let _guard = WRITING.lock().expect("database store poisoned");
    let mut book = load(app)?;
    let mut added = 0;
    for host in incoming.hosts {
        if !book.hosts.iter().any(|one| one.id == host.id) {
            book.hosts.push(host);
            added += 1;
        }
    }
    for one in incoming.databases {
        if !book.databases.iter().any(|saved| saved.id == one.id) {
            book.databases.push(one);
        }
    }
    keep(app, &book)?;
    Ok(added)
}

fn cell_text(text: String) -> String {
    if text.chars().count() <= CELL_LIMIT {
        return text;
    }
    let mut cut: String = text.chars().take(CELL_LIMIT).collect();
    cut.push_str("...");
    cut
}

fn blob_text(bytes: usize) -> String {
    format!("BLOB ({bytes} B)")
}

fn mysql_cell(value: &mysql_async::Value) -> Option<String> {
    match value {
        mysql_async::Value::NULL => None,
        mysql_async::Value::Bytes(bytes) => Some(match std::str::from_utf8(bytes) {
            Ok(text) => cell_text(text.to_string()),
            Err(_) => blob_text(bytes.len()),
        }),
        other => Some(cell_text(other.as_sql(true))),
    }
}

async fn mysql_table_names(conn: &mut mysql_async::Conn) -> Result<Vec<String>, String> {
    conn.query::<String, _>("SHOW TABLES").await.map_err(|error| plain_error(&error))
}

pub async fn tables(app: &AppHandle, reach: &Reach) -> Result<Vec<String>, String> {
    let mut link = open(app, reach).await?;
    let names = bounded(mysql_table_names(&mut link.conn)).await;
    link.close().await;
    names
}

pub async fn rows(app: &AppHandle, reach: &Reach, table: &str, offset: u32) -> Result<TablePage, String> {
    let mut link = open(app, reach).await?;
    let page = bounded(mysql_page(&mut link.conn, table, offset)).await;
    link.close().await;
    page
}

async fn mysql_page(conn: &mut mysql_async::Conn, table: &str, offset: u32) -> Result<TablePage, String> {
    if !mysql_table_names(conn).await?.iter().any(|one| one == table) {
        return Err("This table no longer exists.".into());
    }
    let sql = format!("SELECT * FROM {} LIMIT {} OFFSET {offset}", ident(table), PAGE + 1);
    let mut result = conn.query_iter(sql).await.map_err(|error| plain_error(&error))?;
    let columns: Vec<String> = result.columns_ref().iter().map(|one| one.name_str().into_owned()).collect();
    let found: Vec<mysql_async::Row> = result.collect().await.map_err(|error| plain_error(&error))?;
    let cells = found.iter().map(|row| (0..row.len()).map(|at| row.as_ref(at).and_then(mysql_cell)).collect()).collect();
    Ok(paged(columns, cells))
}

fn paged(columns: Vec<String>, mut rows: Vec<Vec<Option<String>>>) -> TablePage {
    let more = rows.len() > PAGE as usize;
    rows.truncate(PAGE as usize);
    TablePage { columns, rows, more }
}

pub fn is_sqlite_name(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    // H2 files end in .mv.db or .h2.db but aren't SQLite.
    if low.ends_with(".mv.db") || low.ends_with(".h2.db") {
        return false;
    }
    [".db", ".db3", ".sqlite", ".sqlite3"].iter().any(|one| low.ends_with(one))
}

pub fn has_sqlite_header(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0_u8; 16];
    std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut head)).is_ok() && &head == b"SQLite format 3\0"
}

fn sqlite_error(error: rusqlite::Error) -> String {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::NotADatabase) => "This file isn't a SQLite database.".into(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            "The server is writing to this file right now. Try again in a moment.".into()
        }
        _ => format!("Could not read the SQLite file: {error}"),
    }
}

fn open_sqlite(path: &Path) -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(sqlite_error)?;
    conn.busy_timeout(Duration::from_millis(1500)).map_err(sqlite_error)?;
    conn.execute_batch("PRAGMA query_only = ON").map_err(sqlite_error)?;
    Ok(conn)
}

fn sqlite_names(conn: &rusqlite::Connection) -> Result<Vec<String>, String> {
    let mut query = conn
        .prepare("SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(sqlite_error)?;
    let names = query.query_map([], |row| row.get::<_, String>(0)).map_err(sqlite_error)?.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)?;
    Ok(names)
}

pub fn sqlite_tables(path: &Path) -> Result<Vec<String>, String> {
    sqlite_names(&open_sqlite(path)?)
}

pub fn sqlite_rows(path: &Path, table: &str, offset: u32) -> Result<TablePage, String> {
    let conn = open_sqlite(path)?;
    if !sqlite_names(&conn)?.iter().any(|one| one == table) {
        return Err("This table no longer exists.".into());
    }
    let sql = format!("SELECT * FROM \"{}\" LIMIT {} OFFSET {offset}", table.replace('"', "\"\""), PAGE + 1);
    let mut query = conn.prepare(&sql).map_err(sqlite_error)?;
    let columns: Vec<String> = query.column_names().map(str::to_string).collect();
    let width = columns.len();
    let found = query
        .query_map([], |row| {
            (0..width)
                .map(|at| {
                    Ok(match row.get_ref(at)? {
                        rusqlite::types::ValueRef::Null => None,
                        rusqlite::types::ValueRef::Integer(one) => Some(one.to_string()),
                        rusqlite::types::ValueRef::Real(one) => Some(one.to_string()),
                        rusqlite::types::ValueRef::Text(one) => Some(cell_text(String::from_utf8_lossy(one).into_owned())),
                        rusqlite::types::ValueRef::Blob(one) => Some(blob_text(one.len())),
                    })
                })
                .collect::<Result<Vec<_>, rusqlite::Error>>()
        })
        .map_err(sqlite_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_error)?;
    Ok(paged(columns, found))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_db() -> ServerDb {
        ServerDb {
            id: "x".into(),
            server_id: "s".into(),
            host_id: "h".into(),
            database: "s1_lp".into(),
            username: "u1".into(),
            address: "10.0.0.5".into(),
            port: 3307,
            ssh_host: String::new(),
            remote: "%".into(),
            plain: false,
        }
    }

    #[test]
    fn fills_luckperms_and_keeps_the_rest_of_the_file() {
        let text = "# comment\nstorage-method: h2\ndata:\n  address: localhost\n  database: minecraft\n  username: root\n  password: ''\n  table-prefix: 'luckperms_'\nother: 1\n";
        let out = fill_plugin("LuckPerms", "plugins/LuckPerms/config.yml", text, &sample_db(), "secret1").unwrap();
        assert!(out.contains("10.0.0.5:3307"), "{out}");
        assert!(out.contains("s1_lp") && out.contains("u1") && out.contains("secret1"), "{out}");
        assert!(out.contains("MySQL"), "{out}");
        assert!(out.contains("# comment") && out.contains("table-prefix") && out.contains("other: 1"), "{out}");
    }

    #[test]
    fn fills_coreprotect() {
        let text = "use-mysql: false\nmysql-host: 127.0.0.1\nmysql-port: 3306\nmysql-database: database\nmysql-username: root\nmysql-password: ''\n";
        let out = fill_plugin("CoreProtect", "plugins/CoreProtect/config.yml", text, &sample_db(), "secret1").unwrap();
        assert!(out.contains("use-mysql: true") && out.contains("mysql-port: 3307"), "{out}");
    }

    #[test]
    fn a_plain_connection_only_matters_past_the_local_network() {
        assert!(stays_local("127.0.0.1") && stays_local("localhost") && stays_local("192.168.0.4"));
        assert!(!stays_local("8.8.8.8") && !stays_local("db.example.com"));
    }

    #[test]
    fn database_names_are_tagged_and_checked() {
        let name = database_name("server-1", "luckperms_1").unwrap();
        assert!(name.starts_with('s') && name.ends_with("_luckperms_1"));
        assert_eq!(name, database_name("server-1", " luckperms_1 ").unwrap());
        assert_ne!(name, database_name("server-2", "luckperms_1").unwrap());
        for bad in ["", "a b", "a`b", "a;drop", "x-y", &"a".repeat(33)] {
            assert!(database_name("server-1", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn identifiers_and_text_are_quoted() {
        assert_eq!(ident("a`b"), "`a``b`");
        let slash = char::from(92);
        assert_eq!(grant_ident("s1_a"), format!("`s1{slash}_a`"));
        assert_eq!(text_literal("o'k"), "'o''k'");
        assert_eq!(text_literal(&format!("a{slash}")), format!("'a{slash}{slash}'"));
    }

    #[test]
    fn random_text_has_the_length_and_letters_asked_for() {
        let made = random_text(64).unwrap();
        assert_eq!(made.len(), 64);
        assert!(made.chars().all(|one| one.is_ascii_alphanumeric()));
        assert_ne!(made, random_text(64).unwrap());
    }

    #[test]
    fn endpoints_are_checked() {
        assert!(check_endpoint("db.example.com", 3306).is_ok());
        assert!(check_endpoint("127.0.0.1", 3306).is_ok());
        assert!(check_endpoint("", 3306).is_err());
        assert!(check_endpoint("a b", 3306).is_err());
        assert!(check_endpoint("a/b", 3306).is_err());
        assert!(check_endpoint("a", 0).is_err());
    }

    #[test]
    fn sqlite_names_skip_h2() {
        assert!(is_sqlite_name("database.db"));
        assert!(is_sqlite_name("Data.SQLITE3"));
        assert!(!is_sqlite_name("luckperms-h2-v2.mv.db"));
        assert!(!is_sqlite_name("old.h2.db"));
        assert!(!is_sqlite_name("notes.txt"));
    }

    fn sample(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("monolab-sqlite-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let _ = std::fs::remove_file(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE \"we\"\"ird\" (id INTEGER, name TEXT, score REAL, data BLOB);
             CREATE TABLE empty_one (only_column TEXT);
             CREATE VIEW names AS SELECT name FROM \"we\"\"ird\";",
        )
        .unwrap();
        let mut add = conn.prepare("INSERT INTO \"we\"\"ird\" VALUES (?1, ?2, 1.5, x'00ff')").unwrap();
        for at in 0..(PAGE + 3) {
            add.execute(rusqlite::params![at, if at == 0 { None } else { Some(format!("n{at}")) }]).unwrap();
        }
        path
    }

    #[test]
    fn sqlite_lists_tables_and_pages_rows() {
        let path = sample("pages");
        assert!(has_sqlite_header(&path));
        assert_eq!(sqlite_tables(&path).unwrap(), vec!["empty_one", "names", "we\"ird"]);
        let first = sqlite_rows(&path, "we\"ird", 0).unwrap();
        assert_eq!(first.columns, vec!["id", "name", "score", "data"]);
        assert_eq!(first.rows.len(), PAGE as usize);
        assert!(first.more);
        assert_eq!(first.rows[0], vec![Some("0".into()), None, Some("1.5".into()), Some("BLOB (2 B)".into())]);
        let last = sqlite_rows(&path, "we\"ird", PAGE).unwrap();
        assert_eq!(last.rows.len(), 3);
        assert!(!last.more);
        let empty = sqlite_rows(&path, "empty_one", 0).unwrap();
        assert_eq!(empty.columns, vec!["only_column"]);
        assert!(empty.rows.is_empty());
    }

    #[test]
    fn sqlite_refuses_unknown_tables_and_other_files() {
        let path = sample("refuse");
        assert!(sqlite_rows(&path, "nope", 0).is_err());
        assert!(sqlite_rows(&path, "empty_one\"; DROP TABLE empty_one; --", 0).is_err());
        assert_eq!(sqlite_tables(&path).unwrap().len(), 3);
        let other = path.with_file_name("not.db");
        std::fs::write(&other, vec![7_u8; 4096]).unwrap();
        assert!(!has_sqlite_header(&other));
        assert!(sqlite_tables(&other).is_err());
    }

    /// Needs a real server: MONOLAB_TEST_DB=host:port:user:password cargo test --lib real_database -- --ignored
    #[tokio::test]
    #[ignore]
    async fn real_database_lifecycle() {
        async fn dial(address: &str, port: u16, reach: &Reach) -> Result<(mysql_async::Conn, bool), String> {
            super::dial_with(address, port, reach, false, None).await.map(|(conn, encrypted, _)| (conn, encrypted))
        }
        let given = std::env::var("MONOLAB_TEST_DB").expect("MONOLAB_TEST_DB");
        let parts: Vec<&str> = given.splitn(4, ':').collect();
        let (address, port) = (parts[0], parts[1].parse::<u16>().unwrap());
        let reach = |username: &str, password: &str, database: Option<&str>| Reach {
            address: address.into(),
            port,
            username: username.into(),
            password: password.into(),
            database: database.map(str::to_string),
            ssh_host: String::new(),
        };
        let (mut admin, _) = dial(address, port, &reach(parts[2], parts[3], None)).await.unwrap();
        let version: Option<String> = admin.query_first("SELECT VERSION()").await.unwrap();
        println!("server version: {version:?}");
        let tls: Option<(String, String)> = admin.query_first("SHOW SESSION STATUS LIKE 'Ssl_cipher'").await.unwrap();
        println!("tls: {tls:?}");

        let database = database_name("real-test", "luck_perms").unwrap();
        let username = format!("u{}_{}", server_tag("real-test"), random_text(8).unwrap().to_lowercase());
        let password = random_text(24).unwrap();
        let _ = drop_database(&mut admin, &database, &username, "%").await;
        make_database(&mut admin, &database, &username, &password).await.unwrap();

        // A taken name fails without touching the database that has it.
        let other = format!("u{}_{}", server_tag("real-test"), random_text(8).unwrap().to_lowercase());
        let taken = make_database(&mut admin, &database, &other, &password).await.unwrap_err();
        assert_eq!(taken, "A database with this name already exists on the host.");

        // The new account can reach its own database.
        let (mut user, _) = dial(address, port, &reach(&username, &password, Some(&database))).await.unwrap();
        user.query_drop("CREATE TABLE `od``d` (id INT, name VARCHAR(20), data BLOB, at DATETIME)").await.unwrap();
        user.query_drop("CREATE TABLE empty_one (only_column TEXT)").await.unwrap();
        for at in 0..(PAGE + 3) {
            user.query_drop(format!("INSERT INTO `od``d` VALUES ({at}, IF({at} = 0, NULL, 'n{at}'), x'00ff', '2026-11-04 12:00:00')")).await.unwrap();
        }
        let names = mysql_table_names(&mut user).await.unwrap();
        assert_eq!(names, vec!["empty_one", "od`d"]);
        let first = mysql_page(&mut user, "od`d", 0).await.unwrap();
        assert_eq!(first.columns, vec!["id", "name", "data", "at"]);
        assert_eq!(first.rows.len(), PAGE as usize);
        assert!(first.more);
        assert_eq!(first.rows[0], vec![Some("0".into()), None, Some("BLOB (2 B)".into()), Some("2026-11-04 12:00:00".into())]);
        let last = mysql_page(&mut user, "od`d", PAGE).await.unwrap();
        assert_eq!(last.rows.len(), 3);
        assert!(!last.more);
        let empty = mysql_page(&mut user, "empty_one", 0).await.unwrap();
        assert_eq!(empty.columns, vec!["only_column"]);
        assert!(empty.rows.is_empty());
        assert!(mysql_page(&mut user, "nope`; DROP TABLE empty_one; --", 0).await.is_err());
        assert_eq!(mysql_table_names(&mut user).await.unwrap().len(), 2);

        // It can't reach another database, even one its name would match if _ stayed a wildcard.
        let similar = database.replacen('_', "x", 1);
        admin.query_drop(format!("CREATE DATABASE IF NOT EXISTS {}", ident(&similar))).await.unwrap();
        assert!(user.query_drop(format!("CREATE TABLE {}.t (id INT)", ident(&similar))).await.is_err());
        admin.query_drop(format!("DROP DATABASE {}", ident(&similar))).await.unwrap();
        let _ = user.disconnect().await;

        let changed = random_text(24).unwrap();
        change_password(&mut admin, &username, "%", &changed).await.unwrap();
        let wrong = dial(address, port, &reach(&username, &password, Some(&database))).await.unwrap_err();
        assert_eq!(wrong, "The database refused this username or password.");
        dial(address, port, &reach(&username, &changed, Some(&database))).await.unwrap().0.disconnect().await.unwrap();

        drop_database(&mut admin, &database, &username, "%").await.unwrap();
        assert!(dial(address, port, &reach(&username, &changed, None)).await.is_err());
        drop_database(&mut admin, &database, &username, "%").await.unwrap();
        admin.disconnect().await.unwrap();
    }

    #[test]
    fn long_cells_are_cut() {
        let cut = cell_text("x".repeat(CELL_LIMIT + 5));
        assert_eq!(cut.chars().count(), CELL_LIMIT + 3);
        assert_eq!(cell_text("short".into()), "short");
    }
}
