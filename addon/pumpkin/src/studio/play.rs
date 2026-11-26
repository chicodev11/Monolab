//! The studio's interactive kinds on Pumpkin: commands, menus, items, recipes, rules (S3–S5), and NPCs, dialogs,
//! loot tables, advancements and worlds. Same files and the same action lists as the Paper add-on
//! (`StudioActions.java` and friends); what Pumpkin's plugin API can't do yet is said in that file's status.

use super::{Studio, render, shows, values, world_named};
use crate::names;
use pumpkin_plugin_api::command::{ArgumentType, Command, CommandNode, StringType};
use pumpkin_plugin_api::common::Hand;
use pumpkin_plugin_api::gui::Gui;
use pumpkin_plugin_api::java_dialog::{Action, ActionButton, AfterAction, CustomClickAction, Dialog, DialogBody, DialogInput, DialogInputBool, DialogInputNumberRange, DialogInputSingleOption, DialogInputText, DialogType};
use pumpkin_plugin_api::java_dialogs::DynamicCustomAction;
use pumpkin_plugin_api::common::{NbtTag, NbtTree};
use pumpkin_plugin_api::persistent_data::PersistentDataHolder;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::uuid::Uuid;
use pumpkin_plugin_api::world::Entity;
use pumpkin_plugin_api::wit::pumpkin::plugin::recipe::{CookingRecipe, CookingType, Ingredient, ShapedRecipe, ShapelessRecipe, StonecuttingRecipe};
use pumpkin_plugin_api::wit::pumpkin::plugin::server::{CommandSender as RunAs, FlatWorldLayer, FlatWorldSettings, PluginGeneratorSettings, WorldGenerator};
use pumpkin_plugin_api::wit::pumpkin::plugin::sounds::SoundCategory;
use pumpkin_plugin_api::{Dimension, GameRuleValue, ItemStack, Player, Screen, Server};
use serde_json::{Map, Value, json};
use super::blocks::Outcome;

mod shops;
mod steps;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use studio_format::fields::{self, Data};

/// Tag namespace for our items and entities.
pub const TAG: &str = "monolab";
/// The longest a run may wait in all (ten minutes), and the most actions one run chains.
const MOST_WAIT: u64 = 20 * 60 * 10;
const MOST_STEPS: usize = 200;
/// A repeat runs its steps this many times at most; one run goes through this many steps in all (each branch and round).
const MOST_REPEATS: i64 = 100;
const MOST_RUN: usize = 2000;
/// A step put in front of a repeat's round: the rest of the run sees `<index>` as that round.
const ROUND: &str = "\u{0}round";
/// A step put off after a canvas click: draw the dialog again (`dialog`) if it is still the one shown (`seen`).
const REDRAW: &str = "\u{0}redraw";
/// How long a pressed canvas button shows, in studio ticks.
const PRESSED_TICKS: u64 = 1;
const MOST_TRACES: u32 = 20;
const LOOK_DISTANCE: f64 = 8.0;
pub const RULE_EVENTS: [&str; 27] = [
    "join", "first_join", "quit", "chat", "death", "respawn", "break", "place", "world_change", "command", "timer", "schedule", "click", "click_entity", "kill", "hit",
    "hurt", "eat", "craft", "fish", "sneak", "drop", "pickup", "level_up", "teleport", "gamemode", "server_start",
];

/// Who an action list runs for, and its placeholders (`<arg1>`, `<message>`, `<input:kit>`...).
#[derive(Clone, Default)]
pub struct Run {
    pub player: Option<Uuid>,
    pub name: String,
    pub extra: Vec<(String, String)>,
    /// M5: in a mob skill, the mob (entity id) that casts it.
    pub caster: Option<u32>,
    /// M5: in a mob skill, the entity (id) the actions work on when it isn't a player (`player` is set for players).
    pub target: Option<u32>,
}

impl Run {
    pub fn of(player: &Player, extra: Vec<(String, String)>) -> Self {
        Self { player: Some(player.get_id()), name: player.get_name(), extra, caster: None, target: None }
    }

    pub fn server(extra: Vec<(String, String)>) -> Self {
        Self { player: None, name: String::new(), extra, caster: None, target: None }
    }

    /// The same run with more placeholders.
    pub fn with(&self, more: Vec<(String, String)>) -> Self {
        let mut out = self.clone();
        out.extra.extend(more);
        out
    }
}

/// A rule to fire later: when (studio ticks), the event, the player and the placeholders.
type Later = (u64, String, Uuid, Vec<(String, String)>);

/// The rest of a run put off by a `wait`.
struct Waiting {
    due: u64,
    steps: Vec<Value>,
    run: Run,
    waited: u64,
}

/// A command Pumpkin should learn: its names and description. Registered from `lib.rs`, outside the studio lock.
pub struct NewCommand {
    pub names: Vec<String>,
    pub description: String,
}

#[derive(Default)]
pub struct Play {
    /// Structures: block lists read and the builds being put down.
    pub(super) structures: super::structures::Structures,
    /// The studio's file generation these were last built from.
    built: u64,
    /// Songs registered while the server started, with what they were registered as (`song::fingerprint`).
    songs: HashMap<String, String>,
    /// Paintings registered while the server started (`painting::fingerprint`).
    paintings: HashMap<String, String>,
    /// Biomes registered while the server started, with their JSON (`biome::registry_json`).
    biomes: HashMap<String, String>,
    /// Cosmetic displays riding players: by player (uuid text) and slot, the cosmetic id, its file as made, the display.
    cosmetics_shown: HashMap<String, HashMap<String, (String, String, pumpkin_plugin_api::world::Entity)>>,
    /// Where each player was at the last cosmetic update (trails puff when they moved).
    cosmetic_last: HashMap<String, (f64, f64, f64)>,
    cosmetic_ticks: u64,
    /// Studio commands Pumpkin knows: main name → every name (aliases too) and the description.
    registered: HashMap<String, (Vec<String>, String)>,
    /// Recipes given to Pumpkin, by file id: its recipe key, and the JSON (with the studio items it uses) it was
    /// made from.
    recipes: HashMap<String, (String, String)>,
    /// How deep `stack` is in building food leftovers (a leftover can be a studio item with its own).
    leftover_depth: u8,
    /// Open studio menus by player uuid: the menu id, its slots and how many slots the menu has.
    menus: HashMap<String, (String, HashMap<i32, Data>, i32)>,
    /// Open studio shops by player uuid: the shop id and the screen's inventory (drawn again after each click).
    shops: HashMap<String, (String, pumpkin_plugin_api::Inventory)>,
    vars_server: Map<String, Value>,
    vars_players: HashMap<String, Map<String, Value>>,
    vars_loaded: bool,
    vars_dirty: bool,
    cooldowns: HashMap<String, u64>,
    timers: HashMap<String, i64>,
    last_minute: String,
    waiting: Vec<Waiting>,
    /// Each player's last dialog drawn (a count), so a put-off redraw doesn't bring back one they left.
    dialog_shown: HashMap<String, u64>,
    dialog_count: u64,
    /// Set by a `cancel` step while a cancellable event's rules run (`fire_cancellable` reads it).
    cancel: bool,
    /// The steps added with the node editor (`steps.rs`): boss bars, timed permissions, worlds to unload.
    pub(super) steps: steps::Steps,
    traces_this_second: u32,
    traces: Vec<String>,
    /// NPC entities by file name, and file names by entity id.
    npcs: HashMap<String, Entity>,
    npc_ids: HashMap<i32, String>,
    npcs_dirty: bool,
    /// Blocks just broken, by position, for the drop event that follows (it doesn't say which block).
    broken: HashMap<(i32, i32, i32), String>,
    /// Worlds already made or checked this run.
    worlds: HashSet<String>,
    /// The advancement files changed: register them again.
    advancements_dirty: bool,
    /// Advancements we registered (`monolab:<id>`), to take out when their file goes.
    advancements: HashSet<String>,
    random: u64,
    /// Rules to fire a little later (join, respawn), when the player can see what they do.
    later: Vec<Later>,
    /// M4: model copies and their rigs.
    pub(super) models: super::models::Models,
    /// M5: studio mobs alive in the worlds, and their skills.
    pub(super) mobs: super::mobs::Mobs,
    /// Armor set effects each player has from us (effect names), to take away when the set comes off.
    set_given: HashMap<(u64, u64), Vec<String>>,
}

impl Studio {
    // ---------- shared ----------

    /// Files changed since the interactive kinds were last built: check them and report what Pumpkin can't do.
    pub(super) async fn rebuild_play(&mut self, server: &Server) {
        if self.play.built == self.generation {
            return;
        }
        self.play.built = self.generation;
        self.play.npcs_dirty = true;
        self.play.advancements_dirty = true;
        self.play.worlds.clear();
        self.play.models.reload();
        let block_rows: Vec<(String, Data)> = self.active_rows("block").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        let furniture_rows: Vec<(String, Data)> = self.active_rows("furniture").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        let block_problems = self.blocks.as_mut().map(|blocks| blocks.load(server, &block_rows, &furniture_rows)).unwrap_or_default();
        for (kind, id, text) in block_problems {
            self.problem(kind, &id, &text);
        }
        // Middle-clicking one of our registered blocks gives its item (the fork looks up `monolab:<id>`).
        for id in self.blocks.as_ref().map(|blocks| blocks.custom_ids()).unwrap_or_default() {
            if let Some(stack) = self.stack(server, &format!("studio:block/{id}"), 1, &Run::default()).await
                && let Err(error) = pumpkin_plugin_api::custom_block::register_item(&format!("{TAG}:{id}"), &stack)
            {
                tracing::warn!("Could not register the item of block {id}: {error}");
            }
        }
        let ids: HashSet<String> = self.active_rows("rule").into_iter().map(|(id, _)| id.to_string()).collect();
        self.play.timers.retain(|id, _| ids.contains(id));
        self.register_recipes(server).await;
        let mut problems: Vec<(String, String, String)> = Vec::new();
        // M6: actions and conditions other plugins add ("type": "plugin:name") only run on Paper.
        for kind in studio_format::KINDS {
            for (id, data) in self.active_rows(kind) {
                if uses_step(&Value::Object(data.clone()), "set_group") || uses_step(&Value::Object(data.clone()), "group") {
                    problems.push((kind.into(), id.into(), "LuckPerms groups are Paper only: set group does nothing here and the group condition never passes.".into()));
                }
                if uses_step(&Value::Object(data.clone()), "money") {
                    problems.push((kind.into(), id.into(), "Vault money is Paper only: the money action does nothing here and the money condition never passes.".into()));
                }
                if uses_expansion(&Value::Object(data.clone())) {
                    problems.push((kind.into(), id.into(), "Pumpkin plugins can't add studio actions yet, so the plugin's actions and conditions are skipped here.".into()));
                }
            }
        }
        for (id, data) in self.active_rows("rule") {
            let event = when_event(data);
            if event.contains(':') {
                problems.push(("rule".into(), id.into(), "Pumpkin plugins can't add studio triggers yet, so this rule never runs here.".into()));
            } else if !RULE_EVENTS.contains(&event.as_str()) {
                problems.push(("rule".into(), id.into(), format!("\"when.event\" must be one of: {}.", RULE_EVENTS.join(", "))));
            }
        }
        for (id, data) in self.active_rows("dialog") {
            if let Some(text) = super::canvas::problem(data) {
                problems.push(("dialog".into(), id.into(), text));
            }
        }
        for (id, _) in self.active_rows("font") {
            if let Some(text) = super::canvas::font_problem(id) {
                problems.push(("font".into(), id.into(), text));
            }
        }
        for (id, data) in self.active_rows("item") {
            if let Some(text) = item_problem(data) {
                problems.push(("item".into(), id.into(), text));
            }
        }
        for (id, data) in self.active_rows("announcement") {
            if let Some(text) = studio_format::announcement::problems(data).into_iter().next() {
                problems.push(("announcement".into(), id.into(), text));
            }
        }
        for (id, data) in self.active_rows("enchant") {
            if let Some(text) = studio_format::enchant::problems(data).into_iter().next() {
                problems.push(("enchant".into(), id.into(), text));
            }
        }
        for (id, data) in self.active_rows("song") {
            let text = studio_format::song::problems(data).into_iter().next().or_else(|| match self.play.songs.get(id) {
                None => Some("Restart the server to add this song: jukebox songs are only taken while it starts.".into()),
                Some(was) if *was != studio_format::song::fingerprint(data, id) => Some("Changed since the server started: jukeboxes play it as it was until the next restart.".into()),
                Some(_) => None,
            });
            if let Some(text) = text {
                problems.push(("song".into(), id.into(), text));
            }
        }
        problems.extend(self.quest_problems());
        problems.extend(self.structure_problems());
        for (id, data) in self.active_rows("function") {
            if let Some(text) = studio_format::function::problems(id, data).into_iter().next() {
                problems.push(("function".into(), id.into(), text));
            }
        }
        for (id, data) in self.active_rows("shop") {
            let text = studio_format::shop::problems(data).into_iter().next().or_else(|| (studio_format::shop::currency(data) == "vault").then(|| "Vault money is Paper only, so this shop doesn't open on Pumpkin. Use a variable instead.".to_string()));
            if let Some(text) = text {
                problems.push(("shop".into(), id.into(), text));
            }
        }
        for (id, data) in self.active_rows("trade") {
            let text = studio_format::trade::problems(data).into_iter().next().unwrap_or_else(|| "Pumpkin has no trade screen for plugins yet, so the trade action does nothing here (Paper opens it).".into());
            problems.push(("trade".into(), id.into(), text));
        }
        for (id, data) in self.active_rows("painting") {
            let text = studio_format::painting::problems(data).into_iter().next().or_else(|| match self.play.paintings.get(id) {
                None => Some("Restart the server to add this painting: paintings are only taken while it starts.".into()),
                Some(was) if *was != studio_format::painting::fingerprint(data, id) => Some("Changed since the server started: its size and title stay as they were until the next restart (the picture comes from the pack).".into()),
                Some(_) => None,
            });
            if let Some(text) = text {
                problems.push(("painting".into(), id.into(), text));
            }
        }
        for (id, data) in self.active_rows("biome") {
            let text = studio_format::biome::problems(data).into_iter().next().or_else(|| match self.play.biomes.get(id) {
                None => Some("Restart the server to add this biome: biomes are only taken while it starts.".into()),
                Some(was) if *was != studio_format::biome::registry_json(data) => Some("Changed since the server started: it looks as it was until the next restart.".into()),
                Some(_) => None,
            });
            if let Some(text) = text {
                problems.push(("biome".into(), id.into(), text));
            }
        }
        for (id, data) in self.active_rows("world") {
            let kind = fields::text(data, "type", "normal");
            if matches!(kind.as_str(), "amplified" | "large_biomes") && !matches!(fields::text(data, "environment", "normal").as_str(), "normal" | "overworld") {
                problems.push(("world".into(), id.into(), format!("\"{kind}\" works only in the overworld; this one is made with its dimension's own terrain.")));
            }
        }
        for (kind, id, text) in problems {
            self.problem(&kind, &id, &text);
        }
        self.rebuild_mobs(server).await;
    }

    /// Every studio tick (10 server ticks): waits that are due, NPC heads, timers and schedules once a second.
    pub(super) async fn tick_play(&mut self, server: &Server) {
        self.rebuild_play(server).await;
        let now = self.ticks;
        let due: Vec<Waiting> = {
            let (ready, later): (Vec<Waiting>, Vec<Waiting>) = std::mem::take(&mut self.play.waiting).into_iter().partition(|one| one.due <= now);
            self.play.waiting = later;
            ready
        };
        for one in due {
            self.continue_at(server, one.steps, &one.run, one.waited).await;
        }
        let (fire_now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.play.later).into_iter().partition(|one| one.0 <= now);
        self.play.later = later;
        for (_, event, id, extra) in fire_now {
            if let Some(player) = server.get_player_by_uuid(id) {
                self.fire(server, &event, Some(&player), extra, |_| true).await;
            }
        }
        self.tick_steps(server).await;
        self.refresh_npcs(server).await;
        self.tick_mobs(server).await;
        self.tick_structures().await;
        if self.ticks.is_multiple_of(20) {
            self.play.traces_this_second = 0;
            self.second(server).await;
            self.wear_sets(server).await;
            self.load_worlds(server).await;
            self.register_advancements(server).await;
            if let Some(blocks) = &self.blocks {
                blocks.tick(&server.get_all_worlds()).await;
            }
        }
        if self.ticks.is_multiple_of(600) {
            self.save_vars();
        }
    }

    /// Every server tick: model frames (the animations are baked at 20 a second).
    pub async fn tick_models(&mut self, server: &Server) {
        if !self.was_on {
            return;
        }
        let rows: Vec<(String, Data)> = self.active_rows("model").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        let root = self.root.clone();
        for (id, line) in self.play.models.tick(server, &root, &rows, |name| super::world_named(server, name)).await {
            self.problem("model", &id, &line);
        }
    }

