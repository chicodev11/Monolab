//! The studio on Pumpkin: the looks (scoreboard, tab list, MOTD, chat format, join and leave messages,
//! announcements, boss bar, holograms) from the same JSON files the Paper add-on reads, in `<data folder>/studio/<kind>/<id>.json`.
//! The interactive kinds (commands, menus, items, recipes, rules, NPCs, dialogs, loot, advancements, worlds) are in `play`.
//! Files are rechecked every 2 seconds and what loaded goes to `studio/status.json` for the app.
//! The checks and the MiniMessage reader are the shared `studio-format` crate's.

use pumpkin_plugin_api::boss_bar::{BossBar, BossBarColor, BossBarDivision};
use pumpkin_plugin_api::scoreboard::{DisplaySlot, NumberFormat, RenderType, Scoreboard};
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::world::{Entity, World};
use pumpkin_plugin_api::{BillboardMode, DisplayEntityExt, EntityDisplayExt, EntityType, Player, Server, TeamSettings};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use studio_format::fields::{self, Data};
use studio_format::{BOSSBAR_COLORS, BOSSBAR_STYLES, HOLOGRAM_BILLBOARDS, KINDS, PUMPKIN_KINDS, announcement, mini};

pub mod blocks;
pub mod canvas;
mod held;
pub mod mobs;
pub mod models;
pub mod pack;
pub mod play;
pub mod quests;
pub mod structures;
mod tests_tool;
pub mod worldgen;

/// The feature tick runs every this many server ticks; files are rechecked every 4th.
pub const TICK_EVERY: u64 = 10;
const OBJECTIVE: &str = "monolab";
const MOST_LINES: usize = 15;
/// Our hologram entities carry this plus the file name as a hidden custom name. They aren't saved with their chunk;
/// ones an older add-on saved still come back and are taken over or removed by name.
const HOLOGRAM_TAG: &str = "monolab-hologram:";
/// Holograms are matched against the world this often.
const HOLOGRAM_SCAN_SECONDS: u64 = 5;

struct Loaded {
    kind: String,
    id: String,
    data: Option<Data>,
    error: Option<String>,
}

pub struct Studio {
    root: PathBuf,
    version: String,
    stamps: HashMap<PathBuf, u64>,
    loaded: BTreeMap<String, Loaded>,
    was_on: bool,
    status_written: bool,
    off: BTreeSet<String>,
    ticks: u64,
    /// Per player (uuid): when the sidebar / tab list was last drawn, and whether ours is showing.
    board_at: HashMap<String, u64>,
    boarded: HashSet<String>,
    tab_at: HashMap<String, u64>,
    tabbed: HashSet<String>,
    bar_at: HashMap<String, u64>,
    bars: HashMap<String, BossBar>,
    /// Hologram entities in use, by file name; rebuilt from the world on every scan.
    holograms: HashMap<String, Entity>,
    /// The hologram files changed: the next tick removes ours and makes them again.
    holograms_dirty: bool,
    /// Per announcement: seconds counted and the next message.
    announce: HashMap<String, (i64, usize)>,
    /// `after-start` announcements that have had their first message.
    announce_started: HashSet<String>,
    /// `after-join` announcements waiting: (file name, player uuid, seconds left).
    announce_joins: Vec<(String, String, i64)>,
    /// The minute `daily` announcements last looked at, so each time is sent once.
    announce_minute: String,
    /// Players seen joining since the add-on was installed, for `firstJoin`.
    seen: Option<HashSet<String>>,
    random: u64,
    /// Bumped whenever the files change, so the interactive kinds rebuild once.
    generation: u64,
    /// What a feature found wrong with a file it was given (a recipe Pumpkin refused...), by kind/id.
    problems: BTreeMap<String, String>,
    play: play::Play,
    /// M3: studio blocks and furniture. Taken out while a decision names items with the studio's text.
    blocks: Option<blocks::Blocks>,
    pack: pack::Pack,
}

/// What text placeholders need about a player.
pub struct Who {
    pub uuid: String,
    pub name: String,
    pub world: String,
    pub ping: u32,
}

impl Who {
    pub fn of(player: &Player, world: String) -> Self {
        Self { uuid: player.get_id().to_string(), name: player.get_name(), world, ping: player.get_ping() }
    }
}

