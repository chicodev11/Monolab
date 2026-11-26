
pub mod api;
pub mod auth;
pub mod tls;

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager};

pub struct Panel {
    port: u16,
    bound: IpAddr,
    https: bool,
    stop: Arc<AtomicBool>,
    guard: Arc<auth::Guard>,
}

impl Panel {
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn bound(&self) -> IpAddr {
        self.bound
    }
    pub fn https(&self) -> bool {
        self.https
    }
    pub fn viewers(&self) -> usize {
        self.guard.viewers()
    }
    pub fn forget_everyone(&self) {
        self.guard.forget_all();
    }
    pub fn online(&self) -> Vec<auth::Online> {
        self.guard.online()
    }
}

pub type Running = Arc<Mutex<Option<Panel>>>;

pub(crate) fn make_key() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("Could not get randomness from the operating system");
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One address the web panel can listen on: a network card's IPv4 address.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BindChoice {
    pub address: String,
    /// The network card's name, like "Wi-Fi" or "Ethernet".
    pub label: String,
}

/// The IPv4 addresses of this PC's connected network cards (no loopback).
pub fn bind_choices() -> Vec<BindChoice> {
    let mut out: Vec<BindChoice> = Vec::new();
    for card in netdev::get_interfaces() {
        if !card.is_up() || card.is_loopback() {
            continue;
        }
        let label = card.friendly_name.clone().unwrap_or_else(|| card.name.clone());
        for net in &card.ipv4 {
            let address = net.addr().to_string();
            if address.starts_with("169.254.") || out.iter().any(|one| one.address == address) {
                continue;
            }
            out.push(BindChoice { address, label: label.clone() });
        }
    }
    out
}

/// The saved bind setting as an address. "" is every network; anything else must still be on this PC,
/// so a network that went away never turns into a wider one.
pub fn bind_address(saved: &str) -> Result<IpAddr, String> {
    let saved = saved.trim();
    if saved.is_empty() || saved == "0.0.0.0" {
        return Ok(IpAddr::from([0, 0, 0, 0]));
    }
    let ip: IpAddr = saved.parse().map_err(|_| format!("{saved} isn't an address."))?;
    if ip.is_loopback() || bind_choices().iter().any(|one| one.address == saved) {
        return Ok(ip);
    }
    Err(format!(
        "The web panel is set to listen on {saved}, but this PC isn't on that network now. Pick another network in the web panel settings."
    ))
}

pub fn start(app: &AppHandle, port: u16) -> Result<(String, u16), String> {
    let running: Running = app.state::<Running>().inner().clone();
    {
        let held = running.lock().expect("web panel poisoned");
        if let Some(panel) = held.as_ref() {
            return Ok((String::new(), panel.port));
        }
    }
    // The network on purpose: the host picks "Same Wi-Fi" or "Port forwarding" (and confirms the
    // warning) when turning it on, and may narrow it to one network or this PC.
    let settings = crate::read_app_settings(app);
    let bound = bind_address(&settings.web_bind)?;
    let tls = if settings.web_https {
        tls::ensure(app)?;
        Some(tls::server_config(app)?)
    } else {
        None
    };
    let https = tls.is_some();
    // A restart (HTTPS or the network changed) can reach here before the old listener let go of the port.
    let mut tries = 0;
    let listener = loop {
        match TcpListener::bind((bound, port)) {
            Ok(listener) => break listener,
            Err(_) if tries < 10 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(error) => return Err(format!("Could not open the web panel on port {port}: {error}")),
        }
    };
    let stop = Arc::new(AtomicBool::new(false));
    let guard = Arc::new(auth::Guard::default());

    let handle = app.clone();
    let gate = stop.clone();
    let watch = guard.clone();
    let busy = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if gate.load(Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else { continue };
            if busy.load(Ordering::Relaxed) >= 24 {
                let mut stream = stream;
                let _ = reply(
                    &mut stream,
                    503,
                    "application/json; charset=utf-8",
                    "{\"error\":\"The app is busy. Try again once the other work finishes.\"}"
                        .as_bytes(),
                );
                continue;
            }
            busy.fetch_add(1, Ordering::Relaxed);
            let one = handle.clone();
            let keys = watch.clone();
            let count = busy.clone();
            let tls = tls.clone();
            std::thread::spawn(move || {
                let _ = connection(&one, &keys, stream, tls, port);
                count.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });

    println!("[web] {}://{}:{port}/", if https { "https" } else { "http" }, tls::knock_address(bound));
    *running.lock().expect("web panel poisoned") = Some(Panel { port, bound, https, stop, guard });
    Ok((String::new(), port))
}

pub fn stop(app: &AppHandle) {
    let running: Running = app.state::<Running>().inner().clone();
    let Some(panel) = running.lock().expect("web panel poisoned").take() else {
        return;
    };
    panel.stop.store(true, Ordering::Relaxed);
    let _ = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::new(tls::knock_address(panel.bound), panel.port),
        Duration::from_secs(2),
    );
}

