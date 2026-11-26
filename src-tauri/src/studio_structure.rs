//! Turns a `.schem` (Sponge schematic v1–v3, WorldEdit's) or `.nbt` (vanilla structure block) file into the plain
//! block list both add-ons read (`studio_format::structure`). Air and structure voids are left out.

use fastnbt::Value as Nbt;
use std::collections::HashMap;
use studio_format::structure::{MOST_BLOCKS, Structure};

fn int(value: Option<&Nbt>) -> Option<i64> {
    match value? {
        Nbt::Byte(one) => Some(i64::from(*one)),
        Nbt::Short(one) => Some(i64::from(*one)),
        Nbt::Int(one) => Some(i64::from(*one)),
        Nbt::Long(one) => Some(*one),
        _ => None,
    }
}

fn compound(value: Option<&Nbt>) -> Option<&HashMap<String, Nbt>> {
    match value? {
        Nbt::Compound(map) => Some(map),
        _ => None,
    }
}

fn list(value: Option<&Nbt>) -> &[Nbt] {
    match value {
        Some(Nbt::List(list)) => list,
        _ => &[],
    }
}

fn is_air(state: &str) -> bool {
    let name = state.split('[').next().unwrap_or_default().trim_start_matches("minecraft:");
    matches!(name, "air" | "cave_air" | "void_air" | "structure_void")
}

fn short(state: &str) -> String {
    state.strip_prefix("minecraft:").unwrap_or(state).to_string()
}

/// Reads either kind of file (gzip or not).
pub fn read(bytes: Vec<u8>, file_name: &str) -> Result<Structure, String> {
    let root = crate::nbt::decode_nbt(bytes)?;
    let Nbt::Compound(root) = root else { return Err("That file isn't a structure.".into()) };
    let lower = file_name.to_ascii_lowercase();
    let out = if lower.ends_with(".nbt") || root.contains_key("palette") || root.contains_key("palettes") {
        vanilla(&root)?
    } else {
        sponge(compound(root.get("Schematic")).unwrap_or(&root))?
    };
    if out.blocks.is_empty() {
        return Err("The structure has no blocks (only air).".into());
    }
    Ok(out)
}

fn vanilla(root: &HashMap<String, Nbt>) -> Result<Structure, String> {
    let size: Vec<i64> = list(root.get("size")).iter().filter_map(|one| int(Some(one))).collect();
    if size.len() != 3 {
        return Err("The .nbt file has no size.".into());
    }
    let palette_list = if root.contains_key("palette") { list(root.get("palette")) } else { list(list(root.get("palettes")).first()) };
    let mut palette = Vec::new();
    for one in palette_list {
        let entry = compound(Some(one)).ok_or("The .nbt palette is broken.")?;
        let name = match entry.get("Name") {
            Some(Nbt::String(name)) => short(name),
            _ => "air".into(),
        };
        let props: Vec<String> = compound(entry.get("Properties"))
            .map(|map| {
                let mut pairs: Vec<String> = map.iter().filter_map(|(key, value)| if let Nbt::String(value) = value { Some(format!("{key}={value}")) } else { None }).collect();
                pairs.sort();
                pairs
            })
            .unwrap_or_default();
        palette.push(if props.is_empty() { name } else { format!("{name}[{}]", props.join(",")) });
    }
    let mut blocks = Vec::new();
    for one in list(root.get("blocks")) {
        let Some(block) = compound(Some(one)) else { continue };
        let pos: Vec<i64> = list(block.get("pos")).iter().filter_map(|one| int(Some(one))).collect();
        let Some(state) = int(block.get("state")) else { continue };
        if pos.len() != 3 || state < 0 || state as usize >= palette.len() || is_air(&palette[state as usize]) {
            continue;
        }
        blocks.push((pos[0] as i32, pos[1] as i32, pos[2] as i32, state as u32));
        if blocks.len() > MOST_BLOCKS {
            return Err(format!("It has over {MOST_BLOCKS} blocks; that's too big to place."));
        }
    }
    Ok(Structure { size: (size[0] as i32, size[1] as i32, size[2] as i32), palette, blocks })
}

