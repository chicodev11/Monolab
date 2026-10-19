//! Dedicated servers for games other than Minecraft. Each game is a template:
//! where its files come from, what to run, its port and how to stop it.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Steam(u32),
    Terraria,
    FiveM,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stop {
    /// Type this into the console.
    Input(&'static str),
    /// Send Ctrl+C, the way the game expects to be closed from a terminal.
    CtrlC,
}

pub struct Setup<'a> {
    pub name: &'a str,
    pub password: &'a str,
    pub port: u16,
}

pub struct Game {
    pub id: &'static str,
    pub source: Source,
    pub exe: &'static str,
    pub port: u16,
    pub ready: &'static [&'static str],
    pub stop: Stop,
    pub env: &'static [(&'static str, &'static str)],
    pub args: fn(&Setup) -> String,
}

/// The launch arguments live in this file in the server folder, so they can be changed like any other setting.
pub const LAUNCH_FILE: &str = "monolab-launch.txt";

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "'"))
}

pub const GAMES: &[Game] = &[
    Game {
        id: "Terraria",
        source: Source::Terraria,
        exe: "TerrariaServer.exe",
        port: 7777,
        ready: &["Server started"],
        stop: Stop::Input("exit\n"),
        env: &[],
        args: |_| "-config serverconfig.txt".into(),
    },
    Game {
        id: "Valheim",
        source: Source::Steam(896660),
        exe: "valheim_server.exe",
        port: 2456,
        ready: &["Game server connected"],
        stop: Stop::CtrlC,
        env: &[("SteamAppId", "892970")],
        args: |setup| {
            format!(
                "-nographics -batchmode -name {} -port {} -world Dedicated -password {} -public 0 -savedir saves",
                quoted(setup.name),
                setup.port,
                setup.password
            )
        },
    },
    Game {
        id: "Palworld",
        source: Source::Steam(2394010),
        exe: "PalServer.exe",
        port: 8211,
        ready: &["Running Palworld dedicated server"],
        stop: Stop::CtrlC,
        env: &[],
        args: |setup| format!("-port={} -useperfthreads -NoAsyncLoadingThread -UseMultithreadForDS", setup.port),
    },
    Game {
        id: "Project Zomboid",
        source: Source::Steam(380870),
        exe: "StartServer64.bat",
        port: 16261,
        ready: &["SERVER STARTED"],
        stop: Stop::Input("quit\n\n"),
        env: &[],
        args: |setup| format!("-adminpassword {}", setup.password),
    },
    Game {
        id: "Rust",
        source: Source::Steam(258550),
        exe: "RustDedicated.exe",
        port: 28015,
        ready: &["Server startup complete"],
        stop: Stop::CtrlC,
        env: &[],
        args: |setup| {
            format!(
                "-batchmode -nographics +server.port {} +server.level \"Procedural Map\" +server.seed 12345 +server.worldsize 3000 +server.maxplayers 50 +server.hostname {} +server.identity monolab +rcon.port {} +rcon.password {} +rcon.web 1",
                setup.port,
                quoted(setup.name),
                setup.port + 1,
                setup.password
            )
        },
    },
    Game {
        id: "Satisfactory",
        source: Source::Steam(1690800),
        exe: "FactoryServer.exe",
        port: 7777,
        ready: &["Took", "seconds to LoadMap"],
        stop: Stop::CtrlC,
        env: &[],
        args: |setup| format!("-log -unattended -Port={}", setup.port),
    },
    Game {
        id: "7 Days to Die",
        source: Source::Steam(294420),
        exe: "7DaysToDieServer.exe",
        port: 26900,
        ready: &["GameServer.LogOn successful", "StartGame done"],
        stop: Stop::CtrlC,
        env: &[],
        args: |_| "-logfile - -quit -batchmode -nographics -configfile=serverconfig.xml -dedicated".into(),
    },
    Game {
        id: "ARK",
        source: Source::Steam(2430930),
        exe: "ShooterGame/Binaries/Win64/ArkAscendedServer.exe",
        port: 7777,
        ready: &["Server has completed startup"],
        stop: Stop::CtrlC,
        env: &[],
        args: |setup| {
            format!(
                "TheIsland_WP?listen?SessionName={}?Port={}?ServerAdminPassword={} -server -log -NoBattlEye",
                setup.name.replace([' ', '?', '"'], "_"),
                setup.port,
                setup.password
            )
        },
    },
    Game {
        id: "FiveM",
        source: Source::FiveM,
        exe: "FXServer.exe",
        port: 30120,
        ready: &["txAdmin", "Server started"],
        stop: Stop::CtrlC,
        env: &[],
        args: |_| String::new(),
    },
];

