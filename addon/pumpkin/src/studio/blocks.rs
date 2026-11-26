//! Studio maker M3 on Pumpkin: custom blocks (`block`) and furniture (`furniture`), from the same files and the same
//! `ids.json` numbers as Paper (`StudioBlocks.java`, `StudioFurniture.java`). The rules (which state a number borrows,
//! break times, turning cells) are the shared `studio_format::{block, furniture}`.
//!
//! This file decides; the studio runs what it decides (action lists, giving items, loot rolls), so it only needs
//! Pumpkin's world and entity calls. The event handlers in `lib.rs` call in here.
//!
//! Breaking runs on our own clock, like Paper: while a player digs one of ours their break speed is held at 0, the
//! cracks are sent from here every tick, and when its time is up the studio breaks it ([`Blocks::tick_digging`]).
//! Broken furniture drops its item where it stood.
//!
//! Every borrowed state of ours, and the plain note block state, is frozen on the fork (`server.freeze-block-state`):
//! the game leaves it out of neighbour shape updates, redstone, ticks and its own use/attack, like CraftEngine's real
//! blocks. The physics, note and piston holds below stay as a second guard. A state is given back when it stops
//! being ours and when the add-on stops; one another plugin had already frozen is left alone.
//!
//! Blocks enabled when the server starts are real blocks of the fork's (`custom-block.wit`, [`Blocks::register`]):
//! `monolab:<id>` with its own hardness, tool tags, light and sounds, shown to players as the same borrowed state, so
//! the pack draws them unchanged. The server digs them itself (cracks, tool speed, `needsTool`) and asks us for their
//! drops (the `drops` callback). The fork only takes new blocks while plugins load, so a block made later, or one
//! whose shape, hardness, tool, sounds or light changed since, works the old way until the next start: placed as its
//! borrowed state and dug on our clock. Blocks placed before this (borrowed states in the world) keep working that
//! way too.
//!
//! Crops (`growth`) borrow a plant state per stage (`<id>/<stage>` in `ids.json`). Registered at start they are one
//! fork block with an `age` property, each age shown as its stage's borrowed state, growing on the fork's random
//! ticks and bone meal (`on-random-tick`, `on-bonemeal`). Pumpkin can't tell us a block's light yet, so `light` is
//! Paper's only. A crop made after the start breaks and harvests like one, but doesn't grow until a restart.
//!
//! Standing up from a seat (the fork's `EntityDismountEvent`) takes the seat away and puts the player on top, like
//! Paper's `onStand`; [`Blocks::tick`] still clears empty seats once a second in case the event was missed.