    /// Every server tick (each `cosmetic::EVERY_TICKS`th does the work): hats and back pieces ride their players
    /// turned with them, trails puff where they walk. Like Paper's `StudioCosmetics`.
    pub async fn tick_cosmetics(&mut self, server: &Server) {
        use pumpkin_plugin_api::{DisplayTransformation, EntityDisplayExt, EntityType, ItemDisplayMode, Quaternionf, Vector3f};
        use studio_format::cosmetic;
        self.play.cosmetic_ticks += 1;
        if !self.play.cosmetic_ticks.is_multiple_of(cosmetic::EVERY_TICKS) {
            return;
        }
        let rows: HashMap<String, Data> = if self.was_on {
            self.active_rows("cosmetic").into_iter().filter(|(_, data)| cosmetic::problems(data).is_empty()).map(|(id, data)| (id.to_string(), data.clone())).collect()
        } else {
            HashMap::new()
        };
        if rows.is_empty() && self.play.cosmetics_shown.is_empty() {
            return;
        }
        let players = server.get_all_players();
        let online: HashSet<String> = players.iter().map(|one| id_text(one.get_id())).collect();
        self.play.cosmetics_shown.retain(|who, slots| {
            let keep = online.contains(who);
            if !keep {
                slots.values().for_each(|(_, _, display)| display.remove());
            }
            keep
        });
        self.play.cosmetic_last.retain(|who, _| online.contains(who));
        for player in players {
            let who = id_text(player.get_id());
            let body = player.as_entity();
            let worn = cosmetic::parse(&body.get_string(TAG, cosmetic::KEY).await.unwrap_or_default());
            let hidden = matches!(player.get_gamemode(), pumpkin_plugin_api::player::GameMode::Spectator) || body.is_invisible() || body.is_dead();
            let mut allowed = HashMap::new();
            for (_, id) in &worn {
                let permission = rows.get(id).map(cosmetic::permission).unwrap_or_default();
                allowed.insert(id.clone(), permission.is_empty() || player.has_permission(permission).await);
            }
            let wears = |slot: &str| -> Option<(String, Data)> {
                let (_, id) = worn.iter().find(|(one, _)| one == slot)?;
                let data = rows.get(id).filter(|data| cosmetic::slot(data) == slot)?;
                (!hidden && allowed.get(id).copied().unwrap_or(false)).then(|| (id.clone(), data.clone()))
            };
            for slot in ["hat", "back"] {
                let want = wears(slot);
                let had = self.play.cosmetics_shown.get_mut(&who).and_then(|slots| slots.remove(slot));
                let Some((id, data)) = want else {
                    if let Some((_, _, display)) = had {
                        display.remove();
                    }
                    continue;
                };
                let made = serde_json::to_string(&data).unwrap_or_default();
                if let Some((was, was_made, display)) = had {
                    if was == id && was_made == made && display.is_valid() && display.get_vehicle().is_some_and(|ride| ride.get_id() == body.get_id()) {
                        display.set_rotation(body.get_yaw(), 0.0);
                        self.play.cosmetics_shown.entry(who.clone()).or_default().insert(slot.to_string(), (was, was_made, display));
                        continue;
                    }
                    display.remove();
                }
                let Some(stack) = self.stack(server, &cosmetic::item(&data), 1, &Run::of(&player, Vec::new())).await else { continue };
                let entity = body.get_world().spawn_entity(EntityType::ItemDisplay, body.get_position()).await;
                entity.set_saves_to_disk(false);
                entity.set_rotation(body.get_yaw(), 0.0);
                if let Some(display) = entity.as_item_display() {
                    display.set_item(Some(stack)).await;
                    display.set_item_display_mode(ItemDisplayMode::Head);
                }
                if let Some(display) = entity.as_display() {
                    let (x, y, z) = cosmetic::translation(&data);
                    let size = cosmetic::scale(&data);
                    let still = Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };
                    display.set_transformation(DisplayTransformation { translation: Vector3f { x, y, z }, left_rotation: still, scale: Vector3f { x: size, y: size, z: size }, right_rotation: still });
                    display.set_teleport_duration(cosmetic::EVERY_TICKS as i32);
                    body.add_passenger(display.get_entity()).await;
                }
                self.play.cosmetics_shown.entry(who.clone()).or_default().insert(slot.to_string(), (id, made, entity));
            }
            let now = body.get_position();
            let before = self.play.cosmetic_last.insert(who.clone(), now);
            if let Some((_, data)) = wears("trail")
                && let Some(before) = before
                && (now.0 - before.0).powi(2) + (now.1 - before.1).powi(2) + (now.2 - before.2).powi(2) > 0.0025
                && !player.is_flying()
                && let Some(particle) = super::mobs::particle(&cosmetic::particle(&data))
            {
                body.get_world().spawn_particle(particle, (now.0, now.1 + 0.1, now.2), (0.15, 0.05, 0.15), 0.0, cosmetic::count(&data) as i32);
            }
        }
    }

    /// The `cosmetic` action: `mode` toggle (the default), wear or remove.
    async fn wear_cosmetic(&mut self, player: &Player, id: &str, mode: &str) {
        use studio_format::cosmetic;
        let Some(data) = self.data_of("cosmetic", id) else { return };
        if !cosmetic::problems(&data).is_empty() {
            return;
        }
        let slot = cosmetic::slot(&data);
        let body = player.as_entity();
        let list = cosmetic::parse(&body.get_string(TAG, cosmetic::KEY).await.unwrap_or_default());
        let next = cosmetic::after(&list, &slot, id, mode);
        let wears = next.iter().any(|(one, worn)| *one == slot && worn == id);
        let permission = cosmetic::permission(&data);
        if wears && !permission.is_empty() && !player.has_permission(permission).await {
            return;
        }
        if next.is_empty() {
            body.remove_custom_data(TAG, cosmetic::KEY);
        } else {
            body.set_string(TAG, cosmetic::KEY, &cosmetic::write(&next)).await;
        }
    }

    /// When the add-on stops: our NPCs out of the worlds, variables saved.
    pub(super) async fn stop_play(&mut self, server: &Server) {
        self.stop_steps().await;
        for (_, slots) in self.play.cosmetics_shown.drain() {
            slots.into_values().for_each(|(_, _, display)| display.remove());
        }
        let manager = server.get_recipe_manager();
        for (_, (key, _)) in self.play.recipes.drain() {
            manager.remove_recipe(&key);
        }
        for key in self.play.advancements.drain() {
            server.unregister_advancement(&key);
        }
        self.play.npcs.clear();
        self.play.npc_ids.clear();
        self.stop_mobs();
        self.play.models.stop();
        if let Some(blocks) = self.blocks.as_mut() {
            blocks.unfreeze_all(server);
        }
        for (_, found) in tagged_npcs(server).await {
            found.into_iter().for_each(|one| one.remove());
        }
        self.save_vars();
    }

    /// Traces since last asked, for `studio/live/out.jsonl`.
    pub fn take_traces(&mut self) -> Vec<String> {
        std::mem::take(&mut self.play.traces)
    }

    /// Once a second: a full armor set (`equip.set`) gives its effects; ones we gave that no set gives now go.
    async fn wear_sets(&mut self, server: &Server) {
        use pumpkin_plugin_api::wit::pumpkin::plugin::status_effect::StatusEffectInstance;
        use pumpkin_plugin_api::world::EquipmentSlot;
        let sets = studio_format::item::armor_sets(self.active_rows("item"));
        if sets.is_empty() && self.play.set_given.is_empty() {
            return;
        }
        let seconds = studio_format::item::SET_EFFECT_SECONDS;
        for player in server.get_all_players() {
            let mut worn = Vec::new();
            if let Some(living) = player.as_entity().as_living() {
                for slot in [EquipmentSlot::Head, EquipmentSlot::Chest, EquipmentSlot::Legs, EquipmentSlot::Feet] {
                    if let Some(stack) = living.get_equipment(slot)
                        && let Some(id) = stack.get_string(TAG, "item").await
                    {
                        worn.push(id);
                    }
                }
            }
            let mut wanted: HashMap<String, u32> = HashMap::new();
            for set in sets.values().filter(|set| set.complete(&worn)) {
                for effect in &set.effects {
                    let level = wanted.entry(effect.effect.clone()).or_insert(0);
                    *level = (*level).max(effect.level);
                }
            }
            let who = (player.get_id().high, player.get_id().low);
            for name in self.play.set_given.remove(&who).unwrap_or_default() {
                let Some(kind) = super::mobs::status_effect(&name) else { continue };
                if !wanted.contains_key(&name) && player.get_effect(kind).is_some_and(|now| now.ambient && now.duration <= seconds * 20) {
                    player.remove_effect(kind);
                }
            }
            for (name, level) in &wanted {
                let Some(kind) = super::mobs::status_effect(name) else { continue };
                player.add_effect(StatusEffectInstance { effect_type: kind, duration: seconds * 20, amplifier: (*level).clamp(1, 255) as u8 - 1, ambient: true, show_particles: false, show_icon: true }).await;
            }
            if !wanted.is_empty() {
                self.play.set_given.insert(who, wanted.into_keys().collect());
            }
        }
    }

    pub(super) fn data_of(&self, kind: &str, id: &str) -> Option<Data> {
        self.active_rows(kind).into_iter().find(|(name, _)| *name == id).map(|(_, data)| data.clone())
    }

    pub(super) fn next_random(&mut self) -> u64 {
        let state = &mut self.play.random;
        if *state == 0 {
            *state = 0x2545_f491_4f6c_dd1d ^ crate::now_ms();
        }
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    // ---------- text ----------

    /// Text with our placeholders, the run's extras and `<var:name>` / `<pvar:name>`.
    /// Like `text_for`, but the filled MiniMessage itself (a canvas measures and lays it out).
    fn mini_for(&mut self, server: &Server, text: &str, run: &Run) -> String {
        let filled = self.fill_vars(text, run);
        let player = run.player.and_then(|id| server.get_player_by_uuid(id));
        let who = player.as_ref().map(crate::who_of);
        let mut all: Vec<(String, String)> = values(server, who.as_ref()).into_iter().map(|(key, value)| (key.to_string(), value)).collect();
        all.extend(run.extra.iter().map(|(key, value)| (key.clone(), value.clone())));
        let borrowed: Vec<(&str, String)> = all.iter().map(|(key, value)| (key.as_str(), value.clone())).collect();
        studio_format::mini::fill(&super::canvas::swap(&filled), &borrowed)
    }

    fn text_for(&mut self, server: &Server, text: &str, run: &Run) -> TextComponent {
        let filled = self.fill_vars(text, run);
        let player = run.player.and_then(|id| server.get_player_by_uuid(id));
        let who = player.as_ref().map(crate::who_of);
        let mut all: Vec<(String, String)> = values(server, who.as_ref()).into_iter().map(|(key, value)| (key.to_string(), value)).collect();
        all.extend(run.extra.iter().map(|(key, value)| (key.clone(), value.clone())));
        let borrowed: Vec<(&str, String)> = all.iter().map(|(key, value)| (key.as_str(), value.clone())).collect();
        render(&filled, &borrowed)
    }

    /// The same as plain text, for commands.
    fn plain_for(&mut self, server: &Server, text: &str, run: &Run) -> String {
        let mut out = self.fill_vars(text, run);
        let player = run.player.and_then(|id| server.get_player_by_uuid(id));
        let who = player.as_ref().map(crate::who_of);
        for (key, value) in values(server, who.as_ref()) {
            out = out.replace(&format!("<{key}>"), &value);
        }
        for (key, value) in &run.extra {
            out = out.replace(&format!("<{key}>"), value);
        }
        out
    }

    /// `<var:name>` and, with a player, `<pvar:name>` filled in: for boards, tab lists, boss bars and holograms.
    pub(super) fn vars_in(&mut self, text: &str, player: Option<&Player>) -> String {
        let run = match player {
            Some(player) => Run::of(player, Vec::new()),
            None => Run::server(Vec::new()),
        };
        self.fill_vars(text, &run)
    }

    fn fill_vars(&mut self, text: &str, run: &Run) -> String {
        if !text.contains("var:") {
            return text.to_string();
        }
        self.load_vars();
        let mut out = String::new();
        let mut rest = text;
        while let Some(open) = rest.find('<') {
            let Some(close) = rest[open..].find('>').map(|at| open + at) else { break };
            let tag = &rest[open + 1..close];
            let value = if let Some(name) = tag.strip_prefix("var:") {
                Some(show_value(self.play.vars_server.get(name)))
            } else if let Some(name) = tag.strip_prefix("pvar:") {
                run.player.map(|id| show_value(self.play.vars_players.get(&id_text(id)).and_then(|vars| vars.get(name))))
            } else {
                None
            };
            match value {
                Some(value) => {
                    out.push_str(&rest[..open]);
                    out.push_str(&value);
                }
                None => out.push_str(&rest[..=close]),
            }
            rest = &rest[close + 1..];
        }
        out.push_str(rest);
        out
    }

    // ---------- conditions ----------

    /// True when every condition holds (an empty list always does).
    pub(super) async fn test(&mut self, server: &Server, conditions: Option<&Value>, run: &Run) -> bool {
        let Some(Value::Array(list)) = conditions else { return true };
        for one in list.iter().filter_map(Value::as_object) {
            let result = self.check(server, one, run).await;
            if (one.get("negate").and_then(Value::as_bool) == Some(true)) == result {
                return false;
            }
        }
        true
    }

    async fn check(&mut self, server: &Server, one: &Data, run: &Run) -> bool {
        let player = run.player.and_then(|id| server.get_player_by_uuid(id));
        match fields::text(one, "type", "").as_str() {
            "permission" => match &player {
                Some(player) => player.has_permission(fields::text(one, "permission", "")).await,
                None => false,
            },
            "world" => {
                let worlds: Vec<String> = fields::lines(one, "worlds").into_iter().filter(|one| !one.trim().is_empty()).collect();
                player.is_some_and(|player| worlds.is_empty() || worlds.contains(&crate::who_of(&player).world))
            }
            "variable" => {
                self.load_vars();
                let name = fields::text(one, "variable", "");
                let left = if fields::text(one, "scope", "player") == "server" {
                    self.play.vars_server.get(&name).cloned()
                } else {
                    run.player.and_then(|id| self.play.vars_players.get(&id_text(id)).and_then(|vars| vars.get(&name)).cloned())
                };
                let right = self.plain_for(server, &fields::scalar(one, "value", ""), run);
                compare(left.as_ref(), &fields::text(one, "op", "="), &right)
            }
            "chance" => (self.next_random() % 10_000) as f64 / 100.0 < number_of(one.get("percent"), 50.0),
            "online" => compare(Some(&json!(server.get_player_count())), &fields::text(one, "op", ">="), &fields::scalar(one, "value", "1")),
            "holding" => match &player {
                Some(player) => holding(player, &fields::text(one, "item", "")).await,
                None => false,
            },
            "advancement" => player.is_some_and(|player| player.has_advancement(&advancement_key(&fields::text(one, "advancement", "")))),
            "quest" => match &player {
                Some(player) => self.quest_state(player, fields::text(one, "quest", "").trim(), &fields::text(one, "state", "active")).await,
                None => false,
            },
            // Vault money is Paper only.
            "money" => false,
            "has_item" | "gamemode" | "health" | "level" | "sneaking" | "time" | "weather" | "group" => self.more_check(server, one, run, player.as_ref()).await,
            // Unknown ones pass. That includes M6 conditions other plugins add ("plugin:name"): Pumpkin plugins can't
            // add them yet (said in the file's status), so they don't stop the run.
            _ => true,
        }
    }

    // ---------- actions ----------

    /// Runs the actions in order; a `wait` puts the rest off. Returns the types it started.
    pub async fn run_actions(&mut self, server: &Server, actions: Option<&Value>, run: &Run) -> Vec<String> {
        let listed: Vec<Value> = actions.and_then(Value::as_array).cloned().unwrap_or_default();
        // Functions go in place of their step now, so a wait inside one puts the rest of the whole run off.
        let steps = studio_format::function::expand(&listed, &|id| self.data_of("function", id).map(|data| studio_format::function::actions(&data)), MOST_STEPS);
        let started = steps.iter().map(|one| one.get("type").and_then(Value::as_str).unwrap_or("?").to_string()).collect();
        self.continue_at(server, steps, run, 0).await;
        started
    }

    /// Boxed: actions can run actions (skills, menus), so this future is recursive.
    fn continue_at<'a>(&'a mut self, server: &'a Server, steps: Vec<Value>, run: &'a Run, waited: u64) -> pumpkin_plugin_api::BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut steps = steps;
            let mut run = run.clone();
            let mut at = 0;
            let mut gone = 0;
            while at < steps.len() {
                gone += 1;
                if gone > MOST_RUN {
                    return;
                }
                let Some(one) = steps[at].as_object().cloned() else {
                    at += 1;
                    continue;
                };
                at += 1;
                let kind = fields::text(&one, "type", "");
                match kind.as_str() {
                    "wait" => {
                        let ticks = (number_of(one.get("ticks"), 20.0).round() as u64).clamp(1, MOST_WAIT);
                        if waited + ticks > MOST_WAIT {
                            return;
                        }
                        self.play.waiting.push(Waiting { due: self.ticks + ticks, steps: steps.split_off(at), run, waited: waited + ticks });
                        return;
                    }
                    // Flow: an if or a repeat puts its steps in front of the rest, so a wait inside one waits for all after it.
                    "if" | "repeat" => {
                        let mut next = Vec::new();
                        if kind == "if" {
                            let passed = self.test(server, one.get("if"), &run).await;
                            next = self.branch(one.get(if passed { "then" } else { "else" }));
                        } else {
                            let body = self.branch(one.get("actions"));
                            let times = fields::number(&one, "times", 3, 0, MOST_REPEATS);
                            for round in 1..=times {
                                if body.is_empty() || next.len() > MOST_RUN {
                                    break;
                                }
                                next.push(json!({ "type": ROUND, "index": round.to_string() }));
                                next.extend(body.iter().cloned());
                            }
                            if !body.is_empty() && times > 0 {
                                let was = run.extra.iter().find(|(key, _)| key == "index").map(|(_, value)| value.clone()).unwrap_or_default();
                                next.push(json!({ "type": ROUND, "index": was }));
                            }
                        }
                        next.extend(steps.split_off(at));
                        next.truncate(MOST_RUN);
                        steps = next;
                        at = 0;
                    }
                    ROUND => {
                        run.extra.retain(|(key, _)| key != "index");
                        run.extra.push(("index".into(), fields::text(&one, "index", "")));
                    }
                    "stop" => return,
                    "cancel" => self.play.cancel = true,
                    "each_player" => {
                        let body = self.branch(one.get("actions"));
                        if body.is_empty() {
                            continue;
                        }
                        for (each, _) in crate::players_of(server) {
                            let each_run = Run { player: Some(each.get_id()), name: each.get_name(), extra: run.extra.clone(), caster: run.caster, target: None };
                            self.continue_at(server, body.clone(), &each_run, waited).await;
                        }
                    }
                    REDRAW => {
                        let seen = one.get("seen").and_then(Value::as_u64);
                        if let Some(id) = run.player
                            && let Some(player) = server.get_player_by_uuid(id)
                            && self.play.dialog_shown.get(&id_text(id)).copied() == seen
                        {
                            self.show_dialog(server, &player, &fields::text(&one, "dialog", ""), None).await;
                        }
                    }
                    _ => self.step(server, &one, &run).await,
                }
            }
        })
    }

    /// The steps of an `if` branch or a loop body, functions put in place.
    fn branch(&self, list: Option<&Value>) -> Vec<Value> {
        let listed: Vec<Value> = list.and_then(Value::as_array).cloned().unwrap_or_default();
        studio_format::function::expand(&listed, &|id| self.data_of("function", id).map(|data| studio_format::function::actions(&data)), MOST_STEPS)
    }

    async fn step(&mut self, server: &Server, one: &Data, run: &Run) {
        let kind = fields::text(one, "type", "");
        if kind.contains(':') {
            // M6: an action another plugin adds. Pumpkin plugins can't add studio actions yet (said in the file's
            // status), so it's skipped.
            return;
        }
        let player = run.player.and_then(|id| server.get_player_by_uuid(id));
        match kind.as_str() {
            "message" => {
                if let Some(player) = &player {
                    let text = self.text_for(server, &fields::text(one, "text", ""), run);
                    player.send_system_message(text, false);
                }
            }
            "broadcast" => {
                for (each, _) in crate::players_of(server) {
                    let text = self.text_for(server, &fields::text(one, "text", ""), run);
                    each.send_system_message(text, false);
                }
            }
            "title" => {
                if let Some(player) = &player {
                    let stay = fields::number(one, "stayTicks", 60, 10, 20 * 30) as i32;
                    player.send_title_animation(5, stay, 5);
                    let subtitle = self.text_for(server, &fields::text(one, "subtitle", ""), run);
                    player.show_subtitle(subtitle);
                    let title = self.text_for(server, &fields::text(one, "title", ""), run);
                    player.show_title(title);
                }
            }
            "actionbar" => {
                if let Some(player) = &player {
                    let text = self.text_for(server, &fields::text(one, "text", ""), run);
                    player.show_actionbar(text);
                }
            }
            "sound" => {
                if let Some(player) = &player {
                    let sound = fields::text(one, "sound", "entity.experience_orb.pickup").to_ascii_lowercase();
                    let sound = if sound.contains(':') { sound } else { format!("minecraft:{sound}") };
                    player.play_custom_sound(&sound, SoundCategory::Master, number_of(one.get("volume"), 1.0) as f32, number_of(one.get("pitch"), 1.0) as f32);
                }
            }
            "command" => {
                let Some(line) = fields::command_line(&fields::text(one, "command", ""), &run.extra, |text| self.plain_for(server, text, run)) else {
                    if let Some(player) = &player {
                        player.send_system_message(TextComponent::text("That can't go into a command. Use letters, numbers, _ . and - only."), false);
                    }
                    return;
                };
                let line = line.trim().trim_start_matches('/').to_string();
                if line.is_empty() || line.contains('\n') {
                    return;
                }
                if fields::text(one, "as", "console") == "player" {
                    if let Some(player) = player {
                        server.execute_command(line, RunAs::Player(player)).await;
                    }
                } else {
                    server.execute_command(line, RunAs::Console).await;
                }
            }
            "teleport" => {
                if let Some(player) = &player {
                    let name = fields::text(one, "world", &crate::who_of(player).world);
                    if let Some(world) = world_named(server, &name) {
                        let at = (number_of(one.get("x"), 0.0), number_of(one.get("y"), 64.0), number_of(one.get("z"), 0.0));
                        let (yaw, pitch) = (Some(number_of(one.get("yaw"), 0.0) as f32), Some(number_of(one.get("pitch"), 0.0) as f32));
                        self.play.steps.moved(player, self.ticks);
                        // Within the same world a plain move; `teleport_world` would show a world change.
                        if name == crate::who_of(player).world {
                            player.teleport(at, yaw, pitch, world).await;
                        } else {
                            player.teleport_world(world, at, yaw, pitch).await;
                        }
                    }
                }
            }
            "give" => {
                if let Some(player) = &player {
                    let amount = fields::number(one, "amount", 1, 1, 64 * 36) as u32;
                    if let Some(stack) = self.stack(server, &fields::text(one, "item", ""), amount, run).await {
                        give(player, stack).await;
                    }
                }
            }
            "menu" => {
                if let Some(player) = &player {
                    self.open_menu(server, player, &fields::text(one, "menu", "")).await;
                }
            }
            "close" => {
                if let Some(player) = &player {
                    self.close_menu(player).await;
                }
            }
            "shop" => {
                if let Some(player) = &player {
                    self.open_shop(server, player, fields::text(one, "shop", "").trim()).await;
                }
            }
            "set" => self.set_var(server, one, run),
            "emote" => {
                let model = fields::text(one, "model", "").trim().to_string();
                if let Some(player) = &player
                    && let Some(data) = self.data_of("model", &model)
                {
                    let animation = fields::text(one, "animation", "emote").trim().to_string();
                    let root = self.root.clone();
                    if let Err(problem) = self.play.models.emote(&root, player, &model, &data, &animation).await {
                        self.problem("model", &model, &problem);
                    }
                }
            }
            "structure" => self.place_structure_action(server, player.as_ref(), one, run).await,
            "quest" => {
                if let Some(player) = &player {
                    self.quest_action(server, player, fields::text(one, "quest", "").trim(), &fields::text(one, "mode", "start"), run).await;
                }
            }
            "cosmetic" => {
                if let Some(player) = &player {
                    self.wear_cosmetic(player, fields::text(one, "cosmetic", "").trim(), &fields::text(one, "mode", "toggle")).await;
                }
            }
            "enchant" => {
                if let Some(player) = &player {
                    let level = fields::number(one, "level", 1, 0, 255) as u32;
                    self.enchant_held(server, player, fields::text(one, "enchant", "").trim(), level, run).await;
                }
            }
            "dialog" => {
                if let Some(player) = &player {
                    self.open_dialog(server, player, &fields::text(one, "dialog", "")).await;
                }
            }
            "advancement" => {
                if let Some(player) = &player {
                    let key = advancement_key(&fields::text(one, "advancement", ""));
                    if one.get("revoke").and_then(Value::as_bool) == Some(true) {
                        player.revoke_advancement(&key);
                    } else {
                        player.award_advancement(key).await;
                    }
                }
            }
            "animation" => {
                let model = fields::text(one, "model", "");
                let animation = fields::text(one, "animation", "");
                if !model.trim().is_empty() {
                    self.play.models.play_on(model.trim(), &animation);
                } else if let Some(mob) = run.caster {
                    // M5: in a skill, no model means this mob's own.
                    self.play.models.play_on_rider(mob, &animation);
                }
            }
            // M5: actions for mob skills (on rules, commands and items they work on the run's player).
            "damage" | "heal" | "potion" | "launch" | "particle" | "projectile" | "summon" => self.mob_action(server, &kind, one, run).await,
            "loot" => {
                if let Some(player) = &player
                    && let Some(table) = self.data_of("loot", &fields::text(one, "loot", "")) {
                        for stack in self.roll(server, &table, run).await {
                            give(player, stack).await;
                        }
                    }
            }
            "take" | "xp" | "gamemode" | "kick" | "block" | "fill" | "bossbar" | "food" | "health" | "set_permission" | "set_group" | "manage_world" | "region" => {
                self.more_step(server, &kind, one, run, player.as_ref()).await;
            }
            _ => {
                // Unknown actions are skipped, so newer files still half-work on an older add-on.
            }
        }
    }

    // ---------- variables ----------

    fn vars_file(&self) -> std::path::PathBuf {
        self.root.join("data").join("variables.json")
    }

    fn load_vars(&mut self) {
        if self.play.vars_loaded {
            return;
        }
        self.play.vars_loaded = true;
        let Ok(text) = fs::read_to_string(self.vars_file()) else { return };
        let Ok(Value::Object(all)) = serde_json::from_str::<Value>(&text) else { return };
        if let Some(Value::Object(server)) = all.get("server") {
            self.play.vars_server = server.clone();
        }
        if let Some(Value::Object(players)) = all.get("players") {
            for (id, vars) in players {
                if let Value::Object(vars) = vars {
                    self.play.vars_players.insert(id.clone(), vars.clone());
                }
            }
        }
    }

    fn save_vars(&mut self) {
        if !self.play.vars_dirty {
            return;
        }
        self.play.vars_dirty = false;
        let players: Map<String, Value> = self.play.vars_players.iter().map(|(id, vars)| (id.clone(), Value::Object(vars.clone()))).collect();
        let text = json!({ "server": self.play.vars_server, "players": players }).to_string();
        let file = self.vars_file();
        let _ = fs::create_dir_all(file.parent().unwrap_or(&self.root));
        let temporary = file.with_extension("json.tmp");
        if fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, &file)).is_err() {
            tracing::warn!("Could not write studio/data/variables.json");
        }
    }

    fn set_var(&mut self, server: &Server, one: &Data, run: &Run) {
        let name = fields::text(one, "variable", "");
        if name.trim().is_empty() || name.len() > 64 {
            return;
        }
        self.load_vars();
        let value = self.plain_for(server, &fields::scalar(one, "value", ""), run);
        let op = fields::text(one, "op", "set");
        let vars = if fields::text(one, "scope", "player") == "server" {
            &mut self.play.vars_server
        } else {
            let Some(id) = run.player else { return };
            self.play.vars_players.entry(id_text(id)).or_default()
        };
        let next = change(vars.get(&name), &op, &value);
        vars.insert(name, next);
        self.play.vars_dirty = true;
    }

    // ---------- items ----------

    /// A studio item (`studio:<id>`) or a plain one (`diamond`), `amount` of it.
    pub(super) async fn stack(&mut self, server: &Server, item: &str, amount: u32, run: &Run) -> Option<ItemStack> {
        // `studio:item/<id>` (the M5 equipment form) is the same item as `studio:<id>`.
        if let Some(id) = item.strip_prefix("studio:item/") {
            return Box::pin(self.stack(server, &format!("studio:{id}"), amount, run)).await;
        }
        let count = amount.clamp(1, 64) as u8;
        // A painting item that hangs one of ours (registered while the server started).
        if let Some(id) = item.strip_prefix("studio:painting/") {
            if !self.play.paintings.contains_key(id) {
                return None;
            }
            let stack = ItemStack::new(&item_key("painting")?, 1);
            let variant = NbtTree { root: 0, tags: vec![NbtTag::StringTag(format!("{TAG}:{id}"))] };
            stack.set_component_nbt(pumpkin_plugin_api::wit::pumpkin::plugin::data_components::DataComponent::PaintingVariant, variant).await.ok()?;
            stack.set_count(count.min(stack.get_max_count().await)).await;
            return Some(stack);
        }
        if let Some(reference) = item.strip_prefix("studio:")
            && let Some((kind, id)) = reference.split_once('/')
        {
            // M3: a block's or furniture piece's own item, or the studio item it's linked to.
            let own = self.with_blocks(async |me, blocks| {
                let me = RefCell::new(me);
                blocks.own_item(reference, &|text| me.borrow_mut().text_for(server, text, run)).await
            }).await;
            let Some(stack) = own else {
                let link = fields::text(&self.data_of(kind, id)?, "item", "").trim().to_string();
                return if link.is_empty() || link.contains('/') { None } else { Box::pin(self.stack(server, &format!("studio:{link}"), amount, run)).await };
            };
            let most = stack.get_max_count().await;
            stack.set_count(count.min(most)).await;
            return Some(stack);
        }
        if let Some(id) = item.strip_prefix("studio:") {
            let data = self.data_of("item", id)?;
            let stack = self.build_item(server, &data, run).await?;
            item_parts(&stack, id, &data).await;
            let enchants: Vec<(String, u32)> = studio_format::enchant::on_item(&data).into_iter().filter(|(one, _)| self.data_of("enchant", one).is_some()).collect();
            if !enchants.is_empty() {
                self.write_enchants(server, &stack, &enchants, run).await;
            }
            // `food.leftover`: what is left after eating it (a bowl), a plain or studio item.
            let leftover = studio_format::item::part(&data, "food").map(|food| fields::text(food, "leftover", "").trim().to_string()).unwrap_or_default();
            if !leftover.is_empty() && self.play.leftover_depth < 4 {
                self.play.leftover_depth += 1;
                let left = Box::pin(self.stack(server, &leftover, 1, run)).await;
                self.play.leftover_depth -= 1;
                if left.is_some() {
                    stack.set_use_remainder(left).await;
                }
            }
            let most = stack.get_max_count().await;
            stack.set_count(count.min(most)).await;
            stack.set_string(TAG, "item", id).await;
            return Some(stack);
        }
        let key = item_key(item)?;
        let stack = ItemStack::new(&key, 1);
        let most = stack.get_max_count().await;
        stack.set_count(count.min(most)).await;
        Some(stack)
    }

    /// An item from its fields: material, name, lore and enchantments. A studio item's M1 look and components are
    /// added by [`Self::stack`] ([`item_parts`]; what Pumpkin can't take is in the file's status, [`item_problem`]).
    async fn build_item(&mut self, server: &Server, data: &Data, run: &Run) -> Option<ItemStack> {
        let key = item_key(&fields::text(data, "material", "stone"))?;
        let stack = ItemStack::new(&key, 1);
        if fields::has(data, "name") {
            let name = self.text_for(server, &fields::text(data, "name", ""), run);
            stack.set_custom_name(Some(name)).await;
        }
        let lore: Vec<TextComponent> = fields::lines(data, "lore").iter().map(|line| self.text_for(server, line, run)).collect();
        if !lore.is_empty() {
            stack.set_lore(lore).await;
        }
        if let Some(Value::Object(enchantments)) = data.get("enchantments") {
            for (name, level) in enchantments {
                let clean = name.trim().to_ascii_lowercase();
                if let Some(enchantment) = names::enchantment(clean.strip_prefix("minecraft:").unwrap_or(&clean)) {
                    stack.add_enchantment(enchantment, number_of(Some(level), 1.0).clamp(1.0, 255.0) as u32).await;
                }
            }
        }
        Some(stack)
    }

    /// The studio item in either hand and its action list `key`, when it has one.
    async fn held_list(&self, player: &Player, key: &str) -> Option<(String, Value)> {
        for hand in [Hand::Right, Hand::Left] {
            let Some(stack) = player.get_item_in_hand(hand) else { continue };
            let Some(id) = stack.get_string(TAG, "item").await else { continue };
            let Some(data) = self.data_of("item", &id) else { continue };
            if let Some(list) = data.get(key).filter(|one| one.as_array().is_some_and(|list| !list.is_empty())) {
                return Some((id, list.clone()));
            }
            if matches!(hand, Hand::Right) && key != "eatActions" {
                return None;
            }
        }
        None
    }

    /// A studio item was eaten (or drunk): its `eatActions`.
    pub async fn ate_item(&mut self, server: &Server, player: &Player) {
        if let Some((id, list)) = self.held_list(player, "eatActions").await {
            self.run_actions(server, Some(&list), &Run::of(player, vec![("item".into(), id)])).await;
        }
    }

    /// A player hit something with a studio item in the main hand: its `hitActions` (Paper's `onHit`: only a
    /// player hitting directly, not their arrows). `target` is the player's name or the entity type.
    pub async fn hit_with_item(&mut self, server: &Server, victim: i32, damager: i32) {
        let Some(player) = server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == damager as u32) else { return };
        let Some(stack) = player.get_item_in_hand(Hand::Right) else { return };
        let target = match server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == victim as u32) {
            Some(hit) => hit.get_name(),
            None => self
                .entity_by_id(server, victim as u32)
                .map(|entity| crate::map::biome_name(&format!("{:?}", entity.get_type())).trim_start_matches("minecraft:").to_string())
                .unwrap_or_default(),
        };
        self.fire_enchants(server, &player, &stack, "hitActions", vec![("target".into(), target.clone())]).await;
        let Some(id) = stack.get_string(TAG, "item").await else { return };
        let Some(list) = self.data_of("item", &id).and_then(|data| data.get("hitActions").filter(|one| one.as_array().is_some_and(|list| !list.is_empty())).cloned()) else { return };
        self.run_actions(server, Some(&list), &Run::of(&player, vec![("item".into(), id), ("target".into(), target)])).await;
    }

    /// A player was hurt: the studio enchantments on what they wear run their `hurtActions`.
    pub async fn player_hurt(&mut self, server: &Server, entity_id: i32) {
        use pumpkin_plugin_api::world::EquipmentSlot;
        if self.active_rows("enchant").is_empty() {
            return;
        }
        let Some(player) = server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == entity_id as u32) else { return };
        let Some(living) = player.as_entity().as_living() else { return };
        for slot in [EquipmentSlot::Head, EquipmentSlot::Chest, EquipmentSlot::Legs, EquipmentSlot::Feet] {
            if let Some(piece) = living.get_equipment(slot) {
                self.fire_enchants(server, &player, &piece, "hurtActions", Vec::new()).await;
            }
        }
    }

    /// The studio enchantments on `stack` run their action list `key` (each by its chance).
    async fn fire_enchants(&mut self, server: &Server, player: &Player, stack: &ItemStack, key: &str, extra: Vec<(String, String)>) {
        use studio_format::enchant;
        let Some(text) = stack.get_string(TAG, enchant::KEY_ENCHANTS).await else { return };
        for (id, level) in enchant::parse(&text) {
            let Some(data) = self.data_of("enchant", &id) else { continue };
            let Some(list) = data.get(key).filter(|one| one.as_array().is_some_and(|list| !list.is_empty())).cloned() else { continue };
            if (self.next_random() % 10_000) as f64 / 10_000.0 >= enchant::chance(&data) {
                continue;
            }
            let mut more = extra.clone();
            more.push(("enchant".into(), id.clone()));
            more.push(("level".into(), level.to_string()));
            self.run_actions(server, Some(&list), &Run::of(player, more)).await;
        }
    }

    /// Writes a stack's studio enchantments and puts one lore line each first, in place of the old ones.
    async fn write_enchants(&mut self, server: &Server, stack: &ItemStack, list: &[(String, u32)], run: &Run) {
        use studio_format::enchant;
        let had = stack.get_int(TAG, enchant::KEY_LINES).await.unwrap_or(0).max(0) as usize;
        let mut lore = stack.get_lore().await;
        lore.drain(..had.min(lore.len()));
        let mut lines = Vec::new();
        for (id, level) in list {
            if let Some(data) = self.data_of("enchant", id) {
                lines.push(self.text_for(server, &enchant::lore_line(&data, id, *level), run));
            }
        }
        let count = lines.len();
        lines.extend(lore);
        stack.set_lore(lines).await;
        if list.is_empty() {
            stack.remove_custom_data(TAG.to_string(), enchant::KEY_ENCHANTS.to_string()).await;
            stack.remove_custom_data(TAG.to_string(), enchant::KEY_LINES.to_string()).await;
        } else {
            stack.set_string(TAG, enchant::KEY_ENCHANTS, &enchant::write(list)).await;
            stack.set_int(TAG, enchant::KEY_LINES, count as i32).await;
        }
    }

    /// The `enchant` action: one studio enchantment on the held item (level 0 takes it off), when it fits.
    async fn enchant_held(&mut self, server: &Server, player: &Player, id: &str, level: u32, run: &Run) {
        use studio_format::enchant;
        let Some(data) = self.data_of("enchant", id) else { return };
        let Some(held) = player.get_item_in_hand(Hand::Right) else { return };
        if !enchant::fits(&data, &held.get_registry_key().await) {
            return;
        }
        let list = enchant::parse(&held.get_string(TAG, enchant::KEY_ENCHANTS).await.unwrap_or_default());
        let next = enchant::with(&list, id, level.min(enchant::max_level(&data)));
        self.write_enchants(server, &held, &next, run).await;
        player.set_item_in_hand(Hand::Right, Some(held)).await;
    }

    /// A block was broken with a studio item in the main hand: its `breakActions`.
    pub async fn broke_with_item(&mut self, server: &Server, player: &Player, block: &str) {
        let name = block.trim_start_matches("minecraft:").split('[').next().unwrap_or_default().to_string();
        if let Some(stack) = player.get_item_in_hand(Hand::Right) {
            self.fire_enchants(server, player, &stack, "mineActions", vec![("block".into(), name.clone())]).await;
        }
        if let Some((id, list)) = self.held_list(player, "breakActions").await {
            self.run_actions(server, Some(&list), &Run::of(player, vec![("item".into(), id), ("block".into(), name)])).await;
        }
    }

    /// The file of the studio item in the main hand, for digging our blocks with its own tool rules.
    async fn held_item_data(&self, player: &Player) -> Option<Data> {
        let id = player.get_item_in_hand(Hand::Right)?.get_string(TAG, "item").await?;
        self.data_of("item", &id)
    }

    /// A right click with a studio item that has use actions runs them. Returns true to cancel the click.
    pub async fn used_item(&mut self, server: &Server, player: &Player) -> bool {
        let Some(hand) = player.get_item_in_hand(Hand::Right) else { return false };
        let Some(id) = hand.get_string(TAG, "item").await else { return false };
        let Some(data) = self.data_of("item", &id) else { return false };
        let actions = data.get("useActions").cloned();
        if actions.as_ref().and_then(Value::as_array).is_none_or(Vec::is_empty) {
            return false;
        }
        self.run_actions(server, actions.as_ref(), &Run::of(player, vec![("item".into(), id)])).await;
        true
    }

    // ---------- blocks and furniture (M3) ----------

    /// Runs `work` with the blocks taken out, so a decision can name items with the studio's text.
    async fn with_blocks<T: Default>(&mut self, work: impl AsyncFnOnce(&mut Self, &mut super::blocks::Blocks) -> T) -> T {
        let Some(mut blocks) = self.blocks.take() else { return T::default() };
        let out = work(self, &mut blocks).await;
        self.blocks = Some(blocks);
        out
    }

    /// Does what a block or furniture decision asks: its actions, and items (own, linked, loot) to the player or
    /// into `drops`. Returns whether to cancel the event.
    async fn carry_out(&mut self, server: &Server, player: Option<&Player>, out: Outcome, drops: Option<&mut Vec<ItemStack>>) -> bool {
        let run = player.map(|one| Run::of(one, out.extra.clone())).unwrap_or_else(|| Run::server(out.extra.clone()));
        if out.actions.is_some() {
            self.run_actions(server, out.actions.as_ref(), &run).await;
        }
        let mut items = out.items;
        if let Some(link) = &out.studio_item {
            items.extend(self.stack(server, &format!("studio:{link}"), 1, &run).await);
        }
        let tables: Vec<_> = out.loot.iter().filter_map(|id| self.data_of("loot", id)).collect();
        for table in tables {
            let rolled = self.roll(server, &table, &run).await;
            items.extend(rolled);
        }
        match (drops, &out.drop_at, player) {
            (Some(list), _, _) => list.extend(items),
            (None, Some((world, at)), _) => {
                for one in items {
                    world.drop_item_naturally(*at, one).await;
                }
            }
            (None, None, Some(player)) => {
                for one in items {
                    give(player, one).await;
                }
            }
            (None, None, None) => {}
        }
        out.cancel
    }

    /// A right click on a block: our block's use actions, or placing a held studio block or piece. True = ours.
    pub async fn block_clicked(&mut self, server: &Server, player: &Player) -> bool {
        let run = Run::of(player, Vec::new());
        let out = self.with_blocks(async |me, blocks| {
            let me = RefCell::new(me);
            blocks.right_click(player, &|text| me.borrow_mut().text_for(server, text, &run)).await
        }).await;
        self.carry_out(server, Some(player), out, None).await
    }

    /// A block is breaking: ours runs its break actions (its drops come with the drop event).
    pub async fn block_breaking(&mut self, server: &Server, player: Option<&Player>, at: (i32, i32, i32)) {
        let Some(world) = player.map(|one| one.as_entity().get_world()) else { return };
        let item = match player {
            Some(one) => self.held_item_data(one).await,
            None => None,
        };
        let out = self.with_blocks(async |_, blocks| blocks.breaking(player, &world, at, item.as_ref()).await).await;
        self.carry_out(server, player, out, None).await;
    }

    /// The drop event of one of ours: its own item and loot instead of the borrowed block's.
    pub async fn studio_block_drops(&mut self, server: &Server, at: (i32, i32, i32), player: Option<&Player>, items: &mut Vec<ItemStack>) {
        let run = player.map(|one| Run::of(one, Vec::new())).unwrap_or_default();
        let out = self.with_blocks(async |me, blocks| {
            let me = RefCell::new(me);
            blocks.drops(at, &|text| me.borrow_mut().text_for(server, text, &run)).await
        }).await;
        if out.cancel {
            items.clear();
        }
        self.carry_out(server, player, Outcome { cancel: false, ..out }, Some(items)).await;
    }

    /// A fork drops callback for one of ours that no break event came before (an explosion): drop it like a break.
    pub fn block_dropping(&mut self, at: (i32, i32, i32), state: u16) {
        if let Some(blocks) = self.blocks.as_mut() {
            blocks.dropping_without_break(at, state);
        }
    }

    /// The fork's random tick on one of our crops.
    pub async fn crop_tick(&mut self, world: &pumpkin_plugin_api::world::World, at: (i32, i32, i32), state: u16) {
        if let Some(blocks) = self.blocks.as_mut() {
            blocks.crop_tick(world, at, state).await;
        }
    }

    /// Whether bone meal works on one of our crops now.
    pub fn crop_can_bonemeal(&self, state: u16) -> bool {
        self.blocks.as_ref().is_some_and(|blocks| blocks.can_bonemeal(state))
    }

    /// Bone meal used on one of our crops.
    pub async fn crop_bonemeal(&mut self, world: &pumpkin_plugin_api::world::World, at: (i32, i32, i32), state: u16) {
        if let Some(blocks) = self.blocks.as_mut() {
            blocks.bonemeal(world, at, state).await;
        }
    }

    /// A plain note block placed by a player goes to its plain state, so it never looks like one of ours.
    pub async fn block_placed(&self, player: &Player, at: (i32, i32, i32), block: &str) {
        if let Some(blocks) = &self.blocks {
            blocks.placed_plain(&player.as_entity().get_world(), at, block).await;
        }
    }

    /// Physics, notes and pistons leave our blocks alone.
    pub fn block_held(&self, world: &pumpkin_plugin_api::world::World, at: (i32, i32, i32)) -> bool {
        self.blocks.as_ref().is_some_and(|blocks| blocks.holds(world, at))
    }

    /// A plain note block's physics is held too: powering or retuning it would give it one of our states. It still
    /// plays when redstone reaches it.
    pub async fn plain_note_held(&self, world: &pumpkin_plugin_api::world::World, at: (i32, i32, i32)) -> bool {
        let Some(blocks) = &self.blocks else { return false };
        if !blocks.plain_note_at(world, at) {
            return false;
        }
        blocks.powered_plain(world, at).await;
        true
    }

    /// A note at `at`: ours stay quiet, a plain one plays its own kept note. Returns whether to cancel the game's.
    pub async fn note_played(&self, world: &pumpkin_plugin_api::world::World, at: (i32, i32, i32)) -> bool {
        let Some(blocks) = self.blocks.as_ref() else { return false };
        if blocks.holds(world, at) {
            return true;
        }
        if blocks.plain_note_at(world, at) {
            super::blocks::Blocks::play_plain(world, at).await;
            return true;
        }
        false
    }

    pub async fn dig_started(&mut self, player: &Player, at: (i32, i32, i32)) {
        let item = self.held_item_data(player).await;
        if let Some(blocks) = &mut self.blocks {
            blocks.dig_started(player, at, item.as_ref()).await;
        }
    }

    pub fn dig_stopped(&mut self, player: &Player) {
        if let Some(blocks) = &mut self.blocks {
            blocks.dig_stopped(player);
        }
    }

    /// An entity got off another (the fork's dismount event): our furniture seats go on the next tick.
    pub fn stood_up(&mut self, rider_id: i32, seat_id: i32) {
        if let Some(blocks) = &mut self.blocks {
            blocks.stood_up(rider_id, seat_id);
        }
    }

    /// Every server tick: cracks on the blocks being dug, and the ones done break (like a player broke them).
    pub async fn tick_digging(&mut self, server: &Server) {
        let done = match &mut self.blocks {
            Some(blocks) => {
                blocks.tick_seats(server).await;
                blocks.tick_digging(server)
            }
            None => return,
        };
        for (player, world, at) in done {
            self.break_own(server, &player, &world, at).await;
        }
    }

    /// One of ours dug out on our clock: what a break event and its drop event would do, then the block goes.
    async fn break_own(&mut self, server: &Server, player: &Player, world: &pumpkin_plugin_api::world::World, at: (i32, i32, i32)) {
        let block = pumpkin_plugin_api::world::block_state_to_info(world.get_block_state_id(pumpkin_plugin_api::common::BlockPos { x: at.0, y: at.1, z: at.2 })).map(|info| info.name).unwrap_or_default();
        self.block_broken(at, &block);
        self.block_breaking(server, Some(player), at).await;
        // Experience only when it drops (harvested, see `Blocks::breaking`).
        let exp = self.blocks.as_ref().and_then(|blocks| blocks.exp_if_dropping(at));
        let name = block.trim_start_matches("minecraft:").split('[').next().unwrap_or_default().to_string();
        self.fire(server, "break", Some(player), vec![("block".into(), name)], |when| {
            let wanted: Vec<String> = fields::lines(when, "blocks").iter().filter(|one| !one.trim().is_empty()).map(|one| clean_name(one)).collect();
            wanted.is_empty() || wanted.contains(&clean_name(&block))
        }).await;
        self.broke_with_item(server, player, &block).await;
        if let Some(blocks) = &self.blocks {
            blocks.clear(world, at).await;
        }
        let mut items = Vec::new();
        self.studio_block_drops(server, at, Some(player), &mut items).await;
        self.block_drops(server, at, Some(player), &mut items).await;
        let spot = (f64::from(at.0), f64::from(at.1), f64::from(at.2));
        for one in items {
            world.drop_item_naturally(spot, one).await;
        }
        if let Some(exp) = exp {
            world.spawn_experience_orb((spot.0 + 0.5, spot.1 + 0.5, spot.2 + 0.5), exp).await;
        }
    }

    /// A click on a furniture piece: hitting breaks it, right-clicking sits and runs its use actions. True = ours.
    pub async fn piece_clicked(&mut self, server: &Server, player: &Player, entity_id: i32, attack: bool) -> bool {
        let run = Run::of(player, Vec::new());
        let out = self.with_blocks(async |me, blocks| {
            if attack {
                let me = RefCell::new(me);
                blocks.hit_piece(player, entity_id, &|text| me.borrow_mut().text_for(server, text, &run)).await
            } else {
                blocks.use_piece(player, entity_id).await
            }
        }).await;
        self.carry_out(server, Some(player), out, None).await
    }

    /// A middle click on one of our blocks (`state`) or furniture pieces (`entity_id`): the item to pick, the one
    /// breaking it gives. The fork then finds it in the inventory, or gives it in creative.
    pub async fn picked(&mut self, server: &Server, player: &Player, state: Option<u16>, entity_id: Option<i32>) -> Option<ItemStack> {
        let run = Run::of(player, Vec::new());
        let out = self.with_blocks(async |me, blocks| {
            let me = RefCell::new(me);
            let name = |text: &str| me.borrow_mut().text_for(server, text, &run);
            match (state, entity_id) {
                (Some(state), _) => blocks.pick_block(state, &name).await,
                (None, Some(entity_id)) => blocks.pick_piece(player, entity_id, &name).await,
                (None, None) => None,
            }
        }).await?;
        let mut items = out.items;
        if let Some(link) = &out.studio_item {
            items.extend(self.stack(server, &format!("studio:{link}"), 1, &run).await);
        }
        items.into_iter().next()
    }

    // ---------- recipes ----------

    /// Gives Pumpkin the studio recipes: new and changed ones are (re)registered (the same key replaces), removed
    /// ones taken back. Online players get the change at the end of the tick.
    async fn register_recipes(&mut self, server: &Server) {
        let rows: Vec<(String, Data)> = self.active_rows("recipe").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        let manager = server.get_recipe_manager();
        let run = Run::default();
        let wanted: HashSet<String> = rows.iter().map(|(id, _)| id.clone()).collect();
        let gone: Vec<String> = self.play.recipes.keys().filter(|id| !wanted.contains(*id)).cloned().collect();
        for id in gone {
            if let Some((key, _)) = self.play.recipes.remove(&id) {
                manager.remove_recipe(&key);
            }
        }
        for (id, data) in rows {
            // The studio items it uses count too: an exact ingredient is the whole item.
            let mut text = Value::Object(data.clone()).to_string();
            for used in studio_refs(&Value::Object(data.clone())) {
                if let Some(item) = self.data_of("item", used.strip_prefix("item/").unwrap_or(&used)) {
                    text.push_str(&Value::Object(item).to_string());
                }
            }
            if self.play.recipes.get(&id).is_some_and(|(_, was)| *was == text) {
                continue;
            }
            let key = format!("{TAG}:recipe_{}", id.replace('-', "_"));
            let Some(output) = self.recipe_result(server, data.get("result"), &run).await else {
                self.drop_recipe(&manager, &id);
                self.problem("recipe", &id, "\"result\" must be an item or a studio item.");
                continue;
            };
            let kind = fields::text(&data, "type", "shaped");
            let experience = fields::number(&data, "experience", 0, 0, 1000) as f32;
            let cooking_time = fields::number(&data, "cookTicks", 200, 1, 20 * 60 * 10) as u32;
            let made: Result<(), String> = match kind.as_str() {
                "shaped" => {
                    let shape = fields::lines(&data, "shape");
                    let keys = object(&data, "keys").clone();
                    let mut key_list = Vec::new();
                    let mut problem = None;
                    let used: HashSet<char> = shape.iter().flat_map(|row| row.chars()).filter(|letter| *letter != ' ').collect();
                    for letter in used {
                        let name = keys.get(&letter.to_string()).and_then(Value::as_str).map(str::to_string);
                        let found = match name {
                            Some(name) => self.ingredient(server, &name).await,
                            None => None,
                        };
                        match found {
                            Some(found) => key_list.push((letter.to_string(), found)),
                            None => problem = Some(format!("\"keys\" has no item for {letter}.")),
                        }
                    }
                    if shape.is_empty() || shape.len() > 3 || shape.iter().any(|row| row.is_empty() || row.chars().count() > 3) {
                        Err("\"shape\" needs 1 to 3 rows of 1 to 3 letters.".into())
                    } else if let Some(problem) = problem {
                        Err(problem)
                    } else {
                        manager.register_shaped(key.clone(), ShapedRecipe { pattern: shape, key: key_list, output, group: None, category: None, show_notification: None }).await.map_err(refused)
                    }
                }
                "shapeless" => {
                    let wanted: Vec<String> = fields::lines(&data, "ingredients").into_iter().filter(|one| !one.trim().is_empty()).collect();
                    let mut ingredients: Vec<Ingredient> = Vec::new();
                    for one in &wanted {
                        if let Some(found) = self.ingredient(server, one).await {
                            ingredients.push(found);
                        }
                    }
                    if wanted.is_empty() || wanted.len() > 9 || ingredients.len() != wanted.len() {
                        Err("\"ingredients\" needs 1 to 9 items or studio items.".into())
                    } else {
                        manager.register_shapeless(key.clone(), ShapelessRecipe { ingredients, output, group: None, category: None }).await.map_err(refused)
                    }
                }
                "furnace" | "blasting" | "smoking" | "campfire" => match self.ingredient(server, &fields::text(&data, "input", "")).await {
                    Some(input) => {
                        let station = match kind.as_str() {
                            "blasting" => CookingType::Blasting,
                            "smoking" => CookingType::Smoking,
                            "campfire" => CookingType::Campfire,
                            _ => CookingType::Smelting,
                        };
                        manager.register_cooking(key.clone(), station, CookingRecipe { ingredient: input, output, experience, cooking_time, group: None, category: None }).await.map_err(refused)
                    }
                    None => Err("\"input\" must be an item or a studio item.".into()),
                },
                "stonecutting" => match self.ingredient(server, &fields::text(&data, "input", "")).await {
                    Some(ingredient) => {
                        manager.register_stonecutting(key.clone(), StonecuttingRecipe { ingredient, output, group: None }).await.map_err(refused)
                    }
                    None => Err("\"input\" must be an item or a studio item.".into()),
                },
                other => Err(format!("Unknown recipe type {other}.")),
            };
            match made {
                Ok(()) => {
                    self.play.recipes.insert(id, (key, text));
                }
                Err(problem) => {
                    self.drop_recipe(&manager, &id);
                    self.problem("recipe", &id, &problem);
                }
            }
        }
    }

    /// Takes back a recipe whose file is now wrong.
    fn drop_recipe(&mut self, manager: &pumpkin_plugin_api::recipe_wit::RecipeManager, id: &str) {
        if let Some((key, _)) = self.play.recipes.remove(id) {
            manager.remove_recipe(&key);
        }
    }

    /// A recipe ingredient: `#tag`, a plain item, or a studio item (matched with its components).
    async fn ingredient(&mut self, server: &Server, name: &str) -> Option<Ingredient> {
        let name = name.trim();
        if name.starts_with("studio:") {
            return self.stack(server, name, 1, &Run::default()).await.map(Ingredient::Exact);
        }
        if let Some(tag) = name.strip_prefix('#') {
            return Some(Ingredient::Tag(if tag.contains(':') { tag.to_string() } else { format!("minecraft:{tag}") }));
        }
        item_key(name).map(Ingredient::Item)
    }

    async fn recipe_result(&mut self, server: &Server, raw: Option<&Value>, run: &Run) -> Option<ItemStack> {
        let result = raw?.as_object()?;
        self.stack(server, &fields::text(result, "item", ""), fields::number(result, "amount", 1, 1, 64) as u32, run).await
    }

    // ---------- commands & menus ----------

    /// What changed in the studio commands since last asked: main names to take back (removed or changed), and
    /// commands to register (new or changed).
    pub fn command_changes(&mut self) -> (Vec<String>, Vec<NewCommand>) {
        let mut wanted: Vec<(Vec<String>, String)> = Vec::new();
        for (_, data) in self.active_rows("command") {
            let name = fields::text(data, "name", "").trim().to_ascii_lowercase();
            if valid_command(&name) {
                let mut names = vec![name];
                names.extend(fields::lines(data, "aliases").into_iter().map(|one| one.trim().to_ascii_lowercase()).filter(|one| valid_command(one)));
                wanted.push((names, fields::text(data, "description", "A studio command.")));
            }
        }
        for (id, data) in self.active_rows("menu") {
            let name = fields::text(data, "command", "").trim().to_ascii_lowercase();
            if valid_command(&name) {
                wanted.push((vec![name], format!("Opens the {id} menu.")));
            }
        }
        // A name goes to the first file that wants it.
        let mut taken = HashSet::new();
        let mut next: HashMap<String, (Vec<String>, String)> = HashMap::new();
        for (names, description) in wanted {
            let names: Vec<String> = names.into_iter().filter(|one| taken.insert(one.clone())).collect();
            if let Some(main) = names.first().cloned() {
                next.insert(main, (names, description));
            }
        }
        let gone: Vec<String> = self.play.registered.iter().filter(|(main, was)| next.get(*main) != Some(*was)).map(|(main, _)| main.clone()).collect();
        for main in &gone {
            self.play.registered.remove(main);
        }
        let mut fresh = Vec::new();
        for (main, one) in next {
            if self.play.registered.contains_key(&main) {
                continue;
            }
            fresh.push(NewCommand { names: one.0.clone(), description: one.1.clone() });
            self.play.registered.insert(main, one);
        }
        (gone, fresh)
    }

    /// Someone typed a studio command (`name` is the one Pumpkin matched, `args` what followed).
    pub async fn run_command(&mut self, server: &Server, name: &str, sender: &pumpkin_plugin_api::command::CommandSender, args: &str) -> Result<(), String> {
        let player = sender.as_player();
        if let Some((id, menu)) = self.active_rows("menu").into_iter().find(|(_, data)| fields::text(data, "command", "").trim().eq_ignore_ascii_case(name)).map(|(id, data)| (id.to_string(), data.clone())) {
            let _ = menu;
            let Some(player) = player else { return Err("Only players can open menus.".into()) };
            self.open_menu(server, &player, &id).await;
            return Ok(());
        }
        let found = self.active_rows("command").into_iter().find(|(_, data)| {
            fields::text(data, "name", "").trim().eq_ignore_ascii_case(name) || fields::lines(data, "aliases").iter().any(|one| one.trim().eq_ignore_ascii_case(name))
        });
        let Some((id, data)) = found.map(|(id, data)| (id.to_string(), data.clone())) else {
            return Err(format!("/{name} was removed from the studio."));
        };
        let permission = fields::text(&data, "permission", "");
        if !permission.trim().is_empty() && !sender.has_permission(permission.clone()).await {
            let text = fields::text(&data, "permissionMessage", "<red>You can't use this command.");
            sender.send_message(render(&text, &[]));
            return Ok(());
        }
        if player.is_none() && data.get("console").and_then(Value::as_bool) != Some(true) {
            return Err(format!("Only players can use /{name}."));
        }
        let cooldown = fields::number(&data, "cooldownSeconds", 0, 0, 24 * 60 * 60) as u64;
        if let (Some(player), true) = (&player, cooldown > 0) {
            let key = format!("command/{id}/{}", id_text(player.get_id()));
            let now = crate::now_ms();
            let until = self.play.cooldowns.get(&key).copied().unwrap_or(0);
            if now < until {
                let left = (until - now).div_ceil(1000);
                let text = fields::text(&data, "cooldownMessage", "<gray>Wait <seconds>s before using this again.").replace("<seconds>", &left.to_string());
                sender.send_message(render(&text, &[]));
                return Ok(());
            }
            self.play.cooldowns.insert(key, now + cooldown * 1000);
        }
        let words: Vec<&str> = args.split_whitespace().collect();
        let mut extra: Vec<(String, String)> = (0..9).map(|at| (format!("arg{}", at + 1), words.get(at).unwrap_or(&"").to_string())).collect();
        extra.push(("args".into(), words.join(" ")));
        extra.push(("sender".into(), sender.get_name()));
        let run = match &player {
            Some(player) => Run::of(player, extra),
            None => Run::server(extra),
        };
        self.run_actions(server, data.get("actions"), &run).await;
        Ok(())
    }

    async fn open_menu(&mut self, server: &Server, player: &Player, id: &str) {
        let Some(data) = self.data_of("menu", id) else {
            player.send_system_message(TextComponent::text("That menu doesn't exist (any more)."), false);
            return;
        };
        let permission = fields::text(&data, "permission", "");
        if !permission.trim().is_empty() && !player.has_permission(permission.clone()).await {
            player.send_system_message(render("<red>You can't open this menu.", &[]), false);
            return;
        }
        let rows = fields::number(&data, "rows", 3, 1, 6) as i32;
        let screen = match rows {
            1 => Screen::Generic9x1,
            2 => Screen::Generic9x2,
            4 => Screen::Generic9x4,
            5 => Screen::Generic9x5,
            6 => Screen::Generic9x6,
            _ => Screen::Generic9x3,
        };
        let run = Run::of(player, vec![("menu".into(), id.to_string())]);
        let title = super::canvas::glyphs().menu_title(&fields::text(&data, "background", ""), &fields::text(&data, "title", id));
        let title = self.text_for(server, &title, &run);
        let gui = Gui::new(screen, title);
        gui.set_allow_grab_items(false).await;
        gui.set_allow_put_items(false).await;
        let fill = fields::text(&data, "fill", "");
        if let Some(key) = item_key(&fill) {
            for at in 0..rows * 9 {
                let pane = ItemStack::new(&key, 1);
                pane.set_custom_name(Some(TextComponent::text(" "))).await;
                gui.set_item(at as u32, pane).await;
            }
        }
        let mut slots = HashMap::new();
        for slot in listed(&data, "slots") {
            let at = fields::number(&slot, "slot", -1, -1, i64::from(rows * 9 - 1)) as i32;
            if at < 0 {
                continue;
            }
            let item = match slot.get("item") {
                Some(Value::String(name)) => self.stack(server, name, fields::number(&slot, "amount", 1, 1, 64) as u32, &run).await,
                Some(Value::Object(fields_of)) => self.build_item(server, fields_of, &run).await,
                _ => None,
            };
            if let Some(item) = item {
                gui.set_item(at as u32, item).await;
            }
            slots.insert(at, slot);
        }
        player.open_gui(gui).await;
        self.play.menus.insert(id_text(player.get_id()), (id.to_string(), slots, rows * 9));
    }

    /// A click in one of our menus runs that slot's actions. Returns true when the click was in ours.
    pub async fn menu_click(&mut self, server: &Server, player: &Player, slot: i32, click: pumpkin_plugin_api::common::ClickType) -> bool {
        if self.shop_click(server, player, slot, click).await {
            return true;
        }
        let key = id_text(player.get_id());
        let Some((menu, slots, size)) = self.play.menus.get(&key) else { return false };
        if slot < 0 || slot >= *size {
            return true;
        }
        let (menu, found) = (menu.clone(), slots.get(&slot).cloned());
        let Some(found) = found else { return true };
        let run = Run::of(player, vec![("menu".into(), menu)]);
        if self.test(server, found.get("if"), &run).await {
            // Close first: closing after the actions would also close a dialog or menu they just opened.
            if found.get("close").and_then(Value::as_bool) == Some(true) {
                self.close_menu(player).await;
            }
            self.run_actions(server, found.get("actions"), &run).await;
        }
        true
    }

    pub fn menu_closed(&mut self, player: &Player) {
        self.shop_closed(player);
        self.play.menus.remove(&id_text(player.get_id()));
    }

    /// Closes the player's open screen (the `close` action, a slot's "close after"). Its close event may come while
    /// the studio is busy, so the menu is forgotten here.
    async fn close_menu(&mut self, player: &Player) {
        self.shop_closed(player);
        self.play.menus.remove(&id_text(player.get_id()));
        if player.has_open_inventory() {
            player.close_inventory().await;
        }
    }

    // ---------- rules ----------

    /// While plugins load: the enabled blocks become blocks of the fork's own (`Blocks::register`).
    pub fn register_blocks<H: pumpkin_plugin_api::custom_block::CustomBlockHandler + Clone + 'static>(&mut self, handler: &H) {
        let rows: Vec<(String, Data)> = self.active_rows("block").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        if let Some(blocks) = self.blocks.as_mut() {
            blocks.register(&rows, handler);
        }
    }

    /// While plugins load: the enabled songs become jukebox songs and the paintings painting variants
    /// (`monolab:<id>`), which only works now.
    pub fn register_entries(&mut self, server: &Server) {
        use studio_format::song;
        let rows: Vec<(String, Data)> = self.active_rows("song").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        for (id, data) in rows {
            if !song::problems(&data).is_empty() {
                continue;
            }
            match server.register_registry_entry(song::REGISTRY, &format!("{TAG}:{id}"), &song::registry_json(&data, &id)) {
                Ok(()) => {
                    self.play.songs.insert(id.clone(), song::fingerprint(&data, &id));
                }
                Err(error) => tracing::warn!("Could not add studio song {id}: {error}"),
            }
        }
        use studio_format::painting;
        let rows: Vec<(String, Data)> = self.active_rows("painting").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        for (id, data) in rows {
            if !painting::problems(&data).is_empty() {
                continue;
            }
            match server.register_registry_entry(painting::REGISTRY, &format!("{TAG}:{id}"), &painting::registry_json(&data, &id)) {
                Ok(()) => {
                    self.play.paintings.insert(id.clone(), painting::fingerprint(&data, &id));
                }
                Err(error) => tracing::warn!("Could not add studio painting {id}: {error}"),
            }
        }
        use studio_format::biome;
        let rows: Vec<(String, Data)> = self.active_rows("biome").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        for (id, data) in rows {
            let json = biome::registry_json(&data);
            match server.register_registry_entry(biome::REGISTRY, &format!("{TAG}:{id}"), &json) {
                Ok(()) => {
                    self.play.biomes.insert(id.clone(), json);
                }
                Err(error) => tracing::warn!("Could not add studio biome {id}: {error}"),
            }
        }
    }

    /// The app asked for a reread.
    pub fn asked_reload(&mut self) {
        self.play.advancements_dirty = true;
    }

    /// Fires `event` for the player in about half a second.
    pub fn fire_later(&mut self, event: &str, player: &Player) {
        self.play.later.push((self.ticks + 10, event.to_string(), player.get_id(), Vec::new()));
    }

    /// Runs every rule for `event` whose `when` matches.
    pub async fn fire(&mut self, server: &Server, event: &str, player: Option<&Player>, extra: Vec<(String, String)>, matches: impl Fn(&Data) -> bool) {
        let rules: Vec<(String, Data)> = self.active_rows("rule").into_iter().filter(|(_, data)| when_event(data) == event).map(|(id, data)| (id.to_string(), data.clone())).collect();
        for (id, data) in rules {
            if !matches(object(&data, "when")) {
                continue;
            }
            let cooldown = fields::number(&data, "cooldownSeconds", 0, 0, 24 * 60 * 60) as u64;
            if cooldown > 0 {
                let key = format!("rule/{id}/{}", player.map(|one| id_text(one.get_id())).unwrap_or_else(|| "-".into()));
                let now = crate::now_ms();
                if now < self.play.cooldowns.get(&key).copied().unwrap_or(0) {
                    continue;
                }
                self.play.cooldowns.insert(key, now + cooldown * 1000);
            }
            let run = match player {
                Some(player) => Run::of(player, extra.clone()),
                None => Run::server(extra.clone()),
            };
            let passed = self.test(server, data.get("if"), &run).await;
            let ran = self.run_actions(server, data.get(if passed { "then" } else { "else" }), &run).await;
            self.trace(&id, event, &run.name, passed, ran);
        }
    }

    /// Like `fire`, for an event a rule may call off with a `cancel` step: true when one did.
    pub async fn fire_cancellable(&mut self, server: &Server, event: &str, player: Option<&Player>, extra: Vec<(String, String)>) -> bool {
        self.play.cancel = false;
        self.fire(server, event, player, extra.clone(), |when| rule_matches(event, when, &extra)).await;
        std::mem::take(&mut self.play.cancel)
    }

    /// Whether any rule listens for `event` (handlers skip the work otherwise).
    pub fn has_rule(&self, event: &str) -> bool {
        self.active_rows("rule").into_iter().any(|(_, data)| when_event(data) == event)
    }

    /// The app's test tool: runs the rules for `event` (only `only` when it isn't empty) as if it happened, with these
    /// placeholders. Cooldowns are skipped and not started. One row per rule for the event, traced as usual.
    pub(super) async fn fire_test(&mut self, server: &Server, event: &str, only: &str, player: Option<&Player>, extra: Vec<(String, String)>) -> Vec<Value> {
        let rules: Vec<(String, Data)> = self.active_rows("rule").into_iter().filter(|(id, data)| when_event(data) == event && (only.is_empty() || *id == only)).map(|(id, data)| (id.to_string(), data.clone())).collect();
        let mut out = Vec::new();
        for (id, data) in rules {
            if !rule_matches(event, object(&data, "when"), &extra) {
                out.push(json!({ "rule": id, "matched": false }));
                continue;
            }
            let run = match player {
                Some(player) => Run::of(player, extra.clone()),
                None => Run::server(extra.clone()),
            };
            let passed = self.test(server, data.get("if"), &run).await;
            let ran = self.run_actions(server, data.get(if passed { "then" } else { "else" }), &run).await;
            self.trace(&id, event, &run.name, passed, ran.clone());
            out.push(json!({ "rule": id, "matched": true, "passed": passed, "ran": ran }));
        }
        out
    }

    fn trace(&mut self, rule: &str, event: &str, player: &str, passed: bool, ran: Vec<String>) {
        self.play.traces_this_second += 1;
        if self.play.traces_this_second > MOST_TRACES {
            return;
        }
        self.play.traces.push(json!({ "type": "trace", "rule": rule, "event": event, "player": player, "passed": passed, "ran": ran, "at": crate::now_ms() }).to_string());
    }

    /// Once a second: timers and schedules.
    async fn second(&mut self, server: &Server) {
        super::canvas::load(&self.root);
        let timed: Vec<(String, Data)> = self.active_rows("rule").into_iter().filter(|(_, data)| matches!(when_event(data).as_str(), "timer" | "schedule")).map(|(id, data)| (id.to_string(), data.clone())).collect();
        let minute = crate::local_minute();
        let new_minute = minute != self.play.last_minute;
        self.play.last_minute = minute.clone();
        for (id, data) in timed {
            let when = object(&data, "when").clone();
            let due = if when_event(&data) == "timer" {
                let every = fields::number(&when, "everySeconds", 60, 1, 24 * 60 * 60);
                let count = self.play.timers.entry(id.clone()).or_insert(0);
                *count += 1;
                if *count < every {
                    false
                } else {
                    *count = 0;
                    true
                }
            } else {
                new_minute && fields::lines(&when, "at").contains(&minute)
            };
            if !due {
                continue;
            }
            let event = when_event(&data);
            let only = id.clone();
            if when.get("perPlayer").and_then(Value::as_bool) == Some(true) {
                for (player, _) in crate::players_of(server) {
                    let only = only.clone();
                    self.fire(server, &event, Some(&player), Vec::new(), |_| true).await;
                    let _ = only;
                }
            } else {
                self.fire_one(server, &id, &event).await;
            }
        }
    }

    /// A timed rule for the whole server: only that one, not every timer.
    async fn fire_one(&mut self, server: &Server, id: &str, event: &str) {
        let Some(data) = self.data_of("rule", id) else { return };
        let run = Run::server(Vec::new());
        let passed = self.test(server, data.get("if"), &run).await;
        let ran = self.run_actions(server, data.get(if passed { "then" } else { "else" }), &run).await;
        self.trace(id, event, "", passed, ran);
    }

    // ---------- NPCs ----------

    /// Every studio tick: once a second match the world's tagged NPCs to the files (like holograms), and turn
    /// heads toward the nearest player.
    async fn refresh_npcs(&mut self, server: &Server) {
        let wanted: Vec<(String, Data)> = self.active_rows("npc").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        // An NPC gone from the world (killed, its chunk unloaded) is looked for again at once.
        let lost = self.play.npcs.values().any(|entity| !entity.is_valid());
        if self.ticks.is_multiple_of(20) || self.play.npcs_dirty || lost {
            let fresh = std::mem::take(&mut self.play.npcs_dirty);
            self.play.npcs.clear();
            self.play.npc_ids.clear();
            let mut found = tagged_npcs(server).await;
            for (id, data) in &wanted {
                let mut mine = found.remove(id).unwrap_or_default();
                if fresh {
                    mine.drain(..).for_each(|one| one.remove());
                }
                let mut kept = None;
                for entity in mine {
                    if kept.is_none() && !entity.is_dead() {
                        kept = Some(entity);
                    } else {
                        entity.remove();
                    }
                }
                if kept.is_none() {
                    kept = self.spawn_npc(server, id, data).await;
                }
                if let Some(entity) = kept {
                    self.play.npc_ids.insert(entity.get_id() as i32, id.clone());
                    self.play.npcs.insert(id.clone(), entity);
                }
            }
            for (_, leftovers) in found {
                leftovers.into_iter().for_each(|one| one.remove());
            }
        }
        let players = crate::players_of(server);
        for (id, data) in &wanted {
            if data.get("lookAtPlayers").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let Some(entity) = self.play.npcs.get(id) else { continue };
            let (x, y, z) = entity.get_position();
            let eye = y + f64::from(entity.get_eye_height());
            let nearest = players
                .iter()
                .map(|(player, _)| player.get_position())
                .map(|(px, py, pz)| (px, py + 1.62, pz))
                .filter(|(px, py, pz)| (px - x).powi(2) + (py - eye).powi(2) + (pz - z).powi(2) < LOOK_DISTANCE * LOOK_DISTANCE)
                .min_by(|a, b| ((a.0 - x).powi(2) + (a.2 - z).powi(2)).total_cmp(&((b.0 - x).powi(2) + (b.2 - z).powi(2))));
            match nearest {
                Some((px, py, pz)) => {
                    let (dx, dy, dz) = (px - x, py - eye, pz - z);
                    let yaw = (-dx.atan2(dz)).to_degrees() as f32;
                    let pitch = (-dy.atan2((dx * dx + dz * dz).sqrt())).to_degrees() as f32;
                    entity.set_rotation(yaw, pitch);
                }
                None => entity.set_rotation(number_of(data.get("yaw"), 0.0) as f32, 0.0),
            }
        }
    }

    async fn spawn_npc(&mut self, server: &Server, id: &str, data: &Data) -> Option<Entity> {
        let at = |key: &str| data.get(key).and_then(Value::as_f64).filter(|one| one.is_finite());
        let (Some(x), Some(y), Some(z)) = (at("x"), at("y"), at("z")) else {
            self.problem("npc", id, "\"x\", \"y\" and \"z\" are needed.");
            return None;
        };
        let world_name = fields::text(data, "world", "world");
        let Some(world) = world_named(server, &world_name) else {
            self.problem("npc", id, &format!("There is no world named \"{world_name}\"."));
            return None;
        };
        let mob = fields::text(data, "entity", "villager").trim().to_ascii_lowercase();
        let Some(kind) = names::entity_type(mob.strip_prefix("minecraft:").unwrap_or(&mob)) else {
            self.problem("npc", id, &format!("\"{mob}\" isn't a mob an NPC can be."));
            return None;
        };
        world.get_chunk((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16))?;
        let entity = world.spawn_entity(kind, (x, y, z)).await;
        if !entity.is_living() {
            entity.remove();
            self.problem("npc", id, &format!("\"{mob}\" isn't a mob an NPC can be."));
            return None;
        }
        entity.set_string(TAG, "npc", id).await;
        let name = fields::text(data, "name", "");
        if !name.trim().is_empty() {
            entity.set_custom_name(render(&name, &values(server, None)));
            entity.set_custom_name_visible(true);
        }
        entity.set_invulnerable(true);
        entity.set_silent(true);
        entity.set_has_gravity(false);
        entity.set_glowing(data.get("glow").and_then(Value::as_bool) == Some(true));
        entity.set_rotation(number_of(data.get("yaw"), 0.0) as f32, 0.0);
        if let Some(mob) = entity.as_mob() {
            mob.set_ai_disabled(true);
        }
        Some(entity)
    }

    /// A player right-clicked an entity: if it's one of our NPCs, run its actions. Returns true when it was ours.
    pub async fn npc_clicked(&mut self, server: &Server, player: &Player, entity_id: i32) -> bool {
        if let Some(model) = self.play.models.clicked(entity_id, player).await {
            if let Some(data) = self.data_of("model", &model) {
                let run = Run::of(player, vec![("model".into(), model)]);
                self.run_actions(server, data.get("clickActions"), &run).await;
            }
            return true;
        }
        let Some(id) = self.play.npc_ids.get(&entity_id).cloned() else { return self.mob_clicked(server, player, entity_id).await };
        let Some(data) = self.data_of("npc", &id) else { return true };
        // Talking counts for quests even when the NPC's own conditions say no.
        self.quest_event(server, player, "talk", &id, 1).await;
        let run = Run::of(player, vec![("npc".into(), id.clone())]);
        if !self.test(server, data.get("if"), &run).await {
            self.run_actions(server, data.get("deny"), &run).await;
            return true;
        }
        let cooldown = fields::number(&data, "cooldownSeconds", 0, 0, 24 * 60 * 60) as u64;
        if cooldown > 0 {
            let key = format!("npc/{id}/{}", id_text(player.get_id()));
            let now = crate::now_ms();
            if now < self.play.cooldowns.get(&key).copied().unwrap_or(0) {
                return true;
            }
            self.play.cooldowns.insert(key, now + cooldown * 1000);
        }
        self.run_actions(server, data.get("actions"), &run).await;
        true
    }

    /// An NPC can't be hurt: whether this entity is one.
    pub fn is_npc(&self, entity_id: i32) -> bool {
        self.play.npc_ids.contains_key(&entity_id) || self.play.models.is_model(entity_id)
    }

    // ---------- dialogs ----------

    async fn open_dialog(&mut self, server: &Server, player: &Player, id: &str) {
        let Some(data) = self.data_of("dialog", id) else { return };
        let permission = fields::text(&data, "permission", "");
        if !permission.trim().is_empty() && !player.has_permission(permission.clone()).await {
            return;
        }
        let run = Run::of(player, vec![("dialog".into(), id.to_string())]);
        // Starting variables the player has none of yet, then the dialog's own on-open actions, before it draws.
        if let Some(Value::Object(vars)) = data.get("variables") {
            self.load_vars();
            let mine = id_text(player.get_id());
            for (name, value) in vars.iter().take(studio_format::canvas::MOST_VARIABLES) {
                let have = self.play.vars_players.get(&mine).is_some_and(|set| set.contains_key(name));
                if !have && let Some(step) = json!({ "type": "set", "variable": name, "value": value }).as_object() {
                    self.set_var(server, step, &run);
                }
            }
        }
        self.run_actions(server, data.get("onOpen"), &run).await;
        self.show_dialog(server, player, id, None).await;
    }

    /// Draws a dialog for a player (again, after a canvas click), with `pressed` showing that button pressed.
    async fn show_dialog(&mut self, server: &Server, player: &Player, id: &str, pressed: Option<usize>) {
        let Some(data) = self.data_of("dialog", id) else { return };
        let run = Run::of(player, vec![("dialog".into(), id.to_string())]);
        let allowed = allowed_nodes(player, &data).await;
        let mut hidden = HashSet::new();
        for (at, conditions) in super::canvas::conditions(&data) {
            if !self.test(server, Some(&conditions), &run).await {
                hidden.insert(at);
            }
        }
        self.play.dialog_count += 1;
        let shown = self.play.dialog_count;
        self.play.dialog_shown.insert(id_text(player.get_id()), shown);
        // A body line is text, or {"item", "count", "text"}: Pumpkin draws the item, then the text under it.
        let mut body = Vec::new();
        let lines = match data.get("body") {
            Some(Value::Array(list)) => list.clone(),
            Some(Value::String(one)) => vec![Value::String(one.clone())],
            _ => Vec::new(),
        };
        for line in lines {
            match line {
                Value::String(text) => body.push(DialogBody::PlainMessage(self.text_for(server, &text, &run))),
                Value::Object(one) => {
                    let count = fields::number(&one, "count", 1, 1, 64) as u32;
                    if let Some(stack) = self.stack(server, &fields::text(&one, "item", ""), count, &run).await {
                        body.push(DialogBody::Item(stack));
                    }
                    let text = fields::text(&one, "text", "");
                    if !text.trim().is_empty() {
                        body.push(DialogBody::PlainMessage(self.text_for(server, &text, &run)));
                    }
                }
                _ => {}
            }
        }
        if let Some(drawn) = super::canvas::body(id, &data, |text| self.mini_for(server, text, &run), |one| {
            let node = fields::text(one, "permission", "");
            node.trim().is_empty() || allowed.contains(&node)
        }, hidden, pressed) {
            body = vec![drawn];
        }
        let mut inputs = Vec::new();
        for (at, input) in listed(&data, "inputs").into_iter().enumerate() {
            let label = self.text_for(server, &fields::text(&input, "label", &fields::text(&input, "key", "")), &run);
            let key = Some(input_key(&input, at));
            let width = input.get("width").and_then(Value::as_f64).map(|one| one.clamp(1.0, 1024.0) as u32);
            inputs.push(match fields::text(&input, "type", "text").as_str() {
                "bool" => DialogInput::Bool(DialogInputBool { key, label, default_value: input.get("initial").and_then(Value::as_bool) == Some(true), on_true: None, on_false: None }),
                "number" => {
                    let least = number_of(input.get("min"), 0.0) as f32;
                    let most = (number_of(input.get("max"), 10.0) as f32).max(least + 1.0);
                    DialogInput::NumberRange(DialogInputNumberRange {
                        key,
                        label,
                        min_value: least,
                        max_value: most,
                        initial_value: (number_of(input.get("initial"), f64::from(least)) as f32).clamp(least, most),
                        step: (number_of(input.get("step"), 1.0) as f32).max(0.01),
                        label_format: None,
                        width,
                    })
                }
                "choice" => {
                    let options = choice_options(&input);
                    let initial = fields::text(&input, "initial", "");
                    DialogInput::SingleOption(DialogInputSingleOption {
                        key,
                        label,
                        initial_index: options.iter().position(|(id, _)| *id == initial).unwrap_or(0) as u32,
                        options: options.iter().map(|(_, shown)| self.text_for(server, shown, &run)).collect(),
                        width,
                        label_visible: None,
                        option_ids: Some(options.iter().map(|(id, _)| id.clone()).collect()),
                    })
                }
                _ => DialogInput::Text(DialogInputText {
                    key,
                    label,
                    placeholder: TextComponent::text(""),
                    default_value: fields::text(&input, "initial", ""),
                    width,
                    label_visible: None,
                    max_length: input.get("maxLength").and_then(Value::as_f64).map(|one| one.clamp(1.0, 32_767.0) as u32),
                    multiline: None,
                }),
            });
        }
        // With inputs, buttons send their values back (`dynamic/custom`), so `<input:key>` is filled.
        let with_inputs = !inputs.is_empty();
        let mut buttons = Vec::new();
        let mut exit_button = None;
        let mut all: Vec<(String, Data)> = listed(&data, "buttons").into_iter().enumerate().map(|(at, one)| (at.to_string(), one)).collect();
        if let Some(Value::Object(exit)) = data.get("exit") {
            all.push(("exit".into(), exit.clone()));
        }
        for (at, button) in all {
            let permission = fields::text(&button, "permission", "");
            if !permission.trim().is_empty() && !allowed.contains(&permission) {
                continue;
            }
            let tooltip = fields::text(&button, "tooltip", "");
            let url = fields::text(&button, "url", "");
            let click = format!("{TAG}:dialog/{id}/{at}");
            let made = ActionButton {
                text: self.text_for(server, &fields::text(&button, "label", "OK"), &run),
                tooltip: (!tooltip.trim().is_empty()).then(|| self.text_for(server, &tooltip, &run)),
                width: button.get("width").and_then(Value::as_f64).map(|one| one.clamp(1.0, 1024.0) as u32),
                action: if url.starts_with("https://") || url.starts_with("http://") {
                    Action::OpenUrl(url)
                } else if with_inputs {
                    Action::DynamicCustom(DynamicCustomAction { id: click, additions: None })
                } else {
                    Action::CustomClick(CustomClickAction { id: click, payload: None })
                },
            };
            if at == "exit" {
                exit_button = Some(made);
            } else {
                buttons.push(made);
            }
        }
        // The exit button sits under a multi-action grid (and runs on Escape); alone it is the notice's button.
        let (kind, exit_action) = match (buttons.len(), exit_button) {
            (0, Some(exit)) => {
                buttons.push(exit);
                (DialogType::Notice, None)
            }
            (0, None) => (DialogType::Notice, None),
            (_, Some(exit)) => (DialogType::MultiAction, Some(exit)),
            (1, None) => (DialogType::Notice, None),
            (_, None) => (DialogType::MultiAction, None),
        };
        let columns = data.get("columns").and_then(Value::as_f64).map(|one| one.clamp(1.0, 8.0) as u32);
        let title = self.text_for(server, &fields::text(&data, "title", id), &run);
        let Some(java) = player.as_java() else { return };
        java.show_dialog(Dialog {
            title,
            type_: kind,
            body,
            inputs,
            buttons,
            links: Vec::new(),
            // A canvas click can leave the dialog open (`stay`) or wait for it to be drawn again (`refresh`).
            after_action: Some(match super::canvas::after(&data) {
                Some("stay") => AfterAction::Peek,
                Some("refresh") => AfterAction::WaitForResponse,
                _ => AfterAction::Pop,
            }),
            can_close_with_escape: data.get("canClose").and_then(Value::as_bool) != Some(false),
            external_title: None,
            pause: Some(false),
            columns,
            button_width: None,
            exit_action,
        }).await;
    }

    /// A dialog button was clicked: `monolab:dialog/<id>/<button>`, with the inputs' values when the dialog has
    /// inputs. Returns true when it was ours.
    pub async fn dialog_clicked(&mut self, server: &Server, player: &Player, click: &str, payload: Option<&NbtTree>) -> bool {
        let Some(rest) = click.strip_prefix(&format!("{TAG}:dialog/")) else { return false };
        let Some((id, at)) = rest.rsplit_once('/') else { return true };
        let Some(data) = self.data_of("dialog", id) else { return true };
        if let Some((index, element)) = super::canvas::clicked(&data, at) {
            let node = fields::text(&element, "permission", "");
            let run = Run::of(player, vec![("dialog".into(), id.to_string())]);
            if !node.trim().is_empty() && !player.has_permission(node.clone()).await {
                return true;
            }
            // A hidden element has no click; an old dialog's click for one doesn't run either.
            if !self.test(server, element.get("if"), &run).await {
                return true;
            }
            let after = super::canvas::after(&data).unwrap_or("close");
            let stays = after != "close" && studio_format::canvas::leaving(element.get("actions")).is_empty();
            let pressed = stays && studio_format::canvas::pressed_glyph(&element).is_some();
            if pressed {
                self.show_dialog(server, player, id, Some(index)).await;
            }
            self.run_actions(server, element.get("actions"), &run).await;
            if pressed {
                let seen = self.play.dialog_shown.get(&id_text(player.get_id())).copied();
                self.play.waiting.push(Waiting { due: self.ticks + PRESSED_TICKS, steps: vec![json!({ "type": REDRAW, "dialog": id, "seen": seen })], run, waited: 0 });
            } else if stays && after == "refresh" {
                self.show_dialog(server, player, id, None).await;
            }
            return true;
        }
        let button = if at == "exit" {
            data.get("exit").and_then(Value::as_object).cloned()
        } else {
            at.parse::<usize>().ok().and_then(|at| listed(&data, "buttons").into_iter().nth(at))
        };
        let Some(button) = button else { return true };
        let permission = fields::text(&button, "permission", "");
        if !permission.trim().is_empty() && !player.has_permission(permission.clone()).await {
            return true;
        }
        let values = payload.map(compound_values).unwrap_or_default();
        let mut extra = vec![("dialog".to_string(), id.to_string())];
        for (at, input) in listed(&data, "inputs").into_iter().enumerate() {
            let key = input_key(&input, at);
            let value = values.get(&key).cloned().unwrap_or_default();
            // A choice sends its option's id; a switch true/false; a slider its number.
            extra.push((format!("input:{}", fields::text(&input, "key", &key)), value));
        }
        self.run_actions(server, button.get("actions"), &Run::of(player, extra)).await;
        true
    }

    // ---------- loot ----------

    /// One roll of a table: `rolls` times (or up to `rollsMax`) one entry by weight.
    pub(super) async fn roll(&mut self, server: &Server, data: &Data, run: &Run) -> Vec<ItemStack> {
        let least = fields::number(data, "rolls", 1, 0, 64);
        let most = fields::number(data, "rollsMax", least, 0, 64).max(least);
        let times = least + if most > least { (self.next_random() % (most - least + 1) as u64) as i64 } else { 0 };
        let entries: Vec<Data> = listed(data, "entries").into_iter().filter(|one| fields::number(one, "weight", 1, 0, 1_000_000) > 0).collect();
        let total: i64 = entries.iter().map(|one| fields::number(one, "weight", 1, 0, 1_000_000)).sum();
        let mut out = Vec::new();
        for _ in 0..times {
            if total <= 0 {
                break;
            }
            let mut pick = (self.next_random() % total as u64) as i64;
            for entry in &entries {
                pick -= fields::number(entry, "weight", 1, 0, 1_000_000);
                if pick < 0 {
                    let item = fields::text(entry, "item", "");
                    if !item.trim().is_empty() {
                        let low = fields::number(entry, "min", 1, 1, 64 * 36);
                        let high = fields::number(entry, "max", low, 1, 64 * 36).max(low);
                        let amount = low + if high > low { (self.next_random() % (high - low + 1) as u64) as i64 } else { 0 };
                        if let Some(stack) = self.stack(server, item.trim(), amount as u32, run).await {
                            out.push(stack);
                        }
                    }
                    break;
                }
            }
        }
        out
    }

    /// Remembers which block was broken where, for the drop event that follows.
    pub fn block_broken(&mut self, at: (i32, i32, i32), block: &str) {
        if self.play.broken.len() > 256 {
            self.play.broken.clear();
        }
        self.play.broken.insert(at, clean_name(block));
    }

    /// Loot tables that drop from `from` (`chest`, `fishing`), whose `targets` hold `name` (or are empty).
    fn tables_from(&self, from: &str, name: &str) -> Vec<Data> {
        self.active_rows("loot")
            .into_iter()
            .map(|(_, data)| data.clone())
            .filter(|data| {
                let drops = object(data, "drops");
                fields::text(drops, "from", "none") == from && {
                    let targets: Vec<String> = fields::lines(drops, "targets").into_iter().filter(|one| !one.trim().is_empty()).map(|one| clean_name(&one)).collect();
                    targets.is_empty() || targets.iter().any(|one| one == name)
                }
            })
            .collect()
    }

    /// A chest (or other container) filling from a vanilla loot table (`chests/simple_dungeon`): ours add to or
    /// replace what it rolled.
    pub async fn chest_loot(&mut self, server: &Server, table: &str, items: &mut Vec<ItemStack>) {
        for data in self.tables_from("chest", &clean_name(table)) {
            if object(&data, "drops").get("replace").and_then(Value::as_bool) == Some(true) {
                items.clear();
            }
            let rolled = self.roll(server, &data, &Run::default()).await;
            items.extend(rolled);
        }
    }

    /// A player caught something fishing: ours go into their inventory. Returns true when one replaces the catch.
    pub async fn fished(&mut self, server: &Server, player: &Player) -> bool {
        let mut replace = false;
        let run = Run::of(player, Vec::new());
        for data in self.tables_from("fishing", "") {
            if !shows(&data, player, &crate::who_of(player).world) {
                continue;
            }
            replace |= object(&data, "drops").get("replace").and_then(Value::as_bool) == Some(true);
            for stack in self.roll(server, &data, &run).await {
                give(player, stack).await;
            }
        }
        replace
    }

    /// What a broken block drops: the tables that drop from it add to (or replace) the items.
    pub async fn block_drops(&mut self, server: &Server, at: (i32, i32, i32), player: Option<&Player>, items: &mut Vec<ItemStack>) {
        let Some(block) = self.play.broken.remove(&at) else { return };
        let tables: Vec<Data> = self
            .active_rows("loot")
            .into_iter()
            .map(|(_, data)| data.clone())
            .filter(|data| {
                let drops = object(data, "drops");
                fields::text(drops, "from", "none") == "block" && {
                    let targets: Vec<String> = fields::lines(drops, "targets").into_iter().filter(|one| !one.trim().is_empty()).map(|one| clean_name(&one)).collect();
                    targets.is_empty() || targets.contains(&block)
                }
            })
            .collect();
        let run = player.map(|one| Run::of(one, Vec::new())).unwrap_or_default();
        for data in tables {
            if let Some(player) = player
                && !shows(&data, player, &crate::who_of(player).world) {
                    continue;
                }
            if object(&data, "drops").get("replace").and_then(Value::as_bool) == Some(true) {
                items.clear();
            }
            let rolled = self.roll(server, &data, &run).await;
            items.extend(rolled);
        }
    }

    // ---------- advancements ----------

    /// The advancement files changed: registers them with the fork (no data pack), parents first, and takes out
    /// the ones gone. Online players see the new tabs at once.
    async fn register_advancements(&mut self, server: &Server) {
        if !std::mem::take(&mut self.play.advancements_dirty) {
            return;
        }
        use pumpkin_plugin_api::wit::pumpkin::plugin::advancement::{AdvancementDefinition, AdvancementDisplay, FrameType};
        // Older apps wrote these into a data pack: turn it off, or its copies would stand beside ours.
        let packs = server.get_datapack_manager();
        if packs.is_enabled(crate::ADVANCEMENT_PACK) {
            match packs.disable_pack(crate::ADVANCEMENT_PACK.to_string()).await {
                Ok(()) => {
                    if let Err(error) = packs.reload().await {
                        tracing::warn!("Could not reload data packs after turning off the old advancement pack: {error}");
                    }
                    // A reload sets Pumpkin's recipe list to the data packs' alone, taking ours with it.
                    self.play.recipes.clear();
                    self.register_recipes(server).await;
                }
                Err(error) => tracing::warn!("Could not turn off the old studio advancement pack: {error}"),
            }
        }
        let rows: Vec<(String, Data)> = self.active_rows("advancement").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        let ordered = studio_format::advancement::order(&rows);
        let spots = studio_format::advancement::layout(&ordered);
        let wanted: HashSet<String> = ordered.iter().map(|(id, _)| advancement_key(id)).collect();
        for gone in self.play.advancements.iter().filter(|key| !wanted.contains(*key)).cloned().collect::<Vec<_>>() {
            server.unregister_advancement(&gone);
            self.play.advancements.remove(&gone);
        }
        for (id, _) in &rows {
            if !ordered.iter().any(|(one, _)| one == id) {
                self.problem("advancement", id, "Its \"parent\" isn't an advancement here (or they go round in a loop).");
            }
        }
        for (id, data) in ordered {
            let made = studio_format::advancement::json(id, data, TAG, |name| {
                let clean = name.trim().to_ascii_lowercase();
                (!clean.is_empty()).then(|| if clean.contains(':') { clean } else { format!("minecraft:{clean}") })
            });
            let shown = &made["display"];
            let text = |key: &str| TextComponent::from_json(&shown[key].to_string()).unwrap_or_else(|_| TextComponent::text(""));
            let frame = match shown["frame"].as_str() {
                Some("goal") => FrameType::Goal,
                Some("challenge") => FrameType::Challenge,
                _ => FrameType::Task,
            };
            let (x, y) = spots.get(id).copied().unwrap_or_default();
            let icon_name = fields::text(data, "icon", "diamond");
            let icon = match self.stack(server, &icon_name, 1, &Run::default()).await {
                Some(stack) => Some(stack),
                None => self.stack(server, "diamond", 1, &Run::default()).await,
            };
            let key = advancement_key(id);
            let definition = AdvancementDefinition {
                id: key.clone(),
                parent_id: made["parent"].as_str().map(str::to_string),
                display: Some(AdvancementDisplay {
                    title: text("title"),
                    description: text("description"),
                    frame,
                    show_toast: shown["show_toast"].as_bool() != Some(false),
                    hidden: shown["hidden"].as_bool() == Some(true),
                    announce_to_chat: shown["announce_to_chat"].as_bool() != Some(false),
                    background: shown["background"].as_str().map(str::to_string),
                    x,
                    y,
                }),
                icon,
                criteria: vec![studio_format::advancement::CRITERION.to_string()],
                requirements: Vec::new(),
                experience: 0,
            };
            match server.register_advancement(definition) {
                Ok(()) => {
                    self.play.advancements.insert(key);
                }
                Err(error) => self.problem("advancement", id, &format!("The server didn't take it: {error}")),
            }
        }
    }

    /// A player earned an advancement: if it's ours, its rewards run.
    pub async fn advancement_done(&mut self, server: &Server, player: &Player, advancement: &str) {
        let Some(id) = advancement.strip_prefix(&format!("{TAG}:")) else { return };
        let Some(data) = self.data_of("advancement", id) else { return };
        self.run_actions(server, data.get("rewards"), &Run::of(player, vec![("advancement".into(), id.to_string())])).await;
    }

    // ---------- worlds ----------

    /// Once a second: makes the studio worlds that aren't there yet and applies game rules (once per change).
    async fn load_worlds(&mut self, server: &Server) {
        let rows: Vec<(String, Data)> = self.active_rows("world").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        for (id, data) in rows {
            if self.play.worlds.contains(&id) {
                continue;
            }
            self.play.worlds.insert(id.clone());
            // The main world's generator was offered when the add-on loaded (`main_world_generators`).
            if is_main_world(&data) {
                if is_vanilla(&data) {
                    self.problem("world", &id, "Pumpkin can't give the main world the server's own ground with these settings: it keeps its own generator.");
                }
                continue;
            }
            let name = fields::text(&data, "name", &id).trim().to_string();
            let name = if name.is_empty() { id.clone() } else { name };
            if DEFAULT_WORLDS.contains(&name.as_str()) {
                self.problem("world", &id, "This is the name of one of the server's own worlds. To use this generator there, turn on \"Use for the main world\".");
                continue;
            }
            if !name.chars().all(|one| one.is_ascii_alphanumeric() || one == '_' || one == '-') || name.len() > 48 {
                self.problem("world", &id, "\"name\" may only use letters, digits, _ and - (up to 48).");
                continue;
            }
            let world = match world_named(server, &name) {
                // Already open (the add-on was reloaded): a custom world needs its generator again.
                Some(world) => {
                    if fields::text(&data, "type", "normal") == "custom" && !is_vanilla(&data) {
                        let (generator_id, _) = self.custom_generator(&id, &name, &data);
                        world.set_chunk_generator(generator_id);
                    }
                    world
                }
                None => {
                    let dimension = match fields::text(&data, "environment", "normal").as_str() {
                        "nether" => Dimension::Nether,
                        "end" => Dimension::End,
                        _ => Dimension::Overworld,
                    };
                    tracing::info!("Studio world: making {name}");
                    // The generator isn't saved with the world, so it is chosen again on every start.
                    match fields::text(&data, "type", "normal").as_str() {
                        "flat" => server.create_world_with_generator(name.clone(), dimension, WorldGenerator::Flat(flat_settings(&data))).await,
                        "void" => {
                            let world = server.create_world_with_generator(name.clone(), dimension, WorldGenerator::Void).await;
                            void_floor(&world).await;
                            world
                        }
                        "amplified" => server.create_world_with_generator(name.clone(), dimension, WorldGenerator::Amplified).await,
                        "large_biomes" => server.create_world_with_generator(name.clone(), dimension, WorldGenerator::LargeBiomes).await,
                        "custom" if is_vanilla(&data) => {
                            let generator = self.vanilla_generator(&id, &data);
                            server.create_world_with_generator(name.clone(), dimension, generator).await
                        }
                        "custom" => {
                            let (_, generator) = self.custom_generator(&id, &name, &data);
                            server.create_world_with_generator(name.clone(), dimension, generator).await
                        }
                        _ => server.create_world(name.clone(), dimension).await,
                    }
                }
            };
            if let Some(Value::Bool(pvp)) = data.get("pvp")
                && let Some(rule) = names::game_rule("pvp") {
                    world.set_game_rule(rule, GameRuleValue::Bool(*pvp));
                }
            for (key, value) in object(&data, "gamerules") {
                let Some(rule) = game_rule(key) else {
                    self.problem("world", &id, &format!("There is no game rule called {key}."));
                    continue;
                };
                match value {
                    Value::Bool(on) => world.set_game_rule(rule, GameRuleValue::Bool(*on)),
                    Value::Number(count) => world.set_game_rule(rule, GameRuleValue::Int(count.as_f64().unwrap_or(0.0).round() as i32)),
                    _ => self.problem("world", &id, &format!("The game rule {key} needs a switch or a number.")),
                }
            }
        }
    }

    /// Custom worlds marked `"main": true` offer their generator for the server's main world, named by the file id
    /// (fork `register-world-generator`). `pumpkin.toml` `[world.generators]` picks it, which the app writes when the
    /// file is saved; the server makes no chunk of its own worlds before this runs, so even spawn uses it.
    pub fn main_world_generators(&mut self, context: &pumpkin_plugin_api::Context) {
        let Ok(entries) = std::fs::read_dir(self.root.join("world")) else { return };
        let mut files: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        files.sort();
        for path in files {
            let Some(id) = path.file_stem().and_then(|one| one.to_str()).map(str::to_string) else { continue };
            if path.extension().and_then(|one| one.to_str()) != Some("json") || !studio_format::valid_id(&id) {
                continue;
            }
            let Some(data) = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else { continue };
            let Some(data) = data.as_object() else { continue };
            if !is_main_world(data) || is_vanilla(data) || !studio_format::enabled(data) {
                continue;
            }
            let name = fields::text(data, "name", &id).trim().to_string();
            let name = if name.is_empty() { id.clone() } else { name };
            let settings = match self.custom_generator(&id, &name, data) {
                (_, WorldGenerator::PluginWithVanilla(settings)) => settings,
                (generator_id, _) => PluginGeneratorSettings { generator_id, keep_carvers: false, keep_structures: false, keep_vanilla_features: false },
            };
            context.register_world_generator(&id, settings);
            tracing::info!("Studio world: {id} is offered for the main world as monolab:{id}");
        }
    }

    /// A custom world's generator: made from its file once per start (Pumpkin doesn't keep a plugin's generator
    /// with the world) and registered. Returns its id and the world generator a new world is made with, so its
    /// first chunk already uses it.
    fn custom_generator(&mut self, id: &str, name: &str, data: &Data) -> (u32, WorldGenerator) {
        let spec = object(data, "generator").clone();
        for problem in studio_format::worldgen::problems(&spec) {
            self.problem("world", id, &problem);
        }
        let mut made = studio_format::worldgen::Generator::new(&spec, studio_format::worldgen::seed_of(&fields::text(data, "seed", ""), name));
        for structure in made.structure_ids() {
            match self.structure(&structure) {
                Some(built) if !made.add_structure(&structure, &built) => {
                    self.problem("world", id, &format!("The structure {structure} is over {} blocks across, too big for a world feature.", studio_format::structure::MOST_FEATURE_SIZE));
                }
                Some(_) => {}
                None => self.problem("world", id, &format!("There is no structure {structure} (or its file wasn't brought in yet).")),
            }
        }
        let keep = made.keep;
        // Our biomes: registered by name, but the fork's chunk buffer wrapper can't place them by name yet, so the
        // generator puts each one's `like` there (fork request in docs/pumpkin-fork-requests.md).
        let likes: HashMap<String, String> = self.active_rows("biome").into_iter().map(|(biome, data)| (format!("studio:{biome}"), studio_format::biome::like(data))).collect();
        let ours: Vec<String> = made.biome_names().into_iter().filter(|one| one.starts_with("studio:")).collect();
        if !ours.is_empty() {
            self.problem("world", id, &format!("Pumpkin's generator can't place your own biomes yet, so their \"like\" biome is shown instead ({}).", ours.join(", ")));
        }
        let blocks = self.blocks.as_ref();
        let (generator, unknown) = super::worldgen::StudioGenerator::new(made, likes, |block| blocks.and_then(|one| one.state_of(block)));
        for block in unknown {
            self.problem("world", id, &format!("Pumpkin doesn't know the block {block}: stone is used."));
        }
        let generator_id = pumpkin_plugin_api::GeneratorManager::register(generator);
        let world = if keep.structures || keep.carvers || keep.decorations {
            WorldGenerator::PluginWithVanilla(PluginGeneratorSettings {
                generator_id,
                keep_carvers: keep.carvers,
                keep_structures: keep.structures,
                keep_vanilla_features: keep.decorations,
            })
        } else {
            WorldGenerator::Plugin(generator_id)
        };
        (generator_id, world)
    }

    /// `"mode": "vanilla"`: the server's own ground of the preset, twisted by a spiral warp (fork `noise-warped`,
    /// normal ground only). The biome rules can't go on it here yet: the fork's surface populator is per dimension,
    /// not per world.
    fn vanilla_generator(&mut self, id: &str, data: &Data) -> WorldGenerator {
        use pumpkin_plugin_api::wit::pumpkin::plugin::server::{NoiseWarp, SpiralWarp};
        let spec = object(data, "generator").clone();
        for problem in studio_format::worldgen::problems(&spec) {
            self.problem("world", id, &problem);
        }
        let made = studio_format::worldgen::Generator::new(&spec, studio_format::worldgen::seed_of(&fields::text(data, "seed", ""), id));
        if !objects_in(&spec, "biomes").is_empty() {
            self.problem("world", id, "Pumpkin can't put your biome rules on the server's own ground yet: its own biomes stay (Paper only).");
        }
        let terrain = made.vanilla().unwrap_or("normal").to_string();
        if let Some((strength, center_x, center_z)) = made.spiral() {
            if terrain != "normal" {
                self.problem("world", id, &format!("Twisted ground is always normal ground on Pumpkin, not {terrain}."));
            }
            return WorldGenerator::NoiseWarped(NoiseWarp::Spiral(SpiralWarp { strength, center_x, center_z }));
        }
        match terrain.as_str() {
            "amplified" => WorldGenerator::Amplified,
            "large_biomes" => WorldGenerator::LargeBiomes,
            _ => WorldGenerator::Default,
        }
    }
}

