use sqlx::SqlitePool;
use crate::error::Result;
use super::hash::{hamming_distance, SIMILARITY_THRESHOLD};

/// Scan all photos with a phash and group visually similar ones.
/// Returns the number of new dedup groups created.
pub async fn scan(pool: &SqlitePool) -> Result<usize> {
    // Fetch all photos that have a phash computed
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, phash FROM photos WHERE phash IS NOT NULL AND import_status = 'imported'")
            .fetch_all(pool)
            .await?;

    let mut groups_created = 0usize;

    // O(n²) comparison — acceptable for personal photo libraries (< 100k photos)
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            let (id_a, hash_a) = &rows[i];
            let (id_b, hash_b) = &rows[j];

            let dist = match hamming_distance(hash_a, hash_b) {
                Some(d) => d,
                None => continue,
            };
            if dist > SIMILARITY_THRESHOLD {
                continue;
            }

            // Check if either photo is already in a pending group together
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
                continue;
            }

            // Create a new group and insert both members
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

            groups_created += 1;
        }
    }

    Ok(groups_created)
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
        .bind(path) // reuse path as fake sha256
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
    async fn identical_hashes_creates_one_group() {
        let pool = test_pool().await;
        // Use the same hash — distance is 0
        insert_photo(&pool, "/a.jpg", Some("AAAA")).await;
        insert_photo(&pool, "/b.jpg", Some("AAAA")).await;
        let groups = scan(&pool).await.unwrap();
        assert_eq!(groups, 1);
    }

    #[tokio::test]
    async fn second_scan_does_not_duplicate_groups() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", Some("AAAA")).await;
        insert_photo(&pool, "/b.jpg", Some("AAAA")).await;
        scan(&pool).await.unwrap();
        let second = scan(&pool).await.unwrap();
        assert_eq!(second, 0, "second scan should not create duplicate groups");
    }

    #[tokio::test]
    async fn photos_without_phash_are_ignored() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", None).await;
        insert_photo(&pool, "/b.jpg", None).await;
        assert_eq!(scan(&pool).await.unwrap(), 0);
    }
}
