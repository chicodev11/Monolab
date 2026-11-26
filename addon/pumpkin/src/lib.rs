// The studio lock is held across host calls. That is safe here: the server runs one plugin call at a time
// (`LegacySyncReentry` root admission) and only lets the same call chain back in, and every handler that can be
// reached that way takes the studio with `try_lock`, so a callback skips instead of waiting (as on v0.1).
#![allow(clippy::await_holding_lock)]

mod alock;
mod map;
mod names;
mod studio;

use pumpkin_plugin_api::events::{
    BlockBreakEvent, BlockDamageAbortEvent, BlockDamageEvent, BlockDropItemEvent, BlockPhysicsEvent, BlockPistonExtendEvent, BlockPistonRetractEvent, BlockPlaceEvent, NotePlayEvent, DialogClickActionEvent, EntityCombustEvent, EntityDamageByEntityEvent, EntityDamageEvent, EntityDeathEvent, EntityDismountEvent, EntityPickupItemEvent, EntityTargetEvent, CraftItemEvent, EventData, EventHandler, LootGenerateEvent, PlayerFishEvent, EventPriority, InteractAction, InventoryClickEvent,
    InventoryCloseEvent, PlayerAdvancementDoneEvent, PlayerChangeWorldEvent, PlayerChatEvent, PlayerCommandPreprocessEvent, PlayerDeathEvent, PlayerInteractEntityEvent,
    PlayerInteractEvent, PlayerItemConsumeEvent, PlayerJoinEvent, PlayerLeaveEvent, PlayerRespawnEvent, ServerListPingEvent, PlayerToggleSneakEvent, PlayerDropItemEvent,
    PlayerLevelChangeEvent, PlayerTeleportEvent, PlayerGamemodeChangeEvent, PlayerPickBlockEvent, PlayerPickEntityEvent,
};
use pumpkin_plugin_api::permissions;
use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::{Context, Player, Plugin, PluginMetadata, Server, register_plugin};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: &str = "41";
const NAME: &str = "monolab";
const LIVE_TICKS: u64 = 20;
/// Loaded chunks and inventories go out every this many snapshots (5 seconds, like the Java add-ons).
const FILES_EVERY: u64 = 5;
const MAP_TICKS: u64 = 1;
const PERF_TICKS: u64 = 1200;
const KEEP_DAYS: i64 = 90;
const HOTSPOTS: usize = 6;
const HOTSPOT_FLOOR: u32 = 10;
/// The data pack older Monolab apps wrote studio advancements into (`<world>/datapacks/monolab-studio`); it is
/// turned off now that the add-on registers them itself.
pub(crate) const ADVANCEMENT_PACK: &str = "file/monolab-studio";
/// The server's UTC offset in minutes (from `timezone.json`), for rule schedules.
static OFFSET_MINUTES: AtomicI64 = AtomicI64::new(0);

struct Monolab {
    state: Arc<State>,
}

/// The permission node Pumpkin checks before a studio command reaches us (allowed for everyone).
const STUDIO_COMMAND_PERMISSION: &str = "monolab:studio.command";

struct State {
    folder: Mutex<PathBuf>,
    offset_minutes: AtomicI64,
    swept_day: Mutex<String>,
    warned: AtomicBool,
    map: Mutex<Option<map::Map>>,
    map_warned: AtomicBool,
    /// config.yml switches and when they were last read.
    config: Mutex<(u64, HashMap<String, bool>)>,
    studio: alock::AsyncLock<Option<studio::Studio>>,
    /// A model frame is waiting for the studio: later ones skip instead of piling up behind it.
    frame_waiting: AtomicBool,
    /// Kept from `on_load` so studio commands made later can still be registered.
    context: Mutex<Option<Context>>,
    /// The command list changed (or was never written): `commands.json` is written on the next snapshot.
    commands_dirty: AtomicBool,
    /// Snapshots so far, for the files written every few seconds (loaded chunks, inventories).
    snapshots: AtomicU64,
}

const CONFIG_DEFAULTS: &str = "# Monolab add-on. Set a feature to false to turn it off.
# The Monolab app changes these too (Monitor tab, add-on settings).

# Live world map around players.
map: true

# What online players carry, every 5 seconds.
inventories: true

# Which chunks are loaded, every 5 seconds.
loaded-chunks: true

# Command list for console autocomplete.
commands: true

# TPS and memory once a minute, for history.
performance-log: true

# Studio: scoreboard, tab list, MOTD, chat format, join messages, announcements and boss bar from the studio folder.
studio: true
";

