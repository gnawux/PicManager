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
    let rows = sqlx::query(
        "SELECT id, path FROM photos WHERE import_status = 'imported'",
    )
    .fetch_all(pool)
    .await?;

    for row in rows {
        let photo_id: i64 = row.get("id");
        let path: String = row.get("path");

        if let Some(ids) = &scope {
            if !ids.contains(&photo_id) {
                continue;
            }
        }

        sqlx::query("DELETE FROM faces WHERE photo_id = ?")
            .bind(photo_id)
            .execute(pool)
            .await
            .ok();

        match image::open(&path) {
            Ok(img) => crate::face::analyze_one(pool, photo_id, &img).await,
            Err(_) => tracing::warn!("could not open {path} for face re-analysis"),
        }

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
