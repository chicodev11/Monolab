use flate2::read::GzDecoder;
use std::{
    fs,
    io::{Cursor, Read},
    path::PathBuf,
    sync::Mutex,
    time::Duration,
};
use tauri::{AppHandle, Manager};

/// Separates an archive from a file inside it, as in `mods/x.jar!/a/B.class`.
pub const SPLIT: &str = "!/";
pub const FILE_LIMIT: u64 = 64 * 1024 * 1024;
const ENTRY_LIMIT: u64 = 32 * 1024 * 1024;
const TEXT_LIMIT: usize = 2 * 1024 * 1024;
const HEX_LIMIT: usize = 64 * 1024;
const ENTRY_COUNT: usize = 20_000;
const NBT_EXTENSIONS: &[&str] = &["dat", "dat_old", "nbt", "schem", "schematic", "litematic", "mcstructure"];
const VINEFLOWER: &str = "1.12.0";
const VINEFLOWER_SHA1: &str = "85570609a0a5941a7d2918b6260b209de810f66f";

#[derive(serde::Serialize, specta::Type, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    /// text, java, nbt, archive, image or hex
    pub kind: String,
    pub text: String,
    pub entries: Vec<ArchiveEntry>,
    /// decompiled, truncated or binary
    pub note: String,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntry {
    pub path: String,
    pub size: f64,
}

pub fn view_bytes(app: &AppHandle, name: &str, bytes: Vec<u8>, inner: Option<&str>) -> Result<FileView, String> {
    let Some(inner) = inner else {
        return view_one(Some(app), name, bytes);
    };
    let (entry, rest) = match inner.split_once(SPLIT) {
        Some((entry, rest)) => (entry, Some(rest)),
        None => (inner, None),
    };
    let data = zip_entry(&bytes, entry)?;
    let entry_name = entry.rsplit('/').next().unwrap_or(entry);
    view_bytes(app, entry_name, data, rest)
}

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default()
}

fn view_one(app: Option<&AppHandle>, name: &str, bytes: Vec<u8>) -> Result<FileView, String> {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return archive(&bytes);
    }
    if bytes.starts_with(&[0xCA, 0xFE, 0xBA, 0xBE]) {
        if let Some(app) = app {
            return decompile(app, name, &bytes);
        }
    }
    if let Some(mime) = crate::image_kind(name) {
        use base64::Engine;
        return Ok(FileView {
            kind: "image".into(),
            text: format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)),
            ..Default::default()
        });
    }
    if NBT_EXTENSIONS.contains(&extension(name).as_str()) {
        if let Ok(value) = crate::nbt::decode_nbt(bytes.clone()) {
            return Ok(nbt_view(&value));
        }
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        GzDecoder::new(&bytes[..])
            .take(ENTRY_LIMIT)
            .read_to_end(&mut out)
            .map_err(|error| format!("Could not decompress the file: {error}"))?;
        if out.first() == Some(&10) {
            if let Ok(value) = fastnbt::from_bytes::<fastnbt::Value>(&out) {
                return Ok(nbt_view(&value));
            }
        }
        let unpacked = name.strip_suffix(".gz").unwrap_or(name);
        return view_one(app, unpacked, out);
    }
    Ok(text_or_hex(&bytes))
}

fn text_or_hex(bytes: &[u8]) -> FileView {
    let cut = &bytes[..bytes.len().min(TEXT_LIMIT)];
    let text = String::from_utf8_lossy(cut);
    if !crate::looks_binary(&text) {
        return FileView {
            kind: "text".into(),
            text: text.into_owned(),
            note: if bytes.len() > TEXT_LIMIT { "truncated".into() } else { String::new() },
            ..Default::default()
        };
    }
    FileView {
        kind: "hex".into(),
        text: hex_dump(&bytes[..bytes.len().min(HEX_LIMIT)]),
        note: if bytes.len() > HEX_LIMIT { "truncated".into() } else { "binary".into() },
        ..Default::default()
    }
}

fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
        let shown: String = chunk
            .iter()
            .map(|byte| if byte.is_ascii_graphic() || *byte == b' ' { *byte as char } else { '.' })
            .collect();
        out.push_str(&format!("{:08x}  {:<47}  {shown}\n", row * 16, hex.join(" ")));
    }
    out
}

