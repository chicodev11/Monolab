//! Quests (`quest` files): steps a player goes through in order (kill, collect, break, place, craft, fish, talk to a
//! studio NPC, go to a spot), then `rewards` (an action list) and, if set, a studio advancement. The `quest` action
//! starts, abandons or completes one; the `quest` condition checks where a player is with it. Progress is kept on
//! the player (`monolab:quests`) as JSON, so it survives rejoins and restarts. Both add-ons share this file's rules.

use crate::fields;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

type Data = Map<String, Value>;

pub const STEP_TYPES: [&str; 8] = ["kill", "collect", "break", "place", "craft", "fish", "talk", "visit"];
pub const MODES: [&str; 3] = ["start", "abandon", "complete"];
pub const STATES: [&str; 3] = ["active", "done", "new"];
/// The player's data key.
pub const KEY: &str = "quests";
pub const MOST_STEPS: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub kind: String,
    /// What counts, lower case without `minecraft:`; several split by commas; empty = anything. A studio mob is
    /// `studio:<id>`, an NPC its file id.
    pub targets: Vec<String>,
    pub amount: u32,
    pub text: String,
    pub world: String,
    pub spot: (f64, f64, f64),
    pub radius: f64,
}

fn clean(text: &str) -> String {
    text.trim().trim_start_matches("minecraft:").to_ascii_lowercase()
}

pub fn steps(data: &Data) -> Vec<Step> {
    let Some(list) = data.get("steps").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(Value::as_object)
        .take(MOST_STEPS)
        .map(|one| {
            let kind = fields::choice(one, "type", &STEP_TYPES, "kill");
            let targets = fields::text(one, "target", "").split(',').map(clean).filter(|one| !one.is_empty()).collect();
            let amount = if matches!(kind.as_str(), "talk" | "visit") { 1 } else { fields::number(one, "amount", 1, 1, 100_000) as u32 };
            Step {
                kind,
                targets,
                amount,
                text: fields::text(one, "text", "").trim().to_string(),
                world: fields::text(one, "world", "").trim().to_string(),
                spot: (crate::item::float(one, "x", 0.0, -3.0e7, 3.0e7), crate::item::float(one, "y", 64.0, -2048.0, 4096.0), crate::item::float(one, "z", 0.0, -3.0e7, 3.0e7)),
                radius: crate::item::float(one, "radius", 5.0, 1.0, 512.0),
            }
        })
        .collect()
}

/// What the player sees for a step: its own `text`, or one made from the step.
pub fn step_text(step: &Step) -> String {
    if !step.text.is_empty() {
        return step.text.clone();
    }
    let what = if step.targets.is_empty() { "anything".to_string() } else { step.targets.join(" or ").replace('_', " ") };
    match step.kind.as_str() {
        "kill" => format!("Kill {} {what}", step.amount),
        "collect" => format!("Collect {} {what}", step.amount),
        "break" => format!("Break {} {what}", step.amount),
        "place" => format!("Place {} {what}", step.amount),
        "craft" => format!("Craft {} {what}", step.amount),
        "fish" => format!("Catch {} {what}", step.amount),
        "talk" => format!("Talk to {what}"),
        _ => format!("Go to {:.0} {:.0} {:.0}", step.spot.0, step.spot.1, step.spot.2),
    }
}

pub fn name(data: &Data, id: &str) -> String {
    let name = fields::text(data, "name", "").trim().to_string();
    if name.is_empty() { id.replace(['-', '_'], " ") } else { name }
}

pub fn repeatable(data: &Data) -> bool {
    data.get("repeatable").and_then(Value::as_bool) == Some(true)
}

pub fn advancement(data: &Data) -> String {
    fields::text(data, "advancement", "").trim().to_string()
}

/// Where one player is with every quest.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// quest id → (step index, count on that step).
    pub active: BTreeMap<String, (usize, u32)>,
    pub done: BTreeSet<String>,
}

pub fn parse(text: &str) -> Progress {
    let Ok(value) = serde_json::from_str::<Value>(text) else { return Progress::default() };
    let mut out = Progress::default();
    if let Some(active) = value.get("active").and_then(Value::as_object) {
        for (id, at) in active {
            let step = at.get(0).and_then(Value::as_u64).unwrap_or(0) as usize;
            let count = at.get(1).and_then(Value::as_u64).unwrap_or(0) as u32;
            if crate::valid_id(id) {
                out.active.insert(id.clone(), (step, count));
            }
        }
    }
    if let Some(done) = value.get("done").and_then(Value::as_array) {
        out.done.extend(done.iter().filter_map(Value::as_str).filter(|id| crate::valid_id(id)).map(str::to_string));
    }
    out
}

pub fn write(progress: &Progress) -> String {
    let active: Map<String, Value> = progress.active.iter().map(|(id, (step, count))| (id.clone(), json!([step, count]))).collect();
    json!({ "active": active, "done": progress.done }).to_string()
}

pub fn state(progress: &Progress, id: &str) -> &'static str {
    if progress.active.contains_key(id) {
        "active"
    } else if progress.done.contains(id) {
        "done"
    } else {
        "new"
    }
}

