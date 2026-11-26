//! Monolink on the host: which of this PC's servers are attached to which team, and answering a
//! teammate's request with what their role grant allows. The grant comes from the account server;
//! the host still decides, and opens only servers it attached itself (section 5.2-4 and 7 of
//! `docs/monolink.md`). The connection itself is in `direct.rs`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use relay_proto::grant::{Grant, Role};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::web;

/// One team this PC hosts servers for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamLink {
    pub team: String,
    pub servers: Vec<TeamServerLink>,
    /// Teammates the host gave a role of its own on this PC's servers, instead of the one their team role brings.
    #[serde(default)]
    pub people: Vec<TeamPersonRole>,
}

/// A teammate (by handle) and the role they act with on this PC's servers: Manager, Viewer or a custom role.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamPersonRole {
    pub handle: String,
    pub role: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamServerLink {
    /// The server's id in this app.
    pub instance: String,
    /// Its id on the account server.
    pub id: String,
    /// Console commands only from Admins and the Owner.
    #[serde(default)]
    pub admin_only: bool,
    /// Every teammate's agent may work on this server (their own agent account, the host approving changes).
    #[serde(default)]
    pub agents: bool,
    /// Teammates (handles) whose agents may work on it even with `agents` off.
    #[serde(default)]
    pub agent_people: Vec<String>,
    /// F8: a Developer's file changes wait for review (change proposals). Stored on this PC only.
    #[serde(default)]
    pub proposals: bool,
}

/// Grant keys this app trusts, by number: `monolab-relay --data … --grant-pin` on mnlab.cc prints them.
/// A new key is added here (an app update) before the server starts using it.
const GRANT_KEYS: &[(u8, &str)] = &[(1, "b0f2c72bf12713ed2812b8f8bb86b470f36cd162cf5442783678f954aad326ae")];

/// The keys above, plus `MONOLAB_GRANT_KEYS=1:<hex>,2:<hex>` for a local test server. Whoever can set
/// this PC's environment already controls the host app, so the variable gives nothing away.
pub fn trusted_keys() -> Vec<(u8, [u8; 32])> {
    // A test server's keys come first: its key number 1 would otherwise be checked against mnlab.cc's key 1.
    let built_in = GRANT_KEYS.iter().map(|(kid, key)| format!("{kid}:{key}")).collect::<Vec<_>>().join(",");
    let mut keys = parse_keys(&std::env::var("MONOLAB_GRANT_KEYS").unwrap_or_default());
    keys.extend(parse_keys(&built_in));
    keys
}

/// `1:<hex>,2:<hex>`; anything malformed is skipped.
fn parse_keys(text: &str) -> Vec<(u8, [u8; 32])> {
    text.split(',')
        .filter_map(|one| {
            let (kid, key) = one.trim().split_once(':')?;
            Some((kid.parse().ok()?, relay_proto::unhex(key)?.try_into().ok()?))
        })
        .collect()
}

