//! Commands for game servers that aren't Minecraft: creating one, updating its files, and the free space before that.

use super::*;

pub(crate) fn create_game_server(app: &AppHandle, state: &AppState, game: &'static games::Game, name: String, operation: &str) -> Result<InstanceView, String> {
    let folder = free_folder(&managed_servers_root(app)?, &name);
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the server folder: {error}"))?;
    let folder = folder.canonicalize().map(plain_path).map_err(|error| format!("Could not verify the server folder: {error}"))?;
    if !operation.is_empty() {
        state.cancelled_downloads.lock().expect("download state poisoned").remove(operation);
    }
    let password = web::make_key()[..10].to_string();
    let setup = games::Setup { name: &name, password: &password, port: game.port };
    let cancelled = || !operation.is_empty() && state.cancelled_downloads.lock().expect("download state poisoned").contains(operation);
    let progress = |received: u64, total: u64| games::report(app, game, received, total);
    let installed = off_runtime(|| games::install(app, game, &folder, &setup, true, &progress, &cancelled));
    if let Err(error) = installed {
        let _ = fs::remove_dir_all(&folder);
        return Err(error);
    }
    let instance = ServerInstance {
        id: instance_id(),
        name,
        path: folder.to_string_lossy().to_string(),
        core: game.id.to_string(),
        version: "latest".into(),
        launch_mode: "game".into(),
        server_jar: Some(folder.join(&game.exe).to_string_lossy().to_string()),
        java_path: String::new(),
        max_memory_mib: 4096,
        min_memory_mib: 4096,
        jvm_args: String::new(),
        port: game.port,
        sort_order: 0,
        start_with_app: false,
        crash_restart: false,
        core_build: String::new(),
        created_at: unix_now(),
        last_run_at: 0,
        custom_command: String::new(),
        start_script: String::new(),
        start_script_hash: String::new(),
        remote_host: String::new(),
        linked: false,
        panel_host: String::new(),
        panel_server: String::new(),
        panel_daemon: String::new(),
    };
    let _store = lock_instance_store();
    let mut instances = load_instances(app)?;
    instances.push(instance.clone());
    save_instances(app, &instances)?;
    drop(_store);
    Ok(instance_view(instance, state))
}

/// Downloads the latest files for a game server again (SteamCMD update or a fresh official download).
#[tauri::command(async)]
#[specta::specta]
pub fn update_game_server(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    let game = games::find(&instance.core).ok_or("This isn't a game server.")?;
    if is_running(&state, &id) {
        return Err("Stop the server first.".into());
    }
    let setup = games::Setup { name: &instance.name, password: "", port: instance.port };
    let progress = |received: u64, total: u64| games::report(&app, game, received, total);
    off_runtime(|| games::install(&app, game, Path::new(&instance.path), &setup, false, &progress, &|| false))?;
    record_audit(&app, &id, "Server", "Server files updated", &game.id);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn servers_root_free_bytes(app: AppHandle) -> Result<u64, String> {
    Ok(local_free_bytes(&managed_servers_root(&app)?))
}

/// Launch settings for a custom program. Kept in the app's data folder, not the server folder.
#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomProgram {
    /// The file to run, relative to the server folder when it is inside it.
    pub program: String,
    pub args: String,
    /// A line in the console that means the server is ready. Empty: ready a few seconds after start.
    pub ready: String,
    /// Typed into the console to stop it. Empty: send Ctrl+C.
    pub stop_input: String,
    /// What the port speaks, for a playit tunnel: "tcp", "udp" or "both". Empty is tcp.
    pub protocol: String,
}

/// How a playit tunnel for a server looks: port type, playit tunnel type, and how many ports.
pub(crate) struct TunnelKind {
    pub port_type: &'static str,
    pub tunnel_type: Option<String>,
    pub port_count: u16,
}

fn port_type(protocol: &str) -> &'static str {
    match protocol {
        "udp" => "udp",
        "both" => "both",
        _ => "tcp",
    }
}

