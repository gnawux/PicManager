use std::collections::{HashMap, HashSet};
use std::path::Path;
use sqlx::SqlitePool;
use crate::error::Result;
use super::hash::{
    hamming_distance, is_degenerate,
    compute_dcthash, dcthash_distance, DCT_THRESHOLD,
    NEARBY_SECS, SIMILARITY_THRESHOLD, SIMILARITY_THRESHOLD_FAR,
};

/// Incremental scan: compare only photos that have not been scanned yet
/// against all previously scanned photos (and against each other).
/// Returns the number of new dedup groups created or extended.
pub async fn scan(pool: &SqlitePool) -> Result<usize> {
    let new_rows: Vec<(i64, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, phash, taken_at, path FROM photos
         WHERE phash IS NOT NULL AND import_status = 'imported' AND dedup_scanned_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    if new_rows.is_empty() {
        return Ok(0);
    }

    let old_rows: Vec<(i64, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, phash, taken_at, path FROM photos
         WHERE phash IS NOT NULL AND import_status = 'imported' AND dedup_scanned_at IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let mut burst_pairs: Vec<(i64, i64)> = Vec::new();
    let mut far_pairs: Vec<(i64, i64)> = Vec::new();
    let mut dct_cache: HashMap<i64, Option<u64>> = HashMap::new();

    for (id_a, hash_a, ts_a, path_a) in &new_rows {
        for (id_b, hash_b, ts_b, path_b) in &old_rows {
            if should_pair(
                hash_a, ts_a.as_deref(), path_a, *id_a,
                hash_b, ts_b.as_deref(), path_b, *id_b,
                &mut dct_cache,
            ) {
                push_pair(*id_a, *id_b, ts_a.as_deref(), ts_b.as_deref(), &mut burst_pairs, &mut far_pairs);
            }
        }
    }

    for i in 0..new_rows.len() {
        for j in (i + 1)..new_rows.len() {
            let (id_a, hash_a, ts_a, path_a) = &new_rows[i];
            let (id_b, hash_b, ts_b, path_b) = &new_rows[j];
            if should_pair(
                hash_a, ts_a.as_deref(), path_a, *id_a,
                hash_b, ts_b.as_deref(), path_b, *id_b,
                &mut dct_cache,
            ) {
                push_pair(*id_a, *id_b, ts_a.as_deref(), ts_b.as_deref(), &mut burst_pairs, &mut far_pairs);
            }
        }
    }

    let groups_created = write_clusters_incremental(pool, &burst_pairs, &far_pairs).await?;

    let ids: Vec<i64> = new_rows.iter().map(|(id, _, _, _)| *id).collect();
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

    // Clear pending groups so the rescan starts with a clean slate.
    // Resolved groups (user-confirmed decisions) are preserved.
    sqlx::query(
        "DELETE FROM dedup_members WHERE group_id IN \
         (SELECT id FROM dedup_groups WHERE status = 'pending')",
    )
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM dedup_groups WHERE status = 'pending'")
        .execute(pool)
        .await?;

    let all_rows: Vec<(i64, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, phash, taken_at, path FROM photos WHERE phash IS NOT NULL AND import_status = 'imported'",
    )
    .fetch_all(pool)
    .await?;

    if all_rows.is_empty() {
        return Ok(0);
    }

    // Parse each hash string into 8 raw bytes; skip degenerate or unparseable hashes.
    let parsed: Vec<(i64, [u8; 8], Option<String>, String)> = all_rows
        .iter()
        .filter(|(_, s, _, _)| !is_degenerate(s))
        .filter_map(|(id, s, ts, path)| Some((*id, hash_bytes(s)?, ts.clone(), path.clone())))
        .collect();

    // Build 4 inverted indexes: segment_index → segment_value → [position in `parsed`].
    let mut tables: [HashMap<u16, Vec<usize>>; 4] = Default::default();
    for (idx, (_, bytes, _, _)) in parsed.iter().enumerate() {
        for (seg_i, seg_val) in extract_segments(bytes).iter().enumerate() {
            tables[seg_i].entry(*seg_val).or_default().push(idx);
        }
    }

    let mut checked: HashSet<(usize, usize)> = HashSet::new();
    let mut dct_cache: HashMap<i64, Option<u64>> = HashMap::new();
    let mut burst_pairs: Vec<(i64, i64)> = Vec::new();
    let mut far_pairs: Vec<(i64, i64)> = Vec::new();

    for (idx_a, (id_a, bytes_a, ts_a, path_a)) in parsed.iter().enumerate() {
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
                    let (id_b, bytes_b, ts_b, path_b) = &parsed[idx_b];
                    let threshold = time_threshold(ts_a.as_deref(), ts_b.as_deref());
                    if hamming_bytes(bytes_a, bytes_b) <= threshold {
                        let ha = *dct_cache.entry(*id_a).or_insert_with(|| compute_dcthash(Path::new(path_a)));
                        let hb = *dct_cache.entry(*id_b).or_insert_with(|| compute_dcthash(Path::new(path_b)));
                        let dct_ok = match (ha, hb) {
                            (Some(a), Some(b)) => dcthash_distance(a, b) <= DCT_THRESHOLD,
                            _ => true,
                        };
                        if dct_ok {
                            push_pair(*id_a, *id_b, ts_a.as_deref(), ts_b.as_deref(), &mut burst_pairs, &mut far_pairs);
                        }
                    }
                }
            }
        }
    }

    let groups_created = write_clusters(pool, &burst_pairs, &far_pairs).await?;

    sqlx::query(
        "UPDATE photos SET dedup_scanned_at = datetime('now')
         WHERE phash IS NOT NULL AND import_status = 'imported'",
    )
    .execute(pool)
    .await?;

    Ok(groups_created)
}

