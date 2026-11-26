//! The rule steps added with the node editor, as on Paper (`StudioSteps.java`): take items, experience, game mode,
//! kick, blocks and areas (set, fill, copy and paste), boss bars, hunger and health, permissions, and world management;
//! and the conditions has items, game mode, health, level, sneaking, time, weather and group. LuckPerms groups don't
//! exist on Pumpkin: `set_group` does nothing and the `group` condition never passes (said in the file's status).
//! Permissions given here last until the server stops.

use super::{Run, TAG, id_text, item_key, number_of, world_named};
use crate::game_mode_name;
use crate::studio::Studio;
use pumpkin_plugin_api::boss_bar::{BossBar, BossBarColor, BossBarDivision};
use pumpkin_plugin_api::common::{BlockPos, GameMode};
use pumpkin_plugin_api::uuid::Uuid;
use pumpkin_plugin_api::wit::pumpkin::plugin::player::{BedrockDisconnectReason, BedrockKickOptions, JavaKickOptions};
use pumpkin_plugin_api::wit::pumpkin::plugin::server::Difficulty;
use pumpkin_plugin_api::wit::pumpkin::plugin::world::BlockFlags;
use pumpkin_plugin_api::world::World;
use pumpkin_plugin_api::{GameRuleValue, ItemStack, PersistentDataHolder, Player, Server};
use std::collections::HashMap;
use std::fs;
use studio_format::fields::{self, Data};
use studio_format::region::{self, MOST_FILL, MOST_REGION, Region};

/// A box's low and high corners and how many blocks it holds.
type Area = ((i32, i32, i32), (i32, i32, i32), u64);

/// What the steps keep between ticks.
#[derive(Default)]
pub struct Steps {
    /// A boss bar shown by the `bossbar` step, by player (uuid text), and the tick it goes away (0 = stays).
    bars: HashMap<String, (BossBar, u64)>,
    /// Permissions to take back: when (tick), the player and the node.
    permissions: Vec<(u64, Uuid, String)>,
    /// Worlds to unload once their players have left: when (tick) and the name.
    unloads: Vec<(u64, String)>,
    /// When each player was last moved by a step, so the `teleport` event doesn't fire for our own moves.
    moved: HashMap<String, u64>,
    /// Whether `server_start` has fired.
    started: bool,
    /// Block state names by state id, for copying areas.
    names: HashMap<u16, String>,
}

impl Steps {
    pub(super) fn moved(&mut self, player: &Player, now: u64) {
        self.moved.insert(id_text(player.get_id()), now);
    }
}

fn game_mode(name: &str) -> Option<GameMode> {
    match name.trim().to_ascii_lowercase().as_str() {
        "survival" => Some(GameMode::Survival),
        "creative" => Some(GameMode::Creative),
        "adventure" => Some(GameMode::Adventure),
        "spectator" => Some(GameMode::Spectator),
        _ => None,
    }
}

fn compare(left: f64, op: &str, right: f64) -> bool {
    match op {
        "!=" => left != right,
        ">" => left > right,
        "<" => left < right,
        "<=" => left <= right,
        "=" => left == right,
        _ => left >= right,
    }
}

fn name_ok(text: &str, most: usize, extra: &[u8]) -> bool {
    !text.is_empty() && text.len() <= most && text.bytes().all(|one| one.is_ascii_alphanumeric() || extra.contains(&one))
}

fn is_air(state: &str) -> bool {
    let name = state.split('[').next().unwrap_or_default();
    matches!(name.trim_start_matches("minecraft:"), "air" | "cave_air" | "void_air")
}