thread_local! {
    /// The teammate whose console command is running on this thread, for the "who sent it" line.
    static CONSOLE_BY: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Who sent the console command being run right now ("" for this PC's own user).
pub fn console_by() -> String {
    CONSOLE_BY.with(|by| by.borrow().clone())
}

fn with_console_by<T>(handle: &str, run: impl FnOnce() -> T) -> T {
    CONSOLE_BY.with(|by| *by.borrow_mut() = handle.to_string());
    let out = run();
    CONSOLE_BY.with(|by| by.borrow_mut().clear());
    out
}

fn links(app: &AppHandle) -> Vec<TeamLink> {
    crate::read_app_settings(app).monolink_teams
}

/// The servers a grant may reach here: in the grant, attached to that team by this host, and still on this PC.
pub fn reach(app: &AppHandle, grant: &Grant) -> Vec<String> {
    let attached: Vec<String> = links(app).into_iter().filter(|one| one.team == grant.team).flat_map(|one| one.servers).map(|one| one.instance).collect();
    let present: Vec<String> = crate::load_instances(app).unwrap_or_default().into_iter().map(|one| one.id).collect();
    grant.servers.iter().filter(|id| attached.contains(id) && present.contains(id)).cloned().collect()
}

/// This host's attached servers for a team, with their switches.
pub fn server_links(app: &AppHandle, team: &str) -> Vec<TeamServerLink> {
    links(app).into_iter().filter(|one| one.team == team).flat_map(|one| one.servers).collect()
}

/// F8: whether this server holds a Developer's changes for review.
pub fn proposals_on(app: &AppHandle, instance: &str) -> bool {
    links(app).iter().flat_map(|one| &one.servers).any(|one| one.instance == instance && one.proposals)
}

/// The "Review changes" switch of one attached server (F8). Stored on this PC only.
#[tauri::command]
#[specta::specta]
pub fn monolink_team_proposals(app: AppHandle, team: String, instance: String, on: bool) -> Result<Vec<TeamLink>, String> {
    let mut settings = crate::read_app_settings(&app);
    let server = settings
        .monolink_teams
        .iter_mut()
        .filter(|one| one.team == team)
        .flat_map(|one| one.servers.iter_mut())
        .find(|one| one.instance == instance)
        .ok_or("That server isn't attached to this team.")?;
    server.proposals = on;
    crate::write_app_settings(&app, &settings)?;
    Ok(settings.monolink_teams)
}

fn admin_only(app: &AppHandle, team: &str, instance: &str) -> bool {
    links(app).iter().filter(|one| one.team == team).flat_map(|one| &one.servers).any(|one| one.instance == instance && one.admin_only)
}

/// The web panel account a teammate acts as: Viewer stays Viewer, every other role is Manager on the
/// reachable servers, unless the host gave this teammate a role of its own here (`chosen`, a Manager,
/// Viewer or custom role that still exists). Never Admin, so the app-wide admin commands stay shut (section 7).
pub fn account_of(grant: &Grant, servers: Vec<String>, chosen: Option<&str>, custom: &[web::auth::CustomRole]) -> web::auth::Account {
    let role = match chosen.filter(|one| web::auth::is_server_role(one, custom)) {
        Some(one) => one.to_string(),
        None if grant.role == Role::Viewer => web::auth::VIEWER.into(),
        None => web::auth::MANAGER.into(),
    };
    web::auth::Account { name: format!("@{}", grant.handle), hash: String::new(), role, servers }
}

/// The role the host gave this teammate on its servers, if any.
fn chosen_role(app: &AppHandle, team: &str, handle: &str) -> Option<String> {
    links(app).into_iter().filter(|one| one.team == team).flat_map(|one| one.people).find(|one| one.handle == handle).map(|one| one.role)
}

/// Gives a teammate a role of their own on this PC's servers in a team, or (`role` empty) back the
/// one their team role brings. Stored on this PC only; the team roles on the account server don't change.
#[tauri::command]
#[specta::specta]
pub fn monolink_team_person_role(app: AppHandle, team: String, handle: String, role: String) -> Result<Vec<TeamLink>, String> {
    let mut settings = crate::read_app_settings(&app);
    let handle = handle.trim().trim_start_matches('@').to_string();
    if handle.is_empty() {
        return Err("Unknown teammate.".into());
    }
    if !role.is_empty() && !web::auth::is_server_role(&role, &settings.custom_roles) {
        return Err("Pick Manager, Viewer or one of your own roles.".into());
    }
    let named = crate::role_name(&role, &settings.custom_roles);
    let link = settings.monolink_teams.iter_mut().find(|one| one.team == team).ok_or("None of this PC's servers are in that team.")?;
    link.people.retain(|one| one.handle != handle);
    if !role.is_empty() {
        link.people.push(TeamPersonRole { handle: handle.clone(), role });
    }
    let servers: Vec<String> = link.servers.iter().map(|one| one.instance.clone()).collect();
    crate::write_app_settings(&app, &settings)?;
    let words = if named.is_empty() { format!("@{handle}: team role") } else { format!("@{handle}: {named}") };
    for server in servers {
        crate::record_audit(&app, &server, "Server", "Monolink: teammate role on this PC", &words);
    }
    Ok(settings.monolink_teams)
}

/// What a teammate's app learns about the servers when it connects.
pub fn welcome(app: &AppHandle, grant: &Grant) -> Value {
    let servers = reach(app, grant);
    let instances = crate::load_instances(app).unwrap_or_default();
    let list: Vec<Value> = servers
        .iter()
        .map(|id| {
            let name = instances.iter().find(|one| one.id == *id).map(|one| one.name.clone()).unwrap_or_default();
            json!({ "instance": id, "name": name, "adminOnly": admin_only(app, &grant.team, id) })
        })
        .collect();
    json!({ "role": grant.role.as_str(), "servers": list })
}

/// Why a team role may not run a command here, before anything runs. The same for a teammate and
/// their agent. `admin_only` is the server's "commands only from admins" switch.
pub fn role_refusal(role: Role, command: &str, body: &Value, admin_only: bool) -> Option<&'static str> {
    if command == "send_console_command" && role < Role::Admin && admin_only {
        return Some("Only admins can run commands on this server.");
    }
    if web::api::SERVER_OWNER.contains(&command) && role < Role::Owner {
        return Some("Only the team owner can do this.");
    }
    if web::api::SERVER_ADMINS.contains(&command) && role < Role::Admin {
        return Some("Only admins can do this.");
    }
    if crate::team_agent::touches_rules(command, body) && role < Role::Admin {
        return Some("Only the host and team admins can change this server's AGENTS.md.");
    }
    None
}

/// Answers one request from a teammate whose grant was checked when the connection opened (or renewed).
/// A request marked as coming from the teammate's agent first passes the host's agent gate.
pub fn serve(app: &AppHandle, peer: &relay_proto::DeviceId, grant: &Grant, command: &str, body: &Value) -> Result<Value, String> {
    let agent = crate::team_agent::is_agent(body);
    let (body, chat) = crate::team_agent::unmark(body);
    // Someone working on a server through their window (not the long event wait, not their agent) is "here".
    if !agent && command != "__events" && command != "__presence" {
        mark_seen(body.get("id").and_then(Value::as_str).unwrap_or_default(), &grant.handle);
    }
    let result = if agent {
        crate::team_agent::gate(app, grant, command, &body, &chat).and_then(|()| serve_one(app, peer, grant, command, &body, true))
    } else {
        serve_one(app, peer, grant, command, &body, false)
    };
    // The account already noted `@handle` for the files this wrote; an agent's writes say so (F2, Rewind "who").
    if agent && result.is_ok() {
        crate::note_writes(app, body.get("id").and_then(Value::as_str).unwrap_or_default(), command, &body, &crate::authorship::teammate(&grant.handle, true));
    }
    let via = if agent { format!("team · {} · agent", grant.role.as_str()) } else { format!("team · {}", grant.role.as_str()) };
    crate::teamlog::record(app, &format!("@{}", grant.handle), &via, &peer.text(), command, &body, &result);
    result
}

fn serve_one(app: &AppHandle, peer: &relay_proto::DeviceId, grant: &Grant, command: &str, body: &Value, agent: bool) -> Result<Value, String> {
    let servers = reach(app, grant);
    if servers.is_empty() {
        return Err("No server here is shared with this team any more.".into());
    }
    let custom = crate::custom_roles(app);
    let account = account_of(grant, servers.clone(), chosen_role(app, &grant.team, &grant.handle).as_deref(), &custom);
    let id = body.get("id").and_then(Value::as_str).unwrap_or_default();
    if let Some(why) = role_refusal(grant.role, command, body, admin_only(app, &grant.team, id)) {
        return Err(why.into());
    }
    // F8: a Developer's change waits for review when the server says so (checked like any write first).
    if crate::proposals::held(proposals_on(app, id), grant.role.as_str(), command) {
        if !servers.iter().any(|one| one == id) {
            return Err("You don't have permission for this server.".into());
        }
        if !web::api::permits(&web::api::perms_for(app, &account), command) {
            return Err("Your role doesn't allow this here.".into());
        }
        let by = crate::authorship::teammate(&grant.handle, agent);
        let proposal = tauri::async_runtime::block_on(crate::propose(app, id, &by, command, body))?;
        return Ok(json!({ "proposed": proposal.id }));
    }
    match command {
        "__whoami" => Ok(json!({ "name": account.name, "role": account.role, "roleName": crate::role_name(&account.role, &custom), "teamRole": grant.role.as_str() })),
        "__events" => {
            let after = body.get("after").and_then(Value::as_u64).unwrap_or(0);
            let text = web::api::wait_for_events(app, after, std::time::Duration::from_secs(20));
            Ok(web::api::without_console(web::api::events_for(&text, &servers), &web::api::perms_for(app, &account)))
        }
        "__upload_begin" | "__upload_part" | "__upload_end" => crate::monolink_upload(app, peer, command, body),
        // Who else is on this server: other teammates, and `@host` while the host's own window shows it.
        "__presence" => {
            if !servers.iter().any(|one| one == id) {
                return Err("You don't have permission for this server.".into());
            }
            let mut here = present(id, &grant.handle);
            if host_viewing().as_deref() == Some(id) {
                here.insert(0, "host".into());
            }
            Ok(json!(here))
        }
        // F6: the team's Storage on this host, and downloads (Storage files, a studio bundle made for them).
        _ if command.starts_with("__storage_") => storage_request(app, grant, &servers, command, body),
        "__download_begin" => download_begin(app, grant, &servers, body),
        "__download_part" => download_part(grant, body),
        // F8: my proposals on this server; Admins and the Owner see and answer all of them.
        "__proposals" => {
            if !servers.iter().any(|one| one == id) {
                return Err("You don't have permission for this server.".into());
            }
            let mine = format!("@{}", grant.handle);
            let all = crate::proposals_of(app, id)?;
            let shown: Vec<_> = if grant.role >= Role::Admin { all } else { all.into_iter().filter(|one| one.by == mine || one.by == format!("{mine}'s AI")).collect() };
            serde_json::to_value(shown).map_err(|error| error.to_string())
        }
        "__proposal_merge" | "__proposal_reject" => {
            if grant.role < Role::Admin {
                return Err("Only team admins and the owner can answer change proposals.".into());
            }
            if !servers.iter().any(|one| one == id) {
                return Err("You don't have permission for this server.".into());
            }
            let proposal = body.get("proposal").and_then(Value::as_str).unwrap_or_default();
            if command == "__proposal_merge" {
                tauri::async_runtime::block_on(crate::merge_proposal(app, id, proposal, &format!("@{}", grant.handle))).map(|()| Value::Null)
            } else {
                crate::reject_proposal(app, id, proposal, body.get("reason").and_then(Value::as_str).unwrap_or_default()).map(|()| Value::Null)
            }
        }
        "__proposal_texts" => {
            if !servers.iter().any(|one| one == id) {
                return Err("You don't have permission for this server.".into());
            }
            let proposal = body.get("proposal").and_then(Value::as_str).unwrap_or_default();
            serde_json::to_value(tauri::async_runtime::block_on(crate::proposal_texts_of(app, id, proposal))?).map_err(|error| error.to_string())
        }
        // F3: who a to-do on this server can be given to.
        "__team_people" => {
            if !servers.iter().any(|one| one == id) {
                return Err("You don't have permission for this server.".into());
            }
            Ok(json!(tauri::async_runtime::block_on(team_people(app, id))))
        }
        // F2: this teammate has a file open (or closed it); answers with everyone's open files on that server.
        "__editing" => {
            if !servers.iter().any(|one| one == id) {
                return Err("You don't have permission for this server.".into());
            }
            let path = body.get("path").and_then(Value::as_str).unwrap_or_default();
            if !path.is_empty() {
                let closed = body.get("closed").and_then(Value::as_bool).unwrap_or(false);
                crate::editing::mark(app, id, path, &format!("@{}", grant.handle), closed);
            }
            serde_json::to_value(crate::editing::open_files(id)).map_err(|error| error.to_string())
        }
        "send_console_command" => with_console_by(&grant.handle, || web::api::run(app, command, body, Some(&account))),
        // Admin-only for web panel accounts; a team Admin or Owner (checked above) on a team server.
        "save_server_agents_md" => web::api::run_server_admin(app, command, body, &account),
        _ if web::api::SERVER_OWNER.contains(&command) || web::api::SERVER_ADMINS.contains(&command) => web::api::run_server_admin(app, command, body, &account),
        _ => web::api::run(app, command, body, Some(&account)),
    }
}

// ── attaching servers (host side) ──────────────────────────────────────────

/// This PC's team links, for the screens.
#[tauri::command]
#[specta::specta]
pub fn monolink_team_links(app: AppHandle) -> Vec<TeamLink> {
    links(&app)
}

/// Attaches one of this PC's servers to a team: on the account server first, then here.
#[tauri::command(async)]
#[specta::specta]
pub async fn monolink_team_attach(app: AppHandle, team: String, instance: String) -> Result<Vec<TeamLink>, String> {
    let found = crate::instance_by_id(&app, &instance)?;
    let name = if found.name.trim().is_empty() { instance.clone() } else { found.name.clone() };
    let answer = crate::monolink_account::api(&app, "POST", &format!("/v2/teams/{team}/servers"), Some(json!({ "instance_id": instance, "name": name }))).await?;
    let id = answer["servers"]
        .as_array()
        .and_then(|list| list.iter().find(|one| one["instance_id"] == instance.as_str() && one["mine"] == true))
        .and_then(|one| one["id"].as_str())
        .ok_or("The account server didn't list the server.")?
        .to_string();
    let mut settings = crate::read_app_settings(&app);
    match settings.monolink_teams.iter_mut().find(|one| one.team == team) {
        Some(link) => {
            link.servers.retain(|one| one.instance != instance);
            link.servers.push(TeamServerLink { instance: instance.clone(), id, ..Default::default() });
        }
        None => settings.monolink_teams.push(TeamLink { team: team.clone(), servers: vec![TeamServerLink { instance: instance.clone(), id, ..Default::default() }], people: Vec::new() }),
    }
    crate::write_app_settings(&app, &settings)?;
    crate::record_audit(&app, &instance, "Server", "Monolink: attached to a team", &team);
    crate::monolink_account::publish_addr(&app);
    Ok(settings.monolink_teams)
}

/// Takes a server out of a team, here first (so it closes at once) and then on the account server.
#[tauri::command(async)]
#[specta::specta]
pub async fn monolink_team_detach(app: AppHandle, team: String, instance: String) -> Result<Vec<TeamLink>, String> {
    let mut settings = crate::read_app_settings(&app);
    let mut gone = None;
    for link in settings.monolink_teams.iter_mut().filter(|one| one.team == team) {
        if let Some(at) = link.servers.iter().position(|one| one.instance == instance) {
            gone = Some(link.servers.remove(at));
        }
    }
    settings.monolink_teams.retain(|one| !one.servers.is_empty());
    crate::write_app_settings(&app, &settings)?;
    crate::direct::close_team(&team);
    if let Some(gone) = gone {
        crate::record_audit(&app, &instance, "Server", "Monolink: taken out of a team", &team);
        // Already gone there (someone else detached it) is fine.
        match crate::monolink_account::api(&app, "DELETE", &format!("/v2/teams/{team}/servers/{}", gone.id), None).await {
            Ok(_) => {}
            Err(error) if error.contains("not_found") => {}
            Err(error) => return Err(error),
        }
    }
    Ok(settings.monolink_teams)
}

/// The "commands only from Admins" switch of one attached server. Stored on this PC only.
#[tauri::command]
#[specta::specta]
pub fn monolink_team_admin_only(app: AppHandle, team: String, instance: String, on: bool) -> Result<Vec<TeamLink>, String> {
    let mut settings = crate::read_app_settings(&app);
    let server = settings
        .monolink_teams
        .iter_mut()
        .filter(|one| one.team == team)
        .flat_map(|one| one.servers.iter_mut())
        .find(|one| one.instance == instance)
        .ok_or("That server isn't attached to this team.")?;
    server.admin_only = on;
    crate::write_app_settings(&app, &settings)?;
    Ok(settings.monolink_teams)
}

/// The "Teammates' agents" switch of one attached server: `on` lets every teammate's agent in,
/// `people` (handles) lets just those in while it is off. Stored on this PC only.
#[tauri::command]
#[specta::specta]
pub fn monolink_team_agents(app: AppHandle, team: String, instance: String, on: bool, people: Vec<String>) -> Result<Vec<TeamLink>, String> {
    let mut settings = crate::read_app_settings(&app);
    let server = settings
        .monolink_teams
        .iter_mut()
        .filter(|one| one.team == team)
        .flat_map(|one| one.servers.iter_mut())
        .find(|one| one.instance == instance)
        .ok_or("That server isn't attached to this team.")?;
    let mut people: Vec<String> = people.into_iter().map(|one| one.trim().trim_start_matches('@').to_string()).filter(|one| !one.is_empty()).collect();
    people.sort();
    people.dedup();
    let narrowed = (server.agents && !on) || server.agent_people.iter().any(|one| !people.contains(one));
    server.agents = on;
    server.agent_people = people;
    crate::write_app_settings(&app, &settings)?;
    if narrowed {
        crate::team_agent::forget_chats(&team);
    }
    let words = if on { "Monolink: teammates' agents allowed" } else { "Monolink: teammates' agents limited" };
    crate::record_audit(&app, &instance, "Server", words, &team);
    Ok(settings.monolink_teams)
}

/// The account server forgot a team or one of our servers in it (deleted, or we were removed):
/// forget it here too, so nothing stays open for it.
pub fn prune(app: &AppHandle, team: &str, still_there: Option<&[String]>) {
    let mut settings = crate::read_app_settings(app);
    let before = settings.monolink_teams.clone();
    match still_there {
        None => settings.monolink_teams.retain(|one| one.team != team),
        Some(ids) => {
            for link in settings.monolink_teams.iter_mut().filter(|one| one.team == team) {
                link.servers.retain(|one| ids.contains(&one.id));
            }
            settings.monolink_teams.retain(|one| !one.servers.is_empty());
        }
    }
    if settings.monolink_teams != before {
        let _ = crate::write_app_settings(app, &settings);
    }
}

// ── activity (host side) ───────────────────────────────────────────────────

/// Tells every team a server is attached to that something happened on it (started, stopped,
/// crashed). Names and paths only, never contents. Does nothing for servers in no team.
/// Plugins named by an app log entry about adding or installing them ("Added a.jar, b.jar.", "Installed x.jar.",
/// "Some Title 1.2"), for the team's `plugin_added` activity.
pub fn added_plugins(category: &str, action: &str, detail: &str) -> Vec<String> {
    if !matches!(category, "Mod" | "Mods & plugins") {
        return Vec::new();
    }
    let action = action.to_ascii_lowercase();
    // "Added manually…", "Add manually (SSH)", "…install…" — not "Add‑on deleted/toggled".
    let adds = action.starts_with("added") || action.starts_with("add manually") || action.contains("install");
    if !adds || action.contains("modpack") {
        return Vec::new();
    }
    let text = detail.trim().trim_end_matches('.');
    let text = text.strip_prefix("Added ").or_else(|| text.strip_prefix("Installed ")).unwrap_or(text);
    // "X and 3 more" (installs with dependencies) names the one asked for.
    let text = text.split(" and ").next().unwrap_or(text);
    text.split(", ").map(str::trim).filter(|one| !one.is_empty()).take(5).map(str::to_string).collect()
}

/// A file on a team server was saved: `file_changed { path }` (the path only, never the text), at most once a
/// minute for the same file so typing-and-saving doesn't flood the team's activity.
pub fn file_changed(app: &AppHandle, instance: &str, path: &str) {
    static LAST: OnceLock<Mutex<HashMap<(String, String), Instant>>> = OnceLock::new();
    {
        let mut last = LAST.get_or_init(Default::default).lock().unwrap_or_else(|poison| poison.into_inner());
        let key = (instance.to_string(), path.to_string());
        if last.get(&key).is_some_and(|at| at.elapsed() < Duration::from_secs(60)) {
            return;
        }
        last.retain(|_, at| at.elapsed() < Duration::from_secs(60));
        last.insert(key, Instant::now());
    }
    report(app, instance, "file_changed", json!({ "path": path }));
}

/// The team this PC shares a server with, if any (the first one when several).
pub fn team_of(app: &AppHandle, instance: &str) -> Option<String> {
    links(app).into_iter().find(|one| one.servers.iter().any(|server| server.instance == instance)).map(|one| one.team)
}

// ── F6: Storage and downloads for teammates ────────────────────────────────

/// A teammate's Storage request. Roles as in `team_storage::may`.
fn storage_request(app: &AppHandle, grant: &Grant, servers: &[String], command: &str, body: &Value) -> Result<Value, String> {
    let role = grant.role.as_str();
    let text = |key: &str| body.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let number = |key: &str| body.get(key).and_then(Value::as_u64).unwrap_or(0) as u32;
    let need = |action: &str| if crate::team_storage::may(role, action) { Ok(()) } else { Err("Your role in this team can't do that in Storage.".to_string()) };
    let server_ok = |id: &str| if servers.iter().any(|one| one == id) { Ok(()) } else { Err("You don't have permission for this server.".to_string()) };
    let storage = crate::team_storage_of(app, &grant.team)?;
    let by = format!("@{}", grant.handle);
    let json = |value: Result<Value, String>| value;
    match command {
        "__storage_view" => serde_json::to_value(storage.view()).map_err(|error| error.to_string()),
        "__storage_put" => {
            need("put")?;
            let path = crate::remote_upload_path(app, &text("upload"))?;
            let tags: Vec<String> = body.get("tags").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            let item = storage.put(std::path::Path::new(&path), &text("name"), &text("note"), &tags, &by);
            crate::unstash(std::path::Path::new(&path));
            let item = item?;
            if let Some(server) = servers.first() {
                report(app, server, "storage_added", json!({ "name": item.name }));
            }
            json(serde_json::to_value(item).map_err(|error| error.to_string()))
        }
        "__storage_trash" => {
            need("trash")?;
            storage.set_trashed(&text("item"), body.get("trashed").and_then(Value::as_bool).unwrap_or(true)).map(|()| Value::Null)
        }
        "__storage_empty_trash" => {
            need("trash")?;
            storage.empty_trash().map(|gone| json!(gone))
        }
        "__storage_set_limit" => {
            need("limit")?;
            storage.set_limit(u64::from(number("gb"))).map(|()| Value::Null)
        }
        "__storage_to_server" => {
            need("use")?;
            let server = text("id");
            server_ok(&server)?;
            let path = tauri::async_runtime::block_on(crate::storage_into_server(app, &grant.team, &text("item"), number("version"), &server, &text("folder")))?;
            crate::note_writes(app, &server, "save_editable_file", &json!({ "path": path }), &by);
            Ok(json!(path))
        }
        "__storage_from_server" => {
            need("put")?;
            let server = text("id");
            server_ok(&server)?;
            let item = tauri::async_runtime::block_on(crate::storage_from_server(app, &grant.team, &server, &text("path"), &by))?;
            serde_json::to_value(item).map_err(|error| error.to_string())
        }
        _ => Err("Unknown Storage request.".into()),
    }
}

/// Files a teammate is downloading: id → (team, file, size, when it opened, delete it when done). Only the same
/// team's grant may read it.
type Downloads = Mutex<HashMap<String, (String, std::path::PathBuf, u64, Instant, bool)>>;

fn downloads() -> &'static Downloads {
    static DOWNLOADS: OnceLock<Downloads> = OnceLock::new();
    DOWNLOADS.get_or_init(Default::default)
}

