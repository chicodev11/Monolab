//! Git for servers, with the `git` program the user has installed:
//! - **History**: the server folder as a git repository (configs, plugin settings, scripts; never worlds,
//!   logs or jars), with changes, commits, diffs, putting a file back, and push/pull to a remote.
//! - **Dev repos**: a plugin/mod source repository linked to a server. "Build" pulls it, runs its build
//!   (Gradle or Maven) and drops the newest jar into the server's plugins/mods folder.
//!
//! History works for servers on this PC and on SSH servers (git runs on that machine, over the app's SSH
//! connection). Dev repos are cloned and built on this PC, then the jar goes to the server either way.
//! Sign-in for remotes is git's own (Git Credential Manager, SSH keys) on whichever machine runs git.

use crate::data_root::MonolabDirs;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::{hidden_command, instance_by_id, AppState};

/// What a fresh history leaves out: worlds (added separately by name), logs, caches, jars and the app's own folders.
const IGNORE: &str = "# Made by Monolab: what stays out of this server's history.
# Worlds, logs and downloads are big or change all the time; backups cover worlds.
logs/
crash-reports/
cache/
libraries/
versions/
.paper-remapped/
.fabric/
.monolab-backups/
.monolab-trash/
.monolab-pending/
*.jar
*.log
*.log.gz
*.lck
*.lock
*.tmp
session.lock
usercache.json
bluemap/
dynmap/
plugins/*/web/
";

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    pub path: String,
    /// `added`, `changed`, `deleted`, `renamed` or `new` (not tracked yet).
    pub state: String,
}

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitState {
    /// The `git` program was found.
    pub installed: bool,
    /// The server folder is a git repository.
    pub repo: bool,
    pub branch: String,
    pub remote: String,
    pub changes: Vec<GitChange>,
    /// Commits not pushed yet / commits on the remote not pulled yet (when a remote is set and known).
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    pub short: String,
    pub author: String,
    /// Seconds since 1970.
    pub at: u64,
    pub message: String,
    pub files: u32,
}

#[derive(Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DevRepo {
    pub id: String,
    pub server_id: String,
    pub url: String,
    /// Where it is cloned, under the app's data folder.
    pub dir: String,
    /// `gradle`, `maven` or `custom`.
    pub build: String,
    /// The command for `custom` builds (run in the repository folder).
    #[serde(default)]
    pub command: String,
    /// Where built jars land, relative to the repository (empty: build/libs or target).
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub last_build: u64,
    #[serde(default)]
    pub last_jar: String,
}

fn git(dir: &Path, args: &[&str]) -> Result<Output, String> {
    hidden_command("git")
        .args(["-c", "core.quotepath=off", "-c", "color.ui=never"])
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| if error.kind() == std::io::ErrorKind::NotFound { "Git isn't installed. Get it from git-scm.com, then try again.".to_string() } else { format!("Couldn't run git: {error}") })
}

/// Runs git on this PC and gives its output, or its error text.
fn run(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(dir, args)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("git {} failed.", args.first().unwrap_or(&"")) } else { err })
    }
}

/// Where a server's folder is, and so where git runs.
enum Place {
    Here(PathBuf),
    Ssh { app: AppHandle, host: String, folder: String },
}

/// A finished git run: whether it worked, its output and its error text.
struct Ran {
    ok: bool,
    out: String,
    err: String,
}

impl Place {
    fn of(app: &AppHandle, id: &str) -> Result<Self, String> {
        let instance = instance_by_id(app, id)?;
        if !instance.panel_host.is_empty() {
            return Err("Git works for servers on this PC and SSH servers, not panel servers.".into());
        }
        if !instance.remote_host.is_empty() {
            return Ok(Place::Ssh { app: app.clone(), host: instance.remote_host, folder: instance.path.trim_end_matches('/').to_string() });
        }
        Ok(Place::Here(PathBuf::from(instance.path)))
    }

    /// Runs a shell command on the SSH server.
    fn shell(app: &AppHandle, host: &str, command: &str, long: bool) -> Result<crate::remote::Output, String> {
        tauri::async_runtime::block_on(async {
            let conn = crate::remote::connect(app, host).await?;
            if long { conn.run_long(command).await } else { conn.run(command).await }
        })
    }

    fn ssh_git(folder: &str, args: &[&str]) -> String {
        let quoted: Vec<String> = args.iter().map(|one| crate::remote::quote(one)).collect();
        format!(
            "cd {} && GIT_TERMINAL_PROMPT=0 git -c core.quotepath=off -c color.ui=never {}",
            crate::remote::quote(folder),
            quoted.join(" ")
        )
    }

    fn git_ran(&self, args: &[&str], long: bool) -> Result<Ran, String> {
        match self {
            Place::Here(dir) => {
                let out = git(dir, args)?;
                Ok(Ran { ok: out.status.success(), out: String::from_utf8_lossy(&out.stdout).into_owned(), err: String::from_utf8_lossy(&out.stderr).into_owned() })
            }
            Place::Ssh { app, host, folder } => {
                let out = Self::shell(app, host, &Self::ssh_git(folder, args), long)?;
                if out.code == 127 {
                    return Err("Git isn't installed on the SSH server. Install it there (for example `sudo apt install git`).".into());
                }
                Ok(Ran { ok: out.code == 0, out: out.stdout, err: out.stderr })
            }
        }
    }

    /// Runs git and gives its output, or its error text.
    fn run(&self, args: &[&str]) -> Result<String, String> {
        self.run_for(args, false)
    }

    fn run_for(&self, args: &[&str], long: bool) -> Result<String, String> {
        let ran = self.git_ran(args, long)?;
        if ran.ok {
            Ok(ran.out)
        } else {
            let err = ran.err.trim().to_string();
            Err(if err.is_empty() { format!("git {} failed.", args.first().unwrap_or(&"")) } else { err })
        }
    }

    fn installed(&self) -> bool {
        match self {
            Place::Here(_) => installed(),
            Place::Ssh { app, host, .. } => Self::shell(app, host, "git --version", false).is_ok_and(|out| out.code == 0),
        }
    }

    /// Whether something exists at a path inside the server folder.
    fn has(&self, relative: &str) -> bool {
        match self {
            Place::Here(dir) => dir.join(relative).exists(),
            Place::Ssh { app, host, folder } => {
                let at = format!("{folder}/{relative}");
                Self::shell(app, host, &format!("test -e {}", crate::remote::quote(&at)), false).is_ok_and(|out| out.code == 0)
            }
        }
    }

    fn write(&self, relative: &str, text: &str) -> Result<(), String> {
        match self {
            Place::Here(dir) => fs::write(dir.join(relative), text).map_err(|error| format!("Couldn't write {relative}: {error}")),
            Place::Ssh { app, host, folder } => {
                let at = format!("{folder}/{relative}");
                let command = format!("printf %s {} > {}", crate::remote::quote(text), crate::remote::quote(&at));
                let out = Self::shell(app, host, &command, false)?;
                if out.code == 0 { Ok(()) } else { Err(format!("Couldn't write {relative}: {}", out.stderr.trim())) }
            }
        }
    }

    /// Folders holding a `level.dat` at the top of the server folder: worlds, whatever their names.
    fn worlds(&self) -> Vec<String> {
        match self {
            Place::Here(dir) => world_folders(dir),
            Place::Ssh { app, host, folder } => {
                let command = format!("cd {} && for d in */; do [ -f \"$d/level.dat\" ] && printf '%s\\n' \"${{d%/}}\"; done; true", crate::remote::quote(folder));
                Self::shell(app, host, &command, false)
                    .map(|out| out.stdout.lines().map(str::trim).filter(|one| !one.is_empty()).map(str::to_string).collect())
                    .unwrap_or_default()
            }
        }
    }
}

