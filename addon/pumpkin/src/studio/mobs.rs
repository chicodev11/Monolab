//! Studio maker M5 on Pumpkin: studio mobs (the `mob` kind) and their skills. A studio mob is a vanilla mob with
//! our name, attributes, equipment, goals and model, tagged `monolab:mob` = its id so it's known again after a
//! restart or when its chunk loads again. Same files as the Paper add-on (`StudioMobs.java`); the skill actions
//! (`damage`, `heal`, `potion`, `launch`, `particle`, `projectile`, `summon`) also run in rules, commands and
//! items, on the run's player.
//!
//! Drops work like Paper: the death event's drop list is changed (`vanillaDrops`, `dropEquipment`, `loot`) and its
//! experience set (`exp`).
//!
//! What Pumpkin can't do yet (for the fork, see `docs/studio-maker.md` §4); the file's status says so when it's used:
//! - No target goals to add back, so wander and look can only be turned off together with attack.
//! - The player's `launch-projectile` does nothing on this API; projectiles are spawned and pushed instead.

use super::Studio;
use super::play::{Run, TAG, object};
use crate::names;
use pumpkin_plugin_api::common::BlockPos;
use pumpkin_plugin_api::particles::Particle;
use pumpkin_plugin_api::persistent_data::PersistentDataHolder;
use pumpkin_plugin_api::wit::pumpkin::plugin::status_effect::{StatusEffectInstance, StatusEffectType};
use pumpkin_plugin_api::world::{Entity, EquipmentSlot, World};
use pumpkin_plugin_api::{Attribute, DamageType, EntityType, ItemStack, Player, Server};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use studio_format::fields::{self, Data};
use studio_format::mob as rules;

/// Tag key on every studio mob (namespace [`TAG`]); the value is the mob's id.
const KEY: &str = "mob";
/// The most mobs one `summon` makes.
const MOST_SUMMON: f64 = 20.0;
/// The most experience a mob file gives (the same as Paper).
const MOST_EXP: f64 = 100_000.0;

/// The stats a mob file sets (besides `health`), and the attribute each one is.
const ATTRIBUTES: [(&str, Attribute); 7] = [
    ("damage", Attribute::AttackDamage),
    ("speed", Attribute::MovementSpeed),
    ("armor", Attribute::Armor),
    ("toughness", Attribute::ArmorToughness),
    ("knockbackResistance", Attribute::KnockbackResistance),
    ("followRange", Attribute::FollowRange),
    ("size", Attribute::Scale),
];

/// `equipment` keys and their slots.
const SLOTS: [(EquipmentSlot, &str); 6] = [
    (EquipmentSlot::Head, "head"),
    (EquipmentSlot::Chest, "chest"),
    (EquipmentSlot::Legs, "legs"),
    (EquipmentSlot::Feet, "feet"),
    (EquipmentSlot::MainHand, "hand"),
    (EquipmentSlot::OffHand, "offhand"),
];

/// One skill, copied out of the file so it can run while the studio changes.
#[derive(Clone)]
struct Skill {
    trigger: String,
    every: f64,
    cooldown: f64,
    chance: f64,
    targets: String,
    radius: f64,
    conditions: Value,
    actions: Value,
}

/// A mob file as the add-on uses it.
struct Def {
    data: Data,
    /// The file (and its model's file) as text, to tell what changed on a reload.
    text: String,
    /// The name without tags, for `<mob>`.
    plain: String,
    skills: Vec<Skill>,
    loot: String,
    exp: Option<f64>,
    vanilla_drops: bool,
    drop_equipment: bool,
    burn: bool,
    pickup: bool,
}

impl Def {
    fn read(id: &str, data: &Data, model_text: String) -> Def {
        let name = fields::text(data, "name", "");
        let plain = studio_format::mini::plain(&name).trim().to_string();
        let ai = object(data, "ai");
        let skills = rules::skills(data)
            .into_iter()
            .map(|one| Skill {
                trigger: one.trigger,
                every: one.every,
                cooldown: one.cooldown,
                chance: one.chance,
                targets: one.targets,
                radius: one.radius,
                conditions: one.conditions,
                actions: one.actions,
            })
            .collect();
        Def {
            data: data.clone(),
            text: format!("{}{model_text}", Value::Object(data.clone())),
            plain: if plain.is_empty() { id.to_string() } else { plain },
            skills,
            loot: fields::text(data, "loot", "").trim().to_string(),
            exp: data.get("exp").and_then(Value::as_f64).filter(|one| one.is_finite()),
            vanilla_drops: flag(data, "vanillaDrops", false),
            drop_equipment: flag(data, "dropEquipment", false),
            burn: flag(ai, "burn", false),
            pickup: flag(ai, "pickup", false),
        }
    }

    /// What this file asks that Pumpkin can't do yet, for its status.
    fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let ai = object(&self.data, "ai");
        if flag(ai, "attack", true) && (!flag(ai, "wander", true) || !flag(ai, "look", true)) {
            out.push("Pumpkin can't turn off wandering or looking around while the mob still attacks yet.".to_string());
        }
        out
    }
}

/// A studio mob in the world.
struct Alive {
    id: String,
    entity: Entity,
    /// Who hurt it last (entity id): the killer for `death` skills, loot and exp.
    hit_by: Option<u32>,
    /// A damage event came and no attacker followed yet: `damaged` runs with nobody on the next tick.
    hurt: bool,
    /// Per skill (its place in the file): the next `timer` tick, and the tick its cooldown ends.
    due: HashMap<usize, u64>,
    ready: HashMap<usize, u64>,
}

impl Alive {
    fn new(id: &str, entity: Entity) -> Self {
        Self { id: id.to_string(), entity, hit_by: None, hurt: false, due: HashMap::new(), ready: HashMap::new() }
    }
}

/// Who a skill's actions run on.
enum Target {
    Player(Player),
    /// Not a player: entity id and name.
    Entity(u32, String),
}

#[derive(Default)]
pub struct Mobs {
    defs: HashMap<String, Def>,
    alive: HashMap<u32, Alive>,
    /// Per mob id: seconds counted toward its next natural spawn try.
    clocks: HashMap<String, u64>,
    /// Who hit any entity last (entity id → attacker id), for the killer of loot tables that drop from mobs.
    last_hit: HashMap<u32, u32>,
}

impl Studio {
    // ---------- files ----------

