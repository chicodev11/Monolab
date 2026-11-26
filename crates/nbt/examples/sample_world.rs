//! Writes a small fake server with a world, for trying the NBT editor:
//! `cargo run -p monolab-nbt --example sample_world -- <folder>`
use monolab_nbt::*;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("usage: sample_world <folder>"));
    let world = out.join("world");
    std::fs::create_dir_all(world.join("playerdata")).unwrap();
    std::fs::create_dir_all(world.join("region")).unwrap();
    let level = snbt::parse_compound(
        r#"{Data:{LevelName:"world",DataVersion:4325,Difficulty:2b,hardcore:0b,allowCommands:1b,
        GameType:0,SpawnX:16,SpawnY:72,SpawnZ:-32,Time:1840260L,DayTime:6000L,raining:0b,thundering:0b,
        WorldGenSettings:{seed:-4172144997902289642L,generate_features:1b,dimensions:{}},
        GameRules:{doDaylightCycle:"true",keepInventory:"false",mobGriefing:"true",randomTickSpeed:"3"},
        Version:{Id:4325,Name:"1.21.5",Series:"main",Snapshot:0b},
        DataPacks:{Enabled:["vanilla","file/monolab"],Disabled:[]}}}"#,
    )
    .unwrap();
    std::fs::write(world.join("level.dat"), NbtFile::new(level).write()).unwrap();
    let player = snbt::parse_compound(
        r#"{Pos:[16.5d,72.0d,-31.5d],Rotation:[90.0f,0.0f],Health:20.0f,foodLevel:20,XpLevel:30,
        playerGameType:0,Dimension:"minecraft:overworld",
        Inventory:[{Slot:0b,id:"minecraft:diamond_sword",count:1,components:{"minecraft:enchantments":{levels:{"minecraft:sharpness":5}}}},
        {Slot:1b,id:"minecraft:torch",count:64},{Slot:2b,id:"minecraft:cooked_beef",count:32}],
        EnderItems:[],abilities:{flying:0b,mayfly:0b,walkSpeed:0.1f}}"#,
    )
    .unwrap();
    std::fs::write(world.join("playerdata/069a79f4-44e9-4726-a5be-fca90e38aaf5.dat"), NbtFile::new(player).write()).unwrap();
    let mut region = Region::empty();
    for z in 0..4 {
        for x in 0..6 {
            let chunk = snbt::parse_compound(&format!(
                r#"{{DataVersion:4325,xPos:{x},zPos:{z},yPos:-4,Status:"minecraft:full",LastUpdate:1200L,
                Heightmaps:{{WORLD_SURFACE:[L;1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37]}},
                sections:[{{Y:-4b,block_states:{{palette:[{{Name:"minecraft:bedrock"}},{{Name:"minecraft:deepslate"}}]}}}},
                {{Y:4b,block_states:{{palette:[{{Name:"minecraft:air"}}]}}}}],block_entities:[]}}"#
            ))
            .unwrap();
            let file = NbtFile { name: String::new(), root: chunk, compression: Compression::Zlib };
            region.store(x + z * 32, &file, 1_700_000_000).unwrap();
        }
    }
    std::fs::write(world.join("region/r.0.0.mca"), region.write()).unwrap();
    println!("{}", out.display());
}