impl Studio {
    pub fn new(data_folder: &Path, version: &str) -> Self {
        Self {
            root: data_folder.join("studio"),
            version: version.to_string(),
            stamps: HashMap::new(),
            loaded: BTreeMap::new(),
            was_on: false,
            status_written: false,
            off: BTreeSet::new(),
            ticks: 0,
            board_at: HashMap::new(),
            boarded: HashSet::new(),
            tab_at: HashMap::new(),
            tabbed: HashSet::new(),
            bar_at: HashMap::new(),
            bars: HashMap::new(),
            holograms: HashMap::new(),
            holograms_dirty: true,
            announce: HashMap::new(),
            announce_started: HashSet::new(),
            announce_joins: Vec::new(),
            announce_minute: String::new(),
            seen: None,
            random: 0x9e37_79b9_7f4a_7c15,
            generation: 0,
            problems: BTreeMap::new(),
            play: play::Play::default(),
            blocks: Some(blocks::Blocks::new(&data_folder.join("studio"))),
            pack: pack::Pack::new(&data_folder.join("studio")),
        }
    }

    // ---------- files ----------

    /// Rereads changed files. `on` is the `studio` switch, `off` the kinds turned off one by one.
    pub fn poll(&mut self, on: bool, off: BTreeSet<String>) {
        if !on {
            if self.was_on || !self.status_written {
                self.loaded.clear();
                self.stamps.clear();
                self.forget_drawn();
                self.write_status(false);
            }
            self.was_on = false;
            return;
        }
        let mut changed = !self.was_on || !self.status_written;
        self.was_on = true;
        if off != self.off {
            self.off = off;
            changed = true;
        }
        let _ = fs::create_dir_all(&self.root);
        let mut seen = HashMap::new();
        for kind in KINDS {
            let Ok(entries) = fs::read_dir(self.root.join(kind)) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|one| one.to_str()) != Some("json") {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|at| at.as_millis() as u64)
                    .unwrap_or(0);
                let stamp = modified ^ meta.len();
                seen.insert(path.clone(), stamp);
                if self.stamps.get(&path) != Some(&stamp) {
                    changed = true;
                    let one = read(kind, &path);
                    self.loaded.insert(format!("{kind}/{}", one.id), one);
                }
            }
        }
        // The pack's block numbers and model rigs are the app's too: a rebuilt pack (a block saved just after its
        // file was read) reloads the interactive kinds like a changed file does.
        let pack = self.root.join("pack");
        let rigs = fs::read_dir(pack.join("models")).into_iter().flatten().flatten().map(|entry| entry.path());
        for path in std::iter::once(pack.join("ids.json")).chain(rigs) {
            let Ok(meta) = fs::metadata(&path) else { continue };
            let modified = meta
                .modified()
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|at| at.as_millis() as u64)
                .unwrap_or(0);
            let stamp = modified ^ meta.len();
            if self.stamps.get(&path) != Some(&stamp) {
                changed = true;
            }
            seen.insert(path, stamp);
        }
        for gone in self.stamps.keys().filter(|path| !seen.contains_key(*path)) {
            changed = true;
            let kind = gone.parent().and_then(|one| one.file_name()).and_then(|one| one.to_str()).unwrap_or_default();
            self.loaded.remove(&format!("{kind}/{}", stem(gone)));
        }
        self.stamps = seen;
        if changed {
            // Redraw everything with what is there now.
            self.board_at.clear();
            self.tab_at.clear();
            self.bar_at.clear();
            self.holograms_dirty = true;
            let ids: HashSet<String> = self.active_rows("announcement").into_iter().map(|(id, _)| id.to_string()).collect();
            self.announce.retain(|id, _| ids.contains(id));
            self.announce_started.retain(|id| ids.contains(id));
            self.announce_joins.retain(|(id, _, _)| ids.contains(id));
            self.generation += 1;
            self.problems.clear();
            self.write_status(true);
        }
    }

    /// The usable, enabled resources of a kind that runs here, with their file names.
    fn active_rows(&self, kind: &str) -> Vec<(&str, &Data)> {
        if !self.was_on || self.off.contains(kind) || !PUMPKIN_KINDS.contains(&kind) {
            return Vec::new();
        }
        self.loaded
            .values()
            .filter(|one| one.kind == kind)
            .filter_map(|one| Some((one.id.as_str(), one.data.as_ref()?)))
            .filter(|(_, data)| studio_format::enabled(data))
            .collect()
    }

    fn active(&self, kind: &str) -> Vec<&Data> {
        self.active_rows(kind).into_iter().map(|(_, data)| data).collect()
    }

    fn pick(&self, kind: &str, player: Option<&Player>, world: &str) -> Option<Data> {
        let all = self.active(kind);
        fields::pick(&all, |one| one, |data| player.is_none_or(|player| shows(data, player, world))).map(|one| (*one).clone())
    }

    fn write_status(&mut self, on: bool) {
        let resources: Vec<Value> = self
            .loaded
            .values()
            .map(|one| {
                let mut row = json!({ "kind": one.kind, "id": one.id, "enabled": one.data.as_ref().is_some_and(studio_format::enabled) });
                if let Some(error) = one.error.as_ref().or_else(|| self.problems.get(&format!("{}/{}", one.kind, one.id))) {
                    row["error"] = json!(error);
                }
                row
            })
            .collect();
        let text = json!({
            "schema": studio_format::SCHEMA,
            "addon": self.version,
            "at": crate::now_ms(),
            "on": on,
            "off": self.off.iter().collect::<Vec<_>>(),
            "resources": resources,
        })
        .to_string();
        let _ = fs::create_dir_all(&self.root);
        let temporary = self.root.join("status.json.tmp");
        if fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, self.root.join("status.json"))).is_ok() {
            self.status_written = true;
        }
    }

    /// A feature couldn't use a file: shows as that file's error in status.json (written only when it's new).
    fn problem(&mut self, kind: &str, id: &str, text: &str) {
        let key = format!("{kind}/{id}");
        if self.problems.get(&key).map(String::as_str) != Some(text) {
            self.problems.insert(key, text.to_string());
            self.write_status(self.was_on);
        }
    }

    /// Rechecks the files every 40 ticks (2 seconds), like the Paper add-on.
    pub fn poll_every(&mut self, on: bool, off: BTreeSet<String>) {
        if self.ticks.is_multiple_of(40) {
            self.poll(on, off);
            self.pack.check();
        }
    }

    // ---------- every 10 ticks ----------

    pub async fn tick(&mut self, server: &Server, players: &[(Player, Who)]) {
        self.ticks += TICK_EVERY;
        for (player, who) in players {
            self.refresh_board(server, player, who);
            self.refresh_tab(server, player, who);
            self.refresh_bar(server, player, who).await;
        }
        let online: Vec<&Player> = players.iter().map(|(player, _)| player).collect();
        self.pack.send(server, &online).await;
        self.held_requests(&online).await;
        self.test_requests(server, &online).await;
        if self.ticks.is_multiple_of(20) {
            self.announce(server, players);
            self.refresh_holograms(server).await;
        }
        if self.was_on {
            self.tick_play(server).await;
            if self.ticks.is_multiple_of(20) {
                self.tick_quests(server, &online).await;
            }
        }
    }

    /// Takes our sidebar and tab list off everyone and our holograms out of the worlds, for when the add-on stops.
    pub async fn stop(&mut self, server: &Server, players: &[(Player, Who)]) {
        self.stop_play(server).await;
        self.pack.unhost(server);
        self.holograms.clear();
        for (_, found) in tagged_holograms(server) {
            found.into_iter().for_each(|(entity, _)| entity.remove());
        }
        for (player, who) in players {
            self.clear_board(player, who);
            self.clear_tab(player, who);
            self.clear_bar(who).await;
        }
    }

    fn forget_drawn(&mut self) {
        self.board_at.clear();
        self.tab_at.clear();
        self.bar_at.clear();
    }

    // ---------- scoreboard ----------

    fn refresh_board(&mut self, server: &Server, player: &Player, who: &Who) {
        let Some(one) = self.pick("scoreboard", Some(player), &who.world) else {
            self.clear_board(player, who);
            return;
        };
        let every = fields::number(&one, "refreshTicks", 20, 10, 20 * 60) as u64;
        if self.board_at.get(&who.uuid).is_some_and(|last| self.ticks - last < every) {
            return;
        }
        let Some(java) = player.as_java() else { return };
        self.board_at.insert(who.uuid.clone(), self.ticks);
        let board = java.get_scoreboard();
        let values = values(server, Some(who));
        let title = render(&self.vars_in(&fields::text(&one, "title", ""), Some(player)), &values);
        // Asked, not only remembered: Pumpkin may keep a player's board across a rejoin we missed, and a second
        // "add" disconnects the client.
        if self.boarded.insert(who.uuid.clone()) || !board.has_objective(OBJECTIVE) {
            if board.has_objective(OBJECTIVE) {
                board.update_objective(OBJECTIVE, title, RenderType::Integer, Some(NumberFormat::Blank));
            } else {
                board.add_objective(OBJECTIVE, title, RenderType::Integer, Some(NumberFormat::Blank));
            }
            board.set_display_slot(DisplaySlot::Sidebar, OBJECTIVE);
        } else {
            board.update_objective(OBJECTIVE, title, RenderType::Integer, Some(NumberFormat::Blank));
        }
        // A score line can't carry its own text here, so each line is an invisible entry in a team whose
        // prefix is the line (the usual way, and it keeps colors, hex and gradients).
        let lines = fields::lines(&one, "lines");
        let count = lines.len().min(MOST_LINES);
        for at in 0..MOST_LINES {
            let (entry, team) = line_names(at);
            if let Some(line) = lines.get(at).filter(|_| at < count) {
                let settings = team_settings(render(&self.vars_in(line, Some(player)), &values));
                if board.get_team(&team).is_some() {
                    board.update_team(&team, settings);
                } else {
                    board.create_team(&team, settings);
                    board.add_player_to_team(&team, &entry);
                }
                board.update_score(&entry, OBJECTIVE, (count - at) as i32, None);
            } else if board.get_team(&team).is_some() {
                board.remove_score(&entry, OBJECTIVE);
                board.remove_team(&team);
            }
        }
    }

    fn clear_board(&mut self, player: &Player, who: &Who) {
        self.board_at.remove(&who.uuid);
        if !self.boarded.remove(&who.uuid) {
            return;
        }
        if let Some(java) = player.as_java() {
            let board = java.get_scoreboard();
            clear_sidebar(&board);
        }
    }

    // ---------- tab list ----------

    fn refresh_tab(&mut self, server: &Server, player: &Player, who: &Who) {
        let Some(one) = self.pick("tab", Some(player), &who.world) else {
            self.clear_tab(player, who);
            return;
        };
        let every = fields::number(&one, "refreshTicks", 40, 10, 20 * 60) as u64;
        if self.tab_at.get(&who.uuid).is_some_and(|last| self.ticks - last < every) {
            return;
        }
        self.tab_at.insert(who.uuid.clone(), self.ticks);
        self.tabbed.insert(who.uuid.clone());
        let values = values(server, Some(who));
        let header = render(&self.vars_in(&fields::lines(&one, "header").join("<newline>"), Some(player)), &values);
        let footer = render(&self.vars_in(&fields::lines(&one, "footer").join("<newline>"), Some(player)), &values);
        player.set_tab_list_header_footer(header, footer);
        if fields::has(&one, "playerName") {
            let name = self.vars_in(&fields::text(&one, "playerName", ""), Some(player));
            player.set_tab_list_name(Some(render(&name, &values)));
        }
    }

    fn clear_tab(&mut self, player: &Player, who: &Who) {
        self.tab_at.remove(&who.uuid);
        if self.tabbed.remove(&who.uuid) {
            player.set_tab_list_header_footer(TextComponent::text(""), TextComponent::text(""));
            player.set_tab_list_name(None);
        }
    }

    // ---------- boss bar ----------

    async fn refresh_bar(&mut self, server: &Server, player: &Player, who: &Who) {
        let Some(one) = self.pick("bossbar", Some(player), &who.world) else {
            self.clear_bar(who).await;
            return;
        };
        let every = fields::number(&one, "refreshTicks", 20, 10, 20 * 60) as u64;
        if self.bar_at.get(&who.uuid).is_some_and(|last| self.ticks - last < every) {
            return;
        }
        self.bar_at.insert(who.uuid.clone(), self.ticks);
        let title = render(&self.vars_in(&fields::text(&one, "title", ""), Some(player)), &values(server, Some(who)));
        let color = bar_color(&fields::choice(&one, "color", &BOSSBAR_COLORS, "purple"));
        let division = bar_division(&fields::choice(&one, "style", &BOSSBAR_STYLES, "progress"));
        let health = fields::fraction(&one, "progress", 1.0) as f32;
        match self.bars.get(&who.uuid) {
            Some(bar) => {
                bar.set_title(title).await;
                bar.set_color(color).await;
                bar.set_division(division).await;
                bar.set_health(health).await;
            }
            None => {
                // Showing it takes a player handle of its own.
                let Some(owned) = server.get_player_by_uuid(player.get_id()) else { return };
                let bar = BossBar::new(title, color, division);
                bar.set_health(health).await;
                bar.add_player(owned).await;
                self.bars.insert(who.uuid.clone(), bar);
            }
        }
    }

    async fn clear_bar(&mut self, who: &Who) {
        self.bar_at.remove(&who.uuid);
        if let Some(bar) = self.bars.remove(&who.uuid) {
            bar.remove_all().await;
        }
    }

    // ---------- holograms ----------

    /// Once a second: refresh the text. Every 5 seconds (or after a file change): match the world's tagged text
    /// displays to the files: take over one per file, remove extras and leftovers, make what is missing in a
    /// loaded chunk.
    async fn refresh_holograms(&mut self, server: &Server) {
        let wanted: Vec<(String, Data)> =
            self.active_rows("hologram").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        let values = values(server, None);
        let texts: Vec<String> = wanted.iter().map(|(_, data)| fields::lines(data, "lines").join("<newline>")).collect();
        let texts: Vec<String> = texts.iter().map(|text| self.vars_in(text, None)).collect();
        let scan = self.holograms_dirty || (self.ticks / 20).is_multiple_of(HOLOGRAM_SCAN_SECONDS);
        // One gone from the world between scans (killed, its chunk unloaded): scan now so it is made again.
        let lost = self.holograms.values().any(|entity| !entity.is_valid());
        if !scan && !lost {
            for ((id, _), text) in wanted.iter().zip(&texts) {
                if let Some(entity) = self.holograms.get(id) {
                    set_hologram_text(entity, text, &values);
                }
            }
            return;
        }
        let fresh = std::mem::take(&mut self.holograms_dirty);
        self.holograms.clear();
        let mut found = tagged_holograms(server);
        let (mut made, mut taken, mut removed) = (0, 0, 0);
        for ((id, data), text) in wanted.iter().zip(&texts) {
            let mut mine = found.remove(id).unwrap_or_default();
            if fresh {
                // The file changed: start over so position and looks match it.
                removed += mine.len();
                mine.drain(..).for_each(|(entity, _)| entity.remove());
            }
            let place = hologram_place(data);
            let mut kept = None;
            for (entity, world) in mine {
                if kept.is_none() && place.as_ref().is_some_and(|(name, ..)| *name == world) {
                    kept = Some(entity);
                    taken += 1;
                } else {
                    entity.remove();
                    removed += 1;
                }
            }
            if kept.is_none() {
                kept = match place {
                    Some((name, x, y, z)) => spawn_hologram(server, id, &name, (x, y, z), data).await,
                    None => None,
                };
                made += usize::from(kept.is_some());
            }
            if let Some(entity) = kept {
                set_hologram_text(&entity, text, &values);
                self.holograms.insert(id.clone(), entity);
            }
        }
        // Tagged ones whose file is gone or turned off.
        for (_, leftovers) in found {
            removed += leftovers.len();
            leftovers.into_iter().for_each(|(entity, _)| entity.remove());
        }
        if made > 0 || removed > 0 {
            tracing::info!("Studio holograms: {made} made, {taken} taken over, {removed} removed");
        }
    }

    // ---------- MOTD ----------

    /// The server list text, when a MOTD resource has lines.
    pub fn motd(&self, server: &Server) -> Option<TextComponent> {
        let one = self.pick("motd", None, "")?;
        let lines = fields::lines(&one, "lines");
        if lines.is_empty() {
            return None;
        }
        Some(render(&lines[..lines.len().min(2)].join("<newline>"), &values(server, None)))
    }

    // ---------- chat ----------

    /// The formatted chat line, when a chat resource has a `format`.
    pub fn chat(&self, server: &Server, player: &Player, who: &Who, message: &str) -> Option<TextComponent> {
        let one = self.pick("chat", Some(player), &who.world)?;
        if !fields::has(&one, "format") {
            return None;
        }
        let mut values = values(server, Some(who));
        values.push(("message", message.to_string()));
        Some(render(&fields::text(&one, "format", ""), &values))
    }

    // ---------- join & leave ----------

    /// `Some(None)` hides the message (an empty text), `None` leaves the server's own.
    pub fn joined(&mut self, server: &Server, player: &Player, who: &Who) -> Option<Option<TextComponent>> {
        let first = self.first_time(&who.uuid);
        if first {
            self.fire_later("first_join", player);
        }
        self.fire_later("join", player);
        let waits: Vec<(String, String, i64)> = self
            .active_rows("announcement")
            .into_iter()
            .filter(|(_, data)| announcement::schedule(data) == "after-join")
            .map(|(id, data)| (id.to_string(), who.uuid.clone(), announcement::delay_seconds(data)))
            .collect();
        self.announce_joins.extend(waits);
        self.board_at.remove(&who.uuid);
        self.tab_at.remove(&who.uuid);
        self.bar_at.remove(&who.uuid);
        let one = self.pick("message", Some(player), &who.world)?;
        let key = if first && fields::has(&one, "firstJoin") { "firstJoin" } else { "join" };
        self.message(server, who, &one, key)
    }

    pub async fn left(&mut self, server: &Server, player: &Player, who: &Who) -> Option<Option<TextComponent>> {
        self.announce_joins.retain(|(_, uuid, _)| *uuid != who.uuid);
        self.board_at.remove(&who.uuid);
        self.boarded.remove(&who.uuid);
        self.tab_at.remove(&who.uuid);
        self.tabbed.remove(&who.uuid);
        self.clear_bar(who).await;
        self.pack.left(player);
        self.menu_closed(player);
        self.fire(server, "quit", Some(player), Vec::new(), |_| true).await;
        let one = self.pick("message", Some(player), &who.world)?;
        self.message(server, who, &one, "leave")
    }

    fn message(&self, server: &Server, who: &Who, one: &Data, key: &str) -> Option<Option<TextComponent>> {
        if !fields::has(one, key) {
            return None;
        }
        let text = fields::text(one, key, "");
        Some((!text.trim().is_empty()).then(|| render(&text, &values(server, Some(who)))))
    }

    /// Whether this is the player's first join since the add-on came; remembered in `studio/data/seen.txt`.
    fn first_time(&mut self, uuid: &str) -> bool {
        let file = self.root.join("data").join("seen.txt");
        let seen = self
            .seen
            .get_or_insert_with(|| fs::read_to_string(&file).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default());
        if !seen.insert(uuid.to_string()) {
            return false;
        }
        let _ = fs::create_dir_all(self.root.join("data"));
        let mut all: Vec<&String> = seen.iter().collect();
        all.sort();
        let _ = fs::write(&file, all.iter().map(|one| format!("{one}\n")).collect::<String>());
        true
    }

    // ---------- announcements ----------

    /// Once a second: each announcement sends its next message when its schedule says so (see
    /// `studio_format::announcement`): every N seconds, at clock times, once after the studio loads, or after a
    /// player joins (to that player).
    fn announce(&mut self, server: &Server, players: &[(Player, Who)]) {
        let minute = crate::local_minute();
        let new_minute = minute != self.announce_minute;
        self.announce_minute = minute.clone();
        let day = crate::local_weekday();
        let rows: Vec<(String, Data)> =
            self.active_rows("announcement").into_iter().map(|(id, data)| (id.to_string(), data.clone())).collect();
        for (id, one) in &rows {
            let all = announcement_lines(one);
            if all.is_empty() {
                continue;
            }
            let due = match announcement::schedule(one).as_str() {
                "daily" => new_minute && announcement::daily_due(one, &minute, day),
                "after-join" => false,
                "after-start" => {
                    let started = self.announce_started.contains(id);
                    let wait = if started { announcement::repeat_seconds(one) } else { announcement::delay_seconds(one) };
                    let state = self.announce.entry(id.clone()).or_insert((0, 0));
                    if started && wait == 0 {
                        false
                    } else {
                        state.0 += 1;
                        if state.0 < wait {
                            false
                        } else {
                            state.0 = 0;
                            self.announce_started.insert(id.clone());
                            true
                        }
                    }
                }
                _ => {
                    let every = announcement::every_seconds(one);
                    let state = self.announce.entry(id.clone()).or_insert((0, 0));
                    state.0 += 1;
                    if state.0 < every {
                        false
                    } else {
                        state.0 = 0;
                        true
                    }
                }
            };
            if !due {
                continue;
            }
            let pick = self.next_announcement(id, one, all.len());
            for (player, who) in players {
                if shows(one, player, &who.world) {
                    player.send_system_message(render(&all[pick], &values(server, Some(who))), false);
                }
            }
        }
        let mut ready = Vec::new();
        self.announce_joins.retain_mut(|(id, uuid, left)| {
            *left -= 1;
            if *left < 0 {
                ready.push((id.clone(), uuid.clone()));
                false
            } else {
                true
            }
        });
        for (id, uuid) in ready {
            let Some((_, one)) = rows.iter().find(|(row, data)| *row == id && announcement::schedule(data) == "after-join") else {
                continue;
            };
            let Some((player, who)) = players.iter().find(|(_, who)| who.uuid == uuid) else { continue };
            let all = announcement_lines(one);
            if all.is_empty() || !shows(one, player, &who.world) {
                continue;
            }
            let pick = self.next_announcement(&id, one, all.len());
            player.send_system_message(render(&all[pick], &values(server, Some(who))), false);
        }
    }

    /// Which message goes next: a random one, or the next in turn.
    fn next_announcement(&mut self, id: &str, one: &Data, count: usize) -> usize {
        if one.get("random").and_then(Value::as_bool) == Some(true) {
            self.random ^= self.random << 13;
            self.random ^= self.random >> 7;
            self.random ^= self.random << 17;
            (self.random % count as u64) as usize
        } else {
            let state = self.announce.entry(id.to_string()).or_insert((0, 0));
            state.1 += 1;
            (state.1 - 1) % count
        }
    }
}