/// One connection. The first byte tells TLS (0x16, a handshake) from plain HTTP, so one port serves both:
/// with HTTPS on, plain requests are sent to the https address; with it off, TLS is dropped.
fn connection(
    app: &AppHandle,
    guard: &auth::Guard,
    stream: TcpStream,
    tls: Option<Arc<rustls::ServerConfig>>,
    port: u16,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut first = [0_u8; 1];
    if stream.peek(&mut first)? == 0 {
        return Ok(());
    }
    let raw = stream.try_clone()?;
    match (first[0] == 0x16, tls) {
        (true, Some(config)) => {
            let session = rustls::ServerConnection::new(config).map_err(std::io::Error::other)?;
            let mut secure = rustls::StreamOwned::new(session, stream);
            let result = serve(app, guard, &mut secure, &raw);
            secure.conn.send_close_notify();
            let _ = secure.flush();
            result
        }
        (true, None) => Ok(()),
        (false, Some(_)) => redirect(stream, port),
        (false, None) => {
            let mut plain = stream;
            serve(app, guard, &mut plain, &raw)
        }
    }
}

fn redirect(stream: TcpStream, port: u16) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut host = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("host") {
                host = value.trim().to_string();
            }
        }
    }
    let target = first.split_whitespace().nth(1).unwrap_or("/");
    let location = tls::https_location(&host, target, port);
    let head = format!(
        "HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let stream = reader.get_mut();
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

fn serve<S: Read + Write>(app: &AppHandle, guard: &auth::Guard, stream: &mut S, raw: &TcpStream) -> std::io::Result<()> {
    let peer = raw.peer_addr().ok().map(|one| one.ip());
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first)?;

    let mut length = 0_usize;
    let mut session = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().unwrap_or(0),
            "x-se-session" => session = value.to_string(),
            _ => {}
        }
    }

    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    if method == "GET" {
        if let Some((kind, body)) = api::asset(path) {
            return reply(reader.get_mut(), 200, kind, &body);
        }
        if path == "/api-events" {
            let Some(who) = guard.whose(&session) else {
                return json(reader.get_mut(), 401, "{\"error\":\"Please sign in again.\"}");
            };
            let after = param(query, "after").parse().unwrap_or(0);
            let body = api::wait_for_events(app, after, Duration::from_secs(25));
            if who.is_admin() {
                return json(reader.get_mut(), 200, &body);
            }
            // Only the servers this account was given (every event names its server), and console lines only if
            // their role may watch the console — as Monolink guests get them.
            let value = api::events_for(&body, &who.servers);
            return json(reader.get_mut(), 200, &api::without_console(value, &api::perms_for(app, &who)).to_string());
        }
        return reply(
            reader.get_mut(),
            200,
            "text/html; charset=utf-8",
            &api::index(),
        );
    }

    if method != "POST" {
        return reply(reader.get_mut(), 405, "text/plain; charset=utf-8", b"no");
    }

    if path == "/upload" {
        if guard.whose(&session).is_none() {
            return json(reader.get_mut(), 401, "{\"error\":\"Please sign in again.\"}");
        }
        if length > 4 * 1024 * 1024 * 1024 {
            return json(reader.get_mut(), 413, "{\"error\":\"Files larger than 4 GB can't be uploaded.\"}");
        }
        let name = param(query, "name");
        let slot = match crate::stash_slot(app, &name) {
            Ok(slot) => slot,
            Err(error) => return json(reader.get_mut(), 500, &serde_json::json!({ "error": error }).to_string()),
        };
        let mut file = match std::fs::File::create(&slot) {
            Ok(file) => file,
            Err(error) => return json(reader.get_mut(), 500, &serde_json::json!({ "error": error.to_string() }).to_string()),
        };
        raw.set_read_timeout(Some(Duration::from_secs(120)))?;
        let mut taken = std::io::Read::take(std::io::Read::by_ref(&mut reader), length as u64);
        if let Err(error) = std::io::copy(&mut taken, &mut file) {
            let _ = std::fs::remove_file(&slot);
            return json(reader.get_mut(), 500, &serde_json::json!({ "error": error.to_string() }).to_string());
        }
        let token = crate::upload_token(&slot);
        return json(reader.get_mut(), 200, &serde_json::json!({ "value": token }).to_string());
    }

    // Nothing big is read for someone who isn't signed in: sign-in pages send a few hundred bytes.
    let signed_in = guard.whose(&session).is_some();
    if !signed_in && length > 16 * 1024 {
        return json(reader.get_mut(), 413, "{\"error\":\"Please sign in again.\"}");
    }
    if length > 192 * 1024 * 1024 {
        return json(reader.get_mut(), 413, "{\"error\":\"The file you sent is too large.\"}");
    }
    let mut body = vec![0_u8; length];
    if length > 0 {
        std::io::Read::read_exact(&mut reader, &mut body)?;
    }
    let body: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    let text = |name: &str| -> String {
        body.get(name)
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string()
    };

    if path == "/login" {
        let accounts = crate::web_accounts(app);
        if accounts.is_empty() {
            return json(
                reader.get_mut(),
                403,
                "{\"error\":\"Create an account in the app first.\"}",
            );
        }
        return match guard.login_from(&accounts, &text("name"), &text("password"), peer) {
            Ok(token) => json(reader.get_mut(), 200, &serde_json::json!({ "session": token }).to_string()),
            Err(error) => json(reader.get_mut(), 401, &serde_json::json!({ "error": error }).to_string()),
        };
    }

    if path == "/invite-info" {
        return match crate::web_invite_info(app, &text("token")) {
            Some(info) => json(reader.get_mut(), 200, &info.to_string()),
            None => json(reader.get_mut(), 404, "{\"error\":\"This invite link has expired or was already used. Ask for a new one.\"}"),
        };
    }

    if path == "/accept-invite" {
        let accepted = crate::accept_web_invite(app, &text("token"), &text("name"), &text("password"))
            .and_then(|name| guard.login_from(&crate::web_accounts(app), &name, &text("password"), peer));
        return match accepted {
            Ok(token) => json(reader.get_mut(), 200, &serde_json::json!({ "session": token }).to_string()),
            Err(error) => json(reader.get_mut(), 400, &serde_json::json!({ "error": error }).to_string()),
        };
    }

    if path == "/whoami" {
        return match guard.whose(&session) {
            Some(one) => json(
                reader.get_mut(),
                200,
                &serde_json::json!({ "name": one.name, "role": one.role }).to_string(),
            ),
            None => json(reader.get_mut(), 401, "{\"error\":\"Please sign in.\"}"),
        };
    }

    if let Some(command) = path.strip_prefix("/api/") {
        let who = guard.whose(&session);
        if who.is_some() {
            let servers = crate::web_servers_of(app, command, &body);
            guard.touch(&session, servers.first().map(String::as_str).filter(|one| !one.is_empty()));
        }
        let result = api::run(app, command, &body, who.as_ref());
        // Everyone signed in to the web panel is written down, admins too.
        if let Some(who) = who.as_ref() {
            crate::teamlog::record(app, &who.name, &format!("web · {}", who.role), "", command, &body, &result);
        }
        return match result {
            Ok(value) => json(reader.get_mut(), 200, &serde_json::json!({ "value": value }).to_string()),
            Err(error) => json(reader.get_mut(), 200, &serde_json::json!({ "error": error }).to_string()),
        };
    }

    if path == "/logout" {
        guard.forget(&session);
        return json(reader.get_mut(), 200, "{\"ok\":\"Signed out.\"}");
    }
    reply(reader.get_mut(), 404, "text/plain; charset=utf-8", b"not found")
}