fn installed() -> bool {
    hidden_command("git").arg("--version").output().is_ok_and(|out| out.status.success())
}

/// One line of `git status --porcelain=v1 -z` split into entries.
fn parse_status(raw: &str) -> Vec<GitChange> {
    let mut out = Vec::new();
    let mut parts = raw.split('\0').filter(|one| !one.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        let code = code.trim();
        let state = match code {
            "??" => "new",
            c if c.contains('R') => {
                // A rename is followed by the old path.
                parts.next();
                "renamed"
            }
            c if c.contains('D') => "deleted",
            c if c.contains('A') => "added",
            _ => "changed",
        };
        out.push(GitChange { path: path.to_string(), state: state.into() });
    }
    out
}

/// Folders holding a `level.dat` at the top of a folder on this PC.
fn world_folders(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().join("level.dat").is_file())
                .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Who commits: git's own name when set, else the app.
fn author_args(place: &Place) -> Vec<String> {
    let named = place.run(&["config", "user.name"]).is_ok_and(|name| !name.trim().is_empty());
    let mailed = place.run(&["config", "user.email"]).is_ok_and(|mail| !mail.trim().is_empty());
    let mut args = Vec::new();
    if !named {
        args.extend(["-c".to_string(), "user.name=Monolab".to_string()]);
    }
    if !mailed {
        args.extend(["-c".to_string(), "user.email=monolab@localhost".to_string()]);
    }
    args
}