impl Studio {
    /// Once a studio tick: boss bars that ran out, permissions that ran out, worlds to unload, `server_start`.
    pub(super) async fn tick_steps(&mut self, server: &Server) {
        let now = self.ticks;
        if !self.play.steps.started && now >= 100 {
            self.play.steps.started = true;
            self.fire(server, "server_start", None, Vec::new(), |_| true).await;
        }
        let ended: Vec<String> =
            self.play.steps.bars.iter().filter(|(_, (_, until))| *until > 0 && *until <= now).map(|(id, _)| id.clone()).collect();
        for id in ended {
            if let Some((bar, _)) = self.play.steps.bars.remove(&id) {
                bar.remove_all().await;
            }
        }
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.play.steps.permissions).into_iter().partition(|one| one.0 <= now);
        self.play.steps.permissions = later;
        for (_, id, node) in due {
            if let Some(player) = server.get_player_by_uuid(id) {
                player.unset_permission(&node);
            }
        }
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.play.steps.unloads).into_iter().partition(|one| one.0 <= now);
        self.play.steps.unloads = later;
        for (_, name) in due {
            if let Err(problem) = server.unload_world(name.clone()).await {
                tracing::warn!("Couldn't unload the world {name}: {problem}");
            }
        }
        self.play.steps.moved.retain(|_, at| *at + 10 > now);
    }

    /// Whether a step moved this player just now (the `teleport` event skips our own moves).
    pub fn moved_by_step(&self, player: &Player) -> bool {
        self.play.steps.moved.contains_key(&id_text(player.get_id()))
    }

    pub(super) async fn stop_steps(&mut self) {
        for (_, (bar, _)) in self.play.steps.bars.drain() {
            bar.remove_all().await;
        }
    }

    // ---------- conditions ----------

    pub(super) async fn more_check(&mut self, server: &Server, one: &Data, run: &Run, player: Option<&Player>) -> bool {
        let kind = fields::text(one, "type", "");
        let world_of = |player: Option<&Player>| match player {
            Some(player) => Some(player.as_entity().get_world()),
            None => server.get_all_worlds().into_iter().next(),
        };
        match kind.as_str() {
            "has_item" => match player {
                Some(player) => count(player, &fields::text(one, "item", "")).await >= fields::number(one, "amount", 1, 1, 64 * 36) as u32,
                None => false,
            },
            "gamemode" => {
                let modes: Vec<String> = fields::lines(one, "modes").into_iter().filter(|one| !one.trim().is_empty()).collect();
                player.is_some_and(|player| modes.is_empty() || modes.iter().any(|mode| mode == game_mode_name(player.get_gamemode())))
            }
            "health" => player.is_some_and(|player| {
                compare(f64::from(player.get_health()), &fields::text(one, "op", ">="), number_of(one.get("value"), 10.0))
            }),
            "level" => player.is_some_and(|player| {
                compare(f64::from(player.get_experience_level()), &fields::text(one, "op", ">="), number_of(one.get("value"), 10.0))
            }),
            "sneaking" => player.is_some_and(Player::is_sneaking),
            "time" => world_of(player).is_some_and(|world| {
                let time = world.get_time_of_day() % 24_000;
                let day = !(12_300..23_850).contains(&time);
                (fields::text(one, "is", "day") == "night") != day
            }),
            "weather" => world_of(player).is_some_and(|world| {
                let now = if world.is_thundering() {
                    "thunder"
                } else if world.is_raining() {
                    "rain"
                } else {
                    "clear"
                };
                now == fields::text(one, "is", "clear")
            }),
            // LuckPerms groups are Paper only.
            _ => {
                let _ = run;
                false
            }
        }
    }

    // ---------- actions ----------

    pub(super) async fn more_step(&mut self, server: &Server, kind: &str, one: &Data, run: &Run, player: Option<&Player>) {
        match kind {
            "take" => {
                if let Some(player) = player {
                    take(player, &fields::text(one, "item", ""), fields::number(one, "amount", 1, 1, 64 * 36) as u32).await;
                }
            }
            "xp" => {
                if let Some(player) = player {
                    xp(player, one).await;
                }
            }
            "gamemode" => {
                if let Some(player) = player
                    && let Some(mode) = game_mode(&fields::text(one, "mode", "survival"))
                {
                    player.set_gamemode(mode).await;
                }
            }
            "kick" => {
                if let Some(player) = player {
                    let reason = self.text_for(server, &fields::text(one, "text", ""), run);
                    if let Some(java) = player.as_java() {
                        java.kick(JavaKickOptions::new(reason)).await;
                    } else if let Some(bedrock) = player.as_bedrock() {
                        let message = self.plain_for(server, &fields::text(one, "text", ""), run);
                        bedrock.kick(BedrockKickOptions::new(BedrockDisconnectReason::Kicked, message)).await;
                    }
                }
            }
            "food" => {
                if let Some(player) = player {
                    player.set_food_level(fields::number(one, "amount", 20, 0, 20) as u8).await;
                }
            }
            "health" => {
                if let Some(player) = player {
                    let most = f64::from(player.get_max_health());
                    player.set_health(number_of(one.get("amount"), 20.0).clamp(0.0, most) as f32);
                }
            }
            "bossbar" => {
                if let Some(player) = player {
                    self.step_bar(server, player, one, run).await;
                }
            }
            "block" => self.set_block(server, one, run, player).await,
            "fill" => self.fill(server, one, run, player).await,
            "region" => self.region(server, one, run, player).await,
            "set_permission" => {
                let node = self.plain_for(server, &fields::text(one, "permission", ""), run).trim().to_string();
                if let Some(player) = player
                    && name_ok(&node, 128, b"_.*-:")
                {
                    if fields::text(one, "op", "give") == "take" {
                        player.unset_permission(&node);
                    } else {
                        player.set_permission(&node, true);
                        let seconds = fields::number(one, "seconds", 0, 0, i64::from(i32::MAX)) as u64;
                        if seconds > 0 {
                            self.play.steps.permissions.push((self.ticks + seconds * 20, player.get_id(), node));
                        }
                    }
                }
            }
            "manage_world" => self.manage_world(server, one, run, player).await,
            // `set_group`: LuckPerms groups are Paper only.
            _ => {}
        }
    }

    async fn step_bar(&mut self, server: &Server, player: &Player, one: &Data, run: &Run) {
        let key = id_text(player.get_id());
        if let Some((bar, _)) = self.play.steps.bars.remove(&key) {
            bar.remove_all().await;
        }
        if fields::text(one, "op", "show") == "hide" {
            return;
        }
        let color = match fields::text(one, "color", "white").as_str() {
            "pink" => BossBarColor::Pink,
            "blue" => BossBarColor::Blue,
            "red" => BossBarColor::Red,
            "green" => BossBarColor::Green,
            "yellow" => BossBarColor::Yellow,
            "purple" => BossBarColor::Purple,
            _ => BossBarColor::White,
        };
        let title = self.text_for(server, &fields::text(one, "text", ""), run);
        let bar = BossBar::new(title, color, BossBarDivision::NoDivision);
        bar.set_health(number_of(one.get("progress"), 1.0).clamp(0.0, 1.0) as f32).await;
        let Some(owned) = server.get_player_by_uuid(player.get_id()) else { return };
        bar.add_player(owned).await;
        let seconds = fields::number(one, "seconds", 10, 0, 24 * 60 * 60) as u64;
        let until = if seconds > 0 { self.ticks + seconds * 20 } else { 0 };
        self.play.steps.bars.insert(key, (bar, until));
    }

    // ---------- blocks and areas ----------

    /// The world a step names, else the player's, else the first one.
    fn world_for(&mut self, server: &Server, one: &Data, run: &Run, player: Option<&Player>) -> Option<World> {
        let name = self.plain_for(server, &fields::text(one, "world", ""), run).trim().to_string();
        if !name.is_empty() {
            return world_named(server, &name);
        }
        match player {
            Some(player) => Some(player.as_entity().get_world()),
            None => server.get_all_worlds().into_iter().next(),
        }
    }

    /// A spot: each axis a number, `~` (the player's) or `~3`; placeholders work.
    fn spot(&mut self, server: &Server, one: &Data, suffix: &str, run: &Run, player: Option<&Player>) -> (i32, i32, i32) {
        let base = player.map_or((0, 64, 0), |player| {
            let (x, y, z) = player.as_entity().get_position();
            (x.floor() as i32, y.floor() as i32, z.floor() as i32)
        });
        let mut axis = |key: &str, base: i32| {
            let text = self.plain_for(server, &fields::scalar(one, &format!("{key}{suffix}"), "~"), run);
            region::axis(&text, base).unwrap_or(base)
        };
        (axis("x", base.0), axis("y", base.1), axis("z", base.2))
    }

    /// A game block (`oak_stairs[facing=east]`) or one of ours (`studio:block/<id>`).
    fn block_state(&mut self, text: &str) -> Option<u16> {
        let name = text.trim();
        if let Some(id) = name.strip_prefix("studio:") {
            let id = id.strip_prefix("block/").unwrap_or(id);
            return self.blocks.as_ref().and_then(|blocks| blocks.state_of(id));
        }
        let lower = name.to_ascii_lowercase();
        let full = if lower.contains(':') { lower } else { format!("minecraft:{lower}") };
        self.state_of(&full)
    }

    fn height(world: &World) -> (i32, i32) {
        let low = world.get_min_y();
        (low, low + world.get_height() as i32 - 1)
    }

    fn area(world: &World, a: (i32, i32, i32), b: (i32, i32, i32)) -> Option<Area> {
        let (bottom, top) = Self::height(world);
        let low = (a.0.min(b.0), a.1.min(b.1).max(bottom), a.2.min(b.2));
        let high = (a.0.max(b.0), a.1.max(b.1).min(top), a.2.max(b.2));
        if high.1 < low.1 {
            return None;
        }
        let volume = (high.0 - low.0 + 1) as u64 * (high.1 - low.1 + 1) as u64 * (high.2 - low.2 + 1) as u64;
        Some((low, high, volume))
    }

    async fn set_block(&mut self, server: &Server, one: &Data, run: &Run, player: Option<&Player>) {
        let block = self.plain_for(server, &fields::text(one, "block", "stone"), run);
        let (Some(world), Some(state)) = (self.world_for(server, one, run, player), self.block_state(&block)) else { return };
        let at = self.spot(server, one, "", run, player);
        let (bottom, top) = Self::height(&world);
        if at.1 < bottom || at.1 > top {
            return;
        }
        world
            .set_block_state(BlockPos { x: at.0, y: at.1, z: at.2 }, state, BlockFlags::NOTIFY_NEIGHBORS | BlockFlags::NOTIFY_LISTENERS)
            .await;
    }

    async fn fill(&mut self, server: &Server, one: &Data, run: &Run, player: Option<&Player>) {
        let block = self.plain_for(server, &fields::text(one, "block", "air"), run);
        let (Some(world), Some(state)) = (self.world_for(server, one, run, player), self.block_state(&block)) else { return };
        let (a, b) = (self.spot(server, one, "1", run, player), self.spot(server, one, "2", run, player));
        let Some((low, high, volume)) = Self::area(&world, a, b) else { return };
        if volume > MOST_FILL {
            return;
        }
        for y in low.1..=high.1 {
            for z in low.2..=high.2 {
                for x in low.0..=high.0 {
                    world.set_block_state(BlockPos { x, y, z }, state, BlockFlags::NOTIFY_LISTENERS).await;
                }
            }
        }
    }

    async fn region(&mut self, server: &Server, one: &Data, run: &Run, player: Option<&Player>) {
        let name = self.plain_for(server, &fields::text(one, "name", ""), run).trim().to_ascii_lowercase();
        let Some(world) = self.world_for(server, one, run, player) else { return };
        if !region::name_ok(&name) {
            return;
        }
        let path = self.root.join(region::data_path(&name));
        if fields::text(one, "op", "copy") == "paste" {
            let at = self.spot(server, one, "", run, player);
            let quarter = (fields::number(one, "rotate", 0, 0, 270) / 90) as u32;
            let air = one.get("air").and_then(serde_json::Value::as_bool) == Some(true);
            let Some(read) = fs::read_to_string(&path).ok().and_then(|text| Region::from_json(&text)) else { return };
            self.paste(&world, &read, at, quarter, air).await;
            return;
        }
        let (a, b) = (self.spot(server, one, "1", run, player), self.spot(server, one, "2", run, player));
        let Some((low, high, volume)) = Self::area(&world, a, b) else { return };
        if volume > MOST_REGION {
            return;
        }
        let mut states = Vec::with_capacity(volume as usize);
        for y in low.1..=high.1 {
            for z in low.2..=high.2 {
                for x in low.0..=high.0 {
                    let id = world.get_block_state_id(BlockPos { x, y, z });
                    let name = self.play.steps.names.entry(id).or_insert_with(|| state_name(id)).clone();
                    states.push(name);
                }
            }
        }
        let size = ((high.0 - low.0 + 1) as u32, (high.1 - low.1 + 1) as u32, (high.2 - low.2 + 1) as u32);
        let text = Region::from_states(size, &states).to_json();
        let temp = path.with_extension("json.tmp");
        let saved =
            path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&temp, text)).and_then(|()| fs::rename(&temp, &path));
        if let Err(error) = saved {
            tracing::warn!("Couldn't save the studio area {name}: {error}");
        }
    }

    async fn paste(&mut self, world: &World, read: &Region, at: (i32, i32, i32), quarter: u32, air: bool) {
        let palette: Vec<Option<u16>> = read
            .palette
            .iter()
            .map(|state| if !air && is_air(state) { None } else { self.state_of(&region::turn_state(state, quarter)) })
            .collect();
        let (width, height, depth) = read.size;
        let (bottom, top) = Self::height(world);
        for y in 0..height {
            let wy = at.1 + y as i32;
            if wy < bottom || wy > top {
                continue;
            }
            for z in 0..depth {
                for x in 0..width {
                    let Some(Some(state)) = palette.get(read.at(x, y, z) as usize) else { continue };
                    let (tx, tz) = region::turn(x, z, width, depth, quarter);
                    world
                        .set_block_state(BlockPos { x: at.0 + tx as i32, y: wy, z: at.2 + tz as i32 }, *state, BlockFlags::NOTIFY_LISTENERS)
                        .await;
                }
            }
        }
    }

    // ---------- worlds ----------

    async fn manage_world(&mut self, server: &Server, one: &Data, run: &Run, player: Option<&Player>) {
        let op = fields::text(one, "op", "time");
        let name = self.plain_for(server, &fields::text(one, "world", ""), run).trim().to_string();
        let value = self.plain_for(server, &fields::scalar(one, "value", ""), run).trim().to_ascii_lowercase();
        if !name.is_empty() && !name_ok(&name, 64, b"_.-") {
            return;
        }
        if op == "load" {
            if name.is_empty() || server.has_world(&name) {
                return;
            }
            let dimension = match value.as_str() {
                "nether" => pumpkin_plugin_api::Dimension::Nether,
                "end" => pumpkin_plugin_api::Dimension::End,
                _ => pumpkin_plugin_api::Dimension::Overworld,
            };
            server.create_world(name, dimension).await;
            return;
        }
        let Some(world) = self.world_for(server, one, run, player) else { return };
        match op.as_str() {
            "spawn" => {
                if let Some(player) = player {
                    let spawn = world.get_spawn_location();
                    let at = (f64::from(spawn.pos.x) + 0.5, f64::from(spawn.pos.y), f64::from(spawn.pos.z) + 0.5);
                    self.play.steps.moved(player, self.ticks);
                    if crate::who_of(player).world == name || name.is_empty() {
                        player.teleport(at, Some(spawn.yaw), Some(spawn.pitch), world).await;
                    } else {
                        player.teleport_world(world, at, Some(spawn.yaw), Some(spawn.pitch)).await;
                    }
                }
            }
            "unload" => {
                let Some(main) = server.get_all_worlds().into_iter().next() else { return };
                if name.is_empty()
                    || world_named(server, &name)
                        .is_some_and(|found| found.get_id() == main.get_id() && found.get_dimension() == main.get_dimension())
                {
                    return;
                }
                let spawn = main.get_spawn_location();
                let at = (f64::from(spawn.pos.x) + 0.5, f64::from(spawn.pos.y), f64::from(spawn.pos.z) + 0.5);
                for (each, who) in crate::players_of(server) {
                    if who.world == name {
                        self.play.steps.moved(&each, self.ticks);
                        let Some(main) = server.get_all_worlds().into_iter().next() else { return };
                        each.teleport_world(main, at, Some(spawn.yaw), Some(spawn.pitch)).await;
                    }
                }
                self.play.steps.unloads.push((self.ticks + 40, name));
            }
            "time" => world.set_time_of_day(match value.as_str() {
                "noon" => 6_000,
                "night" => 13_000,
                "midnight" => 18_000,
                _ => 1_000,
            }),
            "weather" => {
                world.set_raining(value != "clear").await;
                world.set_thundering(value == "thunder").await;
                if value == "clear" {
                    world.set_clear_weather_duration(20 * 60 * 10);
                }
            }
            // Pumpkin has one difficulty for the whole server.
            "difficulty" => {
                let difficulty = match value.as_str() {
                    "peaceful" => Difficulty::Peaceful,
                    "easy" => Difficulty::Easy,
                    "normal" => Difficulty::Normal,
                    "hard" => Difficulty::Hard,
                    _ => return,
                };
                server.set_difficulty(difficulty).await;
            }
            "border" => {
                let size = value.parse::<f64>().unwrap_or(0.0);
                if (1.0..=59_999_968.0).contains(&size) {
                    world.get_world_border().set_size(size);
                }
            }
            "rule" => {
                let Some(rule) = super::game_rule(fields::text(one, "rule", "").trim()) else { return };
                let set = match value.as_str() {
                    "true" => GameRuleValue::Bool(true),
                    "false" => GameRuleValue::Bool(false),
                    number => match number.parse::<i32>() {
                        Ok(count) => GameRuleValue::Int(count),
                        Err(_) => return,
                    },
                };
                world.set_game_rule(rule, set);
            }
            _ => {}
        }
    }
}