/// An announcement's messages, without the empty rows.
fn announcement_lines(one: &Data) -> Vec<String> {
    fields::lines(one, "messages").into_iter().filter(|line| !line.trim().is_empty()).collect()
}

/// Our tagged text displays in every world: file name -> (entity, world name).
fn tagged_holograms(server: &Server) -> HashMap<String, Vec<(Entity, String)>> {
    let mut out: HashMap<String, Vec<(Entity, String)>> = HashMap::new();
    for world in server.get_all_worlds() {
        let name = crate::world_name(&world.get_id(), &world.get_dimension());
        for entity in world.get_entities() {
            if entity.get_type() != EntityType::TextDisplay {
                continue;
            }
            let tag = entity.get_custom_name().map(|one| one.get_text()).unwrap_or_default();
            let Some(id) = tag.strip_prefix(HOLOGRAM_TAG) else { continue };
            out.entry(id.to_string()).or_default().push((entity, name.clone()));
        }
    }
    out
}

/// Where a hologram goes: world name and position, when the file has them.
fn hologram_place(data: &Data) -> Option<(String, f64, f64, f64)> {
    let at = |key: &str| data.get(key).and_then(Value::as_f64).filter(|one| one.is_finite());
    Some((fields::text(data, "world", "world"), at("x")?, at("y")?, at("z")?))
}