pub fn find(core: &str) -> Option<&'static Game> {
    GAMES.iter().find(|one| one.id == core)
}

pub fn is_game(core: &str) -> bool {
    find(core).is_some()
}

/// Reads the launch arguments for a game server.
pub fn launch_args(folder: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(folder.join(LAUNCH_FILE)).unwrap_or_default();
    let line: String = text.lines().filter(|one| !one.trim_start().starts_with('#')).collect::<Vec<_>>().join(" ");
    shell_words::split(&line).map_err(|error| format!("{LAUNCH_FILE} is malformed: {error}"))
}

/// The program to run and the arguments in front of the game's own ones.
pub fn program(game: &Game, folder: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let exe = folder.join(game.exe);
    if !exe.is_file() {
        return Err(format!("{} was not found in the server folder. Update the server files and try again.", game.exe));
    }
    if game.exe.ends_with(".bat") {
        return Ok((PathBuf::from("cmd"), vec!["/c".into(), exe.to_string_lossy().into_owned()]));
    }
    Ok((exe, Vec::new()))
}

type Progress<'a> = &'a dyn Fn(u64, u64);
type Cancelled<'a> = &'a dyn Fn() -> bool;

/// Downloads or updates a game's server files into the folder and writes its first settings.
pub fn install(app: &AppHandle, game: &Game, folder: &Path, setup: &Setup, fresh: bool, progress: Progress, cancelled: Cancelled) -> Result<(), String> {
    match game.source {
        Source::Steam(id) => steam_install(app, id, folder, progress, cancelled)?,
        Source::Terraria => terraria_install(app, folder, progress, cancelled)?,
        Source::FiveM => fivem_install(app, folder, progress, cancelled)?,
    }
    if !folder.join(game.exe).is_file() {
        return Err(format!("The download finished but {} is missing.", game.exe));
    }
    if fresh {
        let args = (game.args)(setup);
        fs::write(
            folder.join(LAUNCH_FILE),
            format!("# Launch options for {}. Change them here and restart the server.\n{args}\n", game.id),
        )
        .map_err(|error| error.to_string())?;
        if game.id == "Terraria" {
            write_terraria_config(folder, setup)?;
        }
    }
    Ok(())
}

fn client() -> Result<reqwest::blocking::Client, String> {
    crate::net::builder().timeout(Duration::from_secs(600)).build().map_err(|error| error.to_string())
}

fn fetch(url: &str, target: &Path, progress: Progress, cancelled: Cancelled) -> Result<(), String> {
    crate::net::check_download_url(url)?;
    let mut answer = client()?
        .get(url)
        .send()
        .and_then(|answer| answer.error_for_status())
        .map_err(|error| format!("Could not download {url}: {error}"))?;
    let total = answer.content_length().unwrap_or(0);
    let mut file = fs::File::create(target).map_err(|error| error.to_string())?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        if cancelled() {
            drop(file);
            let _ = fs::remove_file(target);
            return Err("Download cancelled.".into());
        }
        let read = answer.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        received += read as u64;
        progress(received, total);
    }
    Ok(())
}

