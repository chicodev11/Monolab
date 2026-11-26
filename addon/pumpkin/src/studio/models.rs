//! Studio maker M4 on Pumpkin: studio models as one item display per bone, stepping through the rig the app baked
//! (`<studio>/pack/models/<id>.json`, see `src-tauri/src/studio_pack_models.rs`). Same frames as Paper; each change
//! is smoothed over one tick by the display's own interpolation.
//!
//! Model entities aren't saved with their chunk (they're made again); leftovers from older add-ons are still
//! cleared once a second. M5: a mob's model (a [`Rider`]) rides the mob at its own spot, so it moves with it on
//! the client; a part that isn't riding (yet, or any more) is moved to the mob instead.
//!
//! `"visibility": "permission"`: the parts and hitbox are hidden by default (the fork's `set_visible_by_default`)
//! and shown once a second to the players with the permission, like Paper's `showByPermission`. A copy whose parts
//! are gone from the world (`is_valid`) is dropped and made again; a mob model whose mob is gone goes with it.

use pumpkin_plugin_api::persistent_data::PersistentDataHolder;
use pumpkin_plugin_api::world::{Entity, World};
use pumpkin_plugin_api::{DisplayTransformation, EntityDisplayExt, EntityType, ItemDisplayMode, ItemStack, Player, Quaternionf, Server, Vector3f};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use studio_format::fields::{self, Data};
use studio_format::model as rules;

/// Tag namespace and key on every entity a model spawns (value: the model id).
const TAG: &str = "monolab";
const KEY: &str = "model";

#[derive(Debug, Default)]
pub struct Animation {
    pub looping: String,
    pub frames: Vec<Vec<f32>>,
}

#[derive(Debug, Default)]
pub struct Rig {
    /// Item model per part (`monolab:model/<id>/p<n>`).
    pub items: Vec<String>,
    pub rest: Vec<f32>,
    pub animations: HashMap<String, Animation>,
    pub seat: Option<[f32; 3]>,
    pub problems: Vec<String>,
}

