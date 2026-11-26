//! Talks to a running monolab-node the way the app's Monolink window will, for trying a node by hand:
//! signs a grant with a test key, connects over iroh, shows it, and sends requests.
//!
//!   cargo run --example try_node -- <node data dir> <seed hex (32 bytes)> <team> <role> <servers,…> '<cmd> <json body>'… ['sleep <seconds>']
//!
//! The node's config must trust `1:<public key of the seed>` (printed first). Test keys only.

use std::time::Duration;

use ed25519_dalek::SigningKey;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, SecretKey};
use relay_proto::DeviceId;
use relay_proto::grant::{Grant, LIFE_SECS, Role, sign};
use serde_json::{Value, json};

const ALPN: &[u8] = b"monolab/monolink/2";

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        eprintln!("usage: try_node <node data dir> <seed hex> <team> <role> <servers,…> ['<cmd> <json>'…]");
        std::process::exit(2);
    }
    let seed: [u8; 32] = relay_proto::unhex(&args[1]).and_then(|one| one.try_into().ok()).expect("seed: 64 hex letters");
    let signer = SigningKey::from_bytes(&seed);
    println!("grant key: 1:{}", relay_proto::hex(&signer.verifying_key().to_bytes()));
    let addr: EndpointAddr = serde_json::from_str(&std::fs::read_to_string(format!("{}/addr.json", args[0])).expect("addr.json")).expect("address");
    let mut mine = [0u8; 32];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom").unwrap(), &mut mine).unwrap();
    let key = SecretKey::from_bytes(&mine);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let grant = Grant {
        v: 1,
        kid: 1,
        user: "01TRY".into(),
        handle: "tester".into(),
        device: DeviceId::of(key.public().as_bytes()),
        team: args[2].clone(),
        // TRY_VERSION=<n>: a newer team version, to see older connections asked to renew and dropped.
        team_version: std::env::var("TRY_VERSION").ok().and_then(|one| one.parse().ok()).unwrap_or(1),
        role: Role::parse(&args[3]).expect("role"),
        host: DeviceId::of(addr.id.as_bytes()),
        servers: args[4].split(',').map(str::to_string).collect(),
        iat: now,
        // TRY_LIFE=<seconds>: a short grant, to see the node close the connection when it runs out.
        exp: now + std::env::var("TRY_LIFE").ok().and_then(|one| one.parse().ok()).unwrap_or(LIFE_SECS),
    };
    let endpoint = Endpoint::builder(presets::N0).secret_key(key).clear_address_lookup().bind().await.expect("endpoint");
    let connection = tokio::time::timeout(Duration::from_secs(15), endpoint.connect(addr, ALPN)).await.expect("connect timed out").expect("connect");
    let (mut send, mut recv) = connection.open_bi().await.expect("control stream");
    let hello = json!({ "type": "hello", "grant": sign(&grant, &signer) }).to_string().into_bytes();
    let mut frame = (hello.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&hello);
    send.write_all(&frame).await.expect("hello");
    let mut size = [0u8; 4];
    recv.read_exact(&mut size).await.expect("answer");
    let mut body = vec![0u8; u32::from_be_bytes(size) as usize];
    recv.read_exact(&mut body).await.expect("answer");
    println!("control: {}", String::from_utf8_lossy(&body));
    for one in &args[5..] {
        let (cmd, body) = one.split_once(' ').unwrap_or((one, "{}"));
        if cmd == "sleep" {
            tokio::time::sleep(Duration::from_secs(body.parse().expect("sleep <seconds>"))).await;
            println!("slept {body}s, close reason: {:?}", connection.close_reason());
            continue;
        }
        let request = json!({ "cmd": cmd, "body": serde_json::from_str::<Value>(body).expect("body json") }).to_string();
        let answer = async {
            let (mut send, mut recv) = connection.open_bi().await.map_err(|error| error.to_string())?;
            send.write_all(request.as_bytes()).await.map_err(|error| error.to_string())?;
            send.finish().map_err(|error| error.to_string())?;
            let bytes = recv.read_to_end(64 * 1024 * 1024).await.map_err(|error| error.to_string())?;
            Ok::<String, String>(String::from_utf8_lossy(&bytes).into_owned())
        }
        .await;
        let shown = match answer {
            Ok(text) if text.len() > 600 => format!("{}… ({} bytes)", &text[..600], text.len()),
            Ok(text) => text,
            Err(error) => format!("(failed: {error})"),
        };
        println!("{cmd} → {shown}");
    }
    connection.close(0u32.into(), b"done");
    endpoint.close().await;
}
