pub mod placer;
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

pub async fn import_dir(
    pool: &SqlitePool,
    source_dir: &Path,
    library_path: &Path,
    copy_only: bool,
) -> Result<ImportSummary> {
    let paths = scanner::scan_dir(source_dir);
    let mut summary = ImportSummary { total: paths.len(), ..Default::default() };

    for path in &paths {
        match import_one(pool, path, library_path, copy_only).await {
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
    album::group_by_location(pool).await?;

    Ok(summary)
}

/// Returns true = newly imported, false = skipped.
async fn import_one(
    pool: &SqlitePool,
    path: &Path,
    library_path: &Path,
    copy_only: bool,
) -> Result<bool> {
    let sha256 = compute_sha256(path)?;
    let decision = decide(pool, &sha256).await?;

    if matches!(decision, ImportDecision::AlreadyImported) {
        return Ok(false);
    }

    let meta = metadata::extract_from_file(path)?;

    // Three-level date inference: EXIF → filename → None (unknown/)
    let date = meta.taken_at
        .map(|dt| dt.date())
        .or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| metadata::infer_date(n))
                .map(|dt| dt.date())
        });

    let final_path = placer::place(path, library_path, date, copy_only)?;

    let phash = compute_phash(&final_path).ok();

    let result = sqlx::query(
        "INSERT OR IGNORE INTO photos (path, sha256, phash, format, taken_at, gps_lat, gps_lon, camera, import_status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'imported')",
    )
    .bind(final_path.to_string_lossy().as_ref())
    .bind(&sha256)
    .bind(&phash)
    .bind(meta.format.as_str())
    .bind(meta.taken_at.map(|t| t.to_string()))
    .bind(meta.gps_lat)
    .bind(meta.gps_lon)
    .bind(meta.camera)
    .execute(pool)
    .await?;

    if result.rows_affected() > 0 {
        sqlx::query("UPDATE photo_stats SET active_count = active_count + 1 WHERE id = 1")
            .execute(pool)
            .await?;
    }

    let photo_id = result.last_insert_rowid();
    if let Ok(img) = image::open(&final_path) {
        crate::face::analyze_one(pool, photo_id, &img).await;
        crate::animal::detect_and_save(pool, photo_id, &img).await;
    }

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
        let lib = tempdir().unwrap();
        // Use copy_only to avoid destroying shared fixture files.
        let summary = import_dir(&pool, &fixtures_dir(), lib.path(), true).await.unwrap();
        assert!(summary.total > 0);
        assert_eq!(summary.errors, 0);
        assert!(summary.imported > 0);
    }

    #[tokio::test]
    async fn import_moves_file_to_library() {
        let pool = test_pool().await;
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();

        // Copy a fixture into src_dir to avoid destroying the fixture.
        let src = src_dir.path().join("with_exif.jpg");
        fs::copy(fixtures_dir().join("with_exif.jpg"), &src).unwrap();

        let summary = import_dir(&pool, src_dir.path(), lib_dir.path(), false).await.unwrap();
        assert_eq!(summary.imported, 1);
        assert_eq!(summary.errors, 0);

        // Source should be gone.
        assert!(!src.exists(), "source file should be moved");

        // Library should have the file under a date directory.
        let mut found = false;
        for entry in fs::read_dir(lib_dir.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                let sub: Vec<_> = fs::read_dir(entry.path()).unwrap().collect();
                if !sub.is_empty() {
                    found = true;
                }
            }
        }
        assert!(found, "file should appear in a library subdirectory");
    }

    #[tokio::test]
    async fn copy_only_preserves_source() {
        let pool = test_pool().await;
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();

        let src = src_dir.path().join("with_exif.jpg");
        fs::copy(fixtures_dir().join("with_exif.jpg"), &src).unwrap();

        let summary = import_dir(&pool, src_dir.path(), lib_dir.path(), true).await.unwrap();
        assert_eq!(summary.imported, 1);
        assert!(src.exists(), "source should be preserved with copy_only");
    }

    #[tokio::test]
    async fn no_date_goes_to_unknown() {
        let pool = test_pool().await;
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();

        let src = src_dir.path().join("no_exif.jpg");
        fs::copy(fixtures_dir().join("no_exif.jpg"), &src).unwrap();

        let summary = import_dir(&pool, src_dir.path(), lib_dir.path(), false).await.unwrap();
        assert_eq!(summary.imported, 1);

        let unknown_dir = lib_dir.path().join("unknown");
        assert!(unknown_dir.exists(), "unknown/ directory should be created");
        let files: Vec<_> = fs::read_dir(&unknown_dir).unwrap().collect();
        assert!(!files.is_empty(), "file should be in unknown/");
    }

    #[tokio::test]
    async fn second_import_is_idempotent() {
        let pool = test_pool().await;
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();

        // First: copy into src_dir and import (move to lib).
        let src = src_dir.path().join("with_exif.jpg");
        fs::copy(fixtures_dir().join("with_exif.jpg"), &src).unwrap();
        let s1 = import_dir(&pool, src_dir.path(), lib_dir.path(), false).await.unwrap();
        assert_eq!(s1.imported, 1);

        // Second: import the lib itself — same SHA already in DB, should skip.
        let s2 = import_dir(&pool, lib_dir.path(), lib_dir.path(), false).await.unwrap();
        assert_eq!(s2.imported, 0, "re-import of already-imported sha should be skipped");
    }

    #[tokio::test]
    async fn import_empty_dir_returns_zero() {
        let pool = test_pool().await;
        let dir = tempdir().unwrap();
        let lib = tempdir().unwrap();
        let summary = import_dir(&pool, dir.path(), lib.path(), false).await.unwrap();
        assert_eq!(summary.total, 0);
        assert_eq!(summary.imported, 0);
    }

    #[tokio::test]
    async fn import_increments_active_count() {
        let pool = test_pool().await;
        let src_dir = tempdir().unwrap();
        let lib = tempdir().unwrap();

        let src = src_dir.path().join("with_exif.jpg");
        fs::copy(fixtures_dir().join("with_exif.jpg"), &src).unwrap();

        import_dir(&pool, src_dir.path(), lib.path(), false).await.unwrap();

        let count: (i64,) =
            sqlx::query_as("SELECT active_count FROM photo_stats WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn second_import_does_not_double_count() {
        let pool = test_pool().await;
        let src_dir = tempdir().unwrap();
        let lib = tempdir().unwrap();

        let src = src_dir.path().join("with_exif.jpg");
        fs::copy(fixtures_dir().join("with_exif.jpg"), &src).unwrap();
        import_dir(&pool, src_dir.path(), lib.path(), false).await.unwrap();
        // Re-import from the library (same SHA, should skip).
        import_dir(&pool, lib.path(), lib.path(), false).await.unwrap();

        let count: (i64,) =
            sqlx::query_as("SELECT active_count FROM photo_stats WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 1, "re-import must not increment counter again");
    }
}
