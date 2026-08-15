use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use crate::error::{AppError, Result};

use super::{EnqueueResult, Job, NewJob};

const JOB_COLUMNS: &str = "id, kind, payload_version, payload_json, status, priority, \
    progress_total, progress_completed, progress_stage, cancel_requested_at, max_attempts, \
    attempt_count, next_run_at, lease_owner, lease_expires_at, error_code, error_message, \
    error_details_json, correlation_id, idempotency_key, created_at, started_at, finished_at, \
    updated_at";

pub async fn enqueue(pool: &SqlitePool, new_job: &NewJob) -> Result<EnqueueResult> {
    validate(new_job)?;
    let payload_json = serde_json::to_string(&new_job.payload)
        .map_err(|error| AppError::Metadata(error.to_string()))?;

    let result = if let Some(idempotency_key) = &new_job.idempotency_key {
        sqlx::query(
            "INSERT OR IGNORE INTO application_jobs \
                 (kind, payload_version, payload_json, priority, max_attempts, \
                  correlation_id, idempotency_key) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&new_job.kind)
        .bind(new_job.payload_version)
        .bind(&payload_json)
        .bind(new_job.priority)
        .bind(new_job.max_attempts)
        .bind(&new_job.correlation_id)
        .bind(idempotency_key)
        .execute(pool)
        .await?
    } else {
        sqlx::query(
            "INSERT INTO application_jobs \
                 (kind, payload_version, payload_json, priority, max_attempts, correlation_id) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&new_job.kind)
        .bind(new_job.payload_version)
        .bind(&payload_json)
        .bind(new_job.priority)
        .bind(new_job.max_attempts)
        .bind(&new_job.correlation_id)
        .execute(pool)
        .await?
    };

    let created = result.rows_affected() == 1;
    let id = if created {
        result.last_insert_rowid()
    } else {
        sqlx::query_scalar("SELECT id FROM application_jobs WHERE kind = ? AND idempotency_key = ?")
            .bind(&new_job.kind)
            .bind(&new_job.idempotency_key)
            .fetch_one(pool)
            .await?
    };
    Ok(EnqueueResult {
        job: get(pool, id).await?,
        created,
    })
}

pub async fn get(pool: &SqlitePool, job_id: i64) -> Result<Job> {
    let sql = format!("SELECT {JOB_COLUMNS} FROM application_jobs WHERE id = ?");
    sqlx::query_as(&sql)
        .bind(job_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("job {job_id}")))
}

pub async fn list(
    pool: &SqlitePool,
    status: Option<&str>,
    kind: Option<&str>,
    before_id: Option<i64>,
    limit: u32,
) -> Result<Vec<Job>> {
    let mut query = QueryBuilder::<Sqlite>::new(format!(
        "SELECT {JOB_COLUMNS} FROM application_jobs WHERE 1 = 1"
    ));
    if let Some(status) = status {
        query.push(" AND status = ").push_bind(status);
    }
    if let Some(kind) = kind {
        query.push(" AND kind = ").push_bind(kind);
    }
    if let Some(before_id) = before_id {
        query.push(" AND id < ").push_bind(before_id);
    }
    query
        .push(" ORDER BY id DESC LIMIT ")
        .push_bind(i64::from(limit.clamp(1, 500)));
    Ok(query.build_query_as().fetch_all(pool).await?)
}

pub async fn request_cancel(pool: &SqlitePool, job_id: i64) -> Result<Job> {
    let result = sqlx::query(
        "UPDATE application_jobs SET \
             cancel_requested_at = COALESCE(cancel_requested_at, datetime('now')), \
             status = CASE WHEN status IN ('queued', 'retry_wait') THEN 'cancelled' ELSE status END, \
             finished_at = CASE WHEN status IN ('queued', 'retry_wait') THEN datetime('now') ELSE finished_at END, \
             updated_at = datetime('now') \
         WHERE id = ? AND status IN ('queued', 'running', 'retry_wait')",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        let job = get(pool, job_id).await?;
        return Ok(job);
    }
    get(pool, job_id).await
}

fn validate(job: &NewJob) -> Result<()> {
    if job.kind.trim().is_empty() {
        return Err(AppError::Metadata("job kind cannot be empty".into()));
    }
    if job.payload_version <= 0 {
        return Err(AppError::Metadata(
            "job payload version must be positive".into(),
        ));
    }
    if job.max_attempts <= 0 {
        return Err(AppError::Metadata(
            "job max attempts must be positive".into(),
        ));
    }
    if job.idempotency_key.as_deref() == Some("") {
        return Err(AppError::Metadata("idempotency key cannot be empty".into()));
    }
    Ok(())
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
    async fn enqueue_round_trips_payload_and_list_filters() {
        let pool = pool().await;
        let mut input = NewJob::new("thumbnail", serde_json::json!({"photo_id": 42}));
        input.priority = 5;
        input.correlation_id = Some("request-42".into());
        let result = enqueue(&pool, &input).await.unwrap();

        assert!(result.created);
        assert_eq!(result.job.status, "queued");
        assert_eq!(result.job.payload().unwrap()["photo_id"], 42);
        assert_eq!(
            list(&pool, Some("queued"), Some("thumbnail"), None, 10)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            list(&pool, Some("failed"), None, None, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn idempotency_returns_the_original_job_without_replacing_payload() {
        let pool = pool().await;
        let mut first = NewJob::new("derived_media", serde_json::json!({"revision": 1}));
        first.idempotency_key = Some("photo:1:r1".into());
        let created = enqueue(&pool, &first).await.unwrap();
        let mut duplicate = NewJob::new("derived_media", serde_json::json!({"revision": 2}));
        duplicate.idempotency_key = first.idempotency_key.clone();
        let existing = enqueue(&pool, &duplicate).await.unwrap();

        assert!(created.created);
        assert!(!existing.created);
        assert_eq!(created.job.id, existing.job.id);
        assert_eq!(existing.job.payload().unwrap()["revision"], 1);
    }

    #[tokio::test]
    async fn cancellation_is_immediate_before_a_job_is_leased() {
        let pool = pool().await;
        let job = enqueue(&pool, &NewJob::new("geocode", serde_json::json!({})))
            .await
            .unwrap()
            .job;
        let cancelled = request_cancel(&pool, job.id).await.unwrap();
        assert_eq!(cancelled.status, "cancelled");
        assert!(cancelled.cancel_requested_at.is_some());
        assert!(cancelled.finished_at.is_some());
    }

    #[tokio::test]
    async fn rejects_invalid_job_contract_fields() {
        let pool = pool().await;
        let error = enqueue(&pool, &NewJob::new("", serde_json::json!({})))
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Metadata(_)));
    }
}
