use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

use crate::{
    catalog::{RenditionCandidate, VariantRole, select_display, select_master},
    error::{AppError, Result},
};

#[derive(Debug, Clone, Deserialize)]
struct RenditionFileManifest {
    role: String,
    relative_path: String,
    original_filename: Option<String>,
    uniform_type_identifier: String,
    mime_type: String,
    sha256: String,
    byte_size: i64,
    width: i64,
    height: i64,
    provenance: String,
    byte_preserved: bool,
    orientation_mode: String,
    source_orientation: Option<i64>,
    display_orientation: Option<i64>,
    color_space: Option<String>,
    generation_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RenditionPackageManifest {
    schema_version: u32,
    source_identifier: String,
    original_filename: String,
    adjusted: bool,
    original: RenditionFileManifest,
    current: Option<RenditionFileManifest>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RenditionCommit {
    pub source_id: i64,
    pub asset_id: i64,
    pub photo_id: i64,
    pub original_variant_id: i64,
    pub current_variant_id: Option<i64>,
    pub master_variant_id: i64,
    pub display_variant_id: i64,
    pub display_revision: i64,
    pub derived_invalidated: bool,
}

pub async fn commit_rendition_package(
    pool: &SqlitePool,
    source_id: i64,
    package_dir: &Path,
) -> Result<RenditionCommit> {
    commit_rendition_package_for_lease(pool, source_id, package_dir, None).await
}
pub async fn commit_rendition_package_for_lease(pool: &SqlitePool, source_id: i64, package_dir: &Path, lease_owner: Option<&str>) -> Result<RenditionCommit> {
    let manifest_text = std::fs::read_to_string(package_dir.join("manifest.json"))?;
    let manifest: RenditionPackageManifest = serde_json::from_str(&manifest_text)
        .map_err(|error| AppError::Metadata(format!("invalid rendition manifest: {error}")))?;
    validate_manifest(&manifest)?;
    let original_path = validate_file(package_dir, &manifest.original)?;
    let current_path = manifest
        .current
        .as_ref()
        .map(|current| validate_file(package_dir, current))
        .transpose()?;

    let mut tx = pool.begin().await?;
    let source: Option<(String, Option<i64>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT external_id, asset_id, original_filename, taken_at FROM asset_sources \
         WHERE id = ? AND provider = 'apple_photos'",
    )
    .bind(source_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (external_id, existing_asset_id, _, taken_at) =
        source.ok_or_else(|| AppError::NotFound(format!("Apple Photos source {source_id}")))?;
    if external_id != manifest.source_identifier {
        return Err(AppError::Metadata(
            "rendition source identity does not match catalog".into(),
        ));
    }

    let (asset_id, photo_id) = if let Some(asset_id) = existing_asset_id {
        let photo_id: Option<i64> = sqlx::query_scalar("SELECT photo_id FROM assets WHERE id = ?")
            .bind(asset_id)
            .fetch_one(&mut *tx)
            .await?;
        match photo_id {
            Some(photo_id) => (asset_id, photo_id),
            None => {
                create_compatibility_photo(
                    &mut tx,
                    asset_id,
                    &original_path,
                    &manifest.original,
                    taken_at.as_deref(),
                )
                .await?
            }
        }
    } else {
        let matches: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM photos WHERE sha256 = ? AND import_status = 'imported' ORDER BY id",
        )
        .bind(&manifest.original.sha256)
        .fetch_all(&mut *tx)
        .await?;
        if matches.len() > 1 {
            return Err(AppError::Metadata(
                "original hash matches multiple existing photos".into(),
            ));
        }
        let photo_id =
            if let Some(photo_id) = matches.first() {
                *photo_id
            } else {
                sqlx::query_scalar(
                "INSERT INTO photos (path, sha256, taken_at, format, import_status, width, height) \
                 VALUES (?, ?, ?, ?, 'imported', ?, ?) RETURNING id",
            ).bind(path_string(&original_path)).bind(&manifest.original.sha256).bind(&taken_at)
            .bind(format_for_mime(&manifest.original.mime_type))
            .bind(positive(manifest.original.width)).bind(positive(manifest.original.height))
            .fetch_one(&mut *tx).await?
            };
        sqlx::query("INSERT OR IGNORE INTO assets (photo_id) VALUES (?)")
            .bind(photo_id)
            .execute(&mut *tx)
            .await?;
        let asset_id: i64 = sqlx::query_scalar("SELECT id FROM assets WHERE photo_id = ?")
            .bind(photo_id)
            .fetch_one(&mut *tx)
            .await?;
        (asset_id, photo_id)
    };

    sqlx::query("UPDATE asset_sources SET asset_id = ?, original_filename = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(asset_id).bind(&manifest.original_filename).bind(source_id).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO asset_links (source_id, photo_id, method, confidence, status, evidence_json, reviewed_at) \
         VALUES (?, ?, 'exact_sha256', 1.0, 'accepted', ?, datetime('now')) \
         ON CONFLICT(source_id, photo_id, method) DO UPDATE SET status = 'accepted', reviewed_at = datetime('now')",
    ).bind(source_id).bind(photo_id)
    .bind(serde_json::json!({"sha256": manifest.original.sha256}).to_string())
    .execute(&mut *tx).await?;

    let existing_original: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, content_sha256 FROM asset_variants \
         WHERE asset_id = ? AND source_id = ? AND role = 'original' ORDER BY id LIMIT 1",
    )
    .bind(asset_id)
    .bind(source_id)
    .fetch_optional(&mut *tx)
    .await?;
    let original_variant_id = if let Some((variant_id, hash)) = existing_original {
        if hash.as_deref() != Some(manifest.original.sha256.as_str()) {
            return Err(AppError::Metadata("immutable original hash changed".into()));
        }
        variant_id
    } else {
        insert_variant(
            &mut tx,
            asset_id,
            source_id,
            &manifest.original,
            &original_path,
            None,
        )
        .await?
    };
    upsert_rendition_metadata(&mut tx, original_variant_id, &manifest.original).await?;

    let current_variant_id = if let (Some(current), Some(path)) =
        (&manifest.current, current_path.as_ref())
    {
        let generation = current.generation_key.as_deref().unwrap_or(&current.sha256);
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM asset_variants WHERE asset_id = ? AND source_id = ? AND role = 'current' \
             AND generation_key = ?",
        ).bind(asset_id).bind(source_id).bind(generation).fetch_optional(&mut *tx).await?;
        let variant_id = if let Some(variant_id) = existing {
            let hash: Option<String> =
                sqlx::query_scalar("SELECT content_sha256 FROM asset_variants WHERE id = ?")
                    .bind(variant_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if hash.as_deref() != Some(current.sha256.as_str()) {
                return Err(AppError::Metadata(
                    "current rendition generation changed bytes".into(),
                ));
            }
            variant_id
        } else {
            insert_variant(
                &mut tx,
                asset_id,
                source_id,
                current,
                path,
                Some(generation),
            )
            .await?
        };
        upsert_rendition_metadata(&mut tx, variant_id, current).await?;
        Some(variant_id)
    } else {
        None
    };

    let candidates = rendition_candidates(&mut tx, asset_id).await?;
    let master_variant_id = select_master(&candidates)
        .ok_or_else(|| AppError::Metadata("asset has no master rendition".into()))?;
    let display_variant_id = select_display(&candidates)
        .ok_or_else(|| AppError::Metadata("asset has no display rendition".into()))?;
    let (previous_display, mut display_revision): (Option<i64>, i64) =
        sqlx::query_as("SELECT display_variant_id, display_revision FROM assets WHERE id = ?")
            .bind(asset_id)
            .fetch_one(&mut *tx)
            .await?;
    let display_changed = previous_display != Some(display_variant_id);
    if display_changed {
        display_revision += 1;
        sqlx::query(
            "INSERT INTO asset_display_revisions (asset_id, revision, previous_variant_id, \
             display_variant_id, reason) VALUES (?, ?, ?, ?, 'rendition_commit')",
        )
        .bind(asset_id)
        .bind(display_revision)
        .bind(previous_display)
        .bind(display_variant_id)
        .execute(&mut *tx)
        .await?;
        crate::derived::invalidate_in_transaction(&mut tx, photo_id).await?;
    }
    sqlx::query(
        "UPDATE assets SET master_variant_id = ?, display_variant_id = ?, display_revision = ?, \
         updated_at = datetime('now') WHERE id = ?",
    )
    .bind(master_variant_id)
    .bind(display_variant_id)
    .bind(display_revision)
    .bind(asset_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE asset_variants SET is_primary = CASE WHEN id = ? THEN 1 ELSE 0 END WHERE asset_id = ?")
        .bind(master_variant_id).bind(asset_id).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE asset_sources SET sync_status = 'ready', exclusion_reason = NULL, last_error = NULL, \
         updated_at = datetime('now') WHERE id = ?",
    ).bind(source_id).execute(&mut *tx).await?;
    if let Some(worker) = lease_owner {
        let result = sqlx::query("UPDATE sync_items SET status = 'succeeded', lease_owner = NULL, lease_expires_at = NULL, last_error = NULL, finished_at = datetime('now'), updated_at = datetime('now') WHERE source_id = ? AND operation = 'export_original' AND status = 'leased' AND lease_owner = ?").bind(source_id).bind(worker).execute(&mut *tx).await?;
        if result.rows_affected() != 1 { return Err(AppError::Metadata("Apple export lease is no longer active".into())); }
        let job_id: i64 = sqlx::query_scalar("SELECT job_id FROM sync_items WHERE source_id = ? AND operation = 'export_original' ORDER BY id DESC LIMIT 1").bind(source_id).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE sync_jobs SET completed_items = (SELECT COUNT(*) FROM sync_items WHERE job_id = ? AND status IN ('succeeded', 'excluded', 'cancelled')), failed_items = (SELECT COUNT(*) FROM sync_items WHERE job_id = ? AND status = 'failed'), status = CASE WHEN EXISTS(SELECT 1 FROM sync_items WHERE job_id = ? AND status IN ('queued', 'leased')) THEN 'running' WHEN EXISTS(SELECT 1 FROM sync_items WHERE job_id = ? AND status = 'failed') THEN 'failed' ELSE 'completed' END, finished_at = CASE WHEN EXISTS(SELECT 1 FROM sync_items WHERE job_id = ? AND status IN ('queued', 'leased')) THEN NULL ELSE datetime('now') END, updated_at = datetime('now') WHERE id = ?").bind(job_id).bind(job_id).bind(job_id).bind(job_id).bind(job_id).bind(job_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;

    Ok(RenditionCommit {
        source_id,
        asset_id,
        photo_id,
        original_variant_id,
        current_variant_id,
        master_variant_id,
        display_variant_id,
        display_revision,
        derived_invalidated: display_changed,
    })
}

fn validate_manifest(manifest: &RenditionPackageManifest) -> Result<()> {
    if manifest.schema_version != 1
        || manifest.source_identifier.is_empty()
        || manifest.original.role != "original"
        || manifest.original.original_filename.as_deref() != Some(manifest.original_filename.as_str())
        || manifest.original.uniform_type_identifier.is_empty()
        || !manifest.original.byte_preserved
        || manifest.original.provenance != "photokit_resource"
        || manifest.adjusted != manifest.current.is_some()
        || manifest
            .current
            .as_ref()
            .is_some_and(|current| {
                current.role != "current"
                    || current.byte_preserved
                    || current.uniform_type_identifier.is_empty()
            })
    {
        return Err(AppError::Metadata(
            "invalid rendition package contract".into(),
        ));
    }
    Ok(())
}

fn validate_file(package_dir: &Path, rendition: &RenditionFileManifest) -> Result<PathBuf> {
    let relative = Path::new(&rendition.relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(AppError::Metadata("rendition path escapes package".into()));
    }
    let path = package_dir.join(relative);
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() != rendition.byte_size as u64 {
        return Err(AppError::Metadata(format!(
            "rendition size mismatch: {}",
            rendition.role
        )));
    }
    let mut file = File::open(&path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if hex::encode(hasher.finalize()) != rendition.sha256 {
        return Err(AppError::Metadata(format!(
            "rendition hash mismatch: {}",
            rendition.role
        )));
    }
    Ok(path)
}

async fn create_compatibility_photo(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    asset_id: i64,
    path: &Path,
    original: &RenditionFileManifest,
    taken_at: Option<&str>,
) -> Result<(i64, i64)> {
    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, taken_at, format, import_status, width, height) \
         VALUES (?, ?, ?, ?, 'imported', ?, ?) RETURNING id",
    )
    .bind(path_string(path))
    .bind(&original.sha256)
    .bind(taken_at)
    .bind(format_for_mime(&original.mime_type))
    .bind(positive(original.width))
    .bind(positive(original.height))
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("UPDATE assets SET photo_id = ? WHERE id = ?")
        .bind(photo_id)
        .bind(asset_id)
        .execute(&mut **tx)
        .await?;
    Ok((asset_id, photo_id))
}

async fn insert_variant(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    asset_id: i64,
    source_id: i64,
    rendition: &RenditionFileManifest,
    path: &Path,
    generation_key: Option<&str>,
) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "INSERT INTO asset_variants (asset_id, source_id, role, path, content_sha256, mime_type, \
         width, height, byte_size, generation_key) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    ).bind(asset_id).bind(source_id).bind(&rendition.role).bind(path_string(path))
    .bind(&rendition.sha256).bind(&rendition.mime_type).bind(positive(rendition.width))
    .bind(positive(rendition.height)).bind(rendition.byte_size).bind(generation_key)
    .fetch_one(&mut **tx).await?)
}

