use std::collections::HashSet;
use sqlx::{Row, SqlitePool};
use crate::error::Result;

/// Spawn a face re-analysis job. Returns the `face_jobs.id` immediately;
/// the actual work runs in a background `tokio::spawn`.
pub async fn run_job(pool: &SqlitePool, scope: Option<Vec<i64>>) -> Result<i64> {
    let total: Option<i64> = match &scope {
        Some(ids) => Some(ids.len() as i64),
        None => sqlx::query_scalar(
            "SELECT COUNT(*) FROM photos WHERE import_status = 'imported'",
        )
        .fetch_one(pool)
        .await
        .ok(),
    };

    let scope_json = scope
        .as_ref()
        .map(|ids| serde_json::to_string(ids).unwrap_or_default());

    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO face_jobs (status, scope, total) VALUES ('running', ?, ?) RETURNING id",
    )
    .bind(scope_json)
    .bind(total)
    .fetch_one(pool)
    .await?;

    let pool2 = pool.clone();
    tokio::spawn(async move {
        if let Err(e) = execute_job(&pool2, job_id, scope).await {
            tracing::error!("face job {job_id} failed: {e}");
            let _ = sqlx::query(
                "UPDATE face_jobs SET status = 'failed', \
                 finished_at = datetime('now') WHERE id = ?",
            )
            .bind(job_id)
            .execute(&pool2)
            .await;
        }
    });

    Ok(job_id)
}