fn sponge(root: &HashMap<String, Nbt>) -> Result<Structure, String> {
    let (w, h, l) = match (int(root.get("Width")), int(root.get("Height")), int(root.get("Length"))) {
        (Some(w), Some(h), Some(l)) => (w as i32 & 0xFFFF, h as i32 & 0xFFFF, l as i32 & 0xFFFF),
        _ => return Err("The .schem file has no size.".into()),
    };
    // v3 keeps blocks under "Blocks" (Palette + Data), v1/v2 at the top (Palette + BlockData).
    let (palette_map, data) = match compound(root.get("Blocks")) {
        Some(blocks) => (compound(blocks.get("Palette")), blocks.get("Data")),
        None => (compound(root.get("Palette")), root.get("BlockData")),
    };
    let palette_map = palette_map.ok_or("The .schem file has no palette. Old MCEdit .schematic files aren't supported; save it again with WorldEdit as .schem.")?;
    let mut palette = vec![String::from("air"); palette_map.len()];
    for (state, index) in palette_map {
        let Some(index) = int(Some(index)) else { continue };
        if index >= 0 && (index as usize) < palette.len() {
            palette[index as usize] = short(state);
        }
    }
    let bytes: Vec<u8> = match data {
        Some(Nbt::ByteArray(array)) => array.iter().map(|one| *one as u8).collect(),
        _ => return Err("The .schem file has no block data.".into()),
    };
    let mut blocks = Vec::new();
    let (mut at, mut index) = (0usize, 0i64);
    while at < bytes.len() {
        // Varints, one per block, x fastest, then z, then y.
        let (mut value, mut shift) = (0u32, 0);
        loop {
            let byte = *bytes.get(at).ok_or("The .schem block data is cut short.")?;
            at += 1;
            value |= u32::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return Err("The .schem block data is broken.".into());
            }
        }
        let (x, z, y) = ((index % i64::from(w)) as i32, ((index / i64::from(w)) % i64::from(l)) as i32, (index / (i64::from(w) * i64::from(l))) as i32);
        index += 1;
        if (value as usize) < palette.len() && !is_air(&palette[value as usize]) && y < h {
            blocks.push((x, y, z, value));
            if blocks.len() > MOST_BLOCKS {
                return Err(format!("It has over {MOST_BLOCKS} blocks; that's too big to place."));
            }
        }
    }
    Ok(Structure { size: (w, h, l), palette, blocks })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nbt_bytes(value: &Nbt) -> Vec<u8> {
        fastnbt::to_bytes(value).unwrap()
    }

    #[test]
    fn reads_vanilla_and_sponge() {
        let mut props = HashMap::new();
        props.insert("facing".to_string(), Nbt::String("east".into()));
        let palette = Nbt::List(vec![
            Nbt::Compound(HashMap::from([("Name".to_string(), Nbt::String("minecraft:air".into()))])),
            Nbt::Compound(HashMap::from([("Name".to_string(), Nbt::String("minecraft:oak_stairs".into())), ("Properties".to_string(), Nbt::Compound(props))])),
        ]);
        let block = |x: i32, state: i32| Nbt::Compound(HashMap::from([("pos".to_string(), Nbt::List(vec![Nbt::Int(x), Nbt::Int(0), Nbt::Int(0)])), ("state".to_string(), Nbt::Int(state))]));
        let root = Nbt::Compound(HashMap::from([
            ("size".to_string(), Nbt::List(vec![Nbt::Int(2), Nbt::Int(1), Nbt::Int(1)])),
            ("palette".to_string(), palette),
            ("blocks".to_string(), Nbt::List(vec![block(0, 0), block(1, 1)])),
        ]));
        let read = read(nbt_bytes(&root), "house.nbt").unwrap();
        assert_eq!(read.size, (2, 1, 1));
        assert_eq!(read.blocks, vec![(1, 0, 0, 1)]);
        assert_eq!(read.palette[1], "oak_stairs[facing=east]");

        let sponge_palette = HashMap::from([("minecraft:air".to_string(), Nbt::Int(0)), ("minecraft:stone".to_string(), Nbt::Int(1))]);
        let schem = Nbt::Compound(HashMap::from([(
            "Schematic".to_string(),
            Nbt::Compound(HashMap::from([
                ("Width".to_string(), Nbt::Short(2)),
                ("Height".to_string(), Nbt::Short(1)),
                ("Length".to_string(), Nbt::Short(2)),
                ("Blocks".to_string(), Nbt::Compound(HashMap::from([("Palette".to_string(), Nbt::Compound(sponge_palette)), ("Data".to_string(), Nbt::ByteArray(fastnbt::ByteArray::new(vec![1, 0, 0, 1])))]))),
            ])),
        )]));
        let read = super::read(nbt_bytes(&schem), "hut.schem").unwrap();
        assert_eq!(read.size, (2, 1, 2));
        assert_eq!(read.blocks.len(), 2);
        assert_eq!(read.blocks[1], (1, 0, 1, 1));
        assert!(super::read(nbt_bytes(&Nbt::Compound(HashMap::new())), "x.schem").is_err());
    }
}
