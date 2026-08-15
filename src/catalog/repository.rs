use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::catalog::types::{AssetSource, AssetVariant, SourceProvider, SourceStatus, VariantRole};
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct SourceInput<'a> {
    pub provider: SourceProvider,
    pub external_id: &'a str,
    pub original_filename: Option<&'a str>,
    pub media_type: Option<&'a str>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub taken_at: Option<&'a str>,
    pub metadata_json: Option<&'a str>,
    pub status: SourceStatus,
    pub last_seen_at: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct VariantInput<'a> {
    pub asset_id: i64,
    pub source_id: Option<i64>,
    pub role: VariantRole,
    pub path: Option<&'a str>,
    pub content_sha256: Option<&'a str>,
    pub mime_type: Option<&'a str>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub byte_size: Option<i64>,
    pub generation_key: Option<&'a str>,
    pub is_primary: bool,
}

pub async fn ensure_asset_for_photo(pool: &SqlitePool, photo_id: i64) -> Result<i64> {
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT OR IGNORE INTO assets (photo_id) VALUES (?)")
        .bind(photo_id)
        .execute(&mut *tx)
        .await?;
    let asset_id = sqlx::query_scalar("SELECT id FROM assets WHERE photo_id = ?")
        .bind(photo_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(asset_id)
}

pub async fn upsert_external_source(
    pool: &SqlitePool,
    input: &SourceInput<'_>,
) -> Result<AssetSource> {
    sqlx::query(
        "INSERT INTO asset_sources (\
             provider, external_id, original_filename, media_type, width, height, \
             taken_at, metadata_json, sync_status, last_seen_at\
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(provider, external_id) WHERE external_id IS NOT NULL DO UPDATE SET \
             original_filename = COALESCE(excluded.original_filename, asset_sources.original_filename), \
             media_type = COALESCE(excluded.media_type, asset_sources.media_type), \
             width = COALESCE(excluded.width, asset_sources.width), \
             height = COALESCE(excluded.height, asset_sources.height), \
             taken_at = COALESCE(excluded.taken_at, asset_sources.taken_at), \
             metadata_json = COALESCE(excluded.metadata_json, asset_sources.metadata_json), \
             last_seen_at = COALESCE(excluded.last_seen_at, asset_sources.last_seen_at), \
             updated_at = datetime('now')",
    )
    .bind(input.provider.as_str())
    .bind(input.external_id)
    .bind(input.original_filename)
    .bind(input.media_type)
    .bind(input.width)
    .bind(input.height)
    .bind(input.taken_at)
    .bind(input.metadata_json)
    .bind(input.status.as_str())
    .bind(input.last_seen_at)
    .execute(pool)
    .await?;

    Ok(sqlx::query_as(
        "SELECT * FROM asset_sources WHERE provider = ? AND external_id = ?",
    )
    .bind(input.provider.as_str())
    .bind(input.external_id)
    .fetch_one(pool)
    .await?)
}

pub async fn find_source(
    pool: &SqlitePool,
    provider: SourceProvider,
    external_id: &str,
) -> Result<Option<AssetSource>> {
    Ok(sqlx::query_as(
        "SELECT * FROM asset_sources WHERE provider = ? AND external_id = ?",
    )
    .bind(provider.as_str())
    .bind(external_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn link_source_to_asset(
    pool: &SqlitePool,
    source_id: i64,
    asset_id: i64,
) -> Result<()> {
    let result = sqlx::query(
        "UPDATE asset_sources SET asset_id = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(asset_id)
    .bind(source_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound.into());
    }
    Ok(())
}

pub async fn add_variant(pool: &SqlitePool, input: &VariantInput<'_>) -> Result<AssetVariant> {
    let mut tx = pool.begin().await?;
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM asset_variants \
         WHERE asset_id = ? AND role = ? \
           AND COALESCE(source_id, -1) = COALESCE(?, -1) \
           AND COALESCE(generation_key, '') = COALESCE(?, '')",
    )
    .bind(input.asset_id)
    .bind(input.role.as_str())
    .bind(input.source_id)
    .bind(input.generation_key)
    .fetch_optional(&mut *tx)
    .await?;

    let variant_id = match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE asset_variants SET path = ?, content_sha256 = ?, mime_type = ?, \
                 width = ?, height = ?, byte_size = ?, updated_at = datetime('now') WHERE id = ?",
            )
            .bind(input.path)
            .bind(input.content_sha256)
            .bind(input.mime_type)
            .bind(input.width)
            .bind(input.height)
            .bind(input.byte_size)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            id
        }
        None => {
            sqlx::query_scalar(
                "INSERT INTO asset_variants (\
                     asset_id, source_id, role, path, content_sha256, mime_type, width, \
                     height, byte_size, generation_key, is_primary\
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0) RETURNING id",
            )
            .bind(input.asset_id)
            .bind(input.source_id)
            .bind(input.role.as_str())
            .bind(input.path)
            .bind(input.content_sha256)
            .bind(input.mime_type)
            .bind(input.width)
            .bind(input.height)
            .bind(input.byte_size)
            .bind(input.generation_key)
            .fetch_one(&mut *tx)
            .await?
        }
    };

    if input.is_primary {
        set_primary_in_transaction(&mut tx, input.asset_id, variant_id).await?;
    }
    let variant = sqlx::query_as("SELECT * FROM asset_variants WHERE id = ?")
        .bind(variant_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(variant)
}

pub async fn set_primary_variant(
    pool: &SqlitePool,
    asset_id: i64,
    variant_id: i64,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    set_primary_in_transaction(&mut tx, asset_id, variant_id).await?;
    tx.commit().await?;
    Ok(())
}

async fn set_primary_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    asset_id: i64,
    variant_id: i64,
) -> Result<()> {
    let belongs: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM asset_variants WHERE id = ? AND asset_id = ?",
    )
    .bind(variant_id)
    .bind(asset_id)
    .fetch_optional(&mut **tx)
    .await?;
    if belongs.is_none() {
        return Err(sqlx::Error::RowNotFound.into());
    }
    sqlx::query("UPDATE asset_variants SET is_primary = 0 WHERE asset_id = ?")
        .bind(asset_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE asset_variants SET is_primary = 1 WHERE id = ?")
        .bind(variant_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
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

    async fn insert_photo(pool: &SqlitePool, id: i64) {
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) VALUES (?, ?, ?, 'jpeg', 'imported')",
        )
        .bind(id)
        .bind(format!("/{id}.jpg"))
        .bind(format!("sha-{id}"))
        .execute(pool)
        .await
        .unwrap();
    }

    fn apple_source<'a>(external_id: &'a str, filename: &'a str) -> SourceInput<'a> {
        SourceInput {
            provider: SourceProvider::ApplePhotos,
            external_id,
            original_filename: Some(filename),
            media_type: Some("image/heic"),
            width: Some(4032),
            height: Some(3024),
            taken_at: Some("2026-08-15 10:00:00"),
            metadata_json: None,
            status: SourceStatus::Discovered,
            last_seen_at: Some("2026-08-15 10:05:00"),
        }
    }

    #[tokio::test]
    async fn ensure_asset_for_photo_is_idempotent() {
        let pool = test_pool().await;
        insert_photo(&pool, 1).await;
        let first = ensure_asset_for_photo(&pool, 1).await.unwrap();
        let second = ensure_asset_for_photo(&pool, 1).await.unwrap();
        assert_eq!(first, second);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn upsert_source_preserves_link_and_refreshes_metadata() {
        let pool = test_pool().await;
        insert_photo(&pool, 1).await;
        let asset_id = ensure_asset_for_photo(&pool, 1).await.unwrap();
        let first = upsert_external_source(&pool, &apple_source("id-1", "UUID.heic"))
            .await.unwrap();
        link_source_to_asset(&pool, first.id, asset_id).await.unwrap();

        let updated = upsert_external_source(&pool, &apple_source("id-1", "IMG_0001.HEIC"))
            .await.unwrap();
        assert_eq!(updated.id, first.id);
        assert_eq!(updated.asset_id, Some(asset_id));
        assert_eq!(updated.original_filename.as_deref(), Some("IMG_0001.HEIC"));
    }

    #[tokio::test]
    async fn add_variant_is_idempotent_and_can_be_primary() {
        let pool = test_pool().await;
        insert_photo(&pool, 1).await;
        let asset_id = ensure_asset_for_photo(&pool, 1).await.unwrap();
        let input = VariantInput {
            asset_id,
            source_id: None,
            role: VariantRole::Imported,
            path: Some("/1.jpg"),
            content_sha256: Some("sha-1"),
            mime_type: Some("image/jpeg"),
            width: Some(100),
            height: Some(80),
            byte_size: Some(1234),
            generation_key: None,
            is_primary: true,
        };
        let first = add_variant(&pool, &input).await.unwrap();
        let second = add_variant(&pool, &input).await.unwrap();
        assert_eq!(first.id, second.id);
        assert!(second.is_primary);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM asset_variants")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn selecting_primary_variant_is_atomic() {
        let pool = test_pool().await;
        insert_photo(&pool, 1).await;
        let asset_id = ensure_asset_for_photo(&pool, 1).await.unwrap();
        let original = add_variant(&pool, &VariantInput {
            asset_id, source_id: None, role: VariantRole::Original,
            path: Some("/original.jpg"), content_sha256: Some("original"),
            mime_type: Some("image/jpeg"), width: None, height: None,
            byte_size: None, generation_key: None, is_primary: true,
        }).await.unwrap();
        let current = add_variant(&pool, &VariantInput {
            asset_id, source_id: None, role: VariantRole::Current,
            path: Some("/current.jpg"), content_sha256: Some("current"),
            mime_type: Some("image/jpeg"), width: None, height: None,
            byte_size: None, generation_key: None, is_primary: false,
        }).await.unwrap();

        set_primary_variant(&pool, asset_id, current.id).await.unwrap();
        let primary: i64 = sqlx::query_scalar(
            "SELECT id FROM asset_variants WHERE asset_id = ? AND is_primary = 1",
        )
        .bind(asset_id).fetch_one(&pool).await.unwrap();
        assert_eq!(primary, current.id);
        assert_ne!(primary, original.id);
    }

    #[tokio::test]
    async fn linking_unknown_source_returns_not_found() {
        let pool = test_pool().await;
        let result = link_source_to_asset(&pool, 999, 999).await;
        assert!(result.is_err());
    }
}