/// `minecraft:oak_stairs[facing=east,half=bottom]` for a state id (properties sorted).
fn state_name(id: u16) -> String {
    let Some(info) = pumpkin_plugin_api::world::block_state_to_info(id) else { return "minecraft:air".into() };
    let name = if info.name.contains(':') { info.name } else { format!("minecraft:{}", info.name) };
    if info.properties.is_empty() {
        return name;
    }
    let mut props = info.properties;
    props.sort();
    format!("{name}[{}]", props.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(","))
}

/// Whether a stack is the item a step names: `studio:<id>` (or `studio:item/<id>`) for ours, a game item otherwise
/// (not one of ours made from it).
async fn matches(stack: &ItemStack, wanted: &str) -> bool {
    let key = stack.get_registry_key().await;
    if key == "minecraft:air" {
        return false;
    }
    let ours = stack.get_string(TAG, "item").await;
    if let Some(id) = wanted.trim().strip_prefix("studio:") {
        let id = id.strip_prefix("item/").unwrap_or(id);
        return ours.as_deref() == Some(id);
    }
    ours.is_none() && item_key(wanted).is_some_and(|wanted| wanted == key)
}

async fn count(player: &Player, wanted: &str) -> u32 {
    let inventory = player.get_inventory().as_inventory();
    let mut count = 0;
    for slot in 0..inventory.get_size() {
        if let Some(stack) = inventory.get_item(slot)
            && matches(&stack, wanted).await
        {
            count += u32::from(stack.get_count().await);
        }
    }
    count
}

