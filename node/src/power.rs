//! Starting and stopping servers (`start_instance`, `stop_instance`, `restart_instance`,
//! `force_stop_instance`). A server runs in its tmux session; stopping types the config's `stop` into
//! it and waits for the session to end, then ends it if the server didn't within a minute.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::ServerConfig;
use crate::console::Tmux;
use crate::events::Events;

/// How long a server gets to stop on its own before its session is ended.
pub const STOP_WAIT: Duration = Duration::from_secs(60);

#[derive(Clone, Default)]
pub struct Power {
    /// Servers being stopped (or restarted) right now.
    stopping: Arc<Mutex<HashSet<String>>>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl Power {
    pub fn is_stopping(&self, id: &str) -> bool {
        locked(&self.stopping).contains(id)
    }

    pub fn start(&self, tmux: &Tmux, events: &Events, server: &ServerConfig) -> Result<(), String> {
        if self.is_stopping(&server.id) {
            return Err("The server is still stopping.".into());
        }
        if !server.path.is_dir() {
            return Err("The server folder isn't there.".into());
        }
        tmux.start(server.session(), &server.path, &server.start)?;
        events.status(&server.id, "running");
        Ok(())
    }

    /// Asks the server to stop and answers at once; the rest happens in the background. With
    /// `again`, it starts the server once it has stopped (restart).
    pub fn stop(&self, tmux: &Tmux, events: &Arc<Events>, server: &ServerConfig, again: bool, wait: Duration) -> Result<(), String> {
        if again && server.start.trim().is_empty() {
            return Err("This server has no start command. Add `start = \"…\"` to it in the node's config.toml.".into());
        }
        if !locked(&self.stopping).insert(server.id.clone()) {
            return Err("The server is already stopping.".into());
        }
        if let Err(error) = tmux.send(server.session(), &server.stop) {
            locked(&self.stopping).remove(&server.id);
            return Err(error);
        }
        events.status(&server.id, "stopping");
        let (power, tmux, events, server) = (self.clone(), tmux.clone(), events.clone(), server.clone());
        std::thread::spawn(move || {
            let began = Instant::now();
            while tmux.running(server.session()) && began.elapsed() < wait {
                std::thread::sleep(Duration::from_millis(500));
            }
            if tmux.running(server.session()) {
                let _ = tmux.kill(server.session());
            }
            locked(&power.stopping).remove(&server.id);
            events.status(&server.id, "stopped");
            if again {
                // Give the old process a moment to let go of the port.
                std::thread::sleep(Duration::from_secs(1));
                if power.start(&tmux, &events, &server).is_err() {
                    events.status(&server.id, "stopped");
                }
            }
        });
        Ok(())
    }

    /// End task: ends the session now.
    pub fn force(&self, tmux: &Tmux, events: &Events, server: &ServerConfig) -> Result<(), String> {
        tmux.kill(server.session())?;
        // Asked for, so the team hears "stopped", not "crashed".
        events.status(&server.id, "stopping");
        events.status(&server.id, "stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for tmux: a shell script that keeps "sessions" as files in a folder.
    fn fake_tmux(base: &std::path::Path) -> Tmux {
        use std::os::unix::fs::PermissionsExt;
        let script = base.join("tmux");
        let sessions = base.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let body = format!(
            r#"#!/bin/sh
d="{}"
case "$1" in
  has-session) [ -f "$d/${{3#=}}" ] ;;
  new-session) printf '%s' "$7" > "$d/$4" ;;
  kill-session) rm -f "$d/${{3#=}}" ;;
  send-keys)
    t="${{3#=}}"; t="${{t%:}}"
    if [ "$4" = "-l" ] && [ "$6" = "stop" ]; then rm -f "$d/$t"; fi ;;
esac
"#,
            sessions.display()
        );
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Tmux { program: script.to_string_lossy().into_owned(), socket: String::new() }
    }

    fn server(root: &std::path::Path, start: &str) -> ServerConfig {
        ServerConfig {
            id: "s".into(),
            name: "S".into(),
            path: root.to_path_buf(),
            tmux: String::new(),
            log: "logs/latest.log".into(),
            start: start.into(),
            stop: "stop".into(),
        }
    }

    #[test]
    fn start_stop_restart_end() {
        let base = crate::files::tests::sandbox("power");
        let root = base.join("server");
        std::fs::create_dir_all(&root).unwrap();
        let tmux = fake_tmux(&base);
        let events = Arc::new(Events::default());
        let power = Power::default();

        assert!(power.start(&tmux, &events, &server(&root, "")).unwrap_err().contains("start command"));
        power.start(&tmux, &events, &server(&root, "java -jar server.jar nogui")).unwrap();
        assert!(tmux.running("s"));
        assert_eq!(std::fs::read_to_string(base.join("sessions/s")).unwrap(), "java -jar server.jar nogui");
        assert!(power.start(&tmux, &events, &server(&root, "x")).unwrap_err().contains("already running"));

        // Stop: the fake server leaves when it reads `stop`.
        power.stop(&tmux, &events, &server(&root, "x"), false, Duration::from_secs(5)).unwrap();
        let began = Instant::now();
        while power.is_stopping("s") && began.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!tmux.running("s"));
        let told: Vec<String> = events.wait(0, &["s".into()], Duration::ZERO)["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|one| one["payload"]["status"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(told, ["running", "stopping", "stopped"]);
        assert!(power.stop(&tmux, &events, &server(&root, "x"), false, STOP_WAIT).unwrap_err().contains("not running"));
        assert!(!power.is_stopping("s"), "a refused stop leaves nothing behind");

        // Restart comes back up; End task ends it.
        power.start(&tmux, &events, &server(&root, "first")).unwrap();
        power.stop(&tmux, &events, &server(&root, "second"), true, Duration::from_secs(5)).unwrap();
        let began = Instant::now();
        while !(tmux.running("s") && !power.is_stopping("s")) && began.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(std::fs::read_to_string(base.join("sessions/s")).unwrap(), "second");
        power.force(&tmux, &events, &server(&root, "x")).unwrap();
        assert!(!tmux.running("s"));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