    /// The files changed: read the mob files again, report what Pumpkin can't do, and dress the living mobs
    /// whose file (or model) changed. Mobs whose file is gone or off are removed, like NPCs.
    pub(super) async fn rebuild_mobs(&mut self, server: &Server) {
        let rows: Vec<(String, Data)> = self.active_rows("mob").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        let mut defs = HashMap::new();
        let mut lines = Vec::new();
        for (id, data) in rows {
            let model = rules::model(&data).trim().to_string();
            let model_text = if model.is_empty() {
                String::new()
            } else {
                self.data_of("model", &model).map(|one| Value::Object(one).to_string()).unwrap_or_default()
            };
            let def = Def::read(&id, &data, model_text);
            let mut problems = def.problems();
            let base = clean(&rules::base(&data));
            if mob_type(&base).is_none() {
                problems.insert(0, format!("\"{base}\" isn't a mob Pumpkin knows."));
            }
            if !model.is_empty() && self.data_of("model", &model).is_none() {
                problems.insert(0, format!("There is no model named \"{model}\"."));
            }
            if !problems.is_empty() {
                lines.push((id.clone(), problems.join(" ")));
            }
            defs.insert(id, def);
        }
        let old = std::mem::replace(&mut self.play.mobs.defs, defs);
        for (id, line) in lines {
            self.problem("mob", &id, &line);
        }
        let changed: HashSet<String> = self
            .play
            .mobs
            .defs
            .iter()
            .filter(|(id, def)| old.get(*id).is_none_or(|was| was.text != def.text))
            .map(|(id, _)| id.clone())
            .collect();
        let living: Vec<u32> = self.play.mobs.alive.keys().copied().collect();
        for eid in living {
            let Some(alive) = self.play.mobs.alive.remove(&eid) else { continue };
            let Some(data) = self.play.mobs.defs.get(&alive.id).map(|def| def.data.clone()) else {
                self.play.models.detach(eid);
                alive.entity.remove();
                continue;
            };
            if changed.contains(&alive.id) {
                let id = alive.id.clone();
                self.dress(server, &alive.entity, &id, &data, false).await;
            }
            self.play.mobs.alive.insert(eid, alive);
        }
    }

    /// When the add-on stops: the mobs stay in the world (they're saved with it and taken over again); their models
    /// go with `Models::stop`.
    pub(super) fn stop_mobs(&mut self) {
        self.play.mobs.alive.clear();
    }

    // ---------- spawning ----------

    /// Spawns studio mob `id` at a spot (only where the chunk is loaded) and runs its `spawn` skills.
    /// Returns its entity id.
    pub(super) async fn spawn_mob(&mut self, server: &Server, id: &str, world: &World, (x, y, z): (f64, f64, f64)) -> Option<u32> {
        let data = self.play.mobs.defs.get(id)?.data.clone();
        let base = clean(&rules::base(&data));
        let Some(kind) = mob_type(&base) else {
            self.problem("mob", id, &format!("\"{base}\" isn't a mob Pumpkin knows."));
            return None;
        };
        world.get_chunk((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16))?;
        let entity = world.spawn_entity(kind, (x, y, z)).await;
        if !entity.is_living() {
            entity.remove();
            self.problem("mob", id, &format!("\"{base}\" isn't a living mob."));
            return None;
        }
        entity.set_string(TAG, KEY, id).await;
        let eid = entity.get_id();
        self.dress(server, &entity, id, &data, true).await;
        self.play.mobs.alive.insert(eid, Alive::new(id, entity));
        self.play.models.play_on_rider(eid, "spawn");
        self.fire_skills(server, eid, "spawn", None).await;
        Some(eid)
    }

    /// Puts a file on a mob: name, stats, equipment, goals and model. `fresh` = just spawned (full health).
    async fn dress(&mut self, server: &Server, entity: &Entity, id: &str, data: &Data, fresh: bool) {
        let name = fields::text(data, "name", "");
        if !name.trim().is_empty() {
            entity.set_custom_name(super::render(&name, &super::values(server, None)));
            entity.set_custom_name_visible(flag(data, "showName", true));
        }
        entity.set_silent(flag(data, "silent", false));
        entity.set_glowing(flag(data, "glowing", false));
        if let Some(living) = entity.as_living() {
            if let Some(health) = rules::stat(data, "health") {
                living.set_attribute_base(Attribute::MaxHealth, health);
                living.set_max_health(health as f32);
                if fresh || living.get_health() > health as f32 {
                    living.set_health(health as f32);
                }
            }
            for (key, attribute) in ATTRIBUTES {
                if let Some(value) = rules::stat(data, key) {
                    living.set_attribute_base(attribute, value);
                }
            }
            let equipment = object(data, "equipment").clone();
            for (slot, key) in SLOTS {
                let item = fields::text(&equipment, key, "").trim().to_string();
                if item.is_empty() {
                    continue;
                }
                match self.stack(server, &item, 1, &Run::default()).await {
                    Some(stack) => living.set_equipment(slot, Some(stack)).await,
                    None => self.problem("mob", id, &format!("\"equipment.{key}\": there is no item \"{item}\".")),
                }
            }
        }
        if let Some(mob) = entity.as_mob() {
            mob.set_persistence_required(flag(data, "persistent", false));
            mob.set_can_pick_up_loot(flag(object(data, "ai"), "pickup", false));
        }
        // Goals off are taken out one by one, the rest stay (Paper's `MobGoals`): attack off takes the target goals,
        // wander and look their own.
        let ai = object(data, "ai");
        let (attack, wander, look) = (flag(ai, "attack", true), flag(ai, "wander", true), flag(ai, "look", true));
        if !(attack && wander && look)
            && let Some(mob) = entity.as_mob()
        {
            let mut gone = HashSet::new();
            for goal in mob.get_ai_goals() {
                let name = goal.key.rsplit(':').next().unwrap_or_default().to_string();
                if (!attack && goal.target) || (!wander && wanders(&name)) || (!look && name.contains("look")) {
                    gone.insert(goal.key);
                }
            }
            for key in gone {
                mob.remove_ai_goal(key).await;
            }
        }
        let model = rules::model(data).trim().to_string();
        if model.is_empty() {
            self.play.models.detach(entity.get_id());
            if !fresh {
                entity.set_invisible(false);
            }
            return;
        }
        let Some(model_data) = self.data_of("model", &model) else {
            self.problem("mob", id, &format!("There is no model named \"{model}\"."));
            return;
        };
        let root = self.root.clone();
        match self.play.models.attach(&root, entity, &model, &model_data).await {
            Ok(()) => entity.set_invisible(true),
            Err(text) => {
                self.problem("mob", id, &text);
                entity.set_invisible(false);
            }
        }
    }

    // ---------- every tick ----------

    /// Every studio tick (10 server ticks): `damaged` without an attacker, `timer` skills; once a second the
    /// world's tagged mobs, sunburn and natural spawns.
    pub(super) async fn tick_mobs(&mut self, server: &Server) {
        if self.play.mobs.defs.is_empty() && self.play.mobs.alive.is_empty() {
            return;
        }
        let now = self.ticks;
        let mut hurt = Vec::new();
        let mut timed = Vec::new();
        {
            let mobs = &mut self.play.mobs;
            for (eid, alive) in &mut mobs.alive {
                if std::mem::take(&mut alive.hurt) {
                    hurt.push(*eid);
                }
                let Some(def) = mobs.defs.get(&alive.id) else { continue };
                for (index, skill) in def.skills.iter().enumerate().filter(|(_, one)| one.trigger == "timer") {
                    let every = (skill.every.max(1.0) * 20.0).round() as u64;
                    let due = alive.due.entry(index).or_insert(now + every);
                    if now >= *due {
                        *due = now + every;
                        timed.push((*eid, index, skill.clone()));
                    }
                }
            }
        }
        for eid in hurt {
            self.fire_skills(server, eid, "damaged", None).await;
        }
        for (eid, index, skill) in timed {
            self.run_skill(server, eid, index, &skill, None).await;
        }
        if now.is_multiple_of(20) {
            self.second_mobs(server).await;
        }
    }