/// How long a download stays open, and the piece size (base64 grows it by a third; the link takes 4 MB).
const DOWNLOAD_FOR: Duration = Duration::from_secs(10 * 60);
const DOWNLOAD_PART: u64 = 1024 * 1024;

/// Opens a download: a Storage file (`kind: "storage"`, `item`, `version`) or a studio bundle the host makes now
/// (`kind: "bundle"`, `id`, `name`, `resources`, `includeAssets`). Gives `{ download, size, name }`.
fn download_begin(app: &AppHandle, grant: &Grant, servers: &[String], body: &Value) -> Result<Value, String> {
    let text = |key: &str| body.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let (path, name, temp) = match text("kind").as_str() {
        "storage" => {
            let storage = crate::team_storage_of(app, &grant.team)?;
            let item = text("item");
            let name = storage.index().items.into_iter().find(|one| one.id == item).map(|one| one.name).ok_or("There is no such file in Storage.")?;
            (storage.file(&item, body.get("version").and_then(Value::as_u64).unwrap_or(0) as u32)?, name, false)
        }
        "bundle" => {
            let server = text("id");
            if !servers.contains(&server) {
                return Err("You don't have permission for this server.".into());
            }
            let resources: Vec<crate::studio_bundle::StudioRef> = serde_json::from_value(body.get("resources").cloned().unwrap_or(Value::Null)).map_err(|_| "Pick the files to share.".to_string())?;
            let name = text("name");
            let path = std::env::temp_dir().join(format!("monolab-bundle-{}-{}.monolab", grant.handle, crate::unix_now()));
            let include = body.get("includeAssets").and_then(Value::as_bool).unwrap_or(true);
            tauri::async_runtime::block_on(crate::studio_bundle::studio_bundle_export(app.clone(), server, name.clone(), resources, include, path.to_string_lossy().to_string()))?;
            let file = if name.trim().is_empty() { "studio.monolab".to_string() } else { format!("{}.monolab", name.trim()) };
            (path, file, true)
        }
        _ => return Err("Unknown download.".into()),
    };
    let size = std::fs::metadata(&path).map_err(|error| format!("Could not read the file: {error}"))?.len();
    let id = format!("dl-{}-{}", crate::unix_now(), downloads().lock().unwrap_or_else(|poison| poison.into_inner()).len());
    let mut held = downloads().lock().unwrap_or_else(|poison| poison.into_inner());
    held.retain(|_, (_, old, _, at, temp)| {
        let keep = at.elapsed() < DOWNLOAD_FOR;
        if !keep && *temp {
            let _ = std::fs::remove_file(old);
        }
        keep
    });
    held.insert(id.clone(), (grant.team.clone(), path, size, Instant::now(), temp));
    Ok(json!({ "download": id, "size": size, "name": name }))
}