async fn take(player: &Player, wanted: &str, amount: u32) {
    let inventory = player.get_inventory().as_inventory();
    let mut left = amount;
    for slot in 0..inventory.get_size() {
        if left == 0 {
            break;
        }
        let Some(stack) = inventory.get_item(slot) else { continue };
        if !matches(&stack, wanted).await {
            continue;
        }
        let has = u32::from(stack.get_count().await);
        let taken = left.min(has);
        left -= taken;
        if taken == has {
            inventory.set_item(slot, None).await;
        } else {
            stack.set_count((has - taken) as u8).await;
            inventory.set_item(slot, Some(stack)).await;
        }
    }
}

async fn xp(player: &Player, one: &Data) {
    let amount = fields::number(one, "amount", 10, 0, 1_000_000) as i32;
    let levels = one.get("levels").and_then(serde_json::Value::as_bool) == Some(true);
    match fields::text(one, "op", "give").as_str() {
        "take" if levels => player.set_experience_level((player.get_experience_level() - amount).max(0)).await,
        "take" => player.add_experience_points(-amount.min(player.get_experience_points())).await,
        "set" if levels => player.set_experience_level(amount).await,
        "set" => player.set_experience_points(amount).await,
        _ if levels => player.add_experience_levels(amount).await,
        _ => player.add_experience_points(amount).await,
    }
}
