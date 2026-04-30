use std::collections::{HashMap, HashSet};
use sqlx::SqlitePool;
use crate::error::Result;
use super::hash::{hamming_distance, is_degenerate, SIMILARITY_THRESHOLD};

/// Incremental scan: compare only photos that have not been scanned yet
/// against all previously scanned photos (and against each other).
/// Returns the number of new dedup groups created.
pub async fn scan(pool: &SqlitePool) -> Result<usize> {
    let new_rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, phash FROM photos
         WHERE phash IS NOT NULL AND import_status = 'imported' AND dedup_scanned_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    if new_rows.is_empty() {
        return Ok(0);
    }

    let old_rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, phash FROM photos
         WHERE phash IS NOT NULL AND import_status = 'imported' AND dedup_scanned_at IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let mut groups_created = 0usize;

    for (id_a, hash_a) in &new_rows {
        for (id_b, hash_b) in &old_rows {
            groups_created +=
                maybe_create_group(pool, *id_a, hash_a, *id_b, hash_b).await?;
        }
    }

    for i in 0..new_rows.len() {
        for j in (i + 1)..new_rows.len() {
            let (id_a, hash_a) = &new_rows[i];
            let (id_b, hash_b) = &new_rows[j];
            groups_created +=
                maybe_create_group(pool, *id_a, hash_a, *id_b, hash_b).await?;
        }
    }

    let ids: Vec<i64> = new_rows.iter().map(|(id, _)| *id).collect();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "UPDATE photos SET dedup_scanned_at = datetime('now') WHERE id IN ({placeholders})"
    );
    let mut q = sqlx::query(&sql);
    for id in &ids {
        q = q.bind(id);
    }
    q.execute(pool).await?;

    Ok(groups_created)
}

/// Full rescan using multi-index hashing (4 × 16-bit segments).
///
/// # Correctness guarantee
/// If two 64-bit hashes have Hamming distance ≤ 10, at least one of the four
/// 16-bit segments must have distance ≤ 2 (pigeonhole: if all four had ≥ 3
/// differences the total would be ≥ 12 > 10). We therefore look up all
/// candidates whose corresponding segment is within Hamming distance 2.
pub async fn scan_full(pool: &SqlitePool) -> Result<usize> {
    sqlx::query("UPDATE photos SET dedup_scanned_at = NULL")
        .execute(pool)
        .await?;

    let all_rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, phash FROM photos WHERE phash IS NOT NULL AND import_status = 'imported'",
    )
    .fetch_all(pool)
    .await?;

    if all_rows.is_empty() {
        return Ok(0);
    }

    // Parse each hash string into 8 raw bytes; skip degenerate or unparseable hashes.
    let parsed: Vec<(i64, [u8; 8])> = all_rows
        .iter()
        .filter(|(_, s)| !is_degenerate(s))
        .filter_map(|(id, s)| Some((*id, hash_bytes(s)?)))
        .collect();

    // Build 4 inverted indexes: segment_index → segment_value → [position in `parsed`].
    let mut tables: [HashMap<u16, Vec<usize>>; 4] = Default::default();
    for (idx, (_, bytes)) in parsed.iter().enumerate() {
        for (seg_i, seg_val) in extract_segments(bytes).iter().enumerate() {
            tables[seg_i].entry(*seg_val).or_default().push(idx);
        }
    }

    let mut checked: HashSet<(usize, usize)> = HashSet::new();
    let mut groups_created = 0usize;

    for (idx_a, (id_a, bytes_a)) in parsed.iter().enumerate() {
        for (seg_i, seg_val_a) in extract_segments(bytes_a).iter().enumerate() {
            for neighbor in u16_neighbors(*seg_val_a, 2) {
                let Some(candidates) = tables[seg_i].get(&neighbor) else { continue };
                for &idx_b in candidates {
                    if idx_b <= idx_a {
                        continue;
                    }
                    if !checked.insert((idx_a, idx_b)) {
                        continue;
                    }
                    let (id_b, bytes_b) = &parsed[idx_b];
                    if hamming_bytes(bytes_a, bytes_b) <= SIMILARITY_THRESHOLD {
                        groups_created += create_group_if_absent(pool, *id_a, *id_b).await?;
                    }
                }
            }
        }
    }

    sqlx::query(
        "UPDATE photos SET dedup_scanned_at = datetime('now')
         WHERE phash IS NOT NULL AND import_status = 'imported'",
    )
    .execute(pool)
    .await?;

    Ok(groups_created)
}

// --- shared helpers ---

async fn maybe_create_group(
    pool: &SqlitePool,
    id_a: i64,
    hash_a: &str,
    id_b: i64,
    hash_b: &str,
) -> Result<usize> {
    if is_degenerate(hash_a) || is_degenerate(hash_b) {
        return Ok(0);
    }
    let dist = match hamming_distance(hash_a, hash_b) {
        Some(d) => d,
        None => return Ok(0),
    };
    if dist > SIMILARITY_THRESHOLD {
        return Ok(0);
    }
    create_group_if_absent(pool, id_a, id_b).await
}