fn world_named(server: &Server, name: &str) -> Option<World> {
    server.get_all_worlds().into_iter().find(|world| crate::world_name(&world.get_id(), &world.get_dimension()) == name)
}

/// Makes the text display, only where the chunk is loaded.
async fn spawn_hologram(server: &Server, id: &str, world: &str, (x, y, z): (f64, f64, f64), data: &Data) -> Option<Entity> {
    let world = world_named(server, world)?;
    world.get_chunk((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16))?;
    let entity = world.spawn_entity(EntityType::TextDisplay, (x, y, z)).await;
    entity.set_saves_to_disk(false);
    entity.set_custom_name(TextComponent::text(&format!("{HOLOGRAM_TAG}{id}")));
    entity.set_custom_name_visible(false);
    if let Some(display) = entity.as_display() {
        display.set_billboard(match fields::choice(data, "billboard", &HOLOGRAM_BILLBOARDS, "center").as_str() {
            "fixed" => BillboardMode::Fixed,
            "vertical" => BillboardMode::Vertical,
            "horizontal" => BillboardMode::Horizontal,
            _ => BillboardMode::Center,
        });
        let scale = data.get("scale").and_then(Value::as_f64).filter(|one| one.is_finite()).unwrap_or(1.0).clamp(0.1, 10.0) as f32;
        if (scale - 1.0).abs() > f32::EPSILON {
            display.set_scale(scale, scale, scale);
        }
    }
    if let Some(text) = entity.as_text_display() {
        text.set_shadow(data.get("shadow").and_then(Value::as_bool) == Some(true));
        text.set_see_through(data.get("seeThrough").and_then(Value::as_bool) == Some(true));
        if data.get("background").and_then(Value::as_bool) == Some(false) {
            text.set_default_background(false);
            text.set_background(0);
        }
    }
    Some(entity)
}

