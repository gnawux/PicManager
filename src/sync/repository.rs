use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct DiscoveryItem<'a> {
    pub source_id: Option<i64>,
    pub external_id: &'a str,
    pub operation: &'a str,
    pub payload_json: Option<&'a str>,
    pub max_attempts: u32,
}

#[derive(Debug, Clone)]
pub struct DiscoveryBatch<'a> {
    pub kind: &'a str,
    pub provider: &'a str,
    pub scope_key: &'a str,
    pub checkpoint_before: Option<&'a [u8]>,
    pub checkpoint_after: Option<&'a [u8]>,
    pub items: Vec<DiscoveryItem<'a>>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SyncJob {
    pub id: i64,
    pub kind: String,
    pub provider: Option<String>,
    pub status: String,
    pub checkpoint_before: Option<Vec<u8>>,
    pub checkpoint_after: Option<Vec<u8>>,
    pub total_items: i64,
    pub completed_items: i64,
    pub failed_items: i64,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SyncItem {
    pub id: i64,
    pub job_id: i64,
    pub source_id: Option<i64>,
    pub external_id: String,
    pub operation: String,
    pub status: String,
    pub attempt_count: i64,
    pub max_attempts: i64,
    pub available_at: String,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<String>,
    pub payload_json: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProviderCheckpoint {
    pub provider: String,
    pub scope_key: String,
    pub token: Option<Vec<u8>>,
    pub generation: i64,
    pub last_full_reconcile: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncJobDetail {
    #[serde(flatten)]
    pub job: SyncJob,
    pub items: Vec<SyncItem>,
}

pub async fn list_jobs(
    pool: &SqlitePool,
    provider: Option<&str>,
    status: Option<&str>,
    before_id: Option<i64>,
    limit: u32,
) -> Result<Vec<SyncJob>> {
    let jobs = sqlx::query_as(
        "SELECT * FROM sync_jobs \
         WHERE (? IS NULL OR provider = ?) \
           AND (? IS NULL OR status = ?) \
           AND (? IS NULL OR id < ?) \
         ORDER BY id DESC LIMIT ?",
    )
    .bind(provider).bind(provider)
    .bind(status).bind(status)
    .bind(before_id).bind(before_id)
    .bind(i64::from(limit.clamp(1, 100)))
    .fetch_all(pool).await?;
    Ok(jobs)
}

pub async fn get_job(pool: &SqlitePool, job_id: i64) -> Result<SyncJobDetail> {
    let job = sqlx::query_as("SELECT * FROM sync_jobs WHERE id = ?")
        .bind(job_id).fetch_optional(pool).await?
        .ok_or_else(|| crate::error::AppError::NotFound(format!("sync job {job_id}")))?;
    let items = sqlx::query_as("SELECT * FROM sync_items WHERE job_id = ? ORDER BY id")
        .bind(job_id).fetch_all(pool).await?;
    Ok(SyncJobDetail { job, items })
}

/// Requeue only terminal failures while preserving already completed work.
pub async fn retry_job(pool: &SqlitePool, job_id: i64) -> Result<SyncJobDetail> {
    let mut tx = pool.begin().await?;
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM sync_jobs WHERE id = ?")
        .bind(job_id).fetch_optional(&mut *tx).await?;
    match status.as_deref() {
        None => return Err(crate::error::AppError::NotFound(format!("sync job {job_id}"))),
        Some("failed") => {}
        Some(value) => return Err(crate::error::AppError::Metadata(format!(
            "sync job {job_id} cannot be retried from {value}"
        ))),
    }
    sqlx::query(
        "UPDATE sync_items SET status = 'queued', attempt_count = 0, \
         available_at = datetime('now'), lease_owner = NULL, lease_expires_at = NULL, \
         last_error = NULL, started_at = NULL, finished_at = NULL, updated_at = datetime('now') \
         WHERE job_id = ? AND status = 'failed'",
    ).bind(job_id).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE sync_jobs SET status = 'queued', failed_items = 0, error = NULL, \
         finished_at = NULL, updated_at = datetime('now') WHERE id = ?",
    ).bind(job_id).execute(&mut *tx).await?;
    tx.commit().await?;
    get_job(pool, job_id).await
}

/// Cancel all unfinished work. Completed items remain immutable and auditable.
pub async fn cancel_job(pool: &SqlitePool, job_id: i64) -> Result<SyncJobDetail> {
    let mut tx = pool.begin().await?;
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM sync_jobs WHERE id = ?")
        .bind(job_id).fetch_optional(&mut *tx).await?;
    match status.as_deref() {
        None => return Err(crate::error::AppError::NotFound(format!("sync job {job_id}"))),
        Some("queued" | "running" | "paused" | "failed") => {}
        Some(value) => return Err(crate::error::AppError::Metadata(format!(
            "sync job {job_id} cannot be cancelled from {value}"
        ))),
    }
    sqlx::query(
        "UPDATE sync_items SET status = 'cancelled', lease_owner = NULL, \
         lease_expires_at = NULL, finished_at = datetime('now'), updated_at = datetime('now') \
         WHERE job_id = ? AND status IN ('queued', 'leased', 'failed')",
    ).bind(job_id).execute(&mut *tx).await?;
    let completed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sync_items WHERE job_id = ? \
         AND status IN ('succeeded', 'excluded', 'cancelled')",
    ).bind(job_id).fetch_one(&mut *tx).await?;
    sqlx::query(
        "UPDATE sync_jobs SET status = 'cancelled', completed_items = ?, failed_items = 0, \
         finished_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
    ).bind(completed).bind(job_id).execute(&mut *tx).await?;
    tx.commit().await?;
    get_job(pool, job_id).await
}