fn floats(value: Option<&Value>) -> Vec<f32> {
    value.and_then(Value::as_array).map(|list| list.iter().map(|one| one.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
}

impl Rig {
    pub fn parse(text: &str) -> Rig {
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else { return Rig::missing() };
        let items = root.get("parts").and_then(Value::as_array).into_iter().flatten().map(|one| one.get("item").and_then(Value::as_str).unwrap_or_default().to_string()).collect();
        let mut animations = HashMap::new();
        for (name, one) in root.get("animations").and_then(Value::as_object).into_iter().flatten() {
            let frames = one.get("frames").and_then(Value::as_array).into_iter().flatten().map(|frame| floats(Some(frame))).collect();
            animations.insert(name.clone(), Animation { looping: one.get("loop").and_then(Value::as_str).unwrap_or("once").to_string(), frames });
        }
        let seat = floats(root.get("seat"));
        Rig {
            items,
            rest: floats(root.get("rest")),
            animations,
            seat: (seat.len() == 3).then(|| [seat[0], seat[1], seat[2]]),
            problems: root.get("problems").and_then(Value::as_array).into_iter().flatten().filter_map(|one| one.as_str().map(str::to_string)).collect(),
        }
    }

    fn missing() -> Rig {
        Rig { problems: vec!["Save the model in the app so the studio pack is built.".into()], ..Rig::default() }
    }

    /// Frame index for `elapsed` ticks: `None` when a `once` animation is over.
    pub fn frame_at(animation: &Animation, elapsed: u64) -> Option<usize> {
        let count = animation.frames.len();
        if count == 0 {
            return None;
        }
        match animation.looping.as_str() {
            "loop" => Some((elapsed % (count.saturating_sub(1).max(1)) as u64) as usize),
            "hold" => Some((elapsed as usize).min(count - 1)),
            _ => ((elapsed as usize) < count).then_some(elapsed as usize),
        }
    }
}

/// A part's transformation in a frame (10 numbers each).
pub fn pose(frame: &[f32], part: usize) -> DisplayTransformation {
    let at = part * 10;
    let get = |offset: usize, fallback: f32| frame.get(at + offset).copied().unwrap_or(fallback);
    let whole = frame.len() >= at + 10;
    DisplayTransformation {
        translation: Vector3f { x: get(0, 0.0), y: get(1, 0.0), z: get(2, 0.0) },
        left_rotation: if whole { Quaternionf { x: get(3, 0.0), y: get(4, 0.0), z: get(5, 0.0), w: get(6, 1.0) } } else { Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } },
        scale: if whole { Vector3f { x: get(7, 1.0), y: get(8, 1.0), z: get(9, 1.0) } } else { Vector3f { x: 1.0, y: 1.0, z: 1.0 } },
        right_rotation: Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
    }
}

fn net_text(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut left = text.len() as u32;
    loop {
        if left & !0x7f == 0 {
            out.push(left as u8);
            break;
        }
        out.push((left & 0x7f | 0x80) as u8);
        left >>= 7;
    }
    out.extend_from_slice(text.as_bytes());
    out
}

/// One copy of a model in the world.
pub struct Instance {
    pub model: String,
    rig: Arc<Rig>,
    roles: Vec<(String, String)>,
    parts: Vec<Entity>,
    hitbox: Option<Entity>,
    seat: Option<Entity>,
    playing: Option<String>,
    started: u64,
    shown: Option<(Option<String>, Option<usize>)>,
    /// M5: a mob's model walks (`walk` role) instead of idling while the mob moves.
    walking: bool,
    /// `"visibility": "permission"`: the permission that shows it (empty: nobody).
    permission: Option<String>,
}

impl Instance {
    fn resolve(&self, name: &str) -> Option<String> {
        let by_role = self.roles.iter().find(|(role, _)| role == name).map(|(_, one)| one.clone());
        by_role.filter(|one| self.rig.animations.contains_key(one)).or_else(|| self.rig.animations.contains_key(name).then(|| name.to_string()))
    }

    fn idle(&self) -> Option<String> {
        self.resolve("idle").filter(|_| self.roles.iter().any(|(role, _)| role == "idle"))
    }

    /// What plays when nothing else does: `walk` while a mob moves (when the model has one), else `idle`.
    fn base(&self) -> Option<String> {
        let walk = self.walking.then(|| self.resolve("walk").filter(|_| self.roles.iter().any(|(role, _)| role == "walk"))).flatten();
        walk.or_else(|| self.idle())
    }

    /// M5: the mob started or stopped moving: idle and walk swap, unless a one-off animation is playing.
    fn set_walking(&mut self, walking: bool, now: u64) {
        if self.walking == walking {
            return;
        }
        let was = self.base();
        self.walking = walking;
        if self.playing == was {
            self.playing = self.base();
            self.started = now;
            self.shown = None;
        }
    }

    /// Ticks the named animation (or role) lasts, when the model has it.
    fn length(&self, name: &str) -> Option<u64> {
        self.resolve(name).and_then(|found| self.rig.animations.get(&found)).map(|one| one.frames.len() as u64)
    }

    pub fn play(&mut self, name: &str, now: u64) {
        if let Some(found) = self.resolve(name) {
            self.playing = Some(found);
            self.started = now;
            self.shown = None;
        }
    }

    fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.parts.iter().chain(&self.hitbox).chain(&self.seat)
    }

    fn ids(&self) -> Vec<u32> {
        self.entities().map(Entity::get_id).collect()
    }

    pub fn remove(&self) {
        self.entities().for_each(Entity::remove);
    }

    /// Still in the world (Paper's `alive`): its first part wasn't removed.
    fn alive(&self) -> bool {
        self.parts.first().is_some_and(Entity::is_valid)
    }

    /// Permission-only models: shown to the players who have the permission, hidden from the rest.
    async fn show_by_permission(&self, players: &[Player]) {
        let Some(permission) = &self.permission else { return };
        for player in players {
            let may = !permission.is_empty() && player.has_permission(permission.clone()).await;
            for one in self.parts.iter().chain(&self.hitbox) {
                // The calls take their own handle of the entity.
                let handle = || one.as_display().map(|display| display.get_entity()).or_else(|| one.as_interaction().map(|box_| box_.get_entity()));
                let Some(seen) = handle() else { continue };
                if player.can_see_entity(seen) != may
                    && let Some(again) = handle()
                {
                    if may {
                        player.show_entity(again);
                    } else {
                        player.hide_entity(again);
                    }
                }
            }
        }
    }

    fn step(&mut self, now: u64) {
        let mut frame = &self.rig.rest;
        let mut index = None;
        if let Some(animation) = self.playing.as_ref().and_then(|name| self.rig.animations.get(name)) {
            match Rig::frame_at(animation, now.saturating_sub(self.started)) {
                Some(at) => {
                    frame = &animation.frames[at];
                    index = Some(at);
                }
                None => {
                    self.playing = self.base();
                    self.started = now;
                    return;
                }
            }
        }
        let state = (self.playing.clone(), index);
        if self.shown.as_ref() == Some(&state) {
            return;
        }
        for (at, part) in self.parts.iter().enumerate() {
            if let Some(display) = part.as_display() {
                display.set_interpolation_start(0);
                display.set_interpolation_duration(1);
                display.set_transformation(pose(frame, at));
            }
        }
        self.shown = Some(state);
    }
}

