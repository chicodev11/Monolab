//! Files coming in and going out, in the app's shapes: a teammate sends a file in pieces
//! (`__upload_begin` → `__upload_part` → `__upload_end`, the app's `monolink_upload`), gets a token, and a
//! later request (`add_server_uploads`) puts it in the server folder. `read_server_file_download` hands
//! one file back whole (base64), as the app does for the web panel.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use relay_proto::DeviceId;
use serde_json::{Value, json};

/// Largest file a teammate may send (the app's limit).
pub const UPLOAD_MAX: u64 = 4 * 1024 * 1024 * 1024;
/// A piece is at most this much (before base64).
const PART_MAX: usize = 4 * 1024 * 1024;
/// Largest file handed back whole (the app's `HANDOUT_LIMIT`).
pub const HANDOUT_MAX: u64 = 64 * 1024 * 1024;
/// Uploads nobody finished, or finished and never used, are dropped after this.
const FORGET_AFTER: Duration = Duration::from_secs(3600);

struct Upload {
    peer: DeviceId,
    slot: PathBuf,
    size: u64,
    written: u64,
    touched: Instant,
    done: bool,
}

/// The uploads in `<data>/uploads/<token folder>/<name>` (0700), each tied to the teammate's device.
pub struct Uploads {
    dir: PathBuf,
    open: Mutex<HashMap<String, Upload>>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn random_hex() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .map_err(|error| format!("No randomness: {error}"))?;
    Ok(relay_proto::hex(&bytes))
}

/// A file name a teammate gave, made safe to write: its last part, without separators or control
/// characters, never empty, `.` or `..`.
pub fn plain_name(name: &str) -> Result<String, String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = last.chars().filter(|one| !one.is_control() && !matches!(one, ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
    let clean = clean.trim().to_string();
    if clean.is_empty() || clean == "." || clean == ".." || clean.len() > 200 {
        return Err("That file name can't be used.".into());
    }
    Ok(clean)
}

impl Uploads {
    pub fn new(data: &Path) -> Uploads {
        Uploads { dir: data.join("uploads"), open: Mutex::new(HashMap::new()) }
    }

    fn tidy(&self, open: &mut HashMap<String, Upload>) {
        open.retain(|_, one| {
            let keep = one.touched.elapsed() < FORGET_AFTER;
            if !keep {
                let _ = fs::remove_dir_all(one.slot.parent().unwrap_or(&one.slot));
            }
            keep
        });
    }

    /// `__upload_begin` `{name,size}` → `{upload}`, `__upload_part` `{upload,at,data}` → bytes so far,
    /// `__upload_end` `{upload}` → the token.
    pub fn request(&self, peer: &DeviceId, command: &str, body: &Value) -> Result<Value, String> {
        let text = |name: &str| body.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        let mut open = locked(&self.open);
        self.tidy(&mut open);
        match command {
            "__upload_begin" => {
                let size = body.get("size").and_then(Value::as_u64).ok_or("Say how big the file is.")?;
                if size > UPLOAD_MAX {
                    return Err("Files larger than 4 GB can't be uploaded.".into());
                }
                if open.values().filter(|one| one.peer == *peer && !one.done).count() >= 4 {
                    return Err("Too many uploads at once. Wait for the others to finish.".into());
                }
                let name = plain_name(&text("name"))?;
                let token = random_hex()?;
                let folder = self.dir.join(&token);
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(&folder)
                    .map_err(|error| format!("Could not keep the upload: {error}"))?;
                let slot = folder.join(&name);
                fs::File::create(&slot).map_err(|error| format!("Could not keep the upload: {error}"))?;
                open.insert(token.clone(), Upload { peer: *peer, slot, size, written: 0, touched: Instant::now(), done: false });
                Ok(json!({ "upload": token }))
            }
            "__upload_part" => {
                let upload =
                    open.get_mut(&text("upload")).filter(|one| one.peer == *peer && !one.done).ok_or("That upload isn't open any more.")?;
                let at = body.get("at").and_then(Value::as_u64).ok_or("Say where this piece goes.")?;
                if at != upload.written {
                    return Err("A piece of the file went missing. Try again.".into());
                }
                let bytes = decode(&text("data")).ok_or("A piece of the file was damaged.")?;
                if bytes.len() > PART_MAX || upload.written + bytes.len() as u64 > upload.size {
                    return Err("The file is bigger than it said.".into());
                }
                fs::OpenOptions::new()
                    .append(true)
                    .open(&upload.slot)
                    .and_then(|mut file| file.write_all(&bytes))
                    .map_err(|error| error.to_string())?;
                upload.written += bytes.len() as u64;
                upload.touched = Instant::now();
                Ok(json!(upload.written))
            }
            _ => {
                let token = text("upload");
                let upload = open.get_mut(&token).filter(|one| one.peer == *peer && !one.done).ok_or("That upload isn't open any more.")?;
                if upload.written != upload.size {
                    let gone = open.remove(&token).map(|one| one.slot);
                    if let Some(slot) = gone {
                        let _ = fs::remove_dir_all(slot.parent().unwrap_or(&slot));
                    }
                    return Err("The file didn't arrive whole. Try again.".into());
                }
                upload.done = true;
                upload.touched = Instant::now();
                Ok(Value::String(token))
            }
        }
    }

    /// The file behind a finished upload's token, taken out of the list (only by the teammate who sent it).
    pub fn take(&self, peer: &DeviceId, token: &str) -> Result<PathBuf, String> {
        let mut open = locked(&self.open);
        match open.get(token) {
            Some(one) if one.peer == *peer && one.done => Ok(open.remove(token).map(|one| one.slot).unwrap_or_default()),
            _ => Err("That upload isn't open any more. Send the file again.".into()),
        }
    }
}

/// Removes what `take` gave once it was used (or not).
pub fn forget(slot: &Path) {
    let _ = fs::remove_dir_all(slot.parent().unwrap_or(slot));
}

/// Moves a file to `target`, copying when the two are on different disks.
pub fn move_file(from: &Path, target: &Path) -> std::io::Result<()> {
    if fs::rename(from, target).is_ok() {
        return Ok(());
    }
    fs::copy(from, target)?;
    fs::remove_file(from)
}

/// `name`, or `name (2).ext`, `name (3).ext`… whichever isn't in `folder` yet.
pub fn free_name(folder: &Path, name: &str) -> PathBuf {
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_string(), format!(".{extension}")),
        _ => (name.to_string(), String::new()),
    };
    (2..).map(|number| folder.join(format!("{stem} ({number}){extension}"))).find(|one| !one.exists()).unwrap_or(first)
}

