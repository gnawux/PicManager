pub mod log;
pub mod placer;
pub mod scanner;
pub mod state;

use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use crate::album;
use crate::dedup::hash::compute_phash;
use crate::error::Result;
use crate::metadata;
use log::{LogEntry, LogStatus, MigrationLog};
use state::{ImportDecision, compute_sha256, decide};

#[derive(Debug, Default)]
pub struct ImportSummary {
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
}

#[derive(Debug)]
pub struct BatchResult {
    pub summary: ImportSummary,
    pub total_files: usize,
    pub remaining: usize,
}

#[derive(Default)]
pub struct ImportProgress {
    pub total: AtomicUsize,
    pub processed: AtomicUsize,
    pub imported: AtomicUsize,
    pub skipped: AtomicUsize,
    pub errors: AtomicUsize,
    pub faces_found: AtomicUsize,
    pub gps_found: AtomicUsize,
    pub geo_total: AtomicUsize,
    pub geo_done: AtomicUsize,
}

pub type SharedImportProgress = Arc<ImportProgress>;

pub async fn import_dir(
    pool: &SqlitePool,
    source_dir: &Path,
    library_path: &Path,
    copy_only: bool,
) -> Result<ImportSummary> {
    import_dir_inner(pool, source_dir, library_path, copy_only, None).await
}

pub async fn import_dir_with_progress(
    pool: &SqlitePool,
    source_dir: &Path,
    library_path: &Path,
    copy_only: bool,
    progress: SharedImportProgress,
) -> Result<ImportSummary> {
    import_dir_inner(pool, source_dir, library_path, copy_only, Some(progress)).await
}

pub async fn import_dir_batch(
    pool: &SqlitePool,
    source_dir: &Path,
    library_path: &Path,
    copy_only: bool,
    batch_size: Option<usize>,
    log_path: Option<&Path>,
    dry_run: bool,
    progress: SharedImportProgress,
) -> Result<BatchResult> {
    let migration_log = log_path.map(|p| MigrationLog::open(p.to_path_buf()));
    let done_paths = migration_log.as_ref()
        .and_then(|l| l.load_done_paths().ok())
        .unwrap_or_default();

    let all_files = scanner::scan_dir(source_dir);
    let total_files = all_files.len();

    let mut pending: Vec<PathBuf> = all_files
        .into_iter()
        .filter(|p| !done_paths.contains(p))
        .collect();

    let remaining_before = pending.len();

    if let Some(n) = batch_size {
        pending.truncate(n);
    }

    let batch_len = pending.len();
    let remaining = remaining_before.saturating_sub(batch_len);

    progress.total.store(batch_len, Relaxed);

    if dry_run {
        return Ok(BatchResult {
            summary: ImportSummary { total: batch_len, ..Default::default() },
            total_files,
            remaining: remaining_before,
        });
    }

    let mut summary = ImportSummary { total: batch_len, ..Default::default() };
    let mut newly_imported_ids: Vec<i64> = Vec::new();

    for path in &pending {
        let (outcome, sha256_opt, err_opt) = match import_one(pool, path, library_path, copy_only).await {
            Ok(Some((photo_id, face_count, has_gps))) => {
                summary.imported += 1;
                newly_imported_ids.push(photo_id);
                progress.imported.fetch_add(1, Relaxed);
                progress.processed.fetch_add(1, Relaxed);
                progress.faces_found.fetch_add(face_count, Relaxed);
                if has_gps { progress.gps_found.fetch_add(1, Relaxed); }
                (LogStatus::Imported, None, None)
            }
            Ok(None) => {
                summary.skipped += 1;
                progress.skipped.fetch_add(1, Relaxed);
                progress.processed.fetch_add(1, Relaxed);
                (LogStatus::Skipped, None, None)
            }
            Err(e) => {
                let msg = e.to_string();
                tracing::warn!("failed to import {}: {msg}", path.display());
                summary.errors += 1;
                progress.errors.fetch_add(1, Relaxed);
                progress.processed.fetch_add(1, Relaxed);
                (LogStatus::Failed, None, Some(msg))
            }
        };

        if let Some(ml) = &migration_log {
            let entry = LogEntry {
                path: path.to_string_lossy().into_owned(),
                status: outcome,
                sha256: sha256_opt,
                error: err_opt,
                ts: log::now_ts(),
            };
            if let Err(e) = ml.append(&entry) {
                tracing::warn!("failed to write migration log: {e}");
            }
        }
    }

    album::group_by_month(pool).await?;
    album::group_by_camera(pool).await?;

    if !newly_imported_ids.is_empty() {
        let dummy_total = AtomicUsize::new(0);
        let dummy_done = AtomicUsize::new(0);
        let (geo_total, geo_done) = (&dummy_total, &dummy_done);
        album::group_by_location_scoped(pool, &newly_imported_ids, geo_total, geo_done).await?;

        if let Err(e) = crate::face::cluster::run_incremental_clustering(pool).await {
            tracing::warn!("incremental clustering failed: {e}");
        }
    }

    Ok(BatchResult { summary, total_files, remaining })
}

