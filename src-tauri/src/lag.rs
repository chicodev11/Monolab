
use crate::events::{GameEvent, LiveState};
use serde::Serialize;

pub const LAGGY_TPS: f64 = 18.0;
pub const LAGGY_MSPT: f64 = 50.0;

pub const HOTSPOT_MEDIUM: u64 = 80;
pub const HOTSPOT_HIGH: u64 = 200;
pub const ITEMS_MEDIUM: u64 = 300;
pub const ITEMS_HIGH: u64 = 1000;
pub const ONE_KIND_MEDIUM: u64 = 400;
pub const ENTITIES_MEDIUM: u64 = 3000;
pub const ENTITIES_HIGH: u64 = 6000;
pub const CHUNKS_RATIO: f64 = 2.0;
pub const CHUNKS_FLOOR: u64 = 1500;
pub const VIEW_DISTANCE_HIGH: u32 = 12;
pub const SIMULATION_DISTANCE_HIGH: u32 = 10;
pub const HEAP_MEDIUM: f64 = 0.8;
pub const HEAP_HIGH: f64 = 0.92;
pub const CPU_HIGH: f32 = 80.0;
pub const GROWTH_WINDOW_MIN: u64 = 30;
pub const GROWTH_FLOOR: u64 = 500;

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LagFinding {
    pub kind: String,
    pub level: String,
    pub value: Option<f64>,
    pub limit: Option<f64>,
    pub world: Option<String>,
    pub x: Option<i64>,
    pub z: Option<i64>,
    pub subject: Option<String>,
}

impl LagFinding {
    fn new(kind: &str, level: &str) -> Self {
        LagFinding {
            kind: kind.into(),
            level: level.into(),
            value: None,
            limit: None,
            world: None,
            x: None,
            z: None,
            subject: None,
        }
    }

    fn value(mut self, value: f64) -> Self {
        self.value = Some(value);
        self
    }

    fn limit(mut self, limit: f64) -> Self {
        self.limit = Some(limit);
        self
    }

    fn subject(mut self, subject: &str) -> Self {
        self.subject = Some(subject.into());
        self
    }

