//! What Monolab offers ACP agents as a client: reading and writing text files (`fs/*`) and running
//! commands in terminals (`terminal/*`). Everything stays inside the agent's folders: its workspace and,
//! for a server on this computer, the server folder (where Rewind keeps every change it makes).

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Text files bigger than this aren't read or written for an agent.
const MAX_TEXT: usize = 8 * 1024 * 1024;
/// Output kept per terminal when the agent doesn't ask for less.
const DEFAULT_OUTPUT: usize = 1024 * 1024;
/// Terminals one agent may keep at a time.
const MAX_TERMINALS: usize = 8;

const OUTSIDE: &str = "Monolab only lets agents reach their workspace and this server's folder.";

/// `path` as a real path inside one of `roots`. `new` allows a file that doesn't exist yet (its folder must).
pub fn resolve(roots: &[PathBuf], path: &str, new: bool) -> Result<PathBuf, String> {
    let wanted = Path::new(path.trim());
    if !wanted.is_absolute() {
        return Err("The agent has to give a full path.".into());
    }
    for root in roots {
        let real = root.canonicalize().map(crate::plain_path).unwrap_or_else(|_| root.clone());
        for base in [root, &real] {
            let Ok(relative) = wanted.strip_prefix(base) else { continue };
            let relative = relative.to_string_lossy().to_string();
            let found =
                if new { crate::paths::resolve_new_in_server(root, &relative) } else { crate::paths::resolve_in_server(root, &relative) };
            return found.map_err(String::from);
        }
    }
    Err(OUTSIDE.into())
}

/// `fs/read_text_file`: `line` is 1-based, `limit` counts lines.
pub fn read_text(roots: &[PathBuf], params: &Value) -> Result<Value, String> {
    let path = resolve(roots, params["path"].as_str().unwrap_or_default(), false)?;
    let size = std::fs::metadata(&path).map_err(|error| error.to_string())?.len();
    if size as usize > MAX_TEXT {
        return Err("That file is too big to read.".into());
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("Could not read the file: {error}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "That isn't a text file.".to_string())?;
    Ok(json!({ "content": lines_of(&text, params["line"].as_u64(), params["limit"].as_u64()) }))
}

fn lines_of(text: &str, line: Option<u64>, limit: Option<u64>) -> String {
    if line.is_none() && limit.is_none() {
        return text.to_string();
    }
    let skip = line.unwrap_or(1).saturating_sub(1) as usize;
    let take = limit.map_or(usize::MAX, |one| one as usize);
    text.split_inclusive('\n').skip(skip).take(take).collect()
}

/// `fs/write_text_file`: makes or replaces the whole file.
pub fn write_text(roots: &[PathBuf], params: &Value) -> Result<Value, String> {
    let content = params["content"].as_str().ok_or("Nothing to write.")?;
    if content.len() > MAX_TEXT {
        return Err("That's too much to write into one file.".into());
    }
    let path = resolve(roots, params["path"].as_str().unwrap_or_default(), true)?;
    if path.is_dir() {
        return Err("That's a folder, not a file.".into());
    }
    std::fs::write(&path, content).map_err(|error| format!("Could not write the file: {error}"))?;
    Ok(Value::Null)
}

// ---------- terminals ----------

#[derive(Default)]
struct Output {
    text: String,
    truncated: bool,
    /// (exit code, signal) once the command has ended.
    exit: Option<(Option<i32>, Option<String>)>,
}

struct Terminal {
    child: Arc<Mutex<Child>>,
    output: Arc<Mutex<Output>>,
}

fn terminals() -> &'static Mutex<HashMap<(String, String), Terminal>> {
    static ALL: OnceLock<Mutex<HashMap<(String, String), Terminal>>> = OnceLock::new();
    ALL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Adds output, keeping only the newest `limit` bytes (cut on a character edge).
fn push_output(output: &mut Output, more: &str, limit: usize) {
    output.text.push_str(more);
    if output.text.len() > limit {
        let mut cut = output.text.len() - limit;
        while !output.text.is_char_boundary(cut) {
            cut += 1;
        }
        output.text.drain(..cut);
        output.truncated = true;
    }
}

/// The program and its arguments. A single line with spaces and no args goes through the shell,
/// as agents often send whole command lines.
fn command_line(command: &str, args: &[String]) -> (String, Vec<String>) {
    let shell = |line: String| {
        if cfg!(windows) {
            ("cmd".to_string(), vec!["/d".to_string(), "/s".to_string(), "/c".to_string(), line])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), line])
        }
    };
    if args.is_empty() && command.trim().contains(' ') {
        return shell(command.trim().to_string());
    }
    if cfg!(windows) {
        // npm, npx, gradlew and the like are .cmd/.bat files that only cmd can start.
        let lower = command.to_ascii_lowercase();
        if !lower.ends_with(".exe") && !Path::new(command).is_absolute() {
            let mut line = vec![command.to_string()];
            line.extend(args.iter().cloned());
            return ("cmd".to_string(), [vec!["/d".to_string(), "/c".to_string()], line].concat());
        }
    }
    (command.to_string(), args.to_vec())
}

