use std::path::Path;

use serde::Serialize;
use tauri::AppHandle;

use crate::ServerInstance;

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitFile {
    pub path: String,
    /// Changed, Added, Deleted, Renamed, Untracked or Conflict.
    pub state: String,
    /// The old path of a renamed file.
    pub from: String,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub repo: bool,
    pub branch: String,
    pub upstream: String,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFile>,
    pub has_ignore: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub at: u64,
    pub subject: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitDiff {
    pub before: String,
    pub after: String,
    pub binary: bool,
}

const TEXT_LIMIT: usize = 2 * 1024 * 1024;

pub const DEFAULT_IGNORE: &str = "# Large or generated files that don't belong in git\nworld/\nworld_nether/\nworld_the_end/\nlogs/\ncache/\nlibraries/\nversions/\ncrash-reports/\n.monolab-trash/\n*.jar\n*.log\n*.gz\nsession.lock\n";

struct Ran {
    code: i32,
    out: String,
    err: String,
}

impl Ran {
    fn ok(self) -> Result<String, String> {
        if self.code == 0 {
            return Ok(self.out);
        }
        let why = if self.err.trim().is_empty() { self.out.trim().to_string() } else { self.err.trim().to_string() };
        Err(format!("Git failed: {why}"))
    }
}

fn run_local(path: &str, args: &[String]) -> Result<Ran, String> {
    let done = crate::hidden_command("git")
        .args(["-c", "core.quotepath=false", "-C", path])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "Git isn't installed on this computer. Install Git for Windows, then try again.".to_string()
            } else {
                error.to_string()
            }
        })?;
    Ok(Ran {
        code: done.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&done.stdout).into_owned(),
        err: String::from_utf8_lossy(&done.stderr).into_owned(),
    })
}

/// Runs git in the server folder, on this PC or on the SSH host. Panel servers have no shell, so no git.
async fn git(app: &AppHandle, instance: &ServerInstance, args: &[&str], long: bool) -> Result<Ran, String> {
    if !instance.panel_host.is_empty() {
        return Err("Git isn't available for panel servers.".into());
    }
    if instance.remote_host.is_empty() {
        let (path, args): (String, Vec<String>) = (instance.path.clone(), args.iter().map(|one| one.to_string()).collect());
        return tauri::async_runtime::spawn_blocking(move || run_local(&path, &args)).await.map_err(|error| error.to_string())?;
    }
    let q = crate::remote::quote;
    let line = format!(
        "GIT_TERMINAL_PROMPT=0 git -c core.quotepath=false -C {} {}",
        q(&instance.path),
        args.iter().map(|one| q(one)).collect::<Vec<_>>().join(" ")
    );
    let conn = crate::remote::connect(app, &instance.remote_host).await?;
    let done = if long { conn.run_long(&line).await? } else { conn.run(&line).await? };
    if done.code == 127 {
        return Err("Git isn't installed on the SSH server.".into());
    }
    Ok(Ran { code: done.code, out: done.stdout, err: done.stderr })
}

/// Whether the server folder itself is a git repository (a .git folder or file at its top).
pub async fn present(app: &AppHandle, instance: &ServerInstance) -> Result<bool, String> {
    if !instance.panel_host.is_empty() {
        return Ok(false);
    }
    if instance.remote_host.is_empty() {
        return Ok(Path::new(&instance.path).join(".git").exists());
    }
    let conn = crate::remote::connect(app, &instance.remote_host).await?;
    let done = conn.run(&format!("test -e {}", crate::remote::quote(&format!("{}/.git", instance.path.trim_end_matches('/'))))).await?;
    Ok(done.code == 0)
}

/// Whether git can run for this server: on this PC, or on the SSH host.
pub async fn installed(app: &AppHandle, instance: &ServerInstance) -> bool {
    if !instance.panel_host.is_empty() {
        return false;
    }
    if instance.remote_host.is_empty() {
        return tauri::async_runtime::spawn_blocking(|| crate::hidden_command("git").arg("--version").output().is_ok_and(|done| done.status.success()))
            .await
            .unwrap_or(false);
    }
    match crate::remote::connect(app, &instance.remote_host).await {
        Ok(conn) => conn.run("command -v git").await.is_ok_and(|done| done.code == 0),
        Err(_) => false,
    }
}

