use std::collections::HashSet;

use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use crate::error::{AppError, Result};

use super::{EnqueueResult, Job, JobFailure, JobLease, JobMetrics, NewJob};

pub async fn metrics(pool: &SqlitePool) -> Result<JobMetrics> {
    Ok(JobMetrics {
        queued: job_count(pool, "queued").await?,
        running: job_count(pool, "running").await?,
        retry_wait: job_count(pool, "retry_wait").await?,
        succeeded: job_count(pool, "succeeded").await?,
        failed: job_count(pool, "failed").await?,
        cancelled: job_count(pool, "cancelled").await?,
        attempts_running: attempt_count(pool, "running").await?,
        attempts_failed: attempt_count(pool, "failed").await?,
        attempts_interrupted: attempt_count(pool, "interrupted").await?,
        warning_events: event_count(pool, "warning").await?,
        error_events: event_count(pool, "error").await?,
    })
}

async fn job_count(pool: &SqlitePool, status: &str) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM application_jobs WHERE status = ?")
        .bind(status).fetch_one(pool).await?)
}

async fn attempt_count(pool: &SqlitePool, status: &str) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM application_job_attempts WHERE status = ?")
        .bind(status).fetch_one(pool).await?)
}

async fn event_count(pool: &SqlitePool, level: &str) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM application_job_events WHERE level = ?")
        .bind(level).fetch_one(pool).await?)
}

const JOB_COLUMNS: &str = "id, kind, payload_version, payload_json, status, priority, \
    progress_total, progress_completed, progress_stage, cancel_requested_at, max_attempts, \
    attempt_count, next_run_at, lease_owner, lease_expires_at, error_code, error_message, \
    error_details_json, result_json, correlation_id, idempotency_key, created_at, started_at, finished_at, \
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
             status = CASE WHEN status IN ('queued', 'retry_wait', 'failed') THEN 'cancelled' ELSE status END, \
             finished_at = CASE WHEN status IN ('queued', 'retry_wait', 'failed') THEN datetime('now') ELSE finished_at END, \
             updated_at = datetime('now') \
         WHERE id = ? AND status IN ('queued', 'running', 'retry_wait', 'failed')",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        let job = get(pool, job_id).await?;
        return Err(AppError::Metadata(format!(
            "job {job_id} cannot be cancelled from {}",
            job.status
        )));
    }
    sqlx::query(
        "INSERT INTO application_job_events (job_id, level, code, message) \
         VALUES (?, 'info', 'cancellation_requested', 'Job cancellation was requested')",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    get(pool, job_id).await
}

pub async fn retry(pool: &SqlitePool, job_id: i64) -> Result<Job> {
    let result = sqlx::query(
        "UPDATE application_jobs SET status = 'queued', \
             max_attempts = attempt_count + 1, next_run_at = NULL, \
             cancel_requested_at = NULL, error_code = NULL, error_message = NULL, \
             error_details_json = NULL, result_json = NULL, finished_at = NULL, \
             updated_at = datetime('now') \
         WHERE id = ? AND status = 'failed'",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        let job = get(pool, job_id).await?;
        return Err(AppError::Metadata(format!(
            "job {job_id} cannot be retried from {}",
            job.status
        )));
    }
    sqlx::query(
        "INSERT INTO application_job_events (job_id, level, code, message) \
         VALUES (?, 'info', 'manual_retry', 'Job was manually requeued')",
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    get(pool, job_id).await
}

pub async fn lease_next(
    pool: &SqlitePool,
    worker_id: &str,
    lease_seconds: u64,
) -> Result<Option<JobLease>> {
    if worker_id.is_empty() || lease_seconds == 0 {
        return Err(AppError::Metadata(
            "worker ID and positive lease duration are required".into(),
        ));
    }
    let mut tx = pool.begin().await?;
    let candidate: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM application_jobs \
         WHERE status IN ('queued', 'retry_wait') \
           AND cancel_requested_at IS NULL \
           AND (next_run_at IS NULL OR next_run_at <= datetime('now')) \
         ORDER BY priority DESC, id ASC LIMIT 1",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(job_id) = candidate else {
        tx.commit().await?;
        return Ok(None);
    };
    let lease_modifier = format!("+{lease_seconds} seconds");
    let result = sqlx::query(
        "UPDATE application_jobs SET status = 'running', lease_owner = ?, \
             lease_expires_at = datetime('now', ?), attempt_count = attempt_count + 1, \
             started_at = COALESCE(started_at, datetime('now')), next_run_at = NULL, \
             error_code = NULL, error_message = NULL, error_details_json = NULL, \
             updated_at = datetime('now') \
         WHERE id = ? AND status IN ('queued', 'retry_wait') AND cancel_requested_at IS NULL",
    )
    .bind(worker_id)
    .bind(&lease_modifier)
    .bind(job_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(None);
    }
    let attempt_number: i64 =
        sqlx::query_scalar("SELECT attempt_count FROM application_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_one(&mut *tx)
            .await?;
    let attempt_id: i64 = sqlx::query_scalar(
        "INSERT INTO application_job_attempts (job_id, attempt_number, worker_id) \
         VALUES (?, ?, ?) RETURNING id",
    )
    .bind(job_id)
    .bind(attempt_number)
    .bind(worker_id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO application_job_events \
         (job_id, attempt_id, level, code, message) \
         VALUES (?, ?, 'info', 'attempt_started', 'Worker leased the job')",
    )
    .bind(job_id)
    .bind(attempt_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(JobLease {
        job: get(pool, job_id).await?,
        attempt_id,
        worker_id: worker_id.to_owned(),
    }))
}

pub async fn renew_lease(pool: &SqlitePool, lease: &JobLease, lease_seconds: u64) -> Result<()> {
    let modifier = format!("+{lease_seconds} seconds");
    let result = sqlx::query(
        "UPDATE application_jobs SET lease_expires_at = datetime('now', ?), \
             updated_at = datetime('now') \
         WHERE id = ? AND status = 'running' AND lease_owner = ?",
    )
    .bind(modifier)
    .bind(lease.job.id)
    .bind(&lease.worker_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::Metadata(format!(
            "job {} lease is no longer owned by {}",
            lease.job.id, lease.worker_id
        )));
    }
    Ok(())
}

