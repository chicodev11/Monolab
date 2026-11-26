use std::io::Read;

const SECTOR: usize = 4096;
const TICKS_PER_MINUTE: i64 = 20 * 60;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    pub max_minutes: u32,
    pub spawn: (i32, i32),
    pub keep_radius: i32,
    /// When set, every chunk inside these chunk bounds (min x, min z, max x, max z) is removed and nothing else.
    pub area: Option<(i32, i32, i32, i32)>,
}

impl Rule {
    fn keeps(&self, cx: i32, cz: i32, inhabited: Option<i64>) -> bool {
        if let Some((x0, z0, x1, z1)) = self.area {
            return !(cx >= x0 && cx <= x1 && cz >= z0 && cz <= z1);
        }
        if (cx - self.spawn.0).abs() <= self.keep_radius && (cz - self.spawn.1).abs() <= self.keep_radius {
            return true;
        }
        match inhabited {
            Some(ticks) => ticks > i64::from(self.max_minutes) * TICKS_PER_MINUTE,
            None => true,
        }
    }
}

pub fn region_coords(name: &str) -> Option<(i32, i32)> {
    let mut parts = name.strip_prefix("r.")?.strip_suffix(".mca")?.split('.');
    let rx = parts.next()?.parse().ok()?;
    let rz = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((rx, rz))
}

fn location(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    let at = index * 4;
    let entry = bytes.get(at..at + 4)?;
    let offset = (usize::from(entry[0]) << 16) | (usize::from(entry[1]) << 8) | usize::from(entry[2]);
    let count = usize::from(entry[3]);
    (offset >= 2 && count > 0).then_some((offset, count))
}

fn chunk_payload(bytes: &[u8], offset: usize) -> Option<(u8, &[u8])> {
    let start = offset * SECTOR;
    let head = bytes.get(start..start + 5)?;
    let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if length == 0 {
        return None;
    }
    let data = bytes.get(start + 5..start + 4 + length)?;
    Some((head[4], data))
}

fn inhabited_time(compression: u8, data: &[u8]) -> Option<i64> {
    let mut raw = Vec::new();
    match compression {
        1 => flate2::read::GzDecoder::new(data).read_to_end(&mut raw).ok()?,
        2 => flate2::read::ZlibDecoder::new(data).read_to_end(&mut raw).ok()?,
        3 => {
            raw.extend_from_slice(data);
            raw.len()
        }
        _ => return None,
    };
    let needle = b"\x04\x00\x0DInhabitedTime";
    let at = raw.windows(needle.len()).position(|window| window == needle)? + needle.len();
    let value = raw.get(at..at + 8)?;
    Some(i64::from_be_bytes(value.try_into().ok()?))
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub total: usize,
    pub removed: Vec<usize>,
}

pub fn plan(bytes: &[u8], rx: i32, rz: i32, rule: &Rule) -> Plan {
    let mut out = Plan::default();
    if bytes.len() < SECTOR * 2 {
        return out;
    }
    for index in 0..1024 {
        let Some((offset, _)) = location(bytes, index) else { continue };
        out.total += 1;
        let cx = rx * 32 + (index % 32) as i32;
        let cz = rz * 32 + (index / 32) as i32;
        let inhabited = chunk_payload(bytes, offset)
            .filter(|(compression, _)| compression & 0x80 == 0)
            .and_then(|(compression, data)| inhabited_time(compression, data));
        if !rule.keeps(cx, cz, inhabited) {
            out.removed.push(index);
        }
    }
    out
}

pub fn strip(bytes: &[u8], removed: &[usize]) -> Option<Vec<u8>> {
    if bytes.len() < SECTOR * 2 {
        return Some(bytes.to_vec());
    }
    let mut out = vec![0u8; SECTOR * 2];
    let mut kept = 0;
    for index in 0..1024 {
        if removed.contains(&index) {
            continue;
        }
        let Some((offset, count)) = location(bytes, index) else { continue };
        let Some(sectors) = bytes.get(offset * SECTOR..(offset + count) * SECTOR) else { continue };
        let new_offset = out.len() / SECTOR;
        out.extend_from_slice(sectors);
        out[index * 4] = (new_offset >> 16) as u8;
        out[index * 4 + 1] = (new_offset >> 8) as u8;
        out[index * 4 + 2] = new_offset as u8;
        out[index * 4 + 3] = count as u8;
        out[SECTOR + index * 4..SECTOR + index * 4 + 4].copy_from_slice(&bytes[SECTOR + index * 4..SECTOR + index * 4 + 4]);
        kept += 1;
    }
    (kept > 0).then_some(out)
}