fn commit_all(place: &Place, message: &str) -> Result<bool, String> {
    place.run(&["add", "-A"])?;
    if place.git_ran(&["diff", "--cached", "--quiet"], false)?.ok {
        return Ok(false);
    }
    let mut args = author_args(place);
    args.extend(["commit".into(), "-q".into(), "-m".into(), message.into()]);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    place.run(&refs)?;
    Ok(true)
}

// ── history ────────────────────────────────────────────────────────────────

#[tauri::command(async)]
#[specta::specta]
pub fn git_state(app: AppHandle, id: String) -> Result<GitState, String> {
    let place = Place::of(&app, &id)?;
    let empty = |installed: bool| GitState { installed, repo: false, branch: String::new(), remote: String::new(), changes: vec![], ahead: 0, behind: 0 };
    if !place.installed() {
        return Ok(empty(false));
    }
    if !place.has(".git") {
        return Ok(empty(true));
    }
    let branch = place.run(&["rev-parse", "--abbrev-ref", "HEAD"]).map(|one| one.trim().to_string()).unwrap_or_default();
    let remote = place.run(&["remote", "get-url", "origin"]).map(|one| one.trim().to_string()).unwrap_or_default();
    let changes = parse_status(&place.run(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?);
    let (mut ahead, mut behind) = (0, 0);
    if !remote.is_empty() {
        if let Ok(counts) = place.run(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) {
            let mut parts = counts.split_whitespace().filter_map(|one| one.parse::<u32>().ok());
            ahead = parts.next().unwrap_or(0);
            behind = parts.next().unwrap_or(0);
        }
    }
    Ok(GitState { installed: true, repo: true, branch, remote, changes, ahead, behind })
}

/// Starts a history in the server folder: a .gitignore that leaves out worlds, logs and jars, then a first commit.
#[tauri::command(async)]
#[specta::specta]
pub fn git_start(app: AppHandle, id: String) -> Result<GitState, String> {
    let place = Place::of(&app, &id)?;
    if !place.has(".git") {
        place.run(&["init", "-q"])?;
        // `init -b` needs git 2.28; this works on older servers too.
        let _ = place.run(&["symbolic-ref", "HEAD", "refs/heads/main"]);
    }
    if !place.has(".gitignore") {
        let mut text = IGNORE.to_string();
        let worlds = place.worlds();
        if !worlds.is_empty() {
            text.push_str("\n# Worlds\n");
            for world in worlds {
                text.push_str(&format!("{world}/\n"));
            }
        }
        place.write(".gitignore", &text)?;
    }
    commit_all(&place, "Start history (Monolab)")?;
    git_state(app, id)
}

#[tauri::command(async)]
#[specta::specta]
pub fn git_commit(app: AppHandle, id: String, message: String) -> Result<bool, String> {
    let place = Place::of(&app, &id)?;
    let message = message.trim();
    commit_all(&place, if message.is_empty() { "Changes" } else { message })
}

#[tauri::command(async)]
#[specta::specta]
pub fn git_log(app: AppHandle, id: String, most: u32) -> Result<Vec<GitCommit>, String> {
    let place = Place::of(&app, &id)?;
    if !place.has(".git") || place.run(&["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
        return Ok(vec![]);
    }
    let most = most.clamp(1, 500).to_string();
    let raw = place.run(&["log", "-n", &most, "--format=%x1e%H%x1f%h%x1f%an%x1f%at%x1f%s", "--shortstat"])?;
    Ok(parse_log(&raw))
}

fn parse_log(raw: &str) -> Vec<GitCommit> {
    raw.split('\x1e')
        .filter(|one| !one.trim().is_empty())
        .filter_map(|block| {
            let mut lines = block.lines();
            let head: Vec<&str> = lines.next()?.split('\x1f').collect();
            let files = lines
                .find(|line| line.contains("changed"))
                .and_then(|line| line.split_whitespace().next())
                .and_then(|count| count.parse().ok())
                .unwrap_or(0);
            Some(GitCommit {
                hash: head.first()?.to_string(),
                short: head.get(1)?.to_string(),
                author: head.get(2)?.to_string(),
                at: head.get(3)?.parse().unwrap_or(0),
                message: head.get(4).unwrap_or(&"").to_string(),
                files,
            })
        })
        .collect()
}

fn hash_ok(hash: &str) -> bool {
    (4..=64).contains(&hash.len()) && hash.chars().all(|one| one.is_ascii_hexdigit())
}

/// The diff of one commit (`commit` set) or of what changed since the last commit, for one file or all.
#[tauri::command(async)]
#[specta::specta]
pub fn git_diff(app: AppHandle, id: String, commit: Option<String>, path: Option<String>) -> Result<String, String> {
    let place = Place::of(&app, &id)?;
    let mut args: Vec<String> = match commit.as_deref().filter(|one| !one.is_empty()) {
        Some(hash) if hash_ok(hash) => vec!["show".into(), "--format=".into(), "--no-ext-diff".into(), hash.into()],
        Some(_) => return Err("Not a commit.".into()),
        None => vec!["diff".into(), "--no-ext-diff".into(), "HEAD".into()],
    };
    if let Some(path) = path.filter(|one| !one.is_empty()) {
        crate::paths::check_relative(&path).map_err(|_| "That path isn't inside the server folder.".to_string())?;
        args.extend(["--".into(), path]);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut text = place.run(&refs)?;
    const MOST: usize = 400_000;
    if text.len() > MOST {
        let mut cut = MOST;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n… (cut: too long to show)\n");
    }
    Ok(text)
}

/// Puts one file back as it was in a commit (or at the last commit). The current version is committed first
/// when it has changes, so this can be undone from the history too.
#[tauri::command(async)]
#[specta::specta]
pub fn git_restore(app: AppHandle, id: String, path: String, commit: Option<String>) -> Result<(), String> {
    let place = Place::of(&app, &id)?;
    crate::paths::check_relative(&path).map_err(|_| "That path isn't inside the server folder.".to_string())?;
    let source = match commit.as_deref().filter(|one| !one.is_empty()) {
        Some(hash) if hash_ok(hash) => hash.to_string(),
        Some(_) => return Err("Not a commit.".into()),
        None => "HEAD".into(),
    };
    commit_all(&place, &format!("Before putting back {path}"))?;
    // `checkout <commit> -- path` (not `restore`) so older git on servers works too.
    place.run(&["checkout", &source, "--", &path])?;
    Ok(())
}

/// Sets (or clears, with an empty url) the remote everything is pushed to.
#[tauri::command(async)]
#[specta::specta]
pub fn git_set_remote(app: AppHandle, id: String, url: String) -> Result<(), String> {
    let place = Place::of(&app, &id)?;
    let url = url.trim();
    let has = place.run(&["remote", "get-url", "origin"]).is_ok();
    match (url.is_empty(), has) {
        (true, true) => place.run(&["remote", "remove", "origin"]).map(|_| ()),
        (true, false) => Ok(()),
        (false, _) if !remote_ok(url) => Err("Use an https:// or git@ address.".into()),
        (false, true) => place.run(&["remote", "set-url", "origin", url]).map(|_| ()),
        (false, false) => place.run(&["remote", "add", "origin", url]).map(|_| ()),
    }
}

fn remote_ok(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("git@") || url.starts_with("ssh://")) && !url.contains(char::is_whitespace) && !url.starts_with('-')
}

#[tauri::command(async)]
#[specta::specta]
pub fn git_push(app: AppHandle, id: String) -> Result<String, String> {
    let place = Place::of(&app, &id)?;
    let branch = place.run(&["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
    let ran = place.git_ran(&["push", "-u", "origin", &branch], true)?;
    let text = format!("{}{}", ran.out, ran.err).trim().to_string();
    if ran.ok { Ok(text) } else { Err(text) }
}

/// Pulls from the remote (fast-forward only, so nothing is merged behind the user's back).
#[tauri::command(async)]
#[specta::specta]
pub fn git_pull(app: AppHandle, id: String) -> Result<String, String> {
    let place = Place::of(&app, &id)?;
    place.run_for(&["pull", "--ff-only"], true).map(|text| text.trim().to_string())
}

// ── dev repos ──────────────────────────────────────────────────────────────

fn repos_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(dir.join("dev-repos.json"))
}

fn load_repos(app: &AppHandle) -> Result<Vec<DevRepo>, String> {
    Ok(crate::store::read(&repos_path(app)?)?.unwrap_or_default())
}

fn save_repos(app: &AppHandle, repos: &[DevRepo]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(repos).map_err(|error| error.to_string())?;
    crate::store::write(&repos_path(app)?, &text).map_err(|error| format!("Couldn't save the linked repositories: {error}"))
}

/// A folder name from a repository address: `https://github.com/me/cool-plugin.git` → `cool-plugin`.
fn repo_name(url: &str) -> String {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or("repo");
    let name: String = last.trim_end_matches(".git").chars().filter(|one| one.is_ascii_alphanumeric() || matches!(one, '-' | '_' | '.')).collect();
    if name.is_empty() || name.starts_with('.') { "repo".into() } else { name }
}

fn guess_build(dir: &Path) -> &'static str {
    if dir.join("gradlew").exists() || dir.join("gradlew.bat").exists() || dir.join("build.gradle").exists() || dir.join("build.gradle.kts").exists() {
        "gradle"
    } else if dir.join("pom.xml").exists() {
        "maven"
    } else {
        "custom"
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn dev_repos(app: AppHandle, id: String) -> Result<Vec<DevRepo>, String> {
    Ok(load_repos(&app)?.into_iter().filter(|repo| repo.server_id == id).collect())
}

/// Clones a repository for a server and works out how it builds.
#[tauri::command(async)]
#[specta::specta]
pub fn dev_repo_add(app: AppHandle, id: String, url: String) -> Result<DevRepo, String> {
    Place::of(&app, &id)?;
    let url = url.trim().to_string();
    if !remote_ok(&url) {
        return Err("Use an https:// or git@ address.".into());
    }
    let root = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("dev-repos");
    fs::create_dir_all(&root).map_err(|error| format!("Couldn't make the folder: {error}"))?;
    let name = repo_name(&url);
    let mut dir = root.join(&name);
    let mut n = 2;
    while dir.exists() {
        dir = root.join(format!("{name}-{n}"));
        n += 1;
    }
    let target = dir.to_string_lossy().to_string();
    run(&root, &["clone", "-q", "--", &url, &target])?;
    let repo = DevRepo {
        id: format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_nanos()).unwrap_or(0)),
        server_id: id,
        url,
        build: guess_build(&dir).into(),
        dir: target,
        command: String::new(),
        output: String::new(),
        last_build: 0,
        last_jar: String::new(),
    };
    let mut all = load_repos(&app)?;
    all.push(repo.clone());
    save_repos(&app, &all)?;
    Ok(repo)
}

#[tauri::command(async)]
#[specta::specta]
pub fn dev_repo_set(app: AppHandle, repo: String, build: String, command: String, output: String) -> Result<DevRepo, String> {
    if !matches!(build.as_str(), "gradle" | "maven" | "custom") {
        return Err("Pick Gradle, Maven or a command.".into());
    }
    if !output.trim().is_empty() {
        crate::paths::check_relative(output.trim()).map_err(|_| "The jar folder must be inside the repository.".to_string())?;
    }
    let mut all = load_repos(&app)?;
    let one = all.iter_mut().find(|one| one.id == repo).ok_or("That repository isn't linked any more.")?;
    one.build = build;
    one.command = command.trim().to_string();
    one.output = output.trim().to_string();
    let saved = one.clone();
    save_repos(&app, &all)?;
    Ok(saved)
}

/// Unlinks a repository. Its clone is kept, moved aside to `dev-repos/.unlinked/`, in case it had local work.
#[tauri::command(async)]
#[specta::specta]
pub fn dev_repo_remove(app: AppHandle, repo: String) -> Result<(), String> {
    let mut all = load_repos(&app)?;
    let Some(at) = all.iter().position(|one| one.id == repo) else { return Ok(()) };
    let gone = all.remove(at);
    save_repos(&app, &all)?;
    let from = Path::new(&gone.dir);
    if let (true, Some(root), Some(name)) = (from.exists(), from.parent(), from.file_name()) {
        let aside = root.join(".unlinked");
        let _ = fs::create_dir_all(&aside);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0);
        let _ = fs::rename(from, aside.join(format!("{}-{stamp}", name.to_string_lossy())));
    }
    Ok(())
}

/// The newest .jar under a folder (not -sources / -javadoc / -dev / -plain jars).
fn newest_jar(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        if !name.ends_with(".jar") || ["-sources", "-javadoc", "-dev", "-plain"].iter().any(|skip| name.contains(skip)) {
            continue;
        }
        let when = entry.metadata().and_then(|meta| meta.modified()).ok()?;
        if best.as_ref().is_none_or(|(old, _)| when > *old) {
            best = Some((when, path));
        }
    }
    best.map(|(_, path)| path)
}

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BuildResult {
    pub ok: bool,
    /// The jar put into the server (file name), when the build worked.
    pub jar: String,
    /// The end of the build's output.
    pub log: String,
}