// ---------- helpers ----------

/// The server's own worlds with the usual `default_level_name` (`world`).
const DEFAULT_WORLDS: [&str; 3] = ["world", "world_nether", "world_the_end"];

/// A custom world on the server's own ground (`"generator": {"mode": "vanilla"}`).
fn is_vanilla(data: &Data) -> bool {
    fields::text(data, "type", "normal") == "custom" && fields::text(object(data, "generator"), "mode", "custom") == "vanilla"
}

fn objects_in(data: &Data, key: &str) -> Vec<Data> {
    data.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object).cloned().collect()
}

/// A custom world file that gives the server's main world its generator instead of making a world of its own.
fn is_main_world(data: &Data) -> bool {
    fields::text(data, "type", "normal") == "custom" && data.get("main").and_then(Value::as_bool) == Some(true)
}

pub fn when_event(data: &Data) -> String {
    fields::text(object(data, "when"), "event", "")
}

static EMPTY: std::sync::LazyLock<Data> = std::sync::LazyLock::new(Data::new);

pub fn object<'a>(data: &'a Data, key: &str) -> &'a Data {
    data.get(key).and_then(Value::as_object).unwrap_or(&EMPTY)
}

fn listed(data: &Data, key: &str) -> Vec<Data> {
    data.get(key).and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_object).cloned().collect()).unwrap_or_default()
}

