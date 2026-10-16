
use serde::Serialize;

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogHit {
    pub source: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub project_type: String,
    pub installable: bool,
    pub page_url: String,
    pub icon_url: Option<String>,
    pub author: Option<String>,
    pub downloads: Option<u64>,
    pub follows: Option<u64>,
    pub updated_at: Option<i64>,
    pub loaders: Vec<String>,
    pub versions: Vec<String>,
}

pub fn is_loader(name: &str) -> bool {
    matches!(
        name,
        "paper" | "folia" | "purpur" | "spigot" | "bukkit" | "fabric" | "forge" | "neoforge" | "quilt"
            | "velocity" | "bungeecord" | "waterfall" | "sponge" | "datapack" | "rift" | "liteloader"
            | "modloader" | "canvas" | "iris" | "optifine" | "vanilla" | "minecraft"
    )
}