impl Plugin for Monolab {
    fn new() -> Self {
        Self {
            state: Arc::new(State {
                folder: Mutex::new(PathBuf::from("data")),
                offset_minutes: AtomicI64::new(0),
                swept_day: Mutex::new(String::new()),
                warned: AtomicBool::new(false),
                map: Mutex::new(None),
                map_warned: AtomicBool::new(false),
                config: Mutex::new((0, HashMap::new())),
                studio: alock::AsyncLock::new(None),
                frame_waiting: AtomicBool::new(false),
                context: Mutex::new(None),
                commands_dirty: AtomicBool::new(true),
                snapshots: AtomicU64::new(0),
            }),
        }
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: NAME.into(),
            version: VERSION.into(),
            authors: vec!["Monolab".into()],
            description: "Records TPS and online players for the Monolab app's monitor, and runs its studio.".into(),
            dependencies: vec![],
            permissions: vec![permissions::FS_WRITE_DATA.into()],
        }
    }

    async fn on_load(&self, context: Context) -> Result<(), String> {
        let state = Arc::clone(&self.state);
        *lock(&state.folder) = PathBuf::from(context.get_data_folder());
        state.read_timezone();
        let config = lock(&state.folder).join("config.yml");
        if !config.exists() {
            let _ = fs::write(&config, CONFIG_DEFAULTS);
        }
        state.sweep();
        *lock(&state.map) = Some(map::Map::new(&lock(&state.folder)));

        let live = Arc::clone(&state);
        context.schedule_repeating_task(LIVE_TICKS, LIVE_TICKS, move |server| {
            let live = Arc::clone(&live);
            async move { live.snapshot(&server).await }
        });
        let perf = Arc::clone(&state);
        context.schedule_repeating_task(PERF_TICKS, PERF_TICKS, move |server| {
            perf.perf(&server);
            std::future::ready(())
        });
        let map = Arc::clone(&state);
        context.schedule_repeating_task(MAP_TICKS, MAP_TICKS, move |server| {
            map.draw(&server);
            std::future::ready(())
        });

        *state.studio.lock().await = Some(studio::Studio::new(&lock(&state.folder), VERSION));
        if let Some(studio) = state.studio.lock().await.as_mut() {
            studio.main_world_generators(&context);
            // Blocks can only be registered now, while plugins load.
            studio.poll(state.on("studio"), std::collections::BTreeSet::new());
            studio.register_blocks(&BlockCalls(Arc::clone(&state)));
            studio.register_entries(&context.get_server());
        }
        let looks = Arc::clone(&state);
        context.schedule_repeating_task(studio::TICK_EVERY, studio::TICK_EVERY, move |server| {
            let looks = Arc::clone(&looks);
            async move { looks.studio_tick(&server).await }
        });
        // M4: model frames every tick, apart from the slower studio tick.
        let models = Arc::clone(&state);
        context.schedule_repeating_task(1, 1, move |server| {
            let models = Arc::clone(&models);
            async move {
                if models.frame_waiting.swap(true, Ordering::AcqRel) {
                    return;
                }
                let mut held = models.studio.lock().await;
                models.frame_waiting.store(false, Ordering::Release);
                if let Some(studio) = held.as_mut() {
                    studio.tick_models(&server).await;
                    studio.tick_digging(&server).await;
                    studio.tick_cosmetics(&server).await;
                }
            }
        });
        // Blocking, so the handlers can change the message, MOTD or chat line.
        context.register_event_handler::<PlayerJoinEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerLeaveEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerChatEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<ServerListPingEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        // The interactive kinds: item use, NPC clicks, menus, rules, loot, dialogs, advancements.
        context.register_event_handler::<PlayerInteractEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerInteractEntityEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerPickBlockEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerPickEntityEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<EntityDamageEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<InventoryClickEvent, _>(Looks(Arc::clone(&state)), EventPriority::Highest, true)?;
        context.register_event_handler::<InventoryCloseEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<PlayerDeathEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<PlayerRespawnEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<BlockBreakEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, true)?;
        context.register_event_handler::<BlockPlaceEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, true)?;
        context.register_event_handler::<BlockDropItemEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerChangeWorldEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<PlayerCommandPreprocessEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, true)?;
        context.register_event_handler::<DialogClickActionEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerAdvancementDoneEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<PlayerItemConsumeEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        // Rule events added with the node editor (blocking where a rule's `cancel` step can call them off).
        context.register_event_handler::<PlayerToggleSneakEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<PlayerDropItemEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerLevelChangeEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<PlayerTeleportEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerGamemodeChangeEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        // M3: our borrowed block states never change, and digging ours takes our time.
        context.register_event_handler::<BlockPhysicsEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<NotePlayEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<BlockPistonExtendEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<BlockPistonRetractEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<BlockDamageEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<BlockDamageAbortEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        // M5: studio mobs.
        // Last and only when it goes ahead (Paper's MONITOR + ignoreCancelled).
        context.register_event_handler_ignoring_cancelled::<EntityDamageByEntityEvent, _>(Looks(Arc::clone(&state)), EventPriority::Highest)?;
        context.register_event_handler::<EntityDeathEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<EntityDismountEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        // Studio loot from chests and fishing.
        context.register_event_handler::<LootGenerateEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<PlayerFishEvent, _>(Looks(Arc::clone(&state)), EventPriority::Highest, true)?;
        context.register_event_handler::<EntityTargetEvent, _>(Looks(Arc::clone(&state)), EventPriority::Lowest, false)?;
        context.register_event_handler::<EntityCombustEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        context.register_event_handler::<EntityPickupItemEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        // Quests: crafting counts.
        context.register_event_handler::<CraftItemEvent, _>(Looks(Arc::clone(&state)), EventPriority::High, true)?;
        // Studio commands that exist now; ones made later are registered as they appear (`studio_tick`).
        let on = state.on("studio");
        let first = match state.studio.lock().await.as_mut() {
            Some(studio) => {
                studio.poll(on, std::collections::BTreeSet::new());
                studio.command_changes().1
            }
            None => Vec::new(),
        };
        // Pumpkin puts a permission on every plugin command (an empty one is "monolab:", which nobody but the
        // console has). Ours is granted to everyone; a studio command's own "permission" is checked when it runs.
        if let Err(problem) = context.register_permission(&pumpkin_plugin_api::permission::Permission {
            node: STUDIO_COMMAND_PERMISSION.into(),
            description: "Use the studio's commands (each can ask for its own permission too).".into(),
            default: pumpkin_plugin_api::permission::PermissionDefault::Allow,
            children: Vec::new(),
        }) {
            tracing::warn!("Couldn't register the studio command permission: {problem}");
        }
        for one in first {
            context.register_command(state.command(one), STUDIO_COMMAND_PERMISSION);
        }
        *lock(&state.context) = Some(context);

        tracing::info!("Starting the Monolab monitor: {}", lock(&state.folder).display());
        Ok(())
    }

    async fn on_unload(&self, context: Context) -> Result<(), String> {
        self.state.set_live(json!({ "at": now_ms(), "players": [] }));
        if let Some(studio) = self.state.studio.lock().await.as_mut() {
            let server = context.get_server();
            studio.stop(&server, &players_of(&server)).await;
        }
        if let Some(map) = lock(&self.state.map).as_mut() {
            let _ = map.flush();
        }
        Ok(())
    }
}

register_plugin!(Monolab);

/// The studio's event handlers.
struct Looks(Arc<State>);

impl Looks {
    /// Runs with the studio unless it is busy (a handler called back during its own tick) or not set up.
    async fn with<T>(&self, work: impl AsyncFnOnce(&mut studio::Studio) -> Option<T>) -> Option<T> {
        let mut held = self.0.studio.lock().await;
        work(held.as_mut()?).await
    }
}

impl EventHandler<PlayerJoinEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<PlayerJoinEvent>) -> EventData<PlayerJoinEvent> {
        let who = who_of(&event.player);
        let mut held = self.0.studio.lock().await;
        let Some(studio) = held.as_mut() else { return event };
        match studio.joined(&server, &event.player, &who) {
            Some(Some(text)) => event.join_message = text,
            Some(None) => event.cancelled = true,
            None => {}
        }
        event
    }
}

impl EventHandler<PlayerLeaveEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<PlayerLeaveEvent>) -> EventData<PlayerLeaveEvent> {
        let who = who_of(&event.player);
        let mut held = self.0.studio.lock().await;
        let Some(studio) = held.as_mut() else { return event };
        match studio.left(&server, &event.player, &who).await {
            Some(Some(text)) => event.leave_message = text,
            Some(None) => event.cancelled = true,
            None => {}
        }
        event
    }
}