fn number_of(value: Option<&Value>, fallback: f64) -> f64 {
    match value {
        Some(Value::Number(one)) => one.as_f64().filter(|one| one.is_finite()).unwrap_or(fallback),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(fallback),
        _ => fallback,
    }
}

fn id_text(id: Uuid) -> String {
    format!("{:016x}{:016x}", id.high, id.low)
}

fn show_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "0".into(),
        Some(Value::Number(one)) => match one.as_f64() {
            Some(number) if number.fract() == 0.0 && number.abs() < 1e15 => format!("{}", number as i64),
            _ => one.to_string(),
        },
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

fn change(was: Option<&Value>, op: &str, value: &str) -> Value {
    let given: Option<f64> = value.trim().parse().ok();
    if op == "set" {
        return given.map_or_else(|| json!(value), |number| json!(number));
    }
    let before = match was {
        Some(Value::Number(one)) => one.as_f64().unwrap_or(0.0),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    };
    let by = given.unwrap_or(0.0);
    json!(if op == "subtract" { before - by } else { before + by })
}

fn compare(left: Option<&Value>, op: &str, right: &str) -> bool {
    let a = match left {
        Some(Value::Number(one)) => one.as_f64(),
        Some(Value::String(text)) => text.trim().parse().ok(),
        None | Some(Value::Null) => None,
        Some(other) => other.to_string().parse().ok(),
    };
    if let (Some(a), Ok(b)) = (a, right.trim().parse::<f64>()) {
        return match op {
            "!=" => a != b,
            ">" => a > b,
            "<" => a < b,
            ">=" => a >= b,
            "<=" => a <= b,
            _ => a == b,
        };
    }
    let text = show_value(left);
    (op == "!=") != text.eq_ignore_ascii_case(right)
}

