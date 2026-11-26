//! monolab-node: a small daemon for a Linux machine that runs Minecraft servers. A Monolink team
//! reaches those servers through it over iroh, with role grants from the account server, while the
//! host PC is off. See `docs/monolab-node.md`.

mod account;
mod activity;
mod addons;
mod backups;
mod config;
mod console;
mod events;
mod files;
mod key;
mod link;
mod power;
mod serve;
mod teamlog;
mod transfer;

use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use iroh::endpoint::presets;
use iroh::{Endpoint, SecretKey};

const USAGE: &str = "monolab-node — lets a Monolink team reach this machine's Minecraft servers

Usage:
  monolab-node [run]  [--config FILE] [--data DIR]   serve (what the systemd service runs)
  monolab-node id     [--data DIR]                   print this node's device id (makes the key once)
  monolab-node check  [--config FILE]                read the config and say what is wrong
  monolab-node pair CODE [--name NAME] [--config FILE] [--data DIR]
                                                     add this node to your account (code from the app)

Defaults: --config /etc/monolab-node/config.toml, --data /var/lib/monolab-node";

struct Args {
    command: String,
    config: PathBuf,
    data: PathBuf,
    /// `pair`: the code, and the name the node gets in the app.
    code: String,
    name: String,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        command: "run".into(),
        config: config::DEFAULT_CONFIG.into(),
        data: config::DEFAULT_DATA.into(),
        code: String::new(),
        name: String::new(),
    };
    let mut list = std::env::args().skip(1);
    while let Some(one) = list.next() {
        match one.as_str() {
            "--config" => out.config = list.next().ok_or("--config needs a file")?.into(),
            "--data" => out.data = list.next().ok_or("--data needs a folder")?.into(),
            "-h" | "--help" => return Err(String::new()),
            "--name" => out.name = list.next().ok_or("--name needs a name")?,
            "run" | "id" | "check" => out.command = one,
            "pair" => {
                out.command = one;
                out.code = list.next().ok_or("pair needs the code from the app")?;
            }
            other => return Err(format!("Unknown argument: {other}")),
        }
    }
    Ok(out)
}