/// Persist all discovered work and advance the provider token in one transaction.
/// A duplicate or invalid item rolls back both the job and checkpoint update.
pub async fn persist_discovery(pool: &SqlitePool, batch: &DiscoveryBatch<'_>) -> Result<SyncJob> {
    let mut tx = pool.begin().await?;
    let current_token: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT token FROM provider_checkpoints WHERE provider = ? AND scope_key = ?",
    )
    .bind(batch.provider)
    .bind(batch.scope_key)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    if current_token.as_deref() != batch.checkpoint_before {
        return Err(crate::error::AppError::Metadata(
            "provider checkpoint changed before discovery commit".to_owned(),
        ));
    }

    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO sync_jobs (\
             kind, provider, checkpoint_before, checkpoint_after, total_items\
         ) VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(batch.kind)
    .bind(batch.provider)
    .bind(batch.checkpoint_before)
    .bind(batch.checkpoint_after)
    .bind(batch.items.len() as i64)
    .fetch_one(&mut *tx)
    .await?;

    for item in &batch.items {
        sqlx::query(
            "INSERT INTO sync_items (\
                 job_id, source_id, external_id, operation, payload_json, max_attempts\
             ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(job_id)
        .bind(item.source_id)
        .bind(item.external_id)
        .bind(item.operation)
        .bind(item.payload_json)
        .bind(i64::from(item.max_attempts.max(1)))
        .execute(&mut *tx)
        .await?;
    }

    if batch.items.is_empty() {
        sqlx::query(
            "UPDATE sync_jobs SET status = 'completed', started_at = datetime('now'), \
             finished_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
        )
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO provider_checkpoints (provider, scope_key, token, generation) \
         VALUES (?, ?, ?, 1) \
         ON CONFLICT(provider, scope_key) DO UPDATE SET \
             token = excluded.token, generation = provider_checkpoints.generation + 1, \
             updated_at = datetime('now')",
    )
    .bind(batch.provider)
    .bind(batch.scope_key)
    .bind(batch.checkpoint_after)
    .execute(&mut *tx)
    .await?;

    let job = sqlx::query_as("SELECT * FROM sync_jobs WHERE id = ?")
        .bind(job_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(job)
}

/// Atomically lease the oldest available item across workers.
pub async fn claim_next_item(
    pool: &SqlitePool,
    worker: &str,
    lease_seconds: u32,
) -> Result<Option<SyncItem>> {
    let modifier = format!("+{} seconds", lease_seconds.max(1));
    let item: Option<SyncItem> = sqlx::query_as(
        "UPDATE sync_items SET \
             status = 'leased', attempt_count = attempt_count + 1, \
             lease_owner = ?, lease_expires_at = datetime('now', ?), \
             started_at = COALESCE(started_at, datetime('now')), updated_at = datetime('now') \
         WHERE id = (\
             SELECT id FROM sync_items \
             WHERE status = 'queued' AND available_at <= datetime('now') \
             ORDER BY available_at, id LIMIT 1\
         ) \
         RETURNING *",
    )
    .bind(worker)
    .bind(modifier)
    .fetch_optional(pool)
    .await?;

    if let Some(item) = &item {
        sqlx::query(
            "UPDATE sync_jobs SET status = 'running', \
             started_at = COALESCE(started_at, datetime('now')), updated_at = datetime('now') \
             WHERE id = ? AND status = 'queued'",
        )
        .bind(item.job_id)
        .execute(pool)
        .await?;
    }
    Ok(item)
}

pub async fn recover_expired_leases(pool: &SqlitePool) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let expired: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM sync_items \
         WHERE status = 'leased' AND lease_expires_at <= datetime('now')",
    ).fetch_all(&mut *tx).await?;
    for item_id in &expired {
        sqlx::query(
            "UPDATE sync_items SET \
             status = CASE WHEN attempt_count >= max_attempts THEN 'failed' ELSE 'queued' END, \
             lease_owner = NULL, lease_expires_at = NULL, \
             last_error = CASE WHEN attempt_count >= max_attempts \
                 THEN 'worker lease expired after maximum attempts' ELSE last_error END, \
             finished_at = CASE WHEN attempt_count >= max_attempts THEN datetime('now') ELSE NULL END, \
             updated_at = datetime('now') WHERE id = ? AND status = 'leased'",
        ).bind(item_id).execute(&mut *tx).await?;
        refresh_job_for_item(&mut tx, *item_id).await?;
    }
    tx.commit().await?;
    Ok(expired.len() as u64)
}