/// One piece of an open download: `{ data }` (base64), empty at the end, which closes it.
fn download_part(grant: &Grant, body: &Value) -> Result<Value, String> {
    use base64::Engine;
    use std::io::{Read, Seek, SeekFrom};
    let id = body.get("download").and_then(Value::as_str).unwrap_or_default();
    let at = body.get("at").and_then(Value::as_u64).unwrap_or(0);
    let (path, size, temp) = {
        let held = downloads().lock().unwrap_or_else(|poison| poison.into_inner());
        let (team, path, size, _, temp) = held.get(id).filter(|one| one.0 == grant.team).ok_or("This download has ended. Start it again.")?;
        let _ = team;
        (path.clone(), *size, *temp)
    };
    let Some((from, to)) = crate::team_storage::part_range(size, at, DOWNLOAD_PART) else {
        if let Some((_, path, _, _, temp)) = downloads().lock().unwrap_or_else(|poison| poison.into_inner()).remove(id) {
            if temp {
                let _ = std::fs::remove_file(path);
            }
        }
        return Ok(json!({ "data": "" }));
    };
    let _ = temp;
    let mut file = std::fs::File::open(&path).map_err(|error| format!("Could not read the file: {error}"))?;
    file.seek(SeekFrom::Start(from)).map_err(|error| error.to_string())?;
    let mut piece = vec![0u8; (to - from) as usize];
    file.read_exact(&mut piece).map_err(|error| format!("Could not read the file: {error}"))?;
    Ok(json!({ "data": base64::engine::general_purpose::STANDARD.encode(piece) }))
}