fn steamcmd(app: &AppHandle, progress: Progress, cancelled: Cancelled) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|error| error.to_string())?.join("steamcmd");
    let exe = dir.join("steamcmd.exe");
    if exe.is_file() {
        return Ok(exe);
    }
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let zip = dir.join("steamcmd.zip");
    fetch("https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip", &zip, progress, cancelled)?;
    unzip_into(&zip, &dir, None)?;
    let _ = fs::remove_file(&zip);
    if !exe.is_file() {
        return Err("SteamCMD could not be set up.".into());
    }
    Ok(exe)
}

/// Pulls the number out of a SteamCMD line like "Update state (0x61) downloading, progress: 45.12 (100 / 222)".
pub fn steam_progress(line: &str) -> Option<f64> {
    let rest = &line[line.find("progress:")? + "progress:".len()..];
    rest.split_whitespace().next()?.parse().ok()
}

fn steam_install(app: &AppHandle, id: u32, folder: &Path, progress: Progress, cancelled: Cancelled) -> Result<(), String> {
    let exe = steamcmd(app, progress, cancelled)?;
    let mut child = crate::hidden_command(&exe)
        .arg("+force_install_dir")
        .arg(folder)
        .args(["+login", "anonymous", "+app_update", &id.to_string(), "validate", "+quit"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start SteamCMD: {error}"))?;
    let stdout = child.stdout.take().ok_or("Could not read SteamCMD's output.")?;
    let mut last = Vec::new();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if cancelled() {
            let _ = child.kill();
            return Err("Download cancelled.".into());
        }
        if let Some(percent) = steam_progress(&line) {
            progress((percent * 100.0) as u64, 10_000);
        }
        if !line.trim().is_empty() {
            last.push(line.trim().to_string());
            if last.len() > 8 {
                last.remove(0);
            }
        }
    }
    let _ = child.wait();
    if last.iter().any(|one| one.contains("Success! App")) {
        return Ok(());
    }
    Err(format!("SteamCMD couldn't download the server files.\n{}", last.join("\n")))
}

fn unzip_into(zip: &Path, target: &Path, strip_after: Option<&str>) -> Result<(), String> {
    let file = fs::File::open(zip).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    crate::zips::check_budget(&mut archive, 8 * 1024 * 1024 * 1024, 200_000)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(name) = entry.enclosed_name() else { continue };
        let name = name.to_string_lossy().replace('\\', "/");
        let relative = match strip_after {
            Some(marker) => match name.find(marker) {
                Some(at) => name[at + marker.len()..].to_string(),
                None => continue,
            },
            None => name,
        };
        if relative.is_empty() {
            continue;
        }
        let out = target.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut writer = fs::File::create(&out).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut writer).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Picks the newest file from Terraria's list of dedicated server downloads.
pub fn newest_terraria(names: &[String]) -> Option<&String> {
    names
        .iter()
        .filter(|name| name.ends_with(".zip"))
        .max_by_key(|name| name.chars().filter(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap_or(0))
}

fn terraria_install(_app: &AppHandle, folder: &Path, progress: Progress, cancelled: Cancelled) -> Result<(), String> {
    let names: Vec<String> = client()?
        .get("https://terraria.org/api/get/dedicated-servers-names")
        .send()
        .and_then(|answer| answer.error_for_status())
        .and_then(|answer| answer.json())
        .map_err(|error| format!("Could not get the Terraria server list: {error}"))?;
    let name = newest_terraria(&names).ok_or("No Terraria server download was found.")?;
    let zip = folder.join("terraria-server.zip");
    fetch(&format!("https://terraria.org/api/download/pc-dedicated-server/{name}"), &zip, progress, cancelled)?;
    let result = unzip_into(&zip, folder, Some("/Windows/"));
    let _ = fs::remove_file(&zip);
    result
}

fn write_terraria_config(folder: &Path, setup: &Setup) -> Result<(), String> {
    let worlds = folder.join("worlds");
    fs::create_dir_all(&worlds).map_err(|error| error.to_string())?;
    let config = format!(
        "world={}\nautocreate=2\nworldname=World\ndifficulty=0\nmaxplayers=8\nport={}\npassword=\nworldpath={}\n",
        worlds.join("World.wld").display(),
        setup.port,
        worlds.display()
    );
    fs::write(folder.join("serverconfig.txt"), config).map_err(|error| error.to_string())
}

fn fivem_install(_app: &AppHandle, folder: &Path, progress: Progress, cancelled: Cancelled) -> Result<(), String> {
    let info: serde_json::Value = client()?
        .get("https://changelogs-live.fivem.net/api/changelog/versions/win32/server")
        .send()
        .and_then(|answer| answer.error_for_status())
        .and_then(|answer| answer.json())
        .map_err(|error| format!("Could not get the FiveM server version: {error}"))?;
    let url = info
        .get("recommended_download")
        .or_else(|| info.get("latest_download"))
        .and_then(|one| one.as_str())
        .ok_or("The FiveM version list is malformed.")?
        .to_string();
    let archive = folder.join("fivem-server.7z");
    fetch(&url, &archive, progress, cancelled)?;
    let result = sevenz_rust2::decompress_file(&archive, folder).map_err(|error| format!("Could not unpack the FiveM server: {error}"));
    let _ = fs::remove_file(&archive);
    result
}

/// Sends Ctrl+C to a console program started without a window.
#[cfg(windows)]
pub fn ctrl_c(pid: u32) -> bool {
    use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler, CTRL_C_EVENT};
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return false;
        }
        SetConsoleCtrlHandler(None, 1);
        let sent = GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) != 0;
        std::thread::sleep(Duration::from_millis(300));
        FreeConsole();
        SetConsoleCtrlHandler(None, 0);
        sent
    }
}