async fn create_group_if_absent(pool: &SqlitePool, id_a: i64, id_b: i64) -> Result<usize> {
    let already: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM dedup_members dm1
         JOIN dedup_members dm2 ON dm1.group_id = dm2.group_id
         JOIN dedup_groups dg ON dg.id = dm1.group_id
         WHERE dm1.photo_id = ? AND dm2.photo_id = ? AND dg.status = 'pending'",
    )
    .bind(id_a)
    .bind(id_b)
    .fetch_one(pool)
    .await?;

    if already.0 > 0 {
        return Ok(0);
    }

    let group_id = sqlx::query("INSERT INTO dedup_groups (status) VALUES ('pending')")
        .execute(pool)
        .await?
        .last_insert_rowid();

    sqlx::query("INSERT INTO dedup_members (group_id, photo_id) VALUES (?, ?), (?, ?)")
        .bind(group_id)
        .bind(id_a)
        .bind(group_id)
        .bind(id_b)
        .execute(pool)
        .await?;

    Ok(1)
}

// --- bucketing helpers (pure, testable without DB) ---

/// Decode a base64 pHash string (from image_hasher) into 8 raw bytes.
fn hash_bytes(s: &str) -> Option<[u8; 8]> {
    use image_hasher::ImageHash;
    let h = ImageHash::<Box<[u8]>>::from_base64(s).ok()?;
    let b = h.as_bytes();
    if b.len() < 8 {
        return None;
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&b[..8]);
    Some(arr)
}