struct Loaded {
    stamp: Option<(SystemTime, u64)>,
    rig: Arc<Rig>,
}

/// M5: a model on a studio mob (the mob itself is invisible). It follows the mob and plays its roles.
struct Rider {
    instance: Instance,
    /// The mob it rides (its own handle).
    mob: Entity,
    /// Where the mob was when the parts were last moved: x, y, z and yaw.
    last: Option<(f64, f64, f64, f32)>,
    /// The mob died: the parts stay where they are until this tick (the death animation), then go.
    gone_at: Option<u64>,
    /// An emote's model: goes at this tick, following its player until then.
    ends_at: Option<u64>,
}

impl Rider {
    async fn follow(&mut self, now: u64) {
        let (x, y, z) = self.mob.get_position();
        let yaw = self.mob.get_yaw();
        let (moved, turned, walking) = match self.last {
            Some((lx, ly, lz, lyaw)) => {
                let step = (x - lx).powi(2) + (z - lz).powi(2);
                (step + (y - ly).powi(2) > 0.000_01, (yaw - lyaw).abs() > 0.5, step > 0.000_4)
            }
            None => (true, true, false),
        };
        if moved || turned {
            for part in &self.instance.parts {
                if moved && part.get_vehicle().is_none() {
                    part.teleport((x, y, z), self.mob.get_world()).await;
                }
                if turned {
                    part.set_rotation(yaw, 0.0);
                }
            }
            self.last = Some((x, y, z, yaw));
        }
        self.instance.set_walking(walking, now);
    }
}

/// Every model copy on the server, and the rigs read so far.
#[derive(Default)]
pub struct Models {
    rigs: HashMap<String, Loaded>,
    /// Placed by their file (`place`), by model id.
    placed: HashMap<String, Instance>,
    ticks: u64,
    /// The files changed: placed copies are rebuilt.
    dirty: bool,
    reported: HashMap<String, String>,
    /// M5: models on studio mobs, by the mob's entity id.
    riders: HashMap<u32, Rider>,
}

impl Models {
    /// The files changed (the studio reloaded): placed models are spawned again on the next tick.
    pub fn reload(&mut self) {
        self.dirty = true;
        self.reported.clear();
    }

    /// The model's rig, reread when its file changed.
    pub fn rig(&mut self, root: &Path, id: &str) -> Arc<Rig> {
        let file = root.join("pack").join("models").join(format!("{id}.json"));
        let stamp = std::fs::metadata(&file).ok().and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        if let Some(one) = self.rigs.get(id).filter(|one| one.stamp == stamp) {
            return one.rig.clone();
        }
        let rig = Arc::new(match stamp {
            Some(_) => std::fs::read_to_string(&file).map(|text| Rig::parse(&text)).unwrap_or_else(|_| Rig::missing()),
            None => Rig::missing(),
        });
        self.rigs.insert(id.to_string(), Loaded { stamp, rig: rig.clone() });
        rig
    }