async fn upsert_rendition_metadata(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    variant_id: i64,
    rendition: &RenditionFileManifest,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO variant_renditions (variant_id, provenance, byte_preserved, orientation_mode, \
         source_orientation, display_orientation, color_space, source_fingerprint, generated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now')) \
         ON CONFLICT(variant_id) DO UPDATE SET provenance = excluded.provenance, \
         byte_preserved = excluded.byte_preserved, orientation_mode = excluded.orientation_mode, \
         source_orientation = excluded.source_orientation, display_orientation = excluded.display_orientation, \
         color_space = excluded.color_space, source_fingerprint = excluded.source_fingerprint, \
         updated_at = datetime('now')",
    ).bind(variant_id).bind(&rendition.provenance).bind(rendition.byte_preserved)
    .bind(&rendition.orientation_mode).bind(rendition.source_orientation)
    .bind(rendition.display_orientation).bind(&rendition.color_space).bind(&rendition.sha256)
    .execute(&mut **tx).await?;
    Ok(())
}

async fn rendition_candidates(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    asset_id: i64,
) -> Result<Vec<RenditionCandidate>> {
    let rows: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT v.id, v.role, COALESCE(r.byte_preserved, 0) FROM asset_variants v \
         LEFT JOIN variant_renditions r ON r.variant_id = v.id \
         WHERE v.asset_id = ? ORDER BY v.id DESC",
    )
    .bind(asset_id)
    .fetch_all(&mut **tx)
    .await?;
    rows.into_iter()
        .map(|(variant_id, role, byte_preserved)| {
            Ok(RenditionCandidate {
                variant_id,
                role: parse_role(&role)?,
                byte_preserved: byte_preserved != 0,
            })
        })
        .collect()
}