/// Starts a repository in the server folder, with the default .gitignore when there is none.
pub async fn init(app: &AppHandle, instance: &ServerInstance) -> Result<(), String> {
    if present(app, instance).await? {
        return Err("This server is already a git repository.".into());
    }
    git(app, instance, &["init"], false).await?.ok()?;
    if instance.remote_host.is_empty() {
        let path = Path::new(&instance.path).join(".gitignore");
        if !path.exists() {
            std::fs::write(&path, DEFAULT_IGNORE).map_err(|error| format!("Could not create .gitignore: {error}"))?;
        }
    } else if crate::remote::read_text(app, &instance.remote_host, &instance.path, ".gitignore").await.is_err() {
        crate::remote::write_text(app, &instance.remote_host, &instance.path, ".gitignore", DEFAULT_IGNORE).await?;
    }
    Ok(())
}

fn state_of(code: &str) -> &'static str {
    let mut letters = code.chars();
    let (x, y) = (letters.next().unwrap_or(' '), letters.next().unwrap_or(' '));
    if code == "??" {
        return "Untracked";
    }
    if x == 'U' || y == 'U' || code == "AA" || code == "DD" {
        return "Conflict";
    }
    if x == 'R' || y == 'R' {
        return "Renamed";
    }
    if x == 'D' || y == 'D' {
        return "Deleted";
    }
    if x == 'A' {
        return "Added";
    }
    "Changed"
}