/// `text` is the file's lines joined, variables already filled.
fn set_hologram_text(entity: &Entity, text: &str, values: &[(&str, String)]) {
    if let Some(display) = entity.as_text_display() {
        display.set_text(render(text, values));
    }
}

fn bar_color(name: &str) -> BossBarColor {
    match name {
        "pink" => BossBarColor::Pink,
        "blue" => BossBarColor::Blue,
        "red" => BossBarColor::Red,
        "green" => BossBarColor::Green,
        "yellow" => BossBarColor::Yellow,
        "white" => BossBarColor::White,
        _ => BossBarColor::Purple,
    }
}

fn bar_division(name: &str) -> BossBarDivision {
    match name {
        "notched_6" => BossBarDivision::Notches6,
        "notched_10" => BossBarDivision::Notches10,
        "notched_12" => BossBarDivision::Notches12,
        "notched_20" => BossBarDivision::Notches20,
        _ => BossBarDivision::NoDivision,
    }
}

fn read(kind: &str, path: &Path) -> Loaded {
    let id = stem(path);
    let checked = fs::read_to_string(path)
        .map_err(|error| format!("Could not read the file: {error}"))
        .and_then(|text| studio_format::check(&id, &text));
    match checked {
        Ok(_) if !PUMPKIN_KINDS.contains(&kind) => {
            Loaded { kind: kind.into(), id, data: None, error: Some("Pumpkin servers don't run this kind yet.".into()) }
        }
        Ok(data) => Loaded { kind: kind.into(), id, data: Some(data), error: None },
        Err(error) => Loaded { kind: kind.into(), id, data: None, error: Some(error) },
    }
}