impl EventHandler<PlayerChatEvent> for Looks {
    /// Our format: the server's line is cancelled and ours goes to the same people as a system message.
    async fn handle(&self, server: Server, mut event: EventData<PlayerChatEvent>) -> EventData<PlayerChatEvent> {
        if event.cancelled {
            return event;
        }
        let who = who_of(&event.player);
        let message = event.message.clone();
        self.with(async |studio| {
            studio.fire(&server, "chat", Some(&event.player), vec![("message".into(), message.clone())], |when| {
                let contains = studio_format::fields::text(when, "contains", "");
                contains.trim().is_empty() || message.to_lowercase().contains(&contains.to_lowercase())
            }).await;
            Some(())
        }).await;
        if let Some(line) = self.with(async |studio| studio.chat(&server, &event.player, &who, &event.message)).await {
            let json = line.to_json();
            for one in &event.recipients {
                if let Ok(copy) = pumpkin_plugin_api::text::TextComponent::from_json(&json) {
                    one.send_system_message(copy, false);
                }
            }
            tracing::info!("<{}> {}", who.name, event.message);
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<PlayerInteractEvent> for Looks {
    /// A right click with a studio item runs its use actions.
    async fn handle(&self, server: Server, mut event: EventData<PlayerInteractEvent>) -> EventData<PlayerInteractEvent> {
        // A right click comes once per hand (main first): ours read the main hand, like Paper's `getHand() == HAND`.
        if !matches!(event.hand, pumpkin_plugin_api::common::Hand::Right) {
            return event;
        }
        // M3 first: placing a studio block or piece, or using one of ours.
        let block = matches!(event.action, InteractAction::RightClickBlock);
        if (block && self.with(async |studio| Some(studio.block_clicked(&server, &event.player).await)).await == Some(true))
            || ((block || matches!(event.action, InteractAction::RightClickAir)) && self.with(async |studio| Some(studio.used_item(&server, &event.player).await)).await == Some(true))
        {
            event.cancelled = true;
        }
        // The `click` rule event.
        let button = if matches!(event.action, InteractAction::RightClickAir | InteractAction::RightClickBlock) { "right" } else { "left" };
        let extra = match event.clicked_pos {
            Some(at) => vec![("button".into(), button.to_string()), ("block".into(), block_name(&event.block)), ("x".into(), at.x.to_string()), ("y".into(), at.y.to_string()), ("z".into(), at.z.to_string())],
            None => vec![("button".into(), button.to_string()), ("block".into(), "air".into()), ("x".into(), String::new()), ("y".into(), String::new()), ("z".into(), String::new())],
        };
        if self.cancellable(&server, "click", &event.player, extra).await {
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<PlayerInteractEntityEvent> for Looks {
    /// Right-clicking one of our NPCs runs its actions instead of trading or riding; hitting one does nothing.
    async fn handle(&self, server: Server, mut event: EventData<PlayerInteractEntityEvent>) -> EventData<PlayerInteractEntityEvent> {
        let attack = matches!(event.action, pumpkin_plugin_api::events_wit::EntityInteractionAction::Attack);
        if self.with(async |studio| Some(studio.piece_clicked(&server, &event.player, event.entity_id, attack).await)).await == Some(true) {
            event.cancelled = true;
            return event;
        }
        let ours = if attack {
            self.with(async |studio| Some(studio.is_npc(event.entity_id))).await
        } else {
            self.with(async |studio| Some(studio.npc_clicked(&server, &event.player, event.entity_id).await || studio.mob_clicked(&server, &event.player, event.entity_id).await)).await
        };
        if ours == Some(true) {
            event.cancelled = true;
        } else if !attack && !event.cancelled {
            let entity = self.with(async |studio| Some(studio.entity_name(&server, event.entity_id))).await.unwrap_or_default();
            let name = entity.trim_start_matches("studio:").to_string();
            if self.cancellable(&server, "click_entity", &event.player, vec![("entity".into(), entity), ("entity_name".into(), name)]).await {
                event.cancelled = true;
            }
        }
        event
    }
}

impl EventHandler<PlayerPickBlockEvent> for Looks {
    /// A middle click on one of our blocks picks its own item, not the paper base or the borrowed block.
    async fn handle(&self, server: Server, mut event: EventData<PlayerPickBlockEvent>) -> EventData<PlayerPickBlockEvent> {
        if let Some(item) = self.with(async |studio| studio.picked(&server, &event.player, Some(event.state), None).await).await {
            event.item = item;
        }
        event
    }
}

impl EventHandler<PlayerPickEntityEvent> for Looks {
    /// A middle click on a furniture piece picks its item (the hitbox has no spawn egg).
    async fn handle(&self, server: Server, mut event: EventData<PlayerPickEntityEvent>) -> EventData<PlayerPickEntityEvent> {
        if let Some(item) = self.with(async |studio| studio.picked(&server, &event.player, None, Some(event.entity_id)).await).await {
            event.item = item;
        }
        event
    }
}

impl EventHandler<EntityDamageEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<EntityDamageEvent>) -> EventData<EntityDamageEvent> {
        if self.with(async |studio| Some(studio.is_npc(event.entity_id))).await == Some(true) {
            event.cancelled = true;
        } else {
            self.with(async |studio| {
                studio.mob_hurt(event.entity_id);
                if !event.cancelled {
                    studio.player_hurt(&server, event.entity_id).await;
                }
                Some(())
            }).await;
            if !event.cancelled
                && let Some(player) = server.get_all_players().into_iter().find(|one| one.as_entity().get_id() as i64 == i64::from(event.entity_id))
            {
                let cause = map::biome_name(&format!("{:?}", event.damage_type)).trim_start_matches("minecraft:").to_string();
                let extra = vec![("cause".into(), cause), ("amount".into(), amount(event.damage)), ("attacker".into(), String::new())];
                if self.cancellable(&server, "hurt", &player, extra).await {
                    event.cancelled = true;
                }
            }
        }
        event
    }
}

impl EventHandler<EntityDamageByEntityEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<EntityDamageByEntityEvent>) -> EventData<EntityDamageByEntityEvent> {
        // Skipped while cancelled (registered with `ignore-cancelled`, Paper's `ignoreCancelled`).
        // The `hit` rule event: a player hitting something themselves.
        if let Some(player) = server.get_all_players().into_iter().find(|one| one.as_entity().get_id() as i64 == i64::from(event.damager_id)) {
            let entity = self.with(async |studio| Some(studio.entity_name(&server, event.entity_id))).await.unwrap_or_default();
            if self.cancellable(&server, "hit", &player, vec![("entity".into(), entity), ("amount".into(), amount(event.damage))]).await {
                event.cancelled = true;
                return event;
            }
        }
        // The one responsible: an arrow's shooter, else whoever hit.
        let by = event.causing_entity_id.unwrap_or(event.damager_id);
        self.with(async |studio| {
            studio.mob_hit(&server, event.entity_id, by).await;
            studio.hit_with_item(&server, event.entity_id, event.damager_id).await;
            Some(())
        }).await;
        event
    }
}

impl EventHandler<EntityDeathEvent> for Looks {
    /// A studio mob's death skills, and its drops and experience (the event's own list, like Paper).
    async fn handle(&self, server: Server, mut event: EventData<EntityDeathEvent>) -> EventData<EntityDeathEvent> {
        let mut drops = std::mem::take(&mut event.drops);
        let mut exp = event.dropped_exp;
        self.with(async |studio| {
            if studio.has_rule("kill")
                && let Some(killer) = studio.killer_of(&server, event.entity_id)
            {
                let entity = studio.entity_name(&server, event.entity_id);
                studio.fire_cancellable(&server, "kill", Some(&killer), vec![("entity".into(), entity)]).await;
            }
            studio.quest_kill(&server, event.entity_id).await;
            studio.mob_loot(&server, event.entity_id, &mut drops).await;
            studio.mob_died(&server, event.entity_id, &mut drops, &mut exp).await;
            Some(())
        }).await;
        event.drops = drops;
        event.dropped_exp = exp;
        event
    }
}

impl EventHandler<LootGenerateEvent> for Looks {
    /// Studio loot tables that drop from chests add to (or replace) what a vanilla chest table rolled.
    async fn handle(&self, server: Server, mut event: EventData<LootGenerateEvent>) -> EventData<LootGenerateEvent> {
        if event.cancelled {
            return event;
        }
        let mut items = std::mem::take(&mut event.items);
        let table = event.loot_table.clone();
        self.with(async |studio| {
            studio.chest_loot(&server, &table, &mut items).await;
            Some(())
        }).await;
        event.items = items;
        event
    }
}

impl EventHandler<PlayerFishEvent> for Looks {
    /// A catch: studio fishing tables give theirs; one that replaces the catch takes the caught item away.
    async fn handle(&self, server: Server, event: EventData<PlayerFishEvent>) -> EventData<PlayerFishEvent> {
        if event.cancelled || !matches!(event.state, pumpkin_plugin_api::wit::pumpkin::plugin::event::PlayerFishState::CaughtFish) {
            return event;
        }
        let caught = match event.caught_uuid.and_then(|caught| event.player.as_entity().get_world().get_entity_by_uuid(caught)).and_then(|item| item.get_item_stack()) {
            Some(stack) => stack.get_registry_key().await,
            None => String::new(),
        };
        let mut event = event;
        if self.cancellable(&server, "fish", &event.player, vec![("item".into(), block_name(&caught)), ("amount".into(), "1".into())]).await {
            event.cancelled = true;
            return event;
        }
        self.with(async |studio| {
            studio.quest_event(&server, &event.player, "fish", &caught, 1).await;
            Some(())
        }).await;
        let replace = self.with(async |studio| Some(studio.fished(&server, &event.player).await)).await == Some(true);
        if replace
            && let Some(caught) = event.caught_uuid
            && let Some(item) = event.player.as_entity().get_world().get_entity_by_uuid(caught)
        {
            item.remove();
        }
        event
    }
}

impl EventHandler<EntityDismountEvent> for Looks {
    /// Standing up from a furniture seat takes the seat away (Paper's `onStand`).
    async fn handle(&self, _server: Server, event: EventData<EntityDismountEvent>) -> EventData<EntityDismountEvent> {
        if !event.cancelled {
            self.with(async |studio| {
                studio.stood_up(event.entity_id, event.dismounted_id);
                Some(())
            }).await;
        }
        event
    }
}

impl EventHandler<EntityTargetEvent> for Looks {
    async fn handle(&self, server: Server, event: EventData<EntityTargetEvent>) -> EventData<EntityTargetEvent> {
        if !event.cancelled {
            self.with(async |studio| {
                studio.mob_targeted(&server, event.entity_id, event.target_id).await;
                Some(())
            }).await;
        }
        event
    }
}

impl EventHandler<EntityCombustEvent> for Looks {
    async fn handle(&self, _server: Server, mut event: EventData<EntityCombustEvent>) -> EventData<EntityCombustEvent> {
        if self.with(async |studio| Some(studio.mob_fireproof(event.entity_id, &event.cause))).await == Some(true) {
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<CraftItemEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<CraftItemEvent>) -> EventData<CraftItemEvent> {
        let item = event.recipe_id.rsplit(['/', ':']).next().unwrap_or_default().to_string();
        if !event.cancelled && self.cancellable(&server, "craft", &event.player, vec![("item".into(), item), ("amount".into(), "1".into())]).await {
            event.cancelled = true;
        }
        if !event.cancelled {
            let recipe = event.recipe_id.clone();
            self.with(async |studio| {
                studio.quest_event(&server, &event.player, "craft", recipe.rsplit('/').next().unwrap_or(&recipe), 1).await;
                Some(())
            }).await;
        }
        event
    }
}

impl EventHandler<EntityPickupItemEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<EntityPickupItemEvent>) -> EventData<EntityPickupItemEvent> {
        if self.with(async |studio| Some(studio.mob_no_pickup(event.entity_id))).await == Some(true) {
            event.cancelled = true;
        } else if !event.cancelled
            && let Some(player) = server.get_all_players().into_iter().find(|one| one.as_entity().get_id() as i64 == i64::from(event.entity_id))
        {
            let item = event.item_name.clone();
            let count = u32::from(event.count);
            if self.cancellable(&server, "pickup", &player, vec![("item".into(), block_name(&item)), ("amount".into(), count.to_string())]).await {
                event.cancelled = true;
                return event;
            }
            self.with(async |studio| {
                studio.quest_event(&server, &player, "collect", &item, count).await;
                Some(())
            }).await;
        }
        event
    }
}

impl EventHandler<InventoryClickEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<InventoryClickEvent>) -> EventData<InventoryClickEvent> {
        if self.with(async |studio| Some(studio.menu_click(&server, &event.player, i32::from(event.raw_slot), event.click_type).await)).await == Some(true) {
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<InventoryCloseEvent> for Looks {
    async fn handle(&self, _server: Server, event: EventData<InventoryCloseEvent>) -> EventData<InventoryCloseEvent> {
        self.with(async |studio| {
            studio.menu_closed(&event.player);
            Some(())
        }).await;
        event
    }
}

impl EventHandler<PlayerDeathEvent> for Looks {
    async fn handle(&self, server: Server, event: EventData<PlayerDeathEvent>) -> EventData<PlayerDeathEvent> {
        self.with(async |studio| {
            studio.fire(&server, "death", Some(&event.player), vec![("killer".into(), String::new())], |_| true).await;
            Some(())
        }).await;
        event
    }
}

impl EventHandler<PlayerRespawnEvent> for Looks {
    async fn handle(&self, _server: Server, event: EventData<PlayerRespawnEvent>) -> EventData<PlayerRespawnEvent> {
        self.with(async |studio| {
            studio.fire_later("respawn", &event.player);
            Some(())
        }).await;
        event
    }
}

impl EventHandler<PlayerItemConsumeEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<PlayerItemConsumeEvent>) -> EventData<PlayerItemConsumeEvent> {
        if !event.cancelled && self.cancellable(&server, "eat", &event.player, vec![("item".into(), block_name(&event.item_name)), ("amount".into(), "1".into())]).await {
            event.cancelled = true;
        }
        if !event.cancelled {
            self.with(async |studio| {
                studio.ate_item(&server, &event.player).await;
                Some(())
            }).await;
        }
        event
    }
}