/// Whether a rule's `when` settings take an event with these placeholders (as the real events check them).
pub fn rule_matches(event: &str, when: &Data, extra: &[(String, String)]) -> bool {
    let value = |key: &str| extra.iter().find(|(name, _)| name == key).map(|(_, one)| one.clone()).unwrap_or_default();
    match event {
        "chat" => {
            let contains = fields::text(when, "contains", "");
            contains.trim().is_empty() || value("message").to_lowercase().contains(&contains.to_lowercase())
        }
        "break" | "place" => {
            let wanted: Vec<String> = fields::lines(when, "blocks").iter().filter(|one| !one.trim().is_empty()).map(|one| clean_name(one)).collect();
            wanted.is_empty() || wanted.contains(&clean_name(&value("block")))
        }
        "world_change" => {
            let to = fields::text(when, "world", "");
            to.trim().is_empty() || to == value("world")
        }
        "command" => {
            let starts = fields::text(when, "command", "");
            let starts = starts.trim().trim_start_matches('/');
            let line = value("command");
            starts.is_empty() || line.trim_start_matches('/').to_ascii_lowercase().starts_with(&starts.to_ascii_lowercase())
        }
        "click" => {
            let button = fields::text(when, "button", "right");
            let block = value("block");
            (button == "any" || button == value("button")) && named(when, "blocks", if block.is_empty() { "air" } else { &block })
        }
        "click_entity" | "kill" | "hit" => named(when, "entities", &value("entity")),
        "eat" | "craft" | "fish" | "drop" | "pickup" => named(when, "items", &value("item")),
        "gamemode" => {
            let mode = fields::text(when, "mode", "any");
            mode == "any" || mode == value("gamemode")
        }
        _ => true,
    }
}

