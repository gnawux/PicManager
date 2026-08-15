use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InventoryResource {
    #[serde(rename = "type")]
    kind: String,
    original_filename: String,
    uniform_type_identifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InventoryAsset {
    schema_version: u32,
    local_identifier: String,
    original_filename: Option<String>,
    media_type: String,
    pixel_width: i64,
    pixel_height: i64,
    creation_date: Option<String>,
    modification_date: Option<String>,
    favorite: bool,
    hidden: bool,
    live_photo: bool,
    adjusted: bool,
    burst_identifier: Option<String>,
    excluded_reason: Option<String>,
    resources: Vec<InventoryResource>,
}

#[derive(Debug)]
struct ParsedInventory {
    checkpoint: Option<Vec<u8>>,
    assets: Vec<InventoryAsset>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppleInventoryReport {
    pub dry_run: bool,
    pub total_assets: usize,
    pub excluded_assets: usize,
    pub exact_links: usize,
    pub ambiguous_links: usize,
    pub queued_assets: usize,
    pub missing_assets: u64,
    pub job_id: Option<i64>,
}

pub async fn ingest_inventory(
    pool: &SqlitePool,
    path: &Path,
    dry_run: bool,
) -> Result<AppleInventoryReport> {
    let text = std::fs::read_to_string(path)?;
    let parsed = parse_inventory(&text)?;
    let legacy = load_legacy_candidates(pool).await?;
    let mut report = AppleInventoryReport {
        dry_run,
        total_assets: parsed.assets.len(),
        excluded_assets: parsed
            .assets
            .iter()
            .filter(|a| a.excluded_reason.is_some())
            .count(),
        exact_links: 0,
        ambiguous_links: 0,
        queued_assets: 0,
        missing_assets: 0,
        job_id: None,
    };
    for asset in &parsed.assets {
        match legacy
            .get(&sanitized_identifier(&asset.local_identifier))
            .map(Vec::len)
        {
            Some(1) => report.exact_links += 1,
            Some(n) if n > 1 => report.ambiguous_links += 1,
            _ if asset.excluded_reason.is_none() => report.queued_assets += 1,
            _ => {}
        }
    }
    if dry_run {
        return Ok(report);
    }

    let marker = format!(
        "inventory:{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let mut tx = pool.begin().await?;
    let checkpoint_before: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT token FROM provider_checkpoints WHERE provider = 'apple_photos' AND scope_key = 'system'",
    ).fetch_optional(&mut *tx).await?.flatten();
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO sync_jobs (kind, provider, checkpoint_before, checkpoint_after, total_items) \
         VALUES ('apple_full_inventory', 'apple_photos', ?, ?, 0) RETURNING id",
    )
    .bind(&checkpoint_before)
    .bind(&parsed.checkpoint)
    .fetch_one(&mut *tx)
    .await?;
    let mut queued = 0_i64;

    for asset in &parsed.assets {
        let metadata =
            serde_json::to_string(asset).map_err(|e| AppError::Metadata(e.to_string()))?;
        let existing: Option<(i64, Option<i64>, String)> = sqlx::query_as(
            "SELECT id, asset_id, sync_status FROM asset_sources \
             WHERE provider = 'apple_photos' AND external_id = ?",
        )
        .bind(&asset.local_identifier)
        .fetch_optional(&mut *tx)
        .await?;
        let initial_status = if asset.excluded_reason.is_some() {
            "excluded"
        } else {
            "discovered"
        };
        sqlx::query(
            "INSERT INTO asset_sources (provider, external_id, original_filename, media_type, width, \
             height, taken_at, metadata_json, sync_status, exclusion_reason, last_seen_at) \
             VALUES ('apple_photos', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(provider, external_id) WHERE external_id IS NOT NULL DO UPDATE SET \
             original_filename = excluded.original_filename, media_type = excluded.media_type, \
             width = excluded.width, height = excluded.height, taken_at = excluded.taken_at, \
             metadata_json = excluded.metadata_json, exclusion_reason = excluded.exclusion_reason, \
             last_seen_at = excluded.last_seen_at, updated_at = datetime('now')",
        )
        .bind(&asset.local_identifier).bind(&asset.original_filename)
        .bind(mime_type(asset)).bind(asset.pixel_width).bind(asset.pixel_height)
        .bind(&asset.creation_date).bind(metadata).bind(initial_status)
        .bind(&asset.excluded_reason).bind(&marker).execute(&mut *tx).await?;
        let source_id: i64 = sqlx::query_scalar(
            "SELECT id FROM asset_sources WHERE provider = 'apple_photos' AND external_id = ?",
        )
        .bind(&asset.local_identifier)
        .fetch_one(&mut *tx)
        .await?;

        if let Some((_, Some(catalog_asset_id), _)) = &existing {
            sqlx::query(
                "UPDATE asset_sources SET asset_id = ?, sync_status = 'ready', \
                 exclusion_reason = NULL, last_error = NULL, updated_at = datetime('now') WHERE id = ?",
            ).bind(catalog_asset_id).bind(source_id).execute(&mut *tx).await?;
            continue;
        }

        let candidates = legacy.get(&sanitized_identifier(&asset.local_identifier));
        if let Some(candidates) = candidates {
            if candidates.len() == 1 {
                let photo_id = candidates[0];
                sqlx::query("INSERT OR IGNORE INTO assets (photo_id) VALUES (?)")
                    .bind(photo_id)
                    .execute(&mut *tx)
                    .await?;
                let catalog_asset_id: i64 =
                    sqlx::query_scalar("SELECT id FROM assets WHERE photo_id = ?")
                        .bind(photo_id)
                        .fetch_one(&mut *tx)
                        .await?;
                sqlx::query(
                    "UPDATE asset_sources SET asset_id = ?, sync_status = 'ready', \
                     exclusion_reason = NULL, last_error = NULL, updated_at = datetime('now') WHERE id = ?",
                ).bind(catalog_asset_id).bind(source_id).execute(&mut *tx).await?;
                sqlx::query(
                    "INSERT INTO asset_links (source_id, photo_id, method, confidence, status, evidence_json, reviewed_at) \
                     VALUES (?, ?, 'legacy_local_identifier', 1.0, 'accepted', ?, datetime('now')) \
                     ON CONFLICT(source_id, photo_id, method) DO UPDATE SET status = 'accepted', \
                     confidence = 1.0, evidence_json = excluded.evidence_json, reviewed_at = datetime('now')",
                ).bind(source_id).bind(photo_id)
                .bind(format!(r#"{{"sanitized_local_identifier":"{}"}}"#, sanitized_identifier(&asset.local_identifier)))
                .execute(&mut *tx).await?;
                continue;
            }
            for photo_id in candidates {
                sqlx::query(
                    "INSERT INTO asset_links (source_id, photo_id, method, confidence, status, evidence_json) \
                     VALUES (?, ?, 'legacy_local_identifier', 1.0, 'conflict', ?) \
                     ON CONFLICT(source_id, photo_id, method) DO UPDATE SET status = 'conflict'",
                ).bind(source_id).bind(photo_id)
                .bind(r#"{"reason":"multiple_legacy_filename_matches"}"#)
                .execute(&mut *tx).await?;
            }
            continue;
        }

        if asset.excluded_reason.is_some() {
            sqlx::query("UPDATE asset_sources SET sync_status = 'excluded' WHERE id = ?")
                .bind(source_id)
                .execute(&mut *tx)
                .await?;
            continue;
        }
        let prior_status = existing.as_ref().map(|row| row.2.as_str());
        if matches!(
            prior_status,
            Some("failed" | "queued" | "downloading" | "downloaded" | "importing")
        ) {
            continue;
        }
        sqlx::query(
            "UPDATE asset_sources SET sync_status = 'queued', exclusion_reason = NULL WHERE id = ?",
        )
        .bind(source_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO sync_items (job_id, source_id, external_id, operation, payload_json) \
             VALUES (?, ?, ?, 'export_original', ?)",
        )
        .bind(job_id)
        .bind(source_id)
        .bind(&asset.local_identifier)
        .bind(serde_json::json!({"original_filename": asset.original_filename}).to_string())
        .execute(&mut *tx)
        .await?;
        queued += 1;
    }

    let missing = sqlx::query(
        "UPDATE asset_sources SET sync_status = 'missing', updated_at = datetime('now') \
         WHERE provider = 'apple_photos' AND (last_seen_at IS NULL OR last_seen_at != ?)",
    )
    .bind(&marker)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query(
        "UPDATE sync_jobs SET total_items = ?, status = CASE WHEN ? = 0 THEN 'completed' ELSE 'queued' END, \
         started_at = CASE WHEN ? = 0 THEN datetime('now') ELSE NULL END, \
         finished_at = CASE WHEN ? = 0 THEN datetime('now') ELSE NULL END, updated_at = datetime('now') WHERE id = ?",
    ).bind(queued).bind(queued).bind(queued).bind(queued).bind(job_id).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO provider_checkpoints (provider, scope_key, token, generation, last_full_reconcile) \
         VALUES ('apple_photos', 'system', ?, 1, datetime('now')) \
         ON CONFLICT(provider, scope_key) DO UPDATE SET token = excluded.token, \
         generation = provider_checkpoints.generation + 1, last_full_reconcile = datetime('now'), \
         updated_at = datetime('now')",
    ).bind(&parsed.checkpoint).execute(&mut *tx).await?;
    tx.commit().await?;

    report.queued_assets = queued as usize;
    report.missing_assets = missing;
    report.job_id = Some(job_id);
    Ok(report)
}

fn parse_inventory(text: &str) -> Result<ParsedInventory> {
    let mut checkpoint = None;
    let mut expected_count = None;
    let mut assets = Vec::new();
    let mut identifiers = HashSet::new();
    let mut saw_header = false;
    let mut saw_end = false;
    for (index, line) in text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| AppError::Metadata(format!("inventory line {}: {e}", index + 1)))?;
        match value.get("record_type").and_then(|v| v.as_str()) {
            Some("inventory_header") if !saw_header && assets.is_empty() => {
                if value.get("schema_version").and_then(|v| v.as_u64()) != Some(1) {
                    return Err(AppError::Metadata(
                        "unsupported Apple inventory schema".into(),
                    ));
                }
                checkpoint = decode_checkpoint(value.get("checkpoint"))?;
                saw_header = true;
            }
            Some("asset") if saw_header && !saw_end => {
                let asset: InventoryAsset = serde_json::from_value(value)
                    .map_err(|e| AppError::Metadata(format!("invalid inventory asset: {e}")))?;
                if asset.schema_version != 1 || asset.local_identifier.is_empty() {
                    return Err(AppError::Metadata(
                        "invalid Apple inventory asset identity".into(),
                    ));
                }
                if !identifiers.insert(asset.local_identifier.clone()) {
                    return Err(AppError::Metadata(format!(
                        "duplicate Apple asset {}",
                        asset.local_identifier
                    )));
                }
                assets.push(asset);
            }
            Some("inventory_end") if saw_header && !saw_end => {
                expected_count = value
                    .get("asset_count")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);
                saw_end = true;
            }
            _ => {
                return Err(AppError::Metadata(format!(
                    "invalid inventory sequence at line {}",
                    index + 1
                )));
            }
        }
    }
    if !saw_header || !saw_end || expected_count != Some(assets.len()) {
        return Err(AppError::Metadata(
            "Apple inventory is incomplete or count does not match".into(),
        ));
    }
    Ok(ParsedInventory { checkpoint, assets })
}

