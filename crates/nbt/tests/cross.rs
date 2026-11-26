//! Checks the codec against fastnbt, which the current app already relies on.
use monolab_nbt::*;

#[test]
fn fastnbt_reads_what_we_write_and_we_read_what_it_writes() {
    let root =
        snbt::parse_compound(r#"{a:1b,b:2s,c:3,d:4L,e:5.5f,f:6.5d,g:"Wörld 😀",h:[1,2],i:[B;1b],j:[I;1,2],k:[L;3L],l:{m:[]}}"#).unwrap();
    let ours = binary::write_root("", &root);
    let value: fastnbt::Value = fastnbt::from_bytes(&ours).unwrap();
    let theirs = fastnbt::to_bytes(&value).unwrap();
    let (_, back) = binary::read_root(&theirs).unwrap();
    // fastnbt does not keep key order, so compare key by key.
    assert_eq!(back.len(), root.len());
    for (key, tag) in root.iter() {
        assert_eq!(back.get(key), Some(tag), "{key}");
    }
    let fastnbt::Value::Compound(map) = value else { panic!() };
    assert_eq!(map["g"], fastnbt::Value::String("Wörld 😀".into()));
    assert_eq!(map["d"], fastnbt::Value::Long(4));
}