/// `add_server_uploads` `{id,path,uploads}`: puts uploaded files in a folder of the server (`""` is the
/// server folder). A name that is already there gets a number. Gives how many were added.
pub fn add_to_server(uploads: &Uploads, peer: &DeviceId, root: &Path, folder: &str, tokens: &[String]) -> Result<u32, String> {
    let root = root.canonicalize().map_err(|error| format!("Server folder not found: {error}"))?;
    let target = if folder.trim().is_empty() || folder.trim() == "/" {
        root.clone()
    } else {
        crate::files::resolve_in_server(&root, folder).map_err(|error| error.to_string())?
    };
    if !target.is_dir() {
        return Err("Not a folder.".into());
    }
    let mut added = 0;
    for token in tokens {
        let slot = uploads.take(peer, token)?;
        let name = slot.file_name().map(|one| one.to_string_lossy().into_owned()).unwrap_or_else(|| "upload".into());
        let moved = move_file(&slot, &free_name(&target, &name));
        forget(&slot);
        moved.map_err(|error| format!("Could not add {name}: {error}"))?;
        added += 1;
    }
    Ok(added)
}

/// `read_server_file_download` `{id,path}` → `{name, data}` (base64). Files only, up to 64 MB.
pub fn download(root: &Path, relative: &str) -> Result<Value, String> {
    let path = crate::files::resolve_in_server(root, relative).map_err(|error| error.to_string())?;
    if path.is_dir() {
        return Err("Download the files inside, or make a backup of the folder. A node can't zip folders yet.".into());
    }
    if fs::metadata(&path).map_err(|error| error.to_string())?.len() > HANDOUT_MAX {
        return Err("Files larger than 64 MB can't be downloaded through Monolink.".into());
    }
    let bytes = fs::read(&path).map_err(|error| format!("Could not read the file: {error}"))?;
    let name = path.file_name().map(|one| one.to_string_lossy().into_owned()).unwrap_or_else(|| "File".into());
    Ok(json!({ "name": name, "data": encode(&bytes) }))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let joined = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(ALPHABET[(joined >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Standard base64 (padding optional, white space ignored); `None` when it isn't.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut buffer, mut bits) = (0u32, 0u32);
    let mut ended = false;
    for byte in text.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'=' {
            ended = true;
            continue;
        }
        if ended {
            return None;
        }
        let value = ALPHABET.iter().position(|one| *one == byte)? as u32;
        buffer = buffer << 6 | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: DeviceId = DeviceId([1; 16]);
    const OTHER: DeviceId = DeviceId([2; 16]);

    #[test]
    fn base64_both_ways() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0, 255, 128, 7]] {
            assert_eq!(decode(&encode(sample)).unwrap(), sample);
        }
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert!(decode("Zm9v!").is_none());
    }

    #[test]
    fn names_are_made_plain() {
        assert_eq!(plain_name("../../etc/passwd").unwrap(), "passwd");
        assert_eq!(plain_name("C:\\x\\plugin.jar").unwrap(), "plugin.jar");
        assert!(plain_name("..").is_err());
        assert!(plain_name("dir/").is_err());
    }

    #[test]
    fn an_upload_lands_in_the_server_once() {
        let base = crate::files::tests::sandbox("uploads");
        let root = base.join("server");
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("plugins/a.jar"), "old").unwrap();
        let uploads = Uploads::new(&base.join("state"));
        let begun = uploads.request(&ME, "__upload_begin", &json!({ "name": "../a.jar", "size": 5 })).unwrap();
        let token = begun["upload"].as_str().unwrap().to_string();
        assert!(
            uploads.request(&OTHER, "__upload_part", &json!({ "upload": token, "at": 0, "data": encode(b"hel") })).is_err(),
            "someone else's upload"
        );
        assert!(
            uploads
                .request(&ME, "__upload_part", &json!({ "upload": token, "at": 1, "data": encode(b"hel") }))
                .unwrap_err()
                .contains("missing")
        );
        uploads.request(&ME, "__upload_part", &json!({ "upload": token, "at": 0, "data": encode(b"hel") })).unwrap();
        uploads.request(&ME, "__upload_part", &json!({ "upload": token, "at": 3, "data": encode(b"lo") })).unwrap();
        assert!(
            uploads
                .request(&ME, "__upload_part", &json!({ "upload": token, "at": 5, "data": encode(b"!") }))
                .unwrap_err()
                .contains("bigger")
        );
        assert_eq!(uploads.request(&ME, "__upload_end", &json!({ "upload": token })).unwrap(), json!(token));
        assert!(add_to_server(&uploads, &OTHER, &root, "plugins", std::slice::from_ref(&token)).is_err(), "only the sender uses the token");
        assert!(add_to_server(&uploads, &ME, &root, "../", std::slice::from_ref(&token)).unwrap_err().contains("outside"));
        // The failed tries above didn't use it up.
        assert_eq!(add_to_server(&uploads, &ME, &root, "plugins", std::slice::from_ref(&token)).unwrap(), 1);
        assert_eq!(fs::read_to_string(root.join("plugins/a (2).jar")).unwrap(), "hello");
        assert_eq!(fs::read_to_string(root.join("plugins/a.jar")).unwrap(), "old");
        assert!(add_to_server(&uploads, &ME, &root, "plugins", &[token]).is_err(), "a token works once");
        assert_eq!(fs::read_dir(base.join("state/uploads")).unwrap().count(), 0, "nothing is left behind");

        // Short of what it said.
        let short =
            uploads.request(&ME, "__upload_begin", &json!({ "name": "b.txt", "size": 3 })).unwrap()["upload"].as_str().unwrap().to_string();
        assert!(uploads.request(&ME, "__upload_end", &json!({ "upload": short })).unwrap_err().contains("whole"));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn downloads_are_files_inside() {
        let root = crate::files::tests::sandbox("download");
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join("logs/latest.log"), "hi").unwrap();
        let handed = download(&root, "logs/latest.log").unwrap();
        assert_eq!(handed, json!({ "name": "latest.log", "data": encode(b"hi") }));
        assert!(download(&root, "logs").unwrap_err().contains("folder"));
        assert!(download(&root, "../x").unwrap_err().contains("outside"));
        fs::remove_dir_all(&root).unwrap();
    }
}
