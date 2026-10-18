use serde::de::IgnoredAny;
use serde::Deserialize;
use std::io::Read;

pub const METRICS: [&str; 7] = ["inhabited", "updated", "entities", "blockEntities", "size", "status", "version"];

pub const STATUSES: [&str; 14] = [
    "empty",
    "structure_starts",
    "structure_references",
    "biomes",
    "noise",
    "surface",
    "carvers",
    "liquid_carvers",
    "features",
    "initialize_light",
    "light",
    "spawn",
    "heightmaps",
    "full",
];

const SECTOR: usize = 4096;

pub type Grid = Vec<Option<f64>>;

#[derive(Clone, Debug, Default)]
pub struct RegionStats {
    pub inhabited: Grid,
    pub updated: Grid,
    pub block_entities: Grid,
    pub size: Grid,
    pub status: Grid,
    pub version: Grid,
}

impl RegionStats {
    pub fn get(&self, metric: &str) -> Option<&Grid> {
        Some(match metric {
            "inhabited" => &self.inhabited,
            "updated" => &self.updated,
            "blockEntities" => &self.block_entities,
            "size" => &self.size,
            "status" => &self.status,
            "version" => &self.version,
            _ => return None,
        })
    }
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(rename = "DataVersion")]
    data_version: Option<i32>,
    #[serde(rename = "InhabitedTime")]
    inhabited: Option<i64>,
    #[serde(rename = "Status")]
    status: Option<String>,
    block_entities: Option<Vec<IgnoredAny>>,
    #[serde(rename = "Level")]
    level: Option<OldLevel>,
}

#[derive(Deserialize)]
struct OldLevel {
    #[serde(rename = "InhabitedTime")]
    inhabited: Option<i64>,
    #[serde(rename = "Status")]
    status: Option<String>,
    #[serde(rename = "TileEntities")]
    tile_entities: Option<Vec<IgnoredAny>>,
}

#[derive(Deserialize)]
struct EntityChunk {
    #[serde(rename = "Entities")]
    entities: Option<Vec<IgnoredAny>>,
}

fn location(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    let entry = bytes.get(index * 4..index * 4 + 4)?;
    let offset = (usize::from(entry[0]) << 16) | (usize::from(entry[1]) << 8) | usize::from(entry[2]);
    let count = usize::from(entry[3]);
    (offset >= 2 && count > 0).then_some((offset, count))
}

fn timestamp(bytes: &[u8], index: usize) -> Option<u32> {
    let at = SECTOR + index * 4;
    let raw = bytes.get(at..at + 4)?;
    let value = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
    (value > 0).then_some(value)
}

fn payload(bytes: &[u8], offset: usize) -> Option<Vec<u8>> {
    let start = offset * SECTOR;
    let head = bytes.get(start..start + 5)?;
    let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if length < 1 {
        return None;
    }
    let data = bytes.get(start + 5..start + 4 + length)?;
    let mut raw = Vec::new();
    match head[4] {
        1 => {
            flate2::read::GzDecoder::new(data).read_to_end(&mut raw).ok()?;
        }
        2 => {
            flate2::read::ZlibDecoder::new(data).read_to_end(&mut raw).ok()?;
        }
        3 => raw.extend_from_slice(data),
        _ => return None,
    }
    Some(raw)
}

fn status_index(name: &str) -> Option<f64> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    STATUSES.iter().position(|one| *one == name).map(|at| at as f64)
}

pub fn read_region(bytes: &[u8]) -> RegionStats {
    let mut out = RegionStats {
        inhabited: vec![None; 1024],
        updated: vec![None; 1024],
        block_entities: vec![None; 1024],
        size: vec![None; 1024],
        status: vec![None; 1024],
        version: vec![None; 1024],
    };
    if bytes.len() < SECTOR * 2 {
        return out;
    }
    for index in 0..1024 {
        let Some((offset, count)) = location(bytes, index) else { continue };
        out.size[index] = Some((count * SECTOR) as f64);
        out.updated[index] = timestamp(bytes, index).map(f64::from);
        let Some(raw) = payload(bytes, offset) else { continue };
        let Ok(chunk) = fastnbt::from_bytes::<Chunk>(&raw) else { continue };
        let old = chunk.level.as_ref();
        out.version[index] = chunk.data_version.map(f64::from);
        out.inhabited[index] = chunk.inhabited.or(old.and_then(|one| one.inhabited)).map(|ticks| ticks as f64 / 1200.0);
        out.status[index] = chunk.status.as_deref().or(old.and_then(|one| one.status.as_deref())).and_then(status_index);
        out.block_entities[index] = chunk
            .block_entities
            .as_ref()
            .or(old.and_then(|one| one.tile_entities.as_ref()))
            .map(|list| list.len() as f64);
    }
    out
}

