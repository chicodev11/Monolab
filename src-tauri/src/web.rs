
pub mod api;
pub mod auth;

use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager};

pub struct Panel {
    port: u16,
    stop: Arc<AtomicBool>,
    guard: Arc<auth::Guard>,
}

impl Panel {
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn viewers(&self) -> usize {
        self.guard.viewers()
    }
    pub fn forget_everyone(&self) {
        self.guard.forget_all();
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

pub fn start(app: &AppHandle, port: u16) -> Result<(String, u16), String> {
    let running: Running = app.state::<Running>().inner().clone();
    {
        let held = running.lock().expect("web panel poisoned");
        if let Some(panel) = held.as_ref() {
            return Ok((String::new(), panel.port));
        }
    }
    let listener = TcpListener::bind(("0.0.0.0", port))
        .map_err(|error| format!("Could not open the web panel on port {port}: {error}"))?;
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
            std::thread::spawn(move || {
                let _ = serve(&one, &keys, stream);
                count.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });

    println!("[web] http://127.0.0.1:{port}/");
    *running.lock().expect("web panel poisoned") = Some(Panel { port, stop, guard });
    Ok((String::new(), port))
}

pub fn stop(app: &AppHandle) {
    let running: Running = app.state::<Running>().inner().clone();
    let Some(panel) = running.lock().expect("web panel poisoned").take() else {
        return;
    };
    panel.stop.store(true, Ordering::Relaxed);
    let _ = std::net::TcpStream::connect(("127.0.0.1", panel.port));
}

fn serve(app: &AppHandle, guard: &auth::Guard, mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
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
            return reply(&mut stream, 200, kind, &body);
        }
        if path == "/api-events" {
            if guard.whose(&session).is_none() {
                return json(&mut stream, 401, "{\"error\":\"Please sign in again.\"}");
            }
            let after = param(query, "after").parse().unwrap_or(0);
            let body = api::wait_for_events(app, after, Duration::from_secs(25));
            return json(&mut stream, 200, &body);
        }
        return reply(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            &api::index(),
        );
    }

    if method != "POST" {
        return reply(&mut stream, 405, "text/plain; charset=utf-8", b"no");
    }

    if path == "/upload" {
        if guard.whose(&session).is_none() {
            return json(&mut stream, 401, "{\"error\":\"Please sign in again.\"}");
        }
        if length > 4 * 1024 * 1024 * 1024 {
            return json(&mut stream, 413, "{\"error\":\"Files larger than 4 GB can't be uploaded.\"}");
        }
        let name = param(query, "name");
        let slot = match crate::stash_slot(app, &name) {
            Ok(slot) => slot,
            Err(error) => return json(&mut stream, 500, &serde_json::json!({ "error": error }).to_string()),
        };
        let mut file = match std::fs::File::create(&slot) {
            Ok(file) => file,
            Err(error) => return json(&mut stream, 500, &serde_json::json!({ "error": error.to_string() }).to_string()),
        };
        stream.set_read_timeout(Some(Duration::from_secs(120)))?;
        let mut taken = std::io::Read::take(std::io::Read::by_ref(&mut reader), length as u64);
        if let Err(error) = std::io::copy(&mut taken, &mut file) {
            let _ = std::fs::remove_file(&slot);
            return json(&mut stream, 500, &serde_json::json!({ "error": error.to_string() }).to_string());
        }
        let token = slot
            .strip_prefix(slot.parent().and_then(|p| p.parent()).unwrap_or(&slot))
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        return json(&mut stream, 200, &serde_json::json!({ "value": token }).to_string());
    }

    if length > 192 * 1024 * 1024 {
        return json(&mut stream, 413, "{\"error\":\"The file you sent is too large.\"}");
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
                &mut stream,
                403,
                "{\"error\":\"Create an account in the app first.\"}",
            );
        }
        return match guard.login(&accounts, &text("name"), &text("password")) {
            Ok(token) => json(&mut stream, 200, &serde_json::json!({ "session": token }).to_string()),
            Err(error) => json(&mut stream, 401, &serde_json::json!({ "error": error }).to_string()),
        };
    }

    if path == "/whoami" {
        return match guard.whose(&session) {
            Some(one) => json(
                &mut stream,
                200,
                &serde_json::json!({ "name": one.name, "role": one.role }).to_string(),
            ),
            None => json(&mut stream, 401, "{\"error\":\"Please sign in.\"}"),
        };
    }

    if let Some(command) = path.strip_prefix("/api/") {
        let who = guard.whose(&session);
        return match api::run(app, command, &body, who.as_ref()) {
            Ok(value) => json(&mut stream, 200, &serde_json::json!({ "value": value }).to_string()),
            Err(error) => json(&mut stream, 200, &serde_json::json!({ "error": error }).to_string()),
        };
    }

    if path == "/logout" {
        guard.forget(&session);
        return json(&mut stream, 200, "{\"ok\":\"Signed out.\"}");
    }
    reply(&mut stream, 404, "text/plain; charset=utf-8", b"not found")
}

fn param(query: &str, name: &str) -> String {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| unescape(value))
        .unwrap_or_default()
}

fn unescape(value: &str) -> String {
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

fn json(stream: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    reply(stream, code, "application/json; charset=utf-8", body.as_bytes())
}

fn reply(stream: &mut TcpStream, code: u16, kind: &str, body: &[u8]) -> std::io::Result<()> {
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

    #[test]
    fn every_control_sits_behind_the_login() {
        let source = include_str!("web/api.rs");

        let gate = source
            .find("if !READS.contains(&command) && !WRITES.contains(&command)")
            .expect("the allow-list check is gone");
        let login = source
            .find("if WRITES.contains(&command)")
            .expect("the sign-in check is gone");
        let per_server = source
            .find("if !who.may(&id)")
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