    /// Every studio tick (20 a second) with the enabled `model` files. Returns (id, problem) lines new since the
    /// last reload, for the file's status.
    pub async fn tick(&mut self, server: &Server, root: &Path, rows: &[(String, Data)], world_named: impl Fn(&str) -> Option<World>) -> Vec<(String, String)> {
        self.ticks += 1;
        let mut problems = Vec::new();
        if self.ticks % 20 == 1 || self.dirty {
            let fresh = std::mem::take(&mut self.dirty);
            if fresh {
                self.placed.drain().for_each(|(_, one)| one.remove());
            }
            // Copies whose parts left the world (killed, chunk gone): the rest of them go, and they're made again.
            self.placed.retain(|_, one| {
                let alive = one.alive();
                if !alive {
                    one.remove();
                }
                alive
            });
            // Mob models whose mob is gone without a death event: a dead mob still plays `death`, a removed one goes.
            let now = self.ticks;
            self.riders.retain(|_, one| {
                if one.gone_at.is_some() || one.mob.is_valid() {
                    return true;
                }
                if one.mob.as_living().is_some_and(|living| living.is_dead()) {
                    let length = one.instance.length("death").unwrap_or(0).min(20 * 10);
                    one.instance.play("death", now);
                    one.gone_at = Some(now + length.max(1));
                    return true;
                }
                one.instance.remove();
                false
            });
            // Leftovers: tagged entities no live copy owns (from before a restart, or a removed file).
            let owned: Vec<u32> = self.placed.values().chain(self.riders.values().map(|one| &one.instance)).flat_map(Instance::ids).collect();
            for world in server.get_all_worlds() {
                for entity in world.get_entities() {
                    if !owned.contains(&entity.get_id()) && entity.get_string(TAG, KEY).await.is_some() {
                        entity.remove();
                    }
                }
            }
            self.placed.retain(|id, _| rows.iter().any(|(one, _)| one == id));
            for (id, data) in rows {
                let rig = self.rig(root, id);
                let problem = &rig.problems;
                if !problem.is_empty() {
                    let line = problem.join(" ");
                    if self.reported.get(id) != Some(&line) {
                        self.reported.insert(id.clone(), line.clone());
                        problems.push((id.clone(), line));
                    }
                }
                if self.placed.contains_key(id) || rig.items.is_empty() {
                    continue;
                }
                let Some(Value::Object(place)) = data.get("place") else { continue };
                let at = |key: &str| place.get(key).and_then(Value::as_f64).filter(|one| one.is_finite());
                let (Some(x), Some(y), Some(z)) = (at("x"), at("y"), at("z")) else { continue };
                let name = fields::text(place, "world", "world");
                let Some(world) = world_named(&name) else {
                    problems.push((id.clone(), format!("There is no world named \"{name}\".")));
                    continue;
                };
                if world.get_chunk((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16)).is_none() {
                    continue;
                }
                let yaw = at("yaw").unwrap_or(0.0) as f32;
                let mut made = spawn(&world, id, rig, data, (x, y, z), yaw).await;
                made.playing = made.idle();
                made.started = self.ticks;
                self.placed.insert(id.clone(), made);
            }
            let players = server.get_all_players();
            for one in self.placed.values().chain(self.riders.values().map(|one| &one.instance)) {
                one.show_by_permission(&players).await;
            }
        }
        let now = self.ticks;
        self.placed.values_mut().for_each(|one| one.step(now));
        // M5: mob models follow their mob; a dead mob's model finishes its death animation, then goes.
        self.riders.retain(|_, one| {
            if one.gone_at.is_some_and(|at| now >= at) || one.ends_at.is_some_and(|at| now >= at) {
                one.instance.remove();
                return false;
            }
            true
        });
        for one in self.riders.values_mut() {
            if one.gone_at.is_none() {
                one.follow(now).await;
            }
            one.instance.step(now);
        }
        problems
    }

    /// Plays an animation (or role) on every placed copy of a model and every mob wearing it (the `animation` action).
    pub fn play_on(&mut self, model: &str, animation: &str) {
        let now = self.ticks;
        self.placed.values_mut().filter(|one| one.model == model).for_each(|one| one.play(animation, now));
        self.riders.values_mut().filter(|one| one.gone_at.is_none() && one.instance.model == model).for_each(|one| one.instance.play(animation, now));
    }

    // ---------- M5: models on mobs ----------

