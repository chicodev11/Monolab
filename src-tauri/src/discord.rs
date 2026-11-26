//! Discord Rich Presence through Discord's local IPC pipe. Opt-in (Settings, off by default).
//! It shows only what the app is doing (how many servers run, their core and version), never
//! a server's address, port or folder. Nothing leaves this PC except to the local Discord app.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The Discord application whose name and art the status shows. Set at build time; without it the
/// switch says the build has no Discord status.
pub const CLIENT_ID: Option<&str> = option_env!("MONOLAB_DISCORD_CLIENT_ID");

/// Discord takes at most five updates per 20 seconds.
const GAP: Duration = Duration::from_secs(4);
/// How often to try again when Discord isn't open yet.
const RETRY: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscordPresence {
    /// The first line, e.g. "Hosting 2 servers".
    pub details: String,
    /// The second line, e.g. "Paper 1.21.4".
    pub state: String,
    /// Unix seconds the elapsed timer counts from.
    pub started_at: Option<u64>,
}

pub fn available() -> bool {
    CLIENT_ID.is_some_and(|id| !id.trim().is_empty())
}

fn sender() -> &'static Mutex<Sender<Option<DiscordPresence>>> {
    static SENDER: OnceLock<Mutex<Sender<Option<DiscordPresence>>>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (send, receive) = mpsc::channel();
        std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || worker(receive))
            .expect("could not start the Discord thread");
        Mutex::new(send)
    })
}

/// Shows `presence`, or clears it with `None`. Never blocks: the pipe is handled on its own thread.
pub fn set(presence: Option<DiscordPresence>) {
    if !available() {
        return;
    }
    let _ = sender().lock().expect("discord sender poisoned").send(presence);
}

