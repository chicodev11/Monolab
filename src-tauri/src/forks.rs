//! The Rust programs Monolab ships beside itself: the PumpkinMC fork (a server)
//! and Nova, the Horizon fork (a game client). Release builds bundle them in
//! `resources/forks`; `<data>/forks` or `MONOLAB_FORKS_DIR` can hold a local
//! build instead, which wins so a developer can try their own.

use crate::data_root::MonolabDirs;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
use tauri::{AppHandle, Manager};

/// The version name a Pumpkin server gets when it runs the bundled fork.
pub const PUMPKIN_FORK: &str = "monolab-fork";
/// The only Minecraft version Nova plays.
pub const NOVA_VERSION: &str = "26.3";

static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();

pub fn init(app: &AppHandle) {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("MONOLAB_FORKS_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Ok(data) = app.path().monolab_data_dir() {
        dirs.push(data.join("forks"));
    }
    if let Ok(resources) = app.path().resource_dir() {
        dirs.push(resources.join("forks"));
    }
    let _ = DIRS.set(dirs);
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The first folder holding `program`, a bare name like `pumpkin`.
pub fn find(program: &str) -> Option<PathBuf> {
    let file = exe(program);
    DIRS.get()?.iter().map(|dir| dir.join(&file)).find(|path| path.is_file())
}

/// Nova's folder: `horizon-client`, `horizon-prepare` and `research-lock.json` together.
pub fn nova_dir() -> Option<PathBuf> {
    let file = exe("horizon-client");
    DIRS.get()?.iter().map(|dir| dir.join("nova")).find(|dir| dir.join(&file).is_file() && dir.join("research-lock.json").is_file())
}

pub fn nova_program(dir: &Path, name: &str) -> PathBuf {
    dir.join(exe(name))
}

/// What this copy of Monolab carries, for the page to offer only what works.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Forks {
    pub pumpkin: bool,
    pub nova: bool,
}

#[tauri::command]
#[specta::specta]
pub fn bundled_forks() -> Forks {
    Forks { pumpkin: find("pumpkin").is_some(), nova: nova_dir().is_some() }
}