    async fn second_mobs(&mut self, server: &Server) {
        // Match the world's tagged mobs: new ones (after a restart, or a chunk loaded again) are taken over, gone
        // ones (unloaded, despawned) forgotten.
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for world in server.get_all_worlds() {
            for entity in world.get_entities() {
                let Some(id) = entity.get_string(TAG, KEY).await else { continue };
                let eid = entity.get_id();
                seen.insert(eid);
                if !self.play.mobs.alive.contains_key(&eid) {
                    found.push((id, entity));
                }
            }
        }
        let gone: Vec<u32> = self.play.mobs.alive.keys().filter(|eid| !seen.contains(*eid)).copied().collect();
        for eid in gone {
            self.play.mobs.alive.remove(&eid);
            self.play.models.detach(eid);
        }
        for (id, entity) in found {
            if entity.is_dead() || entity.as_living().is_none_or(|living| living.is_dead()) {
                continue;
            }
            let Some(data) = self.play.mobs.defs.get(&id).map(|def| def.data.clone()) else {
                entity.remove();
                continue;
            };
            // Name, stats and equipment were saved with it; goals and the model come back.
            self.dress(server, &entity, &id, &data, false).await;
            self.play.mobs.alive.insert(entity.get_id(), Alive::new(&id, entity));
        }
        self.sunburn();
        self.spawn_naturally(server).await;
    }

    /// `"burn": true`: the mob catches fire in daylight under the open sky, like the undead.
    fn sunburn(&self) {
        let mobs = &self.play.mobs;
        for alive in mobs.alive.values() {
            if !mobs.defs.get(&alive.id).is_some_and(|def| def.burn) {
                continue;
            }
            let entity = &alive.entity;
            if entity.get_fire_ticks() > 20 || entity.is_in_water() {
                continue;
            }
            let world = entity.get_world();
            if world.get_time_of_day() % 24_000 >= 12_000 || world.is_raining() {
                continue;
            }
            let (x, y, z) = entity.get_position();
            let head = BlockPos { x: x.floor() as i32, y: (y + f64::from(entity.get_eye_height())).floor() as i32, z: z.floor() as i32 };
            if world.get_sky_light(head) >= 15 {
                entity.set_fire_ticks(160);
            }
        }
    }

    /// Natural spawns: every `every` seconds, for each online player in a listed world, `chance`% to try one spot
    /// 16..`radius` blocks away on the surface.
    async fn spawn_naturally(&mut self, server: &Server) {
        let rows: Vec<(String, rules::Spawn)> =
            self.play.mobs.defs.iter().map(|(id, def)| (id.clone(), rules::spawn(&def.data))).filter(|(_, spawn)| spawn.enabled).collect();
        if rows.is_empty() {
            return;
        }
        let players = crate::players_of(server);
        for (id, spawn) in rows {
            let every = spawn.every.max(1.0).round() as u64;
            let clock = self.play.mobs.clocks.entry(id.clone()).or_insert(0);
            *clock += 1;
            if *clock < every {
                continue;
            }
            *clock = 0;
            for (player, who) in &players {
                let count = self.play.mobs.alive.values().filter(|one| one.id == id).count() as i64;
                if count >= spawn.max {
                    break;
                }
                if !spawn.worlds.is_empty() && !spawn.worlds.iter().any(|one| one.trim() == who.world) {
                    continue;
                }
                if self.fraction() * 100.0 >= spawn.chance {
                    continue;
                }
                let radius = spawn.radius.clamp(17.0, 128.0);
                let angle = self.fraction() * std::f64::consts::TAU;
                let distance = 16.0 + self.fraction() * (radius - 16.0);
                let (px, _, pz) = player.get_position();
                let (x, z) = ((px + angle.cos() * distance).floor() as i32, (pz + angle.sin() * distance).floor() as i32);
                let world = player.get_world();
                if world.get_chunk(x.div_euclid(16), z.div_euclid(16)).is_none() {
                    continue;
                }
                let y = world.get_top_block_y(x, z) + 1;
                let at = BlockPos { x, y, z };
                let light = light_at(&world, at);
                if light < spawn.light_min || light > spawn.light_max {
                    continue;
                }
                if !spawn.biomes.is_empty() && !biome_listed(&spawn.biomes, &crate::map::biome_name(&format!("{:?}", world.get_biome(at))))
                {
                    continue;
                }
                self.spawn_mob(server, &id, &world, (f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5)).await;
            }
        }
    }

    /// A random number in 0..1.
    fn fraction(&mut self) -> f64 {
        (self.next_random() % 1_000_000) as f64 / 1_000_000.0
    }

    // ---------- events (called from lib.rs) ----------

    /// A living entity is about to be hurt (`EntityDamageEvent`). A studio mob plays `hurt`; its `damaged` skills
    /// run on the next tick unless an attacker follows (`mob_hit`).
    pub fn mob_hurt(&mut self, entity_id: i32) {
        let eid = entity_id as u32;
        let ours = match self.play.mobs.alive.get_mut(&eid) {
            Some(alive) => {
                alive.hurt = true;
                true
            }
            None => false,
        };
        if ours {
            self.play.models.play_on_rider(eid, "hurt");
        }
    }

    /// An entity hurts another (`EntityDamageByEntityEvent`): `damaged` skills of a studio mob that was hit and
    /// `attack` skills of one that hit.
    pub async fn mob_hit(&mut self, server: &Server, entity_id: i32, damager_id: i32) {
        let (victim, damager) = (entity_id as u32, damager_id as u32);
        if self.play.mobs.last_hit.len() > 512 {
            self.play.mobs.last_hit.clear();
        }
        self.play.mobs.last_hit.insert(victim, damager);
        let hit = match self.play.mobs.alive.get_mut(&victim) {
            Some(alive) => {
                alive.hurt = false;
                alive.hit_by = Some(damager);
                true
            }
            None => false,
        };
        if hit {
            self.fire_skills(server, victim, "damaged", Some(damager)).await;
        }
        if self.play.mobs.alive.contains_key(&damager) {
            self.play.models.play_on_rider(damager, "attack");
            self.fire_skills(server, damager, "attack", Some(victim)).await;
        }
    }