pub(crate) async fn execute_job(
    pool: &SqlitePool,
    job_id: i64,
    scope: Option<Vec<i64>>,
) -> Result<()> {
    let scope_set: Option<HashSet<i64>> = scope.map(|ids| ids.into_iter().collect());

    let rows = sqlx::query(
        "SELECT id FROM photos WHERE import_status = 'imported'",
    )
    .fetch_all(pool)
    .await?;

    for row in rows {
        let photo_id: i64 = row.get("id");

        if let Some(ids) = &scope_set {
            if !ids.contains(&photo_id) {
                continue;
            }
        }

        reanalyze_one_photo(pool, photo_id).await;

        sqlx::query(
            "UPDATE face_jobs SET processed = processed + 1 WHERE id = ?",
        )
        .bind(job_id)
        .execute(pool)
        .await
        .ok();
    }

    sqlx::query(
        "UPDATE face_jobs SET status = 'done', finished_at = datetime('now') WHERE id = ?",
    )
    .bind(job_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Delete all faces for `photo_id` and re-run detection + embedding.
/// Clears `people.cover_face_id` references first to satisfy FK constraints.
/// Called both by `execute_job` and by the photo PATCH handler after a
/// user-applied rotation/flip changes the display-space orientation.
pub(crate) async fn reanalyze_one_photo(pool: &SqlitePool, photo_id: i64) {
    // people.cover_face_id has no ON DELETE action → clear it before deleting
    // faces to avoid a FK violation that would silently leave stale data.
    sqlx::query(
        "UPDATE people SET cover_face_id = NULL \
         WHERE cover_face_id IN (SELECT id FROM faces WHERE photo_id = ?)",
    )
    .bind(photo_id)
    .execute(pool)
    .await
    .ok();

    sqlx::query("DELETE FROM faces WHERE photo_id = ?")
        .bind(photo_id)
        .execute(pool)
        .await
        .ok();

    let path: Option<String> = sqlx::query_scalar(
        "SELECT path FROM photos WHERE id = ? AND import_status = 'imported'",
    )
    .bind(photo_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let Some(path) = path else { return };

    match image::open(&path) {
        Ok(img) => { crate::face::analyze_one(pool, photo_id, &img).await; }
        Err(_) => tracing::warn!("could not open {path} for face re-analysis after transform"),
    }
}

/// Returns the IDs of all imported photos that have no entry in the `faces` table.
pub async fn scope_for_missing(pool: &SqlitePool) -> Result<Vec<i64>> {
    let ids = sqlx::query_scalar(
        "SELECT id FROM photos WHERE import_status = 'imported' \
         AND NOT EXISTS (SELECT 1 FROM faces WHERE faces.photo_id = photos.id)",
    )
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

/// Returns the IDs of imported photos that have a user-applied rotation or flip
/// AND at least one face record.  These photos had their faces analyzed before
/// the transform was applied, so their embeddings/bboxes are in the wrong
/// orientation space and need to be recomputed.
pub async fn scope_for_rotated_with_faces(pool: &SqlitePool) -> Result<Vec<i64>> {
    let ids = sqlx::query_scalar(
        "SELECT p.id FROM photos p
         WHERE p.import_status = 'imported'
           AND (p.rotation != 0 OR p.flip_h != 0 OR p.flip_v != 0)
           AND EXISTS (SELECT 1 FROM faces f WHERE f.photo_id = p.id)",
    )
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

// ── tests ─────────────────────────────────────────────────────────────────────

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

    async fn insert_job(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO face_jobs (status) VALUES ('running') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn insert_photo(pool: &SqlitePool, id: i64, status: &str) {
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) \
             VALUES (?, ?, ?, 'jpeg', ?)",
        )
        .bind(id)
        .bind(format!("p{id}"))
        .bind(format!("sha{id}"))
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn scope_for_missing_excludes_analyzed_photos() {
        let pool = test_pool().await;
        insert_photo(&pool, 1, "imported").await;
        insert_photo(&pool, 2, "imported").await;
        insert_photo(&pool, 3, "imported").await;
        // photo 1 has a face entry
        sqlx::query(
            "INSERT INTO faces (photo_id, x, y, width, height) VALUES (1, 0, 0, 10, 10)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut missing = scope_for_missing(&pool).await.unwrap();
        missing.sort();
        assert_eq!(missing, vec![2, 3]);
    }

    #[tokio::test]
    async fn scope_for_missing_ignores_deleted_photos() {
        let pool = test_pool().await;
        insert_photo(&pool, 1, "imported").await;
        insert_photo(&pool, 2, "deleted").await;

        let missing = scope_for_missing(&pool).await.unwrap();
        assert_eq!(missing, vec![1], "deleted photos must not appear");
    }

    #[tokio::test]
    async fn scope_for_missing_empty_when_all_analyzed() {
        let pool = test_pool().await;
        insert_photo(&pool, 1, "imported").await;
        sqlx::query(
            "INSERT INTO faces (photo_id, x, y, width, height) VALUES (1, 0, 0, 10, 10)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let missing = scope_for_missing(&pool).await.unwrap();
        assert!(missing.is_empty());
    }

    #[tokio::test]
    async fn run_job_empty_db_creates_done_job() {
        let pool = test_pool().await;
        let job_id = insert_job(&pool).await;
        execute_job(&pool, job_id, None).await.unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM face_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "done");
    }

    #[tokio::test]
    async fn run_job_scoped_only_processes_given_ids() {
        let pool = test_pool().await;
        for (id, sha, path) in [(1i64, "aaa", "path1"), (2i64, "bbb", "path2")] {
            sqlx::query(
                "INSERT INTO photos (id, path, sha256, format, import_status) \
                 VALUES (?, ?, ?, 'jpeg', 'imported')",
            )
            .bind(id)
            .bind(path)
            .bind(sha)
            .execute(&pool)
            .await
            .unwrap();
        }

        let job_id = insert_job(&pool).await;
        execute_job(&pool, job_id, Some(vec![2])).await.unwrap();

        let processed: i64 =
            sqlx::query_scalar("SELECT processed FROM face_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(processed, 1, "only one photo should have been processed");
    }

    #[tokio::test]
    async fn reanalyze_one_photo_deletes_existing_faces() {
        let pool = test_pool().await;
        insert_photo(&pool, 1, "imported").await;
        sqlx::query(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence) \
             VALUES (1, 0, 0, 50, 50, 0.9)",
        )
        .execute(&pool)
        .await
        .unwrap();

        reanalyze_one_photo(&pool, 1).await;

        // In test mode the detector is disabled, so faces end up empty.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM faces WHERE photo_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn reanalyze_one_photo_clears_cover_face_id() {
        let pool = test_pool().await;
        insert_photo(&pool, 1, "imported").await;
        let face_id: i64 = sqlx::query_scalar(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence) \
             VALUES (1, 0, 0, 50, 50, 0.9) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let person_id: i64 =
            sqlx::query_scalar("INSERT INTO people (name, cover_face_id) VALUES ('A', ?) RETURNING id")
                .bind(face_id)
                .fetch_one(&pool)
                .await
                .unwrap();

        reanalyze_one_photo(&pool, 1).await;

        let cover: Option<i64> =
            sqlx::query_scalar("SELECT cover_face_id FROM people WHERE id = ?")
                .bind(person_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(cover.is_none(), "cover_face_id must be cleared before face deletion");
    }

    #[tokio::test]
    async fn scope_for_rotated_with_faces_returns_only_affected() {
        let pool = test_pool().await;
        // photo 1: rotated, has face → should appear
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status, rotation) \
             VALUES (1, 'p1', 'h1', 'jpeg', 'imported', 90)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO faces (photo_id, x, y, width, height) VALUES (1, 0, 0, 10, 10)")
            .execute(&pool)
            .await
            .unwrap();

        // photo 2: rotated, no faces → should NOT appear
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status, rotation) \
             VALUES (2, 'p2', 'h2', 'jpeg', 'imported', 180)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // photo 3: no rotation, has face → should NOT appear
        insert_photo(&pool, 3, "imported").await;
        sqlx::query("INSERT INTO faces (photo_id, x, y, width, height) VALUES (3, 0, 0, 10, 10)")
            .execute(&pool)
            .await
            .unwrap();

        let ids = scope_for_rotated_with_faces(&pool).await.unwrap();
        assert_eq!(ids, vec![1]);
    }

    #[tokio::test]
    async fn reanalysis_does_not_accumulate_faces() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) \
             VALUES (1, 'path1', 'abc', 'jpeg', 'imported')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Pre-seed a stale face row
        sqlx::query(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence) \
             VALUES (1, 10, 10, 50, 50, 0.9)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let job_id = insert_job(&pool).await;
        execute_job(&pool, job_id, Some(vec![1])).await.unwrap();

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM faces WHERE photo_id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "stale faces should be deleted before re-analysis");
    }
}
