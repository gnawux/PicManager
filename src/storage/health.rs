use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::config::Config;
use crate::error::Result;

use super::{ReconciliationReport, reconcile, reconcile_startup};

#[derive(Debug, Clone, Serialize)]
pub struct StartupRecoveryReport {
    pub application_leases_recovered: u64,
    pub sync_leases_recovered: u64,
    pub filesystem_intents_recovered: usize,
    pub filesystem_intents_failed: usize,
    pub derived_records_reset: usize,
    pub stale_cache_files_removed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub status: &'static str,
    pub checked_at: String,
    pub sqlite_quick_check: String,
    pub journal_mode: String,
    pub schema_version: i64,
    pub application_jobs_queued: i64,
    pub application_jobs_running: i64,
    pub application_jobs_failed: i64,
    pub sync_jobs_failed: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconciliation: Option<ReconciliationReport>,
}

pub async fn recover_startup(pool: &SqlitePool, _config: &Config) -> Result<StartupRecoveryReport> {
    let application_leases_recovered = crate::jobs::recover_expired(pool).await?;
    let sync_leases_recovered = crate::sync::recover_expired_leases(pool).await?;
    let reconciliation = reconcile_startup(pool, true).await?;
    Ok(StartupRecoveryReport {
        application_leases_recovered,
        sync_leases_recovered,
        filesystem_intents_recovered: reconciliation.recovered_filesystem_intents,
        filesystem_intents_failed: reconciliation.failed_filesystem_intents,
        derived_records_reset: reconciliation.missing_ready_thumbnails,
        stale_cache_files_removed: reconciliation.removed_cache_files,
    })
}

pub async fn health_report(pool: &SqlitePool, config: &Config, deep: bool) -> Result<HealthReport> {
    let sqlite_quick_check: String = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(pool)
        .await?;
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(pool)
        .await?;
    let schema_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
    )
    .fetch_one(pool)
    .await?;
    let application_jobs_queued = application_job_count(pool, "queued").await?;
    let application_jobs_running = application_job_count(pool, "running").await?;
    let application_jobs_failed = application_job_count(pool, "failed").await?;
    let sync_jobs_failed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sync_jobs WHERE status = 'failed'")
            .fetch_one(pool)
            .await?;
    let reconciliation = if deep {
        Some(reconcile(pool, config, false).await?)
    } else {
        None
    };
    let reconciliation_healthy = reconciliation.as_ref().is_none_or(|report| {
        report.missing_photo_files == 0
            && report.missing_variant_files == 0
            && report.invalid_master_pointers == 0
            && report.invalid_display_pointers == 0
            && report.incomplete_filesystem_intents == 0
    });
    let status = if sqlite_quick_check == "ok" && reconciliation_healthy {
        "healthy"
    } else {
        "degraded"
    };
    Ok(HealthReport {
        status,
        checked_at: Utc::now().to_rfc3339(),
        sqlite_quick_check,
        journal_mode,
        schema_version,
        application_jobs_queued,
        application_jobs_running,
        application_jobs_failed,
        sync_jobs_failed,
        reconciliation,
    })
}

async fn application_job_count(pool: &SqlitePool, status: &str) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM application_jobs WHERE status = ?")
            .bind(status)
            .fetch_one(pool)
            .await?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn health_is_structured_and_deep_checks_degrade_on_missing_media() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let config = Config::default();
        assert_eq!(
            health_report(&pool, &config, false).await.unwrap().status,
            "healthy"
        );
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status) \
             VALUES ('/missing/health.jpg', 'health-missing', 'jpeg', 'imported')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let report = health_report(&pool, &config, true).await.unwrap();
        assert_eq!(report.status, "degraded");
        assert_eq!(report.reconciliation.unwrap().missing_photo_files, 1);
    }
}