fn param(query: &str, name: &str) -> String {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| unescape(value))
        .unwrap_or_default()
}

pub(crate) fn unescape(value: &str) -> String {
    let raw = value.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        match raw[at] {
            b'%' if at + 2 < raw.len() => {
                let pair = std::str::from_utf8(&raw[at + 1..at + 3]).unwrap_or("");
                match u8::from_str_radix(pair, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        at += 3;
                    }
                    Err(_) => {
                        out.push(raw[at]);
                        at += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn json<W: Write>(stream: &mut W, code: u16, body: &str) -> std::io::Result<()> {
    reply(stream, code, "application/json; charset=utf-8", body.as_bytes())
}

fn reply<W: Write>(stream: &mut W, code: u16, kind: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_a_fresh_key_each_time() {
        let one = make_key();
        let two = make_key();
        assert_eq!(one.len(), 64, "32 bytes as hex");
        assert!(one.chars().all(|letter| letter.is_ascii_hexdigit()));
        assert_ne!(one, two, "a key that never changes is pointless");
    }

    #[test]
    fn reads_what_the_address_carries() {
        assert_eq!(unescape("%ED%95%9C%EA%B8%80"), "\u{D55C}\u{AE00}");
        assert_eq!(unescape("say+hi%21"), "say hi!");
        assert_eq!(unescape("plain-id"), "plain-id");
        // Must not trip over broken values.
        assert_eq!(unescape("%zz"), "%zz");
        assert_eq!(unescape("%"), "%");
    }

    #[test]
    fn picks_one_value_out_of_the_query() {
        assert_eq!(param("key=abc&id=xyz", "id"), "xyz");
        assert_eq!(param("key=abc&id=xyz", "key"), "abc");
        assert_eq!(param("key=abc", "after"), "");
        assert_eq!(param("", "key"), "");
    }

    #[test]
    fn answers_only_with_the_right_key() {
        use std::io::{Read, Write};
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("should be able to listen");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) if line.trim().is_empty() => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or_default();
                let target = parts.next().unwrap_or_default();
                let (_, query) = target.split_once('?').unwrap_or((target, ""));
                let given = query
                    .split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .find(|(name, _)| *name == "key")
                    .map(|(_, value)| value)
                    .unwrap_or_default();
                let ok = method == "GET" && given == "secret";
                let code = if ok { 200 } else { 404 };
                let _ = reply(&mut stream, code, "text/plain", b"hi");
            }
        });

        let ask = |line: &str| -> String {
            let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
            socket.write_all(line.as_bytes()).unwrap();
            let mut got = String::new();
            let _ = socket.read_to_string(&mut got);
            got
        };

        assert!(ask("GET /?key=secret HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 200"));
        assert!(ask("GET /?key=wrong HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 404"));
        assert!(ask("POST /?key=secret HTTP/1.1\r\nHost: x\r\n\r\n").starts_with("HTTP/1.1 404"));
    }

    /// A non-admin may only reach a command that names its server, or one listed as safe without one.
    #[test]
    fn every_server_command_names_its_server() {
        let api = include_str!("web/api.rs");
        let lib = include_str!("lib.rs");
        let list = |name: &str| -> Vec<String> {
            let from = api.find(&format!("const {name}")).expect("list missing");
            let body = &api[from..from + api[from..].find("];").expect("list did not close")];
            body.split('"').skip(1).step_by(2).map(str::to_string).collect()
        };
        let free = list("ANY_SERVER");
        for command in list("READS").into_iter().chain(list("WRITES")) {
            if free.contains(&command) {
                continue;
            }
            let at = lib.find(&format!("\"{command}\" =>")).unwrap_or_else(|| panic!("{command} isn't wired up"));
            let arm = &lib[at..at + lib[at..].find("\n        \"").unwrap_or(400).min(600)];
            assert!(
                ["arg(body, \"id\")", "arg(body, \"input\")", "arg(body, \"task\")"].iter().any(|one| arm.contains(one)),
                "{command} names no server, so the per-server check can't run. Add it to ANY_SERVER only if it is safe for every account."
            );
        }
    }

    #[test]
    fn every_control_sits_behind_the_login() {
        let source = include_str!("web/api.rs");

        let gate = source
            .find("if !READS.contains(&command) && !WRITES.contains(&command)")
            .expect("the allow-list check is gone");
        // The role check: what this account's role (built-in or the host's own) may do.
        let login = source
            .find("if !permits(&perms, command)")
            .expect("the role check is gone");
        let per_server = source
            .find("if !who.may(id)")
            .expect("the per-server permission check is gone");
        let run = source.find("crate::web_invoke(app, command, body)").expect("nothing wires it up");

        assert!(gate < login && login < per_server && per_server < run,
            "The three checks must come before running the command.");

        let from = source.find("const READS").expect("the read list is missing");
        let reads = &source[from..from + source[from..].find("];").expect("the menu did not close")];
        for danger in ["start_", "stop_", "save_", "delete_", "remove_", "trash_", "upgrade_", "restore_", "set_"] {
            assert!(
                !reads.contains(&format!("\"{danger}")),
                "A command starting with `{danger}` is in the read list. It would run without signing in."
            );
        }
    }
}