fn block_name(block: &str) -> String {
    block.trim_start_matches("minecraft:").split('[').next().unwrap_or_default().to_string()
}

impl EventHandler<BlockBreakEvent> for Looks {
    async fn handle(&self, server: Server, event: EventData<BlockBreakEvent>) -> EventData<BlockBreakEvent> {
        if event.cancelled {
            return event;
        }
        let at = (event.block_pos.x, event.block_pos.y, event.block_pos.z);
        let block = event.block.clone();
        if let Some(player) = &event.player
            && self.cancellable(&server, "break", player, spot_extra(&block, at)).await
        {
            let mut event = event;
            event.cancelled = true;
            return event;
        }
        self.with(async |studio| {
            studio.block_broken(at, &block);
            studio.block_breaking(&server, event.player.as_ref(), at).await;
            if let Some(player) = &event.player {
                studio.broke_with_item(&server, player, &block).await;
                studio.quest_event(&server, player, "break", &block_name(&block), 1).await;
            }
            Some(())
        }).await;
        event
    }
}

impl EventHandler<BlockPlaceEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<BlockPlaceEvent>) -> EventData<BlockPlaceEvent> {
        if event.cancelled {
            return event;
        }
        let block = event.block_placed.clone();
        let at = (event.block_pos.x, event.block_pos.y, event.block_pos.z);
        if self.cancellable(&server, "place", &event.player, spot_extra(&block, at)).await {
            event.cancelled = true;
            return event;
        }
        self.with(async |studio| {
            studio.block_placed(&event.player, at, &block).await;
            studio.quest_event(&server, &event.player, "place", &block_name(&block), 1).await;
            Some(())
        }).await;
        event
    }
}

