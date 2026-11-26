
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::{fs, io::Read, path::Path};
use zip::ZipArchive;

pub(crate) fn verify_java_archive(path: &Path) -> Result<(), String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Could not open the downloaded file: {error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|_| {
        "The downloaded file is not a valid jar. The distributor may have returned an error page."
            .to_string()
    })?;
    if archive.is_empty() {
        return Err("The downloaded jar is empty.".into());
    }
    archive
        .by_name("META-INF/MANIFEST.MF")
        .map(|_| ())
        .map_err(|_| "The downloaded jar has no MANIFEST and cannot be used as a server file.".to_string())
}

pub(crate) fn verify_windows_exe(path: &Path) -> Result<(), String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("Could not open the downloaded file: {error}"))?;
    let mut head = [0_u8; 2];
    let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let ok = file.read_exact(&mut head).is_ok() && &head == b"MZ" && size > 1024 * 1024;
    if ok {
        Ok(())
    } else {
        Err("The downloaded file is not an executable. The distributor may have returned an error page.".into())
    }
}

pub(crate) fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("SHA‑256 verification of the download failed. The file was not used.".into())
    }
}

pub(crate) fn verify_sha1(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("SHA‑1 verification of the download failed. The file was not used.".into())
    }
}

pub(crate) fn verify_md5(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Md5::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("MD5 verification of the download failed. The file was not used.".into())
    }
}

pub(crate) fn verify_sha512(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha512::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("SHA‑512 verification of the mod/plugin failed. The file was not installed.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn tells_an_exe_from_an_error_page() {
        let dir = std::env::temp_dir().join("se-exe-check");
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("pumpkin.exe");
        let mut file = fs::File::create(&exe).unwrap();
        file.write_all(b"MZ").unwrap();
        file.write_all(&vec![0_u8; 2 * 1024 * 1024]).unwrap();
        drop(file);
        assert!(verify_windows_exe(&exe).is_ok());
        assert!(verify_java_archive(&exe).is_err(), "the jar check rejects exe files, which is why they are separate");

        let html = dir.join("oops.exe");
        fs::write(&html, "<!DOCTYPE html><html>404</html>").unwrap();
        assert!(verify_windows_exe(&html).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
