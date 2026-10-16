
use crate::{hidden_command, kill_process_tree, off_runtime};
use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Stdio},
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Manager};

const DOWNLOAD: &str =
    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe";

const WAIT: Duration = Duration::from_secs(30);

pub struct Tunnel {
    child: Child,
    url: String,
}

impl Tunnel {
    pub fn url(&self) -> &str {
        &self.url
    }
}

pub type Running = Arc<Mutex<Option<Tunnel>>>;

fn exe_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("tunnel");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join("cloudflared.exe"))
}

fn ensure_exe(app: &AppHandle) -> Result<PathBuf, String> {
    let path = exe_path(app)?;
    if path.is_file() {
        return Ok(path);
    }
    let client = crate::net::builder()
        .user_agent("Monolab (local Windows desktop app)")
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| error.to_string())?;
    let body = client
        .get(DOWNLOAD)
        .send()
        .map_err(|error| format!("Could not download cloudflared: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Could not download cloudflared: {error}"))?
        .bytes()
        .map_err(|error| error.to_string())?;

    if body.len() < 1024 * 1024 || !body.starts_with(b"MZ") {
        return Err("The downloaded cloudflared file is invalid. Try again later.".into());
    }

    let staging = path.with_extension("part");
    std::fs::write(&staging, &body)
        .map_err(|error| format!("Could not save cloudflared: {error}"))?;
    std::fs::rename(&staging, &path)
        .map_err(|error| format!("Could not save cloudflared: {error}"))?;
    Ok(path)
}

pub fn start(app: &AppHandle, port: u16) -> Result<String, String> {
    let running: Running = app.state::<Running>().inner().clone();
    {
        let held = running.lock().expect("tunnel poisoned");
        if let Some(tunnel) = held.as_ref() {
            return Ok(tunnel.url.clone());
        }
    }
    let exe = off_runtime(|| ensure_exe(app))?;

    let mut child = hidden_command(&exe)
        .args([
            "tunnel",
            "--no-autoupdate",
            "--url",
        ])
        .arg(format!("http://127.0.0.1:{port}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not run cloudflared: {error}"))?;

    let (sender, receiver) = mpsc::channel();
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Some(found) = find_url(&line) {
                    let _ = sender.send(found);
                }
            }
        });
    }
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for _ in BufReader::new(out).lines().map_while(Result::ok) {}
        });
    }

    match receiver.recv_timeout(WAIT) {
        Ok(url) => {
            *running.lock().expect("tunnel poisoned") = Some(Tunnel {
                child,
                url: url.clone(),
            });
            println!("[tunnel] {url}");
            Ok(url)
        }
        Err(_) => {
            let pid = child.id();
            let _ = child.kill();
            kill_process_tree(pid);
            Err("No tunnel address was received. Check your connection and try again.".into())
        }
    }
}

pub fn stop(app: &AppHandle) {
    let running: Running = app.state::<Running>().inner().clone();
    let Some(mut tunnel) = running.lock().expect("tunnel poisoned").take() else {
        return;
    };
    let pid = tunnel.child.id();
    let _ = tunnel.child.kill();
    kill_process_tree(pid);
    let _ = tunnel.child.wait();
}

fn find_url(line: &str) -> Option<String> {
    let at = line.find("https://")?;
    let rest = &line[at..];
    let end = rest
        .find(|letter: char| letter.is_whitespace() || letter == '|' || letter == '"')
        .unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches('/');
    if url.ends_with(".trycloudflare.com") {
        Some(url.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::find_url;
    use std::{io::BufRead, io::BufReader, time::Duration};

    #[test]
    fn picks_the_address_out_of_the_box() {
        assert_eq!(
            find_url("2026-09-02T10:00:00Z INF |  https://cold-frost-1234.trycloudflare.com  |")
                .as_deref(),
            Some("https://cold-frost-1234.trycloudflare.com")
        );
        assert_eq!(
            find_url("| https://a-b-c.trycloudflare.com/ |").as_deref(),
            Some("https://a-b-c.trycloudflare.com")
        );
        assert_eq!(find_url("INF See https://developers.cloudflare.com/args"), None);
        assert_eq!(find_url("INF Starting tunnel"), None);
    }

    #[test]
    #[ignore]
    fn makes_a_real_tunnel() {
        use std::io::Write as _;
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                loop {
                    let mut header = String::new();
                    match reader.read_line(&mut header) {
                        Ok(0) => break,
                        Ok(_) if header.trim().is_empty() => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                let body = "tunnel check";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });

        let dir = std::env::temp_dir().join("se-tunnel-test");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("cloudflared.exe");
        if !exe.is_file() {
            println!("Downloading cloudflared… (about 55 MB)");
            let client = crate::net::builder()
                .user_agent("Monolab test")
                .timeout(Duration::from_secs(300))
                .build()
                .unwrap();
            let body = client.get(super::DOWNLOAD).send().unwrap().bytes().unwrap();
            assert!(body.starts_with(b"MZ"), "the download is not an executable");
            assert!(body.len() > 1024 * 1024, "the download is too small");
            std::fs::write(&exe, &body).unwrap();
        }
        println!("cloudflared {} bytes", std::fs::metadata(&exe).unwrap().len());

        #[allow(clippy::zombie_processes)]
        let mut child = std::process::Command::new(&exe)
            .args(["tunnel", "--no-autoupdate", "--url"])
            .arg(format!("http://127.0.0.1:{port}"))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("cloudflared should start");

        let (sender, receiver) = std::sync::mpsc::channel();
        let err = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Some(found) = super::find_url(&line) {
                    let _ = sender.send(found);
                }
            }
        });

        let url = receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("should print an address within 60 seconds");
        println!("tunnel address: {url}");
        assert!(url.ends_with(".trycloudflare.com"));

        let client = crate::net::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let mut got = String::new();
        for attempt in 0..8 {
            std::thread::sleep(Duration::from_secs(3));
            match client.get(&url).send() {
                Ok(answer) => {
                    let code = answer.status();
                    let body = answer.text().unwrap_or_default();
                    println!("#{}: {code} · {} chars", attempt + 1, body.chars().count());
                    if code.is_success() {
                        got = body;
                        break;
                    }
                    println!("   {}", body.chars().take(160).collect::<String>());
                }
                Err(error) => println!("#{}: {error}", attempt + 1),
            }
        }
        let pid = child.id();
        let _ = child.kill();
        crate::kill_process_tree(pid);
        assert_eq!(got, "tunnel check", "the tunnel should serve our page");
    }
}
