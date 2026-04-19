use sqlx::SqlitePool;
use crate::error::{AppError, Result};

/// Move all photos from `source_id` into `target_id`, then delete `source_id`.
pub async fn merge(pool: &SqlitePool, source_id: i64, target_id: i64) -> Result<()> {
    if source_id == target_id {
        return Err(AppError::NotFound("source and target must differ".to_string()));
    }

    for id in [source_id, target_id] {
        let exists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await?;
        if exists.0 == 0 {
            return Err(AppError::NotFound(format!("album {id}")));
        }
    }

    // INSERT OR IGNORE to avoid duplicate associations
    sqlx::query(
        "INSERT OR IGNORE INTO photo_albums (photo_id, album_id)
         SELECT photo_id, ? FROM photo_albums WHERE album_id = ?",
    )
    .bind(target_id)
    .bind(source_id)
    .execute(pool)
    .await?;

    sqlx::query("DELETE FROM photo_albums WHERE album_id = ?")
        .bind(source_id)
        .execute(pool)
        .await?;

    sqlx::query("DELETE FROM albums WHERE id = ?")
        .bind(source_id)
        .execute(pool)
        .await?;

    Ok(())
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

    async fn make_album(pool: &SqlitePool, name: &str) -> i64 {
        sqlx::query("INSERT INTO albums (name, kind) VALUES (?, 'manual')")
            .bind(name)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_rowid()
    }

    async fn make_photo(pool: &SqlitePool, path: &str) -> i64 {
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status) VALUES (?, ?, 'jpeg', 'imported')",
        )
        .bind(path)
        .bind(path)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
    }

    async fn add_to_album(pool: &SqlitePool, photo_id: i64, album_id: i64) {
        sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
            .bind(photo_id)
            .bind(album_id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn merge_moves_photos_to_target() {
        let pool = test_pool().await;
        let a = make_album(&pool, "A").await;
        let b = make_album(&pool, "B").await;
        let p1 = make_photo(&pool, "/p1.jpg").await;
        let p2 = make_photo(&pool, "/p2.jpg").await;
        add_to_album(&pool, p1, a).await;
        add_to_album(&pool, p2, b).await;

        merge(&pool, a, b).await.unwrap();

        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
                .bind(b)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 2, "target should have both photos");
    }

    #[tokio::test]
    async fn merge_removes_source_album() {
        let pool = test_pool().await;
        let a = make_album(&pool, "A").await;
        let b = make_album(&pool, "B").await;

        merge(&pool, a, b).await.unwrap();

        let exists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE id = ?")
            .bind(a)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(exists.0, 0, "source album should be deleted");
    }

    #[tokio::test]
    async fn merge_is_idempotent_for_shared_photos() {
        let pool = test_pool().await;
        let a = make_album(&pool, "A").await;
        let b = make_album(&pool, "B").await;
        let p = make_photo(&pool, "/p.jpg").await;
        add_to_album(&pool, p, a).await;
        add_to_album(&pool, p, b).await; // already in both

        merge(&pool, a, b).await.unwrap();

        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
                .bind(b)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 1, "no duplicate photo_album rows");
    }

    #[tokio::test]
    async fn merge_nonexistent_source_returns_error() {
        let pool = test_pool().await;
        let b = make_album(&pool, "B").await;
        assert!(merge(&pool, 9999, b).await.is_err());
    }

    #[tokio::test]
    async fn merge_same_id_returns_error() {
        let pool = test_pool().await;
        let a = make_album(&pool, "A").await;
        assert!(merge(&pool, a, a).await.is_err());
    }
}
