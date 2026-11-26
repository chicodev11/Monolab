//! Structures on Pumpkin (`studio_format::structure`): the block lists the app made from `.schem`/`.nbt` files,
//! put down by the `structure` action a slice at a time (so a big build doesn't stall the server).

use super::Studio;
use super::play::Run;
use pumpkin_plugin_api::common::BlockPos;
use pumpkin_plugin_api::wit::pumpkin::plugin::world::BlockFlags;
use pumpkin_plugin_api::world::World;
use pumpkin_plugin_api::{Player, Server};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::Arc;
use std::time::SystemTime;
use studio_format::fields::{self, Data};
use studio_format::structure::{self, Structure};

/// Blocks put down per studio tick (every 10 server ticks).
const PER_TICK: usize = 8192;

struct Job {
    world: World,
    blocks: Vec<((i32, i32, i32), u16)>,
    at: usize,
}

#[derive(Default)]
pub struct Structures {
    /// By id: when its block list file was written, and what it holds.
    cache: HashMap<String, (SystemTime, Arc<Structure>)>,
    states: HashMap<String, Option<u16>>,
    queue: VecDeque<Job>,
}

impl Studio {
    /// A structure's block list, read again when the app has written a new one.
    pub(super) fn structure(&mut self, id: &str) -> Option<Arc<Structure>> {
        if !studio_format::valid_id(id) {
            return None;
        }
        let path = self.root.join(structure::data_path(id));
        let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok()?;
        if let Some((when, found)) = self.play.structures.cache.get(id)
            && *when == modified
        {
            return Some(Arc::clone(found));
        }
        let read = Arc::new(Structure::from_json(&fs::read_to_string(&path).ok()?).ok()?);
        self.play.structures.cache.insert(id.to_string(), (modified, Arc::clone(&read)));
        Some(read)
    }

    pub(super) fn state_of(&mut self, state: &str) -> Option<u16> {
        if let Some(found) = self.play.structures.states.get(state) {
            return *found;
        }
        let (name, props) = structure::split_state(state);
        let found = pumpkin_plugin_api::world::resolve_block_state(&name, &props).or_else(|| pumpkin_plugin_api::world::resolve_block_state(&name, &[]));
        self.play.structures.states.insert(state.to_string(), found);
        found
    }

    /// The `structure` action: at the player (default) or at a spot, turned as the action or the file says.
    pub(super) async fn place_structure_action(&mut self, server: &Server, player: Option<&Player>, one: &Data, _run: &Run) {
        let id = fields::text(one, "structure", "").trim().to_string();
        let Some(data) = self.data_of("structure", &id) else { return };
        let (world, at) = if fields::text(one, "at", "player") == "spot" {
            let Some(world) = super::world_named(server, &fields::text(one, "world", "world")) else { return };
            let at = (number(one, "x", 0.0) as i32, number(one, "y", 64.0) as i32, number(one, "z", 0.0) as i32);
            (world, at)
        } else {
            let Some(player) = player else { return };
            let (x, y, z) = player.get_position();
            (player.as_entity().get_world(), (x.floor() as i32, y.floor() as i32, z.floor() as i32))
        };
        let rotation = match fields::text(one, "rotation", "").trim() {
            "" => structure::rotation(&data),
            other => other.to_string(),
        };
        let roll = (self.next_random() % 4) as u8;
        self.place_structure(&id, &data, world, at, structure::quarter_of(&rotation, roll));
    }

    /// Queues a structure with its middle (or corner) at `at`.
    pub(super) fn place_structure(&mut self, id: &str, data: &Data, world: World, at: (i32, i32, i32), quarter: u8) {
        let Some(built) = self.structure(id) else { return };
        let corner = structure::corner(data, &built, quarter, at);
        let mut blocks = Vec::with_capacity(built.blocks.len());
        for (dx, dy, dz, state) in built.placements(quarter) {
            if let Some(found) = self.state_of(&state) {
                blocks.push(((corner.0 + dx, corner.1 + dy, corner.2 + dz), found));
            }
        }
        self.play.structures.queue.push_back(Job { world, blocks, at: 0 });
    }

    /// Puts down the next slice of queued structures.
    pub(super) async fn tick_structures(&mut self) {
        let mut left = PER_TICK;
        while left > 0 {
            let Some(job) = self.play.structures.queue.front_mut() else { return };
            let end = (job.at + left).min(job.blocks.len());
            for &((x, y, z), state) in &job.blocks[job.at..end] {
                job.world.set_block_state(BlockPos { x, y, z }, state, BlockFlags::NOTIFY_LISTENERS | BlockFlags::FORCE_STATE).await;
            }
            left -= end - job.at;
            job.at = end;
            if job.at >= job.blocks.len() {
                self.play.structures.queue.pop_front();
            }
        }
    }

    pub(super) fn structure_problems(&self) -> Vec<(String, String, String)> {
        self.active_rows("structure")
            .into_iter()
            .filter(|(id, _)| !self.root.join(structure::data_path(id)).is_file())
            .map(|(id, _)| ("structure".to_string(), id.to_string(), "Bring a .schem or .nbt file in from the app first (Import file).".to_string()))
            .collect()
    }
}

fn number(data: &Data, key: &str, fallback: f64) -> f64 {
    match data.get(key) {
        Some(serde_json::Value::Number(one)) => one.as_f64().unwrap_or(fallback),
        Some(serde_json::Value::String(text)) => text.trim().parse().unwrap_or(fallback),
        _ => fallback,
    }
}