pub fn read_entities(bytes: &[u8]) -> Grid {
    let mut out = vec![None; 1024];
    if bytes.len() < SECTOR * 2 {
        return out;
    }
    for (index, slot) in out.iter_mut().enumerate() {
        let Some((offset, _)) = location(bytes, index) else { continue };
        let Some(raw) = payload(bytes, offset) else { continue };
        if let Ok(chunk) = fastnbt::from_bytes::<EntityChunk>(&raw) {
            *slot = Some(chunk.entities.map_or(0, |list| list.len()) as f64);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn region(chunks: &[(usize, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0u8; SECTOR * 2];
        for (index, nbt) in chunks {
            let mut packed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            packed.write_all(nbt).unwrap();
            let data = packed.finish().unwrap();
            let offset = out.len() / SECTOR;
            let mut body = ((data.len() + 1) as u32).to_be_bytes().to_vec();
            body.push(2);
            body.extend_from_slice(&data);
            body.resize(SECTOR, 0);
            out.extend_from_slice(&body);
            out[index * 4 + 2] = offset as u8;
            out[index * 4 + 3] = 1;
            out[SECTOR + index * 4..SECTOR + index * 4 + 4].copy_from_slice(&1_700_000_000u32.to_be_bytes());
        }
        out
    }

    #[derive(serde::Serialize)]
    struct Sample {
        #[serde(rename = "DataVersion")]
        data_version: i32,
        #[serde(rename = "InhabitedTime")]
        inhabited: i64,
        #[serde(rename = "Status")]
        status: String,
        block_entities: Vec<fastnbt::Value>,
    }

    #[derive(serde::Serialize)]
    struct EntitySample {
        #[serde(rename = "Entities")]
        entities: Vec<fastnbt::Value>,
    }

    #[test]
    fn reads_chunk_stats_from_a_region() {
        let nbt = fastnbt::to_bytes(&Sample {
            data_version: 3953,
            inhabited: 2400,
            status: "minecraft:full".into(),
            block_entities: vec![fastnbt::Value::Byte(1), fastnbt::Value::Byte(2)],
        })
        .unwrap();
        let bytes = region(&[(5, nbt)]);
        let stats = read_region(&bytes);
        assert_eq!(stats.inhabited[5], Some(2.0));
        assert_eq!(stats.version[5], Some(3953.0));
        assert_eq!(stats.status[5], Some((STATUSES.len() - 1) as f64));
        assert_eq!(stats.block_entities[5], Some(2.0));
        assert_eq!(stats.size[5], Some(4096.0));
        assert_eq!(stats.updated[5], Some(1_700_000_000.0));
        assert_eq!(stats.inhabited[6], None);

        let entities = fastnbt::to_bytes(&EntitySample { entities: vec![fastnbt::Value::Int(1); 3] }).unwrap();
        let grid = read_entities(&region(&[(7, entities)]));
        assert_eq!(grid[7], Some(3.0));
        assert_eq!(grid[8], None);
    }
}

#[cfg(test)]
mod real {
    #[test]
    #[ignore]
    fn reads_a_real_region() {
        let dir = std::env::var("SE_STATS_DIR").expect("SE_STATS_DIR");
        let name = std::env::var("SE_STATS_NAME").unwrap_or_else(|_| "r.0.0.mca".into());
        let stats = super::read_region(&std::fs::read(format!("{dir}/region/{name}")).unwrap());
        for metric in ["inhabited", "updated", "blockEntities", "size", "status", "version"] {
            let grid = stats.get(metric).unwrap();
            let some: Vec<f64> = grid.iter().flatten().copied().collect();
            let max = some.iter().cloned().fold(f64::MIN, f64::max);
            println!("{metric}: {} chunks, max {max}", some.len());
        }
        if let Ok(bytes) = std::fs::read(format!("{dir}/entities/{name}")) {
            let grid = super::read_entities(&bytes);
            println!("entities: {} chunks, total {}", grid.iter().flatten().count(), grid.iter().flatten().sum::<f64>());
        }
    }
}
