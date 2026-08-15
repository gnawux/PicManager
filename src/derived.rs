use sqlx::{Sqlite, SqlitePool, Transaction};
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Increment the photo render revision and atomically remove face data whose coordinates
/// belong to the previous display pixels.
pub async fn invalidate_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    photo_id: i64,
) -> Result<i64> {
    sqlx::query(
        "UPDATE people SET cover_face_id = NULL \
         WHERE cover_face_id IN (SELECT id FROM faces WHERE photo_id = ?)",
    )
    .bind(photo_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM faces WHERE photo_id = ?")
        .bind(photo_id)
        .execute(&mut **tx)
        .await?;
    let revision: i64 = sqlx::query_scalar(
        "UPDATE photos SET render_revision = render_revision + 1 WHERE id = ? \
         RETURNING render_revision",
    )
    .bind(photo_id)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO derived_media_state \
             (photo_id, render_revision, thumbnail_status, face_status) \
         VALUES (?, ?, 'pending', 'pending') \
         ON CONFLICT(photo_id) DO UPDATE SET \
             render_revision = excluded.render_revision, \
             thumbnail_status = 'pending', face_status = 'pending', last_error = NULL, \
             invalidated_at = datetime('now'), thumbnail_at = NULL, face_at = NULL, \
             updated_at = datetime('now')",
    )
    .bind(photo_id)
    .bind(revision)
    .execute(&mut **tx)
    .await?;
    Ok(revision)
}

pub async fn invalidate(pool: &SqlitePool, photo_id: i64) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let revision = invalidate_in_transaction(&mut tx, photo_id).await?;
    tx.commit().await?;
    Ok(revision)
}

pub fn thumbnail_cache_path(cache_dir: &Path, photo_id: i64, revision: i64) -> PathBuf {
    if revision == 0 {
        cache_dir.join(format!("{photo_id}.jpg"))
    } else {
        cache_dir.join(format!("{photo_id}_r{revision}.jpg"))
    }
}

pub async fn mark_thumbnail_ready(pool: &SqlitePool, photo_id: i64, revision: i64) {
    let _ = sqlx::query(
        "INSERT INTO derived_media_state \
             (photo_id, render_revision, thumbnail_status, face_status, thumbnail_at) \
         SELECT id, render_revision, 'ready', 'pending', datetime('now') FROM photos \
         WHERE id = ? AND render_revision = ? \
         ON CONFLICT(photo_id) DO UPDATE SET \
             thumbnail_status = 'ready', thumbnail_at = datetime('now'), \
             updated_at = datetime('now') \
         WHERE derived_media_state.render_revision = excluded.render_revision",
    )
    .bind(photo_id)
    .bind(revision)
    .execute(pool)
    .await;
}

pub async fn mark_face_status(
    pool: &SqlitePool,
    photo_id: i64,
    revision: i64,
    status: &str,
    error: Option<&str>,
) {
    let _ = sqlx::query(
        "INSERT INTO derived_media_state \
             (photo_id, render_revision, thumbnail_status, face_status, last_error, face_at) \
         SELECT id, render_revision, 'pending', ?, ?, \
                CASE WHEN ? = 'ready' THEN datetime('now') END \
         FROM photos WHERE id = ? AND render_revision = ? \
         ON CONFLICT(photo_id) DO UPDATE SET \
             face_status = excluded.face_status, last_error = excluded.last_error, \
             face_at = CASE WHEN excluded.face_status = 'ready' THEN datetime('now') \
                            ELSE derived_media_state.face_at END, \
             updated_at = datetime('now') \
         WHERE derived_media_state.render_revision = excluded.render_revision",
    )
    .bind(status)
    .bind(error)
    .bind(status)
    .bind(photo_id)
    .bind(revision)
    .execute(pool)
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn invalidation_advances_revision_and_removes_stale_faces() {
        let pool = pool().await;
        sqlx::query("INSERT INTO photos (id, path, sha256, format) VALUES (1, 'x', 'h', 'jpeg')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO faces (photo_id, x, y, width, height) VALUES (1, 0, 0, 1, 1)")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(invalidate(&pool, 1).await.unwrap(), 1);
        let state: (i64, String, String) = sqlx::query_as(
            "SELECT render_revision, thumbnail_status, face_status \
             FROM derived_media_state WHERE photo_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let faces: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM faces WHERE photo_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(state, (1, "pending".into(), "pending".into()));
        assert_eq!(faces, 0);
    }

    #[test]
    fn revision_zero_preserves_legacy_cache_name() {
        assert_eq!(
            thumbnail_cache_path(Path::new("cache"), 7, 0),
            Path::new("cache/7.jpg")
        );
        assert_eq!(
            thumbnail_cache_path(Path::new("cache"), 7, 3),
            Path::new("cache/7_r3.jpg")
        );
    }
}