/// Parses `git status --porcelain=v1 -b -z`.
pub fn parse_status(text: &str) -> (String, String, u32, u32, Vec<GitFile>) {
    let mut parts = text.split('\0').filter(|one| !one.is_empty()).peekable();
    let (mut branch, mut upstream, mut ahead, mut behind) = (String::new(), String::new(), 0, 0);
    let mut files = Vec::new();
    while let Some(entry) = parts.next() {
        if let Some(head) = entry.strip_prefix("## ") {
            let (names, counts) = head.split_once(" [").map(|(a, b)| (a, b.trim_end_matches(']'))).unwrap_or((head, ""));
            let (local, remote) = names.split_once("...").unwrap_or((names, ""));
            branch = local.trim_start_matches("No commits yet on ").to_string();
            upstream = remote.to_string();
            for part in counts.split(", ") {
                if let Some(n) = part.strip_prefix("ahead ") {
                    ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix("behind ") {
                    behind = n.parse().unwrap_or(0);
                }
            }
            continue;
        }
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(2);
        let state = state_of(code);
        let from = if state == "Renamed" { parts.next().unwrap_or_default().to_string() } else { String::new() };
        files.push(GitFile { path: path[1..].to_string(), state: state.into(), from });
    }
    (branch, upstream, ahead, behind, files)
}

pub async fn status(app: &AppHandle, instance: &ServerInstance) -> Result<GitStatus, String> {
    if !present(app, instance).await? {
        return Ok(GitStatus::default());
    }
    let text = git(app, instance, &["status", "--porcelain=v1", "-b", "-z"], false).await?.ok()?;
    let (branch, upstream, ahead, behind, files) = parse_status(&text);
    let has_ignore = git(app, instance, &["ls-files", "--error-unmatch", ".gitignore"], false).await.is_ok_and(|ran| ran.code == 0)
        || files.iter().any(|one| one.path == ".gitignore");
    Ok(GitStatus { repo: true, branch, upstream, ahead, behind, files, has_ignore })
}

/// Parses `git log` with unit and record separators.
pub fn parse_log(text: &str) -> Vec<GitCommit> {
    text.split('\u{1e}')
        .filter_map(|record| {
            let fields: Vec<&str> = record.trim_matches(['\n', '\r']).split('\u{1f}').collect();
            if fields.len() < 5 {
                return None;
            }
            Some(GitCommit {
                hash: fields[0].to_string(),
                short: fields[1].to_string(),
                author: fields[2].to_string(),
                at: fields[3].parse().unwrap_or(0),
                subject: fields[4].to_string(),
            })
        })
        .collect()
}

pub async fn log(app: &AppHandle, instance: &ServerInstance) -> Result<Vec<GitCommit>, String> {
    let ran = git(app, instance, &["log", "-n", "100", "--format=%H%x1f%h%x1f%an%x1f%at%x1f%s%x1e"], false).await?;
    // A repository without commits has no log yet.
    if ran.code != 0 && ran.err.contains("does not have any commits") {
        return Ok(Vec::new());
    }
    Ok(parse_log(&ran.ok()?))
}

fn checked_hash(hash: &str) -> Result<&str, String> {
    if hash.is_empty() || hash.len() > 64 || !hash.chars().all(|one| one.is_ascii_hexdigit()) {
        return Err("That commit wasn't found.".into());
    }
    Ok(hash)
}

/// Parses `git show --name-status --format= -z`.
pub fn parse_name_status(text: &str) -> Vec<GitFile> {
    let mut parts = text.split('\0').map(|one| one.trim_start_matches('\n')).filter(|one| !one.is_empty());
    let mut files = Vec::new();
    while let Some(code) = parts.next() {
        let (state, from) = match code.chars().next() {
            Some('A') => ("Added", String::new()),
            Some('D') => ("Deleted", String::new()),
            Some('R') | Some('C') => ("Renamed", parts.next().unwrap_or_default().to_string()),
            _ => ("Changed", String::new()),
        };
        if let Some(path) = parts.next() {
            files.push(GitFile { path: path.to_string(), state: state.into(), from });
        }
    }
    files
}

pub async fn commit_files(app: &AppHandle, instance: &ServerInstance, hash: &str) -> Result<Vec<GitFile>, String> {
    let hash = checked_hash(hash)?;
    let text = git(app, instance, &["show", "--name-status", "--format=", "-z", hash], false).await?.ok()?;
    Ok(parse_name_status(&text))
}

fn checked_paths(paths: &[String]) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("Choose at least one file.".into());
    }
    paths
        .iter()
        .map(|one| {
            crate::paths::check_relative(one)
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .map_err(|_| format!("Not a path inside the server: {one}"))
        })
        .collect()
}

async fn show(app: &AppHandle, instance: &ServerInstance, spec: &str) -> Result<Option<String>, String> {
    let ran = git(app, instance, &["show", spec], false).await?;
    Ok((ran.code == 0).then_some(ran.out))
}

pub async fn diff(app: &AppHandle, instance: &ServerInstance, path: &str, hash: &str) -> Result<GitDiff, String> {
    let path = checked_paths(&[path.to_string()])?.remove(0);
    let (before, after) = if hash.is_empty() {
        let before = show(app, instance, &format!("HEAD:{path}")).await?.unwrap_or_default();
        let after = if instance.remote_host.is_empty() {
            let file = crate::paths::resolve_in_server(Path::new(&instance.path), &path).ok();
            file.and_then(|one| std::fs::read(one).ok()).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default()
        } else {
            crate::remote::read_text(app, &instance.remote_host, &instance.path, &path).await.unwrap_or_default()
        };
        (before, after)
    } else {
        let hash = checked_hash(hash)?;
        let before = show(app, instance, &format!("{hash}^:{path}")).await?.unwrap_or_default();
        let after = show(app, instance, &format!("{hash}:{path}")).await?.unwrap_or_default();
        (before, after)
    };
    let binary = before.len() > TEXT_LIMIT || after.len() > TEXT_LIMIT || crate::looks_binary(&before) || crate::looks_binary(&after);
    if binary {
        return Ok(GitDiff { before: String::new(), after: String::new(), binary: true });
    }
    Ok(GitDiff { before, after, binary: false })
}