/// Minecraft is a Minecraft tunnel; a game or a custom program says its own protocol.
pub(crate) fn tunnel_kind(app: &AppHandle, instance: &ServerInstance) -> Result<TunnelKind, String> {
    if let Some(game) = game_for(app, instance) {
        return Ok(TunnelKind {
            port_type: port_type(&game.protocol),
            tunnel_type: game.playit.clone(),
            port_count: game.port_count.unwrap_or(1).max(1),
        });
    }
    Ok(TunnelKind { port_type: "tcp", tunnel_type: Some("minecraft-java".into()), port_count: 1 })
}

fn custom_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(instance_data_dir(app, id)?.join("program.json"))
}

fn read_custom(app: &AppHandle, id: &str) -> Result<CustomProgram, String> {
    let text = fs::read_to_string(custom_path(app, id)?).map_err(|_| "The program settings for this server are missing.".to_string())?;
    serde_json::from_str(&text).map_err(|error| format!("The program settings are malformed: {error}"))
}

fn checked(folder: &Path, mut program: CustomProgram) -> Result<CustomProgram, String> {
    program.program = program.program.trim().to_string();
    program.args = program.args.trim().to_string();
    program.ready = program.ready.trim().to_string();
    if program.program.is_empty() {
        return Err("Choose the program to run.".into());
    }
    let path = folder.join(&program.program);
    if !path.is_file() {
        return Err(format!("{} was not found.", program.program));
    }
    if let Ok(inside) = path.canonicalize().map(plain_path) {
        if let Ok(relative) = inside.strip_prefix(folder) {
            program.program = relative.to_string_lossy().into_owned();
        }
    }
    shell_words::split(&program.args).map_err(|error| format!("The arguments are malformed: {error}"))?;
    Ok(program)
}

fn as_game(program: &CustomProgram, port: u16) -> games::Game {
    let stop_input = program.stop_input.trim();
    games::Game {
        id: games::CUSTOM.into(),
        source: None,
        exe: program.program.clone(),
        port,
        protocol: program.protocol.clone(),
        port_count: None,
        playit: None,
        ready: if program.ready.is_empty() { Vec::new() } else { vec![program.ready.clone()] },
        stop: if stop_input.is_empty() { games::Stop::CtrlC } else { games::Stop::Input(format!("{stop_input}\n")) },
        env: Default::default(),
        args: program.args.clone(),
        requires: Vec::new(),
    }
}

/// How to run, stop and recognise a non-Minecraft server: from kinds.json, or from the user's own settings.
pub(crate) fn game_for(app: &AppHandle, instance: &ServerInstance) -> Option<games::Game> {
    if games::is_custom(&instance.core) {
        return read_custom(app, &instance.id).ok().map(|program| as_game(&program, instance.port));
    }
    games::find(&instance.core).cloned()
}

/// Launch arguments: the user's settings for a custom program, the launch file for a game.
pub(crate) fn args_for(game: &games::Game, folder: &Path) -> Result<Vec<String>, String> {
    if games::is_custom(&game.id) {
        return shell_words::split(&game.args).map_err(|error| format!("The arguments are malformed: {error}"));
    }
    games::launch_args(folder)
}

