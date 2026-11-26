//! Telling the team what happened on this node's servers (`POST /v2/teams/{team}/activity`), as the app does
//! for its own: started, stopped, crashed, plugins added. Names only, never contents.
//!
//! The account server knows a team server by its own id, not by the node's server id, so the node reads its
//! teams (`GET /v2/teams/{team}`) and keeps which of their servers it hosts. It reads them again when a team's
//! version changes (the F7 loop) and every ten minutes.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::events::{Events, STATUS};

/// `(node server id, kind, data)`.
pub type Item = (String, &'static str, Value);

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Node server id → `(team, account server's server id)` for every team server this node hosts.
#[derive(Default)]
pub struct Targets(Mutex<HashMap<String, Vec<(String, String)>>>);

impl Targets {
    pub fn set(&self, map: HashMap<String, Vec<(String, String)>>) {
        *locked(&self.0) = map;
    }

    fn of(&self, server: &str) -> Vec<(String, String)> {
        locked(&self.0).get(server).cloned().unwrap_or_default()
    }
}

/// From `(team id, GET /v2/teams/{team} answer)` pairs: the servers whose host is `me`.
pub fn targets_of(me: &str, teams: &[(String, Value)]) -> HashMap<String, Vec<(String, String)>> {
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (team, answer) in teams {
        for server in answer["servers"].as_array().into_iter().flatten() {
            let (Some(instance), Some(id)) = (server["instance_id"].as_str(), server["id"].as_str()) else { continue };
            if server["host_device"].as_str() == Some(me) {
                map.entry(instance.to_string()).or_default().push((team.clone(), id.to_string()));
            }
        }
    }
    map
}

/// Turns the `server-status` steps of one server into what the team hears. A stop that nobody asked for
/// (running → stopped without stopping first) is a crash.
#[derive(Default)]
pub struct Steps(HashMap<String, String>);

impl Steps {
    pub fn step(&mut self, server: &str, status: &str) -> Option<&'static str> {
        let last = self.0.insert(server.to_string(), status.to_string());
        let last = last.as_deref();
        match status {
            "running" if last != Some("running") => Some("server_started"),
            "stopped" if last == Some("stopping") => Some("server_stopped"),
            "stopped" if last == Some("running") => Some("server_crashed"),
            _ => None,
        }
    }
}

/// Watches the node's events for status changes of `servers` and hands each one to `send`.
pub fn watch_status(events: Arc<Events>, servers: Vec<String>, send: Sender<Item>) {
    std::thread::spawn(move || {
        let mut steps = Steps::default();
        let mut at = events.wait(0, &[], Duration::ZERO)["at"].as_u64().unwrap_or(0);
        loop {
            let got = events.wait(at, &servers, Duration::from_secs(30));
            at = got["at"].as_u64().unwrap_or(at);
            for one in got["events"].as_array().into_iter().flatten() {
                if one["event"] != STATUS {
                    continue;
                }
                let payload = &one["payload"];
                let (Some(server), Some(status)) = (payload["serverId"].as_str(), payload["status"].as_str()) else { continue };
                if let Some(kind) = steps.step(server, status)
                    && send.send((server.to_string(), kind, json!({}))).is_err()
                {
                    return;
                }
            }
        }
    });
}

/// Reads the owner's teams and keeps the servers this node hosts. Leaves the old list when the account
/// server can't be reached.
pub fn refresh(targets: &Targets, accounts: &str, token: &str, me: &str) -> Result<(), String> {
    let teams = crate::account::team_versions(accounts, token)?;
    let mut answers = Vec::new();
    for (team, _) in teams {
        answers.push((team.clone(), crate::account::team(accounts, token, &team)?));
    }
    targets.set(targets_of(me, &answers));
    Ok(())
}

/// Sends what comes in to every team that has the server. Gives up when the node was removed.
pub fn report(receive: Receiver<Item>, targets: Arc<Targets>, accounts: String, token: String) {
    std::thread::spawn(move || {
        for (server, kind, data) in receive {
            for (team, id) in targets.of(&server) {
                if let Err(error) = crate::account::post_activity(&accounts, &token, &team, &id, kind, &data) {
                    if error == "unpaired" {
                        return;
                    }
                    eprintln!("monolab-node: could not tell the team ({kind}): {error}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_nobody_asked_for_is_a_crash() {
        let mut steps = Steps::default();
        assert_eq!(steps.step("a", "running"), Some("server_started"));
        assert_eq!(steps.step("a", "running"), None, "the watcher says it again");
        assert_eq!(steps.step("a", "stopping"), None);
        assert_eq!(steps.step("a", "stopped"), Some("server_stopped"));
        assert_eq!(steps.step("a", "stopped"), None, "the watcher sees the end too");
        assert_eq!(steps.step("a", "running"), Some("server_started"));
        assert_eq!(steps.step("a", "stopped"), Some("server_crashed"));
        assert_eq!(steps.step("b", "stopped"), None, "a server first seen stopped");
    }

    #[test]
    fn keeps_only_the_servers_this_node_hosts() {
        let teams = vec![
            (
                "T1".to_string(),
                json!({ "servers": [
                { "id": "S1", "instance_id": "survival", "host_device": "me" },
                { "id": "S2", "instance_id": "creative", "host_device": "someone" },
            ] }),
            ),
            ("T2".to_string(), json!({ "servers": [{ "id": "S3", "instance_id": "survival", "host_device": "me" }, { "id": "S4" }] })),
        ];
        let map = targets_of("me", &teams);
        assert_eq!(map.len(), 1);
        assert_eq!(map["survival"], [("T1".to_string(), "S1".to_string()), ("T2".to_string(), "S3".to_string())]);
    }
}
