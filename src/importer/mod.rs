pub mod scanner;
pub mod state;

use sqlx::SqlitePool;
use std::path::Path;
use crate::album;
use crate::dedup::hash::compute_phash;
use crate::error::Result;
use crate::metadata;
use state::{ImportDecision, compute_sha256, decide};

#[derive(Debug, Default)]
pub struct ImportSummary {
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
}

pub async fn import_dir(pool: &SqlitePool, source_dir: &Path) -> Result<ImportSummary> {
    let paths = scanner::scan_dir(source_dir);
    let mut summary = ImportSummary { total: paths.len(), ..Default::default() };

    for path in &paths {
        match import_one(pool, path).await {
            Ok(true)  => summary.imported += 1,
            Ok(false) => summary.skipped += 1,
            Err(e) => {
                tracing::warn!("failed to import {}: {e}", path.display());
                summary.errors += 1;
            }
        }
    }
    album::group_by_month(pool).await?;
    album::group_by_camera(pool).await?;

    Ok(summary)
}

/// 返り値: true = 新規インポート, false = スキップ
async fn import_one(pool: &SqlitePool, path: &Path) -> Result<bool> {
    let sha256 = compute_sha256(path)?;
    let decision = decide(pool, path, &sha256).await?;

    match decision {
        ImportDecision::AlreadyImported | ImportDecision::Duplicate { .. } => {
            let status = if matches!(decision, ImportDecision::Duplicate { .. }) {
                "duplicate"
            } else {
                "imported"
            };
            sqlx::query(
                "INSERT OR IGNORE INTO photos (path, sha256, format, import_status) VALUES (?, ?, ?, ?)",
            )
            .bind(path.to_string_lossy().as_ref())
            .bind(&sha256)
            .bind("unknown")
            .bind(status)
            .execute(pool)
            .await?;
            return Ok(false);
        }
        ImportDecision::New => {}
    }

    let meta = metadata::extract_from_file(path)?;
    let phash = compute_phash(path).ok();

    sqlx::query(
        "INSERT INTO photos (path, sha256, phash, format, taken_at, gps_lat, gps_lon, camera, import_status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'imported')",
    )
    .bind(path.to_string_lossy().as_ref())
    .bind(&sha256)
    .bind(&phash)
    .bind(meta.format.as_str())
    .bind(meta.taken_at.map(|t| t.to_string()))
    .bind(meta.gps_lat)
    .bind(meta.gps_lon)
    .bind(meta.camera)
    .execute(pool)
    .await?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::fs;
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

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[tokio::test]
    async fn import_fixtures_dir() {
        let pool = test_pool().await;
        let summary = import_dir(&pool, &fixtures_dir()).await.unwrap();
        assert!(summary.total > 0);
        assert_eq!(summary.errors, 0);
        assert!(summary.imported > 0);
    }

    #[tokio::test]
    async fn second_import_is_idempotent() {
        let pool = test_pool().await;
        let s1 = import_dir(&pool, &fixtures_dir()).await.unwrap();
        let s2 = import_dir(&pool, &fixtures_dir()).await.unwrap();
        assert_eq!(s1.imported, s2.skipped);
        assert_eq!(s2.imported, 0);
    }

    #[tokio::test]
    async fn import_empty_dir_returns_zero() {
        let pool = test_pool().await;
        let dir = tempdir().unwrap();
        let summary = import_dir(&pool, dir.path()).await.unwrap();
        assert_eq!(summary.total, 0);
        assert_eq!(summary.imported, 0);
    }

    #[tokio::test]
    async fn duplicate_file_marked_as_skipped() {
        let pool = test_pool().await;
        let dir = tempdir().unwrap();
        let src = fixtures_dir().join("with_exif.jpg");

        // 同じ内容で2つのパス
        fs::copy(&src, dir.path().join("a.jpg")).unwrap();
        fs::copy(&src, dir.path().join("b.jpg")).unwrap();

        let summary = import_dir(&pool, dir.path()).await.unwrap();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
    }
}
