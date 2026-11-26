use monolab_nbt::*;

fn sample() -> Compound {
    snbt::parse_compound(
        r#"{Data:{LevelName:"Wörld 🌍",hardcore:0b,Time:123456789L,SpawnX:-12,ratio:0.5f,rain:1.25d,
        GameRules:{doDaylightCycle:"true"},Pos:[1.0d,64.0d,-3.5d],Inventory:[{Slot:0b,id:"minecraft:stone",count:3}],
        Bytes:[B;1b,-2b],Ints:[I;1,2,3],Longs:[L;9L],Empty:[],Zero:"a\u0000b"}}"#
            .replace("\\u0000", "\u{0}")
            .as_str(),
    )
    .unwrap()
}

#[test]
fn binary_round_trip_keeps_order_and_bytes() {
    for compression in [Compression::None, Compression::Gzip, Compression::Zlib] {
        let file = NbtFile {
            name: String::new(),
            root: sample(),
            compression,
        };
        let bytes = file.write();
        assert_eq!(Compression::sniff(&bytes), compression);
        let back = NbtFile::read(&bytes).unwrap();
        assert_eq!(back, file);
        let data = back.root.get("Data").unwrap().as_compound().unwrap();
        let keys: Vec<_> = data.keys().cloned().collect();
        assert_eq!(&keys[..4], ["LevelName", "hardcore", "Time", "SpawnX"]);
        // Raw bytes are stable across a second round trip.
        let raw = binary::write_root("", &back.root);
        assert_eq!(
            binary::write_root("", &NbtFile::read(&bytes).unwrap().root),
            raw
        );
    }
}

#[test]
fn modified_utf8_matches_java() {
    assert_eq!(binary::encode_mutf8("\0"), [0xc0, 0x80]);
    assert_eq!(
        binary::encode_mutf8("😀"),
        [0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80]
    );
    for text in ["plain", "Wörld", "😀 emoji", "a\0b", "\u{D55C}\u{AD6D}\u{C5B4}"] {
        assert_eq!(
            binary::decode_mutf8(&binary::encode_mutf8(text)).unwrap(),
            text
        );
    }
    // Plain UTF-8 four-byte sequences written by other tools are accepted.
    assert_eq!(binary::decode_mutf8("😀".as_bytes()).unwrap(), "😀");
}

#[test]
fn damaged_files_are_errors_not_panics() {
    let good = NbtFile::new(sample()).write();
    let raw = binary::decompress(&good, Compression::Gzip).unwrap();
    for cut in [0, 1, 3, 10, raw.len() / 2, raw.len() - 1] {
        assert!(binary::read_root(&raw[..cut]).is_err(), "cut at {cut}");
    }
    assert!(NbtFile::read(&[8, 0, 0]).is_err());
    // A list claiming billions of items must not allocate them up front.
    let mut evil = vec![10, 0, 0, 9, 0, 1, b'x', 1, 0x7f, 0xff, 0xff, 0xff];
    evil.push(0);
    assert!(binary::read_root(&evil).is_err());
}