impl EventHandler<BlockDropItemEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<BlockDropItemEvent>) -> EventData<BlockDropItemEvent> {
        let at = (event.block_pos.x, event.block_pos.y, event.block_pos.z);
        let mut items = std::mem::take(&mut event.items);
        self.with(async |studio| {
            studio.studio_block_drops(&server, at, event.player.as_ref(), &mut items).await;
            studio.block_drops(&server, at, event.player.as_ref(), &mut items).await;
            Some(())
        }).await;
        event.items = items;
        event
    }
}

/// Cancels a block event at one of our studio blocks (they never change).
macro_rules! hold_studio_blocks {
    ($event:ty) => {
        impl EventHandler<$event> for Looks {
            async fn handle(&self, _server: Server, mut event: EventData<$event>) -> EventData<$event> {
                let at = (event.block_pos.x, event.block_pos.y, event.block_pos.z);
                if !event.cancelled && self.with(async |studio| Some(studio.block_held(&event.target_world, at))).await == Some(true) {
                    event.cancelled = true;
                }
                event
            }
        }
    };
}
impl EventHandler<BlockPhysicsEvent> for Looks {
    async fn handle(&self, _server: Server, mut event: EventData<BlockPhysicsEvent>) -> EventData<BlockPhysicsEvent> {
        let at = (event.block_pos.x, event.block_pos.y, event.block_pos.z);
        if !event.cancelled && self.with(async |studio| Some(studio.block_held(&event.target_world, at) || studio.plain_note_held(&event.target_world, at).await)).await == Some(true) {
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<NotePlayEvent> for Looks {
    /// Ours stay quiet; a plain note block plays the note we keep for it (its world state is always note 0).
    async fn handle(&self, _server: Server, mut event: EventData<NotePlayEvent>) -> EventData<NotePlayEvent> {
        let at = (event.block_pos.x, event.block_pos.y, event.block_pos.z);
        if !event.cancelled && self.with(async |studio| Some(studio.note_played(&event.target_world, at).await)).await == Some(true) {
            event.cancelled = true;
        }
        event
    }
}
hold_studio_blocks!(BlockPistonExtendEvent);
hold_studio_blocks!(BlockPistonRetractEvent);

impl EventHandler<BlockDamageEvent> for Looks {
    async fn handle(&self, _server: Server, event: EventData<BlockDamageEvent>) -> EventData<BlockDamageEvent> {
        let at = (event.block_pos.x, event.block_pos.y, event.block_pos.z);
        self.with(async |studio| {
            studio.dig_started(&event.player, at).await;
            Some(())
        }).await;
        event
    }
}

impl EventHandler<BlockDamageAbortEvent> for Looks {
    async fn handle(&self, _server: Server, event: EventData<BlockDamageAbortEvent>) -> EventData<BlockDamageAbortEvent> {
        self.with(async |studio| {
            studio.dig_stopped(&event.player);
            Some(())
        }).await;
        event
    }
}

impl EventHandler<PlayerChangeWorldEvent> for Looks {
    async fn handle(&self, server: Server, event: EventData<PlayerChangeWorldEvent>) -> EventData<PlayerChangeWorldEvent> {
        if event.cancelled {
            return event;
        }
        let world = world_name(&event.new_world.get_id(), &event.new_world.get_dimension());
        self.with(async |studio| {
            studio.fire(&server, "world_change", Some(&event.player), vec![("world".into(), world.clone())], |when| {
                let to = studio_format::fields::text(when, "world", "");
                to.trim().is_empty() || to == world
            }).await;
            Some(())
        }).await;
        event
    }
}

impl EventHandler<PlayerCommandPreprocessEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<PlayerCommandPreprocessEvent>) -> EventData<PlayerCommandPreprocessEvent> {
        if event.cancelled {
            return event;
        }
        let line = event.command.trim_start_matches('/').to_string();
        if self.cancellable(&server, "command", &event.player, vec![("command".into(), line)]).await {
            event.cancelled = true;
        }
        event
    }
}

impl Looks {
    /// Fires a rule event a `cancel` step may call off (skipped quickly when no rule listens): true when one did.
    async fn cancellable(&self, server: &Server, event: &str, player: &Player, extra: Vec<(String, String)>) -> bool {
        self.with(async |studio| {
            if !studio.has_rule(event) {
                return Some(false);
            }
            Some(studio.fire_cancellable(server, event, Some(player), extra).await)
        }).await == Some(true)
    }
}

/// A block event's placeholders: the block and where it is.
fn spot_extra(block: &str, at: (i32, i32, i32)) -> Vec<(String, String)> {
    vec![("block".into(), block_name(block)), ("x".into(), at.0.to_string()), ("y".into(), at.1.to_string()), ("z".into(), at.2.to_string())]
}

/// A game mode as rules write it.
pub(crate) fn game_mode_name(mode: pumpkin_plugin_api::common::GameMode) -> &'static str {
    use pumpkin_plugin_api::common::GameMode;
    match mode {
        GameMode::Survival => "survival",
        GameMode::Creative => "creative",
        GameMode::Adventure => "adventure",
        GameMode::Spectator => "spectator",
    }
}

/// Damage as a rule shows it: `4` or `2.5`.
fn amount(value: f32) -> String {
    if value.fract() == 0.0 { format!("{}", value as i64) } else { format!("{value:.1}") }
}

impl EventHandler<PlayerToggleSneakEvent> for Looks {
    async fn handle(&self, server: Server, event: EventData<PlayerToggleSneakEvent>) -> EventData<PlayerToggleSneakEvent> {
        if !event.cancelled && event.is_sneaking {
            self.cancellable(&server, "sneak", &event.player, Vec::new()).await;
        }
        event
    }
}

