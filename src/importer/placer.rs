use chrono::NaiveDate;
use std::path::{Path, PathBuf};
use crate::error::{AppError, Result};

/// Move (or copy) `src` into `library_path/{yyyy-mm-dd}/` or `library_path/unknown/`.
///
/// - `copy_only = false` (default): rename; cross-device falls back to copy + delete source.
/// - `copy_only = true`: copy only; source file is preserved.
///
/// Returns the final path of the file inside the library.
pub fn place(src: &Path, library_path: &Path, date: Option<NaiveDate>, copy_only: bool) -> Result<PathBuf> {
    let dir_name = date.map_or_else(
        || "unknown".to_string(),
        |d| d.format("%Y-%m-%d").to_string(),
    );
    let target_dir = library_path.join(&dir_name);
    std::fs::create_dir_all(&target_dir)?;

    let filename = src.file_name().ok_or_else(|| {
        AppError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, "source path has no filename"))
    })?;
    let target = unique_path(&target_dir, filename);

    if copy_only {
        std::fs::copy(src, &target)?;
    } else {
        move_file(src, &target)?;
    }
    Ok(target)
}

fn move_file(src: &Path, dst: &Path) -> Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            std::fs::copy(src, dst)?;
            std::fs::remove_file(src)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn unique_path(dir: &Path, filename: &std::ffi::OsStr) -> PathBuf {
    let base = Path::new(filename);
    let stem = base.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = base.extension().and_then(|s| s.to_str());

    let first = dir.join(filename);
    if !first.exists() {
        return first;
    }
    for n in 1u32.. {
        let name = match ext {
            Some(e) => format!("{stem}_{n}.{e}"),
            None => format!("{stem}_{n}"),
        };
        let candidate = dir.join(&name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_file(path: &Path, content: &[u8]) {
        fs::write(path, content).unwrap();
    }

    #[test]
    fn places_file_in_date_dir() {
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();
        let src = src_dir.path().join("photo.jpg");
        write_file(&src, b"data");

        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let dest = place(&src, lib_dir.path(), Some(date), false).unwrap();

        assert_eq!(dest, lib_dir.path().join("2024-06-15/photo.jpg"));
        assert!(dest.exists());
        assert!(!src.exists(), "source should be removed on move");
    }

    #[test]
    fn copy_only_preserves_source() {
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();
        let src = src_dir.path().join("photo.jpg");
        write_file(&src, b"data");

        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let dest = place(&src, lib_dir.path(), Some(date), true).unwrap();

        assert!(dest.exists(), "copy should exist");
        assert!(src.exists(), "source should be preserved with --copy");
    }

    #[test]
    fn no_date_goes_to_unknown() {
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();
        let src = src_dir.path().join("unknown_photo.jpg");
        write_file(&src, b"data");

        let dest = place(&src, lib_dir.path(), None, false).unwrap();

        assert_eq!(dest, lib_dir.path().join("unknown/unknown_photo.jpg"));
        assert!(dest.exists());
    }

    #[test]
    fn conflict_gets_numeric_suffix() {
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();
        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();

        // Pre-populate target with a file of the same name.
        let date_dir = lib_dir.path().join("2024-06-15");
        fs::create_dir_all(&date_dir).unwrap();
        write_file(&date_dir.join("photo.jpg"), b"existing");

        let src = src_dir.path().join("photo.jpg");
        write_file(&src, b"new content");

        let dest = place(&src, lib_dir.path(), Some(date), false).unwrap();
        assert_eq!(dest.file_name().unwrap(), "photo_1.jpg");
        assert!(dest.exists());
    }

    #[test]
    fn conflict_increments_until_free() {
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();
        let date = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();

        let date_dir = lib_dir.path().join("2024-06-15");
        fs::create_dir_all(&date_dir).unwrap();
        write_file(&date_dir.join("photo.jpg"), b"0");
        write_file(&date_dir.join("photo_1.jpg"), b"1");

        let src = src_dir.path().join("photo.jpg");
        write_file(&src, b"new");

        let dest = place(&src, lib_dir.path(), Some(date), false).unwrap();
        assert_eq!(dest.file_name().unwrap(), "photo_2.jpg");
    }
}