pub async fn update_progress(
    pool: &SqlitePool,
    lease: &JobLease,
    completed: i64,
    total: Option<i64>,
    stage: Option<&str>,
) -> Result<()> {
    if completed < 0 || total.is_some_and(|total| total < completed) {
        return Err(AppError::Metadata("invalid job progress".into()));
    }
    let result = sqlx::query(
        "UPDATE application_jobs SET progress_completed = ?, progress_total = ?, \
             progress_stage = ?, updated_at = datetime('now') \
         WHERE id = ? AND status = 'running' AND lease_owner = ?",
    )
    .bind(completed)
    .bind(total)
    .bind(stage)
    .bind(lease.job.id)
    .bind(&lease.worker_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::Metadata("job lease was lost".into()));
    }
    Ok(())
}

pub async fn cancellation_requested(pool: &SqlitePool, lease: &JobLease) -> Result<bool> {
    let requested: bool = sqlx::query_scalar(
        "SELECT cancel_requested_at IS NOT NULL FROM application_jobs \
         WHERE id = ? AND status = 'running' AND lease_owner = ?",
    )
    .bind(lease.job.id)
    .bind(&lease.worker_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Metadata("job lease was lost".into()))?;
    Ok(requested)
}

pub async fn set_result(
    pool: &SqlitePool,
    lease: &JobLease,
    result: &serde_json::Value,
) -> Result<()> {
    let result_json = serde_json::to_string(result)
        .map_err(|error| AppError::Metadata(error.to_string()))?;
    let updated = sqlx::query(
        "UPDATE application_jobs SET result_json = ?, updated_at = datetime('now') \
         WHERE id = ? AND status = 'running' AND lease_owner = ?",
    )
    .bind(result_json)
    .bind(lease.job.id)
    .bind(&lease.worker_id)
    .execute(pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Metadata("job lease was lost".into()));
    }
    Ok(())
}

pub async fn complete(pool: &SqlitePool, lease: &JobLease) -> Result<Job> {
    finish(pool, lease, "succeeded", None, 0).await
}

pub async fn cancel_leased(pool: &SqlitePool, lease: &JobLease) -> Result<Job> {
    finish(pool, lease, "cancelled", None, 0).await
}

pub async fn fail(
    pool: &SqlitePool,
    lease: &JobLease,
    failure: &JobFailure,
    retry_delay_seconds: u64,
) -> Result<Job> {
    finish(pool, lease, "failed", Some(failure), retry_delay_seconds).await
}