impl EventHandler<PlayerDropItemEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<PlayerDropItemEvent>) -> EventData<PlayerDropItemEvent> {
        let extra = vec![("item".into(), block_name(&event.item_name)), ("amount".into(), event.count.to_string())];
        if !event.cancelled && self.cancellable(&server, "drop", &event.player, extra).await {
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<PlayerLevelChangeEvent> for Looks {
    async fn handle(&self, server: Server, event: EventData<PlayerLevelChangeEvent>) -> EventData<PlayerLevelChangeEvent> {
        if event.new_level > event.old_level {
            self.cancellable(&server, "level_up", &event.player, vec![("level".into(), event.new_level.to_string())]).await;
        }
        event
    }
}

impl EventHandler<PlayerTeleportEvent> for Looks {
    /// Moves by our own steps don't count, so a rule that teleports can't set itself off.
    async fn handle(&self, server: Server, mut event: EventData<PlayerTeleportEvent>) -> EventData<PlayerTeleportEvent> {
        if event.cancelled || self.with(async |studio| Some(studio.moved_by_step(&event.player))).await != Some(false) {
            return event;
        }
        let (x, y, z) = event.to_position;
        let world = who_of(&event.player).world;
        let extra = vec![("world".into(), world), ("x".into(), (x.floor() as i64).to_string()), ("y".into(), (y.floor() as i64).to_string()), ("z".into(), (z.floor() as i64).to_string())];
        if self.cancellable(&server, "teleport", &event.player, extra).await {
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<PlayerGamemodeChangeEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<PlayerGamemodeChangeEvent>) -> EventData<PlayerGamemodeChangeEvent> {
        let mode = game_mode_name(event.new_gamemode).to_string();
        if !event.cancelled && self.cancellable(&server, "gamemode", &event.player, vec![("gamemode".into(), mode)]).await {
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<DialogClickActionEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<DialogClickActionEvent>) -> EventData<DialogClickActionEvent> {
        if self.with(async |studio| Some(studio.dialog_clicked(&server, &event.player, &event.id, event.payload_nbt.as_ref()).await)).await == Some(true) {
            event.cancelled = true;
        }
        event
    }
}

impl EventHandler<PlayerAdvancementDoneEvent> for Looks {
    async fn handle(&self, server: Server, event: EventData<PlayerAdvancementDoneEvent>) -> EventData<PlayerAdvancementDoneEvent> {
        self.with(async |studio| {
            studio.advancement_done(&server, &event.player, &event.advancement_id).await;
            Some(())
        }).await;
        event
    }
}

impl EventHandler<ServerListPingEvent> for Looks {
    async fn handle(&self, server: Server, mut event: EventData<ServerListPingEvent>) -> EventData<ServerListPingEvent> {
        if let Some(motd) = self.with(async |studio| studio.motd(&server)).await {
            event.motd = motd;
        }
        event
    }
}

/// Answers our fork blocks' callbacks: their drops are the studio's (own item, linked item, loot roll), like the
/// borrowed blocks' in `BlockDropItemEvent`; crops grow on random ticks and bone meal.
#[derive(Clone)]
struct BlockCalls(Arc<State>);

impl pumpkin_plugin_api::custom_block::CustomBlockHandler for BlockCalls {
    async fn handle(&self, server: Server, _block: &str, call: pumpkin_plugin_api::custom_block::BlockCall) -> pumpkin_plugin_api::custom_block::BlockCallResult {
        use pumpkin_plugin_api::custom_block::{BlockCall, BlockCallResult};
        let mut held = self.0.studio.lock().await;
        let Some(studio) = held.as_mut() else { return BlockCallResult::None };
        match call {
            BlockCall::Drops(context) => {
                let at = (context.pos.x, context.pos.y, context.pos.z);
                let mut items = context.drops;
                studio.block_dropping(at, context.state);
                studio.studio_block_drops(&server, at, None, &mut items).await;
                BlockCallResult::Drops(items)
            }
            BlockCall::OnRandomTick(context) => {
                studio.crop_tick(&context.world, (context.pos.x, context.pos.y, context.pos.z), context.state).await;
                BlockCallResult::None
            }
            BlockCall::CanBonemeal(context) => BlockCallResult::Allowed(studio.crop_can_bonemeal(context.state)),
            BlockCall::OnBonemeal(context) => {
                studio.crop_bonemeal(&context.world, (context.pos.x, context.pos.y, context.pos.z), context.state).await;
                BlockCallResult::None
            }
            _ => BlockCallResult::None,
        }
    }
}

fn who_of(player: &Player) -> studio::Who {
    let world = player.get_world();
    studio::Who::of(player, world_name(&world.get_id(), &world.get_dimension()))
}

fn players_of(server: &Server) -> Vec<(Player, studio::Who)> {
    server
        .get_all_players()
        .into_iter()
        .map(|player| {
            let who = who_of(&player);
            (player, who)
        })
        .collect()
}

impl State {
    async fn snapshot(&self, server: &Server) {
        let count = self.snapshots.fetch_add(1, Ordering::Relaxed);
        if self.commands_dirty.swap(false, Ordering::Relaxed) && self.on("commands") {
            self.write_file("commands.json", &commands_json(server));
        }
        if count.is_multiple_of(FILES_EVERY) {
            if self.on("loaded-chunks") {
                self.write_file("loaded.json", &loaded_json(server));
            }
            if self.on("inventories") {
                self.write_file("inventories.json", &inventories_json(server).await);
            }
        }
        self.live(server);
    }

    /// A whole file in the data folder, replaced at once (written beside it, then renamed).
    fn write_file(&self, name: &str, value: &Value) {
        let folder = lock(&self.folder).clone();
        let temporary = folder.join(format!("{name}.tmp"));
        if fs::write(&temporary, value.to_string()).and_then(|()| fs::rename(&temporary, folder.join(name))).is_err() {
            self.warn_once(&format!("Couldn't write {name}"));
        }
    }

    fn live(&self, server: &Server) {
        let people: Vec<Value> = server
            .get_all_players()
            .iter()
            .map(|player| {
                let mut one = json!({ "name": player.get_name() });
                put_where(&mut one, player);
                put(&mut one, "health", json!(player.get_health().round() as i64));
                put(&mut one, "food", json!(player.get_food_level()));
                put(&mut one, "ping", json!(player.get_ping()));
                one
            })
            .collect();
        let mut live = json!({
            "at": now_ms(),
            "tps": tps_of(server),
            "mspt": hundredths(server.get_mspt()),
            "chunks": loaded_chunks(server),
            "players": people,
        });
        if let (Value::Object(map), Value::Object(tally)) = (&mut live, entity_tally(server)) {
            map.extend(tally);
        }
        self.set_live(live);
    }

    fn draw(&self, server: &Server) {
        if !self.on("map") || !self.on("live-map") {
            return;
        }
        let mut held = lock(&self.map);
        let Some(map) = held.as_mut() else {
            return;
        };
        if let Err(error) = map.tick(server, now_ms())
            && !self.map_warned.swap(true, Ordering::Relaxed)
        {
            tracing::warn!("{error}");
        }
    }

    fn perf(&self, server: &Server) {
        if !self.on("performance-log") {
            return;
        }
        self.add(json!({
            "at": now_ms(),
            "kind": "perf",
            "tps": tps_of(server),
            "mspt": hundredths(server.get_mspt()),
            "players": server.get_player_count(),
            "chunks": loaded_chunks(server),
            "entities": server.get_all_worlds().iter().map(|world| u64::from(world.get_entity_count())).sum::<u64>(),
        }));
    }
}

impl State {
    async fn studio_tick(self: &Arc<Self>, server: &Server) {
        let on = self.on("studio");
        let off = studio_format::KINDS.iter().filter(|kind| !self.on(&format!("studio-{kind}"))).map(|kind| kind.to_string()).collect();
        let players = players_of(server);
        let (fresh, traces) = {
            let mut held = self.studio.lock().await;
            let Some(studio) = held.as_mut() else { return };
            // The app asks for a reread now by leaving `studio/live/reload` (Pumpkin plugins can't open sockets).
            let ask = lock(&self.folder).join("studio").join("live").join("reload");
            if fs::remove_file(&ask).is_ok() {
                studio.poll(on, off);
                studio.asked_reload();
            } else {
                studio.poll_every(on, off);
            }
            studio.tick(server, &players).await;
            (studio.command_changes(), studio.take_traces())
        };
        if !traces.is_empty() {
            self.add_traces(&traces);
        }
        // Outside the studio lock: registering may call back into the plugin.
        let (gone, fresh) = fresh;
        if !gone.is_empty() || !fresh.is_empty() {
            let held = lock(&self.context);
            if let Some(context) = held.as_ref() {
                for name in gone {
                    context.unregister_command(&name);
                }
                for one in fresh {
                    context.register_command(self.command(one), STUDIO_COMMAND_PERMISSION);
                }
            }
            drop(held);
            // Players see the new list at once (Paper's `updateCommands`), and the app's command list follows.
            for player in server.get_all_players() {
                player.update_commands();
            }
            self.commands_dirty.store(true, Ordering::Relaxed);
        }
    }

    /// A Pumpkin command for studio command names; it runs whatever the studio file says at the time.
    fn command(self: &Arc<Self>, one: studio::play::NewCommand) -> pumpkin_plugin_api::command::Command {
        let state = Arc::clone(self);
        studio::play::command(one.names, &one.description, Arc::new(move |name, sender, server, args| {
            let state = Arc::clone(&state);
            Box::pin(async move {
                let mut held = state.studio.lock().await;
                let Some(studio) = held.as_mut() else { return };
                if let Err(problem) = studio.run_command(&server, &name, &sender, args.as_deref().unwrap_or("")).await {
                    sender.send_error(pumpkin_plugin_api::text::TextComponent::text(&problem));
                }
            })
        }))
    }

    /// Rule traces for the app, as lines in `studio/live/out.jsonl` (kept short: the newest 200).
    fn add_traces(&self, lines: &[String]) {
        let folder = lock(&self.folder).join("studio").join("live");
        let _ = fs::create_dir_all(&folder);
        let path = folder.join("out.jsonl");
        let mut all: Vec<String> = fs::read_to_string(&path).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default();
        all.extend(lines.iter().cloned());
        let keep = all.len().saturating_sub(200);
        let text: String = all[keep..].iter().map(|one| format!("{one}\n")).collect();
        let temporary = folder.join("out.jsonl.tmp");
        if fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, &path)).is_err() {
            self.warn_once("Couldn't write studio/live/out.jsonl");
        }
    }


    /// A config.yml switch (missing = on), reread at most every 5 seconds.
    fn on(&self, key: &str) -> bool {
        let now = now_ms();
        let mut held = lock(&self.config);
        if now.saturating_sub(held.0) > 5_000 {
            held.0 = now;
            held.1 = fs::read_to_string(lock(&self.folder).join("config.yml"))
                .map(|text| read_switches(&text))
                .unwrap_or_default();
        }
        held.1.get(key).copied().unwrap_or(true)
    }

    fn add(&self, line: Value) {
        let day = self.today();
        self.sweep_if_new_day(&day);
        let path = lock(&self.folder).join(format!("events-{day}.jsonl"));
        let written = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| {
                let mut text = line.to_string();
                text.push('\n');
                file.write_all(text.as_bytes())
            });
        if let Err(error) = written {
            self.warn_once(&format!("Couldn't write records ({}): {error}", path.display()));
        }
    }

    fn set_live(&self, value: Value) {
        let folder = lock(&self.folder).clone();
        let temporary = folder.join("live.json.tmp");
        let written = fs::write(&temporary, value.to_string())
            .and_then(|()| fs::rename(&temporary, folder.join("live.json")));
        if let Err(error) = written {
            self.warn_once(&format!("Couldn't write live.json: {error}"));
        }
    }

    fn warn_once(&self, message: &str) {
        if !self.warned.swap(true, Ordering::Relaxed) {
            tracing::warn!("{message}");
        }
    }

    fn read_timezone(&self) {
        let path = lock(&self.folder).join("timezone.json");
        let minutes = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value.get("utcOffsetMinutes")?.as_i64())
            .unwrap_or(0);
        self.offset_minutes.store(minutes, Ordering::Relaxed);
        OFFSET_MINUTES.store(minutes, Ordering::Relaxed);
    }

