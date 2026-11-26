//! The Minecraft cores in kinds.json. The UI imports the same file, so a new core is one entry there.

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub struct Minecraft {
    pub id: String,
    pub addons: Option<String>,
    pub proxy: bool,
    /// The app can make a new server of this core.
    #[serde(default)]
    pub create: bool,
}

#[derive(Deserialize)]
struct Table {
    minecraft: Vec<Minecraft>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(include_str!("kinds.json")).expect("kinds.json is malformed"))
}

pub fn minecraft(core: &str) -> Option<&'static Minecraft> {
    table().minecraft.iter().find(|one| one.id == core)
}

/// Cores a new server can be made with, in the order the create screen shows them.
pub fn creatable() -> Vec<&'static str> {
    table().minecraft.iter().filter(|one| one.create).map(|one| one.id.as_str()).collect()
}

pub fn is_proxy(core: &str) -> bool {
    minecraft(core).is_some_and(|one| one.proxy)
}

/// "plugin" or "mod": what the add-ons folder of this core holds. Unknown cores read as mods.
pub fn addon_word(core: &str) -> &'static str {
    match minecraft(core).and_then(|one| one.addons.as_deref()) {
        Some("plugin") => "plugin",
        _ => "mod",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_shared_table() {
        assert!(!is_proxy("Paper"));
        assert!(is_proxy("Velocity") && is_proxy("BungeeCord"));
        assert_eq!(addon_word("Purpur"), "plugin");
        assert_eq!(addon_word("NeoForge"), "mod");
        assert_eq!(addon_word("Custom"), "mod");
    }
}
