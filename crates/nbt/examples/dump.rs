//! Prints any NBT file or region chunk as SNBT:
//! `cargo run -p monolab-nbt --example dump -- <file> [chunk index]`
use monolab_nbt::*;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: dump <file> [chunk]");
    let bytes = std::fs::read(&path).expect("read");
    let root = match file_kind(&path) {
        Some(FileKind::Region) => {
            let region = Region::read(&bytes).expect("region");
            let index = args.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            println!("{} chunks", region.len());
            region.load(index, || Err("external".into())).expect("chunk").root
        }
        _ => NbtFile::read(&bytes).expect("nbt").root,
    };
    println!("{}", snbt::compound_to_snbt(&root, true));
}