async fn finish(
    pool: &SqlitePool,
    lease: &JobLease,
    terminal_status: &str,
    failure: Option<&JobFailure>,
    retry_delay_seconds: u64,
) -> Result<Job> {
    let mut tx = pool.begin().await?;
    let current: (i64, i64, Option<String>) = sqlx::query_as(
        "SELECT attempt_count, max_attempts, cancel_requested_at FROM application_jobs \
         WHERE id = ? AND status = 'running' AND lease_owner = ?",
    )
    .bind(lease.job.id)
    .bind(&lease.worker_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Metadata("job lease was lost".into()))?;

    let cancelled = terminal_status == "cancelled" || current.2.is_some();
    let should_retry =
        failure.is_some_and(|failure| failure.retryable) && current.0 < current.1 && !cancelled;
    let job_status = if cancelled {
        "cancelled"
    } else if should_retry {
        "retry_wait"
    } else {
        terminal_status
    };
    let attempt_status = if cancelled {
        "cancelled"
    } else if failure.is_some() {
        "failed"
    } else {
        "succeeded"
    };
    let details_json = failure
        .and_then(|failure| failure.details.as_ref())
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| AppError::Metadata(error.to_string()))?;
    let retry_modifier = format!("+{retry_delay_seconds} seconds");

    sqlx::query(
        "UPDATE application_job_attempts SET status = ?, error_code = ?, \
             error_message = ?, error_details_json = ?, finished_at = datetime('now') \
         WHERE id = ? AND status = 'running'",
    )
    .bind(attempt_status)
    .bind(failure.map(|failure| failure.code.as_str()))
    .bind(failure.map(|failure| failure.message.as_str()))
    .bind(&details_json)
    .bind(lease.attempt_id)
    .execute(&mut *tx)
    .await?;
    let event_level = if cancelled {
        "warning"
    } else if failure.is_some() {
        "error"
    } else {
        "info"
    };
    let event_code = if should_retry { "retry_scheduled" } else { job_status };
    sqlx::query(
        "INSERT INTO application_job_events \
         (job_id, attempt_id, level, code, message, details_json) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(lease.job.id)
    .bind(lease.attempt_id)
    .bind(event_level)
    .bind(event_code)
    .bind(failure.map_or(job_status, |failure| failure.message.as_str()))
    .bind(&details_json)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE application_jobs SET status = ?, lease_owner = NULL, lease_expires_at = NULL, \
             next_run_at = CASE WHEN ? THEN datetime('now', ?) ELSE NULL END, \
             error_code = ?, error_message = ?, error_details_json = ?, \
             finished_at = CASE WHEN ? THEN NULL ELSE datetime('now') END, \
             updated_at = datetime('now') WHERE id = ?",
    )
    .bind(job_status)
    .bind(should_retry)
    .bind(retry_modifier)
    .bind(failure.map(|failure| failure.code.as_str()))
    .bind(failure.map(|failure| failure.message.as_str()))
    .bind(details_json)
    .bind(should_retry)
    .bind(lease.job.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get(pool, lease.job.id).await
}

pub async fn recover_expired(pool: &SqlitePool) -> Result<u64> {
    recover_expired_excluding(pool, &HashSet::new()).await
}