/// Pulls, builds and puts the newest jar into the server (queued for the next start if it's running).
#[tauri::command(async)]
#[specta::specta]
pub async fn dev_repo_build(app: AppHandle, state: State<'_, AppState>, repo: String) -> Result<BuildResult, String> {
    let found = load_repos(&app)?.into_iter().find(|one| one.id == repo).ok_or("That repository isn't linked any more.")?;
    let dir = PathBuf::from(&found.dir);
    let mut log = String::new();
    match run(&dir, &["pull", "--ff-only"]) {
        Ok(text) => log.push_str(&format!("$ git pull\n{}\n", text.trim())),
        Err(error) => log.push_str(&format!("$ git pull (skipped)\n{error}\n")),
    }
    let windows = cfg!(windows);
    let (program, args): (String, Vec<String>) = match found.build.as_str() {
        "gradle" if dir.join(if windows { "gradlew.bat" } else { "gradlew" }).exists() => (
            dir.join(if windows { "gradlew.bat" } else { "gradlew" }).to_string_lossy().to_string(),
            vec!["build".into(), "-x".into(), "test".into(), "--console=plain".into()],
        ),
        "gradle" => ("gradle".into(), vec!["build".into(), "-x".into(), "test".into(), "--console=plain".into()]),
        "maven" => (if windows { "mvn.cmd" } else { "mvn" }.into(), vec!["-q".into(), "-DskipTests".into(), "package".into()]),
        _ if found.command.is_empty() => return Err("Set the build command first.".into()),
        _ if windows => ("cmd".into(), vec!["/C".into(), found.command.clone()]),
        _ => ("sh".into(), vec!["-c".into(), found.command.clone()]),
    };
    log.push_str(&format!("$ {} {}\n", Path::new(&program).file_name().map(|one| one.to_string_lossy().to_string()).unwrap_or_default(), args.join(" ")));
    let build_dir = dir.clone();
    let out = tauri::async_runtime::spawn_blocking(move || hidden_command(&program).args(&args).current_dir(&build_dir).output())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("Couldn't start the build: {error}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let tail: Vec<&str> = text.lines().rev().take(80).collect();
    log.push_str(&tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
    if !out.status.success() {
        return Ok(BuildResult { ok: false, jar: String::new(), log });
    }
    let places: Vec<PathBuf> = if found.output.is_empty() {
        vec![dir.join("build").join("libs"), dir.join("target")]
    } else {
        vec![dir.join(&found.output)]
    };
    let Some(jar) = places.iter().find_map(|place| newest_jar(place)) else {
        log.push_str("\nThe build worked but no .jar was found. Set the jar folder.");
        return Ok(BuildResult { ok: false, jar: String::new(), log });
    };
    crate::add_addon_files(app.clone(), state, found.server_id.clone(), vec![jar.to_string_lossy().to_string()]).await?;
    let name = jar.file_name().map(|one| one.to_string_lossy().to_string()).unwrap_or_default();
    let mut all = load_repos(&app)?;
    if let Some(one) = all.iter_mut().find(|one| one.id == repo) {
        one.last_build = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0);
        one.last_jar = name.clone();
    }
    save_repos(&app, &all)?;
    Ok(BuildResult { ok: true, jar: name, log })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines() {
        let raw = " M server.properties\0?? plugins/Foo/config.yml\0R  new.yml\0old.yml\0 D ops.json\0A  bukkit.yml\0";
        let all = parse_status(raw);
        let got: Vec<(&str, &str)> = all.iter().map(|one| (one.path.as_str(), one.state.as_str())).collect();
        assert_eq!(
            got,
            [("server.properties", "changed"), ("plugins/Foo/config.yml", "new"), ("new.yml", "renamed"), ("ops.json", "deleted"), ("bukkit.yml", "added")]
        );
    }

    #[test]
    fn ssh_commands_are_quoted() {
        let line = Place::ssh_git("/home/u/my server", &["commit", "-m", "it's done; rm -rf /"]);
        assert_eq!(line, r#"cd '/home/u/my server' && GIT_TERMINAL_PROMPT=0 git -c core.quotepath=off -c color.ui=never 'commit' '-m' 'it'\''s done; rm -rf /'"#);
    }

    /// A real history on this PC: change, commit, read the status and log back, put the file back.
    /// Uses only the local helpers: anything reaching the SSH code links the window libraries, and a test
    /// exe without the comctl32 v6 manifest then can't start at all.
    #[test]
    fn history_round_trip() {
        if !installed() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("monolab-git-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("world")).unwrap();
        fs::write(dir.join("world").join("level.dat"), b"x").unwrap();
        fs::write(dir.join("server.properties"), "view-distance=10\n").unwrap();
        fs::write(dir.join(".gitignore"), "world/\n").unwrap();
        assert_eq!(world_folders(&dir), ["world"]);
        let who = ["-c", "user.name=t", "-c", "user.email=t@t"];
        let commit = |message: &str| {
            run(&dir, &["add", "-A"]).unwrap();
            let mut args = who.to_vec();
            args.extend(["commit", "-q", "-m", message]);
            run(&dir, &args).unwrap();
        };
        run(&dir, &["init", "-q"]).unwrap();
        commit("first");
        fs::write(dir.join("server.properties"), "view-distance=12\n").unwrap();
        let changes = parse_status(&run(&dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]).unwrap());
        assert_eq!(changes.len(), 1);
        assert_eq!((changes[0].path.as_str(), changes[0].state.as_str()), ("server.properties", "changed"));
        commit("raise view distance");
        let log = parse_log(&run(&dir, &["log", "-n", "10", "--format=%x1e%H%x1f%h%x1f%an%x1f%at%x1f%s", "--shortstat"]).unwrap());
        assert_eq!(log.iter().map(|one| one.message.as_str()).collect::<Vec<_>>(), ["raise view distance", "first"]);
        assert_eq!(log[0].files, 1);
        run(&dir, &["checkout", &log[1].hash, "--", "server.properties"]).unwrap();
        assert_eq!(fs::read_to_string(dir.join("server.properties")).unwrap().replace("\r\n", "\n"), "view-distance=10\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn repo_names_and_remotes() {
        assert_eq!(repo_name("https://github.com/me/cool-plugin.git"), "cool-plugin");
        assert_eq!(repo_name("git@github.com:me/Thing.git"), "Thing");
        assert_eq!(repo_name("https://example.com/"), "example.com");
        assert!(remote_ok("https://github.com/a/b"));
        assert!(remote_ok("git@github.com:a/b.git"));
        assert!(!remote_ok("--upload-pack=evil"));
        assert!(!remote_ok("file:///c:/x"));
        assert!(!remote_ok("https://a b"));
        assert!(hash_ok("abc1234"));
        assert!(!hash_ok("HEAD;rm"));
    }
}