/// Split 8 bytes into four 16-bit big-endian segments.
fn extract_segments(bytes: &[u8; 8]) -> [u16; 4] {
    [
        u16::from_be_bytes([bytes[0], bytes[1]]),
        u16::from_be_bytes([bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
    ]
}

/// Enumerate all u16 values within Hamming distance `max_dist` of `v`.
fn u16_neighbors(v: u16, max_dist: u32) -> impl Iterator<Item = u16> {
    (0u32..=u16::MAX as u32)
        .filter(move |&x| (v ^ x as u16).count_ones() <= max_dist)
        .map(|x| x as u16)
}

/// Bit-level Hamming distance between two 8-byte arrays.
fn hamming_bytes(a: &[u8; 8], b: &[u8; 8]) -> u32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x ^ y).count_ones()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dedup::hash::compute_phash;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    async fn insert_photo(pool: &SqlitePool, path: &str, phash: Option<&str>) -> i64 {
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, phash, import_status) VALUES (?, ?, 'jpeg', ?, 'imported')",
        )
        .bind(path)
        .bind(path)
        .bind(phash)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
    }

    async fn insert_scanned_photo(pool: &SqlitePool, path: &str, phash: &str) -> i64 {
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, phash, import_status, dedup_scanned_at)
             VALUES (?, ?, 'jpeg', ?, 'imported', datetime('now'))",
        )
        .bind(path)
        .bind(path)
        .bind(phash)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
    }

    // ---- unit tests for pure helpers (no DB) ----

    #[test]
    fn extract_segments_splits_bytes_correctly() {
        let bytes: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let segs = extract_segments(&bytes);
        assert_eq!(segs[0], 0x0102);
        assert_eq!(segs[1], 0x0304);
        assert_eq!(segs[2], 0x0506);
        assert_eq!(segs[3], 0x0708);
    }

    #[test]
    fn hamming_bytes_identical_is_zero() {
        let a = [1u8, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(hamming_bytes(&a, &a), 0);
    }

    #[test]
    fn hamming_bytes_single_bit_flip() {
        let a = [0u8; 8];
        let mut b = [0u8; 8];
        b[0] = 1;
        assert_eq!(hamming_bytes(&a, &b), 1);
    }

    #[test]
    fn hamming_bytes_max_distance() {
        let a = [0u8; 8];
        let b = [0xFFu8; 8];
        assert_eq!(hamming_bytes(&a, &b), 64);
    }

    #[test]
    fn u16_neighbors_distance_zero_returns_only_self() {
        let v = 0xABCDu16;
        let neighbors: Vec<u16> = u16_neighbors(v, 0).collect();
        assert_eq!(neighbors, vec![v]);
    }

    #[test]
    fn u16_neighbors_distance_2_count() {
        // C(16,0) + C(16,1) + C(16,2) = 1 + 16 + 120 = 137
        let count = u16_neighbors(0, 2).count();
        assert_eq!(count, 137);
    }

    #[test]
    fn pigeonhole_guarantee_distance_10() {
        // If two hashes have Hamming distance ≤ 10, at least one of four 16-bit
        // segments must have distance ≤ 2.
        let a = [0u8; 8];
        // Flip exactly 10 bits: 8 in byte 0 + 2 in byte 1 = 10 total.
        let b: [u8; 8] = [0xFF, 0x03, 0, 0, 0, 0, 0, 0];
        assert_eq!(hamming_bytes(&a, &b), 10);

        let segs_a = extract_segments(&a);
        let segs_b = extract_segments(&b);
        let min_seg_dist = segs_a
            .iter()
            .zip(segs_b.iter())
            .map(|(sa, sb)| (sa ^ sb).count_ones())
            .min()
            .unwrap();
        assert!(
            min_seg_dist <= 2,
            "pigeonhole: at least one segment must have dist ≤ 2, got min={min_seg_dist}"
        );
    }

    #[test]
    fn pigeonhole_does_not_apply_above_threshold() {
        // Distance 11 — no guarantee about segments.
        let a = [0u8; 8];
        let b: [u8; 8] = [0xFF, 0x07, 0, 0, 0, 0, 0, 0]; // 8+3=11
        assert_eq!(hamming_bytes(&a, &b), 11);
        // This pair should NOT be found by scan_full (dist > SIMILARITY_THRESHOLD).
    }

    // ---- incremental scan tests ----

    #[tokio::test]
    async fn no_photos_returns_zero() {
        let pool = test_pool().await;
        assert_eq!(scan(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn no_new_photos_returns_zero() {
        let pool = test_pool().await;
        insert_scanned_photo(&pool, "/a.jpg", "AAAA").await;
        insert_scanned_photo(&pool, "/b.jpg", "AAAA").await;
        assert_eq!(scan(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn identical_hashes_creates_one_group() {
        let pool = test_pool().await;
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        insert_photo(&pool, "/a.jpg", Some(&h)).await;
        insert_photo(&pool, "/b.jpg", Some(&h)).await;
        let groups = scan(&pool).await.unwrap();
        assert_eq!(groups, 1);
    }

    #[tokio::test]
    async fn scan_marks_photos_as_scanned() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", Some("AAAA")).await;
        scan(&pool).await.unwrap();
        let unscanned: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM photos WHERE dedup_scanned_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(unscanned.0, 0);
    }

    #[tokio::test]
    async fn second_scan_without_new_photos_creates_no_groups() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", Some("AAAA")).await;
        insert_photo(&pool, "/b.jpg", Some("AAAA")).await;
        scan(&pool).await.unwrap();
        let second = scan(&pool).await.unwrap();
        assert_eq!(second, 0, "second scan with no new photos should be no-op");
    }

    #[tokio::test]
    async fn new_photo_matched_against_existing_scanned() {
        let pool = test_pool().await;
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        insert_scanned_photo(&pool, "/a.jpg", &h).await;
        insert_photo(&pool, "/b.jpg", Some(&h)).await;
        let groups = scan(&pool).await.unwrap();
        assert_eq!(groups, 1);
    }

    #[tokio::test]
    async fn photos_without_phash_are_ignored() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", None).await;
        insert_photo(&pool, "/b.jpg", None).await;
        assert_eq!(scan(&pool).await.unwrap(), 0);
    }

    // ---- scan_full (bucketed) tests ----

    #[tokio::test]
    async fn scan_full_finds_similar_fixture_images() {
        let pool = test_pool().await;
        let h1 = compute_phash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_phash(&fixture("with_exif_small.jpg")).unwrap();
        // Insert as pre-scanned so that only scan_full (not incremental scan) finds them.
        insert_scanned_photo(&pool, "/a.jpg", &h1).await;
        insert_scanned_photo(&pool, "/b.jpg", &h2).await;
        let groups = scan_full(&pool).await.unwrap();
        assert_eq!(groups, 1, "similar images should form one group");
    }

    #[tokio::test]
    async fn scan_full_does_not_group_different_images() {
        let pool = test_pool().await;
        let h1 = compute_phash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_phash(&fixture("different.jpg")).unwrap();
        insert_scanned_photo(&pool, "/a.jpg", &h1).await;
        insert_scanned_photo(&pool, "/b.jpg", &h2).await;
        let groups = scan_full(&pool).await.unwrap();
        assert_eq!(groups, 0, "different images should not be grouped");
    }

    #[tokio::test]
    async fn scan_full_matches_incremental_scan_results() {
        // Both approaches should find the same pairs.
        let pool_bucketed = test_pool().await;
        let pool_brute = test_pool().await;

        let h1 = compute_phash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_phash(&fixture("with_exif_small.jpg")).unwrap();
        let h3 = compute_phash(&fixture("different.jpg")).unwrap();

        for (path, hash) in [("/a.jpg", &h1), ("/b.jpg", &h2), ("/c.jpg", &h3)] {
            // Pre-scanned for bucketed pool (scan_full resets and re-finds).
            insert_scanned_photo(&pool_bucketed, path, hash).await;
            // Unscanned for brute pool (scan() does O(n²)).
            insert_photo(&pool_brute, path, Some(hash)).await;
        }

        let bucketed = scan_full(&pool_bucketed).await.unwrap();
        let brute = scan(&pool_brute).await.unwrap();
        assert_eq!(bucketed, brute, "bucketed and brute-force must find the same number of groups");
    }

    #[tokio::test]
    async fn scan_full_marks_all_photos_scanned() {
        let pool = test_pool().await;
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        insert_scanned_photo(&pool, "/a.jpg", &h).await;
        scan_full(&pool).await.unwrap();
        let unscanned: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM photos WHERE dedup_scanned_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(unscanned.0, 0);
    }
}