pub(crate) async fn recover_expired_excluding(
    pool: &SqlitePool,
    active_owners: &HashSet<String>,
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let expired: Vec<(i64, i64, i64, Option<String>, String)> = sqlx::query_as(
        "SELECT id, attempt_count, max_attempts, cancel_requested_at, lease_owner \
         FROM application_jobs WHERE status = 'running' \
           AND lease_expires_at <= datetime('now')",
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .filter(|(_, _, _, _, owner)| !active_owners.contains(owner))
    .collect();
    for (job_id, attempt_count, max_attempts, cancel_requested_at, _) in &expired {
        sqlx::query(
            "UPDATE application_job_attempts SET status = 'interrupted', \
                 error_code = 'lease_expired', error_message = 'Worker lease expired', \
                 finished_at = datetime('now') \
             WHERE job_id = ? AND status = 'running'",
        )
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO application_job_events \
             (job_id, level, code, message) VALUES (?, 'warning', 'lease_expired', \
             'An expired worker lease was recovered')",
        )
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        let status = if cancel_requested_at.is_some() {
            "cancelled"
        } else if attempt_count < max_attempts {
            "queued"
        } else {
            "failed"
        };
        sqlx::query(
            "UPDATE application_jobs SET status = ?, lease_owner = NULL, \
                 lease_expires_at = NULL, error_code = 'lease_expired', \
                 error_message = 'Worker lease expired', \
                 finished_at = CASE WHEN ? IN ('failed', 'cancelled') \
                                    THEN datetime('now') ELSE NULL END, \
                 updated_at = datetime('now') WHERE id = ?",
        )
        .bind(status)
        .bind(status)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(expired.len() as u64)
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

    #[tokio::test]
    async fn lease_progress_and_completion_record_one_attempt() {
        let pool = pool().await;
        enqueue(&pool, &NewJob::new("thumbnail", serde_json::json!({})))
            .await
            .unwrap();
        let lease = lease_next(&pool, "worker-a", 60).await.unwrap().unwrap();
        assert_eq!(lease.job.status, "running");
        assert_eq!(lease.job.attempt_count, 1);
        update_progress(&pool, &lease, 2, Some(5), Some("decoding"))
            .await
            .unwrap();
        let completed = complete(&pool, &lease).await.unwrap();
        assert_eq!(completed.status, "succeeded");
        assert_eq!(completed.progress_completed, 2);
        let attempt: (String, Option<String>) =
            sqlx::query_as("SELECT status, finished_at FROM application_job_attempts WHERE id = ?")
                .bind(lease.attempt_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(attempt.0, "succeeded");
        assert!(attempt.1.is_some());
    }

    #[tokio::test]
    async fn retryable_failure_requeues_until_attempt_budget_is_exhausted() {
        let pool = pool().await;
        let mut input = NewJob::new("geocode", serde_json::json!({}));
        input.max_attempts = 2;
        enqueue(&pool, &input).await.unwrap();
        let first = lease_next(&pool, "worker-a", 60).await.unwrap().unwrap();
        let waiting = fail(
            &pool,
            &first,
            &JobFailure::retryable("offline", "network unavailable"),
            0,
        )
        .await
        .unwrap();
        assert_eq!(waiting.status, "retry_wait");

        let second = lease_next(&pool, "worker-b", 60).await.unwrap().unwrap();
        let failed = fail(
            &pool,
            &second,
            &JobFailure::retryable("offline", "network unavailable"),
            0,
        )
        .await
        .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.attempt_count, 2);
    }

    #[tokio::test]
    async fn manual_retry_preserves_attempt_history_and_extends_budget() {
        let pool = pool().await;
        let mut input = NewJob::new("geocode", serde_json::json!({}));
        input.max_attempts = 1;
        let job = enqueue(&pool, &input).await.unwrap().job;
        let lease = lease_next(&pool, "worker-a", 60).await.unwrap().unwrap();
        fail(&pool, &lease, &JobFailure::terminal("offline", "offline"), 0)
            .await
            .unwrap();

        let queued = retry(&pool, job.id).await.unwrap();
        assert_eq!(queued.status, "queued");
        assert_eq!(queued.attempt_count, 1);
        assert_eq!(queued.max_attempts, 2);
        let second = lease_next(&pool, "worker-b", 60).await.unwrap().unwrap();
        assert_eq!(second.job.attempt_count, 2);
        let attempts: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM application_job_attempts WHERE job_id = ?",
        )
        .bind(job.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(attempts, 2);
    }

    #[tokio::test]
    async fn expired_lease_is_interrupted_and_recoverable() {
        let pool = pool().await;
        let job = enqueue(&pool, &NewJob::new("face_analysis", serde_json::json!({})))
            .await
            .unwrap()
            .job;
        let lease = lease_next(&pool, "dead-worker", 60).await.unwrap().unwrap();
        sqlx::query(
            "UPDATE application_jobs SET lease_expires_at = datetime('now', '-1 second') \
             WHERE id = ?",
        )
        .bind(job.id)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(recover_expired(&pool).await.unwrap(), 1);
        let recovered = get(&pool, job.id).await.unwrap();
        assert_eq!(recovered.status, "queued");
        let attempt_status: String =
            sqlx::query_scalar("SELECT status FROM application_job_attempts WHERE id = ?")
                .bind(lease.attempt_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(attempt_status, "interrupted");
    }

    #[tokio::test]
    async fn expired_lease_recovery_skips_an_active_local_owner() {
        let pool = pool().await;
        let job = enqueue(&pool, &NewJob::new("geocode", serde_json::json!({})))
            .await
            .unwrap()
            .job;
        lease_next(&pool, "active-worker", 60)
            .await
            .unwrap()
            .unwrap();
        sqlx::query(
            "UPDATE application_jobs SET lease_expires_at = datetime('now', '-1 second') \
             WHERE id = ?",
        )
        .bind(job.id)
        .execute(&pool)
        .await
        .unwrap();

        let active = HashSet::from(["active-worker".to_owned()]);
        assert_eq!(recover_expired_excluding(&pool, &active).await.unwrap(), 0);
        assert_eq!(get(&pool, job.id).await.unwrap().status, "running");

        assert_eq!(recover_expired(&pool).await.unwrap(), 1);
        assert_eq!(get(&pool, job.id).await.unwrap().status, "queued");
    }
}
