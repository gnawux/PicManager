use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use std::path::Path;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BackfillReport {
    pub dry_run: bool,
    pub migration_run_id: Option<i64>,
    pub total_photos: i64,
    pub existing_assets: i64,
    pub existing_sources: i64,
    pub existing_variants: i64,
    pub created_assets: u64,
    pub created_sources: u64,
    pub created_variants: u64,
    pub created_links: u64,
}

#[derive(Debug, FromRow)]
struct LegacyPhoto {
    id: i64,
    path: String,
    sha256: String,
    format: String,
    taken_at: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
}

pub async fn backfill_legacy_local(pool: &SqlitePool, dry_run: bool) -> Result<BackfillReport> {
    let baseline = baseline_report(pool, dry_run).await?;
    if dry_run {
        return Ok(baseline);
    }

    let run_id: i64 = sqlx::query_scalar(
        "INSERT INTO migration_runs (kind, status, dry_run, started_at) \
         VALUES ('backfill_legacy_local', 'running', 0, datetime('now')) RETURNING id",
    )
    .fetch_one(pool)
    .await?;

    match execute_backfill(pool, run_id, baseline).await {
        Ok(report) => Ok(report),
        Err(error) => {
            let _ = sqlx::query(
                "UPDATE migration_runs SET status = 'failed', error = ?, \
                 finished_at = datetime('now') WHERE id = ?",
            )
            .bind(error.to_string())
            .bind(run_id)
            .execute(pool)
            .await;
            Err(error)
        }
    }
}

async fn baseline_report(pool: &SqlitePool, dry_run: bool) -> Result<BackfillReport> {
    let total_photos: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photos")
        .fetch_one(pool).await?;
    let existing_assets: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM assets WHERE photo_id IS NOT NULL",
    )
    .fetch_one(pool).await?;
    let existing_sources: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM asset_sources WHERE provider = 'legacy_local'",
    )
    .fetch_one(pool).await?;
    let existing_variants: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM asset_variants WHERE role = 'imported'",
    )
    .fetch_one(pool).await?;

    Ok(BackfillReport {
        dry_run,
        migration_run_id: None,
        total_photos,
        existing_assets,
        existing_sources,
        existing_variants,
        created_assets: 0,
        created_sources: 0,
        created_variants: 0,
        created_links: 0,
    })
}