// --- cluster writers ---

/// Write pairs as clusters (connected components) into the DB.
/// Burst pairs use their own Union-Find; far pairs use a separate Union-Find
/// that excludes photos already in a burst cluster, preventing structural-similarity
/// chains from contaminating genuine burst-shot groups.
async fn write_clusters(
    pool: &SqlitePool,
    burst_pairs: &[(i64, i64)],
    far_pairs: &[(i64, i64)],
) -> Result<usize> {
    let (burst_comps, in_burst) = burst_components(burst_pairs);
    let far_comps = far_components(far_pairs, &in_burst);

    let mut groups_created = 0;
    for (_, mut members) in burst_comps.into_iter().chain(far_comps) {
        if members.len() < 2 {
            continue;
        }
        members.sort_unstable();
        let group_id = sqlx::query("INSERT INTO dedup_groups (status) VALUES ('pending')")
            .execute(pool)
            .await?
            .last_insert_rowid();
        for photo_id in &members {
            sqlx::query("INSERT INTO dedup_members (group_id, photo_id) VALUES (?, ?)")
                .bind(group_id)
                .bind(photo_id)
                .execute(pool)
                .await?;
        }
        groups_created += 1;
    }
    Ok(groups_created)
}

/// Write pairs as clusters into the DB, merging with any pre-existing pending groups.
/// Used by the incremental scan where old photos may already belong to groups.
/// Burst and far pairs use separate Union-Finds (see write_clusters for rationale).
async fn write_clusters_incremental(
    pool: &SqlitePool,
    burst_pairs: &[(i64, i64)],
    far_pairs: &[(i64, i64)],
) -> Result<usize> {
    if burst_pairs.is_empty() && far_pairs.is_empty() {
        return Ok(0);
    }
    let (burst_comps, in_burst) = burst_components(burst_pairs);
    let far_comps = far_components(far_pairs, &in_burst);

    let mut groups_created = 0;
    for (_, mut members) in burst_comps.into_iter().chain(far_comps) {
        if members.len() < 2 {
            continue;
        }
        members.sort_unstable();

        // Find existing pending groups that contain any photo in this cluster.
        let placeholders = members.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql = format!(
            "SELECT DISTINCT dm.group_id FROM dedup_members dm
             JOIN dedup_groups dg ON dg.id = dm.group_id AND dg.status = 'pending'
             WHERE dm.photo_id IN ({placeholders})
             ORDER BY dm.group_id"
        );
        let mut q = sqlx::query_as::<_, (i64,)>(&sql);
        for &m in &members {
            q = q.bind(m);
        }
        let existing: Vec<i64> = q.fetch_all(pool).await?.into_iter().map(|(id,)| id).collect();

        let target = if existing.is_empty() {
            let gid = sqlx::query("INSERT INTO dedup_groups (status) VALUES ('pending')")
                .execute(pool)
                .await?
                .last_insert_rowid();
            groups_created += 1;
            gid
        } else {
            let gid = existing[0];
            // Merge extra groups into the target: remove duplicates first, then move.
            for &extra in &existing[1..] {
                sqlx::query(
                    "DELETE FROM dedup_members WHERE group_id = ? AND photo_id IN \
                     (SELECT photo_id FROM dedup_members WHERE group_id = ?)",
                )
                .bind(extra)
                .bind(gid)
                .execute(pool)
                .await?;
                sqlx::query("UPDATE dedup_members SET group_id = ? WHERE group_id = ?")
                    .bind(gid)
                    .bind(extra)
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM dedup_groups WHERE id = ?")
                    .bind(extra)
                    .execute(pool)
                    .await?;
            }
            gid
        };

        // Add any cluster members not yet in the target group.
        for photo_id in &members {
            let exists: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM dedup_members WHERE group_id = ? AND photo_id = ?",
            )
            .bind(target)
            .bind(photo_id)
            .fetch_one(pool)
            .await?;
            if exists.0 == 0 {
                sqlx::query("INSERT INTO dedup_members (group_id, photo_id) VALUES (?, ?)")
                    .bind(target)
                    .bind(photo_id)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(groups_created)
}

// --- cluster helpers ---

/// Route a matched pair into the burst or far bucket based on taken_at timestamps.
fn push_pair(
    id_a: i64, id_b: i64,
    ts_a: Option<&str>, ts_b: Option<&str>,
    burst: &mut Vec<(i64, i64)>,
    far: &mut Vec<(i64, i64)>,
) {
    if is_burst_pair(ts_a, ts_b) { burst.push((id_a, id_b)); } else { far.push((id_a, id_b)); }
}

/// Build Union-Find components for burst pairs and return (components, set of burst photo IDs).
fn burst_components(pairs: &[(i64, i64)]) -> (HashMap<i64, Vec<i64>>, HashSet<i64>) {
    let mut uf = UnionFind::new();
    for &(a, b) in pairs { uf.union(a, b); }
    let comps = uf.components();
    let mut in_burst = HashSet::new();
    for ms in comps.values() { for &m in ms { in_burst.insert(m); } }
    (comps, in_burst)
}

/// Build Union-Find components for far pairs, skipping pairs where either photo
/// is already in a burst cluster (prevents structural similarity from contaminating
/// genuine burst-shot groups).
fn far_components(pairs: &[(i64, i64)], in_burst: &HashSet<i64>) -> HashMap<i64, Vec<i64>> {
    let mut uf = UnionFind::new();
    for &(a, b) in pairs {
        if !in_burst.contains(&a) && !in_burst.contains(&b) { uf.union(a, b); }
    }
    uf.components()
}

/// Returns true if both timestamps are known and within NEARBY_SECS of each other.
fn is_burst_pair(ts_a: Option<&str>, ts_b: Option<&str>) -> bool {
    match (ts_a, ts_b) {
        (Some(a), Some(b)) => parse_secs_diff(a, b) <= NEARBY_SECS,
        _ => false,
    }
}

// --- shared helpers ---

/// Returns true if the two photos pass both Layer-1 (Gradient pHash) and
/// Layer-2 (DCT pHash) similarity checks.
fn should_pair(
    hash_a: &str,
    ts_a: Option<&str>,
    path_a: &str,
    id_a: i64,
    hash_b: &str,
    ts_b: Option<&str>,
    path_b: &str,
    id_b: i64,
    dct_cache: &mut HashMap<i64, Option<u64>>,
) -> bool {
    if is_degenerate(hash_a) || is_degenerate(hash_b) {
        return false;
    }
    let dist = match hamming_distance(hash_a, hash_b) {
        Some(d) => d,
        None => return false,
    };
    if dist > time_threshold(ts_a, ts_b) {
        return false;
    }
    // Layer 2: DCT pHash. Falls through (accepts) when either image cannot be opened.
    let ha = *dct_cache.entry(id_a).or_insert_with(|| compute_dcthash(Path::new(path_a)));
    let hb = *dct_cache.entry(id_b).or_insert_with(|| compute_dcthash(Path::new(path_b)));
    if let (Some(a), Some(b)) = (ha, hb) {
        if dcthash_distance(a, b) > DCT_THRESHOLD {
            return false;
        }
    }
    true
}

/// Returns the applicable Hamming distance threshold based on how far apart two photos were taken.
fn time_threshold(ts_a: Option<&str>, ts_b: Option<&str>) -> u32 {
    let secs = match (ts_a, ts_b) {
        (Some(a), Some(b)) => parse_secs_diff(a, b),
        _ => i64::MAX,
    };
    if secs <= NEARBY_SECS { SIMILARITY_THRESHOLD } else { SIMILARITY_THRESHOLD_FAR }
}

/// Parse two SQLite datetime strings ("YYYY-MM-DD HH:MM:SS") and return absolute difference in seconds.
fn parse_secs_diff(a: &str, b: &str) -> i64 {
    fn to_secs(s: &str) -> Option<i64> {
        let s = s.trim();
        if s.len() < 19 { return None; }
        let yr: i64 = s[0..4].parse().ok()?;
        let mo: i64 = s[5..7].parse().ok()?;
        let dy: i64 = s[8..10].parse().ok()?;
        let hr: i64 = s[11..13].parse().ok()?;
        let mn: i64 = s[14..16].parse().ok()?;
        let sc: i64 = s[17..19].parse().ok()?;
        Some(((yr * 365 + mo * 30 + dy) * 86400) + hr * 3600 + mn * 60 + sc)
    }
    match (to_secs(a), to_secs(b)) {
        (Some(sa), Some(sb)) => (sa - sb).abs(),
        _ => i64::MAX,
    }
}

// --- Union-Find ---

struct UnionFind {
    parent: HashMap<i64, i64>,
}

impl UnionFind {
    fn new() -> Self {
        Self { parent: HashMap::new() }
    }

    fn find(&mut self, x: i64) -> i64 {
        if !self.parent.contains_key(&x) {
            self.parent.insert(x, x);
            return x;
        }
        // Iterative path compression.
        let mut root = x;
        while self.parent[&root] != root {
            root = self.parent[&root];
        }
        let mut node = x;
        while node != root {
            let next = self.parent[&node];
            *self.parent.get_mut(&node).unwrap() = root;
            node = next;
        }
        root
    }

    fn union(&mut self, x: i64, y: i64) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx != ry {
            self.parent.insert(ry, rx);
        }
    }

    fn components(mut self) -> HashMap<i64, Vec<i64>> {
        let keys: Vec<i64> = self.parent.keys().cloned().collect();
        let mut result: HashMap<i64, Vec<i64>> = HashMap::new();
        for k in keys {
            let root = self.find(k);
            result.entry(root).or_default().push(k);
        }
        result
    }
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
            "INSERT INTO photos (path, sha256, format, phash, import_status, taken_at) \
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

    async fn insert_photo_at(pool: &SqlitePool, path: &str, phash: &str, taken_at: &str) -> i64 {
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, phash, import_status, taken_at) \
             VALUES (?, ?, 'jpeg', ?, 'imported', ?)",
        )
        .bind(path).bind(path).bind(phash).bind(taken_at)
        .execute(pool).await.unwrap().last_insert_rowid()
    }

    async fn insert_scanned_photo(pool: &SqlitePool, path: &str, phash: &str) -> i64 {
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, phash, import_status, dedup_scanned_at, taken_at) \
             VALUES (?, ?, 'jpeg', ?, 'imported', datetime('now'), datetime('now'))",
        )
        .bind(path)
        .bind(path)
        .bind(phash)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
    }

    async fn group_count(pool: &SqlitePool) -> i64 {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM dedup_groups WHERE status = 'pending'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        n
    }

    async fn member_count(pool: &SqlitePool, group_id: i64) -> i64 {
        let (n,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM dedup_members WHERE group_id = ?")
                .bind(group_id)
                .fetch_one(pool)
                .await
                .unwrap();
        n
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
        let a = [0u8; 8];
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
        assert!(min_seg_dist <= 2, "pigeonhole: min={min_seg_dist}");
    }

    #[test]
    fn pigeonhole_does_not_apply_above_threshold() {
        let a = [0u8; 8];
        let b: [u8; 8] = [0xFF, 0x07, 0, 0, 0, 0, 0, 0];
        assert_eq!(hamming_bytes(&a, &b), 11);
    }

    #[test]
    fn is_burst_pair_within_60s() {
        assert!(is_burst_pair(Some("2020-01-01 12:00:00"), Some("2020-01-01 12:01:00")));
        assert!(!is_burst_pair(Some("2020-01-01 12:00:00"), Some("2020-01-01 12:01:01")));
        assert!(!is_burst_pair(None, Some("2020-01-01 12:00:00")));
    }

    #[test]
    fn union_find_clusters_correctly() {
        let mut uf = UnionFind::new();
        uf.union(1, 2);
        uf.union(2, 3);
        uf.union(5, 6);
        let comps = uf.components();
        // Should produce two clusters: {1,2,3} and {5,6}
        assert_eq!(comps.len(), 2);
        let sizes: HashSet<usize> = comps.values().map(|v| v.len()).collect();
        assert!(sizes.contains(&3));
        assert!(sizes.contains(&2));
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
        assert_eq!(scan(&pool).await.unwrap(), 1);
        assert_eq!(group_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn three_similar_photos_form_one_cluster() {
        let pool = test_pool().await;
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        insert_photo(&pool, "/a.jpg", Some(&h)).await;
        insert_photo(&pool, "/b.jpg", Some(&h)).await;
        insert_photo(&pool, "/c.jpg", Some(&h)).await;
        assert_eq!(scan(&pool).await.unwrap(), 1);
        assert_eq!(group_count(&pool).await, 1);
        // The single group must contain all three photos.
        let (group_id,): (i64,) =
            sqlx::query_as("SELECT id FROM dedup_groups WHERE status = 'pending'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(member_count(&pool, group_id).await, 3);
    }

    #[tokio::test]
    async fn incremental_joins_new_photo_into_existing_cluster() {
        let pool = test_pool().await;
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        // First scan: A and B form a cluster.
        insert_photo(&pool, "/a.jpg", Some(&h)).await;
        insert_photo(&pool, "/b.jpg", Some(&h)).await;
        scan(&pool).await.unwrap();
        assert_eq!(group_count(&pool).await, 1);
        // Second scan: C arrives and matches A (and transitively B).
        insert_photo(&pool, "/c.jpg", Some(&h)).await;
        scan(&pool).await.unwrap();
        // Still one group, now with 3 members.
        assert_eq!(group_count(&pool).await, 1);
        let (group_id,): (i64,) =
            sqlx::query_as("SELECT id FROM dedup_groups WHERE status = 'pending'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(member_count(&pool, group_id).await, 3);
    }

    #[tokio::test]
    async fn scan_marks_photos_as_scanned() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", Some("AAAA")).await;
        scan(&pool).await.unwrap();
        let (unscanned,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM photos WHERE dedup_scanned_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(unscanned, 0);
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
        assert_eq!(scan(&pool).await.unwrap(), 1);
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
        insert_scanned_photo(&pool, "/a.jpg", &h1).await;
        insert_scanned_photo(&pool, "/b.jpg", &h2).await;
        assert_eq!(scan_full(&pool).await.unwrap(), 1, "similar images should form one group");
    }

    #[tokio::test]
    async fn scan_full_does_not_group_different_images() {
        let pool = test_pool().await;
        let h1 = compute_phash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_phash(&fixture("different.jpg")).unwrap();
        insert_scanned_photo(&pool, "/a.jpg", &h1).await;
        insert_scanned_photo(&pool, "/b.jpg", &h2).await;
        assert_eq!(scan_full(&pool).await.unwrap(), 0, "different images should not be grouped");
    }

    #[tokio::test]
    async fn scan_full_three_similar_photos_one_cluster() {
        let pool = test_pool().await;
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        insert_scanned_photo(&pool, "/a.jpg", &h).await;
        insert_scanned_photo(&pool, "/b.jpg", &h).await;
        insert_scanned_photo(&pool, "/c.jpg", &h).await;
        assert_eq!(scan_full(&pool).await.unwrap(), 1);
        assert_eq!(group_count(&pool).await, 1);
        let (group_id,): (i64,) =
            sqlx::query_as("SELECT id FROM dedup_groups WHERE status = 'pending'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(member_count(&pool, group_id).await, 3);
    }

    #[tokio::test]
    async fn scan_full_matches_incremental_scan_results() {
        let pool_bucketed = test_pool().await;
        let pool_brute = test_pool().await;

        let h1 = compute_phash(&fixture("with_exif.jpg")).unwrap();
        let h2 = compute_phash(&fixture("with_exif_small.jpg")).unwrap();
        let h3 = compute_phash(&fixture("different.jpg")).unwrap();

        for (path, hash) in [("/a.jpg", &h1), ("/b.jpg", &h2), ("/c.jpg", &h3)] {
            insert_scanned_photo(&pool_bucketed, path, hash).await;
            insert_photo(&pool_brute, path, Some(hash)).await;
        }

        let bucketed = scan_full(&pool_bucketed).await.unwrap();
        let brute = scan(&pool_brute).await.unwrap();
        assert_eq!(bucketed, brute, "bucketed and brute-force must agree");
    }

    #[tokio::test]
    async fn burst_cluster_isolated_from_far_structural_similarity() {
        // A and B are burst shots (1 second apart, identical hash).
        // C has the same hash but is from a year later → far pair.
        // Expected: {A, B} is a burst cluster; C is not added because A and B
        // are in the burst cluster (far pair involving burst photos is skipped).
        let pool = test_pool().await;
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        insert_photo_at(&pool, "/a.jpg", &h, "2020-01-01 12:00:00").await;
        insert_photo_at(&pool, "/b.jpg", &h, "2020-01-01 12:00:01").await;
        insert_photo_at(&pool, "/c.jpg", &h, "2021-06-01 09:00:00").await;
        scan(&pool).await.unwrap();
        // Only 1 group: the burst cluster {A, B}. C is not in any group.
        assert_eq!(group_count(&pool).await, 1);
        let (group_id,): (i64,) =
            sqlx::query_as("SELECT id FROM dedup_groups WHERE status = 'pending'")
                .fetch_one(&pool).await.unwrap();
        assert_eq!(member_count(&pool, group_id).await, 2);
    }

    #[tokio::test]
    async fn scan_full_marks_all_photos_scanned() {
        let pool = test_pool().await;
        let h = compute_phash(&fixture("with_exif.jpg")).unwrap();
        insert_scanned_photo(&pool, "/a.jpg", &h).await;
        scan_full(&pool).await.unwrap();
        let (unscanned,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM photos WHERE dedup_scanned_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(unscanned, 0);
    }
}