use pumpkin_plugin_api::common::Hand;
use pumpkin_plugin_api::particles::Particle;
use pumpkin_plugin_api::persistent_data::PersistentDataHolder;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::uuid::Uuid;
use pumpkin_plugin_api::wit::pumpkin::plugin::data_components::DataComponent;
use pumpkin_plugin_api::wit::pumpkin::plugin::sounds::SoundCategory;
use pumpkin_plugin_api::wit::pumpkin::plugin::world::{BlockDirection, BlockFlags, BlockPos};
use pumpkin_plugin_api::world::{Entity, World};
use pumpkin_plugin_api::{
    Attribute, AttributeModifier, DisplayEntityExt, EntityDisplayExt, EntityType, ItemDisplayEntityExt, ItemDisplayMode, ItemStack,
    ModifierOperation, Player, Server,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use studio_format::fields::{self, Data};
use studio_format::{block, furniture};

/// Our tag namespace (the same as `play::TAG`).
const TAG: &str = "monolab";
/// The attribute modifier that matches a player's dig time to one of our blocks.
const DIG_MODIFIER: &str = "monolab:block_breaking";
/// How far a player reaches to place.
const REACH: f64 = 5.0;
/// Players this near a block being dug see its cracks.
const CRACK_DISTANCE: f64 = 32.0;

/// A player digging one of ours, on our clock.
struct Digging {
    world: World,
    at: (i32, i32, i32),
    needed: u32,
    done: u32,
}

/// A loaded block.
struct Loaded {
    id: String,
    data: Data,
    shape: String,
    /// The borrowed state (what players see, and what blocks placed the old way are).
    state: u16,
    /// Its own state on the fork, when it was registered at start and hasn't changed since (a crop's age 0).
    custom: Option<u16>,
    /// A crop's later stages' borrowed states (stage 1..).
    stages: Vec<u16>,
    /// How many ages a registered crop has on the fork (its states run from `custom`); 1 for other blocks.
    ages: u16,
}

impl Loaded {
    /// A crop's stage for a state of its own: 0 for its first, then 1.. (borrowed or fork ages alike).
    fn stage(&self, state: u16) -> u32 {
        if let Some(first) = self.custom
            && state >= first
            && state < first + self.ages
        {
            return u32::from(state - first);
        }
        self.stages.iter().position(|one| *one == state).map_or(0, |at| at as u32 + 1)
    }
}

/// A block registered with the fork when the server started.
pub struct Registered {
    /// Its own state (a crop's age 0).
    state: u16,
    /// How many states it has (a crop's ages).
    count: u16,
    /// The borrowed state players are shown.
    client: u16,
    /// What it was registered with ([`print`]): a change needs a restart.
    print: String,
}

/// The parts of a block file the fork's registration holds, to tell when a restart is needed.
fn print(data: &Data, shape: &str, client: u16, stages: &[u16]) -> String {
    serde_json::json!({
        "stages": stages,
        "shape": shape,
        "hardness": block::hardness(data),
        "tool": fields::text(data, "tool", ""),
        "needsTool": data.get("needsTool").and_then(Value::as_bool) == Some(true),
        "sounds": block::sounds(data),
        "light": block::light(data),
        "client": client,
    })
    .to_string()
}

/// A crop's later stages' borrowed states, as far as `ids.json` has them (fewer than its stages = not built yet).
fn stage_states(given: &serde_json::Map<String, Value>, id: &str, data: &Data) -> Vec<u16> {
    let Some(growth) = block::growth(data) else { return Vec::new() };
    let mut out = Vec::new();
    for stage in 1..=growth.last() {
        let number =
            given.get(block::IDS_PLANT).and_then(|kind| kind.get(block::stage_key(id, stage))).and_then(Value::as_u64).unwrap_or(0) as u32;
        match borrowed_state("plant", number) {
            Ok(state) if number > 0 => out.push(state),
            _ => break,
        }
    }
    out
}

/// The borrowed state for block number `number` of `shape`, or what is wrong.
fn borrowed_state(shape: &str, number: u32) -> Result<u16, String> {
    let Some(state) = block::state(shape, number) else {
        return Err(format!("No free block state is left for this block ({} at most).", block::most(shape)));
    };
    let properties: Vec<(String, String)> = state.properties.iter().map(|(name, value)| ((*name).to_string(), value.clone())).collect();
    pumpkin_plugin_api::world::resolve_block_state(&format!("minecraft:{}", state.block), &properties)
        .ok_or_else(|| format!("Pumpkin doesn't know the block state {}.", state.text()))
}

/// The fork's definition of a studio block: one state shown as `client`.
fn definition(id: &str, data: &Data, shape: &str, client: u16, stages: &[u16]) -> pumpkin_plugin_api::custom_block::BlockDefinition {
    use pumpkin_plugin_api::custom_block::{
        BlockCallbacks, BlockDefinition, BlockProperty, BlockShape, BlockStateDefinition, PistonReaction, ShapeBox, SoundGroup,
    };
    let plant = shape == "plant";
    let group = sound_group(&block::sounds(data));
    let sound = |what: &str| format!("minecraft:block.{group}.{what}");
    let tool = fields::text(data, "tool", "").trim().to_string();
    let state = BlockStateDefinition {
        client_state: client,
        hardness: block::hardness(data) as f32,
        resistance: block::hardness(data) as f32,
        luminance: block::light(data),
        opacity: plant.then_some(0),
        collision: if plant { BlockShape::Empty } else { BlockShape::FullCube },
        outline: plant.then(|| {
            BlockShape::Boxes(vec![ShapeBox { min_x: 0.125, min_y: 0.0, min_z: 0.125, max_x: 0.875, max_y: 0.8125, max_z: 0.875 }])
        }),
        piston: if plant { PistonReaction::Destroy } else { PistonReaction::Block },
        sound_group: if plant { "GRASS".into() } else { "STONE".into() },
        custom_sounds: Some(SoundGroup {
            volume: 1.0,
            pitch: 0.8,
            break_sound: sound("break"),
            step_sound: sound("step"),
            place_sound: sound("place"),
            hit_sound: sound("hit"),
            fall_sound: sound("fall"),
        }),
        instrument: None,
        map_color: if plant { 7 } else { 11 },
        loot_table: None,
        requires_tool: !tool.is_empty() && data.get("needsTool").and_then(Value::as_bool) == Some(true),
        replaceable: false,
        burnable: false,
        random_ticks: false,
        redstone_conductor: plant.then_some(false),
        occludes: plant.then_some(false),
    };
    // A crop: one state per age, each shown as its stage's borrowed state; all but the last get random ticks.
    let crop = !stages.is_empty();
    let mut states = vec![BlockStateDefinition { random_ticks: crop, ..state.clone() }];
    for (at, stage_client) in stages.iter().enumerate() {
        states.push(BlockStateDefinition { client_state: *stage_client, random_ticks: at + 1 < stages.len(), ..state.clone() });
    }
    let properties = if crop {
        vec![BlockProperty { name: "age".into(), values: (0..=stages.len()).map(|age| age.to_string()).collect() }]
    } else {
        Vec::new()
    };
    let callbacks = if crop {
        BlockCallbacks::DROPS | BlockCallbacks::ON_RANDOM_TICK | BlockCallbacks::CAN_BONEMEAL | BlockCallbacks::ON_BONEMEAL
    } else {
        BlockCallbacks::DROPS
    };
    BlockDefinition {
        id: format!("{TAG}:{id}"),
        properties,
        default_state: 0,
        states,
        friction: None,
        speed_factor: None,
        jump_factor: None,
        item: Some(format!("minecraft:{}", if plant { block::PLANT_ITEM } else { block::CUBE_ITEM })),
        flammable: None,
        tags: if tool.is_empty() { Vec::new() } else { vec![format!("minecraft:mineable/{tool}")] },
        // Drops come from us (own item, linked item, loot roll), like the borrowed blocks'.
        callbacks,
        block_entity: None,
    }
}

/// Something the studio should do after a click or a break.
#[derive(Default)]
pub struct Outcome {
    /// Cancel the event (the click was ours).
    pub cancel: bool,
    /// An action list to run for the player, with its placeholders.
    pub actions: Option<Value>,
    pub extra: Vec<(String, String)>,
    /// Items for the player or the drop list.
    pub items: Vec<ItemStack>,
    /// A studio item to give (`studio:<id>` for a linked item; the caller builds it).
    pub studio_item: Option<String>,
    /// Studio loot tables to roll into the drops.
    pub loot: Vec<String>,
    /// Drop the items here (furniture) instead of giving them to the player.
    pub drop_at: Option<(World, (f64, f64, f64))>,
}

pub struct Blocks {
    ids_file: PathBuf,
    blocks: HashMap<String, Loaded>,
    by_state: HashMap<u16, String>,
    by_item: HashMap<String, String>,
    furniture: HashMap<String, Data>,
    furniture_by_item: HashMap<String, String>,
    /// Blocks just broken, by position, for the drop event that follows: id and whether it was a grown crop.
    broken: HashMap<(i32, i32, i32), (String, bool)>,
    /// A small random source for crop growth (wasm has no other).
    seed: u64,
    /// The vanilla state each borrowed block is reset to when placed plainly (note block: harp, note 0).
    plain_note: Option<u16>,
    /// The states we froze (and so give back), not ones another plugin froze first.
    frozen: HashSet<u16>,
    /// Blocks registered with the fork at start, by file id.
    registered: HashMap<String, Registered>,
    /// Blocks the fork turned down at start, with why.
    refused: HashMap<String, String>,
    /// The states we gave a break hardness (`set-block-state-hardness`), so the client's own guess (a tripwire
    /// breaks at once) is held back while our digging counts.
    hardened: HashSet<u16>,
    /// The plain note state is frozen, so the game no longer plays it when hit: we do.
    plain_quiet: bool,
    /// Who is digging one of ours, by player uuid.
    digging: HashMap<(u64, u64), Digging>,
    /// The online players as of the last digging tick, who see the cracks.
    viewers: Vec<Player>,
    /// Who stood up from which seat (rider id, seat id), handled on the next tick.
    stood: Vec<(u32, u32)>,
    /// When each player last placed one of ours (ms): one click can arrive as two interact events, and the second
    /// would place another against the first.
    last_placed: HashMap<(u64, u64), u64>,
}

/// The crack overlay id for a block: negative (no entity has it) and the same for the same block, so two blocks
/// dug near one player don't share an overlay.
fn crack_source(at: (i32, i32, i32)) -> i32 {
    let mixed = (at.0 as u32).wrapping_mul(73_856_093) ^ (at.1 as u32).wrapping_mul(19_349_663) ^ (at.2 as u32).wrapping_mul(83_492_791);
    -1 - (mixed & 0x3FFF_FFFF) as i32
}

fn var_int(out: &mut Vec<u8>, value: i32) {
    let mut left = value as u32;
    loop {
        if left & !0x7F == 0 {
            out.push(left as u8);
            return;
        }
        out.push((left & 0x7F | 0x80) as u8);
        left >>= 7;
    }
}

/// An item showing `monolab:<path>` from the app's pack.
async fn with_model(material: &str, path: &str) -> ItemStack {
    let stack = ItemStack::new(material, 1);
    let name = format!("monolab:{path}");
    let mut bytes = Vec::new();
    var_int(&mut bytes, name.len() as i32);
    bytes.extend_from_slice(name.as_bytes());
    stack.set_component(DataComponent::ItemModel, bytes).await;
    stack
}

fn ids_given(file: &Path) -> serde_json::Map<String, Value> {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|all| all.get("given").and_then(Value::as_object).cloned())
        .unwrap_or_default()
}

fn pos(at: (i32, i32, i32)) -> BlockPos {
    BlockPos { x: at.0, y: at.1, z: at.2 }
}

fn flags() -> BlockFlags {
    BlockFlags::NOTIFY_LISTENERS | BlockFlags::FORCE_STATE
}