    fn rank(&self) -> u8 {
        match self.level.as_str() {
            "high" => 0,
            "medium" => 1,
            _ => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LagDiagnosis {
    pub has_live: bool,
    pub lagging: bool,
    pub tps: Option<f64>,
    pub mspt: Option<f64>,
    pub findings: Vec<LagFinding>,
    pub spark: Option<crate::spark::SparkInfo>,
}

pub struct Input<'a> {
    pub live: Option<&'a LiveState>,
    pub max_memory_mib: u64,
    pub machine_memory_mib: u64,
    pub cpu_percent: Option<f32>,
    pub view_distance: Option<u32>,
    pub simulation_distance: Option<u32>,
    pub jvm_args: &'a str,
    pub perf: &'a [GameEvent],
    pub now_ms: u64,
}

pub fn diagnose(input: &Input) -> LagDiagnosis {
    let mut out = Vec::new();
    let live = input.live;
    let tps = live.and_then(|state| state.tps);
    let mspt = live.and_then(|state| state.mspt);
    let lagging = tps.is_some_and(|value| value < LAGGY_TPS)
        || mspt.is_some_and(|value| value > LAGGY_MSPT);

    if let Some(state) = live {
        for spot in state.hotspots.iter().filter(|spot| spot.count >= HOTSPOT_MEDIUM).take(3) {
            let level = if spot.count >= HOTSPOT_HIGH { "high" } else { "medium" };
            let mut finding = LagFinding::new("hotspot", level)
                .value(spot.count as f64)
                .limit(HOTSPOT_MEDIUM as f64);
            finding.world = spot.world.clone();
            finding.x = Some(spot.x * 16 + 8);
            finding.z = Some(spot.z * 16 + 8);
            finding.subject = spot.top.clone();
            out.push(finding);
        }

        let count_of = |kind: &str| {
            state
                .entity_kinds
                .iter()
                .find(|one| one.r#type == kind)
                .map(|one| one.count)
                .unwrap_or(0)
        };
        let items = count_of("minecraft:item");
        let items_in_hotspots: u64 = state
            .hotspots
            .iter()
            .filter(|spot| spot.top.as_deref() == Some("minecraft:item") && spot.count >= HOTSPOT_MEDIUM)
            .map(|spot| spot.count)
            .sum();
        if items >= ITEMS_MEDIUM && items_in_hotspots * 10 < items * 8 {
            let level = if items >= ITEMS_HIGH { "high" } else { "medium" };
            out.push(
                LagFinding::new("items", level)
                    .value(items as f64)
                    .limit(ITEMS_MEDIUM as f64)
                    .subject("minecraft:item"),
            );
        }

        if let Some(kind) = state
            .entity_kinds
            .iter()
            .filter(|one| {
                one.r#type != "minecraft:item"
                    && one.r#type != "minecraft:player"
                    && one.r#type != "minecraft:experience_orb"
            })
            .max_by_key(|one| one.count)
        {
            if kind.count >= ONE_KIND_MEDIUM {
                out.push(
                    LagFinding::new("one-kind", "medium")
                        .value(kind.count as f64)
                        .limit(ONE_KIND_MEDIUM as f64)
                        .subject(&kind.r#type),
                );
            }
        }

        if let Some(total) = state.entities {
            if total >= ENTITIES_MEDIUM {
                let level = if total >= ENTITIES_HIGH { "high" } else { "medium" };
                out.push(
                    LagFinding::new("entities", level)
                        .value(total as f64)
                        .limit(ENTITIES_MEDIUM as f64),
                );
            }
        }

        if let Some(chunks) = state.chunks {
            let players = state.players.len().max(1) as f64;
            let radius = input.view_distance.unwrap_or(10) as f64;
            let expected = players * (2.0 * radius + 1.0).powi(2);
            if chunks >= CHUNKS_FLOOR && chunks as f64 > expected * CHUNKS_RATIO {
                out.push(
                    LagFinding::new("chunks", "medium")
                        .value(chunks as f64)
                        .limit(expected.round()),
                );
            }
        }

        if let (Some(used), Some(max)) = (state.memory_mib, state.max_memory_mib) {
            if max > 0 {
                let ratio = used as f64 / max as f64;
                if ratio >= HEAP_MEDIUM {
                    let level = if ratio >= HEAP_HIGH { "high" } else { "medium" };
                    out.push(
                        LagFinding::new("heap", level)
                            .value((ratio * 100.0).round())
                            .limit(max as f64),
                    );
                }
            }
        }

        if let (Some(now), Some(then)) = (
            state.entities,
            oldest_within(input.perf, input.now_ms, GROWTH_WINDOW_MIN * 60_000)
                .and_then(|event| event.entities),
        ) {
            if now >= then.saturating_mul(2) && now.saturating_sub(then) >= GROWTH_FLOOR {
                out.push(
                    LagFinding::new("entity-growth", "medium")
                        .value(now as f64)
                        .limit(then as f64),
                );
            }
        }
    }

    if lagging {
        if let Some(distance) = input.view_distance {
            if distance >= VIEW_DISTANCE_HIGH {
                out.push(
                    LagFinding::new("view-distance", "low")
                        .value(distance as f64)
                        .limit(10.0),
                );
            }
        }
        if let Some(distance) = input.simulation_distance {
            if distance >= SIMULATION_DISTANCE_HIGH {
                out.push(
                    LagFinding::new("simulation-distance", "low")
                        .value(distance as f64)
                        .limit(8.0),
                );
            }
        }
    }

    if input.machine_memory_mib > 0
        && input.max_memory_mib > input.machine_memory_mib.saturating_sub(2048)
    {
        out.push(
            LagFinding::new("machine-memory", "medium")
                .value(input.max_memory_mib as f64)
                .limit(input.machine_memory_mib as f64),
        );
    }

    if let Some(cpu) = input.cpu_percent {
        if cpu >= CPU_HIGH {
            out.push(LagFinding::new("cpu", "medium").value(cpu as f64).limit(CPU_HIGH as f64));
        }
    }

    if lagging && input.max_memory_mib >= 4096 && !input.jvm_args.contains("MaxGCPauseMillis") {
        out.push(LagFinding::new("aikar", "low"));
    }

    out.sort_by_key(LagFinding::rank);
    LagDiagnosis {
        has_live: live.is_some(),
        lagging,
        tps,
        mspt,
        findings: out,
        spark: None,
    }
}

fn oldest_within(perf: &[GameEvent], now_ms: u64, window_ms: u64) -> Option<&GameEvent> {
    perf.iter()
        .find(|event| event.entities.is_some() && now_ms.saturating_sub(event.at) <= window_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EntityKind, Hotspot};

    fn quiet() -> LiveState {
        LiveState {
            at: 0,
            tps: Some(20.0),
            mspt: Some(12.0),
            memory_mib: Some(1200),
            max_memory_mib: Some(4096),
            chunks: Some(600),
            entities: Some(300),
            entity_kinds: vec![EntityKind { r#type: "minecraft:cow".into(), count: 40 }],
            hotspots: Vec::new(),
            worlds: Vec::new(),
            players: Vec::new(),
            jvm: None,
        }
    }

    fn input<'a>(live: Option<&'a LiveState>, perf: &'a [GameEvent]) -> Input<'a> {
        Input {
            live,
            max_memory_mib: 4096,
            machine_memory_mib: 32_768,
            cpu_percent: Some(12.0),
            view_distance: Some(10),
            simulation_distance: Some(8),
            jvm_args: "-XX:+UseG1GC",
            perf,
            now_ms: 1_000_000_000,
        }
    }

    fn kinds(found: &LagDiagnosis) -> Vec<&str> {
        found.findings.iter().map(|one| one.kind.as_str()).collect()
    }

    #[test]
    fn a_quiet_server_has_nothing_to_say() {
        let live = quiet();
        let found = diagnose(&input(Some(&live), &[]));
        assert!(found.has_live);
        assert!(!found.lagging);
        assert!(found.findings.is_empty(), "{:?}", found.findings);
    }

    #[test]
    fn points_at_the_mob_farm_first() {
        let mut live = quiet();
        live.tps = Some(9.5);
        live.entities = Some(3500);
        live.entity_kinds = vec![
            EntityKind { r#type: "minecraft:zombie".into(), count: 2100 },
            EntityKind { r#type: "minecraft:item".into(), count: 350 },
        ];
        live.hotspots = vec![Hotspot {
            world: Some("world".into()),
            x: 12,
            z: -4,
            count: 1900,
            top: Some("minecraft:zombie".into()),
        }];
        let found = diagnose(&input(Some(&live), &[]));
        assert!(found.lagging);
        assert_eq!(kinds(&found), vec!["hotspot", "items", "one-kind", "entities", "aikar"]);
        let spot = &found.findings[0];
        assert_eq!(spot.level, "high");
        assert_eq!((spot.x, spot.z), (Some(12 * 16 + 8), Some(-4 * 16 + 8)));
        assert_eq!(spot.subject.as_deref(), Some("minecraft:zombie"));
    }

    #[test]
    fn sees_the_heap_and_the_machine() {
        let mut live = quiet();
        live.memory_mib = Some(3900);
        let mut given = input(Some(&live), &[]);
        given.max_memory_mib = 31_000;
        given.cpu_percent = Some(91.0);
        let found = diagnose(&given);
        assert!(!found.lagging);
        assert_eq!(kinds(&found), vec!["heap", "machine-memory", "cpu"]);
        assert_eq!(found.findings[0].level, "high");
        assert_eq!(found.findings[0].value, Some(95.0));
    }

    #[test]
    fn notices_entities_doubling_within_half_an_hour() {
        let mut live = quiet();
        live.entities = Some(2400);
        let perf = |at: u64, entities: u64| GameEvent {
            at,
            kind: "perf".into(),
            entities: Some(entities),
            ..Default::default()
        };
        let now = 1_000_000_000u64;
        let rows = vec![perf(now - 40 * 60_000, 100), perf(now - 25 * 60_000, 800), perf(now - 60_000, 2300)];
        let found = diagnose(&input(Some(&live), &rows));
        assert_eq!(kinds(&found), vec!["entity-growth"]);
        assert_eq!(found.findings[0].limit, Some(800.0));
        let old = vec![GameEvent { at: now - 60_000, kind: "perf".into(), ..Default::default() }];
        assert!(diagnose(&input(Some(&live), &old)).findings.is_empty());
    }

    #[test]
    fn does_not_say_item_pile_twice() {
        let mut live = quiet();
        live.entities = Some(420);
        live.entity_kinds = vec![EntityKind { r#type: "minecraft:item".into(), count: 406 }];
        live.hotspots = vec![Hotspot {
            world: Some("world".into()),
            x: 0,
            z: 0,
            count: 406,
            top: Some("minecraft:item".into()),
        }];
        assert_eq!(kinds(&diagnose(&input(Some(&live), &[]))), vec!["hotspot"]);
        live.entity_kinds[0].count = 900;
        assert_eq!(kinds(&diagnose(&input(Some(&live), &[]))), vec!["hotspot", "items"]);
    }

    #[test]
    fn only_nags_about_distances_when_it_is_actually_slow() {
        let mut given = input(None, &[]);
        given.view_distance = Some(16);
        given.simulation_distance = Some(12);
        let found = diagnose(&given);
        assert!(!found.has_live);
        assert!(found.findings.is_empty());

        let mut live = quiet();
        live.tps = Some(14.0);
        live.chunks = Some(4000);
        let mut given = input(Some(&live), &[]);
        given.view_distance = Some(16);
        given.simulation_distance = Some(12);
        let found = diagnose(&given);
        assert_eq!(kinds(&found), vec!["chunks", "view-distance", "simulation-distance", "aikar"]);
        assert_eq!(found.findings[0].limit, Some(1089.0));
    }
}