async fn import_dir_inner(
    pool: &SqlitePool,
    source_dir: &Path,
    library_path: &Path,
    copy_only: bool,
    progress: Option<SharedImportProgress>,
) -> Result<ImportSummary> {
    let paths = scanner::scan_dir(source_dir);
    let total = paths.len();
    if let Some(p) = &progress {
        p.total.store(total, Relaxed);
    }
    let mut summary = ImportSummary { total, ..Default::default() };
    let mut newly_imported_ids: Vec<i64> = Vec::new();

    for path in &paths {
        match import_one(pool, path, library_path, copy_only).await {
            Ok(Some((photo_id, face_count, has_gps))) => {
                summary.imported += 1;
                newly_imported_ids.push(photo_id);
                if let Some(p) = &progress {
                    p.imported.fetch_add(1, Relaxed);
                    p.processed.fetch_add(1, Relaxed);
                    p.faces_found.fetch_add(face_count, Relaxed);
                    if has_gps { p.gps_found.fetch_add(1, Relaxed); }
                }
            }
            Ok(None) => {
                summary.skipped += 1;
                if let Some(p) = &progress {
                    p.skipped.fetch_add(1, Relaxed);
                    p.processed.fetch_add(1, Relaxed);
                }
            }
            Err(e) => {
                tracing::warn!("failed to import {}: {e}", path.display());
                summary.errors += 1;
                if let Some(p) = &progress {
                    p.errors.fetch_add(1, Relaxed);
                    p.processed.fetch_add(1, Relaxed);
                }
            }
        }
    }

    album::group_by_month(pool).await?;
    album::group_by_camera(pool).await?;

    // Only geocode photos imported in this run, not the whole library.
    if !newly_imported_ids.is_empty() {
        let dummy_total = AtomicUsize::new(0);
        let dummy_done = AtomicUsize::new(0);
        let (geo_total, geo_done) = progress.as_ref()
            .map(|p| (&p.geo_total, &p.geo_done))
            .unwrap_or((&dummy_total, &dummy_done));
        album::group_by_location_scoped(pool, &newly_imported_ids, geo_total, geo_done).await?;

        // Assign newly detected faces to existing people or create new person clusters.
        if let Err(e) = crate::face::cluster::run_incremental_clustering(pool).await {
            tracing::warn!("incremental clustering failed: {e}");
        }
    }

    Ok(summary)
}