pub async fn mark_item_succeeded(pool: &SqlitePool, item_id: i64, worker: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE sync_items SET status = 'succeeded', lease_owner = NULL, \
         lease_expires_at = NULL, last_error = NULL, finished_at = datetime('now'), \
         updated_at = datetime('now') \
         WHERE id = ? AND status = 'leased' AND lease_owner = ?",
    )
    .bind(item_id)
    .bind(worker)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound.into());
    }
    refresh_job_for_item(&mut tx, item_id).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn fail_item(
    pool: &SqlitePool,
    item_id: i64,
    worker: &str,
    error: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let item: Option<(i64, i64)> = sqlx::query_as(
        "SELECT attempt_count, max_attempts FROM sync_items \
         WHERE id = ? AND status = 'leased' AND lease_owner = ?",
    )
    .bind(item_id)
    .bind(worker)
    .fetch_optional(&mut *tx)
    .await?;
    let (attempt_count, max_attempts) = item.ok_or(sqlx::Error::RowNotFound)?;
    let will_retry = attempt_count < max_attempts;

    if will_retry {
        let exponent = attempt_count.clamp(1, 10) as u32;
        let delay = 2_i64.pow(exponent).min(3600);
        let modifier = format!("+{delay} seconds");
        let result = sqlx::query(
            "UPDATE sync_items SET status = 'queued', available_at = datetime('now', ?), \
             lease_owner = NULL, lease_expires_at = NULL, last_error = ?, \
             updated_at = datetime('now') \
             WHERE id = ? AND status = 'leased' AND lease_owner = ?",
        )
        .bind(modifier)
        .bind(error)
        .bind(item_id)
        .bind(worker)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 { return Err(sqlx::Error::RowNotFound.into()); }
    } else {
        let result = sqlx::query(
            "UPDATE sync_items SET status = 'failed', lease_owner = NULL, \
             lease_expires_at = NULL, last_error = ?, finished_at = datetime('now'), \
             updated_at = datetime('now') \
             WHERE id = ? AND status = 'leased' AND lease_owner = ?",
        )
        .bind(error)
        .bind(item_id)
        .bind(worker)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 { return Err(sqlx::Error::RowNotFound.into()); }
    }
    refresh_job_for_item(&mut tx, item_id).await?;
    tx.commit().await?;
    Ok(will_retry)
}