/// A unified diff of one file: the working copy against the last commit, or what a commit changed.
pub async fn patch(app: &AppHandle, instance: &ServerInstance, path: &str, hash: &str) -> Result<String, String> {
    const LIMIT: usize = 20_000;
    let path = checked_paths(&[path.to_string()])?.remove(0);
    let text = if hash.is_empty() {
        git(app, instance, &["diff", "HEAD", "--", &path], false).await?.ok()?
    } else {
        git(app, instance, &["show", "--format=", checked_hash(hash)?, "--", &path], false).await?.ok()?
    };
    if text.trim().is_empty() {
        return Ok("No changes in this file (a new untracked file has no diff until it is added).".into());
    }
    if text.len() > LIMIT {
        let mut cut = LIMIT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        return Ok(format!("{}
… (diff cut at {LIMIT} characters)", &text[..cut]));
    }
    Ok(text)
}

pub async fn commit(app: &AppHandle, instance: &ServerInstance, paths: &[String], message: &str) -> Result<String, String> {
    let paths = checked_paths(paths)?;
    let message = message.trim();
    if message.is_empty() {
        return Err("Write a commit message.".into());
    }
    // A staged rename's old path is gone from disk and index, so it only goes into the commit, not into add.
    let listed = git(app, instance, &["status", "--porcelain=v1", "-z"], false).await?.ok()?;
    let origins: Vec<String> = parse_status(&listed).4.into_iter().filter(|one| !one.from.is_empty() && paths.contains(&one.path)).map(|one| one.from).collect();
    let mut add: Vec<&str> = vec!["add", "-A", "--"];
    add.extend(paths.iter().map(String::as_str));
    git(app, instance, &add, true).await?.ok()?;
    let mut commit: Vec<&str> = vec!["commit", "-m", message, "--"];
    commit.extend(paths.iter().chain(origins.iter()).map(String::as_str));
    git(app, instance, &commit, true).await?.ok()?;
    Ok(git(app, instance, &["rev-parse", "--short", "HEAD"], false).await?.ok()?.trim().to_string())
}

/// Puts tracked files back to the last commit. The caller backs them up first.
pub async fn restore(app: &AppHandle, instance: &ServerInstance, paths: &[String]) -> Result<(), String> {
    let paths = checked_paths(paths)?;
    let mut args: Vec<&str> = vec!["checkout", "HEAD", "--"];
    args.extend(paths.iter().map(String::as_str));
    git(app, instance, &args, true).await?.ok().map(|_| ())
}

pub async fn sync(app: &AppHandle, instance: &ServerInstance, push: bool) -> Result<String, String> {
    let args: &[&str] = if push { &["push"] } else { &["pull", "--ff-only"] };
    let ran = git(app, instance, args, true).await?;
    let said = format!("{}{}", ran.out.trim(), if ran.err.trim().is_empty() { String::new() } else { format!("\n{}", ran.err.trim()) });
    if ran.code != 0 {
        return Err(format!("Git failed: {}", said.trim()));
    }
    Ok(said.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) -> Ran {
        let mut all = vec!["-c".to_string(), "user.name=t".into(), "-c".into(), "user.email=t@t".into()];
        all.extend(args.iter().map(|one| one.to_string()));
        run_local(&dir.to_string_lossy(), &all).expect("git runs")
    }

    #[test]
    fn the_app_git_steps_work_on_a_real_repository() {
        if run_local(".", &["--version".to_string()]).is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("monolab-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(run(&dir, &["init", "-q"]).code, 0);
        std::fs::write(dir.join("server.properties"), "view-distance=10
").unwrap();
        std::fs::write(dir.join("a.txt"), "a
").unwrap();
        assert_eq!(run(&dir, &["add", "-A", "--", "server.properties", "a.txt"]).code, 0);
        assert_eq!(run(&dir, &["commit", "-q", "-m", "First", "--", "server.properties", "a.txt"]).code, 0);

        std::fs::write(dir.join("server.properties"), "view-distance=12
").unwrap();
        assert_eq!(run(&dir, &["mv", "a.txt", "b.txt"]).code, 0);
        std::fs::write(dir.join("new.yml"), "x: 1
").unwrap();
        let (_, _, _, _, files) = parse_status(&run(&dir, &["status", "--porcelain=v1", "-b", "-z"]).out);
        let renamed = files.iter().find(|one| one.state == "Renamed").expect("rename listed");
        assert_eq!((renamed.path.as_str(), renamed.from.as_str()), ("b.txt", "a.txt"));
        assert!(files.iter().any(|one| one.path == "new.yml" && one.state == "Untracked"));

        // Commit the rename and the new file the way commit() does: add the picked paths, commit them plus rename origins.
        assert_eq!(run(&dir, &["add", "-A", "--", "b.txt", "new.yml"]).code, 0);
        assert_eq!(run(&dir, &["commit", "-q", "-m", "Second", "--", "b.txt", "new.yml", "a.txt"]).code, 0);
        let (_, _, _, _, left) = parse_status(&run(&dir, &["status", "--porcelain=v1", "-b", "-z"]).out);
        assert_eq!(left.iter().map(|one| one.path.as_str()).collect::<Vec<_>>(), vec!["server.properties"]);

        let log = parse_log(&run(&dir, &["log", "-n", "100", "--format=%H%x1f%h%x1f%an%x1f%at%x1f%s%x1e"]).out);
        assert_eq!(log.iter().map(|one| one.subject.as_str()).collect::<Vec<_>>(), vec!["Second", "First"]);
        let changed = parse_name_status(&run(&dir, &["show", "--name-status", "--format=", "-z", &log[0].hash]).out);
        assert!(changed.iter().any(|one| one.path == "b.txt" && one.state == "Renamed"));

        assert_eq!(run(&dir, &["show", "HEAD:server.properties"]).out.trim(), "view-distance=10");
        assert_eq!(run(&dir, &["checkout", "HEAD", "--", "server.properties"]).code, 0);
        assert_eq!(std::fs::read_to_string(dir.join("server.properties")).unwrap().trim(), "view-distance=10");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_status_branch_counts_and_files() {
        let text = "## main...origin/main [ahead 2, behind 1]\0 M server.properties\0?? plugins/new.yml\0R  b.txt\0a.txt\0D  old.json\0UU ops.json\0";
        let (branch, upstream, ahead, behind, files) = parse_status(text);
        assert_eq!((branch.as_str(), upstream.as_str(), ahead, behind), ("main", "origin/main", 2, 1));
        let got: Vec<(&str, &str)> = files.iter().map(|one| (one.path.as_str(), one.state.as_str())).collect();
        assert_eq!(got, vec![("server.properties", "Changed"), ("plugins/new.yml", "Untracked"), ("b.txt", "Renamed"), ("old.json", "Deleted"), ("ops.json", "Conflict")]);
    }

    #[test]
    fn reads_a_fresh_repository_and_log_and_commit_files() {
        let (branch, upstream, ..) = parse_status("## No commits yet on main\0?? a.txt\0");
        assert_eq!((branch.as_str(), upstream.as_str()), ("main", ""));
        let log = parse_log("abc\u{1f}ab\u{1f}Kim\u{1f}1790000000\u{1f}Raise view distance\u{1e}\ndef\u{1f}de\u{1f}Lee\u{1f}1780000000\u{1f}First\u{1e}");
        assert_eq!(log.len(), 2);
        assert_eq!((log[0].short.as_str(), log[0].at, log[1].subject.as_str()), ("ab", 1790000000, "First"));
        let files = parse_name_status("M\0server.properties\0A\0plugins/x.yml\0R100\0a.txt\0b.txt\0");
        let got: Vec<(&str, &str)> = files.iter().map(|one| (one.path.as_str(), one.state.as_str())).collect();
        assert_eq!(got, vec![("server.properties", "Changed"), ("plugins/x.yml", "Added"), ("b.txt", "Renamed")]);
    }
}
