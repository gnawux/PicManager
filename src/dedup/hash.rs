use image::ImageReader;
use image_hasher::{HashAlg, HasherConfig};
use std::path::Path;
use crate::error::{AppError, Result};

pub fn compute_phash(path: &Path) -> Result<String> {
    let img = ImageReader::open(path)
        .map_err(AppError::Io)?
        .decode()
        .map_err(|e| AppError::Metadata(e.to_string()))?;

    let hasher = HasherConfig::new().hash_alg(HashAlg::Gradient).to_hasher();
    let hash = hasher.hash_image(&img);
    Ok(hash.to_base64())
}

pub fn hamming_distance(a: &str, b: &str) -> Option<u32> {
    use image_hasher::ImageHash;
    let ha = ImageHash::<Box<[u8]>>::from_base64(a).ok()?;
    let hb = ImageHash::<Box<[u8]>>::from_base64(b).ok()?;
    Some(ha.dist(&hb))
}

pub const SIMILARITY_THRESHOLD: u32 = 10;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn phash_is_deterministic() {
        let h1 = compute_phash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_phash(&fixture("with_exif.jpg")).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn scaled_image_has_small_distance() {
        let h_orig = compute_phash(&fixture("with_exif.jpg")).unwrap();
        let h_small = compute_phash(&fixture("with_exif_small.jpg")).unwrap();
        let dist = hamming_distance(&h_orig, &h_small).unwrap();
        assert!(
            dist <= SIMILARITY_THRESHOLD,
            "scaled image distance {dist} should be <= {SIMILARITY_THRESHOLD}"
        );
    }

    #[test]
    fn different_images_have_large_distance() {
        let h1 = compute_phash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_phash(&fixture("different.jpg")).unwrap();
        let dist = hamming_distance(&h1, &h2).unwrap();
        assert!(
            dist > SIMILARITY_THRESHOLD,
            "different images distance {dist} should be > {SIMILARITY_THRESHOLD}"
        );
    }

    #[test]
    fn invalid_base64_returns_none() {
        assert!(hamming_distance("not_valid!!!", "also_not").is_none());
    }
}
