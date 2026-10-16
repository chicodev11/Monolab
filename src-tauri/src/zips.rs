
use std::io::{Read, Seek};

use zip::ZipArchive;

pub const MAX_UNPACKED: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 50_000;

pub fn check_budget<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    max_unpacked: u64,
    max_entries: usize,
) -> Result<(), String> {
    if archive.len() > max_entries {
        return Err(format!(
            "The archive has too many entries ({}). Up to {max_entries} are unpacked.",
            archive.len()
        ));
    }
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|error| error.to_string())?;
        if entry.is_symlink() {
            return Err(format!("The archive contains a symbolic link, so it was not unpacked: {}", entry.name()));
        }
        total = total.saturating_add(entry.size());
        if total > max_unpacked {
            return Err(format!(
                "Unpacking would exceed {} MB. Up to {} MB is unpacked.",
                total / 1024 / 1024,
                max_unpacked / 1024 / 1024
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    fn zip_with(entries: &[(&str, usize)]) -> ZipArchive<Cursor<Vec<u8>>> {
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, size) in entries {
            out.start_file(*name, SimpleFileOptions::default()).unwrap();
            out.write_all(&vec![0u8; *size]).unwrap();
        }
        let cursor = out.finish().unwrap();
        ZipArchive::new(Cursor::new(cursor.into_inner())).unwrap()
    }

    #[test]
    fn refuses_bombs_before_unpacking() {
        let mut fine = zip_with(&[("config/a.toml", 100), ("mods/b.jar", 100)]);
        assert!(check_budget(&mut fine, 1000, 10).is_ok());
        let mut big = zip_with(&[("a", 1024 * 1024), ("b", 1024 * 1024)]);
        assert!(check_budget(&mut big, 1024 * 1024 + 1, 10).is_err());
        let mut many = zip_with(&[("a", 1), ("b", 1), ("c", 1)]);
        assert!(check_budget(&mut many, 1000, 2).is_err());
    }
}