/// Everyone in the teams this PC shares a server with, as `@handle` (F3: who a to-do can be given to). Asks the
/// account server, at most once a minute per team.
pub async fn team_people(app: &AppHandle, instance: &str) -> Vec<String> {
    /// Team id → when it was read, and its members.
    type People = Mutex<HashMap<String, (Instant, Vec<String>)>>;
    static CACHE: OnceLock<People> = OnceLock::new();
    let teams: Vec<String> = links(app).into_iter().filter(|one| one.servers.iter().any(|server| server.instance == instance)).map(|one| one.team).collect();
    let mut out = Vec::new();
    for team in teams {
        let cached = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|poison| poison.into_inner()).get(&team).filter(|(at, _)| at.elapsed() < Duration::from_secs(60)).map(|(_, list)| list.clone());
        let list = match cached {
            Some(list) => list,
            None => {
                let Ok(answer) = crate::monolink_account::api(app, "GET", &format!("/v2/teams/{team}"), None).await else { continue };
                let list: Vec<String> = answer["members"].as_array().into_iter().flatten().filter_map(|one| one["handle"].as_str()).map(|handle| format!("@{handle}")).collect();
                CACHE.get_or_init(Default::default).lock().unwrap_or_else(|poison| poison.into_inner()).insert(team.clone(), (Instant::now(), list.clone()));
                list
            }
        };
        for one in list {
            if !out.contains(&one) {
                out.push(one);
            }
        }
    }
    out.sort();
    out
}

