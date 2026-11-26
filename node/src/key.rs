//! The node's device key: 32 random bytes in `<data>/device.key`, readable by the service user only.
//! It is the iroh key, so the node's device id is the hash of its public half (like the app's).

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub const FILE: &str = "device.key";

/// Reads the key, or makes one the first time. A key file others can read is refused.
pub fn load_or_create(data: &Path) -> Result<[u8; 32], String> {
    let path = data.join(FILE);
    match fs::metadata(&path) {
        Ok(meta) => {
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(format!("{} can be read by others. Run: chmod 600 {}", path.display(), path.display()));
            }
            let bytes = fs::read(&path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
            bytes.try_into().map_err(|_| format!("{} is not a device key (32 bytes).", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(data).map_err(|error| format!("Could not make {}: {error}", data.display()))?;
            let mut key = [0u8; 32];
            fs::File::open("/dev/urandom")
                .and_then(|mut random| random.read_exact(&mut key))
                .map_err(|error| format!("No randomness: {error}"))?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
            file.write_all(&key).and_then(|_| file.sync_all()).map_err(|error| format!("Could not write {}: {error}", path.display()))?;
            Ok(key)
        }
        Err(error) => Err(format!("Could not read {}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn made_once_private_and_kept() {
        let data = crate::files::tests::sandbox("key");
        let first = load_or_create(&data).unwrap();
        assert_eq!(fs::metadata(data.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(load_or_create(&data).unwrap(), first);
        fs::set_permissions(data.join(FILE), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_or_create(&data).unwrap_err().contains("chmod 600"));
        fs::remove_dir_all(&data).unwrap();
    }
}