    /// A living entity died (`EntityDeathEvent`, before its drops): a studio mob runs its `death` skills, its
    /// model plays `death`, and its drops and experience are changed like on Paper: without `vanillaDrops` the
    /// vanilla ones go, `dropEquipment` adds the file's equipment, `loot` adds a roll, `exp` sets the experience.
    /// Before the drops: the player who killed it (last hit) counts it for quests, as `studio:<id>` for our mobs
    /// and the vanilla name otherwise (a studio mob counts for both).
    pub async fn quest_kill(&mut self, server: &Server, entity_id: i32) {
        let eid = entity_id as u32;
        let Some(killer) = self.play.mobs.alive.get(&eid).and_then(|one| one.hit_by).or_else(|| self.play.mobs.last_hit.get(&eid).copied())
        else {
            return;
        };
        let Some(player) = server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == killer) else { return };
        if let Some(id) = self.play.mobs.alive.get(&eid).map(|one| one.id.clone()) {
            self.quest_event(server, &player, "kill", &format!("studio:{id}"), 1).await;
        }
        if let Some(entity) = self.entity_by_id(server, eid) {
            let kind = clean(&crate::map::biome_name(&format!("{:?}", entity.get_type())));
            self.quest_event(server, &player, "kill", &kind, 1).await;
        }
    }

    /// What a rule calls an entity: `studio:<id>` for our mobs, else its type (`zombie`).
    pub fn entity_name(&self, server: &Server, entity_id: i32) -> String {
        let eid = entity_id as u32;
        if let Some(one) = self.play.mobs.alive.get(&eid) {
            return format!("studio:{}", one.id);
        }
        if server.get_all_players().iter().any(|one| one.as_entity().get_id() == eid) {
            return "player".into();
        }
        self.entity_by_id(server, eid).map(|entity| clean(&crate::map::biome_name(&format!("{:?}", entity.get_type())))).unwrap_or_default()
    }

    /// The player who last hit this entity, for the `kill` rule event.
    pub fn killer_of(&self, server: &Server, entity_id: i32) -> Option<Player> {
        let eid = entity_id as u32;
        let killer = self.play.mobs.alive.get(&eid).and_then(|one| one.hit_by).or_else(|| self.play.mobs.last_hit.get(&eid).copied())?;
        server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == killer)
    }

    pub async fn mob_died(&mut self, server: &Server, entity_id: i32, drops: &mut Vec<ItemStack>, dropped_exp: &mut i32) {
        let eid = entity_id as u32;
        let Some((id, killer)) = self.play.mobs.alive.get(&eid).map(|one| (one.id.clone(), one.hit_by)) else { return };
        self.fire_skills(server, eid, "death", killer).await;
        self.play.models.rider_died(eid);
        let Some((data, plain, loot, exp, vanilla, drop_equipment)) = self
            .play
            .mobs
            .defs
            .get(&id)
            .map(|def| (def.data.clone(), def.plain.clone(), def.loot.clone(), def.exp, def.vanilla_drops, def.drop_equipment))
        else {
            self.play.mobs.alive.remove(&eid);
            return;
        };
        let player = killer.and_then(|killer| server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == killer));
        let extra = vec![("mob".to_string(), plain), ("mob_id".to_string(), id.clone())];
        let run = match &player {
            Some(player) => Run::of(player, extra),
            None => Run::server(extra),
        };
        if !vanilla {
            drops.clear();
        }
        if drop_equipment {
            let equipment = object(&data, "equipment").clone();
            for (_, key) in SLOTS {
                let item = fields::text(&equipment, key, "").trim().to_string();
                if !item.is_empty() {
                    drops.extend(self.stack(server, &item, 1, &run).await);
                }
            }
        }
        if !loot.is_empty()
            && let Some(table) = self.data_of("loot", &loot)
        {
            let rolled = self.roll(server, &table, &run).await;
            drops.extend(rolled);
        }
        if let Some(points) = exp {
            *dropped_exp = points.round().clamp(0.0, MOST_EXP) as i32;
        }
        self.play.mobs.alive.remove(&eid);
    }

    /// Any mob died (before [`Self::mob_died`]): the loot tables that drop from it add to (or replace) its drops,
    /// like Paper. A studio mob without `vanillaDrops` drops only what its file says.
    pub async fn mob_loot(&mut self, server: &Server, entity_id: i32, drops: &mut Vec<ItemStack>) {
        let eid = entity_id as u32;
        let killer = self.play.mobs.last_hit.remove(&eid);
        if self.play.mobs.alive.get(&eid).and_then(|alive| self.play.mobs.defs.get(&alive.id)).is_some_and(|def| !def.vanilla_drops) {
            return;
        }
        let tables: Vec<Data> = self
            .active_rows("loot")
            .into_iter()
            .map(|(_, data)| data.clone())
            .filter(|data| fields::text(object(data, "drops"), "from", "none") == "mob")
            .collect();
        if tables.is_empty() || server.get_all_players().iter().any(|one| one.as_entity().get_id() == eid) {
            return;
        }
        let Some(entity) = self.entity_by_id(server, eid) else { return };
        let kind = clean(&crate::map::biome_name(&format!("{:?}", entity.get_type())));
        let world = entity.get_world();
        let world = crate::world_name(&world.get_id(), &world.get_dimension());
        let player = killer.and_then(|killer| server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == killer));
        let run = player
            .as_ref()
            .map(|one| Run::of(one, vec![("mob".into(), kind.clone())]))
            .unwrap_or_else(|| Run::server(vec![("mob".into(), kind.clone())]));
        for data in tables {
            let from = object(&data, "drops");
            let targets: Vec<String> =
                fields::lines(from, "targets").into_iter().filter(|one| !one.trim().is_empty()).map(|one| clean(&one)).collect();
            if !targets.is_empty() && !targets.iter().any(|one| *one == kind || one == "*") {
                continue;
            }
            if from.get("needsPlayer").and_then(Value::as_bool) == Some(true) && player.is_none() {
                continue;
            }
            let shown = match &player {
                Some(player) => super::shows(&data, player, &world),
                // With no player to ask, only the world part of `show` counts; a permission means "players only".
                None => {
                    let show = fields::show(&data);
                    show.permission.is_empty() && (show.worlds.is_empty() || show.worlds.contains(&world))
                }
            };
            if !shown {
                continue;
            }
            if from.get("replace").and_then(Value::as_bool) == Some(true) {
                drops.clear();
            }
            let rolled = self.roll(server, &data, &run).await;
            drops.extend(rolled);
        }
    }

    /// A mob picked a new target (`EntityTargetEvent`): a studio mob's `target` skills.
    pub async fn mob_targeted(&mut self, server: &Server, entity_id: i32, target_id: Option<i32>) {
        let eid = entity_id as u32;
        if let Some(target) = target_id
            && self.play.mobs.alive.contains_key(&eid)
        {
            self.fire_skills(server, eid, "target", Some(target as u32)).await;
        }
    }

    /// A player right-clicked an entity (after NPCs and models): a studio mob's `interact` skills. True (cancel
    /// the vanilla click) when it has any.
    pub async fn mob_clicked(&mut self, server: &Server, player: &Player, entity_id: i32) -> bool {
        let eid = entity_id as u32;
        let Some(id) = self.play.mobs.alive.get(&eid).map(|one| one.id.clone()) else { return false };
        let has = self.play.mobs.defs.get(&id).is_some_and(|def| def.skills.iter().any(|one| one.trigger == "interact"));
        self.play.models.play_on_rider(eid, "interact");
        self.fire_skills(server, eid, "interact", Some(player.as_entity().get_id())).await;
        has
    }

    /// An entity is about to catch fire (`EntityCombustEvent`): true (cancel it) when a studio mob with
    /// `"burn": false` would burn in daylight (`cause` is `sunlight`). Fire, lava and burning arrows still burn it.
    pub fn mob_fireproof(&self, entity_id: i32, cause: &str) -> bool {
        let mobs = &self.play.mobs;
        cause == "sunlight" && mobs.alive.get(&(entity_id as u32)).and_then(|alive| mobs.defs.get(&alive.id)).is_some_and(|def| !def.burn)
    }

    /// An entity is about to pick up an item (`EntityPickupItemEvent`): true (cancel it) for a studio mob with
    /// `"pickup": false`.
    pub fn mob_no_pickup(&self, entity_id: i32) -> bool {
        let mobs = &self.play.mobs;
        mobs.alive.get(&(entity_id as u32)).and_then(|alive| mobs.defs.get(&alive.id)).is_some_and(|def| !def.pickup)
    }

    // ---------- skills ----------

    async fn fire_skills(&mut self, server: &Server, eid: u32, trigger: &str, other: Option<u32>) {
        let Some(id) = self.play.mobs.alive.get(&eid).map(|one| one.id.clone()) else { return };
        let skills: Vec<(usize, Skill)> = self
            .play
            .mobs
            .defs
            .get(&id)
            .map(|def| def.skills.iter().enumerate().filter(|(_, one)| one.trigger == trigger).map(|(at, one)| (at, one.clone())).collect())
            .unwrap_or_default();
        for (index, skill) in skills {
            self.run_skill(server, eid, index, &skill, other).await;
        }
    }

    /// One skill: cooldown and chance, then its conditions and actions once per target.
    async fn run_skill(&mut self, server: &Server, eid: u32, index: usize, skill: &Skill, other: Option<u32>) {
        let now = self.ticks;
        let Some((id, ready)) = self.play.mobs.alive.get(&eid).map(|one| (one.id.clone(), one.ready.get(&index).copied().unwrap_or(0)))
        else {
            return;
        };
        if now < ready {
            return;
        }
        if skill.chance < 100.0 && self.fraction() * 100.0 >= skill.chance {
            return;
        }
        if skill.cooldown > 0.0
            && let Some(one) = self.play.mobs.alive.get_mut(&eid)
        {
            one.ready.insert(index, now + (skill.cooldown * 20.0).round() as u64);
        }
        let plain = self.play.mobs.defs.get(&id).map(|def| def.plain.clone()).unwrap_or_else(|| id.clone());
        for target in self.skill_targets(server, eid, skill, other) {
            let mut extra = vec![("mob".to_string(), plain.clone()), ("mob_id".to_string(), id.clone())];
            let run = match target {
                Target::Player(player) => {
                    extra.push(("target".into(), player.get_name()));
                    Run { caster: Some(eid), ..Run::of(&player, extra) }
                }
                Target::Entity(entity, name) => {
                    extra.push(("target".into(), name.clone()));
                    Run { player: None, name, extra, caster: Some(eid), target: Some(entity) }
                }
            };
            if self.test(server, Some(&skill.conditions), &run).await {
                self.run_actions(server, Some(&skill.actions), &run).await;
            }
        }
    }

    fn skill_targets(&self, server: &Server, eid: u32, skill: &Skill, other: Option<u32>) -> Vec<Target> {
        let Some(alive) = self.play.mobs.alive.get(&eid) else { return Vec::new() };
        match skill.targets.as_str() {
            "self" => self.target_of(server, eid).into_iter().collect(),
            "target" => alive
                .entity
                .as_mob()
                .and_then(|mob| mob.get_target())
                .and_then(|target| self.target_of(server, target.get_id()))
                .into_iter()
                .collect(),
            "nearby" => {
                let (x, y, z) = alive.entity.get_position();
                let world = alive.entity.get_world();
                let world = crate::world_name(&world.get_id(), &world.get_dimension());
                let radius = skill.radius.clamp(1.0, 64.0);
                crate::players_of(server)
                    .into_iter()
                    .filter(|(_, who)| who.world == world)
                    .filter(|(player, _)| {
                        let (px, py, pz) = player.get_position();
                        (px - x).powi(2) + (py - y).powi(2) + (pz - z).powi(2) <= radius * radius
                    })
                    .map(|(player, _)| Target::Player(player))
                    .collect()
            }
            _ => other.and_then(|id| self.target_of(server, id)).into_iter().collect(),
        }
    }

    /// A player, one of our mobs, or any entity, by entity id.
    fn target_of(&self, server: &Server, id: u32) -> Option<Target> {
        if let Some(player) = server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == id) {
            return Some(Target::Player(player));
        }
        if let Some(alive) = self.play.mobs.alive.get(&id) {
            let name = self.play.mobs.defs.get(&alive.id).map(|def| def.plain.clone()).unwrap_or_else(|| alive.id.clone());
            return Some(Target::Entity(id, name));
        }
        let entity = self.entity_by_id(server, id)?;
        Some(Target::Entity(id, entity_name(&entity)))
    }

    /// An entity by id: ours first, then players, then every world.
    pub(super) fn entity_by_id(&self, server: &Server, id: u32) -> Option<Entity> {
        if let Some(living) = self.play.mobs.alive.get(&id).and_then(|alive| alive.entity.as_living()) {
            return Some(living.as_entity());
        }
        if let Some(player) = server.get_all_players().into_iter().find(|one| one.as_entity().get_id() == id) {
            return Some(player.as_entity());
        }
        server.get_all_worlds().into_iter().find_map(|world| world.get_entities().into_iter().find(|one| one.get_id() == id))
    }

    /// The entity an action works on: the skill's target, else the run's player.
    fn acted_on(&self, server: &Server, run: &Run) -> Option<Entity> {
        match run.target {
            Some(id) => self.entity_by_id(server, id),
            None => run.player.and_then(|id| server.get_player_by_uuid(id)).map(|player| player.as_entity()),
        }
    }

    /// Who casts: the skill's mob, else the run's player.
    fn caster_of(&self, server: &Server, run: &Run) -> Option<Entity> {
        match run.caster {
            Some(id) => self.entity_by_id(server, id),
            None => run.player.and_then(|id| server.get_player_by_uuid(id)).map(|player| player.as_entity()),
        }
    }

    // ---------- actions ----------

    /// The M5 actions: `damage`, `heal`, `potion`, `launch`, `particle`, `projectile`, `summon`. In a skill they
    /// work on each target; elsewhere on the run's player.
    pub(super) async fn mob_action(&mut self, server: &Server, kind: &str, one: &Data, run: &Run) {
        // A player target goes through the player API (it syncs to their client).
        let player = if run.target.is_none() { run.player.and_then(|id| server.get_player_by_uuid(id)) } else { None };
        let Some(target) = self.acted_on(server, run) else { return };
        match kind {
            "damage" => {
                let amount = number(one, "amount", 4.0, 0.0, 1000.0) as f32;
                // In a skill the mob deals it (kill credit, the target hits back); elsewhere no one does.
                let caster = run.caster.filter(|id| *id != target.get_id()).and_then(|id| self.entity_by_id(server, id));
                match (&player, target.as_living(), caster) {
                    (_, Some(living), Some(caster)) => living.damage_by_entity(amount, DamageType::Generic, &caster).await,
                    (Some(player), _, None) => player.damage(amount, DamageType::Generic).await,
                    (None, Some(living), None) => living.damage(amount, DamageType::Generic).await,
                    _ => {}
                }
            }
            "heal" => {
                let amount = number(one, "amount", 4.0, 0.0, 1000.0) as f32;
                match &player {
                    Some(player) => player.heal(amount).await,
                    None => {
                        if let Some(living) = target.as_living()
                            && !living.is_dead()
                        {
                            living.heal(amount).await;
                        }
                    }
                }
            }
            "potion" => {
                let Some(effect) = status_effect(&clean(&fields::text(one, "effect", "speed"))) else { return };
                let seconds = number(one, "seconds", 5.0, 0.0, 24.0 * 60.0 * 60.0);
                let level = number(one, "level", 1.0, 1.0, 255.0);
                let instance = StatusEffectInstance {
                    effect_type: effect,
                    duration: (seconds * 20.0).round() as u32,
                    amplifier: (level.round() as u8).saturating_sub(1),
                    ambient: false,
                    show_particles: true,
                    show_icon: true,
                };
                match (&player, target.as_living()) {
                    (Some(player), _) => player.add_effect(instance).await,
                    (None, Some(living)) => living.add_effect(instance).await,
                    _ => {}
                }
            }
            "launch" => {
                let up = number(one, "up", 0.8, -10.0, 10.0);
                let forward = number(one, "forward", 0.0, -10.0, 10.0);
                let (tx, _, tz) = target.get_position();
                // Away from the mob in a skill; the target's own facing elsewhere.
                let away = run
                    .caster
                    .filter(|id| *id != target.get_id())
                    .and_then(|id| self.entity_by_id(server, id))
                    .map(|caster| {
                        let (cx, _, cz) = caster.get_position();
                        (tx - cx, tz - cz)
                    })
                    .filter(|(dx, dz)| dx * dx + dz * dz > 1e-6);
                let (dx, dz) = match away {
                    Some((dx, dz)) => {
                        let length = (dx * dx + dz * dz).sqrt();
                        (dx / length, dz / length)
                    }
                    None => facing(target.get_yaw()),
                };
                let velocity = (dx * forward, up, dz * forward);
                match &player {
                    Some(player) => player.set_velocity(velocity),
                    None => target.set_velocity(velocity),
                }
            }
            "particle" => {
                let Some(particle) = particle(&clean(&fields::text(one, "particle", "flame"))) else { return };
                let count = number(one, "count", 10.0, 1.0, 1000.0) as i32;
                let (x, y, z) = target.get_position();
                let (width, height) = (f64::from(target.get_width()), f64::from(target.get_height()));
                target.get_world().spawn_particle(
                    particle,
                    (x, y + height / 2.0, z),
                    (width / 2.0, height / 2.0, width / 2.0),
                    0.02,
                    count,
                );
            }
            "projectile" => self.shoot(server, one, run, &target).await,
            "summon" => self.summon(server, one, &target).await,
            _ => {}
        }
    }

    /// Fires a projectile from the caster toward the target (in a skill) or where the caster looks.
    async fn shoot(&self, server: &Server, one: &Data, run: &Run, target: &Entity) {
        let wanted = clean(&fields::text(one, "projectile", "arrow"));
        let kind = if rules::PROJECTILES.contains(&wanted.as_str()) { wanted } else { "arrow".to_string() };
        let Some(entity_type) = names::entity_type(&kind) else { return };
        let speed = number(one, "speed", 1.5, 0.1, 5.0);
        let Some(caster) = self.caster_of(server, run) else { return };
        let (ex, ey, ez) = caster.get_eye_position();
        let aimed = (run.caster.is_some() && target.get_id() != caster.get_id())
            .then(|| {
                let (tx, ty, tz) = target.get_position();
                unit((tx - ex, ty + f64::from(target.get_height()) * 0.5 - ey, tz - ez))
            })
            .flatten();
        let (dx, dy, dz) = aimed.unwrap_or_else(|| look(caster.get_yaw(), caster.get_pitch()));
        let projectile = caster.get_world().spawn_entity(entity_type, (ex + dx, ey + dy, ez + dz)).await;
        projectile.set_rotation((-dx.atan2(dz)).to_degrees() as f32, (-dy.asin()).to_degrees() as f32);
        projectile.set_velocity((dx * speed, dy * speed, dz * speed));
    }

    /// Spawns a studio mob (its id, or `studio:mob/<id>`) or a vanilla entity at the target, `count` times.
    async fn summon(&mut self, server: &Server, one: &Data, target: &Entity) {
        let name = fields::text(one, "mob", "").trim().to_string();
        if name.is_empty() {
            return;
        }
        let count = number(one, "count", 1.0, 1.0, MOST_SUMMON) as u32;
        let at = target.get_position();
        let id = name.strip_prefix("studio:mob/").or_else(|| name.strip_prefix("studio:")).unwrap_or(&name).to_string();
        if self.play.mobs.defs.contains_key(&id) {
            for _ in 0..count {
                let world = target.get_world();
                self.spawn_mob(server, &id, &world, at).await;
            }
            return;
        }
        let Some(kind) =
            mob_type(&clean(&name)).or_else(|| names::entity_type(&clean(&name)).filter(|kind| !matches!(kind, EntityType::Player)))
        else {
            return;
        };
        let world = target.get_world();
        for _ in 0..count {
            let _ = world.spawn_entity(kind, at).await;
        }
    }
}