/// The vanilla sound group a block name sounds like, for Pumpkin's named sounds.
fn sound_group(name: &str) -> &'static str {
    let has = |part: &str| name.contains(part);
    if has("planks") || has("log") || has("wood") || has("chest") {
        "wood"
    } else if has("glass") {
        "glass"
    } else if has("wool") || has("carpet") {
        "wool"
    } else if has("sand") {
        "sand"
    } else if has("gravel") {
        "gravel"
    } else if has("grass") || has("leaves") || has("fern") {
        "grass"
    } else if has("iron") || has("gold") || has("copper") || has("metal") {
        "metal"
    } else {
        "stone"
    }
}

/// The block name at a spot (`minecraft:stone`).
fn name_at(world: &World, at: (i32, i32, i32)) -> String {
    pumpkin_plugin_api::world::block_state_to_info(world.get_block_state_id(pos(at))).map(|info| info.name).unwrap_or_default()
}

fn is_air(name: &str) -> bool {
    let bare = name.trim_start_matches("minecraft:");
    matches!(
        bare,
        "air"
            | "cave_air"
            | "void_air"
            | "short_grass"
            | "tall_grass"
            | "fern"
            | "snow"
            | "water"
            | "lava"
            | "light"
            | "dead_bush"
            | "vine"
    )
}

fn step(at: (i32, i32, i32), face: BlockDirection) -> (i32, i32, i32) {
    match face {
        BlockDirection::Down => (at.0, at.1 - 1, at.2),
        BlockDirection::Up => (at.0, at.1 + 1, at.2),
        BlockDirection::North => (at.0, at.1, at.2 - 1),
        BlockDirection::South => (at.0, at.1, at.2 + 1),
        BlockDirection::West => (at.0 - 1, at.1, at.2),
        BlockDirection::East => (at.0 + 1, at.1, at.2),
    }
}

impl Blocks {
    /// The state one of our blocks is placed as (its own when registered), for world generators.
    pub fn state_of(&self, id: &str) -> Option<u16> {
        self.blocks.get(id).map(|one| one.custom.unwrap_or(one.state))
    }