/// A picture, model or sound went into a team server's studio (its name only).
pub fn studio_asset_added(app: &AppHandle, instance: &str, name: &str) {
    report(app, instance, "studio_asset_added", json!({ "name": name }));
}

/// Teammates who asked the host about a server lately: (instance, handle) → when. Kept in memory only.
fn seen() -> &'static Mutex<HashMap<(String, String), Instant>> {
    static SEEN: OnceLock<Mutex<HashMap<(String, String), Instant>>> = OnceLock::new();
    SEEN.get_or_init(Default::default)
}

/// How long a teammate counts as "here" after their last request about a server.
const PRESENT_FOR: Duration = Duration::from_secs(60);

fn mark_seen(instance: &str, handle: &str) {
    if instance.is_empty() {
        return;
    }
    let mut all = seen().lock().unwrap_or_else(|poison| poison.into_inner());
    all.retain(|_, at| at.elapsed() < PRESENT_FOR);
    all.insert((instance.to_string(), handle.to_ascii_lowercase()), Instant::now());
}

/// Teammates (`@handle`) looking at a server on this PC right now, leaving out `except`.
pub fn present(instance: &str, except: &str) -> Vec<String> {
    let all = seen().lock().unwrap_or_else(|poison| poison.into_inner());
    let mut out: Vec<String> = all
        .iter()
        .filter(|((id, handle), at)| id == instance && at.elapsed() < PRESENT_FOR && !handle.eq_ignore_ascii_case(except))
        .map(|((_, handle), _)| format!("@{handle}"))
        .collect();
    out.sort();
    out
}