fn stem(path: &Path) -> String {
    path.file_stem().and_then(|one| one.to_str()).unwrap_or_default().to_string()
}

/// `show.worlds` and `show.permission`. A permission counts only when it is set for the player.
fn shows(data: &Data, player: &Player, world: &str) -> bool {
    fields::show(data).allows(world, |node| player.has_permission_set(node) == Some(true))
}

/// Our placeholders: `<online>`, `<max>`, `<tps>`, and for a player `<player>`, `<world>`, `<ping>`.
fn values(server: &Server, who: Option<&Who>) -> Vec<(&'static str, String)> {
    let mut out = vec![
        ("online", server.get_player_count().to_string()),
        ("max", server.get_max_players().to_string()),
        ("tps", format!("{:.1}", server.get_tps().min(20.0))),
    ];
    if let Some(who) = who {
        out.push(("player", who.name.clone()));
        out.push(("world", who.world.clone()));
        out.push(("ping", who.ping.to_string()));
    }
    out
}

/// MiniMessage with placeholders filled, as a text component (Pumpkin's own MiniMessage reader, like Paper's
/// Adventure; placeholder values are escaped by `mini::fill`).
pub fn render(text: &str, values: &[(&str, String)]) -> TextComponent {
    TextComponent::from_minimessage(&mini::fill(&canvas::swap(text), values))
}