// ---------- helpers ----------

fn flag(data: &Data, key: &str, fallback: bool) -> bool {
    data.get(key).and_then(Value::as_bool).unwrap_or(fallback)
}

/// A number field (a number or numeric text), clamped.
fn number(data: &Data, key: &str, fallback: f64, least: f64, most: f64) -> f64 {
    let value = match data.get(key) {
        Some(Value::Number(one)) => one.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    value.filter(|one| one.is_finite()).unwrap_or(fallback).clamp(least, most)
}

/// A Minecraft name without `minecraft:`, lower case.
fn clean(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    lower.strip_prefix("minecraft:").unwrap_or(&lower).to_string()
}

/// The entity type of a mob base: any living kind Pumpkin knows, not a player.
fn mob_type(base: &str) -> Option<EntityType> {
    names::entity_type(base).filter(|kind| !matches!(kind, EntityType::Player))
}

/// An entity's name: its custom name, else its type (`zombie`).
fn entity_name(entity: &Entity) -> String {
    if let Some(name) = entity.get_custom_name() {
        let text = name.get_text();
        if !text.trim().is_empty() {
            return text;
        }
    }
    crate::map::biome_name(&format!("{:?}", entity.get_type())).trim_start_matches("minecraft:").to_string()
}

/// A goal that moves the mob about on its own (Paper's check: stroll, wander, random swim or fly).
fn wanders(goal: &str) -> bool {
    goal.contains("stroll") || goal.contains("wander") || goal.contains("random_swim") || goal.contains("random_fly")
}

/// Light a mob would spawn in: block light, or sky light dimmed at night and in rain (like Paper's light level).
fn light_at(world: &World, at: BlockPos) -> i64 {
    let time = world.get_time_of_day() % 24_000;
    let mut dark = if (13_000..23_000).contains(&time) { 11 } else { 0 };
    if world.is_raining() {
        dark = (dark + 3).min(11);
    }
    let sky = i64::from(world.get_sky_light(at)) - dark;
    sky.max(i64::from(world.get_block_light(at)))
}

/// Whether a biome (`minecraft:plains`) is in a list that may leave out `minecraft:`.
fn biome_listed(list: &[String], biome: &str) -> bool {
    list.iter()
        .map(|one| one.trim().to_ascii_lowercase())
        .filter(|one| !one.is_empty())
        .any(|one| if one.contains(':') { one == biome } else { format!("minecraft:{one}") == biome })
}

/// The horizontal way a yaw (degrees) faces.
fn facing(yaw: f32) -> (f64, f64) {
    let turn = f64::from(yaw).to_radians();
    (-turn.sin(), turn.cos())
}

/// The way a yaw and pitch (degrees) look.
fn look(yaw: f32, pitch: f32) -> (f64, f64, f64) {
    let (turn, tilt) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    (-turn.sin() * tilt.cos(), -tilt.sin(), turn.cos() * tilt.cos())
}

fn unit((x, y, z): (f64, f64, f64)) -> Option<(f64, f64, f64)> {
    let length = (x * x + y * y + z * z).sqrt();
    (length > 1e-6).then(|| (x / length, y / length, z / length))
}

/// A vanilla effect by its key (`speed`, `jump_boost`).
pub(super) fn status_effect(name: &str) -> Option<StatusEffectType> {
    Some(match name {
        "speed" => StatusEffectType::Speed,
        "slowness" => StatusEffectType::Slowness,
        "haste" => StatusEffectType::Haste,
        "mining_fatigue" => StatusEffectType::MiningFatigue,
        "strength" => StatusEffectType::Strength,
        "instant_health" => StatusEffectType::InstantHealth,
        "instant_damage" => StatusEffectType::InstantDamage,
        "jump_boost" => StatusEffectType::JumpBoost,
        "nausea" => StatusEffectType::Nausea,
        "regeneration" => StatusEffectType::Regeneration,
        "resistance" => StatusEffectType::Resistance,
        "fire_resistance" => StatusEffectType::FireResistance,
        "water_breathing" => StatusEffectType::WaterBreathing,
        "invisibility" => StatusEffectType::Invisibility,
        "blindness" => StatusEffectType::Blindness,
        "night_vision" => StatusEffectType::NightVision,
        "hunger" => StatusEffectType::Hunger,
        "weakness" => StatusEffectType::Weakness,
        "poison" => StatusEffectType::Poison,
        "wither" => StatusEffectType::Wither,
        "health_boost" => StatusEffectType::HealthBoost,
        "absorption" => StatusEffectType::Absorption,
        "saturation" => StatusEffectType::Saturation,
        "glowing" => StatusEffectType::Glowing,
        "levitation" => StatusEffectType::Levitation,
        "luck" => StatusEffectType::Luck,
        "unluck" => StatusEffectType::Unluck,
        "slow_falling" => StatusEffectType::SlowFalling,
        "conduit_power" => StatusEffectType::ConduitPower,
        "dolphins_grace" => StatusEffectType::DolphinsGrace,
        "bad_omen" => StatusEffectType::BadOmen,
        "hero_of_the_village" => StatusEffectType::HeroOfTheVillage,
        "darkness" => StatusEffectType::Darkness,
        "trial_omen" => StatusEffectType::TrialOmen,
        "raid_omen" => StatusEffectType::RaidOmen,
        "wind_charged" => StatusEffectType::WindCharged,
        "weaving" => StatusEffectType::Weaving,
        "oozing" => StatusEffectType::Oozing,
        "infested" => StatusEffectType::Infested,
        _ => return None,
    })
}

/// A particle by its Minecraft name (`flame`, `happy_villager`). Made from the API's `particles.wit`.
pub(super) fn particle(name: &str) -> Option<Particle> {
    Some(match name {
        "angry_villager" => Particle::AngryVillager,
        "block" => Particle::Block,
        "block_marker" => Particle::BlockMarker,
        "bubble" => Particle::Bubble,
        "sulfur_bubbles" => Particle::SulfurBubbles,
        "noxious_gas" => Particle::NoxiousGas,
        "noxious_gas_cloud" => Particle::NoxiousGasCloud,
        "geyser" => Particle::Geyser,
        "geyser_base" => Particle::GeyserBase,
        "geyser_poof" => Particle::GeyserPoof,
        "geyser_plume" => Particle::GeyserPlume,
        "cloud" => Particle::Cloud,
        "copper_fire_flame" => Particle::CopperFireFlame,
        "crit" => Particle::Crit,
        "damage_indicator" => Particle::DamageIndicator,
        "dragon_breath" => Particle::DragonBreath,
        "dripping_lava" => Particle::DrippingLava,
        "falling_lava" => Particle::FallingLava,
        "landing_lava" => Particle::LandingLava,
        "dripping_water" => Particle::DrippingWater,
        "falling_water" => Particle::FallingWater,
        "dust" => Particle::Dust,
        "dust_color_transition" => Particle::DustColorTransition,
        "effect" => Particle::Effect,
        "elder_guardian" => Particle::ElderGuardian,
        "enchanted_hit" => Particle::EnchantedHit,
        "enchant" => Particle::Enchant,
        "end_rod" => Particle::EndRod,
        "entity_effect" => Particle::EntityEffect,
        "explosion_emitter" => Particle::ExplosionEmitter,
        "explosion" => Particle::Explosion,
        "gust" => Particle::Gust,
        "small_gust" => Particle::SmallGust,
        "gust_emitter_large" => Particle::GustEmitterLarge,
        "gust_emitter_small" => Particle::GustEmitterSmall,
        "sonic_boom" => Particle::SonicBoom,
        "falling_dust" => Particle::FallingDust,
        "firework" => Particle::Firework,
        "fishing" => Particle::Fishing,
        "flame" => Particle::Flame,
        "infested" => Particle::Infested,
        "cherry_leaves" => Particle::CherryLeaves,
        "pale_oak_leaves" => Particle::PaleOakLeaves,
        "red_poplar_leaves" => Particle::RedPoplarLeaves,
        "orange_poplar_leaves" => Particle::OrangePoplarLeaves,
        "yellow_poplar_leaves" => Particle::YellowPoplarLeaves,
        "tinted_leaves" => Particle::TintedLeaves,
        "sculk_soul" => Particle::SculkSoul,
        "sculk_charge" => Particle::SculkCharge,
        "sculk_charge_pop" => Particle::SculkChargePop,
        "soul_fire_flame" => Particle::SoulFireFlame,
        "soul" => Particle::Soul,
        "flash" => Particle::Flash,
        "happy_villager" => Particle::HappyVillager,
        "composter" => Particle::Composter,
        "heart" => Particle::Heart,
        "instant_effect" => Particle::InstantEffect,
        "item" => Particle::Item,
        "vibration" => Particle::Vibration,
        "trail" => Particle::Trail,
        "pause_mob_growth" => Particle::PauseMobGrowth,
        "reset_mob_growth" => Particle::ResetMobGrowth,
        "item_slime" => Particle::ItemSlime,
        "item_cobweb" => Particle::ItemCobweb,
        "item_snowball" => Particle::ItemSnowball,
        "large_smoke" => Particle::LargeSmoke,
        "lava" => Particle::Lava,
        "mycelium" => Particle::Mycelium,
        "note" => Particle::Note,
        "poof" => Particle::Poof,
        "portal" => Particle::Portal,
        "rain" => Particle::Rain,
        "smoke" => Particle::Smoke,
        "white_smoke" => Particle::WhiteSmoke,
        "sneeze" => Particle::Sneeze,
        "spit" => Particle::Spit,
        "squid_ink" => Particle::SquidInk,
        "sweep_attack" => Particle::SweepAttack,
        "totem_of_undying" => Particle::TotemOfUndying,
        "underwater" => Particle::Underwater,
        "splash" => Particle::Splash,
        "witch" => Particle::Witch,
        "bubble_pop" => Particle::BubblePop,
        "current_down" => Particle::CurrentDown,
        "bubble_column_up" => Particle::BubbleColumnUp,
        "nautilus" => Particle::Nautilus,
        "dolphin" => Particle::Dolphin,
        "campfire_cosy_smoke" => Particle::CampfireCosySmoke,
        "campfire_signal_smoke" => Particle::CampfireSignalSmoke,
        "dripping_honey" => Particle::DrippingHoney,
        "falling_honey" => Particle::FallingHoney,
        "landing_honey" => Particle::LandingHoney,
        "falling_nectar" => Particle::FallingNectar,
        "falling_spore_blossom" => Particle::FallingSporeBlossom,
        "ash" => Particle::Ash,
        "crimson_spore" => Particle::CrimsonSpore,
        "warped_spore" => Particle::WarpedSpore,
        "spore_blossom_air" => Particle::SporeBlossomAir,
        "dripping_obsidian_tear" => Particle::DrippingObsidianTear,
        "falling_obsidian_tear" => Particle::FallingObsidianTear,
        "landing_obsidian_tear" => Particle::LandingObsidianTear,
        "reverse_portal" => Particle::ReversePortal,
        "white_ash" => Particle::WhiteAsh,
        "small_flame" => Particle::SmallFlame,
        "snowflake" => Particle::Snowflake,
        "dripping_dripstone_lava" => Particle::DrippingDripstoneLava,
        "falling_dripstone_lava" => Particle::FallingDripstoneLava,
        "dripping_dripstone_water" => Particle::DrippingDripstoneWater,
        "falling_dripstone_water" => Particle::FallingDripstoneWater,
        "glow_squid_ink" => Particle::GlowSquidInk,
        "glow" => Particle::Glow,
        "wax_on" => Particle::WaxOn,
        "wax_off" => Particle::WaxOff,
        "electric_spark" => Particle::ElectricSpark,
        "scrape" => Particle::Scrape,
        "shriek" => Particle::Shriek,
        "egg_crack" => Particle::EggCrack,
        "dust_plume" => Particle::DustPlume,
        "trial_spawner_detection" => Particle::TrialSpawnerDetection,
        "trial_spawner_detection_ominous" => Particle::TrialSpawnerDetectionOminous,
        "vault_connection" => Particle::VaultConnection,
        "dust_pillar" => Particle::DustPillar,
        "ominous_spawning" => Particle::OminousSpawning,
        "raid_omen" => Particle::RaidOmen,
        "trial_omen" => Particle::TrialOmen,
        "block_crumble" => Particle::BlockCrumble,
        "firefly" => Particle::Firefly,
        "sulfur_cube_goo" => Particle::SulfurCubeGoo,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_and_numbers() {
        assert_eq!(clean(" minecraft:Zombie "), "zombie");
        assert!(mob_type("zombie").is_some());
        assert!(mob_type("player").is_none());
        assert!(mob_type("nope").is_none());
        assert!(status_effect("jump_boost").is_some());
        assert!(status_effect("nope").is_none());
        assert!(particle("happy_villager").is_some());
        assert!(particle("nope").is_none());
        let data: Data = serde_json::from_str(r#"{"amount":"6","speed":99}"#).unwrap();
        assert_eq!(number(&data, "amount", 4.0, 0.0, 1000.0), 6.0);
        assert_eq!(number(&data, "speed", 1.5, 0.1, 5.0), 5.0);
        assert_eq!(number(&data, "missing", 1.5, 0.1, 5.0), 1.5);
    }

    #[test]
    fn matches_biomes_with_or_without_the_namespace() {
        let list = vec!["plains".to_string(), "minecraft:desert".to_string(), " ".to_string()];
        assert!(biome_listed(&list, "minecraft:plains"));
        assert!(biome_listed(&list, "minecraft:desert"));
        assert!(!biome_listed(&list, "minecraft:forest"));
    }

    #[test]
    fn points_the_right_way() {
        let (x, z) = facing(0.0);
        assert!(x.abs() < 1e-9 && (z - 1.0).abs() < 1e-9, "yaw 0 faces +z");
        let (x, _, _) = look(90.0, 0.0);
        assert!((x + 1.0).abs() < 1e-9, "yaw 90 faces -x");
        assert!(unit((0.0, 0.0, 0.0)).is_none());
        let (x, y, z) = unit((3.0, 0.0, 4.0)).unwrap();
        assert!((x - 0.6).abs() < 1e-9 && y == 0.0 && (z - 0.8).abs() < 1e-9);
    }
}
