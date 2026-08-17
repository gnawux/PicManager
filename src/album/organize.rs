use sqlx::{Sqlite, SqlitePool, Transaction};
use crate::error::Result;

/// Attach one imported photo to its derived month album inside the caller's transaction.
/// This keeps provider-specific incremental commits consistent without rescanning the library.
pub(crate) async fn assign_month_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    photo_id: i64,
) -> Result<()> {
    let month: Option<String> = sqlx::query_scalar(
        "SELECT substr(taken_at, 1, 7) FROM photos \
         WHERE id = ? AND import_status = 'imported' AND taken_at IS NOT NULL \
           AND length(taken_at) >= 7",
    )
    .bind(photo_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(month) = month else { return Ok(()); };

    let album_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM albums WHERE name = ? AND kind = 'time' ORDER BY id LIMIT 1",
    )
    .bind(&month)
    .fetch_optional(&mut **tx)
    .await?;
    let album_id = match album_id {
        Some(id) => id,
        None => sqlx::query("INSERT INTO albums (name, kind) VALUES (?, 'time')")
            .bind(&month)
            .execute(&mut **tx)
            .await?
            .last_insert_rowid(),
    };
    sqlx::query("INSERT OR IGNORE INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
        .bind(photo_id)
        .bind(album_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Group all imported photos into monthly time albums (e.g. "2024-06").
pub async fn group_by_month(pool: &SqlitePool) -> Result<()> {
    let months: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT substr(taken_at, 1, 7) FROM photos
         WHERE import_status = 'imported' AND taken_at IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    for (month,) in months {
        ensure_album(pool, &month, "time").await?;
        sqlx::query(
            "INSERT OR IGNORE INTO photo_albums (photo_id, album_id)
             SELECT p.id, a.id FROM photos p, albums a
             WHERE a.name = ? AND a.kind = 'time'
               AND p.import_status = 'imported'
               AND substr(p.taken_at, 1, 7) = ?",
        )
        .bind(&month)
        .bind(&month)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Group all imported photos into per-camera albums.
pub async fn group_by_camera(pool: &SqlitePool) -> Result<()> {
    let cameras: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT camera FROM photos
         WHERE import_status = 'imported' AND camera IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    for (camera,) in cameras {
        ensure_album(pool, &camera, "camera").await?;
        sqlx::query(
            "INSERT OR IGNORE INTO photo_albums (photo_id, album_id)
             SELECT p.id, a.id FROM photos p, albums a
             WHERE a.name = ? AND a.kind = 'camera'
               AND p.import_status = 'imported'
               AND p.camera = ?",
        )
        .bind(&camera)
        .bind(&camera)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn ensure_album(pool: &SqlitePool, name: &str, kind: &str) -> Result<i64> {
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM albums WHERE name = ? AND kind = ?")
            .bind(name)
            .bind(kind)
            .fetch_optional(pool)
            .await?;

    if let Some((id,)) = existing {
        return Ok(id);
    }
    let id = sqlx::query("INSERT INTO albums (name, kind) VALUES (?, ?)")
        .bind(name)
        .bind(kind)
        .execute(pool)
        .await?
        .last_insert_rowid();
    Ok(id)
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

    async fn insert_photo(pool: &SqlitePool, path: &str, taken_at: Option<&str>, camera: Option<&str>) {
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, taken_at, camera, import_status)
             VALUES (?, ?, 'jpeg', ?, ?, 'imported')",
        )
        .bind(path)
        .bind(path)
        .bind(taken_at)
        .bind(camera)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn group_by_month_creates_albums() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", Some("2024-06-15 10:00:00"), None).await;
        insert_photo(&pool, "/b.jpg", Some("2024-06-20 12:00:00"), None).await;
        insert_photo(&pool, "/c.jpg", Some("2024-07-01 08:00:00"), None).await;

        group_by_month(&pool).await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'time'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count.0, 2, "should create 2 monthly albums");

        let june: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM photo_albums pa JOIN albums a ON a.id = pa.album_id WHERE a.name = '2024-06'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(june.0, 2, "June album should have 2 photos");
    }

    #[tokio::test]
    async fn group_by_month_is_idempotent() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", Some("2024-06-15 10:00:00"), None).await;

        group_by_month(&pool).await.unwrap();
        group_by_month(&pool).await.unwrap();

        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM photo_albums").fetch_one(&pool).await.unwrap();
        assert_eq!(count.0, 1, "idempotent: no duplicate associations");
    }

    #[tokio::test]
    async fn group_by_camera_creates_albums() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", None, Some("Apple iPhone 15 Pro")).await;
        insert_photo(&pool, "/b.jpg", None, Some("Apple iPhone 15 Pro")).await;
        insert_photo(&pool, "/c.jpg", None, Some("Sony A7IV")).await;
        insert_photo(&pool, "/d.jpg", None, None).await; // no camera, should be skipped

        group_by_camera(&pool).await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'camera'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count.0, 2);

        let iphone: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM photo_albums pa JOIN albums a ON a.id = pa.album_id WHERE a.name = 'Apple iPhone 15 Pro'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(iphone.0, 2);
    }

    #[tokio::test]
    async fn photos_without_taken_at_skipped_in_month_grouping() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", None, None).await;
        group_by_month(&pool).await.unwrap();
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM albums").fetch_one(&pool).await.unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn month_membership_repair_is_additive_and_idempotent() {
        let pool = test_pool().await;
        insert_photo(&pool, "/repair.jpg", Some("2026-08-17T10:30:00"), None).await;
        insert_photo(&pool, "/undated.jpg", None, None).await;

        let repair = include_str!("../../migrations/0030_backfill_month_album_membership.sql");
        sqlx::raw_sql(repair).execute(&pool).await.unwrap();
        sqlx::raw_sql(repair).execute(&pool).await.unwrap();

        let memberships: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM photo_albums pa JOIN albums a ON a.id = pa.album_id \
             WHERE a.kind = 'time' AND a.name = '2026-08'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(memberships, 1);
        let undated: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM photo_albums pa JOIN photos p ON p.id = pa.photo_id \
             WHERE p.path = '/undated.jpg'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(undated, 0);
    }
}