#[cfg(not(windows))]
pub fn ctrl_c(pid: u32) -> bool {
    std::process::Command::new("kill").arg("-INT").arg(pid.to_string()).status().is_ok_and(|status| status.success())
}

/// Emits download progress the create dialog already knows how to show.
pub fn report(app: &AppHandle, game: &Game, received: u64, total: u64) {
    let _ = app.emit(
        "server-download",
        serde_json::json!({ "stage": "downloading", "core": game.id, "version": "latest", "received": received, "total": total }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_is_complete() {
        let mut ids: Vec<&str> = GAMES.iter().map(|one| one.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), GAMES.len());
        let setup = Setup { name: "My \"Server\"", password: "pw12345", port: 1234 };
        for game in GAMES {
            assert!(!game.exe.is_empty());
            assert!(game.port > 1024);
            let args = (game.args)(&setup);
            assert!(shell_words::split(&args).is_ok(), "{} has broken launch args: {args}", game.id);
        }
        assert!(is_game("Valheim"));
        assert!(!is_game("Paper"));
    }

    #[test]
    fn reads_steamcmd_progress_and_terraria_versions() {
        assert_eq!(steam_progress(" Update state (0x61) downloading, progress: 45.12 (100 / 222)"), Some(45.12));
        assert_eq!(steam_progress("Success! App '896660' fully installed."), None);
        let names = vec!["terraria-server-1449.zip".to_string(), "terraria-server-1450.zip".to_string(), "terraria-server-1436.zip".to_string()];
        assert_eq!(newest_terraria(&names).map(String::as_str), Some("terraria-server-1450.zip"));
    }

    #[test]
    fn reads_launch_args_with_comments() {
        let dir = std::env::temp_dir().join("monolab-games-args");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join(LAUNCH_FILE), "# note\n-name \"My Server\" -port 2456\n").unwrap();
        assert_eq!(launch_args(&dir).unwrap(), vec!["-name", "My Server", "-port", "2456"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