/// Whether a `when` list (empty = any) names this block, entity or item; `studio:<id>` names one of ours.
fn named(when: &Data, key: &str, value: &str) -> bool {
    let wanted: Vec<String> = fields::lines(when, key).iter().filter(|one| !one.trim().is_empty()).map(|one| clean_name(one)).collect();
    wanted.is_empty() || wanted.contains(&clean_name(value))
}

/// `minecraft:Diamond_Ore` and `diamond_ore` both become `diamond_ore`.
fn clean_name(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    let lower = lower.split('[').next().unwrap_or_default().to_string();
    lower.strip_prefix("minecraft:").map(str::to_string).unwrap_or(lower)
}

// ---------- M1 item components ----------
//
// Pumpkin's plugin API takes a component as its network bytes (`item-stack.set-component`); bytes it can't read are
// dropped quietly, so a wrong guess here leaves the item plain instead of breaking it.

fn var_int(out: &mut Vec<u8>, value: i32) {
    let mut left = value as u32;
    loop {
        if left & !0x7f == 0 {
            out.push(left as u8);
            return;
        }
        out.push((left & 0x7f | 0x80) as u8);
        left >>= 7;
    }
}

fn net_text(out: &mut Vec<u8>, text: &str) {
    var_int(out, text.len() as i32);
    out.extend_from_slice(text.as_bytes());
}

