//! Quests on Pumpkin (same files and rules as Paper's `StudioQuests`, shared in `studio_format::quest`). Progress is
//! kept on the player (`monolab:quests`); what happens to them (kills, pickups, blocks, crafts, catches, NPC talks,
//! where they stand) moves their active quests on, with an action bar line, a chat line per step and a title at the end.

use super::Studio;
use super::play::{Run, TAG};
use pumpkin_plugin_api::persistent_data::PersistentDataHolder;
use pumpkin_plugin_api::{Player, Server};
use serde_json::json;
use studio_format::fields::Data;
use studio_format::quest::{self, Change, Progress};

impl Studio {
    async fn quest_progress(&self, player: &Player) -> Progress {
        quest::parse(&player.as_entity().get_string(TAG, quest::KEY).await.unwrap_or_default())
    }

    async fn save_quest_progress(&self, player: &Player, progress: &Progress) {
        player.as_entity().set_string(TAG, quest::KEY, &quest::write(progress)).await;
    }

    fn quest_lookup(&self) -> impl Fn(&str) -> Option<Data> + '_ {
        move |id: &str| self.data_of("quest", id)
    }

    /// Something happened to a player that quest steps may count.
    pub async fn quest_event(&mut self, server: &Server, player: &Player, kind: &str, target: &str, amount: u32) {
        if self.active_rows("quest").is_empty() {
            return;
        }
        let mut progress = self.quest_progress(player).await;
        if progress.active.is_empty() {
            return;
        }
        let changes = quest::record(&mut progress, &self.quest_lookup(), kind, target, amount);
        if changes.is_empty() {
            return;
        }
        self.save_quest_progress(player, &progress).await;
        self.quest_changes(server, player, changes).await;
    }

    /// Once a second: `visit` steps for players who wait on one.
    pub async fn tick_quests(&mut self, server: &Server, players: &[&Player]) {
        if self.active_rows("quest").is_empty() {
            return;
        }
        for player in players {
            let mut progress = self.quest_progress(player).await;
            if !quest::wants_visit(&progress, &self.quest_lookup()) {
                continue;
            }
            let who = crate::who_of(player);
            let (x, y, z) = player.as_entity().get_position();
            let changes = quest::visit(&mut progress, &self.quest_lookup(), &who.world, (x, y, z));
            if !changes.is_empty() {
                self.save_quest_progress(player, &progress).await;
                self.quest_changes(server, player, changes).await;
            }
        }
    }

    async fn quest_changes(&mut self, server: &Server, player: &Player, changes: Vec<(String, Change)>) {
        for (id, change) in changes {
            let Some(data) = self.data_of("quest", &id) else { continue };
            let name = quest::name(&data, &id);
            let run = Run::of(player, vec![("quest".into(), id.clone()), ("quest_name".into(), name.clone())]);
            let steps = match change {
                Change::Counted(count, amount, text) => {
                    json!([{ "type": "actionbar", "text": format!("<gold>{name}</gold> <gray>·</gray> {text} <gray>({count}/{amount})") }])
                }
                Change::Next(text) => {
                    json!([{ "type": "message", "text": format!("<gold>{name}</gold> <gray>›</gray> {text}") }, { "type": "sound", "sound": "entity.experience_orb.pickup", "volume": 1, "pitch": 1.2 }])
                }
                Change::Finished => {
                    self.quest_finished(server, &data, &run).await;
                    continue;
                }
            };
            self.run_actions(server, Some(&steps), &run).await;
        }
    }

    async fn quest_finished(&mut self, server: &Server, data: &Data, run: &Run) {
        let name = run.extra.iter().find(|(key, _)| key == "quest_name").map(|(_, value)| value.clone()).unwrap_or_default();
        let shout = json!([
            { "type": "title", "title": "<gold>Quest complete", "subtitle": name, "stayTicks": 50 },
            { "type": "sound", "sound": "ui.toast.challenge_complete", "volume": 1, "pitch": 1 },
        ]);
        self.run_actions(server, Some(&shout), run).await;
        self.run_actions(server, data.get("rewards"), run).await;
        let advancement = quest::advancement(data);
        if !advancement.is_empty() {
            let grant = json!([{ "type": "advancement", "advancement": advancement }]);
            self.run_actions(server, Some(&grant), run).await;
        }
    }

    /// The `quest` action: `start` (runs `startActions` and shows the first step), `abandon` or `complete`.
    pub(super) async fn quest_action(&mut self, server: &Server, player: &Player, id: &str, mode: &str, run: &Run) {
        let Some(data) = self.data_of("quest", id) else { return };
        let mut progress = self.quest_progress(player).await;
        match mode {
            "abandon" => {
                if progress.active.remove(id).is_none() {
                    return;
                }
                self.save_quest_progress(player, &progress).await;
            }
            "complete" => {
                if quest::state(&progress, id) == "done" && !quest::repeatable(&data) {
                    return;
                }
                quest::finish(&mut progress, id);
                self.save_quest_progress(player, &progress).await;
                let run = run.with(vec![("quest".into(), id.to_string()), ("quest_name".into(), quest::name(&data, id))]);
                self.quest_finished(server, &data, &run).await;
            }
            _ => {
                if !quest::start(&mut progress, id, &data) {
                    return;
                }
                self.save_quest_progress(player, &progress).await;
                let name = quest::name(&data, id);
                let run = run.with(vec![("quest".into(), id.to_string()), ("quest_name".into(), name.clone())]);
                let first = quest::steps(&data).first().map(quest::step_text).unwrap_or_default();
                let shout = json!([{ "type": "message", "text": format!("<gold>{name}</gold> <gray>›</gray> {first}") }]);
                self.run_actions(server, Some(&shout), &run).await;
                self.run_actions(server, data.get("startActions"), &run).await;
            }
        }
    }

    /// The `quest` condition: where the player is with a quest (`active`, `done` or `new`).
    pub(super) async fn quest_state(&self, player: &Player, id: &str, state: &str) -> bool {
        quest::state(&self.quest_progress(player).await, id) == state
    }

    pub(super) fn quest_problems(&self) -> Vec<(String, String, String)> {
        self.active_rows("quest")
            .into_iter()
            .filter_map(|(id, data)| quest::problems(data).into_iter().next().map(|text| ("quest".to_string(), id.to_string(), text)))
            .collect()
    }
}