/// Starts a quest unless it runs already, or was done and isn't repeatable. True when it started.
pub fn start(progress: &mut Progress, id: &str, data: &Data) -> bool {
    if progress.active.contains_key(id) || (progress.done.contains(id) && !repeatable(data)) || steps(data).is_empty() {
        return false;
    }
    progress.active.insert(id.to_string(), (0, 0));
    true
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Counted on the current step: (count, amount, step text).
    Counted(u32, u32, String),
    /// A step finished; the text of the next one.
    Next(String),
    /// The last step finished: the quest is done.
    Finished,
}

/// Marks a quest done (the `complete` mode, or its last step).
pub fn finish(progress: &mut Progress, id: &str) {
    progress.active.remove(id);
    progress.done.insert(id.to_string());
}

fn matches(step: &Step, target: &str) -> bool {
    step.targets.is_empty() || step.targets.iter().any(|one| one == &clean(target))
}

/// Something happened to the player (`kind` is a step type, `target` what it was, `amount` how many). Every active
/// quest whose current step it fits moves on. `quest` gives a quest file by id.
pub fn record(progress: &mut Progress, quest: &dyn Fn(&str) -> Option<Data>, kind: &str, target: &str, amount: u32) -> Vec<(String, Change)> {
    advance(progress, quest, |step| step.kind == kind && matches(step, target), amount)
}

/// The player stands at (x, y, z) in `world`: `visit` steps there finish.
pub fn visit(progress: &mut Progress, quest: &dyn Fn(&str) -> Option<Data>, world: &str, at: (f64, f64, f64)) -> Vec<(String, Change)> {
    advance(
        progress,
        quest,
        |step| {
            step.kind == "visit"
                && (step.world.is_empty() || step.world.eq_ignore_ascii_case(world))
                && ((step.spot.0 - at.0).powi(2) + (step.spot.1 - at.1).powi(2) + (step.spot.2 - at.2).powi(2)).sqrt() <= step.radius
        },
        1,
    )
}

fn advance(progress: &mut Progress, quest: &dyn Fn(&str) -> Option<Data>, fits: impl Fn(&Step) -> bool, amount: u32) -> Vec<(String, Change)> {
    let mut out = Vec::new();
    let ids: Vec<String> = progress.active.keys().cloned().collect();
    for id in ids {
        let Some(data) = quest(&id) else { continue };
        let list = steps(&data);
        let Some(&(at, count)) = progress.active.get(&id) else { continue };
        let Some(step) = list.get(at) else {
            // The file lost steps since: whatever is left counts as done.
            finish(progress, &id);
            out.push((id, Change::Finished));
            continue;
        };
        if !fits(step) {
            continue;
        }
        let count = count.saturating_add(amount.max(1)).min(step.amount);
        if count < step.amount {
            progress.active.insert(id.clone(), (at, count));
            out.push((id, Change::Counted(count, step.amount, step_text(step))));
        } else if let Some(next) = list.get(at + 1) {
            progress.active.insert(id.clone(), (at + 1, 0));
            out.push((id, Change::Next(step_text(next))));
        } else {
            finish(progress, &id);
            out.push((id, Change::Finished));
        }
    }
    out
}

/// True when some active quest waits on a `visit` step (so the add-on checks positions only then).
pub fn wants_visit(progress: &Progress, quest: &dyn Fn(&str) -> Option<Data>) -> bool {
    progress.active.iter().any(|(id, (at, _))| quest(id).is_some_and(|data| steps(&data).get(*at).is_some_and(|step| step.kind == "visit")))
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let list = steps(data);
    if list.is_empty() {
        out.push("A quest needs \"steps\": what players do, in order.".into());
    }
    for (at, step) in list.iter().enumerate() {
        if step.kind == "talk" && step.targets.is_empty() {
            out.push(format!("Step {} needs the NPC to talk to (\"target\").", at + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_go_in_order_and_finish() {
        let data: Data = serde_json::from_str(r#"{"steps":[{"type":"kill","target":"minecraft:zombie, husk","amount":2},{"type":"talk","target":"elder"},{"type":"visit","world":"world","x":10,"y":64,"z":0,"radius":3}]}"#).unwrap();
        let lookup = |id: &str| (id == "hunt").then(|| data.clone());
        let mut progress = Progress::default();
        assert!(start(&mut progress, "hunt", &data));
        assert!(!start(&mut progress, "hunt", &data));
        assert!(record(&mut progress, &lookup, "kill", "creeper", 1).is_empty());
        assert_eq!(record(&mut progress, &lookup, "kill", "Zombie", 1), vec![("hunt".into(), Change::Counted(1, 2, "Kill 2 zombie or husk".into()))]);
        assert_eq!(record(&mut progress, &lookup, "kill", "husk", 1), vec![("hunt".into(), Change::Next("Talk to elder".into()))]);
        assert_eq!(record(&mut progress, &lookup, "talk", "elder", 1).len(), 1);
        assert!(wants_visit(&progress, &lookup));
        assert!(visit(&mut progress, &lookup, "world", (0.0, 64.0, 0.0)).is_empty());
        assert_eq!(visit(&mut progress, &lookup, "world", (9.0, 64.0, 1.0)), vec![("hunt".into(), Change::Finished)]);
        assert_eq!(state(&progress, "hunt"), "done");
        assert!(!start(&mut progress, "hunt", &data));
        let back = parse(&write(&progress));
        assert_eq!(back, progress);
        assert_eq!(problems(&serde_json::from_str(r#"{"steps":[{"type":"talk"}]}"#).unwrap()).len(), 1);
    }
}
