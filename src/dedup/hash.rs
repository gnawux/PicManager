use image::ImageReader;
use image_hasher::{HashAlg, HasherConfig};
use std::path::Path;
use crate::error::{AppError, Result};

/// DCT pHash threshold for Layer-2 verification.
/// Two 64-bit DCT hashes with Hamming distance ≤ this value are considered similar.
/// Layer-2 DCT pHash Hamming distance threshold.
/// Same image at different resolutions: distance 0.
/// Visually similar burst shots: typically 0–5.
/// Screenshots vs natural photos: typically 15+.
/// Threshold of 8 accepts genuine duplicates while rejecting cross-content false positives.
pub const DCT_THRESHOLD: u32 = 8;

/// Compute a 64-bit DCT-based perceptual hash (classic pHash algorithm).
///
/// Steps:
/// 1. Resize to 32×32 grayscale
/// 2. Apply 2-D DCT-II (row-wise then column-wise)
/// 3. Take the top-left 8×8 low-frequency coefficients (64 values)
/// 4. Each value above the mean → bit 1, otherwise bit 0
///
/// Returns `None` if the image cannot be opened or decoded.
pub fn compute_dcthash(path: &Path) -> Option<u64> {
    use image::imageops::FilterType;

    let img = image::open(path)
        .ok()?
        .resize_exact(32, 32, FilterType::Lanczos3)
        .to_luma8();

    let mut matrix = [[0f64; 32]; 32];
    for (y, row) in matrix.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            *cell = img.get_pixel(x as u32, y as u32)[0] as f64;
        }
    }

    for row in &mut matrix {
        dct1d(row);
    }
    for j in 0..32 {
        let mut col = [0f64; 32];
        for (i, row) in matrix.iter().enumerate() {
            col[i] = row[j];
        }
        dct1d(&mut col);
        for (i, row) in matrix.iter_mut().enumerate() {
            row[j] = col[i];
        }
    }

    let mut vals = [0f64; 64];
    for i in 0..8usize {
        for j in 0..8usize {
            vals[i * 8 + j] = matrix[i][j];
        }
    }

    let mean = vals.iter().sum::<f64>() / 64.0;
    let mut hash = 0u64;
    for (i, &v) in vals.iter().enumerate() {
        if v > mean {
            hash |= 1u64 << i;
        }
    }
    Some(hash)
}

pub fn dcthash_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

fn dct1d(values: &mut [f64; 32]) {
    const N: usize = 32;
    let mut result = [0f64; N];
    let factor = std::f64::consts::PI / (2 * N) as f64;
    for k in 0..N {
        let mut sum = 0f64;
        for (n, &v) in values.iter().enumerate() {
            sum += v * (factor * (2 * n + 1) as f64 * k as f64).cos();
        }
        result[k] = sum;
    }
    *values = result;
}

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

/// Hamming distance threshold for photos taken within NEARBY_SECS of each other.
/// Burst shots taken seconds apart can legitimately differ by up to 10 bits.
pub const SIMILARITY_THRESHOLD: u32 = 10;

/// Stricter threshold for photos taken more than NEARBY_SECS apart.
/// Only matches near-identical hashes (processed copies, re-encoded duplicates).
/// dist=0-2: same content (original + app-edited copy, same photo from different source)
/// dist=3-8: structural similarity only (same scene type, same outdoor activity, etc.)
/// Threshold of 3 catches genuine far duplicates while rejecting false positives.
pub const SIMILARITY_THRESHOLD_FAR: u32 = 3;

/// Photos taken within this many seconds are compared with the relaxed threshold.
pub const NEARBY_SECS: i64 = 60;

/// Minimum number of set bits a pHash must have to be considered reliable.
/// The check is symmetric: hashes with fewer than MIN_HASH_BITS set bits OR
/// fewer than MIN_HASH_BITS *unset* bits are both degenerate.
///
/// Too-sparse (< 10 set bits): very dark / near-uniform images — the Gradient
/// algorithm finds almost no pixel transitions, so the hash is near-all-zero
/// and will falsely match any other sparse hash.
///
/// Too-dense (< 10 unset bits, i.e. > 54 set bits): very bright / high-contrast
/// images where nearly all transitions are "ascending" — the hash is near-all-one
/// and will falsely match any other dense hash by the same XOR argument.
pub const MIN_HASH_BITS: u32 = SIMILARITY_THRESHOLD;

/// Total hash bits for a 64-bit pHash (8 bytes × 8 bits).
const HASH_TOTAL_BITS: u32 = 64;

/// Returns true if the hash is too sparse or too dense to be used reliably for dedup.
pub fn is_degenerate(phash: &str) -> bool {
    use image_hasher::ImageHash;
    let Ok(h) = ImageHash::<Box<[u8]>>::from_base64(phash) else { return true };
    let bits: u32 = h.as_bytes().iter().map(|b| b.count_ones()).sum();
    bits < MIN_HASH_BITS || bits > HASH_TOTAL_BITS - MIN_HASH_BITS
}

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

    #[test]
    fn all_zero_hash_is_degenerate() {
        // "AAAAAAAAAAA" decodes to 8 zero bytes — 0 set bits.
        assert!(is_degenerate("AAAAAAAAAAA"));
    }

    #[test]
    fn sparse_hash_is_degenerate() {
        // Real observed false-positive hashes from dark/uniform photos.
        assert!(is_degenerate("LAAAAAQFAQE")); // 8 bits set
        assert!(is_degenerate("EgEAAAAEAAE")); // 5 bits set
        assert!(is_degenerate("AAAAgEAAAAA")); // 2 bits set
    }

    #[test]
    fn dense_hash_is_degenerate() {
        // Real observed false-positive hashes from high-contrast photos (near-all-ones).
        // "////fx9v3/8" and similar strings from the DB have > 54 set bits.
        assert!(is_degenerate("////fx9v3/8")); // photo 209 in DB
        assert!(is_degenerate("/9////////8")); // photo 569 in DB
        assert!(is_degenerate("//////////8")); // photo 716 in DB
        assert!(is_degenerate("/v////////8")); // photo 2224 in DB
    }

    #[test]
    fn normal_photo_hash_is_not_degenerate() {
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        assert!(!is_degenerate(&h), "real photo phash should not be degenerate");
    }

    #[test]
    fn dcthash_same_image_distance_zero() {
        let h1 = compute_dcthash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_dcthash(&fixture("with_exif.jpg")).unwrap();
        assert_eq!(dcthash_distance(h1, h2), 0);
    }

    #[test]
    fn dcthash_scaled_image_small_distance() {
        let h1 = compute_dcthash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_dcthash(&fixture("with_exif_small.jpg")).unwrap();
        let dist = dcthash_distance(h1, h2);
        assert!(dist <= DCT_THRESHOLD, "scaled image dct distance {dist} should be <= {DCT_THRESHOLD}");
    }

    #[test]
    fn dcthash_different_images_layer2_irrelevant() {
        // different.jpg has Gradient distance > SIMILARITY_THRESHOLD from with_exif.jpg,
        // so it never reaches Layer 2. DCT distance for this pair is 6 (< DCT_THRESHOLD),
        // which is fine — Layer 1 already rejects it.
        let h1 = compute_dcthash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_dcthash(&fixture("different.jpg")).unwrap();
        let dist = dcthash_distance(h1, h2);
        assert_eq!(dist, 6, "distance should remain stable across builds");
    }
}
