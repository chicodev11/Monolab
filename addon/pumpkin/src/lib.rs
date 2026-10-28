
mod map;

use pumpkin_plugin_api::permissions;
use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::{Context, Player, Plugin, PluginMetadata, Server, register_plugin};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: &str = "32";
const NAME: &str = "monolab";
const LIVE_TICKS: u64 = 20;
const MAP_TICKS: u64 = 1;
const PERF_TICKS: u64 = 1200;
const KEEP_DAYS: i64 = 90;
const HOTSPOTS: usize = 6;
const HOTSPOT_FLOOR: u32 = 10;

struct Monolab {
    state: Arc<State>,
}

struct State {
    folder: Mutex<PathBuf>,
    offset_minutes: AtomicI64,
    swept_day: Mutex<String>,
    warned: AtomicBool,
    map: Mutex<Option<map::Map>>,
    map_warned: AtomicBool,
}

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
            }),
        }
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: NAME.into(),
            version: VERSION.into(),
            authors: vec!["Monolab".into()],
            description: "Records TPS and online players for the Monolab app's monitor.".into(),
            dependencies: vec![],
            permissions: vec![permissions::FS_WRITE_DATA.into()],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let state = Arc::clone(&self.state);
        *lock(&state.folder) = PathBuf::from(context.get_data_folder());
        state.read_timezone();
        state.sweep();
        *lock(&state.map) = Some(map::Map::new(&lock(&state.folder)));

        let live = Arc::clone(&state);
        context.schedule_repeating_task(LIVE_TICKS, LIVE_TICKS, move |server| live.snapshot(&server));
        let perf = Arc::clone(&state);
        context.schedule_repeating_task(PERF_TICKS, PERF_TICKS, move |server| perf.perf(&server));
        let map = Arc::clone(&state);
        context.schedule_repeating_task(MAP_TICKS, MAP_TICKS, move |server| map.draw(&server));

        tracing::info!("Starting the Monolab monitor: {}", lock(&state.folder).display());
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<(), String> {
        self.state.set_live(json!({ "at": now_ms(), "players": [] }));
        if let Some(map) = lock(&self.state.map).as_mut() {
            let _ = map.flush();
        }
        Ok(())
    }
}

register_plugin!(Monolab);

impl State {
    fn snapshot(&self, server: &Server) {
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
            "players": people,
        });
        if let (Value::Object(map), Value::Object(tally)) = (&mut live, entity_tally(server)) {
            map.extend(tally);
        }
        self.set_live(live);
    }

    fn draw(&self, server: &Server) {
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
        self.add(json!({
            "at": now_ms(),
            "kind": "perf",
            "tps": tps_of(server),
            "mspt": hundredths(server.get_mspt()),
            "players": server.get_player_count(),
        }));
    }
}

impl State {
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

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
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

fn world_name(level: &str, dimension: &str) -> String {
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
}
