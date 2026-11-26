//! A server's console on this machine: the server runs in a tmux session (one per server), commands
//! are typed into it with `tmux send-keys`, and what it printed is read from its log file.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde::Serialize;

use crate::config::ServerConfig;

/// How much of the end of the log is read for the console tail.
const TAIL_BYTES: u64 = 256 * 1024;

/// How tmux is run: the program (`tmux`, or another for tests) and the socket, if any.
#[derive(Clone, Debug)]
pub struct Tmux {
    pub program: String,
    pub socket: String,
}

impl Tmux {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        if !self.socket.is_empty() {
            command.arg("-S").arg(&self.socket);
        }
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        command
    }

    /// Whether the server's session is there (the server is taken to be running while it is).
    pub fn running(&self, session: &str) -> bool {
        self.command().args(["has-session", "-t", &format!("={session}")]).status().is_ok_and(|status| status.success())
    }

    /// Types one line into the session and presses Enter. The text is sent as-is (`-l`), never read
    /// by a shell.
    pub fn send(&self, session: &str, line: &str) -> Result<(), String> {
        let line = console_line(line)?;
        if line.is_empty() {
            return Err("Type a command first.".into());
        }
        if !self.running(session) {
            return Err("The server is not running.".into());
        }
        let target = format!("={session}:");
        self.run(&["send-keys", "-t", &target, "-l", "--", &line], "Could not send the command")?;
        self.run(&["send-keys", "-t", &target, "Enter"], "Could not send the command")
    }

    fn run(&self, args: &[&str], failed: &str) -> Result<(), String> {
        let out = self.command().args(args).output().map_err(|error| format!("Could not run tmux: {error}"))?;
        if out.status.success() { Ok(()) } else { Err(format!("{failed}: {}", String::from_utf8_lossy(&out.stderr).trim())) }
    }

    /// Opens the server's session in its folder with the config's start command (tmux runs it with
    /// `sh -c`; it comes from the config, which only root writes, never from a teammate).
    pub fn start(&self, session: &str, folder: &Path, command: &str) -> Result<(), String> {
        if command.trim().is_empty() {
            return Err("This server has no start command. Add `start = \"…\"` to it in the node's config.toml.".into());
        }
        if self.running(session) {
            return Err("The server is already running.".into());
        }
        // The first session would make a tmux server inside the node's own locked-down service, where
        // Java can't run and the servers would end with the node. That tmux is a service of its own.
        if !self.command().arg("list-sessions").status().is_ok_and(|status| status.success()) {
            return Err("The node's tmux isn't running. On the machine: sudo systemctl start monolab-node-tmux".into());
        }
        let folder = folder.to_string_lossy();
        self.run(&["new-session", "-d", "-s", session, "-c", &folder, command], "Could not start the server")
    }

    /// Ends the session and whatever runs in it (End task).
    pub fn kill(&self, session: &str) -> Result<(), String> {
        if !self.running(session) {
            return Err("The server is not running.".into());
        }
        self.run(&["kill-session", "-t", &format!("={session}")], "Could not end the server")
    }
}

/// The app's rule (`launch::console_line`): one line, trimmed.
pub fn console_line(text: &str) -> Result<String, String> {
    let line = text.trim();
    if line.contains(['\n', '\r', '\0']) {
        return Err("Console commands cannot contain line breaks.".into());
    }
    Ok(line.to_string())
}

/// One console line in the app's shape (`ServerLogEvent`).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerLogEvent {
    pub server_id: String,
    pub line: String,
    pub stream: String,
    pub noise: bool,
    pub ansi: String,
}

fn ansi_codes() -> &'static regex::Regex {
    static CODES: OnceLock<regex::Regex> = OnceLock::new();
    CODES.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07").expect("ansi regex"))
}

/// The last `limit` lines of the server's log. A missing log is an empty console, not an error.
pub fn tail(server: &ServerConfig, limit: usize) -> Result<Vec<ServerLogEvent>, String> {
    let path = match crate::files::resolve_in_server(&server.path, &server.log) {
        Ok(path) => path,
        Err(crate::files::PathError::Missing) => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let text = read_end(&path, TAIL_BYTES).map_err(|error| format!("Could not read the console log: {error}"))?;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..].iter().map(|line| log_event(&server.id, line)).collect())
}

/// One log line as the window shows it, colour codes removed.
pub fn log_event(server: &str, line: &str) -> ServerLogEvent {
    ServerLogEvent {
        server_id: server.to_string(),
        line: ansi_codes().replace_all(line, "").into_owned(),
        stream: "stdout".into(),
        noise: false,
        ansi: String::new(),
    }
}

/// The end of a file, from the first whole line.
fn read_end(path: &Path, bytes: u64) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let start = size.saturating_sub(bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if start > 0 {
        text = text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_is_the_last_lines_without_colours() {
        let root = crate::files::tests::sandbox("tail");
        std::fs::create_dir_all(root.join("logs")).unwrap();
        let mut text = String::new();
        for n in 0..20 {
            text.push_str(&format!("\x1b[32mline {n}\x1b[0m\n"));
        }
        std::fs::write(root.join("logs/latest.log"), text).unwrap();
        let server = ServerConfig {
            id: "s".into(),
            name: "S".into(),
            path: root.clone(),
            tmux: String::new(),
            log: "logs/latest.log".into(),
            start: String::new(),
            stop: "stop".into(),
        };
        let lines = tail(&server, 3).unwrap();
        assert_eq!(lines.iter().map(|one| one.line.as_str()).collect::<Vec<_>>(), ["line 17", "line 18", "line 19"]);
        assert_eq!(lines[0].server_id, "s");
        std::fs::remove_file(root.join("logs/latest.log")).unwrap();
        assert!(tail(&server, 3).unwrap().is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn console_lines_are_one_line() {
        assert!(console_line("say hi\nop me").is_err());
        assert_eq!(console_line("  list ").unwrap(), "list");
    }
}
