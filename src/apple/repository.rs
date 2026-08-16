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
#[derive(Debug, Clone, Serialize)]
pub struct AppleExportClaim { pub item_id: i64, pub source: AppleSourceView }

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AppleLinkCandidate {
    pub id: i64,
    pub source_id: i64,
    pub photo_id: i64,
    pub method: String,
    pub confidence: f64,
    pub status: String,
    pub evidence_json: Option<String>,
    pub original_filename: Option<String>,
    pub source_taken_at: Option<String>,
    pub source_width: Option<i64>,
    pub source_height: Option<i64>,
    pub photo_path: String,
    pub photo_taken_at: Option<String>,
    pub photo_width: Option<i64>,
    pub photo_height: Option<i64>,
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
pub async fn claim_next_export(pool: &SqlitePool, worker: &str, lease_seconds: u32) -> Result<Option<AppleExportClaim>> {
    let mut tx = pool.begin().await?;
    let item: Option<(i64, i64)> = sqlx::query_as("SELECT i.id, i.source_id FROM sync_items i JOIN asset_sources s ON s.id = i.source_id WHERE i.status = 'queued' AND i.available_at <= datetime('now') AND i.operation = 'export_original' AND s.provider = 'apple_photos' ORDER BY i.available_at, i.id LIMIT 1").fetch_optional(&mut *tx).await?;
    let Some((item_id, source_id)) = item else { tx.commit().await?; return Ok(None); };
    let modifier = format!("+{} seconds", lease_seconds.clamp(60, 3600));
    sqlx::query("UPDATE sync_items SET status = 'leased', attempt_count = attempt_count + 1, lease_owner = ?, lease_expires_at = datetime('now', ?), started_at = COALESCE(started_at, datetime('now')), updated_at = datetime('now') WHERE id = ? AND status = 'queued'").bind(worker).bind(modifier).bind(item_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE asset_sources SET sync_status = 'downloading', last_error = NULL, updated_at = datetime('now') WHERE id = ?").bind(source_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE sync_jobs SET status = 'running', started_at = COALESCE(started_at, datetime('now')), updated_at = datetime('now') WHERE id = (SELECT job_id FROM sync_items WHERE id = ?)").bind(item_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Some(AppleExportClaim { item_id, source: get_source(pool, source_id).await? }))
}
pub async fn renew_export_lease(pool: &SqlitePool, item_id: i64, worker: &str, lease_seconds: u32) -> Result<()> {
    let modifier = format!("+{} seconds", lease_seconds.clamp(60, 3600));
    let result = sqlx::query("UPDATE sync_items SET lease_expires_at = datetime('now', ?), updated_at = datetime('now') WHERE id = ? AND status = 'leased' AND lease_owner = ?").bind(modifier).bind(item_id).bind(worker).execute(pool).await?;
    if result.rows_affected() == 0 { return Err(AppError::NotFound("Apple export lease".into())); }
    Ok(())
}
pub async fn fail_export(pool: &SqlitePool, item_id: i64, worker: &str, error: &str) -> Result<()> {
    let retrying = crate::sync::fail_item(pool, item_id, worker, error).await?;
    let status = if retrying { "queued" } else { "failed" };
    sqlx::query("UPDATE asset_sources SET sync_status = ?, last_error = ?, updated_at = datetime('now') WHERE id = (SELECT source_id FROM sync_items WHERE id = ?)")
        .bind(status).bind(error).bind(item_id).execute(pool).await?;
    Ok(())
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

pub async fn list_link_candidates(
    pool: &SqlitePool,
    before_id: Option<i64>,
    limit: u32,
) -> Result<Vec<AppleLinkCandidate>> {
    Ok(sqlx::query_as(
        "SELECT l.id, l.source_id, l.photo_id, l.method, l.confidence, l.status, l.evidence_json, \
         s.original_filename, s.taken_at AS source_taken_at, s.width AS source_width, \
         s.height AS source_height, p.path AS photo_path, p.taken_at AS photo_taken_at, \
         p.width AS photo_width, p.height AS photo_height \
         FROM asset_links l JOIN asset_sources s ON s.id = l.source_id \
         JOIN photos p ON p.id = l.photo_id \
         WHERE s.provider = 'apple_photos' AND l.status IN ('candidate', 'conflict') \
           AND (? IS NULL OR l.id < ?) ORDER BY l.id DESC LIMIT ?",
    )
    .bind(before_id)
    .bind(before_id)
    .bind(i64::from(limit.clamp(1, 100)))
    .fetch_all(pool)
    .await?)
}

pub async fn review_link(pool: &SqlitePool, link_id: i64, accept: bool) -> Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let link: Option<(i64, i64, String, Option<i64>, String)> = sqlx::query_as(
        "SELECT l.source_id, l.photo_id, l.status, s.asset_id, s.external_id \
         FROM asset_links l JOIN asset_sources s ON s.id = l.source_id \
         WHERE l.id = ? AND s.provider = 'apple_photos'",
    )
    .bind(link_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (source_id, photo_id, status, existing_asset_id, external_id) =
        link.ok_or_else(|| AppError::NotFound(format!("Apple Photos link candidate {link_id}")))?;
    if !matches!(status.as_str(), "candidate" | "conflict") {
        return Err(AppError::Metadata(format!(
            "link candidate {link_id} is already {status}"
        )));
    }

    if accept {
        sqlx::query("INSERT OR IGNORE INTO assets (photo_id) VALUES (?)")
            .bind(photo_id)
            .execute(&mut *tx)
            .await?;
        let asset_id: i64 = sqlx::query_scalar("SELECT id FROM assets WHERE photo_id = ?")
            .bind(photo_id)
            .fetch_one(&mut *tx)
            .await?;
        if existing_asset_id.is_some_and(|existing| existing != asset_id) {
            return Err(AppError::Metadata(
                "source is already linked to another asset".into(),
            ));
        }
        sqlx::query(
            "UPDATE asset_links SET status = 'accepted', confidence = 1.0, \
             reviewed_at = datetime('now') WHERE id = ?",
        )
        .bind(link_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE asset_links SET status = 'rejected', reviewed_at = datetime('now') \
             WHERE source_id = ? AND id != ? AND status IN ('candidate', 'conflict')",
        )
        .bind(source_id)
        .bind(link_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE asset_sources SET asset_id = ?, sync_status = 'ready', last_error = NULL, \
             updated_at = datetime('now') WHERE id = ?",
        )
        .bind(asset_id)
        .bind(source_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(None);
    }

    sqlx::query(
        "UPDATE asset_links SET status = 'rejected', reviewed_at = datetime('now') WHERE id = ?",
    )
    .bind(link_id)
    .execute(&mut *tx)
    .await?;
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM asset_links WHERE source_id = ? AND status IN ('candidate', 'conflict')",
    ).bind(source_id).fetch_one(&mut *tx).await?;
    let job_id = if remaining == 0 && existing_asset_id.is_none() {
        let job_id: i64 = sqlx::query_scalar(
            "INSERT INTO sync_jobs (kind, provider, total_items) \
             VALUES ('apple_candidate_rejected', 'apple_photos', 1) RETURNING id",
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
        sqlx::query("UPDATE asset_sources SET sync_status = 'queued', updated_at = datetime('now') WHERE id = ?")
            .bind(source_id).execute(&mut *tx).await?;
        Some(job_id)
    } else {
        None
    };
    tx.commit().await?;
    Ok(job_id)
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

    #[tokio::test]
    async fn export_claim_leases_item_and_marks_source_downloading() {
        let pool = test_pool().await;
        let source_id = source(&pool, "export-one", "IMG_0001.HEIC", "queued").await;
        let job_id: i64 = sqlx::query_scalar(
            "INSERT INTO sync_jobs (kind, provider, total_items) \
             VALUES ('apple_full_inventory', 'apple_photos', 1) RETURNING id",
        ).fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO sync_items (job_id, source_id, external_id, operation) \
             VALUES (?, ?, 'export-one', 'export_original')",
        ).bind(job_id).bind(source_id).execute(&pool).await.unwrap();
        let claim = claim_next_export(&pool, "native-test", 300).await.unwrap().unwrap();
        assert_eq!(claim.source.id, source_id);
        assert_eq!(claim.source.sync_status, "downloading");
        assert!(claim_next_export(&pool, "another-native-test", 300).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn candidate_review_links_existing_photo_or_queues_after_rejection() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) \
             VALUES (1, '/one.jpg', 'one', 'jpeg', 'imported'), \
                    (2, '/two.jpg', 'two', 'jpeg', 'imported')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let source_id = source(&pool, "apple-one", "IMG_0001.JPG", "discovered").await;
        let first: i64 = sqlx::query_scalar(
            "INSERT INTO asset_links (source_id, photo_id, method, confidence, status) \
             VALUES (?, 1, 'structured_metadata', 0.9, 'candidate') RETURNING id",
        )
        .bind(source_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let second: i64 = sqlx::query_scalar(
            "INSERT INTO asset_links (source_id, photo_id, method, confidence, status) \
             VALUES (?, 2, 'structured_metadata', 0.8, 'candidate') RETURNING id",
        )
        .bind(source_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(review_link(&pool, first, false).await.unwrap().is_none());
        assert!(review_link(&pool, second, true).await.unwrap().is_none());
        let linked: (String, Option<i64>) =
            sqlx::query_as("SELECT sync_status, asset_id FROM asset_sources WHERE id = ?")
                .bind(source_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(linked.0, "ready");
        assert!(linked.1.is_some());

        let rejected_source = source(&pool, "apple-two", "IMG_0002.JPG", "discovered").await;
        let rejected: i64 = sqlx::query_scalar(
            "INSERT INTO asset_links (source_id, photo_id, method, confidence, status) \
             VALUES (?, 1, 'structured_metadata', 0.7, 'candidate') RETURNING id",
        )
        .bind(rejected_source)
        .fetch_one(&pool)
        .await
        .unwrap();
        let job_id = review_link(&pool, rejected, false).await.unwrap().unwrap();
        let item_job: i64 = sqlx::query_scalar("SELECT job_id FROM sync_items WHERE source_id = ?")
            .bind(rejected_source)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(item_job, job_id);
    }
}