fn worker(receive: Receiver<Option<DiscordPresence>>) {
    let mut pipe: Option<Pipe> = None;
    let mut wanted: Option<DiscordPresence> = None;
    let mut shown: Option<DiscordPresence> = None;
    loop {
        match receive.recv_timeout(RETRY) {
            Ok(next) => {
                wanted = next;
                while let Ok(next) = receive.try_recv() {
                    wanted = next;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let Some(presence) = wanted.clone() else {
            // Closing the pipe takes the status off the profile.
            pipe = None;
            shown = None;
            continue;
        };
        if pipe.is_some() && shown.as_ref() == Some(&presence) {
            continue;
        }
        if pipe.is_none() {
            pipe = Pipe::open().ok();
        }
        if let Some(open) = pipe.as_mut() {
            if open.activity(Some(&presence)).is_ok() {
                shown = Some(presence);
            } else {
                pipe = None;
                shown = None;
            }
        }
        std::thread::sleep(GAP);
    }
}

/// Cuts `text` to what Discord shows (2 to 128 characters).
fn fit(text: &str) -> String {
    let text: String = text.trim().chars().take(128).collect();
    if text.chars().count() < 2 {
        format!("{text}  ")
    } else {
        text
    }
}

/// The SET_ACTIVITY request for `presence` (`None` clears it).
pub fn activity_json(presence: Option<&DiscordPresence>, pid: u32, nonce: u64) -> serde_json::Value {
    let activity = presence.map(|one| {
        let mut activity = serde_json::json!({
            "details": fit(&one.details),
            "state": fit(&one.state),
            "assets": { "large_image": "monolab", "large_text": "Monolab" },
        });
        if let Some(start) = one.started_at {
            activity["timestamps"] = serde_json::json!({ "start": start });
        }
        activity
    });
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": activity },
        "nonce": nonce.to_string(),
    })
}

/// One frame: opcode and length (little endian), then the JSON.
pub fn frame(op: u32, body: &serde_json::Value) -> Vec<u8> {
    let text = body.to_string();
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&op.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

struct Pipe {
    stream: Box<dyn Stream>,
    nonce: u64,
}

impl Pipe {
    fn open() -> Result<Self, String> {
        let id = CLIENT_ID.ok_or("No Discord application in this build.")?;
        let mut last = String::from("Discord isn't running.");
        for index in 0..10 {
            match connect(index) {
                Ok(stream) => {
                    let mut pipe = Pipe { stream, nonce: 0 };
                    pipe.send(0, &serde_json::json!({ "v": 1, "client_id": id }))?;
                    pipe.receive()?;
                    return Ok(pipe);
                }
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    fn send(&mut self, op: u32, body: &serde_json::Value) -> Result<(), String> {
        self.stream.write_all(&frame(op, body)).map_err(|error| error.to_string())?;
        self.stream.flush().map_err(|error| error.to_string())
    }

    fn receive(&mut self) -> Result<serde_json::Value, String> {
        let mut head = [0u8; 8];
        self.stream.read_exact(&mut head).map_err(|error| error.to_string())?;
        let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let length = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if length > 64 * 1024 {
            return Err("Discord sent too much.".into());
        }
        let mut body = vec![0u8; length];
        self.stream.read_exact(&mut body).map_err(|error| error.to_string())?;
        // 2 is Discord closing the pipe (a wrong application id, for one).
        if op == 2 {
            return Err("Discord closed the connection.".into());
        }
        serde_json::from_slice(&body).map_err(|error| error.to_string())
    }

    fn activity(&mut self, presence: Option<&DiscordPresence>) -> Result<(), String> {
        self.nonce += 1;
        let body = activity_json(presence, std::process::id(), self.nonce);
        self.send(1, &body)?;
        let answer = self.receive()?;
        if answer.get("evt").and_then(serde_json::Value::as_str) == Some("ERROR") {
            return Err("Discord refused the status.".into());
        }
        Ok(())
    }
}

#[cfg(windows)]
fn connect(index: u32) -> Result<Box<dyn Stream>, String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\pipe\discord-ipc-{index}"))
        .map_err(|error| error.to_string())?;
    Ok(Box::new(file))
}

#[cfg(unix)]
fn connect(index: u32) -> Result<Box<dyn Stream>, String> {
    let mut last = String::from("Discord isn't running.");
    for variable in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        let Ok(folder) = std::env::var(variable) else { continue };
        match std::os::unix::net::UnixStream::connect(std::path::Path::new(&folder).join(format!("discord-ipc-{index}"))) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                return Ok(Box::new(stream));
            }
            Err(error) => last = error.to_string(),
        }
    }
    std::os::unix::net::UnixStream::connect(format!("/tmp/discord-ipc-{index}"))
        .map(|stream| {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            Box::new(stream) as Box<dyn Stream>
        })
        .map_err(|_| last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_carry_opcode_and_length() {
        let bytes = frame(1, &serde_json::json!({ "a": 1 }));
        assert_eq!(&bytes[0..4], &1u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &7u32.to_le_bytes());
        assert_eq!(&bytes[8..], br#"{"a":1}"#);
    }

    #[test]
    fn activity_has_only_what_it_was_given() {
        let presence = DiscordPresence { details: "Hosting 2 servers".into(), state: "Paper 1.21.4".into(), started_at: Some(100) };
        let body = activity_json(Some(&presence), 7, 3);
        assert_eq!(body["cmd"], "SET_ACTIVITY");
        assert_eq!(body["args"]["pid"], 7);
        assert_eq!(body["args"]["activity"]["details"], "Hosting 2 servers");
        assert_eq!(body["args"]["activity"]["timestamps"]["start"], 100);
        assert_eq!(body["nonce"], "3");
        let cleared = activity_json(None, 7, 4);
        assert!(cleared["args"]["activity"].is_null());
    }

    #[test]
    fn short_and_long_text_fit() {
        assert_eq!(fit("a").chars().count(), 3);
        assert_eq!(fit(&"x".repeat(300)).chars().count(), 128);
    }
}