async fn execute_backfill(
    pool: &SqlitePool,
    run_id: i64,
    mut report: BackfillReport,
) -> Result<BackfillReport> {
    let photos: Vec<LegacyPhoto> = sqlx::query_as(
        "SELECT id, path, sha256, format, taken_at, width, height FROM photos ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    let mut tx = pool.begin().await?;

    for photo in photos {
        let inserted_asset = sqlx::query("INSERT OR IGNORE INTO assets (photo_id) VALUES (?)")
            .bind(photo.id)
            .execute(&mut *tx)
            .await?;
        report.created_assets += inserted_asset.rows_affected();
        let asset_id: i64 = sqlx::query_scalar("SELECT id FROM assets WHERE photo_id = ?")
            .bind(photo.id)
            .fetch_one(&mut *tx)
            .await?;

        let external_id = format!("photo:{}", photo.id);
        let filename = Path::new(&photo.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&photo.path);
        let inserted_source = sqlx::query(
            "INSERT OR IGNORE INTO asset_sources (\
                 asset_id, provider, external_id, original_filename, media_type, width, \
                 height, taken_at, sync_status, last_seen_at\
             ) VALUES (?, 'legacy_local', ?, ?, ?, ?, ?, ?, 'ready', datetime('now'))",
        )
        .bind(asset_id)
        .bind(&external_id)
        .bind(filename)
        .bind(mime_for_format(&photo.format))
        .bind(photo.width)
        .bind(photo.height)
        .bind(&photo.taken_at)
        .execute(&mut *tx)
        .await?;
        report.created_sources += inserted_source.rows_affected();
        sqlx::query(
            "UPDATE asset_sources SET asset_id = ?, \
             original_filename = COALESCE(original_filename, ?), \
             media_type = COALESCE(media_type, ?), width = COALESCE(width, ?), \
             height = COALESCE(height, ?), taken_at = COALESCE(taken_at, ?), \
             sync_status = 'ready', updated_at = datetime('now') \
             WHERE provider = 'legacy_local' AND external_id = ?",
        )
        .bind(asset_id)
        .bind(filename)
        .bind(mime_for_format(&photo.format))
        .bind(photo.width)
        .bind(photo.height)
        .bind(&photo.taken_at)
        .bind(&external_id)
        .execute(&mut *tx)
        .await?;
        let source_id: i64 = sqlx::query_scalar(
            "SELECT id FROM asset_sources WHERE provider = 'legacy_local' AND external_id = ?",
        )
        .bind(&external_id)
        .fetch_one(&mut *tx)
        .await?;

        let inserted_variant = sqlx::query(
            "INSERT OR IGNORE INTO asset_variants (\
                 asset_id, source_id, role, path, content_sha256, mime_type, width, height\
             ) VALUES (?, ?, 'imported', ?, ?, ?, ?, ?)",
        )
        .bind(asset_id)
        .bind(source_id)
        .bind(&photo.path)
        .bind(&photo.sha256)
        .bind(mime_for_format(&photo.format))
        .bind(photo.width)
        .bind(photo.height)
        .execute(&mut *tx)
        .await?;
        report.created_variants += inserted_variant.rows_affected();
        let variant_id: i64 = sqlx::query_scalar(
            "SELECT id FROM asset_variants \
             WHERE asset_id = ? AND role = 'imported' AND source_id = ? AND generation_key IS NULL",
        )
        .bind(asset_id)
        .bind(source_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE asset_variants SET path = ?, content_sha256 = ?, \
             mime_type = COALESCE(mime_type, ?), width = COALESCE(width, ?), \
             height = COALESCE(height, ?), updated_at = datetime('now') WHERE id = ?",
        )
        .bind(&photo.path)
        .bind(&photo.sha256)
        .bind(mime_for_format(&photo.format))
        .bind(photo.width)
        .bind(photo.height)
        .bind(variant_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE asset_variants SET is_primary = 1 \
             WHERE id = ? AND NOT EXISTS (\
                 SELECT 1 FROM asset_variants WHERE asset_id = ? AND is_primary = 1\
             )",
        )
        .bind(variant_id)
        .bind(asset_id)
        .execute(&mut *tx)
        .await?;

        let inserted_link = sqlx::query(
            "INSERT OR IGNORE INTO asset_links (\
                 source_id, photo_id, method, confidence, status, evidence_json, reviewed_at\
             ) VALUES (?, ?, 'legacy_photo_id', 1.0, 'accepted', ?, datetime('now'))",
        )
        .bind(source_id)
        .bind(photo.id)
        .bind(format!("{{\"photo_id\":{}}}", photo.id))
        .execute(&mut *tx)
        .await?;
        report.created_links += inserted_link.rows_affected();
    }

    report.migration_run_id = Some(run_id);
    let summary = serde_json::to_string(&report).unwrap_or_else(|_| "{}".to_owned());
    sqlx::query(
        "UPDATE migration_runs SET status = 'completed', summary_json = ?, \
         finished_at = datetime('now') WHERE id = ?",
    )
    .bind(summary)
    .bind(run_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(report)
}

fn mime_for_format(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "tiff" | "tif" => "image/tiff",
        "arw" => "image/x-sony-arw",
        _ => "application/octet-stream",
    }
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

    async fn seed_legacy_photo(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, taken_at, width, height, import_status) \
             VALUES (42, '/library/2026-08-15/UUID.heic', 'abc123', 'heic', \
                     '2026-08-15 10:00:00', 4032, 3024, 'imported')",
        )
        .execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO albums (id, name, kind) VALUES (7, 'Existing', 'manual')",
        )
        .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (42, 7)")
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn dry_run_does_not_write_catalog_or_audit() {
        let pool = test_pool().await;
        seed_legacy_photo(&pool).await;
        let report = backfill_legacy_local(&pool, true).await.unwrap();

        assert!(report.dry_run);
        assert_eq!(report.total_photos, 1);
        for table in ["assets", "asset_sources", "asset_variants", "migration_runs"] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool).await.unwrap();
            assert_eq!(count, 0, "dry-run wrote {table}");
        }
    }

    #[tokio::test]
    async fn backfill_preserves_photo_and_relationships() {
        let pool = test_pool().await;
        seed_legacy_photo(&pool).await;
        let report = backfill_legacy_local(&pool, false).await.unwrap();

        assert_eq!(report.created_assets, 1);
        assert_eq!(report.created_sources, 1);
        assert_eq!(report.created_variants, 1);
        assert_eq!(report.created_links, 1);
        let photo_id: i64 = sqlx::query_scalar("SELECT photo_id FROM assets")
            .fetch_one(&pool).await.unwrap();
        let filename: String = sqlx::query_scalar("SELECT original_filename FROM asset_sources")
            .fetch_one(&pool).await.unwrap();
        let path: String = sqlx::query_scalar("SELECT path FROM asset_variants")
            .fetch_one(&pool).await.unwrap();
        let memberships: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM photo_albums WHERE photo_id = 42 AND album_id = 7",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(photo_id, 42);
        assert_eq!(filename, "UUID.heic");
        assert_eq!(path, "/library/2026-08-15/UUID.heic");
        assert_eq!(memberships, 1);
    }

    #[tokio::test]
    async fn backfill_is_idempotent_and_records_completed_runs() {
        let pool = test_pool().await;
        seed_legacy_photo(&pool).await;
        backfill_legacy_local(&pool, false).await.unwrap();
        let second = backfill_legacy_local(&pool, false).await.unwrap();

        assert_eq!(second.created_assets, 0);
        assert_eq!(second.created_sources, 0);
        assert_eq!(second.created_variants, 0);
        assert_eq!(second.created_links, 0);
        let completed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM migration_runs WHERE kind = 'backfill_legacy_local' AND status = 'completed'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(completed, 2);
    }

    #[tokio::test]
    async fn failed_backfill_is_rolled_back_and_audited() {
        let pool = test_pool().await;
        seed_legacy_photo(&pool).await;
        let other_asset: i64 = sqlx::query_scalar("INSERT INTO assets DEFAULT VALUES RETURNING id")
            .fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO asset_variants (asset_id, role, path) \
             VALUES (?, 'original', '/library/2026-08-15/UUID.heic')",
        )
        .bind(other_asset).execute(&pool).await.unwrap();

        let result = backfill_legacy_local(&pool, false).await;
        assert!(result.is_err());
        let linked_asset: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM assets WHERE photo_id = 42",
        )
        .fetch_optional(&pool).await.unwrap();
        let failed: (String, Option<String>) = sqlx::query_as(
            "SELECT status, error FROM migration_runs WHERE kind = 'backfill_legacy_local'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(linked_asset, None, "catalog writes must roll back together");
        assert_eq!(failed.0, "failed");
        assert!(failed.1.is_some());
    }

    #[test]
    fn maps_known_formats_without_guessing_from_extension() {
        assert_eq!(mime_for_format("heic"), "image/heic");
        assert_eq!(mime_for_format("jpeg"), "image/jpeg");
        assert_eq!(mime_for_format("arw"), "image/x-sony-arw");
    }
}
