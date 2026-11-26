//! Shell tab: a real terminal in the server's folder. A local server gets this PC's shell (PowerShell on
//! Windows, `$SHELL` elsewhere) through a pseudo terminal; an SSH server gets a login shell on that machine with a
//! PTY, started in its folder. Output goes to the window as `shell-output` events (base64, so escape codes and
//! split UTF-8 survive), `shell-exit` when the shell ends. App only: the web panel never gets these commands,
//! since a shell runs anything on the host.

use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

/// The most shells open at once (each is a process or an SSH channel).
const MOST: usize = 8;

enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

fn sessions() -> &'static Mutex<HashMap<String, UnboundedSender<Input>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, UnboundedSender<Input>>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Output<'a> {
    session: &'a str,
    data: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Exit<'a> {
    session: &'a str,
    message: String,
}

fn send_output(app: &AppHandle, session: &str, bytes: &[u8]) {
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let _ = app.emit("shell-output", Output { session, data });
}

fn finish(app: &AppHandle, session: &str, message: String) {
    sessions().lock().expect("shell poisoned").remove(session);
    let _ = app.emit("shell-exit", Exit { session, message });
}

fn size(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(10, 1000), rows.clamp(4, 500))
}

/// Opens a shell for a server. `remote` is the SSH host id (empty for this PC), `folder` where it starts.
pub async fn open(app: AppHandle, remote: String, folder: String, cols: u16, rows: u16) -> Result<String, String> {
    if sessions().lock().expect("shell poisoned").len() >= MOST {
        return Err(format!("{MOST} shells are open already. Close one first."));
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis()).unwrap_or(0);
    let session = format!("sh-{stamp}-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let (tx, rx) = unbounded_channel();
    let (cols, rows) = size(cols, rows);
    if remote.is_empty() {
        open_local(app, session.clone(), folder, cols, rows, rx)?;
    } else {
        let conn = crate::remote::connect(&app, &remote).await?;
        let channel = conn.open_shell(cols, rows).await?;
        let start = format!("cd -- {} 2>/dev/null; clear\n", crate::remote::quote(&folder));
        channel.data(start.as_bytes()).await.map_err(|error| format!("Could not start the shell: {error}"))?;
        tauri::async_runtime::spawn(run_remote(app, session.clone(), channel, rx));
    }
    sessions().lock().expect("shell poisoned").insert(session.clone(), tx);
    Ok(session)
}

fn local_shell() -> CommandBuilder {
    if cfg!(windows) {
        let mut command = CommandBuilder::new("powershell.exe");
        command.arg("-NoLogo");
        command
    } else {
        let shell = std::env::var("SHELL").ok().filter(|one| !one.is_empty()).unwrap_or_else(|| "/bin/sh".into());
        let mut command = CommandBuilder::new(shell);
        command.arg("-l");
        command
    }
}

fn open_local(
    app: AppHandle,
    session: String,
    folder: String,
    cols: u16,
    rows: u16,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Input>,
) -> Result<(), String> {
    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|error| format!("Could not open a terminal: {error}"))?;
    let mut command = local_shell();
    if std::path::Path::new(&folder).is_dir() {
        command.cwd(&folder);
    }
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).map_err(|error| format!("Could not start the shell: {error}"))?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|error| format!("Could not read the terminal: {error}"))?;
    let mut writer = pair.master.take_writer().map_err(|error| format!("Could not write to the terminal: {error}"))?;
    let master = pair.master;
    let mut killer = child.clone_killer();

    let out_app = app.clone();
    let out_session = session.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => send_output(&out_app, &out_session, &buffer[..read]),
            }
        }
        let message = match child.wait() {
            Ok(status) if status.success() => String::new(),
            Ok(status) => format!("The shell ended with code {}.", status.exit_code()),
            Err(error) => error.to_string(),
        };
        finish(&out_app, &out_session, message);
    });
    std::thread::spawn(move || {
        while let Some(input) = rx.blocking_recv() {
            match input {
                Input::Data(bytes) => {
                    if writer.write_all(&bytes).and_then(|()| writer.flush()).is_err() {
                        break;
                    }
                }
                Input::Resize(cols, rows) => {
                    let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                }
                Input::Close => break,
            }
        }
        let _ = killer.kill();
    });
    Ok(())
}

async fn run_remote(
    app: AppHandle,
    session: String,
    mut channel: russh::Channel<russh::client::Msg>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Input>,
) {
    let mut message = String::new();
    loop {
        tokio::select! {
            input = rx.recv() => match input {
                Some(Input::Data(bytes)) => {
                    if channel.data(&bytes[..]).await.is_err() {
                        break;
                    }
                }
                Some(Input::Resize(cols, rows)) => {
                    let _ = channel.window_change(u32::from(cols), u32::from(rows), 0, 0).await;
                }
                Some(Input::Close) | None => {
                    let _ = channel.close().await;
                    break;
                }
            },
            event = channel.wait() => match event {
                Some(russh::ChannelMsg::Data { data }) => send_output(&app, &session, &data),
                Some(russh::ChannelMsg::ExtendedData { data, .. }) => send_output(&app, &session, &data),
                Some(russh::ChannelMsg::ExitStatus { exit_status }) if exit_status != 0 => {
                    message = format!("The shell ended with code {exit_status}.");
                }
                Some(russh::ChannelMsg::Close) | None => break,
                _ => {}
            },
        }
    }
    finish(&app, &session, message);
}

fn send(session: &str, input: Input) -> Result<(), String> {
    let held = sessions().lock().expect("shell poisoned");
    let Some(tx) = held.get(session) else { return Err("This shell has ended.".into()) };
    tx.send(input).map_err(|_| "This shell has ended.".to_string())
}

pub fn input(session: &str, data: &str) -> Result<(), String> {
    send(session, Input::Data(data.as_bytes().to_vec()))
}

pub fn resize(session: &str, cols: u16, rows: u16) -> Result<(), String> {
    let (cols, rows) = size(cols, rows);
    send(session, Input::Resize(cols, rows))
}

pub fn close(session: &str) {
    let _ = send(session, Input::Close);
    sessions().lock().expect("shell poisoned").remove(session);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_sizes_sane() {
        assert_eq!(size(0, 0), (10, 4));
        assert_eq!(size(120, 40), (120, 40));
        assert_eq!(size(u16::MAX, u16::MAX), (1000, 500));
    }
}
