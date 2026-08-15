use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::error::Result;
use crate::migration::{inspect, IntegrityReport};

#[derive(Debug, Clone, Serialize)]
pub struct CatalogVerificationReport {
    pub integrity: IntegrityReport,
    pub total_assets: i64,
    pub linked_assets: i64,
    pub photos_without_assets: i64,
    pub assets_without_photos: i64,
    pub legacy_sources: i64,
    pub photos_without_legacy_sources: i64,
    pub legacy_sources_without_assets: i64,
    pub total_variants: i64,
    pub assets_without_primary_variants: i64,
    pub conflicting_links: i64,
}

impl CatalogVerificationReport {
    pub fn is_healthy(&self) -> bool {
        self.integrity.is_healthy()
            && self.photos_without_assets == 0
            && self.assets_without_photos == 0
            && self.photos_without_legacy_sources == 0
            && self.legacy_sources_without_assets == 0
            && self.assets_without_primary_variants == 0
            && self.conflicting_links == 0
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MigrationRun {
    pub id: i64,
    pub kind: String,
    pub status: String,
    pub dry_run: bool,
    pub checkpoint: Option<String>,
    pub summary_json: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
}

pub async fn verify_catalog(
    pool: &SqlitePool,
    check_files: bool,
) -> Result<CatalogVerificationReport> {
    let integrity = inspect(pool, check_files).await?;
    let total_assets = scalar(pool, "SELECT COUNT(*) FROM assets").await?;
    let linked_assets = scalar(pool, "SELECT COUNT(*) FROM assets WHERE photo_id IS NOT NULL").await?;
    let photos_without_assets = scalar(
        pool,
        "SELECT COUNT(*) FROM photos p LEFT JOIN assets a ON a.photo_id = p.id WHERE a.id IS NULL",
    )
    .await?;
    let assets_without_photos = scalar(
        pool,
        "SELECT COUNT(*) FROM assets a LEFT JOIN photos p ON p.id = a.photo_id WHERE p.id IS NULL",
    )
    .await?;
    let legacy_sources = scalar(
        pool,
        "SELECT COUNT(*) FROM asset_sources WHERE provider = 'legacy_local'",
    )
    .await?;
    let photos_without_legacy_sources = scalar(
        pool,
        "SELECT COUNT(*) FROM photos p \
         LEFT JOIN assets a ON a.photo_id = p.id \
         LEFT JOIN asset_sources s ON s.asset_id = a.id AND s.provider = 'legacy_local' \
         WHERE s.id IS NULL",
    )
    .await?;
    let legacy_sources_without_assets = scalar(
        pool,
        "SELECT COUNT(*) FROM asset_sources s \
         LEFT JOIN assets a ON a.id = s.asset_id \
         WHERE s.provider = 'legacy_local' AND a.id IS NULL",
    )
    .await?;
    let total_variants = scalar(pool, "SELECT COUNT(*) FROM asset_variants").await?;
    let assets_without_primary_variants = scalar(
        pool,
        "SELECT COUNT(*) FROM assets a \
         LEFT JOIN asset_variants v ON v.asset_id = a.id AND v.is_primary = 1 \
         WHERE v.id IS NULL",
    )
    .await?;
    let conflicting_links = scalar(
        pool,
        "SELECT COUNT(*) FROM asset_links WHERE status = 'conflict'",
    )
    .await?;

    Ok(CatalogVerificationReport {
        integrity,
        total_assets,
        linked_assets,
        photos_without_assets,
        assets_without_photos,
        legacy_sources,
        photos_without_legacy_sources,
        legacy_sources_without_assets,
        total_variants,
        assets_without_primary_variants,
        conflicting_links,
    })
}

pub async fn list_migration_runs(pool: &SqlitePool, limit: u32) -> Result<Vec<MigrationRun>> {
    let limit = limit.clamp(1, 1000) as i64;
    Ok(sqlx::query_as(
        "SELECT id, kind, status, dry_run, checkpoint, summary_json, error, \
         started_at, finished_at, created_at \
         FROM migration_runs ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

async fn scalar(pool: &SqlitePool, sql: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(sql).fetch_one(pool).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::backfill_legacy_local;
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

    async fn seed_existing_file(pool: &SqlitePool) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let path = dir.path().join("photo.jpg");
        std::fs::write(&path, b"photo").unwrap();
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) \
             VALUES (1, ?, 'sha-1', 'jpeg', 'imported')",
        )
        .bind(path.to_string_lossy().as_ref())
        .execute(pool).await.unwrap();
        sqlx::query("UPDATE photo_stats SET active_count = 1 WHERE id = 1")
            .execute(pool).await.unwrap();
        dir
    }

    #[tokio::test]
    async fn catalog_is_unhealthy_before_legacy_backfill() {
        let pool = test_pool().await;
        let _dir = seed_existing_file(&pool).await;
        let report = verify_catalog(&pool, true).await.unwrap();

        assert!(!report.is_healthy());
        assert_eq!(report.photos_without_assets, 1);
        assert_eq!(report.photos_without_legacy_sources, 1);
    }

    #[tokio::test]
    async fn catalog_is_healthy_after_legacy_backfill() {
        let pool = test_pool().await;
        let _dir = seed_existing_file(&pool).await;
        backfill_legacy_local(&pool, false).await.unwrap();
        let report = verify_catalog(&pool, true).await.unwrap();

        assert!(report.is_healthy());
        assert_eq!(report.total_assets, 1);
        assert_eq!(report.linked_assets, 1);
        assert_eq!(report.legacy_sources, 1);
        assert_eq!(report.total_variants, 1);
    }

    #[tokio::test]
    async fn migration_report_returns_newest_runs_first() {
        let pool = test_pool().await;
        for kind in ["first", "second"] {
            sqlx::query(
                "INSERT INTO migration_runs (kind, status, dry_run) VALUES (?, 'completed', 0)",
            )
            .bind(kind).execute(&pool).await.unwrap();
        }

        let runs = list_migration_runs(&pool, 1).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].kind, "second");
    }
}
