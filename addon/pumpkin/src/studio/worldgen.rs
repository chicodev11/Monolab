//! Studio custom worlds (`"type": "custom"`): the shared generator in `studio_format::worldgen` (the one the app's
//! preview draws) wrapped as a Pumpkin chunk generator. Block names are looked up once, when the world is made.
//!
//! The world is made with this generator from its first chunk (fork `world-generator.plugin`), and `keep` asks
//! the fork for vanilla carvers, structures and decorations on top (`plugin-with-vanilla`). Terrain goes in a
//! section (16×16×16) at a time. Features already carry the parts that start in the next chunk, so they aren't cut.

use pumpkin_plugin_api::wit::pumpkin::plugin::biomes::Biome;
use pumpkin_plugin_api::{ChunkBuffer, ChunkGenerator};
use std::collections::HashMap;
use studio_format::worldgen::{Generator, Only};

pub struct StudioGenerator {
    generator: Generator,
    states: HashMap<String, u16>,
    stone: u16,
    air: u16,
    biomes: HashMap<String, Biome>,
}

impl StudioGenerator {
    /// `ours` gives the state of one of our blocks (`studio:<id>`); `likes` the vanilla biome shown for each of our
    /// biomes (`studio:<id>` → its `like`), since the fork's chunk buffer wrapper can't place one by name yet.
    /// Returns the generator and the block names Pumpkin didn't know (they become stone).
    pub fn new(generator: Generator, likes: HashMap<String, String>, ours: impl Fn(&str) -> Option<u16>) -> (Self, Vec<String>) {
        let lookup = |name: &str| match name.strip_prefix("studio:") {
            Some(id) => ours(id),
            None if name.contains('[') => {
                // A structure's block with its properties; without them when Pumpkin doesn't know those.
                let (block, props) = studio_format::structure::split_state(name);
                pumpkin_plugin_api::world::resolve_block_state(&block, &props)
                    .or_else(|| pumpkin_plugin_api::world::resolve_block_state(&block, &[]))
            }
            None => pumpkin_plugin_api::world::resolve_block_state(&format!("minecraft:{name}"), &[]),
        };
        let stone_state = lookup(&generator.stone).or_else(|| lookup("stone")).unwrap_or(1);
        let air = lookup("air").unwrap_or(0);
        let mut states = HashMap::new();
        let mut unknown = Vec::new();
        for name in generator.block_names() {
            match lookup(&name) {
                Some(state) => {
                    states.insert(name, state);
                }
                None => unknown.push(name),
            }
        }
        let mut biomes: HashMap<String, Biome> = biome_table().into_iter().map(|(name, biome)| (name.to_string(), biome)).collect();
        for (name, like) in likes {
            let shown = biomes.get(&like).copied().unwrap_or(Biome::Plains);
            biomes.insert(name, shown);
        }
        (Self { generator, states, stone: stone_state, air, biomes }, unknown)
    }

    fn state(&self, name: &str) -> u16 {
        self.states.get(name).copied().unwrap_or(self.stone)
    }

    fn biome(&self, name: &str) -> Biome {
        self.biomes.get(name).copied().unwrap_or(Biome::Plains)
    }
}

impl ChunkGenerator for StudioGenerator {
    fn generate_biomes(&self, chunk: &mut ChunkBuffer) {
        let (cx, cz, min_y, height) = (chunk.x(), chunk.z(), chunk.min_y(), chunk.height() as i32);
        // Biomes are kept per 4×4 column (Minecraft's own grain); one fill when the chunk has just one.
        let mut picked = Vec::with_capacity(16);
        for qx in 0..4 {
            for qz in 0..4 {
                // The cell's corner, like Paper asks for it, so both servers draw the same edges.
                let column = self.generator.column(cx * 16 + qx * 4, cz * 16 + qz * 4);
                picked.push((qx as u8, qz as u8, self.biome(self.generator.biome(&column))));
            }
        }
        if picked.iter().all(|one| one.2 == picked[0].2) {
            chunk.fill_biome(picked[0].2);
            return;
        }
        for (qx, qz, biome) in picked {
            for y in (min_y..min_y + height).step_by(4) {
                chunk.set_biome(qx * 4, y, qz * 4, biome);
            }
        }
    }

    fn generate_noise(&self, chunk: &mut ChunkBuffer) {
        let (cx, cz, min_y) = (chunk.x(), chunk.z(), chunk.min_y());
        let height = chunk.height() as i32;
        let max_y = min_y + height - 1;
        // The whole chunk in the section order (y, z, x), then one call per section that isn't all air.
        let mut states = vec![self.air; (height as usize) * 256];
        for lx in 0..16i32 {
            for lz in 0..16i32 {
                for run in self.generator.runs(cx * 16 + lx, cz * 16 + lz, min_y, max_y) {
                    let state = self.state(&run.block);
                    for y in run.from.max(min_y)..=run.to.min(max_y) {
                        states[(((y - min_y) * 16 + lz) * 16 + lx) as usize] = state;
                    }
                }
            }
        }
        for (index, section) in states.chunks(4096).enumerate() {
            if section.iter().any(|state| *state != self.air) {
                chunk.set_section((min_y >> 4) + index as i32, section);
            }
        }
    }