    /// While plugins load: makes every enabled block a block of the fork's own, its drops answered by `handler`. A
    /// block the fork turns down is remembered for its status.
    pub fn register<H: pumpkin_plugin_api::custom_block::CustomBlockHandler + Clone + 'static>(
        &mut self,
        rows: &[(String, Data)],
        handler: &H,
    ) {
        if !pumpkin_plugin_api::custom_block::is_registration_open() {
            return;
        }
        let given = ids_given(&self.ids_file);
        for (id, data) in rows {
            if !block::problems(data).is_empty() || self.registered.contains_key(id) {
                continue;
            }
            let shape = block::shape(data);
            let number = given.get(block::ids_kind(&shape)).and_then(|kind| kind.get(id)).and_then(Value::as_u64).unwrap_or(0) as u32;
            let Ok(client) = borrowed_state(&shape, number) else { continue };
            let stages = stage_states(&given, id, data);
            match pumpkin_plugin_api::custom_block::register_block(definition(id, data, &shape, client, &stages), handler.clone()) {
                Ok(done) => {
                    self.registered.insert(
                        id.clone(),
                        Registered {
                            state: done.first_state,
                            count: done.state_count,
                            client,
                            print: print(data, &shape, client, &stages),
                        },
                    );
                }
                Err(error) => {
                    self.refused.insert(
                        id.clone(),
                        format!("Pumpkin didn't take it as a block of its own ({error}); it borrows a state like on Paper."),
                    );
                }
            }
        }
    }

    /// The loaded blocks placed as their own fork block.
    pub fn custom_ids(&self) -> Vec<String> {
        self.blocks.values().filter(|one| one.custom.is_some()).map(|one| one.id.clone()).collect()
    }

    /// Whether the state at `at` is one of our registered blocks (the server digs and drops it itself).
    fn custom_at(&self, world: &World, at: (i32, i32, i32)) -> bool {
        let state = world.get_block_state_id(pos(at));
        self.registered.values().any(|one| state >= one.state && state < one.state + one.count)
    }

    pub fn new(studio_root: &Path) -> Self {
        Self {
            ids_file: studio_root.join("pack").join("ids.json"),
            blocks: HashMap::new(),
            by_state: HashMap::new(),
            by_item: HashMap::new(),
            furniture: HashMap::new(),
            furniture_by_item: HashMap::new(),
            broken: HashMap::new(),
            seed: crate::now_ms() | 1,
            plain_note: None,
            frozen: HashSet::new(),
            registered: HashMap::new(),
            refused: HashMap::new(),
            hardened: HashSet::new(),
            plain_quiet: false,
            digging: HashMap::new(),
            viewers: Vec::new(),
            stood: Vec::new(),
            last_placed: HashMap::new(),
        }
    }

    /// Takes the enabled block and furniture files after a reload. Returns what was wrong: (kind, id, text).
    pub fn load(
        &mut self,
        server: &Server,
        blocks: &[(String, Data)],
        furniture_files: &[(String, Data)],
    ) -> Vec<(&'static str, String, String)> {
        let mut problems = Vec::new();
        let given = ids_given(&self.ids_file);
        self.blocks.clear();
        self.by_state.clear();
        self.by_item.clear();
        self.plain_note = pumpkin_plugin_api::world::resolve_block_state(
            "minecraft:note_block",
            &[("instrument".into(), "harp".into()), ("note".into(), "0".into()), ("powered".into(), "false".into())],
        );
        for (id, data) in blocks {
            if let Some(wrong) = block::problems(data).into_iter().next() {
                problems.push(("block", id.clone(), wrong));
                continue;
            }
            let shape = block::shape(data);
            let number = given.get(block::ids_kind(&shape)).and_then(|kind| kind.get(id)).and_then(Value::as_u64).unwrap_or(0) as u32;
            if number == 0 {
                problems.push((
                    "block",
                    id.clone(),
                    "This block has no state yet: save it in the app so the resource pack is built.".into(),
                ));
                continue;
            }
            let state_id = match borrowed_state(&shape, number) {
                Ok(state) => state,
                Err(problem) => {
                    problems.push(("block", id.clone(), problem));
                    continue;
                }
            };
            let stages = stage_states(&given, id, data);
            if let Some(growth) = block::growth(data)
                && stages.len() < growth.stages.len()
            {
                problems.push((
                    "block",
                    id.clone(),
                    format!("Stage {} has no plant state yet: save it in the app so the resource pack is built.", stages.len() + 1),
                ));
                continue;
            }
            let mut ages = 1;
            let custom = match self.registered.get(id) {
                Some(one) if one.client == state_id && one.print == print(data, &shape, state_id, &stages) => {
                    ages = one.count;
                    Some(one.state)
                }
                Some(_) => {
                    problems.push(("block", id.clone(), "Restart the server to use the changed shape, hardness, tool, sounds or light; until then it is placed the old way.".into()));
                    None
                }
                None if self.refused.contains_key(id) => {
                    problems.push(("block", id.clone(), self.refused[id].clone()));
                    None
                }
                None => {
                    problems.push(("block", id.clone(), "Made after the server started: restart it so Pumpkin knows it as a block of its own (light, break time). Until then it borrows a state like on Paper.".into()));
                    None
                }
            };
            if let Some(custom) = custom {
                for age in 0..ages {
                    self.by_state.insert(custom + age, id.clone());
                }
            }
            for stage in &stages {
                self.by_state.insert(*stage, id.clone());
            }
            let link = fields::text(data, "item", "").trim().to_string();
            if !link.is_empty() {
                self.by_item.insert(link, id.clone());
            }
            self.by_state.insert(state_id, id.clone());
            self.blocks.insert(id.clone(), Loaded { id: id.clone(), data: data.clone(), shape, state: state_id, custom, stages, ages });
        }
        self.furniture.clear();
        self.furniture_by_item.clear();
        for (id, data) in furniture_files {
            if let Some(wrong) = furniture::problems(data).into_iter().next() {
                problems.push(("furniture", id.clone(), wrong));
                continue;
            }
            let link = fields::text(data, "item", "").trim().to_string();
            if !link.is_empty() {
                self.furniture_by_item.insert(link, id.clone());
            }
            self.furniture.insert(id.clone(), data.clone());
        }
        self.freeze(server);
        problems
    }

    /// Freezes the states in use now (ours and, while there are any, the plain note block) and gives back the ones we
    /// froze that aren't any more.
    fn freeze(&mut self, server: &Server) {
        let mut wanted: HashSet<u16> = self.by_state.keys().copied().collect();
        if !self.blocks.is_empty()
            && let Some(plain) = self.plain_note
        {
            wanted.insert(plain);
        }
        for state in self.frozen.iter().copied().filter(|state| !wanted.contains(state)).collect::<Vec<_>>() {
            server.unfreeze_block_state(state);
            self.frozen.remove(&state);
        }
        for state in wanted {
            if !self.frozen.contains(&state) && server.freeze_block_state(state) == Ok(true) {
                self.frozen.insert(state);
            }
        }
        self.plain_quiet = !self.blocks.is_empty() && self.plain_note.is_some_and(|plain| server.is_block_state_frozen(plain));
        let hard: HashMap<u16, f32> = self
            .blocks
            .values()
            .flat_map(|one| {
                std::iter::once(one.state).chain(one.stages.iter().copied()).map(|state| (state, block::hardness(&one.data) as f32))
            })
            .collect();
        for state in self.hardened.iter().copied().filter(|state| !hard.contains_key(state)).collect::<Vec<_>>() {
            server.set_block_state_hardness(state, None);
            self.hardened.remove(&state);
        }
        for (state, hardness) in hard {
            server.set_block_state_hardness(state, Some(hardness));
            self.hardened.insert(state);
        }
    }

    /// Gives every state we froze its vanilla behaviour back (the add-on stops).
    pub fn unfreeze_all(&mut self, server: &Server) {
        for state in self.frozen.drain() {
            server.unfreeze_block_state(state);
        }
        for state in self.hardened.drain() {
            server.set_block_state_hardness(state, None);
        }
        self.plain_quiet = false;
    }

    fn at(&self, world: &World, at: (i32, i32, i32)) -> Option<&Loaded> {
        if self.by_state.is_empty() {
            return None;
        }
        self.by_state.get(&world.get_block_state_id(pos(at))).and_then(|id| self.blocks.get(id))
    }

    /// The block or piece of furniture a held stack places: its own item, or a studio item one links to.
    async fn placed_by(&self, stack: &ItemStack) -> (Option<String>, Option<String>) {
        if let Some(id) = stack.get_string(TAG, "block").await.filter(|id| self.blocks.contains_key(id)) {
            return (Some(id), None);
        }
        if let Some(id) = stack.get_string(TAG, "furniture").await.filter(|id| self.furniture.contains_key(id)) {
            return (None, Some(id));
        }
        let Some(item) = stack.get_string(TAG, "item").await else { return (None, None) };
        (self.by_item.get(&item).cloned(), self.furniture_by_item.get(&item).cloned())
    }

    /// A block's or piece's own item (`block/<id>`, `furniture/<id>`), named by `name`. `None` for a linked one (the
    /// studio gives `studio:<item>`) or an unknown id.
    pub async fn own_item(&self, reference: &str, name: &dyn Fn(&str) -> TextComponent) -> Option<ItemStack> {
        let (data, stack) = if let Some(id) = reference.strip_prefix("block/") {
            let one = self.blocks.get(id)?;
            let material = if one.shape == "plant" { block::PLANT_ITEM } else { block::CUBE_ITEM };
            let stack = with_model(&format!("minecraft:{material}"), &format!("block/{id}")).await;
            stack.set_string(TAG, "block", id).await;
            (&one.data, stack)
        } else {
            let id = reference.strip_prefix("furniture/")?;
            let data = self.furniture.get(id)?;
            let stack = with_model(&format!("minecraft:{}", furniture::ITEM), &format!("furniture/{id}")).await;
            stack.set_string(TAG, "furniture", id).await;
            (data, stack)
        };
        if !fields::text(data, "item", "").trim().is_empty() {
            return None;
        }
        if fields::has(data, "name") {
            stack.set_custom_name(Some(name(&fields::text(data, "name", "")))).await;
        }
        let lore: Vec<TextComponent> = fields::lines(data, "lore").iter().map(|line| name(line)).collect();
        if !lore.is_empty() {
            stack.set_lore(lore).await;
        }
        Some(stack)
    }

    /// The linked studio item that stands for a block or piece, when it has one.
    fn link(data: &Data) -> Option<String> {
        let link = fields::text(data, "item", "").trim().to_string();
        (!link.is_empty()).then_some(link)
    }

    async fn give_back(&self, reference: &str, data: &Data, out: &mut Outcome, name: &dyn Fn(&str) -> TextComponent) {
        match Self::link(data) {
            Some(link) => out.studio_item = Some(link),
            None => out.items.extend(self.own_item(reference, name).await),
        }
    }

    /// What a middle click on one of ours picks: what breaking it gives back (its own item or the linked studio
    /// item). `state` is the picked block's; `None` when it isn't ours.
    pub async fn pick_block(&self, state: u16, name: &dyn Fn(&str) -> TextComponent) -> Option<Outcome> {
        let one = self.by_state.get(&state).and_then(|id| self.blocks.get(id))?;
        let mut out = Outcome::default();
        self.give_back(&format!("block/{}", one.id), &one.data, &mut out, name).await;
        Some(out)
    }

    /// What a middle click on a piece's display or hitbox picks, like [`Self::pick_block`].
    pub async fn pick_piece(&self, player: &Player, entity_id: i32, name: &dyn Fn(&str) -> TextComponent) -> Option<Outcome> {
        let (_, _, id) = Self::piece(player, entity_id).await?;
        let data = self.furniture.get(&id)?;
        let mut out = Outcome::default();
        self.give_back(&format!("furniture/{id}"), data, &mut out, name).await;
        Some(out)
    }

    // ---------- clicks ----------

    /// A right click on a block: our block's use actions, or placing a held studio block or piece of furniture.
    pub async fn right_click(&mut self, player: &Player, name: &dyn Fn(&str) -> TextComponent) -> Outcome {
        let mut out = Outcome::default();
        if self.blocks.is_empty() && self.furniture.is_empty() {
            return out;
        }
        let body = player.as_entity();
        let Some(hit) = body.raycast(REACH, false) else { return out };
        let world = body.get_world();
        let clicked = (hit.pos.x, hit.pos.y, hit.pos.z);
        let sneaking = body.is_sneaking();
        let held_now = player.get_item_in_hand(Hand::Right);
        let placing = match held_now.as_ref() {
            Some(held) => {
                let (block, furniture) = self.placed_by(held).await;
                block.is_some() || furniture.is_some()
            }
            None => false,
        };
        if !sneaking && !placing && self.plain_note_at(&world, clicked) {
            out.cancel = true;
            self.tune_plain(&world, clicked).await;
            return out;
        }
        if let Some(one) = self.at(&world, clicked).filter(|_| !sneaking) {
            // Bone meal on a crop is the fork's (`on-bonemeal`), so the click goes through.
            if block::growth(&one.data).is_some()
                && let Some(held) = held_now.as_ref()
                && held.get_registry_key().await.trim_start_matches("minecraft:") == "bone_meal"
            {
                return out;
            }
            out.cancel = true;
            if let Some(list) = one.data.get("useActions").filter(|list| list.as_array().is_some_and(|list| !list.is_empty())) {
                out.actions = Some(list.clone());
                out.extra = vec![("block".into(), one.id.clone())];
                return out;
            }
        }
        let Some(held) = player.get_item_in_hand(Hand::Right) else { return out };
        let (block_id, furniture_id) = self.placed_by(&held).await;
        let target = if is_air(&name_at(&world, clicked)) { clicked } else { step(clicked, hit.face) };
        if block_id.is_some() || furniture_id.is_some() {
            let now = crate::now_ms();
            if self.last_placed.insert(key(player.get_id()), now).is_some_and(|before| now.saturating_sub(before) < 150) {
                out.cancel = true;
                return out;
            }
        }
        if let Some(id) = block_id {
            out.cancel = true;
            self.place_block(player, &world, &id, target, &held, &mut out).await;
        } else if let Some(id) = furniture_id {
            out.cancel = true;
            self.place_furniture(player, &world, &id, target, &held, name).await;
        }
        out
    }

    async fn take_one(player: &Player, held: &ItemStack) {
        if matches!(player.get_gamemode(), pumpkin_plugin_api::player::GameMode::Creative) {
            return;
        }
        let left = held.get_count().await.saturating_sub(1);
        if left == 0 {
            player.set_item_in_hand(Hand::Right, None).await;
        } else if let Some(now) = player.get_item_in_hand(Hand::Right) {
            now.set_count(left).await;
            player.set_item_in_hand(Hand::Right, Some(now)).await;
        }
    }

    async fn place_block(&self, player: &Player, world: &World, id: &str, target: (i32, i32, i32), held: &ItemStack, out: &mut Outcome) {
        let Some(one) = self.blocks.get(id) else { return };
        if !is_air(&name_at(world, target)) {
            return;
        }
        let below = name_at(world, (target.0, target.1 - 1, target.2));
        if one.shape == "plant" && is_air(&below) {
            return;
        }
        if block::growth(&one.data).is_some_and(|growth| !growth.soil_ok(&below)) {
            return;
        }
        world.set_block_state(pos(target), one.custom.unwrap_or(one.state), flags()).await;
        Self::take_one(player, held).await;
        let group = sound_group(&block::sounds(&one.data));
        world.play_custom_sound(
            &format!("minecraft:block.{group}.place"),
            SoundCategory::Blocks,
            (f64::from(target.0) + 0.5, f64::from(target.1) + 0.5, f64::from(target.2) + 0.5),
            1.0,
            0.8,
        );
        if let Some(list) = one.data.get("placeActions").filter(|list| list.as_array().is_some_and(|list| !list.is_empty())) {
            out.actions = Some(list.clone());
            out.extra = vec![("block".into(), id.to_string())];
        }
    }

    async fn place_furniture(
        &self,
        player: &Player,
        world: &World,
        id: &str,
        base: (i32, i32, i32),
        held: &ItemStack,
        name: &dyn Fn(&str) -> TextComponent,
    ) {
        let Some(data) = self.furniture.get(id) else { return };
        let yaw = furniture::snap_yaw(f64::from(player.get_yaw()), furniture::turns(data));
        let mut taken = Vec::new();
        for cell in furniture::cells(data) {
            let turned = furniture::turn_cell(cell, yaw);
            let spot = (base.0 + turned.x as i32, base.1 + turned.y as i32, base.2 + turned.z as i32);
            if !is_air(&name_at(world, spot)) {
                return;
            }
            taken.push(spot);
        }
        let center = (f64::from(base.0) + 0.5, f64::from(base.1), f64::from(base.2) + 0.5);
        let entity = world.spawn_entity(EntityType::ItemDisplay, (center.0, center.1 + furniture::lift(data), center.2)).await;
        entity.set_rotation(yaw as f32, 0.0);
        if let Some(display) = entity.as_item_display() {
            let shown = match self.own_item(&format!("furniture/{id}"), name).await {
                Some(shown) => shown,
                None => with_model(&format!("minecraft:{}", furniture::ITEM), &format!("furniture/{id}")).await,
            };
            display.set_item_stack(Some(shown)).await;
            display.set_item_display_mode(match furniture::display(data).as_str() {
                "none" => ItemDisplayMode::None,
                "head" => ItemDisplayMode::Head,
                "ground" => ItemDisplayMode::Ground,
                _ => ItemDisplayMode::Fixed,
            });
            let scale = furniture::scale(data) as f32;
            display.get_display().set_scale(scale, scale, scale);
        }
        let cells: Vec<String> = taken.iter().map(|(x, y, z)| format!("{x},{y},{z}")).collect();
        entity.set_string(TAG, "furniture", id).await;
        entity.set_string(TAG, "furniture_cells", &cells.join(";")).await;
        let hitbox = world.spawn_entity(EntityType::Interaction, center).await;
        if let Some(box_) = hitbox.as_interaction() {
            let (width, height) = furniture::hitbox(data);
            let grow = if furniture::solid(data) { 0.02 } else { 0.0 };
            box_.set_width(width as f32 + grow);
            box_.set_height(height as f32 + grow);
            box_.set_response(true);
        }
        hitbox.set_string(TAG, "furniture", id).await;
        hitbox.set_string(TAG, "furniture_display", &entity.get_uuid().to_string()).await;
        entity.set_string(TAG, "furniture_hitbox", &hitbox.get_uuid().to_string()).await;
        let light = furniture::light(data);
        let lit = (light > 0)
            .then(|| {
                pumpkin_plugin_api::world::resolve_block_state(
                    "minecraft:light",
                    &[("level".into(), light.to_string()), ("waterlogged".into(), "false".into())],
                )
            })
            .flatten();
        for spot in &taken {
            if furniture::solid(data) {
                world.set_block_by_name(pos(*spot), "minecraft:barrier".into(), flags()).await;
            } else if let Some(state) = lit {
                world.set_block_state(pos(*spot), state, flags()).await;
            }
        }
        Self::take_one(player, held).await;
        world.play_custom_sound("minecraft:block.wood.place", SoundCategory::Blocks, center, 1.0, 0.9);
    }

    /// A note block placed by a player goes to its plain state, so it never matches one of ours. (Ours are set
    /// straight into the world and never fire a place event.)
    pub async fn placed_plain(&self, world: &World, at: (i32, i32, i32), block_name: &str) {
        if self.blocks.is_empty() || !block_name.trim_start_matches("minecraft:").starts_with("note_block") {
            return;
        }
        if let Some(state) = self.plain_note {
            world.set_block_state(pos(at), state, BlockFlags::NOTIFY_LISTENERS).await;
        }
        Self::forget_note(world, at).await;
    }

    /// Whether to cancel a physics update, note or piston at `at` (ours never change).
    pub fn holds(&self, world: &World, at: (i32, i32, i32)) -> bool {
        self.at(world, at).is_some()
    }

    // ---------- plain note blocks ----------
    // A plain note block stays `harp, note 0` in the world (every other state may be one of ours), so its note is
    // kept in the chunk and we tune and play it ourselves, like CraftEngine's real blocks leave vanilla ones alone.
    // Its state is frozen too, so the game never retunes or powers it on a neighbour update: hitting it plays from
    // `dig_started`, and a neighbour update reads the redstone it gets (`powered_plain`), as the game would.

    /// A vanilla note block while studio blocks borrow note block states.
    pub fn plain_note_at(&self, world: &World, at: (i32, i32, i32)) -> bool {
        !self.blocks.is_empty() && self.at(world, at).is_none() && name_at(world, at).trim_start_matches("minecraft:") == "note_block"
    }

    fn note_key(at: (i32, i32, i32)) -> String {
        format!("note.{}.{}.{}", at.0, at.1, at.2)
    }

    async fn stored_note(world: &World, at: (i32, i32, i32)) -> u32 {
        let Some(chunk) = world.get_chunk(at.0.div_euclid(16), at.2.div_euclid(16)) else { return 0 };
        chunk.get_int("monolab", &Self::note_key(at)).await.map_or(0, |note| note.clamp(0, block::NOTES as i32 - 1) as u32)
    }

    async fn forget_note(world: &World, at: (i32, i32, i32)) {
        if let Some(chunk) = world.get_chunk(at.0.div_euclid(16), at.2.div_euclid(16)) {
            chunk.remove_custom_data("monolab", &Self::note_key(at));
            chunk.remove_custom_data("monolab", &Self::power_key(at));
        }
    }

    fn power_key(at: (i32, i32, i32)) -> String {
        format!("powered.{}.{}.{}", at.0, at.1, at.2)
    }

    /// A neighbour changed next to a plain note block: it plays when it starts getting power (the game's
    /// `powered` false → true), with the last power kept in the chunk like its note.
    pub async fn powered_plain(&self, world: &World, at: (i32, i32, i32)) {
        let Some(chunk) = world.get_chunk(at.0.div_euclid(16), at.2.div_euclid(16)) else { return };
        let now = world.is_block_indirectly_powered(pos(at));
        let was = chunk.get_int("monolab", &Self::power_key(at)).await == Some(1);
        if now == was {
            return;
        }
        if now {
            chunk.set_int("monolab", &Self::power_key(at), 1).await;
            Self::play_plain(world, at).await;
        } else {
            chunk.remove_custom_data("monolab", &Self::power_key(at));
        }
    }

    /// A right click on a plain note block: one note up, then play it (what the game would do).
    pub async fn tune_plain(&self, world: &World, at: (i32, i32, i32)) {
        let note = (Self::stored_note(world, at).await + 1) % block::NOTES;
        if let Some(chunk) = world.get_chunk(at.0.div_euclid(16), at.2.div_euclid(16)) {
            chunk.set_int("monolab", &Self::note_key(at), note as i32).await;
        }
        Self::play_plain(world, at).await;
    }

    /// Plays a plain note block's own note with the instrument under it (only with air above, like the game).
    pub async fn play_plain(world: &World, at: (i32, i32, i32)) {
        if !is_air(&name_at(world, (at.0, at.1 + 1, at.2))) {
            return;
        }
        let note = Self::stored_note(world, at).await;
        let instrument = block::instrument_under(&name_at(world, (at.0, at.1 - 1, at.2)));
        let center = (f64::from(at.0) + 0.5, f64::from(at.1) + 0.5, f64::from(at.2) + 0.5);
        world.play_custom_sound(
            &format!("minecraft:block.note_block.{instrument}"),
            SoundCategory::Records,
            center,
            3.0,
            block::note_pitch(note),
        );
        world.spawn_particle(Particle::Note, (center.0, center.1 + 0.7, center.2), (f64::from(note) / 24.0, 0.0, 0.0), 1.0, 0);
    }

    // ---------- breaking ----------

    /// A player started digging: on one of ours their own break speed is held at 0 and [`Self::tick_digging`]
    /// counts its time (Paper does the same). Creative players and blocks that break at once are left to the game.
    pub async fn dig_started(&mut self, player: &Player, at: (i32, i32, i32), item: Option<&Data>) {
        self.dig_stopped(player);
        let body = player.as_entity();
        let world = body.get_world();
        if self.plain_quiet && self.plain_note_at(&world, at) {
            Self::play_plain(&world, at).await;
        }
        let Some(one) = self.at(&world, at) else { return };
        if matches!(player.get_gamemode(), pumpkin_plugin_api::player::GameMode::Creative) || self.custom_at(&world, at) {
            return;
        }
        let held = match player.get_item_in_hand(Hand::Right) {
            Some(stack) => stack.get_registry_key().await,
            None => String::new(),
        };
        let needed = block::dig_with(&one.data, &one.id, &held, item).0;
        if needed == 0 {
            return;
        }
        if let Some(living) = body.as_living() {
            living.add_attribute_modifier(
                Attribute::BlockBreakSpeed,
                &AttributeModifier { id: DIG_MODIFIER.into(), amount: -1.0, operation: ModifierOperation::MultiplyTotal },
            );
        }
        self.digging.insert(key(player.get_id()), Digging { world, at, needed, done: 0 });
    }

    /// The player stopped digging (or broke it): their speed comes back and the cracks go.
    pub fn dig_stopped(&mut self, player: &Player) {
        if let Some(living) = player.as_entity().as_living() {
            living.remove_attribute_modifier(Attribute::BlockBreakSpeed, DIG_MODIFIER);
        }
        if let Some(was) = self.digging.remove(&key(player.get_id())) {
            self.cracks(&was.world, was.at, -1.0);
        }
    }

    /// Every server tick: one more tick of digging for each player at it, with the cracks drawn. Returns the
    /// blocks whose time is up, for the studio to break (actions, drops) like the player broke them.
    pub fn tick_digging(&mut self, server: &Server) -> Vec<(Player, World, (i32, i32, i32))> {
        if self.digging.is_empty() {
            self.viewers.clear();
            return Vec::new();
        }
        self.viewers = server.get_all_players();
        let mut done = Vec::new();
        let mut stop = Vec::new();
        let ours: Vec<(u64, u64)> = self.digging.keys().copied().collect();
        for who in ours {
            let Some(player) = server.get_player_by_uuid(Uuid { high: who.0, low: who.1 }) else {
                if let Some(was) = self.digging.remove(&who) {
                    self.cracks(&was.world, was.at, -1.0);
                }
                continue;
            };
            let still = self.digging.get(&who).is_some_and(|one| self.at(&one.world, one.at).is_some());
            if !still {
                stop.push(player);
                continue;
            }
            let Some(one) = self.digging.get_mut(&who) else { continue };
            one.done += 1;
            let (at, progress, finished) = (one.at, (one.done as f32 / one.needed as f32).min(1.0), one.done >= one.needed);
            if let Some(one) = self.digging.get(&who) {
                self.cracks(&one.world, at, progress);
            }
            if finished {
                done.push((player, at));
            }
        }
        for player in stop {
            self.dig_stopped(&player);
        }
        let mut out = Vec::new();
        for (player, at) in done {
            if let Some(was) = self.digging.get(&key(player.get_id())) {
                let world = player.as_entity().get_world();
                let same = world.get_id() == was.world.get_id();
                self.dig_stopped(&player);
                if same {
                    out.push((player, world, at));
                }
            }
        }
        out
    }

    /// Cracks at `at` for the players near it: `progress` 0..1, anything else clears them. One overlay per block
    /// (a negative id no entity has).
    fn cracks(&self, world: &World, at: (i32, i32, i32), progress: f32) {
        let (x, y, z) = (f64::from(at.0) + 0.5, f64::from(at.1) + 0.5, f64::from(at.2) + 0.5);
        let here = world.get_id();
        let source = crack_source(at);
        for player in &self.viewers {
            let (px, py, pz) = player.get_position();
            if player.get_world().get_id() == here
                && (px - x).powi(2) + (py - y).powi(2) + (pz - z).powi(2) <= CRACK_DISTANCE * CRACK_DISTANCE
            {
                player.send_block_damage(pos(at), progress, Some(source));
            }
        }
    }

    /// Air where one of ours was (after its break was handled).
    pub async fn clear(&self, world: &World, at: (i32, i32, i32)) {
        world.set_block_by_name(pos(at), "minecraft:air".into(), flags()).await;
    }

    /// The experience a block just marked as dropping ([`Self::breaking`]) gives, when it gives any.
    pub fn exp_if_dropping(&self, at: (i32, i32, i32)) -> Option<u32> {
        let (id, grown) = self.broken.get(&at)?;
        let one = self.blocks.get(id)?;
        let harvest = if *grown { block::growth(&one.data).map_or(0, |growth| growth.harvest_exp) } else { 0 };
        let exp = block::drops(&one.data).exp + harvest;
        (exp > 0).then_some(exp)
    }

    /// A block is breaking (before it goes): remembers ours for the drop event and returns its break actions.
    pub async fn breaking(&mut self, player: Option<&Player>, world: &World, at: (i32, i32, i32), item: Option<&Data>) -> Outcome {
        let mut out = Outcome::default();
        let Some(one) = self.at(world, at) else { return out };
        let id = one.id.clone();
        let group = sound_group(&block::sounds(&one.data));
        let mut keep = true;
        if let Some(player) = player {
            let creative = matches!(player.get_gamemode(), pumpkin_plugin_api::player::GameMode::Creative);
            let held = match player.get_item_in_hand(Hand::Right) {
                Some(stack) => stack.get_registry_key().await,
                None => String::new(),
            };
            keep = !creative && block::dig_with(&one.data, &id, &held, item).1;
            if let Some(list) = one.data.get("breakActions").filter(|list| list.as_array().is_some_and(|list| !list.is_empty())) {
                out.actions = Some(list.clone());
                out.extra = vec![("block".into(), id.clone())];
            }
        }
        if keep {
            let state = world.get_block_state_id(pos(at));
            let grown = block::growth(&one.data).is_some_and(|growth| one.stage(state) >= growth.last());
            self.broken.insert(at, (id, grown));
        }
        // A registered block makes its own break sound.
        if !self.custom_at(world, at) {
            world.play_custom_sound(
                &format!("minecraft:block.{group}.break"),
                SoundCategory::Blocks,
                (f64::from(at.0) + 0.5, f64::from(at.1) + 0.5, f64::from(at.2) + 0.5),
                1.0,
                0.8,
            );
        }
        out
    }

    /// The drop event after [`Self::breaking`]: our own item (or the linked studio item) and the loot table instead
    /// of the borrowed block's. `cancel` = clear the event's items first.
    pub async fn drops(&mut self, at: (i32, i32, i32), name: &dyn Fn(&str) -> TextComponent) -> Outcome {
        let mut out = Outcome::default();
        let Some((id, grown)) = self.broken.remove(&at) else { return out };
        let Some(one) = self.blocks.get(&id) else { return out };
        out.cancel = true;
        let drops = block::drops(&one.data);
        if drops.own {
            self.give_back(&format!("block/{id}"), &one.data, &mut out, name).await;
        }
        if !drops.loot.is_empty() {
            out.loot.push(drops.loot);
        }
        if grown && let Some(growth) = block::growth(&one.data).filter(|growth| !growth.harvest_loot.is_empty()) {
            out.loot.push(growth.harvest_loot);
        }
        out
    }

    /// A fork drops callback with no break before it (an explosion, a piston): it is ours to drop like a broken one.
    pub fn dropping_without_break(&mut self, at: (i32, i32, i32), state: u16) {
        if self.broken.contains_key(&at) {
            return;
        }
        let Some(one) = self.by_state.get(&state).and_then(|id| self.blocks.get(id)) else { return };
        let grown = block::growth(&one.data).is_some_and(|growth| one.stage(state) >= growth.last());
        self.broken.insert(at, (one.id.clone(), grown));
    }

    // ---------- crops ----------

    fn roll(&mut self) -> f64 {
        // xorshift64
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A registered crop at `state`: its file and its stage, when it may still grow.
    fn growing(&self, state: u16) -> Option<(&Loaded, block::Growth, u32)> {
        let one = self.by_state.get(&state).and_then(|id| self.blocks.get(id))?;
        one.custom?;
        let growth = block::growth(&one.data)?;
        let stage = one.stage(state);
        (stage < growth.last()).then_some((one, growth, stage))
    }

    /// Whether bone meal works on a crop now (the fork asks before it uses the meal up).
    pub fn can_bonemeal(&self, state: u16) -> bool {
        self.growing(state).is_some_and(|(_, growth, _)| growth.bonemeal)
    }

    /// The fork's random tick on a crop: its chance per tick, on the right soil (moist farmland when it wants water).
    pub async fn crop_tick(&mut self, world: &World, at: (i32, i32, i32), state: u16) {
        let Some((_, growth, _)) = self.growing(state) else { return };
        if self.roll() >= growth.chance(block::RANDOM_TICK_SECONDS) || !Self::soil_lets(world, at, &growth) {
            return;
        }
        self.grow(world, at, state, 1).await;
    }

    /// Bone meal on a crop: one or two stages.
    pub async fn bonemeal(&mut self, world: &World, at: (i32, i32, i32), state: u16) {
        if !self.can_bonemeal(state) {
            return;
        }
        let stages = block::bonemeal_stages(self.roll());
        self.grow(world, at, state, stages).await;
    }

    async fn grow(&self, world: &World, at: (i32, i32, i32), state: u16, by: u32) {
        let Some((one, growth, stage)) = self.growing(state) else { return };
        let Some(first) = one.custom else { return };
        let next = (stage + by).min(growth.last());
        world.set_block_state(pos(at), first + next as u16, flags()).await;
    }

    fn soil_lets(world: &World, at: (i32, i32, i32), growth: &block::Growth) -> bool {
        let below = (at.0, at.1 - 1, at.2);
        let below_state = world.get_block_state_id(pos(below));
        let Some((name, properties)) = pumpkin_plugin_api::custom_block::describe_state(below_state) else { return false };
        if !growth.soil_ok(&name) {
            return false;
        }
        !growth.water || properties.iter().any(|(key, value)| key == "moisture" && value.parse::<u8>().is_ok_and(|wet| wet > 0))
    }

    // ---------- furniture entities ----------

    /// Our furniture entity with this network id near the player: (display, hitbox, furniture id).
    async fn piece(player: &Player, entity_id: i32) -> Option<(Entity, Option<Entity>, String)> {
        let mut near = player.as_entity().get_nearby_entities(8.0, 8.0, 8.0);
        let at = near.iter().position(|one| one.get_id() as i32 == entity_id)?;
        let found = near.swap_remove(at);
        let id = found.get_string(TAG, "furniture").await?;
        let mut take = |uuid: Option<String>| {
            let uuid = uuid?;
            let at = near.iter().position(|one| one.get_uuid().to_string() == uuid)?;
            Some(near.swap_remove(at))
        };
        match found.get_string(TAG, "furniture_display").await {
            Some(display) => Some((take(Some(display))?, Some(found), id)),
            None => {
                let hitbox = take(found.get_string(TAG, "furniture_hitbox").await);
                Some((found, hitbox, id))
            }
        }
    }

    /// A right click on a piece: sit (when it has a seat) and its use actions.
    pub async fn use_piece(&self, player: &Player, entity_id: i32) -> Outcome {
        let mut out = Outcome::default();
        let Some((display, hitbox, id)) = Self::piece(player, entity_id).await else { return out };
        out.cancel = true;
        let Some(data) = self.furniture.get(&id) else { return out };
        let body = player.as_entity();
        if let (Some(height), None) = (furniture::seat(data), body.get_vehicle()) {
            let (x, y, z) = hitbox.as_ref().unwrap_or(&display).get_position();
            // Someone already sits here (Paper's `taken`): no second seat on top.
            let mut taken = false;
            for one in body.get_nearby_entities(8.0, 8.0, 8.0) {
                let (sx, sy, sz) = one.get_position();
                if !one.get_passengers().is_empty()
                    && (sx - x).powi(2) + (sy - y - height).powi(2) + (sz - z).powi(2) < 0.09
                    && one.get_string(TAG, "furniture_seat").await.is_some()
                {
                    taken = true;
                    break;
                }
            }
            if !taken {
                let world = body.get_world();
                let seat = world.spawn_entity(EntityType::ItemDisplay, (x, y + height, z)).await;
                seat.set_rotation(display.get_yaw() + 180.0, 0.0);
                seat.set_string(TAG, "furniture_seat", &id).await;
                seat.set_saves_to_disk(false);
                seat.add_passenger(body).await;
            }
        }
        if let Some(list) = data.get("useActions").filter(|list| list.as_array().is_some_and(|list| !list.is_empty())) {
            out.actions = Some(list.clone());
            out.extra = vec![("furniture".into(), id)];
        }
        out
    }

    /// A hit on a piece breaks it: its entities and cells go; its item (unless creative or `drops: false`) and break
    /// actions come back.
    pub async fn hit_piece(&self, player: &Player, entity_id: i32, name: &dyn Fn(&str) -> TextComponent) -> Outcome {
        let mut out = Outcome::default();
        let Some((display, hitbox, id)) = Self::piece(player, entity_id).await else { return out };
        out.cancel = true;
        let world = display.get_world();
        for cell in display.get_string(TAG, "furniture_cells").await.unwrap_or_default().split(';') {
            let parts: Vec<i32> = cell.split(',').filter_map(|one| one.parse().ok()).collect();
            if let [x, y, z] = parts[..] {
                let key = name_at(&world, (x, y, z));
                if key.ends_with("barrier") || key.ends_with(":light") {
                    world.set_block_by_name(pos((x, y, z)), "minecraft:air".into(), flags()).await;
                }
            }
        }
        let at = display.get_position();
        if let Some(hitbox) = hitbox {
            hitbox.remove();
        }
        display.remove();
        world.play_custom_sound("minecraft:block.wood.break", SoundCategory::Blocks, at, 1.0, 0.9);
        let Some(data) = self.furniture.get(&id) else { return out };
        if furniture::drops(data) && !matches!(player.get_gamemode(), pumpkin_plugin_api::player::GameMode::Creative) {
            self.give_back(&format!("furniture/{id}"), data, &mut out, name).await;
            out.drop_at = Some((world, at));
        }
        if let Some(list) = data.get("breakActions").filter(|list| list.as_array().is_some_and(|list| !list.is_empty())) {
            out.actions = Some(list.clone());
            out.extra = vec![("furniture".into(), id)];
        }
        out
    }

    /// An entity got off another (the dismount event). If it was one of our seats, the next tick takes it away.
    pub fn stood_up(&mut self, rider_id: i32, seat_id: i32) {
        if !self.furniture.is_empty() {
            self.stood.push((rider_id as u32, seat_id as u32));
        }
    }

    /// Every server tick: seats someone just stood up from go, and the rider is put 0.6 up (Paper's `onStand`), so
    /// they don't end up inside the chair. Not when they left by teleport (they're already far from the seat).
    pub async fn tick_seats(&mut self, server: &Server) {
        if self.stood.is_empty() {
            return;
        }
        let players = server.get_all_players();
        for (rider_id, seat_id) in std::mem::take(&mut self.stood) {
            let Some(player) = players.iter().find(|one| one.as_entity().get_id() == rider_id) else {
                // Not a player: just the seat, found in every world.
                for world in server.get_all_worlds() {
                    for seat in world.get_entities().into_iter().filter(|one| one.get_id() == seat_id) {
                        if seat.get_string(TAG, "furniture_seat").await.is_some() {
                            seat.remove();
                        }
                    }
                }
                continue;
            };
            let body = player.as_entity();
            let near = body.get_nearby_entities(3.0, 3.0, 3.0);
            let mut found = None;
            for one in near.into_iter().filter(|one| one.get_id() == seat_id) {
                if one.get_string(TAG, "furniture_seat").await.is_some() {
                    found = Some(one);
                    break;
                }
            }
            let Some(seat) = found else { continue };
            let (sx, sy, sz) = seat.get_position();
            seat.remove();
            if body.get_vehicle().is_none() {
                let (x, y, z) = body.get_position();
                if (x - sx).powi(2) + (y - sy).powi(2) + (z - sz).powi(2) < 4.0 {
                    // A plain move: `teleport_world` sends a world change (a loading screen, a fresh spawn spot).
                    player.teleport((x, y + 0.6, z), None, None, body.get_world()).await;
                }
            }
        }
    }

    /// Removes seats nobody sits on. Call every few seconds.
    pub async fn tick(&self, worlds: &[World]) {
        if self.furniture.is_empty() {
            return;
        }
        for world in worlds {
            for entity in world.get_entities() {
                if entity.get_passengers().is_empty() && entity.get_string(TAG, "furniture_seat").await.is_some() {
                    entity.remove();
                }
            }
        }
    }
}

/// A player's uuid as a map key.
fn key(id: Uuid) -> (u64, u64) {
    (id.high, id.low)
}