    /// Puts model `id` on a mob (`data` is the model's file). Says what's wrong when it can't.
    pub async fn attach(&mut self, root: &Path, mob: &Entity, id: &str, data: &Data) -> Result<(), String> {
        let Some(own) = mob.as_living().map(|living| living.as_entity()) else { return Err("Only a living mob can wear a model.".into()) };
        let rig = self.rig(root, id);
        if rig.items.is_empty() {
            let problem = rig.problems.join(" ");
            return Err(if problem.is_empty() { format!("The model \"{id}\" has no parts.") } else { problem });
        }
        self.detach(mob.get_id());
        let (x, y, z) = own.get_position();
        let yaw = own.get_yaw();
        let permission = permission_of(data);
        let parts = spawn_parts(&own.get_world(), id, &rig, (x, y, z), yaw, permission.is_some()).await;
        // Ride the mob where it stands (not on its seat), and stay there when it's gone (the death animation).
        for part in &parts {
            part.set_riding_offset(Some((0.0, 0.0, 0.0)));
            part.set_keeps_position_while_riding(true);
            if let Some(display) = part.as_display() {
                own.add_passenger(display.get_entity()).await;
            }
        }
        let mut instance = Instance { model: id.to_string(), rig, roles: rules::roles(data), parts, hitbox: None, seat: None, playing: None, started: self.ticks, shown: None, walking: false, permission };
        instance.playing = instance.base();
        self.riders.insert(mob.get_id(), Rider { instance, mob: own, last: Some((x, y, z, yaw)), gone_at: None, ends_at: None });
        Ok(())
    }

    /// The `emote` action: the model rides the player, plays the animation once and goes (a new one ends the last).
    pub async fn emote(&mut self, root: &Path, player: &Player, id: &str, data: &Data, animation: &str) -> Result<(), String> {
        let body = player.as_entity();
        self.attach(root, &body, id, data).await?;
        let now = self.ticks;
        let Some(one) = self.riders.get_mut(&body.get_id()) else { return Ok(()) };
        let Some(length) = one.instance.length(animation).filter(|length| *length > 0) else {
            self.detach(body.get_id());
            return Err(format!("The model {id} has no animation {animation}."));
        };
        one.instance.play(animation, now);
        one.ends_at = Some(now + length.min(20 * 60) + 1);
        Ok(())
    }

    /// Takes a mob's model off (the mob is gone, or its file changed).
    pub fn detach(&mut self, mob_id: u32) {
        if let Some(one) = self.riders.remove(&mob_id) {
            one.instance.remove();
        }
    }

    /// Every mob model off, for a reload.
    pub fn detach_all(&mut self) {
        self.riders.drain().for_each(|(_, one)| one.instance.remove());
    }

    /// Plays an animation (or role) on one mob's model, when it has one.
    pub fn play_on_rider(&mut self, mob_id: u32, animation: &str) {
        let now = self.ticks;
        if let Some(one) = self.riders.get_mut(&mob_id).filter(|one| one.gone_at.is_none()) {
            one.instance.play(animation, now);
        }
    }

    /// The mob died: its model plays `death` where it stands, then goes (at once when there's no death animation).
    pub fn rider_died(&mut self, mob_id: u32) {
        let now = self.ticks;
        if let Some(one) = self.riders.get_mut(&mob_id).filter(|one| one.gone_at.is_none()) {
            let length = one.instance.length("death").unwrap_or(0).min(20 * 10);
            one.instance.play("death", now);
            one.gone_at = Some(now + length.max(1));
        }
    }

    /// A player right-clicked an entity: if it's part of a model, seat them (when it has a seat) and hand back the
    /// model id for the caller to run its `clickActions`.
    pub async fn clicked(&mut self, entity_id: i32, player: &Player) -> Option<String> {
        let one = self.placed.values().find(|one| one.ids().contains(&(entity_id as u32)))?;
        if let Some(seat) = &one.seat
            && seat.is_valid()
            && seat.get_passengers().is_empty()
        {
            seat.add_passenger(player.as_entity()).await;
        }
        Some(one.model.clone())
    }

    pub fn is_model(&self, entity_id: i32) -> bool {
        self.placed.values().any(|one| one.ids().contains(&(entity_id as u32)))
    }

    /// When the add-on turns off.
    pub fn stop(&mut self) {
        self.placed.drain().for_each(|(_, one)| one.remove());
        self.detach_all();
    }
}