#[cfg(test)]
mod area_tests {
    use super::*;

    #[test]
    fn an_area_removes_only_what_is_inside() {
        let rule = Rule { max_minutes: 0, spawn: (0, 0), keep_radius: 50, area: Some((2, 2, 4, 4)) };
        assert!(!rule.keeps(3, 3, Some(1_000_000)));
        assert!(rule.keeps(5, 3, Some(0)));
        assert!(!rule.keeps(2, 4, None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn chunk(inhabited: i64) -> Vec<u8> {
        let mut nbt = vec![0x0A, 0, 0];
        nbt.extend_from_slice(b"\x04\x00\x0DInhabitedTime");
        nbt.extend_from_slice(&inhabited.to_be_bytes());
        nbt.push(0);
        let mut packed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        packed.write_all(&nbt).unwrap();
        packed.finish().unwrap()
    }

    fn region(chunks: &[(usize, i64)]) -> Vec<u8> {
        let mut out = vec![0u8; SECTOR * 2];
        for &(index, inhabited) in chunks {
            let data = chunk(inhabited);
            let offset = out.len() / SECTOR;
            let mut body = ((data.len() + 1) as u32).to_be_bytes().to_vec();
            body.push(2);
            body.extend_from_slice(&data);
            body.resize(SECTOR, 0);
            out.extend_from_slice(&body);
            out[index * 4 + 2] = offset as u8;
            out[index * 4 + 3] = 1;
            out[SECTOR + index * 4 + 3] = 7;
        }
        out
    }

    #[test]
    fn removes_chunks_nobody_stayed_in() {
        let bytes = region(&[(0, 0), (1, 20 * 60 * 10), (40, 5), (1023, 0)]);
        let rule = Rule { max_minutes: 1, spawn: (0, 0), keep_radius: 0, area: None };
        let found = plan(&bytes, 0, 0, &rule);
        assert_eq!(found.total, 4);
        assert_eq!(found.removed, vec![40, 1023]);
        let rule = Rule { max_minutes: 1, spawn: (100, 100), keep_radius: 0, area: None };
        let found = plan(&bytes, 0, 0, &rule);
        assert_eq!(found.removed, vec![0, 40, 1023]);

        let smaller = strip(&bytes, &found.removed).unwrap();
        assert_eq!(smaller.len(), SECTOR * 3);
        let after = plan(&smaller, 0, 0, &Rule { max_minutes: 0, spawn: (100, 100), keep_radius: 0, area: None });
        assert_eq!(after.total, 1);
        assert!(after.removed.is_empty());
        assert_eq!(smaller[SECTOR + 7], 7);
        assert!(strip(&bytes, &[0, 1, 40, 1023]).is_none());
    }

    #[test]
    #[ignore]
    fn plans_a_real_world() {
        let dir = std::env::var("SE_TRIM_DIR").expect("SE_TRIM_DIR");
        let rule = Rule { max_minutes: 0, spawn: (0, 0), keep_radius: 8, area: None };
        let (mut total, mut removed, mut before, mut after) = (0, 0, 0, 0);
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some((rx, rz)) = region_coords(&name) else { continue };
            let bytes = std::fs::read(entry.path()).unwrap();
            let found = plan(&bytes, rx, rz, &rule);
            total += found.total;
            removed += found.removed.len();
            before += bytes.len();
            after += strip(&bytes, &found.removed).map_or(0, |one| one.len());
            if let Some(smaller) = strip(&bytes, &found.removed) {
                assert_eq!(plan(&smaller, rx, rz, &rule).total, found.total - found.removed.len());
            }
        }
        println!("chunks {total} removable {removed} bytes {before} -> {after}");
        if std::env::var("SE_TRIM_APPLY").is_ok() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some((rx, rz)) = region_coords(&name) else { continue };
                let bytes = std::fs::read(entry.path()).unwrap();
                let found = plan(&bytes, rx, rz, &rule);
                match strip(&bytes, &found.removed) {
                    Some(smaller) => std::fs::write(entry.path(), smaller).unwrap(),
                    None => std::fs::remove_file(entry.path()).unwrap(),
                }
            }
        }
        assert!(total > 0);
    }

    #[test]
    fn reads_region_names() {
        assert_eq!(region_coords("r.-2.15.mca"), Some((-2, 15)));
        assert_eq!(region_coords("r.1.2.mcc"), None);
        assert_eq!(region_coords("c.1.2.mca"), None);
    }
}
