use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use crate::metadata::format::detect;

/// 支持格式の拡張子（magic bytes チェックの前段フィルタ）
const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "arw",
];

pub fn scan_dir(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| has_supported_extension(e.path()))
        .filter(|e| is_supported_by_magic(e.path()))
        .map(|e| e.into_path())
        .collect()
}

fn has_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn is_supported_by_magic(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut header = [0u8; 12];
    let n = f.read(&mut header).unwrap_or(0);
    detect(path, &header[..n]).is_supported()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn manifest_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn scan_finds_jpeg_fixtures() {
        let fixtures = manifest_dir().join("tests/fixtures");
        let found = scan_dir(&fixtures);
        assert!(!found.is_empty(), "should find at least one image");
        assert!(found.iter().all(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str())
        }));
    }

    #[test]
    fn scan_skips_unsupported_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("readme.txt"), b"hello").unwrap();
        fs::write(dir.path().join("data.bin"), b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0A\x0B").unwrap();
        let found = scan_dir(dir.path());
        assert!(found.is_empty(), "should skip non-image files");
    }

    #[test]
    fn scan_empty_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let found = scan_dir(dir.path());
        assert!(found.is_empty());
    }

    #[test]
    fn scan_recurses_subdirectories() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("2024/06");
        fs::create_dir_all(&sub).unwrap();

        let src = manifest_dir().join("tests/fixtures/no_exif.jpg");
        fs::copy(&src, sub.join("photo.jpg")).unwrap();

        let found = scan_dir(dir.path());
        assert_eq!(found.len(), 1);
    }
}