async fn refresh_job_for_item(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    item_id: i64,
) -> Result<()> {
    let job_id: i64 = sqlx::query_scalar("SELECT job_id FROM sync_items WHERE id = ?")
        .bind(item_id)
        .fetch_one(&mut **tx)
        .await?;
    let (completed, failed, pending): (i64, i64, i64) = sqlx::query_as(
        "SELECT \
             SUM(CASE WHEN status IN ('succeeded', 'excluded', 'cancelled') THEN 1 ELSE 0 END), \
             SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), \
             SUM(CASE WHEN status IN ('queued', 'leased') THEN 1 ELSE 0 END) \
         FROM sync_items WHERE job_id = ?",
    )
    .bind(job_id)
    .fetch_one(&mut **tx)
    .await?;
    let status = if pending > 0 {
        "running"
    } else if failed > 0 {
        "failed"
    } else {
        "completed"
    };
    sqlx::query(
        "UPDATE sync_jobs SET completed_items = ?, failed_items = ?, status = ?, \
         finished_at = CASE WHEN ? = 0 THEN datetime('now') ELSE NULL END, \
         updated_at = datetime('now') WHERE id = ?",
    )
    .bind(completed)
    .bind(failed)
    .bind(status)
    .bind(pending)
    .bind(job_id)
    .execute(&mut **tx)
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

    fn item<'a>(external_id: &'a str, max_attempts: u32) -> DiscoveryItem<'a> {
        DiscoveryItem {
            source_id: None,
            external_id,
            operation: "download",
            payload_json: None,
            max_attempts,
        }
    }

    fn batch<'a>(before: Option<&'a [u8]>, after: Option<&'a [u8]>) -> DiscoveryBatch<'a> {
        DiscoveryBatch {
            kind: "apple_incremental",
            provider: "apple_photos",
            scope_key: "system",
            checkpoint_before: before,
            checkpoint_after: after,
            items: vec![item("asset-1", 3), item("asset-2", 3)],
        }
    }

    #[tokio::test]
    async fn discovery_persists_items_before_advancing_checkpoint() {
        let pool = test_pool().await;
        let job = persist_discovery(&pool, &batch(None, Some(b"token-1")))
            .await.unwrap();
        assert_eq!(job.total_items, 2);
        let checkpoint: ProviderCheckpoint = sqlx::query_as(
            "SELECT * FROM provider_checkpoints WHERE provider = 'apple_photos'",
        )
        .fetch_one(&pool).await.unwrap();
        let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_items")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(checkpoint.token.as_deref(), Some(b"token-1".as_slice()));
        assert_eq!(checkpoint.generation, 1);
        assert_eq!(items, 2);
    }

    #[tokio::test]
    async fn failed_discovery_rolls_back_job_items_and_checkpoint() {
        let pool = test_pool().await;
        let invalid = DiscoveryBatch {
            items: vec![item("duplicate", 3), item("duplicate", 3)],
            ..batch(None, Some(b"token-1"))
        };
        assert!(persist_discovery(&pool, &invalid).await.is_err());
        for table in ["sync_jobs", "sync_items", "provider_checkpoints"] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool).await.unwrap();
            assert_eq!(count, 0, "discovery partially committed {table}");
        }
    }

    #[tokio::test]
    async fn stale_checkpoint_cannot_overwrite_newer_discovery() {
        let pool = test_pool().await;
        persist_discovery(&pool, &batch(None, Some(b"token-1"))).await.unwrap();
        let stale = DiscoveryBatch {
            items: vec![item("asset-3", 3)],
            ..batch(None, Some(b"token-2"))
        };
        assert!(persist_discovery(&pool, &stale).await.is_err());
        let token: Vec<u8> = sqlx::query_scalar(
            "SELECT token FROM provider_checkpoints WHERE provider = 'apple_photos'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(token, b"token-1");
    }

    #[tokio::test]
    async fn empty_discovery_advances_checkpoint_and_completes_job() {
        let pool = test_pool().await;
        let empty = DiscoveryBatch {
            items: vec![],
            ..batch(None, Some(b"token-1"))
        };
        let job = persist_discovery(&pool, &empty).await.unwrap();
        assert_eq!(job.status, "completed");
        assert_eq!(job.total_items, 0);
        assert!(job.finished_at.is_some());
    }

    #[tokio::test]
    async fn worker_claims_and_completes_job() {
        let pool = test_pool().await;
        persist_discovery(&pool, &DiscoveryBatch {
            items: vec![item("asset-1", 3)],
            ..batch(None, Some(b"token-1"))
        }).await.unwrap();
        let claimed = claim_next_item(&pool, "worker-1", 60).await.unwrap().unwrap();
        assert_eq!(claimed.status, "leased");
        assert_eq!(claimed.attempt_count, 1);
        mark_item_succeeded(&pool, claimed.id, "worker-1").await.unwrap();

        let job: SyncJob = sqlx::query_as("SELECT * FROM sync_jobs")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(job.status, "completed");
        assert_eq!(job.completed_items, 1);
        assert!(job.finished_at.is_some());
    }

    #[tokio::test]
    async fn expired_lease_returns_to_queue() {
        let pool = test_pool().await;
        persist_discovery(&pool, &DiscoveryBatch {
            items: vec![item("asset-1", 3)],
            ..batch(None, Some(b"token-1"))
        }).await.unwrap();
        let claimed = claim_next_item(&pool, "dead-worker", 60).await.unwrap().unwrap();
        sqlx::query("UPDATE sync_items SET lease_expires_at = datetime('now', '-1 second') WHERE id = ?")
            .bind(claimed.id).execute(&pool).await.unwrap();

        assert_eq!(recover_expired_leases(&pool).await.unwrap(), 1);
        let recovered = claim_next_item(&pool, "worker-2", 60).await.unwrap().unwrap();
        assert_eq!(recovered.id, claimed.id);
        assert_eq!(recovered.attempt_count, 2);
    }

    #[tokio::test]
    async fn expired_lease_at_attempt_limit_fails_job() {
        let pool = test_pool().await;
        persist_discovery(&pool, &DiscoveryBatch {
            items: vec![item("asset-1", 1)],
            ..batch(None, Some(b"token-1"))
        }).await.unwrap();
        let claimed = claim_next_item(&pool, "dead-worker", 60).await.unwrap().unwrap();
        sqlx::query("UPDATE sync_items SET lease_expires_at = datetime('now', '-1 second') WHERE id = ?")
            .bind(claimed.id).execute(&pool).await.unwrap();
        assert_eq!(recover_expired_leases(&pool).await.unwrap(), 1);
        assert!(claim_next_item(&pool, "worker-2", 60).await.unwrap().is_none());
        let detail = get_job(&pool, claimed.job_id).await.unwrap();
        assert_eq!(detail.job.status, "failed");
        assert_eq!(detail.job.failed_items, 1);
        assert_eq!(detail.items[0].status, "failed");
        assert!(detail.items[0].last_error.as_deref().unwrap().contains("lease expired"));
    }

    #[tokio::test]
    async fn only_lease_owner_can_complete_item() {
        let pool = test_pool().await;
        persist_discovery(&pool, &DiscoveryBatch {
            items: vec![item("asset-1", 3)],
            ..batch(None, Some(b"token-1"))
        }).await.unwrap();
        let claimed = claim_next_item(&pool, "worker-1", 60).await.unwrap().unwrap();
        assert!(mark_item_succeeded(&pool, claimed.id, "worker-2").await.is_err());
        let status: String = sqlx::query_scalar("SELECT status FROM sync_items WHERE id = ?")
            .bind(claimed.id).fetch_one(&pool).await.unwrap();
        assert_eq!(status, "leased");
    }

    #[tokio::test]
    async fn failures_retry_then_become_terminal() {
        let pool = test_pool().await;
        persist_discovery(&pool, &DiscoveryBatch {
            items: vec![item("asset-1", 2)],
            ..batch(None, Some(b"token-1"))
        }).await.unwrap();
        let first = claim_next_item(&pool, "worker", 60).await.unwrap().unwrap();
        assert!(fail_item(&pool, first.id, "worker", "network").await.unwrap());
        sqlx::query("UPDATE sync_items SET available_at = datetime('now') WHERE id = ?")
            .bind(first.id).execute(&pool).await.unwrap();
        let second = claim_next_item(&pool, "worker", 60).await.unwrap().unwrap();
        assert!(!fail_item(&pool, second.id, "worker", "network").await.unwrap());

        let job: SyncJob = sqlx::query_as("SELECT * FROM sync_jobs")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(job.status, "failed");
        assert_eq!(job.failed_items, 1);
    }

    #[tokio::test]
    async fn failed_job_retry_preserves_success_and_requeues_failures() {
        let pool = test_pool().await;
        let job = persist_discovery(&pool, &batch(None, Some(b"token-1"))).await.unwrap();
        let first = claim_next_item(&pool, "worker", 60).await.unwrap().unwrap();
        mark_item_succeeded(&pool, first.id, "worker").await.unwrap();
        let second = claim_next_item(&pool, "worker", 60).await.unwrap().unwrap();
        sqlx::query("UPDATE sync_items SET attempt_count = max_attempts WHERE id = ?")
            .bind(second.id).execute(&pool).await.unwrap();
        assert!(!fail_item(&pool, second.id, "worker", "offline").await.unwrap());
        let retried = retry_job(&pool, job.id).await.unwrap();
        assert_eq!(retried.job.status, "queued");
        assert_eq!(retried.job.completed_items, 1);
        assert_eq!(retried.job.failed_items, 0);
        assert_eq!(retried.items[0].status, "succeeded");
        assert_eq!(retried.items[1].status, "queued");
        assert_eq!(retried.items[1].attempt_count, 0);
        assert!(retried.items[1].last_error.is_none());
    }

    #[tokio::test]
    async fn cancellation_releases_leases_and_is_not_retryable() {
        let pool = test_pool().await;
        let job = persist_discovery(&pool, &batch(None, Some(b"token-1"))).await.unwrap();
        claim_next_item(&pool, "worker", 60).await.unwrap().unwrap();
        let cancelled = cancel_job(&pool, job.id).await.unwrap();
        assert_eq!(cancelled.job.status, "cancelled");
        assert_eq!(cancelled.job.completed_items, 2);
        assert!(cancelled.items.iter().all(|item| item.status == "cancelled"));
        assert!(cancelled.items.iter().all(|item| item.lease_owner.is_none()));
        assert!(retry_job(&pool, job.id).await.is_err());
        assert!(cancel_job(&pool, job.id).await.is_err());
    }

    #[tokio::test]
    async fn job_queries_filter_and_cursor_without_hiding_details() {
        let pool = test_pool().await;
        let first = persist_discovery(&pool, &batch(None, Some(b"token-1"))).await.unwrap();
        let second = persist_discovery(&pool, &DiscoveryBatch {
            checkpoint_before: Some(b"token-1"), checkpoint_after: Some(b"token-2"),
            items: vec![], ..batch(None, None)
        }).await.unwrap();
        let completed = list_jobs(&pool, Some("apple_photos"), Some("completed"), None, 10)
            .await.unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].id, second.id);
        let older = list_jobs(&pool, None, None, Some(second.id), 10).await.unwrap();
        assert_eq!(older.len(), 1);
        assert_eq!(older[0].id, first.id);
        assert_eq!(get_job(&pool, first.id).await.unwrap().items.len(), 2);
        assert!(get_job(&pool, 9999).await.is_err());
    }
}