fn net_float(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&(value as f32).to_be_bytes());
}

/// A sound written out by name (not a registry number), with no fixed range.
fn net_sound(out: &mut Vec<u8>, name: &str) {
    var_int(out, 0);
    net_text(out, name);
    out.push(0);
}

fn float_of(data: &Data, key: &str, fallback: f64, least: f64, most: f64) -> f64 {
    studio_format::item::float(data, key, fallback, least, most)
}

/// What Pumpkin can't do of an item's M1 fields (or what is wrong with them), for its status.
fn item_problem(data: &Data) -> Option<String> {
    let out = studio_format::item::problems(data);
    (!out.is_empty()).then(|| out.join(" "))
}

/// Sets a studio item's M1 look and components on `stack`.
async fn item_parts(stack: &ItemStack, id: &str, data: &Data) {
    use pumpkin_plugin_api::wit::pumpkin::plugin::data_components::DataComponent as Part;
    use pumpkin_plugin_api::wit::pumpkin::plugin::enchantments::AttributeModifierSlot as Slot;
    use pumpkin_plugin_api::{Attribute, AttributeModifier, ItemAttributeModifier, ModifierOperation};
    use studio_format::item;

    let set = async |part: Part, bytes: Vec<u8>| stack.set_component(part, bytes).await;
    if item::look(data).is_some() && item::problems(data).iter().all(|one| !one.starts_with("\"look")) {
        let mut bytes = Vec::new();
        net_text(&mut bytes, &format!("monolab:{id}"));
        set(Part::ItemModel, bytes).await;
    }
    let durability = fields::number(data, "durability", 0, 0, i64::from(i32::MAX)) as i32;
    if durability > 0 {
        let mut one = Vec::new();
        var_int(&mut one, 1);
        set(Part::MaxStackSize, one).await;
        let mut most = Vec::new();
        var_int(&mut most, durability);
        set(Part::MaxDamage, most).await;
        set(Part::Damage, vec![0]).await;
    } else if data.get("maxStack").is_some_and(Value::is_number) {
        let mut most = Vec::new();
        var_int(&mut most, fields::number(data, "maxStack", 64, 1, 99) as i32);
        set(Part::MaxStackSize, most).await;
    }
    if data.get("unbreakable").and_then(Value::as_bool) == Some(true) {
        set(Part::Unbreakable, Vec::new()).await;
    }
    let rarity = fields::text(data, "rarity", "");
    if let Some(at) = item::RARITIES.iter().position(|one| *one == rarity) {
        set(Part::Rarity, vec![at as u8]).await;
    }
    if data.get("glow").and_then(Value::as_bool) == Some(true) {
        set(Part::EnchantmentGlintOverride, vec![1]).await;
    }
    if data.get("hideTooltip").and_then(Value::as_bool) == Some(true) {
        // Hide all of it, no single components listed.
        set(Part::TooltipDisplay, vec![1, 0]).await;
    }
    if let Some(food) = item::part(data, "food") {
        let mut bytes = Vec::new();
        var_int(&mut bytes, fields::number(food, "nutrition", 4, 0, 1000) as i32);
        net_float(&mut bytes, float_of(food, "saturation", 2.4, 0.0, 1000.0));
        bytes.push(u8::from(food.get("alwaysEat").and_then(Value::as_bool) == Some(true)));
        set(Part::Food, bytes).await;
        let drink = fields::text(food, "animation", "eat") == "drink";
        let mut bytes = Vec::new();
        net_float(&mut bytes, float_of(food, "seconds", 1.6, 0.05, 60.0));
        var_int(&mut bytes, if drink { 2 } else { 1 });
        net_sound(&mut bytes, if drink { "minecraft:entity.generic.drink" } else { "minecraft:entity.generic.eat" });
        // Particles on, no effects.
        bytes.push(1);
        var_int(&mut bytes, 0);
        set(Part::Consumable, bytes).await;
    }
    let cooldown = float_of(data, "cooldown", 0.0, 0.0, 3600.0);
    if cooldown > 0.0 {
        let mut bytes = Vec::new();
        net_float(&mut bytes, cooldown);
        bytes.push(1);
        net_text(&mut bytes, &format!("monolab:{id}"));
        set(Part::UseCooldown, bytes).await;
    }
    if let Some(tool) = item::part(data, "tool") {
        let kinds: Vec<String> = fields::lines(tool, "mines").into_iter().map(|one| one.trim().to_string()).filter(|one| item::TOOL_KINDS.contains(&one.as_str())).collect();
        let mut bytes = Vec::new();
        var_int(&mut bytes, kinds.len() as i32);
        for kind in &kinds {
            // A block tag, then the speed and "drops its loot", both given.
            var_int(&mut bytes, 0);
            net_text(&mut bytes, &format!("minecraft:mineable/{kind}"));
            bytes.push(1);
            net_float(&mut bytes, float_of(tool, "speed", 6.0, 0.0, 1000.0));
            bytes.extend_from_slice(&[1, 1]);
        }
        net_float(&mut bytes, 1.0);
        var_int(&mut bytes, fields::number(tool, "damagePerBlock", 1, 0, 1000) as i32);
        bytes.push(1);
        // Own block rules go in the component's NBT form (blocks by name; ours as the fork's `monolab:<id>`, which
        // a block not registered this start isn't, so those are tried without). The plain form stays the fallback.
        let rules = item::tool_rules(data);
        let done = if rules.is_empty() {
            false
        } else if stack.set_component_nbt(Part::Tool, tool_tree(tool, &kinds, &rules, true)).await.is_ok() {
            true
        } else {
            stack.set_component_nbt(Part::Tool, tool_tree(tool, &kinds, &rules, false)).await.is_ok()
        };
        if !done {
            set(Part::Tool, bytes).await;
        }
    }
    let equip = item::part(data, "equip");
    if let Some(equip) = equip {
        let mut bytes = Vec::new();
        var_int(&mut bytes, match fields::text(equip, "slot", "head").as_str() {
            "feet" => 1,
            "legs" => 2,
            "chest" => 3,
            _ => 4,
        });
        net_sound(&mut bytes, "minecraft:item.armor.equip_generic");
        let worn = !fields::text(equip, "texture", "").trim().is_empty() || !fields::text(equip, "legsTexture", "").trim().is_empty();
        bytes.push(u8::from(worn));
        if worn {
            net_text(&mut bytes, &format!("monolab:{id}"));
        }
        // No camera overlay, any entity; dispensable, swappable, damaged when hurt, not on interact, not shearable.
        bytes.extend_from_slice(&[0, 0, 1, 1, 1, 0, 0]);
        net_sound(&mut bytes, "minecraft:item.shears.snip");
        set(Part::Equippable, bytes).await;
    }
    if let Some(song) = item::disc(data) {
        // `jukebox_playable` is just the song's id; one this start didn't register is refused.
        let _ = stack.set_component_nbt(Part::JukeboxPlayable, NbtTree { root: 0, tags: vec![NbtTag::StringTag(song)] }).await;
    }
    if let Some(trim) = item::trim(data) {
        // `{pattern, material}` in the component's NBT form; a pattern the server doesn't know is refused.
        use pumpkin_plugin_api::common::NbtEntry;
        let tree = NbtTree {
            root: 0,
            tags: vec![
                NbtTag::Compound(vec![NbtEntry { key: "pattern".into(), value: 1 }, NbtEntry { key: "material".into(), value: 2 }]),
                NbtTag::StringTag(trim.pattern),
                NbtTag::StringTag(trim.material),
            ],
        };
        let _ = stack.set_component_nbt(Part::Trim, tree).await;
    }
    let mut modifiers = Vec::new();
    if let Some(attack) = item::part(data, "attack") {
        // The player's own 1 damage and 4 attacks a second, which these add to.
        if attack.get("damage").is_some_and(Value::is_number) {
            modifiers.push((Attribute::AttackDamage, "damage", float_of(attack, "damage", 1.0, 0.0, 2048.0) - 1.0, Slot::MainHand));
        }
        if attack.get("speed").is_some_and(Value::is_number) {
            modifiers.push((Attribute::AttackSpeed, "speed", float_of(attack, "speed", 4.0, 0.0, 1024.0) - 4.0, Slot::MainHand));
        }
    }
    if let Some(equip) = equip {
        let slot = || match fields::text(equip, "slot", "head").as_str() {
            "feet" => Slot::Feet,
            "legs" => Slot::Legs,
            "chest" => Slot::Chest,
            _ => Slot::Head,
        };
        let armor = float_of(equip, "armor", 0.0, 0.0, 30.0);
        let toughness = float_of(equip, "toughness", 0.0, 0.0, 20.0);
        if armor > 0.0 {
            modifiers.push((Attribute::Armor, "armor", armor, slot()));
        }
        if toughness > 0.0 {
            modifiers.push((Attribute::ArmorToughness, "toughness", toughness, slot()));
        }
    }
    // Ours replace the base item's own (a diamond sword's +6 / -2.4); added on top, the speed went below zero and the
    // attack never charged.
    let mut cleared = HashSet::new();
    for (attribute, ..) in &modifiers {
        if cleared.insert(format!("{attribute:?}")) {
            stack.remove_attribute_modifiers(*attribute).await;
        }
    }
    for (attribute, name, amount, slot) in modifiers {
        stack.add_attribute_modifier(ItemAttributeModifier {
            attribute,
            modifier: AttributeModifier { id: format!("monolab:{}_{name}", id.replace('-', "_")), amount, operation: ModifierOperation::Add },
            slot,
        }).await;
    }
}

