//! This device's identity and Windows-protected secrets.
//!
//! The device key is made once and kept in the app data folder, wrapped with Windows DPAPI so a copy
//! of the file is useless on another PC or account. Monolink knows this device by it (the account
//! server and iroh both use it), and `seal`/`unseal` also protect other saved secrets.

use crate::data_root::MonolabDirs;
use std::path::PathBuf;

use ed25519_dalek::SigningKey;
use tauri::{AppHandle, Manager};

/// Same file name as before, so the device keeps its id.
const KEY_FILE: &str = "relay-device.key";

fn key_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().monolab_data_dir().map_err(|error| error.to_string())?.join(KEY_FILE))
}

fn existing_key(app: &AppHandle) -> Option<SigningKey> {
    let sealed = std::fs::read(key_path(app).ok()?).ok()?;
    let seed: [u8; 32] = unseal(&sealed)?.try_into().ok()?;
    Some(SigningKey::from_bytes(&seed))
}

/// This device's key, made on first use. A file that can't be opened here (copied from another PC)
/// is replaced, which makes this a new device (teams see it as a new device).
pub(crate) fn device_key(app: &AppHandle) -> Result<SigningKey, String> {
    if let Some(key) = existing_key(app) {
        return Ok(key);
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| format!("Could not make a device key: {error}"))?;
    let sealed = seal(&seed).ok_or("Windows couldn't protect the device key.")?;
    let path = key_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, &sealed)
        .and_then(|()| std::fs::rename(&temp, &path))
        .map_err(|error| format!("Could not save the device key: {error}"))?;
    Ok(SigningKey::from_bytes(&seed))
}

#[cfg(windows)]
fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: `input` points at `data`, which outlives the call; Windows allocates `output`, which is
    // copied out and then released with LocalFree as the API requires.
    unsafe {
        let ok = if protect {
            CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        } else {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        };
        if ok == 0 || output.pbData.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        Some(bytes)
    }
}

#[cfg(windows)]
pub(crate) fn seal(seed: &[u8]) -> Option<Vec<u8>> {
    dpapi(seed, true)
}

#[cfg(windows)]
pub(crate) fn unseal(sealed: &[u8]) -> Option<Vec<u8>> {
    dpapi(sealed, false)
}

#[cfg(not(windows))]
pub(crate) fn seal(seed: &[u8]) -> Option<Vec<u8>> {
    Some(seed.to_vec())
}

#[cfg(not(windows))]
pub(crate) fn unseal(sealed: &[u8]) -> Option<Vec<u8>> {
    Some(sealed.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_keys_open_again() {
        let seed = [7u8; 32];
        let sealed = seal(&seed).unwrap();
        #[cfg(windows)]
        assert_ne!(sealed, seed.to_vec(), "DPAPI wraps the key");
        assert_eq!(unseal(&sealed).unwrap(), seed.to_vec());
        #[cfg(windows)]
        assert!(unseal(b"not sealed").is_none());
    }
}
