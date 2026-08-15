mod backfill;
mod verification;

pub use backfill::{backfill_legacy_local, BackfillReport};
pub use verification::{list_migration_runs, verify_catalog, CatalogVerificationReport, MigrationRun};

use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::Result;

const MISSING_FILE_SAMPLE_LIMIT: usize = 20;

/// Read-only snapshot used as the safety baseline before and after catalog migrations.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IntegrityReport {
    pub schema_version: i64,
    pub total_photos: i64,
    pub status_counts: BTreeMap<String, i64>,
    pub active_photos: i64,
    pub recorded_active_photos: i64,
    pub active_count_matches: bool,
    pub duplicate_sha_groups: i64,
    pub foreign_key_violations: usize,
    pub sqlite_integrity: String,
    pub checked_files: bool,
    pub missing_files: usize,
    pub missing_file_samples: Vec<String>,
}

impl IntegrityReport {
    pub fn is_healthy(&self) -> bool {
        self.active_count_matches
            && self.foreign_key_violations == 0
            && self.sqlite_integrity == "ok"
            && (!self.checked_files || self.missing_files == 0)
    }
}

/// Inspect the current catalog without modifying database or media files.
pub async fn inspect(pool: &SqlitePool, check_files: bool) -> Result<IntegrityReport> {
    let schema_version: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
    )
    .fetch_one(pool)
    .await?;

    let total_photos: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photos")
        .fetch_one(pool)
        .await?;

    let status_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT import_status, COUNT(*) FROM photos GROUP BY import_status ORDER BY import_status",
    )
    .fetch_all(pool)
    .await?;
    let status_counts: BTreeMap<String, i64> = status_rows.into_iter().collect();
    let active_photos = status_counts.get("imported").copied().unwrap_or(0);

    let recorded_active_photos: i64 =
        sqlx::query_scalar("SELECT active_count FROM photo_stats WHERE id = 1")
            .fetch_optional(pool)
            .await?
            .unwrap_or(0);

    let duplicate_sha_groups: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (\
             SELECT sha256 FROM photos WHERE import_status = 'imported' \
             GROUP BY sha256 HAVING COUNT(*) > 1\
         )",
    )
    .fetch_one(pool)
    .await?;

    let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await?
        .len();

    let sqlite_integrity: String = sqlx::query("PRAGMA integrity_check")
        .fetch_one(pool)
        .await?
        .try_get(0)?;

    let (missing_files, missing_file_samples) = if check_files {
        let paths: Vec<String> = sqlx::query_scalar(
            "SELECT path FROM photos WHERE import_status = 'imported' ORDER BY id",
        )
        .fetch_all(pool)
        .await?;
        tokio::task::spawn_blocking(move || inspect_paths(paths))
            .await
            .unwrap_or_else(|_| (0, Vec::new()))
    } else {
        (0, Vec::new())
    };

    Ok(IntegrityReport {
        schema_version,
        total_photos,
        status_counts,
        active_photos,
        recorded_active_photos,
        active_count_matches: active_photos == recorded_active_photos,
        duplicate_sha_groups,
        foreign_key_violations,
        sqlite_integrity,
        checked_files: check_files,
        missing_files,
        missing_file_samples,
    })
}

fn inspect_paths(paths: Vec<String>) -> (usize, Vec<String>) {
    let mut missing = 0usize;
    let mut samples = Vec::new();
    for path in paths {
        if !PathBuf::from(&path).is_file() {
            missing += 1;
            if samples.len() < MISSING_FILE_SAMPLE_LIMIT {
                samples.push(path);
            }
        }
    }
    (missing, samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::tempdir;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn empty_catalog_is_healthy() {
        let pool = test_pool().await;
        let report = inspect(&pool, true).await.unwrap();

        assert!(report.is_healthy());
        assert!(report.schema_version >= 19);
        assert_eq!(report.total_photos, 0);
        assert!(report.status_counts.is_empty());
        assert_eq!(report.missing_files, 0);
    }

    #[tokio::test]
    async fn reports_status_counts_and_stats_mismatch() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status) VALUES \
             ('/missing/a.jpg', 'a', 'jpeg', 'imported'), \
             ('/missing/b.jpg', 'b', 'jpeg', 'deleted')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let report = inspect(&pool, false).await.unwrap();

        assert_eq!(report.total_photos, 2);
        assert_eq!(report.status_counts.get("imported"), Some(&1));
        assert_eq!(report.status_counts.get("deleted"), Some(&1));
        assert!(!report.active_count_matches);
        assert!(!report.is_healthy());
        assert!(!report.checked_files);
    }

    #[tokio::test]
    async fn reports_missing_files_but_limits_samples() {
        let pool = test_pool().await;
        for i in 0..25 {
            sqlx::query(
                "INSERT INTO photos (path, sha256, format, import_status) VALUES (?, ?, 'jpeg', 'imported')",
            )
            .bind(format!("/definitely/missing/{i}.jpg"))
            .bind(format!("sha-{i}"))
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("UPDATE photo_stats SET active_count = 25 WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();

        let report = inspect(&pool, true).await.unwrap();

        assert_eq!(report.missing_files, 25);
        assert_eq!(report.missing_file_samples.len(), MISSING_FILE_SAMPLE_LIMIT);
        assert!(!report.is_healthy());
    }

    #[tokio::test]
    async fn existing_file_keeps_report_healthy() {
        let pool = test_pool().await;
        let dir = tempdir().unwrap();
        let path = dir.path().join("photo.jpg");
        std::fs::write(&path, b"fixture").unwrap();
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status) VALUES (?, 'a', 'jpeg', 'imported')",
        )
        .bind(path.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE photo_stats SET active_count = 1 WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();

        let report = inspect(&pool, true).await.unwrap();

        assert!(report.is_healthy());
        assert_eq!(report.missing_files, 0);
    }

    #[tokio::test]
    async fn reports_duplicate_active_hash_groups() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status) VALUES \
             ('/a.jpg', 'same', 'jpeg', 'imported'), \
             ('/b.jpg', 'same', 'jpeg', 'imported'), \
             ('/c.jpg', 'same', 'jpeg', 'deleted')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let report = inspect(&pool, false).await.unwrap();
        assert_eq!(report.duplicate_sha_groups, 1);
    }
}