/// Line `at`'s invisible entry (a color code nobody sees) and its team.
fn line_names(at: usize) -> (String, String) {
    (format!("\u{a7}{:x}\u{a7}r", at), format!("monolab_line{at}"))
}

fn team_settings(prefix: TextComponent) -> TeamSettings {
    pumpkin_plugin_api::TeamSettingsBuilder::new().prefix(prefix).build()
}

fn clear_sidebar(board: &Scoreboard) {
    for at in 0..MOST_LINES {
        let (entry, team) = line_names(at);
        if board.get_team(&team).is_some() {
            board.remove_score(&entry, OBJECTIVE);
            board.remove_team(&team);
        }
    }
    board.remove_objective(OBJECTIVE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("monolab-pumpkin-studio-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn status(root: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(root.join("studio/status.json")).unwrap()).unwrap()
    }

    #[test]
    fn reports_what_loaded_like_the_paper_addon() {
        let root = folder("status");
        let studio_dir = root.join("studio");
        fs::create_dir_all(studio_dir.join("motd")).unwrap();
        fs::create_dir_all(studio_dir.join("menu")).unwrap();
        fs::create_dir_all(studio_dir.join("tab")).unwrap();
        fs::write(studio_dir.join("motd/default.json"), r#"{"schema":1,"id":"default","lines":["<gold>Hi"]}"#).unwrap();
        fs::write(studio_dir.join("menu/shop.json"), r#"{"schema":1,"id":"shop"}"#).unwrap();
        fs::write(studio_dir.join("tab/broken.json"), "{").unwrap();
        let mut studio = Studio::new(&root, "40");
        studio.poll(true, BTreeSet::new());
        let read = status(&root);
        assert_eq!(read["on"], true);
        assert_eq!(read["addon"], "40");
        let rows = read["resources"].as_array().unwrap();
        let row = |kind: &str| rows.iter().find(|one| one["kind"] == kind).unwrap().clone();
        assert_eq!(row("motd")["enabled"], true);
        assert!(row("motd").get("error").is_none());
        // Every kind runs on Pumpkin now.
        assert!(row("menu").get("error").is_none());
        assert!(row("tab")["error"].as_str().unwrap().starts_with("Invalid JSON"));
        assert_eq!(studio.active("motd").len(), 1);
        assert_eq!(studio.active("menu").len(), 1);

        // A kind turned off: kept and reported, not used.
        studio.poll(true, BTreeSet::from(["motd".to_string()]));
        assert!(studio.active("motd").is_empty());
        assert_eq!(status(&root)["off"], json!(["motd"]));

        // The whole studio off.
        studio.poll(false, BTreeSet::new());
        assert_eq!(status(&root)["on"], false);
        assert!(studio.active("motd").is_empty());

        // A removed file goes from the list.
        studio.poll(true, BTreeSet::new());
        fs::remove_file(studio_dir.join("menu/shop.json")).unwrap();
        studio.poll(true, BTreeSet::new());
        assert!(status(&root)["resources"].as_array().unwrap().iter().all(|one| one["kind"] != "menu"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remembers_first_joins() {
        let root = folder("seen");
        let mut studio = Studio::new(&root, "40");
        assert!(studio.first_time("a"));
        assert!(!studio.first_time("a"));
        let mut again = Studio::new(&root, "40");
        assert!(!again.first_time("a"));
        assert!(again.first_time("b"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn line_entries_are_unique_and_invisible() {
        let names: HashSet<String> = (0..MOST_LINES).map(|at| line_names(at).0).collect();
        assert_eq!(names.len(), MOST_LINES);
        assert!(
            names
                .iter()
                .all(|one| mini::plain(one).chars().all(|letter| letter == '\u{a7}' || letter.is_ascii_hexdigit() || letter == 'r'))
        );
    }
}
