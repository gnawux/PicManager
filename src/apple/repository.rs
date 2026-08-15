use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use std::collections::BTreeMap;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AppleSourceView {
    pub id: i64,
    pub asset_id: Option<i64>,
    pub photo_id: Option<i64>,
    pub external_id: String,
    pub original_filename: Option<String>,
    pub media_type: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub taken_at: Option<String>,
    pub sync_status: String,
    pub exclusion_reason: Option<String>,
    pub last_error: Option<String>,
    pub last_seen_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppleSourcePage {
    pub sources: Vec<AppleSourceView>,
    pub status_counts: BTreeMap<String, i64>,
    pub next_before_id: Option<i64>,
}

pub async fn list_sources(
    pool: &SqlitePool,
    status: Option<&str>,
    search: Option<&str>,
    before_id: Option<i64>,
    limit: u32,
) -> Result<AppleSourcePage> {
    let status = match status {
        Some("synced") => Some("ready"),
        Some("all") | None => None,
        value => value,
    };
    let sources: Vec<AppleSourceView> = sqlx::query_as(
        "SELECT s.id, s.asset_id, a.photo_id, s.external_id, s.original_filename, \
         s.media_type, s.width, s.height, s.taken_at, s.sync_status, s.exclusion_reason, \
         s.last_error, s.last_seen_at, s.updated_at \
         FROM asset_sources s LEFT JOIN assets a ON a.id = s.asset_id \
         WHERE s.provider = 'apple_photos' \
           AND (? IS NULL OR s.sync_status = ?) \
           AND (? IS NULL OR s.original_filename LIKE '%' || ? || '%' \
                OR s.external_id LIKE '%' || ? || '%') \
           AND (? IS NULL OR s.id < ?) \
         ORDER BY s.id DESC LIMIT ?",
    )
    .bind(status)
    .bind(status)
    .bind(search)
    .bind(search)
    .bind(search)
    .bind(before_id)
    .bind(before_id)
    .bind(i64::from(limit.clamp(1, 100)))
    .fetch_all(pool)
    .await?;
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT sync_status, COUNT(*) FROM asset_sources \
         WHERE provider = 'apple_photos' GROUP BY sync_status ORDER BY sync_status",
    )
    .fetch_all(pool)
    .await?;
    let mut status_counts: BTreeMap<String, i64> = rows.into_iter().collect();
    if let Some(ready) = status_counts.remove("ready") {
        status_counts.insert("synced".to_owned(), ready);
    }
    let next_before_id = sources.last().map(|source| source.id);
    Ok(AppleSourcePage {
        sources,
        status_counts,
        next_before_id,
    })
}

pub async fn get_source(pool: &SqlitePool, source_id: i64) -> Result<AppleSourceView> {
    sqlx::query_as(
        "SELECT s.id, s.asset_id, a.photo_id, s.external_id, s.original_filename, \
         s.media_type, s.width, s.height, s.taken_at, s.sync_status, s.exclusion_reason, \
         s.last_error, s.last_seen_at, s.updated_at \
         FROM asset_sources s LEFT JOIN assets a ON a.id = s.asset_id \
         WHERE s.provider = 'apple_photos' AND s.id = ?",
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Apple Photos source {source_id}")))
}

pub async fn retry_source(pool: &SqlitePool, source_id: i64) -> Result<(i64, AppleSourceView)> {
    let mut tx = pool.begin().await?;
    let source: Option<(String, String)> = sqlx::query_as(
        "SELECT external_id, sync_status FROM asset_sources \
         WHERE provider = 'apple_photos' AND id = ?",
    )
    .bind(source_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (external_id, status) =
        source.ok_or_else(|| AppError::NotFound(format!("Apple Photos source {source_id}")))?;
    if status != "failed" {
        return Err(AppError::Metadata(format!(
            "Apple Photos source {source_id} cannot be retried from {status}"
        )));
    }
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO sync_jobs (kind, provider, total_items) \
         VALUES ('apple_source_retry', 'apple_photos', 1) RETURNING id",
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO sync_items (job_id, source_id, external_id, operation) \
         VALUES (?, ?, ?, 'export_original')",
    )
    .bind(job_id)
    .bind(source_id)
    .bind(external_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE asset_sources SET sync_status = 'queued', last_error = NULL, \
         updated_at = datetime('now') WHERE id = ?",
    )
    .bind(source_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((job_id, get_source(pool, source_id).await?))
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

    async fn source(pool: &SqlitePool, id: &str, filename: &str, status: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO asset_sources (provider, external_id, original_filename, sync_status) \
             VALUES ('apple_photos', ?, ?, ?) RETURNING id",
        )
        .bind(id)
        .bind(filename)
        .bind(status)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn source_page_maps_ready_to_synced_and_filters_by_filename() {
        let pool = test_pool().await;
        source(&pool, "one", "IMG_0001.HEIC", "ready").await;
        source(&pool, "two", "Vacation.JPG", "failed").await;
        let page = list_sources(&pool, Some("synced"), Some("IMG"), None, 20)
            .await
            .unwrap();
        assert_eq!(page.sources.len(), 1);
        assert_eq!(page.sources[0].sync_status, "ready");
        assert_eq!(page.status_counts.get("synced"), Some(&1));
        assert_eq!(page.status_counts.get("failed"), Some(&1));
    }

    #[tokio::test]
    async fn only_failed_source_can_create_retry_job() {
        let pool = test_pool().await;
        let failed = source(&pool, "one", "IMG_0001.HEIC", "failed").await;
        let ready = source(&pool, "two", "IMG_0002.HEIC", "ready").await;
        let (job_id, retried) = retry_source(&pool, failed).await.unwrap();
        assert_eq!(retried.sync_status, "queued");
        let item_job: i64 = sqlx::query_scalar("SELECT job_id FROM sync_items WHERE source_id = ?")
            .bind(failed)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(item_job, job_id);
        assert!(retry_source(&pool, failed).await.is_err());
        assert!(retry_source(&pool, ready).await.is_err());
    }
}