/// Registers a server program that is already on this PC. Nothing is downloaded or copied.
#[tauri::command(async)]
#[specta::specta]
pub fn create_custom_server(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: String,
    name: String,
    port: u16,
    program: CustomProgram,
) -> Result<InstanceView, String> {
    let folder = Path::new(&folder)
        .canonicalize()
        .map(plain_path)
        .map_err(|_| "The server folder was not found.".to_string())?;
    if !folder.is_dir() {
        return Err("The server folder was not found.".into());
    }
    if port == 0 {
        return Err("Enter the port the server uses.".into());
    }
    let program = checked(&folder, program)?;
    let name = if name.trim().is_empty() {
        folder.file_name().map(|one| one.to_string_lossy().into_owned()).unwrap_or_else(|| "Server".into())
    } else {
        name.trim().to_string()
    };
    let instance = ServerInstance {
        id: instance_id(),
        name,
        path: folder.to_string_lossy().to_string(),
        core: games::CUSTOM.into(),
        version: String::new(),
        launch_mode: "game".into(),
        server_jar: Some(folder.join(&program.program).to_string_lossy().to_string()),
        java_path: String::new(),
        max_memory_mib: 0,
        min_memory_mib: 0,
        jvm_args: String::new(),
        port,
        sort_order: 0,
        start_with_app: false,
        crash_restart: false,
        core_build: String::new(),
        created_at: unix_now(),
        last_run_at: 0,
        custom_command: String::new(),
        start_script: String::new(),
        start_script_hash: String::new(),
        remote_host: String::new(),
        linked: true,
        panel_host: String::new(),
        panel_server: String::new(),
        panel_daemon: String::new(),
    };
    let path = custom_path(&app, &instance.id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, serde_json::to_string_pretty(&program).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    if instances.iter().any(|one| Path::new(&one.path) == folder) {
        return Err("This folder is already in the server list.".into());
    }
    instances.push(instance.clone());
    save_instances(&app, &instances)?;
    drop(_store);
    record_audit(&app, &instance.id, "Server", "Custom program added", &program.program);
    Ok(instance_view(instance, &state))
}

#[tauri::command(async)]
#[specta::specta]
pub fn read_custom_program(app: AppHandle, id: String) -> Result<CustomProgram, String> {
    let instance = instance_by_id(&app, &id)?;
    if !games::is_custom(&instance.core) {
        return Err("This isn't a custom program server.".into());
    }
    read_custom(&app, &id)
}

/// Saves new launch settings for a custom program. They apply on the next start.
#[tauri::command(async)]
#[specta::specta]
pub fn save_custom_program(app: AppHandle, id: String, port: u16, program: CustomProgram) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    if !games::is_custom(&instance.core) {
        return Err("This isn't a custom program server.".into());
    }
    if port == 0 {
        return Err("Enter the port the server uses.".into());
    }
    let folder = Path::new(&instance.path).to_path_buf();
    let program = checked(&folder, program)?;
    fs::write(custom_path(&app, &id)?, serde_json::to_string_pretty(&program).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    if let Some(one) = instances.iter_mut().find(|one| one.id == id) {
        one.port = port;
        one.server_jar = Some(folder.join(&program.program).to_string_lossy().to_string());
    }
    save_instances(&app, &instances)?;
    drop(_store);
    record_audit(&app, &id, "Server", "Custom program settings saved", &program.program);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_custom_program_runs_like_a_game() {
        let dir = std::env::temp_dir().join("monolab-custom-program");
        let _ = fs::create_dir_all(dir.join("ruby/bin"));
        fs::write(dir.join("ruby/bin/ruby.exe"), b"").unwrap();
        let folder = dir.canonicalize().map(plain_path).unwrap();
        let program = checked(
            &folder,
            CustomProgram { program: "ruby/bin/ruby.exe".into(), args: " Server.rb \"two words\" ".into(), ready: " Server started ".into(), stop_input: String::new(), protocol: String::new() },
        )
        .unwrap();
        assert_eq!(program.ready, "Server started");
        let game = as_game(&program, 12345);
        assert!(games::is_custom(&game.id));
        assert_eq!(game.stop, games::Stop::CtrlC);
        assert_eq!(game.ready, vec!["Server started".to_string()]);
        assert_eq!(args_for(&game, &folder).unwrap(), vec!["Server.rb", "two words"]);
        let typed = as_game(&CustomProgram { stop_input: "exit".into(), ..program.clone() }, 1);
        assert_eq!(typed.stop, games::Stop::Input("exit\n".into()));
        assert!(checked(&folder, CustomProgram { program: "missing.exe".into(), ..Default::default() }).is_err());
        assert!(checked(&folder, CustomProgram { args: "\"open".into(), ..program }).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