#[test]
fn snbt_round_trip() {
    let root = sample();
    for pretty in [false, true] {
        let text = snbt::compound_to_snbt(&root, pretty);
        assert_eq!(snbt::parse_compound(&text).unwrap(), root, "{text}");
    }
    assert_eq!(snbt::parse("1b").unwrap(), Tag::Byte(1));
    assert_eq!(snbt::parse("true").unwrap(), Tag::Byte(1));
    assert_eq!(snbt::parse("-3s").unwrap(), Tag::Short(-3));
    assert_eq!(snbt::parse("7").unwrap(), Tag::Int(7));
    assert_eq!(snbt::parse("7L").unwrap(), Tag::Long(7));
    assert_eq!(snbt::parse("1.5").unwrap(), Tag::Double(1.5));
    assert_eq!(snbt::parse("1.5f").unwrap(), Tag::Float(1.5));
    assert_eq!(
        snbt::parse("stone_1.x").unwrap(),
        Tag::String("stone_1.x".into())
    );
    assert!(snbt::parse("minecraft:stone").is_err());
    assert_eq!(snbt::parse("'it''s'").ok(), None);
    assert_eq!(
        snbt::parse(r#"'say "hi"'"#).unwrap(),
        Tag::String("say \"hi\"".into())
    );
    assert!(snbt::parse("[1, 2b]").is_err());
    assert!(snbt::parse("[B; 300]").is_err());
    assert!(snbt::parse("{a:1").is_err());
    assert_eq!(snbt::key("needs space"), "\"needs space\"");
    assert_eq!(snbt::key("plain_key.1"), "plain_key.1");
}

#[test]
fn typed_field_values() {
    assert_eq!(snbt::parse_as(Kind::Byte, "true").unwrap(), Tag::Byte(1));
    assert_eq!(snbt::parse_as(Kind::Byte, "5b").unwrap(), Tag::Byte(5));
    assert!(snbt::parse_as(Kind::Byte, "300").is_err());
    assert_eq!(snbt::parse_as(Kind::Long, " 12L ").unwrap(), Tag::Long(12));
    assert_eq!(
        snbt::parse_as(Kind::String, " keep spaces ").unwrap(),
        Tag::String(" keep spaces ".into())
    );
    assert_eq!(
        snbt::parse_as(Kind::IntArray, "[I;1,2]").unwrap(),
        Tag::IntArray(vec![1, 2])
    );
    assert!(snbt::parse_as(Kind::IntArray, "[1,2]").is_err());
    assert_eq!(snbt::edit_text(&Tag::Double(64.0)), "64.0");
    assert_eq!(snbt::edit_text(&Tag::String("x".into())), "x");
}

fn key(name: &str) -> Step {
    Step::Key(name.into())
}

#[test]
fn editor_edits_undo_and_redo() {
    let mut ed = Editor::new(NbtFile::new(sample()));
    assert!(!ed.dirty());
    let time = vec![key("Data"), key("Time")];
    ed.set_text(&time, "5").unwrap();
    assert_eq!(ed.get(&time), Some(&Tag::Long(5)));
    assert!(ed.dirty());
    assert!(ed.set_text(&time, "nope").is_err());

    ed.rename(&time, "Ticks").unwrap();
    assert!(ed.get(&time).is_none());
    let ticks = vec![key("Data"), key("Ticks")];
    // Renaming keeps the key's place.
    let keys: Vec<_> = ed
        .get(&[key("Data")])
        .unwrap()
        .as_compound()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(keys[2], "Ticks");
    assert!(ed.rename(&ticks, "SpawnX").is_err());

    let pos = vec![key("Data"), key("Pos")];
    let added = ed.add(&pos, Tag::Double(9.0), "").unwrap();
    assert_eq!(added, vec![key("Data"), key("Pos"), Step::Index(3)]);
    assert!(ed.add(&pos, Tag::Int(1), "").is_err());
    ed.remove(&[key("Data"), key("Pos"), Step::Index(0)])
        .unwrap();
    assert_eq!(ed.get(&pos).unwrap().len(), 3);

    let n = ed.add(&[key("Data")], Tag::Int(1), "SpawnX").unwrap();
    assert_eq!(n.last(), Some(&key("SpawnX 2")));

    for _ in 0..5 {
        ed.undo().unwrap();
    }
    assert!(!ed.dirty());
    assert_eq!(ed.root(), &sample());
    ed.redo().unwrap();
    assert_eq!(ed.get(&time), Some(&Tag::Long(5)));
    ed.mark_saved();
    assert!(!ed.dirty());
    ed.undo().unwrap();
    assert!(ed.dirty());
    // A new edit after undoing past the save point makes it unreachable.
    ed.set_text(&time, "1").unwrap();
    ed.undo().unwrap();
    assert!(ed.dirty());
}

#[test]
fn editor_rows_follow_open_state_and_search() {
    let mut ed = Editor::new(NbtFile::new(sample()));
    let rows = ed.rows("");
    assert_eq!(rows.len(), 2, "root and Data");
    ed.toggle(&[key("Data")]);
    let rows = ed.rows("");
    assert_eq!(rows[2].label, "LevelName");
    assert_eq!(rows[2].preview, "Wörld 🌍");
    assert_eq!(rows[2].depth, 2);

    ed.toggle(&[key("Data"), key("Pos")]);
    ed.remove(&[key("Data"), key("Pos"), Step::Index(0)])
        .unwrap();
    assert!(ed.is_open(&[key("Data"), key("Pos")]));
    ed.rename(&[key("Data"), key("Pos")], "Where").unwrap();
    assert!(ed.is_open(&[key("Data"), key("Where")]));

    let hits = ed.rows("stone");
    let labels: Vec<_> = hits.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(labels, ["", "Data", "Inventory", "[0]", "id"]);
    assert!(hits.last().unwrap().hit && !hits[1].hit);

    ed.expand_all(&[]);
    assert!(ed.rows("").len() > 20);
    ed.collapse_all();
    assert_eq!(ed.rows("").len(), 2, "root stays open");
}

#[test]
fn region_round_trip() {
    assert_eq!(region_coords("r.-1.2.mca"), Some((-1, 2)));
    assert_eq!(region_coords("r.1.mca"), None);
    let mut region = Region::empty();
    let mut chunk = sample();
    chunk.insert("xPos", Tag::Int(3));
    region
        .store(
            3 + 32 * 5,
            &NbtFile {
                name: String::new(),
                root: chunk.clone(),
                compression: Compression::Zlib,
            },
            77,
        )
        .unwrap();
    region.store(0, &NbtFile::new(sample()), 1).unwrap();
    let bytes = region.write();
    assert_eq!(bytes.len() % 4096, 0);
    let back = Region::read(&bytes).unwrap();
    let chunks = back.chunks();
    assert_eq!(chunks.len(), 2);
    assert_eq!((chunks[1].x, chunks[1].z, chunks[1].timestamp), (3, 5, 77));
    let loaded = back.load(chunks[1].index, || unreachable!()).unwrap();
    assert_eq!(loaded.root, chunk);
    assert_eq!(loaded.compression, Compression::Zlib);
    assert_eq!(
        back.load(0, || unreachable!()).unwrap().compression,
        Compression::Gzip
    );
    assert!(back.load(5, || unreachable!()).is_err());
    assert_eq!(Region::read(&[]).unwrap().len(), 0);
    assert!(Region::read(&[0; 100]).is_err());
}

#[test]
fn file_kinds() {
    assert_eq!(file_kind("level.dat"), Some(FileKind::Document));
    assert_eq!(file_kind("r.0.0.mca"), Some(FileKind::Region));
    assert_eq!(file_kind("server.properties"), None);
}