    fn today(&self) -> String {
        civil_date(now_ms() as i64 / 1000 + self.offset_minutes.load(Ordering::Relaxed) * 60)
    }

    fn sweep_if_new_day(&self, day: &str) {
        let changed = {
            let mut swept = lock(&self.swept_day);
            if swept.as_str() == day {
                false
            } else {
                *swept = day.to_string();
                true
            }
        };
        if changed {
            self.read_timezone();
            self.sweep();
        }
    }

    fn sweep(&self) {
        let cutoff = civil_date(
            now_ms() as i64 / 1000 + self.offset_minutes.load(Ordering::Relaxed) * 60 - KEEP_DAYS * 86_400,
        );
        let folder = lock(&self.folder).clone();
        let Ok(entries) = fs::read_dir(&folder) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(day) = name.strip_prefix("events-").and_then(|rest| rest.strip_suffix(".jsonl")) else {
                continue;
            };
            if day < cutoff.as_str() {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn read_switches(text: &str) -> HashMap<String, bool> {
    text.lines()
        .filter(|line| !line.starts_with([' ', '\t']))
        .filter_map(|line| {
            let bare = line.split('#').next()?.trim();
            let (key, value) = bare.split_once(':')?;
            match value.trim() {
                "true" => Some((key.trim().to_string(), true)),
                "false" => Some((key.trim().to_string(), false)),
                _ => None,
            }
        })
        .collect()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

/// The server's local time as `HH:MM`, for rule schedules.
pub(crate) fn local_minute() -> String {
    let seconds = (now_ms() as i64 / 1000 + OFFSET_MINUTES.load(Ordering::Relaxed) * 60).rem_euclid(86_400);
    format!("{:02}:{:02}", seconds / 3600, seconds % 3600 / 60)
}

/// The server's local weekday (`mon`..`sun`), for daily announcements.
pub(crate) fn local_weekday() -> &'static str {
    let seconds = now_ms() as i64 / 1000 + OFFSET_MINUTES.load(Ordering::Relaxed) * 60;
    studio_format::announcement::weekday(seconds.div_euclid(86_400))
}

fn hundredths(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn tps_of(server: &Server) -> f64 {
    hundredths(server.get_tps().min(20.0))
}

fn put(line: &mut Value, key: &str, value: Value) {
    if let Value::Object(map) = line {
        map.insert(key.to_string(), value);
    }
}

fn put_where(line: &mut Value, player: &Player) {
    let (x, y, z) = player.get_position();
    let world = player.get_world();
    put(line, "world", json!(world_name(&world.get_id(), &world.get_dimension())));
    put(line, "x", json!(x));
    put(line, "y", json!(y));
    put(line, "z", json!(z));
}

/// Loaded chunks over every world.
fn loaded_chunks(server: &Server) -> u64 {
    server.get_all_worlds().iter().map(|world| u64::from(world.get_loaded_chunk_count())).sum()
}

/// `loaded.json` as the Java add-ons write it: per world, the loaded chunks and the force-loaded ones apart, as
/// flat `x, z` lists.
fn loaded_json(server: &Server) -> Value {
    let worlds: Vec<Value> = server
        .get_all_worlds()
        .into_iter()
        .map(|world| {
            let (mut chunks, mut forced) = (Vec::new(), Vec::new());
            for chunk in world.get_loaded_chunks() {
                let list = if chunk.is_force_loaded() { &mut forced } else { &mut chunks };
                list.push(chunk.get_x());
                list.push(chunk.get_z());
            }
            json!({ "world": world_name(&world.get_id(), &world.get_dimension()), "chunks": chunks, "forced": forced })
        })
        .collect();
    json!({ "at": now_ms(), "worlds": worlds })
}

/// One item of `inventories.json`: `[slot, id, count, damage]`.
async fn inventory_item(slot: i32, stack: &pumpkin_plugin_api::ItemStack) -> Option<Value> {
    let id = stack.get_registry_key().await;
    let count = stack.get_count().await;
    if count == 0 || id == "minecraft:air" {
        return None;
    }
    Some(json!([slot, id, count, stack.get_damage().await.max(0)]))
}

/// `inventories.json` as the Java add-ons write it, with Paper's slots (0-35 the bag, 100-103 boots to helmet,
/// -106 the off hand).
async fn inventories_json(server: &Server) -> Value {
    use pumpkin_plugin_api::world::EquipmentSlot;
    let mut players = Vec::new();
    for player in server.get_all_players() {
        let mut items = Vec::new();
        for (slot, stack) in player.get_inventory().as_inventory().get_all_items().into_iter().enumerate() {
            if let Some(stack) = stack
                && let Some(one) = inventory_item(slot as i32, &stack).await
            {
                items.push(one);
            }
        }
        if let Some(living) = player.as_entity().as_living() {
            for (slot, part) in [(100, EquipmentSlot::Feet), (101, EquipmentSlot::Legs), (102, EquipmentSlot::Chest), (103, EquipmentSlot::Head), (-106, EquipmentSlot::OffHand)] {
                if let Some(stack) = living.get_equipment(part)
                    && let Some(one) = inventory_item(slot, &stack).await
                {
                    items.push(one);
                }
            }
        }
        let mut ender = Vec::new();
        for (slot, stack) in player.get_ender_chest().get_all_items().into_iter().enumerate() {
            if let Some(stack) = stack
                && let Some(one) = inventory_item(slot as i32, &stack).await
            {
                ender.push(one);
            }
        }
        let id = player.get_id();
        players.push(json!({ "name": player.get_name(), "uuid": uuid_text(id.high, id.low), "items": items, "ender": ender }));
    }
    json!({ "at": now_ms(), "players": players })
}

/// `commands.json` as the Java add-ons write it (format 2): every command and alias without a namespace, its
/// usage without the slash and the plugin that made it. Pumpkin has no way to ask a command for its
/// suggestions, so `args` stays empty.
fn commands_json(server: &Server) -> Value {
    let mut out = Vec::new();
    for one in server.get_known_commands() {
        let usage = one.usage.as_deref().map(|text| text.trim_start_matches('/').to_string()).filter(|text| !text.trim().is_empty());
        for name in std::iter::once(&one.name).chain(one.aliases.iter()) {
            if name.contains(':') {
                continue;
            }
            out.push(json!({ "name": name, "usage": usage, "from": one.plugin, "args": [] }));
        }
    }
    json!({ "at": now_ms(), "format": 2, "commands": out })
}

/// A UUID's usual text form from its two halves.
fn uuid_text(high: u64, low: u64) -> String {
    let all = format!("{high:016x}{low:016x}");
    format!("{}-{}-{}-{}-{}", &all[0..8], &all[8..12], &all[12..16], &all[16..20], &all[20..32])
}

/// Entity counts in the shape the Java addons write: total, kinds, crowded chunks and per-world totals.
fn entity_tally(server: &Server) -> Value {
    let mut kinds: HashMap<String, u64> = HashMap::new();
    let mut chunks: HashMap<(String, i32, i32), HashMap<String, u32>> = HashMap::new();
    let mut worlds = Vec::new();
    let mut total = 0u64;
    for world in server.get_all_worlds() {
        let name = world_name(&world.get_id(), &world.get_dimension());
        let entities = world.get_entities();
        worlds.push(json!({ "name": name, "entities": entities.len() }));
        for entity in entities {
            let kind = map::biome_name(&format!("{:?}", entity.get_type()));
            let (x, _, z) = entity.get_position();
            total += 1;
            *kinds.entry(kind.clone()).or_default() += 1;
            let spot = (name.clone(), (x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16));
            *chunks.entry(spot).or_default().entry(kind).or_default() += 1;
        }
    }
    let mut kinds: Vec<(String, u64)> = kinds.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut spots: Vec<((String, i32, i32), u32, String)> = chunks
        .into_iter()
        .map(|(spot, counts)| {
            let count = counts.values().sum();
            let top = counts.into_iter().max_by_key(|(_, n)| *n).map(|(kind, _)| kind).unwrap_or_default();
            (spot, count, top)
        })
        .filter(|(_, count, _)| *count >= HOTSPOT_FLOOR)
        .collect();
    spots.sort_by_key(|spot| std::cmp::Reverse(spot.1));
    spots.truncate(HOTSPOTS);
    json!({
        "entities": total,
        "entityKinds": kinds.iter().map(|(kind, count)| json!({ "type": kind, "count": count })).collect::<Vec<_>>(),
        "hotspots": spots.iter().map(|((world, x, z), count, top)| json!({ "world": world, "x": x, "z": z, "count": count, "top": top })).collect::<Vec<_>>(),
        "worlds": worlds,
    })
}

pub(crate) fn world_name(level: &str, dimension: &str) -> String {
    match dimension.rsplit(':').next().unwrap_or(dimension) {
        "the_nether" => format!("{level}_nether"),
        "the_end" => format!("{level}_the_end"),
        _ => level.to_string(),
    }
}

fn civil_date(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_seconds_into_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(951_782_400), "2000-02-29");
        let late = 1_789_255_800;
        assert_eq!(civil_date(late), "2026-09-12");
        assert_eq!(civil_date(late + 9 * 3600), "2026-09-13");
        assert_eq!(civil_date(-1), "1969-12-31");
    }

    #[test]
    fn names_worlds_like_paper() {
        assert_eq!(world_name("world", "minecraft:overworld"), "world");
        assert_eq!(world_name("world", "minecraft:the_nether"), "world_nether");
        assert_eq!(world_name("survival", "minecraft:the_end"), "survival_the_end");
    }

    #[test]
    fn writes_uuids() {
        assert_eq!(uuid_text(0x0011_2233_4455_6677, 0x8899_aabb_ccdd_eeff), "00112233-4455-6677-8899-aabbccddeeff");
    }
}