fn parse_role(role: &str) -> Result<VariantRole> {
    match role {
        "original" => Ok(VariantRole::Original),
        "current" => Ok(VariantRole::Current),
        "imported" => Ok(VariantRole::Imported),
        "preview" => Ok(VariantRole::Preview),
        "thumbnail" => Ok(VariantRole::Thumbnail),
        "raw_companion" => Ok(VariantRole::RawCompanion),
        _ => Err(AppError::Metadata(format!("unknown variant role {role}"))),
    }
}

fn positive(value: i64) -> Option<i64> {
    (value > 0).then_some(value)
}
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
fn format_for_mime(mime: &str) -> &str {
    match mime {
        "image/jpeg" => "jpeg",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "image/png" => "png",
        "image/tiff" => "tiff",
        "image/gif" => "gif",
        _ => "unknown",
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

    fn hash(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    fn write_package(root: &Path, source: &str, original: &[u8], current: Option<&[u8]>) {
        std::fs::create_dir_all(root.join("original")).unwrap();
        std::fs::write(root.join("original/IMG_0001.HEIC"), original).unwrap();
        if let Some(current) = current {
            std::fs::create_dir_all(root.join("current")).unwrap();
            std::fs::write(root.join("current/current.jpg"), current).unwrap();
        }
        let file = |role: &str,
                    path: &str,
                    bytes: &[u8],
                    preserved: bool,
                    generation: Option<&str>| {
            serde_json::json!({
                "role": role, "relative_path": path,
                "original_filename": if role == "original" { Some("IMG_0001.HEIC") } else { None },
                "uniform_type_identifier": if role == "original" { "public.heic" } else { "public.jpeg" },
                "mime_type": if role == "original" { "image/heic" } else { "image/jpeg" },
                "sha256": hash(bytes), "byte_size": bytes.len(), "width": 100, "height": 80,
                "provenance": if role == "original" { "photokit_resource" } else { "photokit_current" },
                "byte_preserved": preserved, "orientation_mode": "metadata",
                "source_orientation": 6, "display_orientation": 6, "color_space": "RGB",
                "generation_key": generation,
            })
        };
        let manifest = serde_json::json!({
            "schema_version": 1, "source_identifier": source,
            "original_filename": "IMG_0001.HEIC", "adjusted": current.is_some(),
            "original": file("original", "original/IMG_0001.HEIC", original, true, None),
            "current": current.map(|bytes| file("current", "current/current.jpg", bytes, false, Some("v1"))),
        });
        std::fs::write(root.join("manifest.json"), manifest.to_string()).unwrap();
    }

    #[tokio::test]
    async fn commits_verified_original_and_current_with_separate_selection() {
        let pool = test_pool().await;
        let source_id: i64 = sqlx::query_scalar(
            "INSERT INTO asset_sources (provider, external_id, original_filename, sync_status) \
             VALUES ('apple_photos', 'asset/L0/001', 'IMG_0001.HEIC', 'downloaded') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        write_package(
            temp.path(),
            "asset/L0/001",
            b"original bytes",
            Some(b"edited bytes"),
        );

        let committed = commit_rendition_package(&pool, source_id, temp.path())
            .await
            .unwrap();
        assert_eq!(committed.master_variant_id, committed.original_variant_id);
        assert_eq!(
            Some(committed.display_variant_id),
            committed.current_variant_id
        );
        assert_eq!(committed.display_revision, 1);
        assert!(committed.derived_invalidated);
        let derived: (i64, String, String) = sqlx::query_as(
            "SELECT render_revision, thumbnail_status, face_status \
             FROM derived_media_state WHERE photo_id = ?",
        )
        .bind(committed.photo_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(derived, (1, "pending".into(), "pending".into()));
        let source: (String, Option<i64>) =
            sqlx::query_as("SELECT sync_status, asset_id FROM asset_sources WHERE id = ?")
                .bind(source_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(source, ("ready".to_owned(), Some(committed.asset_id)));
        let variants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM asset_variants")
            .fetch_one(&pool)
            .await
            .unwrap();
        let provenance: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM variant_renditions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((variants, provenance), (2, 2));

        let repeated = commit_rendition_package(&pool, source_id, temp.path())
            .await
            .unwrap();
        assert_eq!(repeated.display_revision, committed.display_revision);
        assert!(!repeated.derived_invalidated);
        let render_revision: i64 =
            sqlx::query_scalar("SELECT render_revision FROM photos WHERE id = ?")
                .bind(committed.photo_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(render_revision, 1);
        let revisions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM asset_display_revisions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(revisions, 1);

        let next = tempfile::tempdir().unwrap();
        write_package(
            next.path(),
            "asset/L0/001",
            b"original bytes",
            Some(b"new edited bytes"),
        );
        let manifest_path = next.path().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest["current"]["generation_key"] = serde_json::json!("v2");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        let changed = commit_rendition_package(&pool, source_id, next.path())
            .await
            .unwrap();
        assert_eq!(changed.display_revision, 2);
        assert!(changed.derived_invalidated);
        let state_revision: i64 = sqlx::query_scalar(
            "SELECT render_revision FROM derived_media_state WHERE photo_id = ?",
        )
        .bind(changed.photo_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state_revision, 2);
    }

    #[tokio::test]
    async fn rejects_changed_original_and_unsafe_package_paths_without_mutation() {
        let pool = test_pool().await;
        let source_id: i64 = sqlx::query_scalar(
            "INSERT INTO asset_sources (provider, external_id, sync_status) \
             VALUES ('apple_photos', 'asset/L0/001', 'downloaded') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let first = tempfile::tempdir().unwrap();
        write_package(first.path(), "asset/L0/001", b"original bytes", None);
        commit_rendition_package(&pool, source_id, first.path())
            .await
            .unwrap();
        let second = tempfile::tempdir().unwrap();
        write_package(second.path(), "asset/L0/001", b"different original", None);
        assert!(
            commit_rendition_package(&pool, source_id, second.path())
                .await
                .is_err()
        );
        let variants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM asset_variants")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(variants, 1);

        let manifest_path = first.path().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
        manifest["original"]["relative_path"] = serde_json::json!("../escape.heic");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        assert!(
            commit_rendition_package(&pool, source_id, first.path())
                .await
                .is_err()
        );
    }
}
