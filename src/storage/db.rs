use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use std::str::FromStr;
use crate::error::Result;

pub async fn connect(db_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePool::connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        // 单连接共享内存库，连接关闭前数据持久
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn connect_creates_db_and_runs_migrations() {
        let pool = test_pool().await;
        // migrations が通れば photos テーブルが存在する
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photos")
            .fetch_one(&pool)
            .await
            .expect("photos table should exist");
        assert_eq!(row.0, 0);
    }

    #[tokio::test]
    async fn insert_and_query_photo() {
        let pool = test_pool().await;

        sqlx::query(
            "INSERT INTO photos (path, sha256, format) VALUES (?, ?, ?)",
        )
        .bind("/tmp/test.jpg")
        .bind("abc123")
        .bind("jpeg")
        .execute(&pool)
        .await
        .unwrap();

        let row: (String, String) =
            sqlx::query_as("SELECT path, format FROM photos WHERE sha256 = ?")
                .bind("abc123")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(row.0, "/tmp/test.jpg");
        assert_eq!(row.1, "jpeg");
    }

    #[tokio::test]
    async fn foreign_keys_enforced() {
        let pool = test_pool().await;
        // photo_albums は photos が存在しないと挿入できない
        let result = sqlx::query(
            "INSERT INTO photo_albums (photo_id, album_id) VALUES (999, 999)",
        )
        .execute(&pool)
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn all_tables_exist() {
        let pool = test_pool().await;
        for table in &["photos", "albums", "photo_albums", "dedup_groups", "dedup_members", "import_sessions", "faces", "face_jobs", "assets", "asset_sources", "asset_variants", "asset_links", "migration_runs", "sync_jobs", "sync_items", "provider_checkpoints", "variant_renditions", "asset_display_revisions", "derived_media_state"] {
            let row: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(row.0, 1, "table {table} should exist");
        }
    }

    #[tokio::test]
    async fn asset_catalog_enforces_external_identity_and_lifecycle() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO asset_sources (provider, external_id) VALUES ('apple_photos', 'asset-1')",
        )
        .execute(&pool).await.unwrap();

        let duplicate = sqlx::query(
            "INSERT INTO asset_sources (provider, external_id) VALUES ('apple_photos', 'asset-1')",
        )
        .execute(&pool).await;
        assert!(duplicate.is_err(), "provider source identity must be unique");

        let invalid_status = sqlx::query(
            "INSERT INTO asset_sources (provider, external_id, sync_status) VALUES ('apple_photos', 'asset-2', 'unknown')",
        )
        .execute(&pool).await;
        assert!(invalid_status.is_err(), "unknown lifecycle states must be rejected");
    }

    #[tokio::test]
    async fn asset_catalog_allows_one_primary_variant() {
        let pool = test_pool().await;
        let asset_id: i64 = sqlx::query_scalar("INSERT INTO assets DEFAULT VALUES RETURNING id")
            .fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO asset_variants (asset_id, role, path, is_primary) VALUES (?, 'original', '/original.jpg', 1)",
        )
        .bind(asset_id).execute(&pool).await.unwrap();

        let second_primary = sqlx::query(
            "INSERT INTO asset_variants (asset_id, role, path, is_primary) VALUES (?, 'current', '/current.jpg', 1)",
        )
        .bind(asset_id).execute(&pool).await;
        assert!(second_primary.is_err(), "an asset may only have one primary variant");
    }

    #[tokio::test]
    async fn rendition_schema_separates_master_display_and_orientation_policy() {
        let pool = test_pool().await;
        let asset_id: i64 = sqlx::query_scalar("INSERT INTO assets DEFAULT VALUES RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let original: i64 = sqlx::query_scalar(
            "INSERT INTO asset_variants (asset_id, role, path, is_primary) \
             VALUES (?, 'original', '/original.heic', 1) RETURNING id",
        ).bind(asset_id).fetch_one(&pool).await.unwrap();
        let current: i64 = sqlx::query_scalar(
            "INSERT INTO asset_variants (asset_id, role, path) \
             VALUES (?, 'current', '/current.jpg') RETURNING id",
        ).bind(asset_id).fetch_one(&pool).await.unwrap();
        sqlx::query(
            "UPDATE assets SET master_variant_id = ?, display_variant_id = ? WHERE id = ?",
        ).bind(original).bind(current).bind(asset_id).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO variant_renditions (variant_id, provenance, byte_preserved, \
             orientation_mode, source_orientation, display_orientation) \
             VALUES (?, 'photokit_resource', 1, 'metadata', 6, 6)",
        ).bind(original).execute(&pool).await.unwrap();
        let invalid = sqlx::query(
            "INSERT INTO variant_renditions (variant_id, provenance, orientation_mode, display_orientation) \
             VALUES (?, 'photokit_current', 'baked_pixels', 9)",
        ).bind(current).execute(&pool).await;
        assert!(invalid.is_err());

        let selected: (i64, i64) = sqlx::query_as(
            "SELECT master_variant_id, display_variant_id FROM assets WHERE id = ?",
        ).bind(asset_id).fetch_one(&pool).await.unwrap();
        assert_eq!(selected, (original, current));
        sqlx::query("DELETE FROM asset_variants WHERE id = ?")
            .bind(current).execute(&pool).await.unwrap();
        let display: Option<i64> = sqlx::query_scalar(
            "SELECT display_variant_id FROM assets WHERE id = ?",
        ).bind(asset_id).fetch_one(&pool).await.unwrap();
        assert_eq!(display, None);
    }

    #[tokio::test]
    async fn deleting_asset_cascades_variants_but_preserves_source_inventory() {
        let pool = test_pool().await;
        let asset_id: i64 = sqlx::query_scalar("INSERT INTO assets DEFAULT VALUES RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let source_id: i64 = sqlx::query_scalar(
            "INSERT INTO asset_sources (asset_id, provider, external_id) VALUES (?, 'apple_photos', 'asset-1') RETURNING id",
        )
        .bind(asset_id).fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO asset_variants (asset_id, source_id, role, path) VALUES (?, ?, 'original', '/original.jpg')",
        )
        .bind(asset_id).bind(source_id).execute(&pool).await.unwrap();

        sqlx::query("DELETE FROM assets WHERE id = ?")
            .bind(asset_id).execute(&pool).await.unwrap();

        let variants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM asset_variants")
            .fetch_one(&pool).await.unwrap();
        let source_asset: Option<i64> = sqlx::query_scalar(
            "SELECT asset_id FROM asset_sources WHERE id = ?",
        )
        .bind(source_id).fetch_one(&pool).await.unwrap();
        assert_eq!(variants, 0);
        assert_eq!(source_asset, None, "provider inventory survives unlinking");
    }

    #[tokio::test]
    async fn sync_schema_requires_valid_leases_and_progress() {
        let pool = test_pool().await;
        let job_id: i64 = sqlx::query_scalar(
            "INSERT INTO sync_jobs (kind, provider, total_items) \
             VALUES ('apple_incremental', 'apple_photos', 1) RETURNING id",
        )
        .fetch_one(&pool).await.unwrap();

        let invalid_lease = sqlx::query(
            "INSERT INTO sync_items (job_id, external_id, operation, status) \
             VALUES (?, 'asset-1', 'download', 'leased')",
        )
        .bind(job_id).execute(&pool).await;
        assert!(invalid_lease.is_err(), "leased work requires owner and expiry");

        let invalid_progress = sqlx::query(
            "UPDATE sync_jobs SET completed_items = 2 WHERE id = ?",
        )
        .bind(job_id).execute(&pool).await;
        assert!(invalid_progress.is_err(), "job progress cannot exceed total work");
    }

    #[tokio::test]
    async fn faces_insert_and_query() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO photos (path, sha256, format) VALUES (?, ?, ?)")
            .bind("/tmp/a.jpg").bind("aaa").bind("jpeg")
            .execute(&pool).await.unwrap();
        let photo_id: i64 = sqlx::query_scalar("SELECT id FROM photos WHERE sha256 = 'aaa'")
            .fetch_one(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(photo_id).bind(10).bind(20).bind(100).bind(100).bind(0.97_f32)
        .execute(&pool).await.unwrap();

        let row: (i64, i64, i64, i64, f64) =
            sqlx::query_as("SELECT x, y, width, height, confidence FROM faces WHERE photo_id = ?")
                .bind(photo_id)
                .fetch_one(&pool).await.unwrap();
        assert_eq!((row.0, row.1, row.2, row.3), (10, 20, 100, 100));
        assert!((row.4 - 0.97).abs() < 0.001);
    }

    #[tokio::test]
    async fn faces_embedding_blob_roundtrip() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO photos (path, sha256, format) VALUES (?, ?, ?)")
            .bind("/tmp/b.jpg").bind("bbb").bind("jpeg")
            .execute(&pool).await.unwrap();
        let photo_id: i64 = sqlx::query_scalar("SELECT id FROM photos WHERE sha256 = 'bbb'")
            .fetch_one(&pool).await.unwrap();

        // 512 次元の f32 embedding を BLOB として格納・復元
        let embedding: Vec<f32> = (0..512).map(|i| i as f32 / 512.0).collect();
        let blob: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        sqlx::query(
            "INSERT INTO faces (photo_id, x, y, width, height, embedding, embed_model) VALUES (?, 0, 0, 50, 50, ?, ?)",
        )
        .bind(photo_id).bind(&blob).bind("arcface-mobilenet-v1")
        .execute(&pool).await.unwrap();

        let (stored_blob, model): (Vec<u8>, String) =
            sqlx::query_as("SELECT embedding, embed_model FROM faces WHERE photo_id = ?")
                .bind(photo_id)
                .fetch_one(&pool).await.unwrap();

        assert_eq!(stored_blob.len(), 512 * 4);
        assert_eq!(model, "arcface-mobilenet-v1");
        let restored: Vec<f32> = stored_blob
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(restored.len(), 512);
        assert!((restored[1] - embedding[1]).abs() < 1e-6);
    }

    #[tokio::test]
    async fn faces_foreign_key_enforced() {
        let pool = test_pool().await;
        let result = sqlx::query(
            "INSERT INTO faces (photo_id, x, y, width, height) VALUES (999, 0, 0, 10, 10)",
        )
        .execute(&pool).await;
        assert!(result.is_err(), "faces.photo_id must reference an existing photo");
    }

    #[tokio::test]
    async fn face_jobs_insert_and_query() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO face_jobs (status, scope, total) VALUES (?, ?, ?)",
        )
        .bind("running").bind(serde_json::json!([1, 2, 3]).to_string()).bind(3_i64)
        .execute(&pool).await.unwrap();

        let row: (String, Option<String>, i64, i64) =
            sqlx::query_as("SELECT status, scope, total, processed FROM face_jobs")
                .fetch_one(&pool).await.unwrap();
        assert_eq!(row.0, "running");
        assert_eq!(row.2, 3);
        assert_eq!(row.3, 0);
    }
}