/// A plain item's registry key, when Pumpkin knows the item.
fn item_key(name: &str) -> Option<String> {
    let clean = clean_name(name);
    if clean.is_empty() || clean == "air" {
        return None;
    }
    pumpkin_plugin_api::item::Item::from_name(&clean).map(|one| one.resource_location().to_string())
}

/// The studio items a recipe file names (`studio:<id>`, `studio:item/<id>`), for telling when it changed.
fn studio_refs(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => text.trim().strip_prefix("studio:").map(|one| vec![one.to_string()]).unwrap_or_default(),
        Value::Array(list) => list.iter().flat_map(studio_refs).collect(),
        Value::Object(map) => map.values().flat_map(studio_refs).collect(),
        _ => Vec::new(),
    }
}

/// An input's key: the file's `key` made safe (letters, digits, `_`), else `input_<at>` like the game.
fn input_key(input: &Data, at: usize) -> String {
    let key: String = fields::text(input, "key", "").trim().chars().map(|one| if one.is_ascii_alphanumeric() || one == '_' { one } else { '_' }).collect();
    if key.is_empty() { format!("input_{at}") } else { key }
}

/// A choice input's options: "warrior", or {"id": "warrior", "label": "<red>Warrior"}.
fn choice_options(input: &Data) -> Vec<(String, String)> {
    match input.get("options") {
        Some(Value::Array(list)) => list
            .iter()
            .map(|one| match one {
                Value::Object(one) => {
                    let id = fields::text(one, "id", "");
                    (id.clone(), fields::text(one, "label", &id))
                }
                other => (other.as_str().unwrap_or_default().to_string(), other.as_str().unwrap_or_default().to_string()),
            })
            .filter(|(id, _)| !id.trim().is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// A `dynamic/custom` payload (a compound of input key → value) as text values.
/// The tool component as NBT: own rules first (the game takes the first that matches), then the mined tags.
fn tool_tree(tool: &Data, kinds: &[String], rules: &[studio_format::item::ToolRule], ours: bool) -> NbtTree {
    use pumpkin_plugin_api::common::NbtEntry;
    let mut tags = vec![NbtTag::Compound(Vec::new())];
    let push = |tags: &mut Vec<NbtTag>, tag: NbtTag| {
        tags.push(tag);
        (tags.len() - 1) as u32
    };
    let mut list = Vec::new();
    let rule = |tags: &mut Vec<NbtTag>, blocks: NbtTag, speed: f32, drops: bool| {
        let blocks = push(tags, blocks);
        let speed = push(tags, NbtTag::Float(speed));
        let drops = push(tags, NbtTag::Byte(i8::from(drops)));
        push(tags, NbtTag::Compound(vec![
            NbtEntry { key: "blocks".into(), value: blocks },
            NbtEntry { key: "speed".into(), value: speed },
            NbtEntry { key: "correct_for_drops".into(), value: drops },
        ]))
    };
    for one in rules {
        let names: Vec<String> = one.blocks.iter().filter_map(|name| match name.strip_prefix("studio:") {
            Some(id) => ours.then(|| format!("{TAG}:{id}")),
            None => Some(if name.contains(':') { name.clone() } else { format!("minecraft:{name}") }),
        }).collect();
        if names.is_empty() {
            continue;
        }
        let ids: Vec<u32> = names.into_iter().map(|name| push(&mut tags, NbtTag::StringTag(name))).collect();
        list.push(rule(&mut tags, NbtTag::ListTag(ids), one.speed as f32, one.drops));
    }
    let speed = float_of(tool, "speed", 6.0, 0.0, 1000.0) as f32;
    for kind in kinds {
        list.push(rule(&mut tags, NbtTag::StringTag(format!("#minecraft:mineable/{kind}")), speed, true));
    }
    let rules = push(&mut tags, NbtTag::ListTag(list));
    let default_speed = push(&mut tags, NbtTag::Float(1.0));
    let damage = push(&mut tags, NbtTag::Int(fields::number(tool, "damagePerBlock", 1, 0, 1000) as i32));
    tags[0] = NbtTag::Compound(vec![
        NbtEntry { key: "rules".into(), value: rules },
        NbtEntry { key: "default_mining_speed".into(), value: default_speed },
        NbtEntry { key: "damage_per_block".into(), value: damage },
    ]);
    NbtTree { root: 0, tags }
}

fn compound_values(tree: &NbtTree) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(NbtTag::Compound(entries)) = tree.tags.get(tree.root as usize) else { return out };
    for entry in entries {
        let text = match tree.tags.get(entry.value as usize) {
            Some(NbtTag::Byte(one)) => (if *one != 0 { "true" } else { "false" }).to_string(),
            Some(NbtTag::Short(one)) => one.to_string(),
            Some(NbtTag::Int(one)) => one.to_string(),
            Some(NbtTag::Long(one)) => one.to_string(),
            Some(NbtTag::Float(one)) => show_value(Some(&json!(f64::from(*one)))),
            Some(NbtTag::Double(one)) => show_value(Some(&json!(*one))),
            Some(NbtTag::StringTag(one)) => one.clone(),
            _ => continue,
        };
        out.insert(entry.key.clone(), text);
    }
    out
}

/// Flat layers, bottom first (`[{"block": "bedrock", "height": 1}, ...]`); none gives the usual grass, like Paper.
fn flat_settings(data: &Data) -> FlatWorldSettings {
    let mut layers: Vec<FlatWorldLayer> = listed(data, "layers")
        .iter()
        .filter_map(|one| {
            let block = clean_name(&fields::text(one, "block", ""));
            if block.is_empty() || pumpkin_plugin_api::world::resolve_block_state(&format!("minecraft:{block}"), &[]).is_none() {
                return None;
            }
            Some(FlatWorldLayer { block: format!("minecraft:{block}"), height: fields::number(one, "height", 1, 1, 384) as u32 })
        })
        .collect();
    if layers.is_empty() {
        layers = vec![
            FlatWorldLayer { block: "minecraft:bedrock".into(), height: 1 },
            FlatWorldLayer { block: "minecraft:dirt".into(), height: 2 },
            FlatWorldLayer { block: "minecraft:grass_block".into(), height: 1 },
        ];
    }
    let biome = clean_name(&fields::text(data, "biome", "plains"));
    FlatWorldSettings { layers, biome: Some(format!("minecraft:{}", if biome.is_empty() { "plains" } else { &biome })) }
}

/// A void world gets one bedrock block under its spawn, like Paper.
async fn void_floor(world: &pumpkin_plugin_api::world::World) {
    let spawn = world.get_spawn_location().pos;
    let under = pumpkin_plugin_api::common::BlockPos { x: spawn.x, y: spawn.y - 1, z: spawn.z };
    let name = pumpkin_plugin_api::world::block_state_to_info(world.get_block_state_id(under)).map(|info| info.name).unwrap_or_default();
    if name.ends_with("air") {
        world.set_block_by_name(under, "minecraft:bedrock".into(), pumpkin_plugin_api::wit::pumpkin::plugin::world::BlockFlags::NOTIFY_LISTENERS).await;
    }
}

async fn holding(player: &Player, wanted: &str) -> bool {
    let Some(hand) = player.get_item_in_hand(Hand::Right) else { return false };
    if wanted.trim().is_empty() {
        return hand.get_registry_key().await != "minecraft:air";
    }
    if let Some(id) = wanted.strip_prefix("studio:") {
        return hand.get_string(TAG, "item").await.as_deref() == Some(id);
    }
    match item_key(wanted) {
        Some(key) => key == hand.get_registry_key().await,
        None => false,
    }
}

/// Whether a file's action or condition lists use a step of this type.
fn uses_step(value: &Value, wanted: &str) -> bool {
    let steps = |key: &str| key.ends_with("Actions") || matches!(key, "actions" | "then" | "else" | "if" | "deny" | "rewards" | "conditions");
    match value {
        Value::Object(map) => map.iter().any(|(key, inner)| {
            (steps(key) && inner.as_array().is_some_and(|list| list.iter().any(|one| one.get("type").and_then(Value::as_str) == Some(wanted)))) || uses_step(inner, wanted)
        }),
        Value::Array(list) => list.iter().any(|one| uses_step(one, wanted)),
        _ => false,
    }
}

/// M6: whether a file's action or condition lists use one another plugin adds (`"type": "plugin:name"`).
fn uses_expansion(value: &Value) -> bool {
    let steps = |key: &str| key.ends_with("Actions") || matches!(key, "actions" | "then" | "else" | "if" | "deny" | "rewards" | "conditions");
    match value {
        Value::Object(map) => map.iter().any(|(key, inner)| {
            (steps(key) && inner.as_array().is_some_and(|list| list.iter().any(|one| one.get("type").and_then(Value::as_str).is_some_and(|kind| kind.contains(':')))))
                || uses_expansion(inner)
        }),
        Value::Array(list) => list.iter().any(uses_expansion),
        _ => false,
    }
}

fn valid_command(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'-' || one == b'_')
}

fn advancement_key(id: &str) -> String {
    let id = id.trim();
    if id.contains(':') { id.to_string() } else { format!("{TAG}:{id}") }
}

/// Puts a stack into the player's inventory: onto matching stacks first, then empty slots. What doesn't fit drops
/// at their feet.
pub(super) async fn give(player: &Player, stack: ItemStack) {
    let key = stack.get_registry_key().await;
    let tag = stack.get_string(TAG, "item").await;
    let named = stack.get_custom_name().await.is_some();
    let max = stack.get_max_count().await;
    let mut left = stack.get_count().await;
    for slot in 0..36u8 {
        if left == 0 {
            return;
        }
        let Some(there) = player.get_inventory_item(slot) else { continue };
        let count = there.get_count().await;
        if count < max && there.get_registry_key().await == key && there.get_string(TAG, "item").await == tag && there.get_custom_name().await.is_some() == named {
            let moved = left.min(max - count);
            there.set_count(count + moved).await;
            player.set_inventory_item(slot, Some(there)).await;
            left -= moved;
        }
    }
    for slot in 0..36u8 {
        if left == 0 {
            return;
        }
        let empty = match player.get_inventory_item(slot) {
            Some(one) => one.get_registry_key().await == "minecraft:air",
            None => true,
        };
        if empty {
            let copy = stack.clone().await;
            copy.set_count(left).await;
            player.set_inventory_item(slot, Some(copy)).await;
            left = 0;
        }
    }
    if left > 0 {
        let rest = stack.clone().await;
        rest.set_count(left).await;
        player.get_world().drop_item(player.get_position(), rest).await;
    }
}

/// The permission nodes a dialog file names (its own, its buttons' and its canvas elements') that the player has,
/// asked once up front because the canvas is drawn with a plain closure.
async fn allowed_nodes(player: &Player, data: &Data) -> HashSet<String> {
    fn nodes(value: &Value, out: &mut HashSet<String>) {
        match value {
            Value::Object(map) => {
                for (key, inner) in map {
                    match (key.as_str(), inner) {
                        ("permission", Value::String(node)) if !node.trim().is_empty() => {
                            out.insert(node.clone());
                        }
                        _ => nodes(inner, out),
                    }
                }
            }
            Value::Array(list) => list.iter().for_each(|one| nodes(one, out)),
            _ => {}
        }
    }
    let mut named = HashSet::new();
    nodes(&Value::Object(data.clone()), &mut named);
    let mut allowed = HashSet::new();
    for node in named {
        if player.has_permission(node.clone()).await {
            allowed.insert(node);
        }
    }
    allowed
}

/// Our tagged NPCs in every world, by file name.
async fn tagged_npcs(server: &Server) -> HashMap<String, Vec<Entity>> {
    let mut out: HashMap<String, Vec<Entity>> = HashMap::new();
    for world in server.get_all_worlds() {
        for entity in world.get_entities() {
            if let Some(id) = entity.get_string(TAG, "npc").await {
                out.entry(id).or_default().push(entity);
            }
        }
    }
    out
}

/// Game rules by their 26.x name (`advance_time`) or the old one (`doDaylightCycle`).
fn game_rule(name: &str) -> Option<pumpkin_plugin_api::GameRule> {
    let renamed = match name {
        "doDaylightCycle" => "advance_time",
        "doWeatherCycle" => "advance_weather",
        "doMobSpawning" => "spawn_mobs",
        "doTileDrops" => "block_drops",
        "doEntityDrops" => "entity_drops",
        "doMobLoot" => "mob_drops",
        "doImmediateRespawn" => "immediate_respawn",
        "naturalRegeneration" => "natural_health_regeneration",
        "announceAdvancements" => "show_advancement_messages",
        "doInsomnia" => "spawn_phantoms",
        "doPatrolSpawning" => "spawn_patrols",
        "doTraderSpawning" => "spawn_wandering_traders",
        "doWardenSpawning" => "spawn_wardens",
        other => other,
    };
    let mut snake = String::new();
    for letter in renamed.chars() {
        if letter.is_ascii_uppercase() {
            snake.push('_');
            snake.push(letter.to_ascii_lowercase());
        } else {
            snake.push(letter);
        }
    }
    names::game_rule(&snake)
}

/// A recipe the server turned down (an unknown item, a bad pattern), for its status.
fn refused(reason: String) -> String {
    format!("The server didn't take this recipe: {reason}")
}

/// What a studio command runs: the name Pumpkin matched, who typed it, and whatever followed the name.
pub type CommandRun = std::sync::Arc<dyn Fn(String, pumpkin_plugin_api::command::CommandSender, Server, Option<String>) -> pumpkin_plugin_api::BoxFuture<'static, ()> + Send + Sync>;

/// Commands: a handler per registered name. Pumpkin calls it with whatever followed the name.
pub fn command(names: Vec<String>, description: &str, handler: CommandRun) -> Command {
    let main = names[0].clone();
    let plain = std::sync::Arc::clone(&handler);
    let name = main.clone();
    let command = Command::new(&names, description).execute(Handler(move |sender, server, _args| plain(name.clone(), sender, server, None)));
    let name = main;
    command.then(CommandNode::argument("args", &ArgumentType::String(StringType::Greedy)).execute(Handler(move |sender, server, args: pumpkin_plugin_api::command::ConsumedArgs| {
        let text = match args.get_value("args") {
            pumpkin_plugin_api::command::Arg::Simple(text) | pumpkin_plugin_api::command::Arg::Msg(text) => text,
            _ => String::new(),
        };
        handler(name.clone(), sender, server, Some(text))
    })))
}

struct Handler<F>(F);

impl<F> pumpkin_plugin_api::commands::CommandHandler for Handler<F>
where
    F: Fn(pumpkin_plugin_api::command::CommandSender, Server, pumpkin_plugin_api::command::ConsumedArgs) -> pumpkin_plugin_api::BoxFuture<'static, ()> + Send + Sync,
{
    async fn handle(&self, sender: pumpkin_plugin_api::command::CommandSender, server: Server, args: pumpkin_plugin_api::command::ConsumedArgs) -> pumpkin_plugin_api::Result<i32, pumpkin_plugin_api::command::CommandError> {
        (self.0)(sender, server, args).await;
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_like_the_paper_addon() {
        assert!(compare(Some(&json!(5)), ">=", "3"));
        assert!(!compare(Some(&json!(2)), ">", "3"));
        assert!(compare(Some(&json!("Red")), "=", "red"));
        assert!(compare(None, "=", "0") || compare(None, "!=", "0"));
        assert!(compare(Some(&json!("abc")), "!=", "xyz"));
    }

    #[test]
    fn changes_variables() {
        assert_eq!(change(None, "add", "2"), json!(2.0));
        assert_eq!(change(Some(&json!(5)), "subtract", "2"), json!(3.0));
        assert_eq!(change(Some(&json!(5)), "set", "hello"), json!("hello"));
        assert_eq!(show_value(Some(&json!(3.0))), "3");
        assert_eq!(show_value(None), "0");
    }

    #[test]
    fn reads_names() {
        assert_eq!(clean_name("minecraft:Diamond_Ore"), "diamond_ore");
        assert_eq!(clean_name("oak_log[axis=y]"), "oak_log");
        assert!(valid_command("spawn"));
        assert!(!valid_command("Spawn"));
        assert_eq!(advancement_key("first"), "monolab:first");
        assert_eq!(advancement_key("minecraft:story/root"), "minecraft:story/root");
        assert!(game_rule("doDaylightCycle").is_some());
        assert!(game_rule("keepInventory").is_some());
        assert!(game_rule("keep_inventory").is_some());
        assert!(game_rule("nope").is_none());
    }

    #[test]
    fn finds_actions_other_plugins_add() {
        assert!(uses_expansion(&json!({ "then": [{ "type": "shop:give_money", "amount": "5" }] })));
        assert!(uses_expansion(&json!({ "skills": [{ "trigger": "attack", "conditions": [{ "type": "shop:has_money" }] }] })));
        assert!(uses_expansion(&json!({ "slots": [{ "clickActions": [{ "type": "x:y" }] }] })));
        assert!(!uses_expansion(&json!({ "then": [{ "type": "give", "item": "minecraft:diamond" }] })));
        assert!(!uses_expansion(&json!({ "look": { "type": "minecraft:paper" } })));
    }
}