/// The server the host's own window shows, and when it last said so (it asks every few seconds while open).
fn host_page() -> &'static Mutex<Option<(String, Instant)>> {
    static PAGE: OnceLock<Mutex<Option<(String, Instant)>>> = OnceLock::new();
    PAGE.get_or_init(Default::default)
}

/// The server the host is looking at in this app right now, if any.
fn host_viewing() -> Option<String> {
    host_page().lock().unwrap_or_else(|poison| poison.into_inner()).as_ref().filter(|(_, at)| at.elapsed() < PRESENT_FOR).map(|(id, _)| id.clone())
}

/// Teammates looking at one of this PC's servers right now, for the host's own server page (which also tells
/// teammates the host is there).
#[tauri::command]
#[specta::specta]
pub fn team_presence(id: String) -> Vec<String> {
    *host_page().lock().unwrap_or_else(|poison| poison.into_inner()) = Some((id.clone(), Instant::now()));
    present(&id, "")
}

pub fn report(app: &AppHandle, instance: &str, kind: &'static str, data: Value) {
    let targets: Vec<(String, String)> = links(app)
        .into_iter()
        .flat_map(|link| link.servers.into_iter().filter(|one| one.instance == instance).map(move |one| (link.team.clone(), one.id)))
        .collect();
    for (team, id) in targets {
        let app = app.clone();
        let data = data.clone();
        tauri::async_runtime::spawn(async move {
            let _ = crate::monolink_account::api(&app, "POST", &format!("/v2/teams/{team}/activity"), Some(json!({ "server": id, "kind": kind, "data": data }))).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_added_plugins_from_log_entries() {
        assert_eq!(added_plugins("Mods & plugins", "Added manually", "Added a.jar, b.jar."), ["a.jar", "b.jar"]);
        assert_eq!(added_plugins("Mod", "Modrinth install", "Installed lp.jar."), ["lp.jar"]);
        assert_eq!(added_plugins("Mod", "Installed with dependencies", "LuckPerms and 2 more"), ["LuckPerms"]);
        assert_eq!(added_plugins("Mods & plugins", "Install from Pumpkin Market", "PumpkinTAB 1.2"), ["PumpkinTAB 1.2"]);
        assert!(added_plugins("Mod", "Add‑on deleted", "a.jar").is_empty());
        assert!(added_plugins("Mod", "Modpack export", "3 items").is_empty());
        assert!(added_plugins("File", "Added", "x").is_empty());
    }

    #[test]
    fn teammates_count_as_here_for_a_minute() {
        mark_seen("srv-presence", "Bob");
        mark_seen("srv-presence", "amy");
        mark_seen("", "nobody");
        assert_eq!(present("srv-presence", "bob"), ["@amy"]);
        assert_eq!(present("srv-presence", ""), ["@amy", "@bob"]);
        assert!(present("other", "").is_empty());
    }
    use relay_proto::DeviceId;

    fn grant(role: Role) -> Grant {
        Grant {
            v: 1,
            kid: 1,
            user: "u".into(),
            handle: "chico".into(),
            device: DeviceId([1; 16]),
            team: "t".into(),
            team_version: 1,
            role,
            host: DeviceId([2; 16]),
            servers: vec!["a".into()],
            iat: 0,
            exp: 1,
        }
    }

    #[test]
    fn roles_become_web_panel_accounts_never_admin() {
        assert_eq!(account_of(&grant(Role::Viewer), vec![], None, &[]).role, web::auth::VIEWER);
        for role in [Role::Developer, Role::Admin, Role::Owner] {
            let account = account_of(&grant(role), vec!["a".into()], None, &[]);
            assert_eq!(account.role, web::auth::MANAGER);
            assert!(!account.is_admin());
            assert_eq!(account.name, "@chico");
        }
    }

    #[test]
    fn the_host_can_give_a_teammate_its_own_role() {
        let custom = vec![web::auth::CustomRole { id: "role:x".into(), name: "Builder".into(), perms: vec!["studio.read".into(), "studio.write".into()] }];
        let account = account_of(&grant(Role::Developer), vec!["a".into()], Some("role:x"), &custom);
        assert_eq!(account.role, "role:x");
        let perms = web::auth::perms_of(&account.role, &custom);
        assert!(web::api::permits(&perms, "studio_save"));
        assert!(!web::api::permits(&perms, "send_console_command"));
        assert_eq!(account_of(&grant(Role::Viewer), vec![], Some("Manager"), &custom).role, web::auth::MANAGER, "the host may widen on its own servers");
        assert_eq!(account_of(&grant(Role::Owner), vec![], Some("Admin"), &custom).role, web::auth::MANAGER, "never Admin");
        assert_eq!(account_of(&grant(Role::Viewer), vec![], Some("role:gone"), &custom).role, web::auth::VIEWER, "a deleted role falls back");
        // Team-level refusals still apply whatever the role here.
        assert!(role_refusal(Role::Developer, "restore_backup", &json!({}), false).is_some());
    }

    #[test]
    fn events_are_kept_to_the_granted_servers() {
        let text = json!({ "at": 9, "events": [
            { "event": "server-status", "payload": { "serverId": "a", "status": "stopped" } },
            { "event": "server-status", "payload": { "serverId": "b", "status": "stopped" } },
            { "event": "log:batch", "payload": [{ "serverId": "a", "line": "x" }, { "serverId": "b", "line": "y" }] },
            { "event": "log:batch", "payload": [{ "serverId": "b", "line": "z" }] },
            { "event": "app-thing", "payload": { "other": 1 } },
        ] })
        .to_string();
        let kept = web::api::events_for(&text, &["a".to_string()]);
        let events = kept["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["payload"]["serverId"], "a");
        assert_eq!(events[1]["payload"].as_array().unwrap().len(), 1);
        assert_eq!(kept["at"], 9);
    }

    #[test]
    fn key_lists_skip_what_is_malformed() {
        let key = relay_proto::hex(&[3u8; 32]);
        assert_eq!(parse_keys(&format!("2:{key}, bad, 3:zz, 4:{}", &key[2..])), vec![(2, [3u8; 32])]);
        assert!(parse_keys("").is_empty());
    }
}