fn archive(bytes: &[u8]) -> Result<FileView, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| format!("Could not read the archive: {error}"))?;
    let mut entries = Vec::new();
    for index in 0..zip.len().min(ENTRY_COUNT) {
        let Ok(file) = zip.by_index_raw(index) else { continue };
        if file.is_dir() {
            continue;
        }
        let Ok(name) = file.name() else { continue };
        entries.push(ArchiveEntry { path: name.to_string(), size: file.size() as f64 });
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(FileView {
        kind: "archive".into(),
        note: if zip.len() > ENTRY_COUNT { "truncated".into() } else { String::new() },
        entries,
        ..Default::default()
    })
}

fn zip_entry(bytes: &[u8], entry: &str) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| format!("Could not read the archive: {error}"))?;
    let file = zip.by_name(entry).map_err(|_| "File not found.".to_string())?;
    if file.size() > ENTRY_LIMIT {
        return Err("This file is too large to open in the app.".into());
    }
    let mut out = Vec::new();
    file.take(ENTRY_LIMIT)
        .read_to_end(&mut out)
        .map_err(|error| format!("Could not read the archive: {error}"))?;
    Ok(out)
}

fn nbt_view(value: &fastnbt::Value) -> FileView {
    let mut text = String::new();
    snbt(value, 0, &mut text);
    FileView { kind: "nbt".into(), text, ..Default::default() }
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn key(text: &str) -> String {
    let plain = !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'));
    if plain { text.to_string() } else { quote(text) }
}

fn numbers<T: std::fmt::Display>(prefix: &str, suffix: &str, items: &[T], out: &mut String) {
    const SHOWN: usize = 64;
    let mut parts: Vec<String> = items.iter().take(SHOWN).map(|one| format!("{one}{suffix}")).collect();
    if items.len() > SHOWN {
        parts.push(format!("… {}", items.len()));
    }
    out.push_str(&format!("[{prefix}; {}]", parts.join(", ")));
}

fn snbt(value: &fastnbt::Value, depth: usize, out: &mut String) {
    use fastnbt::Value;
    let pad = |depth: usize| "    ".repeat(depth);
    match value {
        Value::Byte(v) => out.push_str(&format!("{v}b")),
        Value::Short(v) => out.push_str(&format!("{v}s")),
        Value::Int(v) => out.push_str(&v.to_string()),
        Value::Long(v) => out.push_str(&format!("{v}L")),
        Value::Float(v) => out.push_str(&format!("{v}f")),
        Value::Double(v) => out.push_str(&format!("{v}d")),
        Value::String(v) => out.push_str(&quote(v)),
        Value::ByteArray(v) => numbers("B", "b", v, out),
        Value::IntArray(v) => numbers("I", "", v, out),
        Value::LongArray(v) => numbers("L", "L", v, out),
        Value::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            let flat = items.iter().all(|one| !matches!(one, Value::Compound(_) | Value::List(_)));
            if flat && items.len() <= 16 {
                out.push('[');
                for (at, one) in items.iter().enumerate() {
                    if at > 0 {
                        out.push_str(", ");
                    }
                    snbt(one, depth, out);
                }
                out.push(']');
                return;
            }
            out.push_str("[\n");
            for (at, one) in items.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                snbt(one, depth + 1, out);
                out.push_str(if at + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push(']');
        }
        Value::Compound(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut names: Vec<&String> = map.keys().collect();
            names.sort();
            out.push_str("{\n");
            for (at, name) in names.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                out.push_str(&key(name));
                out.push_str(": ");
                snbt(&map[*name], depth + 1, out);
                out.push_str(if at + 1 < names.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push('}');
        }
    }
}

static TOOL_JAVA: Mutex<Option<PathBuf>> = Mutex::new(None);

fn tool_java(app: &AppHandle) -> Result<PathBuf, String> {
    let mut kept = TOOL_JAVA.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(path) = kept.as_ref() {
        return Ok(path.clone());
    }
    let path = match crate::java::scan_installed_javas(app).into_iter().find(|runtime| runtime.major >= 17) {
        Some(found) => PathBuf::from(found.path),
        None => PathBuf::from(crate::java::fetch_runtime(app, 21)?.path),
    };
    *kept = Some(path.clone());
    Ok(path)
}

fn vineflower(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|error| error.to_string())?.join("tools");
    let jar = dir.join(format!("vineflower-{VINEFLOWER}.jar"));
    if jar.is_file() {
        return Ok(jar);
    }
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let part = dir.join(format!("vineflower-{VINEFLOWER}.part"));
    let client = crate::net::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())?;
    let url = format!("https://repo1.maven.org/maven2/org/vineflower/vineflower/{VINEFLOWER}/vineflower-{VINEFLOWER}.jar");
    crate::download_file(&client, &url, &part)?;
    if let Err(error) = crate::hashes::verify_sha1(&part, VINEFLOWER_SHA1) {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    fs::rename(&part, &jar).map_err(|error| error.to_string())?;
    Ok(jar)
}

fn decompile(app: &AppHandle, name: &str, bytes: &[u8]) -> Result<FileView, String> {
    let java = tool_java(app)?;
    let jar = vineflower(app)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let work = std::env::temp_dir().join(format!("monolab-decompile-{stamp}"));
    let out = work.join("out");
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let file: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '-' | '.')).collect();
    let file = if file.ends_with(".class") { file } else { format!("{file}.class") };
    let input = work.join(&file);
    let result = (|| {
        fs::write(&input, bytes).map_err(|error| error.to_string())?;
        let output = crate::hidden_command(&java)
            .arg("-jar")
            .arg(&jar)
            .arg("-log=ERROR")
            .arg(&input)
            .arg(&out)
            .output()
            .map_err(|error| format!("Could not run Java: {error}"))?;
        let source = find_java_file(&out).ok_or_else(|| {
            let detail = String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("").to_string();
            format!("Could not decompile the class file. {detail}").trim().to_string()
        })?;
        let text = fs::read_to_string(&source).map_err(|error| error.to_string())?;
        Ok(FileView { kind: "java".into(), text, note: "decompiled".into(), ..Default::default() })
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

fn find_java_file(dir: &std::path::Path) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_java_file(&path) {
                return Some(found);
            }
        } else if path.extension().is_some_and(|ext| ext == "java") {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lists_archive_entries_and_reads_one() {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("b/config.json", options).unwrap();
            zip.write_all(b"{\"a\":1}").unwrap();
            zip.start_file("a.txt", options).unwrap();
            zip.write_all(b"hello").unwrap();
            zip.finish().unwrap();
        }
        let bytes = buffer.into_inner();
        let view = view_one(None, "x.jar", bytes.clone()).unwrap();
        assert_eq!(view.kind, "archive");
        assert_eq!(view.entries.iter().map(|one| one.path.as_str()).collect::<Vec<_>>(), ["a.txt", "b/config.json"]);
        let inner = view_one(None, "config.json", zip_entry(&bytes, "b/config.json").unwrap()).unwrap();
        assert_eq!((inner.kind.as_str(), inner.text.as_str()), ("text", "{\"a\":1}"));
    }

    #[test]
    fn shows_gzip_nbt_as_snbt() {
        let mut map = std::collections::HashMap::new();
        map.insert("Name".to_string(), fastnbt::Value::String("Steve".into()));
        map.insert("Health".to_string(), fastnbt::Value::Float(20.0));
        map.insert("Pos".to_string(), fastnbt::Value::List(vec![fastnbt::Value::Double(1.5), fastnbt::Value::Double(64.0)]));
        let raw = fastnbt::to_bytes(&fastnbt::Value::Compound(map)).unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&raw).unwrap();
        let view = view_one(None, "player.dat", gz.finish().unwrap()).unwrap();
        assert_eq!(view.kind, "nbt");
        assert_eq!(view.text, "{\n    Health: 20f,\n    Name: \"Steve\",\n    Pos: [1.5d, 64d]\n}");
    }

    #[test]
    fn falls_back_to_hex_for_binary() {
        let view = view_one(None, "blob.bin", vec![0, 1, 2, 255, 65]).unwrap();
        assert_eq!(view.kind, "hex");
        assert!(view.text.starts_with("00000000  00 01 02 ff 41"));
    }
}