fn main() {
    let args = match args() {
        Ok(args) => args,
        Err(error) => {
            if !error.is_empty() {
                eprintln!("{error}\n");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let result = match args.command.as_str() {
        "id" => key::load_or_create(&args.data).map(|key| println!("{}", device_id(&key).text())),
        "check" => config::Config::load(&args.config).map(|config| {
            println!("OK: {} server(s), {} team(s), {} grant key(s).", config.servers.len(), config.teams.len(), config.grant_keys.len());
            for server in &config.servers {
                if !server.path.is_dir() {
                    println!("warning: server \"{}\": {} is not a folder.", server.id, server.path.display());
                }
            }
        }),
        "pair" => pair(&args),
        _ => run(args),
    };
    if let Err(error) = result {
        eprintln!("monolab-node: {error}");
        std::process::exit(1);
    }
}

fn pair(args: &Args) -> Result<(), String> {
    let config = config::Config::load(&args.config)?;
    let key = key::load_or_create(&args.data)?;
    let host = std::fs::read_to_string("/etc/hostname").map(|text| text.trim().to_string()).unwrap_or_default();
    let name = if args.name.trim().is_empty() {
        if host.is_empty() { "Monolab node".to_string() } else { host }
    } else {
        args.name.trim().to_string()
    };
    let device = account::pair(&config.accounts_url, &args.data, &key, &args.code, &name)?;
    println!("Paired as \"{name}\" ({device}). Restart the service so it tells the account server where it is:");
    println!("  sudo systemctl restart monolab-node");
    Ok(())
}

fn device_id(key: &[u8; 32]) -> relay_proto::DeviceId {
    link::device_of(&SecretKey::from_bytes(key).public())
}

fn run(args: Args) -> Result<(), String> {
    let config = config::Config::load(&args.config)?;
    let key = key::load_or_create(&args.data)?;
    let me = device_id(&key);
    let node_accounts = config.accounts_url.clone();
    let tmux = console::Tmux { program: "tmux".into(), socket: config.tmux_socket.clone() };
    let node = serve::Node::new(config, me, &args.data, tmux);
    // Console lines and servers coming and going, for teammates' windows (`__events`).
    events::watch(node.config.servers.clone(), node.tmux.clone(), node.events.clone());
    // What happens on the servers goes to the team's activity once the node is paired.
    let targets = std::sync::Arc::new(activity::Targets::default());
    if let Some(token) = account::session(&args.data) {
        let ids: Vec<String> = node.config.servers.iter().map(|one| one.id.clone()).collect();
        activity::watch_status(node.events.clone(), ids, node.activity.clone());
        if let Some(out) = node.activity_out.lock().unwrap_or_else(|poison| poison.into_inner()).take() {
            activity::report(out, targets.clone(), node_accounts.clone(), token);
        }
    }
    let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
    runtime.block_on(async move {
        let endpoint = Endpoint::builder(presets::N0)
            .relay_mode(relay_mode_for(&relay_choice(&node_accounts)))
            .secret_key(SecretKey::from_bytes(&key))
            .alpns(vec![link::ALPN_TEAM.to_vec()])
            // The address goes to teammates through the account server, not to a public directory.
            .clear_address_lookup()
            .bind()
            .await
            .map_err(|error| format!("Could not open the iroh endpoint: {error}"))?;
        eprintln!("monolab-node {}: device {} ({})", env!("CARGO_PKG_VERSION"), me.text(), me.short());
        let server = link::Server::new(node);
        {
            let (endpoint, data) = (endpoint.clone(), args.data.clone());
            tokio::spawn(async move {
                // Wait for a home relay so the address has one, then write it down for the operator.
                let _ = tokio::time::timeout(std::time::Duration::from_secs(20), endpoint.online()).await;
                write_addr(&data, &endpoint);
            });
        }
        if let Some(token) = account::session(&args.data) {
            // F7: team changes (a role lowered, someone removed) reach open connections within 30 seconds.
            // The team servers this node hosts are read again when a version moves and every ten minutes.
            let (server, accounts, token, targets) = (server.clone(), node_accounts.clone(), token.clone(), targets.clone());
            let me = me.text();
            tokio::spawn(async move {
                let mut known: Vec<(String, u64)> = Vec::new();
                let mut rounds = 0u32;
                loop {
                    let (asked, with) = (accounts.clone(), token.clone());
                    match tokio::task::spawn_blocking(move || account::team_versions(&asked, &with))
                        .await
                        .unwrap_or_else(|error| Err(error.to_string()))
                    {
                        Ok(teams) => {
                            teams.iter().for_each(|(team, version)| server.raise(team, *version));
                            if teams != known || rounds.is_multiple_of(20) {
                                let (asked, with, targets, me) = (accounts.clone(), token.clone(), targets.clone(), me.clone());
                                let read = tokio::task::spawn_blocking(move || activity::refresh(&targets, &asked, &with, &me)).await;
                                if !matches!(read, Ok(Ok(()))) {
                                    // Try again next round instead of waiting for the next change.
                                    known.clear();
                                    rounds = rounds.wrapping_add(1);
                                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                    continue;
                                }
                            }
                            known = teams;
                        }
                        Err(error) if error == "unpaired" => return,
                        Err(_) => {}
                    }
                    rounds = rounds.wrapping_add(1);
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
            });
        }
        if let Some(token) = account::session(&args.data) {
            // Every 5 minutes, so grants for this node carry a fresh address (the server keeps it 15).
            let (endpoint, accounts) = (endpoint.clone(), node_accounts.clone());
            tokio::spawn(async move {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(20), endpoint.online()).await;
                loop {
                    let addr = serde_json::to_value(endpoint.addr()).unwrap_or_default();
                    let (accounts, token) = (accounts.clone(), token.clone());
                    let sent = tokio::task::spawn_blocking(move || account::send_addr(&accounts, &token, &addr))
                        .await
                        .unwrap_or_else(|error| Err(error.to_string()));
                    match sent {
                        Err(error) if error == "unpaired" => {
                            eprintln!("monolab-node: the account server doesn't know this node any more; pair it again.");
                            return;
                        }
                        Err(error) => eprintln!("monolab-node: could not report the address: {error}"),
                        Ok(()) => {}
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                }
            });
        } else {
            eprintln!("monolab-node: not paired yet; run `monolab-node pair <code>` with a code from the app.");
        }
        let serving = async {
            while let Some(incoming) = endpoint.accept().await {
                let server = server.clone();
                tokio::spawn(async move {
                    let Ok(connection) = incoming.await else { return };
                    if connection.alpn() == link::ALPN_TEAM {
                        server.serve_team(connection).await;
                    } else {
                        connection.close(1u32.into(), b"unknown protocol");
                    }
                });
            }
        };
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).map_err(|error| error.to_string())?;
        tokio::select! {
            _ = serving => {}
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
        eprintln!("monolab-node: stopping");
        endpoint.close().await;
        Ok(())
    })
}

/// The iroh relay for connections with no direct path: Monolink's own at the account server's address
/// (`relay/deploy/iroh-relay.toml`), as the app uses. `MONOLAB_IROH_RELAY` names another, `n0` for n0's.
fn relay_choice(accounts: &str) -> String {
    std::env::var("MONOLAB_IROH_RELAY")
        .ok()
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty())
        .unwrap_or_else(|| accounts.trim_end_matches('/').to_string())
}

/// A local test account server (plain http) has no relay next to it, so it keeps n0's.
fn relay_mode_for(choice: &str) -> iroh::RelayMode {
    if choice == "n0" || !choice.starts_with("https://") {
        return iroh::RelayMode::Default;
    }
    match choice.parse::<iroh::RelayUrl>() {
        Ok(url) => iroh::RelayMode::Custom(iroh::RelayMap::from(url)),
        Err(_) => iroh::RelayMode::Default,
    }
}

/// `<data>/addr.json`: this node's iroh address, as the app's `PUT /v2/me/addr` would carry it.
fn write_addr(data: &std::path::Path, endpoint: &Endpoint) {
    let Ok(text) = serde_json::to_string_pretty(&endpoint.addr()) else { return };
    let path = data.join("addr.json");
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()));
    match written {
        Ok(()) => eprintln!("monolab-node: address written to {}", path.display()),
        Err(error) => eprintln!("monolab-node: could not write {}: {error}", path.display()),
    }
}