    fn generate_features(&self, chunk: &mut ChunkBuffer) {
        let (cx, cz, min_y) = (chunk.x(), chunk.z(), chunk.min_y());
        let max_y = min_y + chunk.height() as i32 - 1;
        for one in self.generator.features(cx, cz, min_y, max_y) {
            let (lx, lz) = ((one.x - cx * 16) as u8, (one.z - cz * 16) as u8);
            let there = match one.only {
                Only::Any => None,
                _ => Some(chunk.get_block(lx, one.y, lz)),
            };
            let fits = match (one.only, there) {
                (Only::Air, Some(state)) => state == self.air,
                (Only::Stone, Some(state)) => state == self.stone,
                _ => true,
            };
            if fits {
                chunk.set_block(lx, one.y, lz, self.state(&one.block));
            }
        }
    }
}

/// Minecraft's biome names (short, as the studio files write them) and Pumpkin's biomes.
fn biome_table() -> Vec<(&'static str, Biome)> {
    vec![
        ("badlands", Biome::Badlands),
        ("bamboo_jungle", Biome::BambooJungle),
        ("basalt_deltas", Biome::BasaltDeltas),
        ("beach", Biome::Beach),
        ("birch_forest", Biome::BirchForest),
        ("cherry_grove", Biome::CherryGrove),
        ("cold_ocean", Biome::ColdOcean),
        ("crimson_forest", Biome::CrimsonForest),
        ("dappled_forest", Biome::DappledForest),
        ("dark_forest", Biome::DarkForest),
        ("deep_cold_ocean", Biome::DeepColdOcean),
        ("deep_dark", Biome::DeepDark),
        ("deep_frozen_ocean", Biome::DeepFrozenOcean),
        ("deep_lukewarm_ocean", Biome::DeepLukewarmOcean),
        ("deep_ocean", Biome::DeepOcean),
        ("desert", Biome::Desert),
        ("dripstone_caves", Biome::DripstoneCaves),
        ("end_barrens", Biome::EndBarrens),
        ("end_highlands", Biome::EndHighlands),
        ("end_midlands", Biome::EndMidlands),
        ("eroded_badlands", Biome::ErodedBadlands),
        ("flower_forest", Biome::FlowerForest),
        ("forest", Biome::Forest),
        ("frozen_ocean", Biome::FrozenOcean),
        ("frozen_peaks", Biome::FrozenPeaks),
        ("frozen_river", Biome::FrozenRiver),
        ("grove", Biome::Grove),
        ("ice_spikes", Biome::IceSpikes),
        ("jagged_peaks", Biome::JaggedPeaks),
        ("jungle", Biome::Jungle),
        ("lukewarm_ocean", Biome::LukewarmOcean),
        ("lush_caves", Biome::LushCaves),
        ("mangrove_swamp", Biome::MangroveSwamp),
        ("meadow", Biome::Meadow),
        ("mushroom_fields", Biome::MushroomFields),
        ("nether_wastes", Biome::NetherWastes),
        ("ocean", Biome::Ocean),
        ("old_growth_birch_forest", Biome::OldGrowthBirchForest),
        ("old_growth_pine_taiga", Biome::OldGrowthPineTaiga),
        ("old_growth_spruce_taiga", Biome::OldGrowthSpruceTaiga),
        ("pale_garden", Biome::PaleGarden),
        ("plains", Biome::Plains),
        ("river", Biome::River),
        ("savanna", Biome::Savanna),
        ("savanna_plateau", Biome::SavannaPlateau),
        ("small_end_islands", Biome::SmallEndIslands),
        ("snowy_beach", Biome::SnowyBeach),
        ("snowy_plains", Biome::SnowyPlains),
        ("snowy_slopes", Biome::SnowySlopes),
        ("snowy_taiga", Biome::SnowyTaiga),
        ("soul_sand_valley", Biome::SoulSandValley),
        ("sparse_jungle", Biome::SparseJungle),
        ("stony_peaks", Biome::StonyPeaks),
        ("stony_shore", Biome::StonyShore),
        ("sulfur_caves", Biome::SulfurCaves),
        ("sunflower_plains", Biome::SunflowerPlains),
        ("swamp", Biome::Swamp),
        ("taiga", Biome::Taiga),
        ("the_end", Biome::TheEnd),
        ("the_void", Biome::TheVoid),
        ("warm_ocean", Biome::WarmOcean),
        ("warped_forest", Biome::WarpedForest),
        ("windswept_forest", Biome::WindsweptForest),
        ("windswept_gravelly_hills", Biome::WindsweptGravellyHills),
        ("windswept_hills", Biome::WindsweptHills),
        ("windswept_savanna", Biome::WindsweptSavanna),
        ("wooded_badlands", Biome::WoodedBadlands),
    ]
}