fn reader(mut stream: impl Read + Send + 'static, output: Arc<Mutex<Output>>, limit: usize) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        let mut carry: Vec<u8> = Vec::new();
        loop {
            let read = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            carry.extend_from_slice(&buffer[..read]);
            // Hold back a cut-off UTF-8 character until the rest arrives.
            let good = match std::str::from_utf8(&carry) {
                Ok(_) => carry.len(),
                Err(error) if error.error_len().is_none() => error.valid_up_to(),
                Err(_) => carry.len(),
            };
            let text = String::from_utf8_lossy(&carry[..good]).to_string();
            carry.drain(..good);
            push_output(&mut output.lock().expect("terminal poisoned"), &text, limit);
        }
        if !carry.is_empty() {
            push_output(&mut output.lock().expect("terminal poisoned"), &String::from_utf8_lossy(&carry), limit);
        }
    });
}

/// `terminal/create`: starts the command and returns at once with its id.
pub fn create(owner: &str, roots: &[PathBuf], params: &Value) -> Result<Value, String> {
    let command = params["command"].as_str().unwrap_or_default().trim().to_string();
    if command.is_empty() {
        return Err("No command to run.".into());
    }
    if terminals().lock().expect("terminals poisoned").keys().filter(|(who, _)| who == owner).count() >= MAX_TERMINALS {
        return Err("Too many terminals are open. Release one first.".into());
    }
    let args: Vec<String> =
        params["args"].as_array().map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let cwd = match params["cwd"].as_str().filter(|one| !one.trim().is_empty()) {
        Some(dir) => resolve(roots, dir, false)?,
        None => roots.first().cloned().ok_or(OUTSIDE)?,
    };
    if !cwd.is_dir() {
        return Err("The terminal's folder doesn't exist.".into());
    }
    let limit = params["outputByteLimit"].as_u64().map_or(DEFAULT_OUTPUT, |one| (one as usize).clamp(1024, 16 * DEFAULT_OUTPUT));
    let (program, args) = command_line(&command, &args);
    let mut process = crate::hidden_command(&program);
    process.args(&args).current_dir(&cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for pair in params["env"].as_array().into_iter().flatten() {
        if let (Some(name), Some(value)) = (pair["name"].as_str(), pair["value"].as_str()) {
            process.env(name, value);
        }
    }
    let mut child = process.spawn().map_err(|error| format!("Could not run {command}: {error}"))?;
    let output: Arc<Mutex<Output>> = Arc::default();
    if let Some(stream) = child.stdout.take() {
        reader(stream, output.clone(), limit);
    }
    if let Some(stream) = child.stderr.take() {
        reader(stream, output.clone(), limit);
    }
    let child = Arc::new(Mutex::new(child));
    {
        let (child, output) = (child.clone(), output.clone());
        std::thread::spawn(move || loop {
            let status = child.lock().expect("terminal poisoned").try_wait();
            match status {
                Ok(Some(status)) => {
                    output.lock().expect("terminal poisoned").exit = Some((status.code(), signal_of(&status)));
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => {
                    output.lock().expect("terminal poisoned").exit = Some((None, None));
                    break;
                }
            }
        });
    }
    let id = format!("term{}", crate::acp::next_number());
    terminals().lock().expect("terminals poisoned").insert((owner.to_string(), id.clone()), Terminal { child, output });
    Ok(json!({ "terminalId": id }))
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map(|one| one.to_string())
}

#[cfg(not(unix))]
fn signal_of(_: &std::process::ExitStatus) -> Option<String> {
    None
}

fn output_of(owner: &str, params: &Value) -> Result<Arc<Mutex<Output>>, String> {
    let id = params["terminalId"].as_str().unwrap_or_default();
    terminals()
        .lock()
        .expect("terminals poisoned")
        .get(&(owner.to_string(), id.to_string()))
        .map(|one| one.output.clone())
        .ok_or_else(|| "That terminal is gone.".into())
}

fn exit_json(exit: &(Option<i32>, Option<String>)) -> Value {
    json!({ "exitCode": exit.0, "signal": exit.1 })
}

/// `terminal/output`: what it printed so far, and how it ended if it has.
pub fn output(owner: &str, params: &Value) -> Result<Value, String> {
    let output = output_of(owner, params)?;
    let now = output.lock().expect("terminal poisoned");
    let mut answer = json!({ "output": now.text, "truncated": now.truncated });
    if let Some(exit) = &now.exit {
        answer["exitStatus"] = exit_json(exit);
    }
    Ok(answer)
}

/// `terminal/wait_for_exit`: blocks until the command ends (or its terminal is released).
pub fn wait_for_exit(owner: &str, params: &Value) -> Result<Value, String> {
    let output = output_of(owner, params)?;
    let key = (owner.to_string(), params["terminalId"].as_str().unwrap_or_default().to_string());
    loop {
        if let Some(exit) = &output.lock().expect("terminal poisoned").exit {
            return Ok(exit_json(exit));
        }
        if !terminals().lock().expect("terminals poisoned").contains_key(&key) {
            return Err("That terminal was released.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn stop(terminal: &Terminal) {
    let mut child = terminal.child.lock().expect("terminal poisoned");
    if matches!(child.try_wait(), Ok(None)) {
        crate::kill_process_tree(child.id());
        let _ = child.kill();
    }
}

/// `terminal/kill`: ends the command but keeps the terminal so its output can still be read.
pub fn kill(owner: &str, params: &Value) -> Result<Value, String> {
    let id = params["terminalId"].as_str().unwrap_or_default();
    let all = terminals().lock().expect("terminals poisoned");
    let one = all.get(&(owner.to_string(), id.to_string())).ok_or("That terminal is gone.")?;
    stop(one);
    Ok(Value::Null)
}

/// `terminal/release`: ends the command if it still runs and forgets the terminal.
pub fn release(owner: &str, params: &Value) -> Result<Value, String> {
    let id = params["terminalId"].as_str().unwrap_or_default();
    if let Some(one) = terminals().lock().expect("terminals poisoned").remove(&(owner.to_string(), id.to_string())) {
        stop(&one);
    }
    Ok(Value::Null)
}

/// Ends and forgets every terminal of an agent that closed.
pub fn release_all(owner: &str) {
    let gone: Vec<Terminal> = {
        let mut all = terminals().lock().expect("terminals poisoned");
        let keys: Vec<_> = all.keys().filter(|(who, _)| who == owner).cloned().collect();
        keys.into_iter().filter_map(|key| all.remove(&key)).collect()
    };
    for one in &gone {
        stop(one);
    }
}

/// Answers one client-side request from an agent, or `None` when it isn't one of ours.
pub fn handle(owner: &str, roots: &[PathBuf], method: &str, params: &Value) -> Option<Result<Value, String>> {
    Some(match method {
        "fs/read_text_file" => read_text(roots, params),
        "fs/write_text_file" => write_text(roots, params),
        "terminal/create" => create(owner, roots, params),
        "terminal/output" => output(owner, params),
        "terminal/wait_for_exit" => wait_for_exit(owner, params),
        "terminal/kill" => kill(owner, params),
        "terminal/release" => release(owner, params),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("monolab-acp-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("work");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(base.join("outside").join("secret.txt"), "s").unwrap();
        (base, root.canonicalize().map(crate::plain_path).unwrap())
    }

    #[test]
    fn files_stay_inside_the_folders() {
        let (base, root) = sandbox("files");
        let roots = [root.clone()];
        let file = root.join("sub").join("a.txt");
        let path = file.to_string_lossy().to_string();
        write_text(&roots, &json!({ "path": path, "content": "one\ntwo\nthree\n" })).unwrap();
        assert_eq!(read_text(&roots, &json!({ "path": path })).unwrap()["content"], "one\ntwo\nthree\n");
        assert_eq!(read_text(&roots, &json!({ "path": path, "line": 2, "limit": 1 })).unwrap()["content"], "two\n");
        let outside = base.join("outside").join("secret.txt").to_string_lossy().to_string();
        assert!(read_text(&roots, &json!({ "path": outside })).is_err());
        let climb = root.join("..").join("outside").join("secret.txt").to_string_lossy().to_string();
        assert!(read_text(&roots, &json!({ "path": climb })).is_err());
        assert!(write_text(&roots, &json!({ "path": climb, "content": "x" })).is_err());
        assert_eq!(std::fs::read_to_string(base.join("outside").join("secret.txt")).unwrap(), "s");
        assert!(read_text(&roots, &json!({ "path": "sub/a.txt" })).is_err());
        assert!(write_text(&roots, &json!({ "path": root.join("nope").join("b.txt").to_string_lossy(), "content": "x" })).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn output_keeps_the_newest_bytes() {
        let mut output = Output::default();
        push_output(&mut output, "abcdef", 4);
        assert_eq!(output.text, "cdef");
        assert!(output.truncated);
        let mut wide = Output::default();
        push_output(&mut wide, "\u{ac00}\u{b098}\u{b2e4}", 4);
        assert_eq!(wide.text, "\u{b2e4}");
    }

    #[test]
    fn whole_lines_go_through_the_shell() {
        let (program, args) = command_line("echo hi there", &[]);
        assert!(program == "cmd" || program == "sh");
        assert_eq!(args.last().unwrap(), "echo hi there");
        let (program, args) = command_line(if cfg!(windows) { "C:/x/git.exe" } else { "/usr/bin/git" }, &["status".into()]);
        assert!(program.ends_with("git.exe") || program.ends_with("git"));
        assert_eq!(args, ["status"]);
    }

    #[test]
    fn runs_a_command_and_reads_its_output() {
        let (base, root) = sandbox("term");
        let roots = [root.clone()];
        let made = create("t1", &roots, &json!({ "command": "echo hello-acp" })).unwrap();
        let id = made["terminalId"].clone();
        let exit = wait_for_exit("t1", &json!({ "terminalId": id })).unwrap();
        assert_eq!(exit["exitCode"], 0);
        // The readers may finish a moment after the process; on a busy machine that moment can be long.
        let mut got = output("t1", &json!({ "terminalId": id })).unwrap();
        for _ in 0..50 {
            if got["output"].as_str().unwrap_or("").contains("hello-acp") {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            got = output("t1", &json!({ "terminalId": id })).unwrap();
        }
        assert!(got["output"].as_str().unwrap().contains("hello-acp"));
        assert!(got.get("exitStatus").is_some());
        assert!(output("other", &json!({ "terminalId": id })).is_err());
        let outside = base.join("outside").to_string_lossy().to_string();
        assert!(create("t1", &roots, &json!({ "command": "echo x", "cwd": outside })).is_err());
        release("t1", &json!({ "terminalId": id })).unwrap();
        assert!(output("t1", &json!({ "terminalId": id })).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
