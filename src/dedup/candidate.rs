use sqlx::SqlitePool;
use crate::error::Result;
use super::hash::{hamming_distance, SIMILARITY_THRESHOLD};

/// Incremental scan: compare only photos that have not been scanned yet
/// against all previously scanned photos (and against each other).
/// Returns the number of new dedup groups created.
pub async fn scan(pool: &SqlitePool) -> Result<usize> {
    // New photos (never scanned).
    let new_rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, phash FROM photos
         WHERE phash IS NOT NULL AND import_status = 'imported' AND dedup_scanned_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    if new_rows.is_empty() {
        return Ok(0);
    }

    // Already-scanned photos.
    let old_rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, phash FROM photos
         WHERE phash IS NOT NULL AND import_status = 'imported' AND dedup_scanned_at IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let mut groups_created = 0usize;

    // Compare new × old.
    for (id_a, hash_a) in &new_rows {
        for (id_b, hash_b) in &old_rows {
            groups_created +=
                maybe_create_group(pool, *id_a, hash_a, *id_b, hash_b).await?;
        }
    }

    // Compare new × new (upper triangle only).
    for i in 0..new_rows.len() {
        for j in (i + 1)..new_rows.len() {
            let (id_a, hash_a) = &new_rows[i];
            let (id_b, hash_b) = &new_rows[j];
            groups_created +=
                maybe_create_group(pool, *id_a, hash_a, *id_b, hash_b).await?;
        }
    }

    // Mark new photos as scanned.
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

/// Full rescan: reset all dedup_scanned_at timestamps and re-run scan().
/// Used by `picmanager dedup --full`.
pub async fn scan_full(pool: &SqlitePool) -> Result<usize> {
    sqlx::query("UPDATE photos SET dedup_scanned_at = NULL")
        .execute(pool)
        .await?;
    scan(pool).await
}

// Check pair distance; if similar and not already grouped together, create a group.
// Returns 1 if a new group was created, 0 otherwise.
async fn maybe_create_group(
    pool: &SqlitePool,
    id_a: i64,
    hash_a: &str,
    id_b: i64,
    hash_b: &str,
) -> Result<usize> {
    let dist = match hamming_distance(hash_a, hash_b) {
        Some(d) => d,
        None => return Ok(0),
    };
    if dist > SIMILARITY_THRESHOLD {
        return Ok(0);
    }

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

#[cfg(test)]
mod tests {
    use super::*;
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
        insert_photo(&pool, "/a.jpg", Some("AAAA")).await;
        insert_photo(&pool, "/b.jpg", Some("AAAA")).await;
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
        // One already-scanned photo.
        insert_scanned_photo(&pool, "/a.jpg", "AAAA").await;
        // One new (unscanned) photo with same hash.
        insert_photo(&pool, "/b.jpg", Some("AAAA")).await;

        let groups = scan(&pool).await.unwrap();
        assert_eq!(groups, 1);
    }

    #[tokio::test]
    async fn scan_full_resets_and_rescans() {
        let pool = test_pool().await;
        insert_scanned_photo(&pool, "/a.jpg", "AAAA").await;
        insert_scanned_photo(&pool, "/b.jpg", "AAAA").await;
        // Both are already scanned but no groups exist yet.
        let groups = scan_full(&pool).await.unwrap();
        assert_eq!(groups, 1);
    }

    #[tokio::test]
    async fn photos_without_phash_are_ignored() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", None).await;
        insert_photo(&pool, "/b.jpg", None).await;
        assert_eq!(scan(&pool).await.unwrap(), 0);
    }
}