fn decode_checkpoint(value: Option<&serde_json::Value>) -> Result<Option<Vec<u8>>> {
    match value.and_then(|v| v.as_str()) {
        Some(encoded) => STANDARD
            .decode(encoded)
            .map(Some)
            .map_err(|e| AppError::Metadata(format!("invalid inventory checkpoint: {e}"))),
        None => Ok(None),
    }
}

async fn load_legacy_candidates(pool: &SqlitePool) -> Result<HashMap<String, Vec<i64>>> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, path FROM photos WHERE import_status = 'imported' ORDER BY id")
            .fetch_all(pool)
            .await?;
    let mut result: HashMap<String, Vec<i64>> = HashMap::new();
    for (id, path) in rows {
        if let Some(stem) = PathBuf::from(path).file_stem().and_then(|s| s.to_str()) {
            result
                .entry(stem.to_ascii_lowercase())
                .or_default()
                .push(id);
        }
    }
    Ok(result)
}

fn sanitized_identifier(identifier: &str) -> String {
    identifier.replace('/', "_").to_ascii_lowercase()
}

fn mime_type(asset: &InventoryAsset) -> &str {
    let uti = asset
        .resources
        .iter()
        .find(|r| r.kind == "photo")
        .or_else(|| asset.resources.first())
        .map(|r| r.uniform_type_identifier.as_str());
    match uti {
        Some("public.jpeg" | "public.jpg") => "image/jpeg",
        Some("public.heic") => "image/heic",
        Some("public.heif") => "image/heif",
        Some("public.png") => "image/png",
        Some("public.tiff" | "public.tif") => "image/tiff",
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

    fn asset(id: &str, filename: &str, excluded: Option<&str>) -> String {
        serde_json::json!({
            "record_type": "asset", "schema_version": 1,
            "local_identifier": id, "original_filename": filename,
            "media_type": "image", "pixel_width": 4032, "pixel_height": 3024,
            "creation_date": "2026-08-15T01:02:03Z", "modification_date": null,
            "favorite": false, "hidden": false, "live_photo": false,
            "adjusted": false, "burst_identifier": null,
            "excluded_reason": excluded,
            "resources": [{"type":"photo", "original_filename":filename,
                "uniform_type_identifier":"public.heic"}]
        })
        .to_string()
    }

    fn inventory(assets: &[String]) -> String {
        let mut lines = vec![
            serde_json::json!({
                "record_type":"inventory_header", "schema_version":1, "checkpoint":"dG9rZW4="
            })
            .to_string(),
        ];
        lines.extend_from_slice(assets);
        lines.push(
            serde_json::json!({
                "record_type":"inventory_end", "schema_version":1,
                "checkpoint":"dG9rZW4y", "asset_count":assets.len()
            })
            .to_string(),
        );
        lines.join("\n")
    }

    #[test]
    fn rejects_partial_duplicate_and_mismatched_inventories() {
        assert!(
            parse_inventory(r#"{"record_type":"inventory_header","schema_version":1}"#).is_err()
        );
        let duplicate = inventory(&[asset("same", "a.heic", None), asset("same", "a.heic", None)]);
        assert!(parse_inventory(&duplicate).is_err());
        let mismatch = inventory(&[asset("one", "a.heic", None)])
            .replace("\"asset_count\":1", "\"asset_count\":2");
        assert!(parse_inventory(&mismatch).is_err());
    }

    #[tokio::test]
    async fn inventory_links_legacy_names_queues_new_assets_and_keeps_exclusions_visible() {
        let pool = test_pool().await;
        let local_id = "12345678-1234-1234-1234-123456789abc/L0/001";
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status) VALUES (?, 'sha', 'heic', 'imported')",
        ).bind(format!("/library/{}.heic", sanitized_identifier(local_id)))
            .execute(&pool).await.unwrap();
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            temp.path(),
            inventory(&[
                asset(local_id, "IMG_0001.HEIC", None),
                asset("new/L0/002", "IMG_0002.HEIC", None),
                asset("raw/L0/003", "DSC0003.ARW", Some("raw_only")),
            ]),
        )
        .unwrap();

        let report = ingest_inventory(&pool, temp.path(), false).await.unwrap();
        assert_eq!(report.total_assets, 3);
        assert_eq!(report.exact_links, 1);
        assert_eq!(report.queued_assets, 1);
        assert_eq!(report.excluded_assets, 1);
        let rows: Vec<(String, String, Option<String>, Option<i64>)> = sqlx::query_as(
            "SELECT external_id, sync_status, original_filename, asset_id \
             FROM asset_sources WHERE provider = 'apple_photos' ORDER BY external_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows[0].2.as_deref(), Some("IMG_0001.HEIC"));
        assert_eq!(rows[0].1, "ready");
        assert!(rows[0].3.is_some());
        assert_eq!(rows[1].1, "queued");
        assert_eq!(rows[2].1, "excluded");
        let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(items, 1);
        let token: Vec<u8> = sqlx::query_scalar("SELECT token FROM provider_checkpoints")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(token, b"token");
    }

    #[tokio::test]
    async fn completed_reconciliation_marks_unseen_sources_missing_and_dry_run_writes_nothing() {
        let pool = test_pool().await;
        let first_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            first_file.path(),
            inventory(&[asset("one", "one.heic", None)]),
        )
        .unwrap();
        ingest_inventory(&pool, first_file.path(), false)
            .await
            .unwrap();

        let empty_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(empty_file.path(), inventory(&[])).unwrap();
        let dry = ingest_inventory(&pool, empty_file.path(), true)
            .await
            .unwrap();
        assert!(dry.dry_run);
        let before: String = sqlx::query_scalar("SELECT sync_status FROM asset_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before, "queued");
        let report = ingest_inventory(&pool, empty_file.path(), false)
            .await
            .unwrap();
        assert_eq!(report.missing_assets, 1);
        let after: String = sqlx::query_scalar("SELECT sync_status FROM asset_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after, "missing");
    }
}
