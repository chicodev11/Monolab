use std::net::SocketAddr;
use std::path::PathBuf;

const USAGE: &str = "monolab-relay [--api 127.0.0.1:8080] [--public-url https://mnlab.cc] [--data /var/lib/monolab-relay]
              [--fake-sign-in] [--grant-pin] [--notify-updates]

  --api           Address and port of the Monolink account server (HTTP, meant to sit behind Caddy).
  --public-url    Where people reach the account server; links and the Discord callback use it.
  --data          Folder for accounts.db and the key that signs role grants.
  --fake-sign-in  Pretend sign-in at /auth/fake instead of Discord. Only with a localhost --public-url.
  --grant-pin     Print the public key that signs Monolink role grants (apps carry it), then exit.
  --notify-updates  Ask the server running at --api (on this machine) to read the release list now and
                  tell connected apps about a new add-on or fork, then exit.

Discord sign-in reads MONOLAB_DISCORD_CLIENT_SECRET (and MONOLAB_DISCORD_CLIENT_ID to use another app).
MONOLAB_ADMIN_USERS: account ids or 8-digit numbers (comma-separated) who may open /admin.";

/// The Monolab app on Discord. Public by design (it shows in every sign-in link); the secret is not.
const DISCORD_CLIENT_ID: &str = "1572893457385783387";

#[tokio::main]
async fn main() {
    let mut api: SocketAddr = "127.0.0.1:8080".parse().expect("valid default");
    let mut data = PathBuf::from("/var/lib/monolab-relay");
    let mut public_url: Option<String> = None;
    let mut fake_sign_in = false;
    let mut grant_pin = false;
    let mut notify_updates = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--api" => api = args.next().and_then(|one| one.parse().ok()).unwrap_or_else(|| fail("--api needs an address like 127.0.0.1:8080")),
            "--public-url" => public_url = Some(args.next().unwrap_or_else(|| fail("--public-url needs an address like https://mnlab.cc"))),
            "--data" => data = args.next().map(PathBuf::from).unwrap_or_else(|| fail("--data needs a folder")),
            "--fake-sign-in" => fake_sign_in = true,
            "--grant-pin" => grant_pin = true,
            "--notify-updates" => notify_updates = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => fail(&format!("Unknown option {other}\n\n{USAGE}")),
        }
    }
    if grant_pin {
        let key = relay::accounts::load_or_create_grant_key(&data).unwrap_or_else(|error| fail(&error.to_string()));
        println!("{} {}", relay::accounts::GRANT_KID, relay_proto::hex(&key.verifying_key().to_bytes()));
        return;
    }
    if notify_updates {
        notify(api).await;
        return;
    }
    let public_url = public_url.unwrap_or_else(|| fail(&format!("--public-url is needed\n\n{USAGE}")));
    let local = public_url.starts_with("http://127.0.0.1") || public_url.starts_with("http://localhost");
    if fake_sign_in && !local {
        fail("--fake-sign-in only works with a localhost --public-url. Anyone could sign in as anyone.");
    }
    let client_id = std::env::var("MONOLAB_DISCORD_CLIENT_ID").ok().filter(|one| !one.is_empty()).unwrap_or_else(|| DISCORD_CLIENT_ID.into());
    let discord = match std::env::var("MONOLAB_DISCORD_CLIENT_SECRET") {
        Ok(client_secret) if !client_secret.is_empty() => Some(relay::accounts::oauth::Discord { client_id, client_secret }),
        _ => None,
    };
    if discord.is_none() && !fake_sign_in {
        fail("Set MONOLAB_DISCORD_CLIENT_SECRET (in /etc/monolab-relay/env), or use --fake-sign-in on this machine.");
    }
    let admins = std::env::var("MONOLAB_ADMIN_USERS").unwrap_or_default().split(',').map(|one| one.trim().to_string()).filter(|one| !one.is_empty()).collect();
    let config = relay::accounts::Config {
        listen: api,
        data,
        public_url,
        discord,
        fake_sign_in,
        updates_url: Some(relay::accounts::app::UPDATES_URL.into()),
        admins,
    };
    let accounts = relay::accounts::start(config).await.unwrap_or_else(|error| fail(&format!("Couldn't start the account server: {error}")));
    println!("account server listening on {}", accounts.addr);
    let _ = tokio::signal::ctrl_c().await;
    println!("stopping");
    accounts.stop();
}

/// `--notify-updates`: a loopback-only call to the running server (it refuses anything that came
/// through Caddy, and Caddy hides the path).
async fn notify(api: SocketAddr) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut target = api;
    if target.ip().is_unspecified() {
        target.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
    }
    let url = format!("http://{target}/v2/admin/notify-updates");
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap_or_else(|error| fail(&error.to_string()));
    match client.post(&url).send().await {
        Ok(response) if response.status().as_u16() == 204 => println!("The account server is reading the release list now."),
        Ok(response) => fail(&format!("The account server at {target} answered {}.", response.status())),
        Err(error) => fail(&format!("Couldn't reach the account server at {target}: {error}")),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}