/// `"visibility": "permission"`: the permission that shows the model (empty: nobody).
fn permission_of(data: &Data) -> Option<String> {
    (fields::text(data, "visibility", "everyone") == "permission").then(|| fields::text(data, "permission", "").trim().to_string())
}

/// Spawns a model's displays (and hitbox and seat) at a spot.
pub async fn spawn(world: &World, id: &str, rig: Arc<Rig>, data: &Data, (x, y, z): (f64, f64, f64), yaw: f32) -> Instance {
    let permission = permission_of(data);
    let hidden = permission.is_some();
    let parts = spawn_parts(world, id, &rig, (x, y, z), yaw, hidden).await;
    let hitbox = match rules::hitbox(data) {
        None => None,
        Some((width, height)) => {
        let entity = world.spawn_entity(EntityType::Interaction, (x, y, z)).await;
        entity.set_saves_to_disk(false);
        entity.set_visible_by_default(!hidden);
        entity.set_string(TAG, KEY, id).await;
        if let Some(interaction) = entity.as_interaction() {
            interaction.set_width(width as f32);
            interaction.set_height(height as f32);
            interaction.set_response(true);
        }
        Some(entity)
        }
    };
    let seat = match rig.seat {
        None => None,
        Some([sx, sy, sz]) => {
        // The rig is in model space; turn the seat by the model's yaw like the displays turn.
        let turn = f64::from(-yaw).to_radians();
        let (sx, sy, sz) = (f64::from(sx), f64::from(sy), f64::from(sz));
        let (dx, dz) = (sx * turn.cos() + sz * turn.sin(), -sx * turn.sin() + sz * turn.cos());
        let entity = world.spawn_entity(EntityType::ItemDisplay, (x + dx, y + sy, z + dz)).await;
        entity.set_saves_to_disk(false);
        entity.set_string(TAG, KEY, id).await;
        Some(entity)
        }
    };
    Instance { model: id.to_string(), rig, roles: rules::roles(data), parts, hitbox, seat, playing: None, started: 0, shown: None, walking: false, permission }
}

/// One item display per part, at rest. `hidden`: nobody sees them until shown (permission-only models).
async fn spawn_parts(world: &World, id: &str, rig: &Rig, (x, y, z): (f64, f64, f64), yaw: f32, hidden: bool) -> Vec<Entity> {
    use pumpkin_plugin_api::wit::pumpkin::plugin::data_components::DataComponent;
    let mut parts = Vec::new();
    for (at, item) in rig.items.iter().enumerate() {
        let entity = world.spawn_entity(EntityType::ItemDisplay, (x, y, z)).await;
        entity.set_saves_to_disk(false);
        entity.set_visible_by_default(!hidden);
        entity.set_string(TAG, KEY, id).await;
        entity.set_rotation(yaw, 0.0);
        if let Some(display) = entity.as_item_display() {
            let stack = ItemStack::new("minecraft:paper", 1);
            stack.set_component(DataComponent::ItemModel, net_text(item)).await;
            display.set_item(Some(stack)).await;
            display.set_item_display_mode(ItemDisplayMode::None);
        }
        if let Some(display) = entity.as_display() {
            display.set_transformation(pose(&rig.rest, at));
            display.set_interpolation_duration(1);
            display.set_teleport_duration(1);
        }
        parts.push(entity);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_rig() {
        let rig = Rig::parse(r#"{"parts":[{"bone":"a","item":"monolab:model/x/p0"}],"rest":[0,1,0,0,0,0,1,1,1,1],
            "animations":{"spin":{"loop":"loop","frames":[[0],[1],[2],[0]]},"pop":{"loop":"once","frames":[[0],[1]]}},"seat":[0,2,0],"problems":[]}"#);
        assert_eq!(rig.items, vec!["monolab:model/x/p0".to_string()]);
        assert_eq!(rig.seat, Some([0.0, 2.0, 0.0]));
        let spin = &rig.animations["spin"];
        assert_eq!(Rig::frame_at(spin, 4), Some(1), "the last frame is the first again");
        assert_eq!(Rig::frame_at(&rig.animations["pop"], 2), None);
        assert_eq!(pose(&rig.rest, 0).translation.y, 1.0);
        assert_eq!(pose(&rig.rest, 3).scale.x, 1.0, "missing part = still");
        assert!(!Rig::parse("nope").problems.is_empty());
    }
}