/// Returns `Some((photo_id, face_count, has_gps))` if newly imported, `None` if skipped.
async fn import_one(
    pool: &SqlitePool,
    path: &Path,
    library_path: &Path,
    copy_only: bool,
) -> Result<Option<(i64, usize, bool)>> {
    let sha256 = compute_sha256(path)?;
    let decision = decide(pool, &sha256).await?;

    if matches!(decision, ImportDecision::AlreadyImported) {
        return Ok(None);
    }

    let meta = metadata::extract_from_file(path)?;

    // Four-level date inference: EXIF → file mtime → filename → None (unknown/)
    // mtime is set by photobridge to asset.creationDate; reliable for photos without EXIF.
    let effective_taken_at = meta.taken_at
        .or_else(|| metadata::mtime_to_naive_datetime(path));

    let date = effective_taken_at
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
        "INSERT OR IGNORE INTO photos (path, sha256, phash, format, taken_at, gps_lat, gps_lon, camera, import_status, exif_orientation)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'imported', ?)",
    )
    .bind(final_path.to_string_lossy().as_ref())
    .bind(&sha256)
    .bind(&phash)
    .bind(meta.format.as_str())
    .bind(effective_taken_at.map(|t| t.to_string()))
    .bind(meta.gps_lat)
    .bind(meta.gps_lon)
    .bind(meta.camera)
    .bind(meta.exif_orientation as i32)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    sqlx::query("UPDATE photo_stats SET active_count = active_count + 1 WHERE id = 1")
        .execute(pool)
        .await?;

    let photo_id = result.last_insert_rowid();
    let mut face_count = 0usize;
    if let Ok(img) = crate::image_open::open_image(final_path.as_ref()) {
        sqlx::query("UPDATE photos SET width = ?, height = ? WHERE id = ?")
            .bind(img.width() as i64)
            .bind(img.height() as i64)
            .bind(photo_id)
            .execute(pool)
            .await
            .ok();
        face_count = crate::face::analyze_one(pool, photo_id, &img).await;
    }

    Ok(Some((photo_id, face_count, meta.gps_lat.is_some())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::fs;
    use std::sync::atomic::Ordering::Relaxed;
    use tempfile::tempdir;
    use filetime;

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
    async fn test_progress_counters_match_summary() {
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let progress = SharedImportProgress::default();
        let summary = import_dir_with_progress(
            &pool, &fixtures_dir(), lib.path(), true, progress.clone(),
        ).await.unwrap();
        assert_eq!(progress.total.load(Relaxed), summary.total);
        assert_eq!(progress.imported.load(Relaxed), summary.imported);
        assert_eq!(progress.skipped.load(Relaxed), summary.skipped);
        assert_eq!(progress.errors.load(Relaxed), summary.errors);
        assert_eq!(
            progress.processed.load(Relaxed),
            summary.imported + summary.skipped + summary.errors,
        );
    }

    #[tokio::test]
    async fn test_progress_total_is_nonzero_for_nonempty_dir() {
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let progress = SharedImportProgress::default();
        import_dir_with_progress(
            &pool, &fixtures_dir(), lib.path(), true, progress.clone(),
        ).await.unwrap();
        assert!(progress.total.load(Relaxed) > 0);
    }

    #[tokio::test]
    async fn test_faces_found_is_zero_for_fixture_images() {
        // fixtures are tiny synthetic JPEGs with no real faces
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let progress = SharedImportProgress::default();
        import_dir_with_progress(
            &pool, &fixtures_dir(), lib.path(), true, progress.clone(),
        ).await.unwrap();
        // Face detector is disabled in test mode (cfg!(test) guard in detector),
        // so faces_found should be 0 regardless.
        assert_eq!(progress.faces_found.load(Relaxed), 0);
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
    async fn no_exif_uses_mtime_for_placement() {
        let pool = test_pool().await;
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();

        let src = src_dir.path().join("no_exif.jpg");
        fs::copy(fixtures_dir().join("no_exif.jpg"), &src).unwrap();
        // Set known mtime: 2024-06-01 00:00:00 UTC
        let known = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_717_200_000);
        filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(known)).unwrap();

        let summary = import_dir(&pool, src_dir.path(), lib_dir.path(), false).await.unwrap();
        assert_eq!(summary.imported, 1);

        let dated_dir = lib_dir.path().join("2024-06-01");
        assert!(dated_dir.exists(), "file should be placed in mtime-derived dated directory");
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

    // ── Step 31b: import_dir_batch tests ───────────────────────────────────

    #[tokio::test]
    async fn batch_imports_only_n_files() {
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let progress = SharedImportProgress::default();
        // fixtures dir has 7 files; batch_size=3 → import at most 3
        let result = import_dir_batch(
            &pool, &fixtures_dir(), lib.path(), true,
            Some(3), None, false, progress,
        ).await.unwrap();
        assert_eq!(result.summary.imported, 3);
        assert_eq!(result.total_files, 7);
        assert_eq!(result.remaining, result.total_files - 3 - result.summary.skipped - result.summary.errors);
    }

    #[tokio::test]
    async fn batch_remaining_zero_when_all_fit() {
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let progress = SharedImportProgress::default();
        let result = import_dir_batch(
            &pool, &fixtures_dir(), lib.path(), true,
            Some(100), None, false, progress,
        ).await.unwrap();
        assert_eq!(result.remaining, 0);
    }

    #[tokio::test]
    async fn batch_skips_logged_files() {
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let log_dir = tempdir().unwrap();
        let log_path = log_dir.path().join("import.log");

        // Scan directory first to know actual file paths.
        let all_files = scanner::scan_dir(&fixtures_dir());
        assert!(all_files.len() >= 3, "need at least 3 fixture files");

        // Pre-log first 2 files as already imported.
        let ml = MigrationLog::open(log_path.clone());
        for path in &all_files[..2] {
            ml.append(&LogEntry {
                path: path.to_string_lossy().into_owned(),
                status: LogStatus::Imported,
                sha256: None,
                error: None,
                ts: "2026-01-01T00:00:00Z".to_owned(),
            }).unwrap();
        }

        let progress = SharedImportProgress::default();
        let result = import_dir_batch(
            &pool, &fixtures_dir(), lib.path(), true,
            Some(2), Some(&log_path), false, progress,
        ).await.unwrap();

        // Only up to 2 files from the remaining (7-2=5) should be imported.
        assert!(result.summary.imported <= 2);
        assert_eq!(result.total_files, all_files.len());
    }

    #[tokio::test]
    async fn batch_dry_run_no_import() {
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let progress = SharedImportProgress::default();
        let result = import_dir_batch(
            &pool, &fixtures_dir(), lib.path(), true,
            None, None, true, progress,
        ).await.unwrap();

        assert_eq!(result.summary.imported, 0);
        assert_eq!(result.summary.errors, 0);
        // No photos in DB.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photos")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn batch_writes_log_entries() {
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let log_dir = tempdir().unwrap();
        let log_path = log_dir.path().join("import.log");
        let progress = SharedImportProgress::default();

        let result = import_dir_batch(
            &pool, &fixtures_dir(), lib.path(), true,
            Some(3), Some(&log_path), false, progress,
        ).await.unwrap();

        assert!(log_path.exists(), "log file should be created");
        let content = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<_> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), result.summary.imported + result.summary.skipped + result.summary.errors);
    }

    #[tokio::test]
    async fn batch_no_log_works_without_file() {
        let pool = test_pool().await;
        let lib = tempdir().unwrap();
        let progress = SharedImportProgress::default();
        let result = import_dir_batch(
            &pool, &fixtures_dir(), lib.path(), true,
            Some(2), None, false, progress,
        ).await.unwrap();
        assert!(result.summary.imported > 0 || result.summary.skipped > 0);
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

    #[tokio::test]
    async fn no_exif_uses_mtime_as_taken_at() {
        let pool = test_pool().await;
        let src_dir = tempdir().unwrap();
        let lib_dir = tempdir().unwrap();

        let src = src_dir.path().join("no_exif.jpg");
        fs::copy(fixtures_dir().join("no_exif.jpg"), &src).unwrap();

        // Set mtime to 2023-03-15 00:00:00 UTC
        let known_secs: i64 = 1_678_838_400;
        let known_ft = filetime::FileTime::from_unix_time(known_secs, 0);
        filetime::set_file_mtime(&src, known_ft).unwrap();

        let summary = import_dir(&pool, src_dir.path(), lib_dir.path(), false).await.unwrap();
        assert_eq!(summary.imported, 1);

        // File should be placed in dated directory, not unknown/
        let dated_dir = lib_dir.path().join("2023-03-15");
        assert!(dated_dir.exists(), "should be placed in 2023-03-15/, not unknown/");

        // DB taken_at should reflect the mtime
        let row: (Option<String>,) =
            sqlx::query_as("SELECT taken_at FROM photos LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        let taken_at = row.0.expect("taken_at should be set from mtime");
        assert!(taken_at.starts_with("2023-03-15"), "taken_at should be 2023-03-15, got {taken_at}");
    }
}
