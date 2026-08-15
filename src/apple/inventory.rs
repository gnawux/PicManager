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

#[derive(Debug)]
struct ParsedChanges {
    checkpoint_before: Option<Vec<u8>>,
    checkpoint_after: Option<Vec<u8>>,
    assets: Vec<InventoryAsset>,
    removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppleInventoryReport {
    pub dry_run: bool,
    pub total_assets: usize,
    pub excluded_assets: usize,
    pub exact_links: usize,
    pub ambiguous_links: usize,
    pub review_candidates: usize,
    pub queued_assets: usize,
    pub missing_assets: u64,
    pub job_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppleChangesReport {
    pub changed_assets: usize,
    pub removed_assets: usize,
    pub exact_links: usize,
    pub review_candidates: usize,
    pub queued_assets: usize,
    pub job_id: i64,
}

pub async fn ingest_inventory(
    pool: &SqlitePool,
    path: &Path,
    dry_run: bool,
) -> Result<AppleInventoryReport> {
    let text = std::fs::read_to_string(path)?;
    let parsed = parse_inventory(&text)?;
    let legacy = load_legacy_catalog(pool).await?;
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
        review_candidates: 0,
        queued_assets: 0,
        missing_assets: 0,
        job_id: None,
    };
    for asset in &parsed.assets {
        match legacy
            .names
            .get(&sanitized_identifier(&asset.local_identifier))
            .map(Vec::len)
        {
            Some(1) => report.exact_links += 1,
            Some(n) if n > 1 => report.ambiguous_links += 1,
            _ if asset.excluded_reason.is_none() => {
                let candidates = structured_candidates(asset, &legacy);
                if candidates.is_empty() {
                    report.queued_assets += 1;
                } else {
                    report.review_candidates += candidates.len();
                }
            }
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

        let candidates = legacy
            .names
            .get(&sanitized_identifier(&asset.local_identifier));
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
        let secondary = structured_candidates(asset, &legacy);
        if !secondary.is_empty() {
            for candidate in secondary {
                sqlx::query(
                    "INSERT INTO asset_links (source_id, photo_id, method, confidence, status, evidence_json) \
                     VALUES (?, ?, 'structured_metadata', ?, 'candidate', ?) \
                     ON CONFLICT(source_id, photo_id, method) DO UPDATE SET confidence = excluded.confidence, \
                     evidence_json = excluded.evidence_json WHERE asset_links.status = 'candidate'",
                )
                .bind(source_id).bind(candidate.photo_id).bind(candidate.confidence)
                .bind(candidate.evidence_json).execute(&mut *tx).await?;
            }
            sqlx::query("UPDATE asset_sources SET sync_status = 'discovered' WHERE id = ?")
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

pub async fn ingest_changes(pool: &SqlitePool, path: &Path) -> Result<AppleChangesReport> {
    let text = std::fs::read_to_string(path)?;
    let parsed = parse_changes(&text)?;
    let legacy = load_legacy_catalog(pool).await?;
    let mut tx = pool.begin().await?;
    let current: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT token FROM provider_checkpoints WHERE provider = 'apple_photos' AND scope_key = 'system'",
    ).fetch_optional(&mut *tx).await?.flatten();
    if current != parsed.checkpoint_before {
        return Err(AppError::Metadata(
            "Apple Photos checkpoint changed before incremental commit".into(),
        ));
    }
    let job_id: i64 = sqlx::query_scalar(
        "INSERT INTO sync_jobs (kind, provider, checkpoint_before, checkpoint_after, total_items) \
         VALUES ('apple_incremental', 'apple_photos', ?, ?, 0) RETURNING id",
    )
    .bind(&parsed.checkpoint_before)
    .bind(&parsed.checkpoint_after)
    .fetch_one(&mut *tx)
    .await?;
    let mut report = AppleChangesReport {
        changed_assets: parsed.assets.len(),
        removed_assets: parsed.removed.len(),
        exact_links: 0,
        review_candidates: 0,
        queued_assets: 0,
        job_id,
    };

    for asset in &parsed.assets {
        let metadata =
            serde_json::to_string(asset).map_err(|error| AppError::Metadata(error.to_string()))?;
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
             VALUES ('apple_photos', ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now')) \
             ON CONFLICT(provider, external_id) WHERE external_id IS NOT NULL DO UPDATE SET \
             original_filename = excluded.original_filename, media_type = excluded.media_type, \
             width = excluded.width, height = excluded.height, taken_at = excluded.taken_at, \
             metadata_json = excluded.metadata_json, exclusion_reason = excluded.exclusion_reason, \
             last_seen_at = datetime('now'), updated_at = datetime('now')",
        ).bind(&asset.local_identifier).bind(&asset.original_filename).bind(mime_type(asset))
        .bind(asset.pixel_width).bind(asset.pixel_height).bind(&asset.creation_date)
        .bind(metadata).bind(initial_status).bind(&asset.excluded_reason)
        .execute(&mut *tx).await?;
        let source_id: i64 = sqlx::query_scalar(
            "SELECT id FROM asset_sources WHERE provider = 'apple_photos' AND external_id = ?",
        )
        .bind(&asset.local_identifier)
        .fetch_one(&mut *tx)
        .await?;

        if asset.excluded_reason.is_some() {
            sqlx::query("UPDATE asset_sources SET sync_status = 'excluded' WHERE id = ?")
                .bind(source_id)
                .execute(&mut *tx)
                .await?;
            continue;
        }
        if let Some((_, Some(_), status)) = &existing {
            if matches!(
                status.as_str(),
                "queued" | "downloading" | "downloaded" | "importing" | "failed"
            ) {
                continue;
            }
            queue_change_item(
                &mut tx,
                job_id,
                source_id,
                &asset.local_identifier,
                "refresh_renditions",
                asset,
            )
            .await?;
            report.queued_assets += 1;
            continue;
        }

        if let Some(candidates) = legacy
            .names
            .get(&sanitized_identifier(&asset.local_identifier))
        {
            if candidates.len() == 1 {
                let photo_id = candidates[0];
                sqlx::query("INSERT OR IGNORE INTO assets (photo_id) VALUES (?)")
                    .bind(photo_id)
                    .execute(&mut *tx)
                    .await?;
                let asset_id: i64 = sqlx::query_scalar("SELECT id FROM assets WHERE photo_id = ?")
                    .bind(photo_id)
                    .fetch_one(&mut *tx)
                    .await?;
                sqlx::query(
                    "UPDATE asset_sources SET asset_id = ?, sync_status = 'ready' WHERE id = ?",
                )
                .bind(asset_id)
                .bind(source_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO asset_links (source_id, photo_id, method, confidence, status, evidence_json, reviewed_at) \
                     VALUES (?, ?, 'legacy_local_identifier', 1.0, 'accepted', ?, datetime('now')) \
                     ON CONFLICT(source_id, photo_id, method) DO UPDATE SET status = 'accepted', reviewed_at = datetime('now')",
                ).bind(source_id).bind(photo_id)
                .bind(serde_json::json!({"sanitized_local_identifier": sanitized_identifier(&asset.local_identifier)}).to_string())
                .execute(&mut *tx).await?;
                report.exact_links += 1;
                continue;
            }
        }
        let candidates = structured_candidates(asset, &legacy);
        if !candidates.is_empty() {
            report.review_candidates += candidates.len();
            for candidate in candidates {
                sqlx::query(
                    "INSERT INTO asset_links (source_id, photo_id, method, confidence, status, evidence_json) \
                     VALUES (?, ?, 'structured_metadata', ?, 'candidate', ?) \
                     ON CONFLICT(source_id, photo_id, method) DO UPDATE SET confidence = excluded.confidence, \
                     evidence_json = excluded.evidence_json WHERE asset_links.status = 'candidate'",
                ).bind(source_id).bind(candidate.photo_id).bind(candidate.confidence)
                .bind(candidate.evidence_json).execute(&mut *tx).await?;
            }
            sqlx::query("UPDATE asset_sources SET sync_status = 'discovered' WHERE id = ?")
                .bind(source_id)
                .execute(&mut *tx)
                .await?;
            continue;
        }
        queue_change_item(
            &mut tx,
            job_id,
            source_id,
            &asset.local_identifier,
            "export_original",
            asset,
        )
        .await?;
        report.queued_assets += 1;
    }

    for external_id in &parsed.removed {
        let source_id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM asset_sources WHERE provider = 'apple_photos' AND external_id = ?",
        )
        .bind(external_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(source_id) = source_id {
            sqlx::query("UPDATE asset_sources SET sync_status = 'missing', updated_at = datetime('now') WHERE id = ?")
                .bind(source_id).execute(&mut *tx).await?;
            sqlx::query(
                "UPDATE sync_items SET status = 'cancelled', lease_owner = NULL, lease_expires_at = NULL, \
                 finished_at = datetime('now'), updated_at = datetime('now') \
                 WHERE source_id = ? AND status IN ('queued', 'leased')",
            ).bind(source_id).execute(&mut *tx).await?;
        }
    }
    refresh_affected_jobs(&mut tx).await?;
    let queued = report.queued_assets as i64;
    sqlx::query(
        "UPDATE sync_jobs SET total_items = ?, status = CASE WHEN ? = 0 THEN 'completed' ELSE 'queued' END, \
         started_at = CASE WHEN ? = 0 THEN datetime('now') ELSE NULL END, \
         finished_at = CASE WHEN ? = 0 THEN datetime('now') ELSE NULL END, updated_at = datetime('now') WHERE id = ?",
    ).bind(queued).bind(queued).bind(queued).bind(queued).bind(job_id).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO provider_checkpoints (provider, scope_key, token, generation) \
         VALUES ('apple_photos', 'system', ?, 1) \
         ON CONFLICT(provider, scope_key) DO UPDATE SET token = excluded.token, \
         generation = provider_checkpoints.generation + 1, updated_at = datetime('now')",
    )
    .bind(&parsed.checkpoint_after)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(report)
}

async fn queue_change_item(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    job_id: i64,
    source_id: i64,
    external_id: &str,
    operation: &str,
    asset: &InventoryAsset,
) -> Result<()> {
    sqlx::query("UPDATE asset_sources SET sync_status = 'queued', last_error = NULL WHERE id = ?")
        .bind(source_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO sync_items (job_id, source_id, external_id, operation, payload_json) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(job_id)
    .bind(source_id)
    .bind(external_id)
    .bind(operation)
    .bind(serde_json::to_string(asset).map_err(|error| AppError::Metadata(error.to_string()))?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn refresh_affected_jobs(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<()> {
    let job_ids: Vec<i64> =
        sqlx::query_scalar("SELECT DISTINCT job_id FROM sync_items WHERE status = 'cancelled'")
            .fetch_all(&mut **tx)
            .await?;
    for job_id in job_ids {
        let (completed, failed, pending): (i64, i64, i64) = sqlx::query_as(
            "SELECT SUM(CASE WHEN status IN ('succeeded','excluded','cancelled') THEN 1 ELSE 0 END), \
             SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), \
             SUM(CASE WHEN status IN ('queued','leased') THEN 1 ELSE 0 END) \
             FROM sync_items WHERE job_id = ?",
        ).bind(job_id).fetch_one(&mut **tx).await?;
        let status = if pending > 0 {
            "running"
        } else if failed > 0 {
            "failed"
        } else {
            "completed"
        };
        sqlx::query(
            "UPDATE sync_jobs SET completed_items = ?, failed_items = ?, status = ?, \
             finished_at = CASE WHEN ? = 0 THEN datetime('now') ELSE NULL END, \
             updated_at = datetime('now') WHERE id = ? AND status != 'cancelled'",
        )
        .bind(completed)
        .bind(failed)
        .bind(status)
        .bind(pending)
        .bind(job_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
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

fn parse_changes(text: &str) -> Result<ParsedChanges> {
    let mut before = None;
    let mut after = None;
    let mut expected_assets = None;
    let mut expected_removed = None;
    let mut assets = Vec::new();
    let mut removed = Vec::new();
    let mut identities = HashSet::new();
    let mut saw_header = false;
    let mut saw_end = false;
    for (index, line) in text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| AppError::Metadata(format!("changes line {}: {error}", index + 1)))?;
        match value.get("record_type").and_then(|value| value.as_str()) {
            Some("changes_header") if !saw_header => {
                if value.get("schema_version").and_then(|value| value.as_u64()) != Some(1)
                    || value.get("mode").and_then(|value| value.as_str()) != Some("incremental")
                {
                    return Err(AppError::Metadata(
                        "unsupported Apple changes schema".into(),
                    ));
                }
                before = decode_checkpoint(value.get("checkpoint_before"))?;
                saw_header = true;
            }
            Some("asset") if saw_header && !saw_end => {
                let asset: InventoryAsset = serde_json::from_value(value).map_err(|error| {
                    AppError::Metadata(format!("invalid changed asset: {error}"))
                })?;
                if !identities.insert(asset.local_identifier.clone()) {
                    return Err(AppError::Metadata(format!(
                        "duplicate changed Apple asset {}",
                        asset.local_identifier
                    )));
                }
                assets.push(asset);
            }
            Some("removed_asset") if saw_header && !saw_end => {
                let identifier = value
                    .get("local_identifier")
                    .and_then(|value| value.as_str())
                    .filter(|identifier| !identifier.is_empty())
                    .ok_or_else(|| {
                        AppError::Metadata("removed Apple asset has no identity".into())
                    })?;
                if !identities.insert(identifier.to_owned()) {
                    return Err(AppError::Metadata(format!(
                        "duplicate Apple change {identifier}"
                    )));
                }
                removed.push(identifier.to_owned());
            }
            Some("changes_end") if saw_header && !saw_end => {
                after = decode_checkpoint(value.get("checkpoint_after"))?;
                expected_assets = value
                    .get("asset_count")
                    .and_then(|value| value.as_u64())
                    .map(|count| count as usize);
                expected_removed = value
                    .get("removed_count")
                    .and_then(|value| value.as_u64())
                    .map(|count| count as usize);
                saw_end = true;
            }
            _ => {
                return Err(AppError::Metadata(format!(
                    "invalid Apple changes sequence at line {}",
                    index + 1
                )));
            }
        }
    }
    if !saw_header
        || !saw_end
        || expected_assets != Some(assets.len())
        || expected_removed != Some(removed.len())
        || after.is_none()
    {
        return Err(AppError::Metadata(
            "Apple changes are incomplete or counts do not match".into(),
        ));
    }
    Ok(ParsedChanges {
        checkpoint_before: before,
        checkpoint_after: after,
        assets,
        removed,
    })
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

#[derive(Debug)]
struct LegacyPhotoEvidence {
    id: i64,
    filename: String,
    width: Option<i64>,
    height: Option<i64>,
    taken_at: Option<i64>,
}

#[derive(Debug, Default)]
struct LegacyCatalog {
    names: HashMap<String, Vec<i64>>,
    photos: Vec<LegacyPhotoEvidence>,
    by_filename: HashMap<String, Vec<usize>>,
    by_second: HashMap<i64, Vec<usize>>,
}

#[derive(Debug)]
struct StructuredCandidate {
    photo_id: i64,
    confidence: f64,
    evidence_json: String,
}

async fn load_legacy_catalog(pool: &SqlitePool) -> Result<LegacyCatalog> {
    let rows: Vec<(i64, String, Option<String>, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT id, path, taken_at, width, height FROM photos \
         WHERE import_status = 'imported' ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    let mut result = LegacyCatalog::default();
    for (id, path, taken_at, width, height) in rows {
        if let Some(stem) = PathBuf::from(&path).file_stem().and_then(|s| s.to_str()) {
            result
                .names
                .entry(stem.to_ascii_lowercase())
                .or_default()
                .push(id);
        }
        let filename = PathBuf::from(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&path)
            .to_ascii_lowercase();
        let taken_at = taken_at.as_deref().and_then(parse_timestamp);
        let index = result.photos.len();
        result.photos.push(LegacyPhotoEvidence {
            id,
            filename: filename.clone(),
            width,
            height,
            taken_at,
        });
        result.by_filename.entry(filename).or_default().push(index);
        if let Some(second) = taken_at {
            result.by_second.entry(second).or_default().push(index);
        }
    }
    Ok(result)
}

fn structured_candidates(
    asset: &InventoryAsset,
    legacy: &LegacyCatalog,
) -> Vec<StructuredCandidate> {
    let filename = asset
        .original_filename
        .as_deref()
        .map(str::to_ascii_lowercase);
    let taken_at = asset.creation_date.as_deref().and_then(parse_timestamp);
    let mut indices: HashSet<usize> = HashSet::new();
    if let Some(filename) = &filename {
        if let Some(found) = legacy.by_filename.get(filename) {
            indices.extend(found.iter().copied());
        }
    }
    if let Some(second) = taken_at {
        for offset in -2..=2 {
            if let Some(found) = legacy.by_second.get(&(second + offset)) {
                indices.extend(found.iter().copied());
            }
        }
    }
    let mut candidates = Vec::new();
    for index in indices {
        let photo = &legacy.photos[index];
        let filename_match = filename.as_deref() == Some(photo.filename.as_str());
        let dimension_match =
            photo.width == Some(asset.pixel_width) && photo.height == Some(asset.pixel_height);
        let rotated_dimension_match =
            photo.width == Some(asset.pixel_height) && photo.height == Some(asset.pixel_width);
        let time_delta = taken_at.zip(photo.taken_at).map(|(a, b)| (a - b).abs());
        let mut confidence = if filename_match { 0.5 } else { 0.0 };
        if dimension_match {
            confidence += 0.2;
        } else if rotated_dimension_match {
            confidence += 0.15;
        }
        if matches!(time_delta, Some(0..=2)) {
            confidence += 0.3;
        }
        if confidence < 0.65 {
            continue;
        }
        let evidence_json = serde_json::json!({
            "filename_match": filename_match,
            "dimension_match": dimension_match,
            "rotated_dimension_match": rotated_dimension_match,
            "time_delta_seconds": time_delta,
        })
        .to_string();
        candidates.push(StructuredCandidate {
            photo_id: photo.id,
            confidence,
            evidence_json,
        });
    }
    candidates.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then(a.photo_id.cmp(&b.photo_id))
    });
    candidates
}

fn parse_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.timestamp())
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.and_utc().timestamp())
                .ok()
        })
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

    fn changes(before: &str, after: &str, assets: &[String], removed: &[&str]) -> String {
        let mut lines = vec![
            serde_json::json!({
                "record_type":"changes_header", "schema_version":1, "mode":"incremental",
                "checkpoint_before":before, "checkpoint_after":null
            })
            .to_string(),
        ];
        lines.extend_from_slice(assets);
        for identifier in removed {
            lines.push(
                serde_json::json!({
                    "record_type":"removed_asset", "schema_version":1,
                    "local_identifier":identifier
                })
                .to_string(),
            );
        }
        lines.push(
            serde_json::json!({
                "record_type":"changes_end", "schema_version":1, "mode":"incremental",
                "checkpoint_before":null, "checkpoint_after":after,
                "asset_count":assets.len(), "removed_count":removed.len()
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

    #[tokio::test]
    async fn structured_evidence_creates_review_candidate_without_auto_link_or_download() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status, taken_at, width, height) \
             VALUES ('/library/IMG_0099.HEIC', 'sha-99', 'heic', 'imported', \
                     '2026-08-15 01:02:03', 4032, 3024)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            temp.path(),
            inventory(&[asset("unlinked/L0/099", "IMG_0099.HEIC", None)]),
        )
        .unwrap();

        let report = ingest_inventory(&pool, temp.path(), false).await.unwrap();
        assert_eq!(report.review_candidates, 1);
        assert_eq!(report.queued_assets, 0);
        let source: (String, Option<i64>) = sqlx::query_as(
            "SELECT sync_status, asset_id FROM asset_sources WHERE provider = 'apple_photos'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(source.0, "discovered");
        assert!(source.1.is_none());
        let link: (String, String, f64) =
            sqlx::query_as("SELECT method, status, confidence FROM asset_links")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(link.0, "structured_metadata");
        assert_eq!(link.1, "candidate");
        assert_eq!(link.2, 1.0);
        let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(items, 0);
    }

    #[tokio::test]
    async fn incremental_changes_persist_work_and_removals_before_advancing_token() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO provider_checkpoints (provider, scope_key, token) VALUES ('apple_photos', 'system', ?)")
            .bind(b"old".as_slice()).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) \
             VALUES (1, '/linked.jpg', 'linked', 'jpeg', 'imported')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO assets (id, photo_id) VALUES (1, 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO asset_sources (id, asset_id, provider, external_id, sync_status) \
             VALUES (1, 1, 'apple_photos', 'updated/L0/001', 'ready'), \
                    (2, NULL, 'apple_photos', 'removed/L0/002', 'queued')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let old_job: i64 = sqlx::query_scalar(
            "INSERT INTO sync_jobs (kind, provider, total_items) \
             VALUES ('old', 'apple_photos', 1) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sync_items (job_id, source_id, external_id, operation) \
             VALUES (?, 2, 'removed/L0/002', 'export_original')",
        )
        .bind(old_job)
        .execute(&pool)
        .await
        .unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            changes(
                "b2xk",
                "bmV3",
                &[
                    asset("updated/L0/001", "linked.jpg", None),
                    asset("new/L0/003", "new.heic", None),
                ],
                &["removed/L0/002"],
            ),
        )
        .unwrap();

        let report = ingest_changes(&pool, file.path()).await.unwrap();
        assert_eq!(report.queued_assets, 2);
        assert_eq!(report.removed_assets, 1);
        let operations: Vec<String> =
            sqlx::query_scalar("SELECT operation FROM sync_items WHERE job_id = ? ORDER BY id")
                .bind(report.job_id)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(operations, vec!["refresh_renditions", "export_original"]);
        let removed_status: String =
            sqlx::query_scalar("SELECT sync_status FROM asset_sources WHERE id = 2")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(removed_status, "missing");
        let old_item: String = sqlx::query_scalar("SELECT status FROM sync_items WHERE job_id = ?")
            .bind(old_job)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(old_item, "cancelled");
        let token: Vec<u8> = sqlx::query_scalar("SELECT token FROM provider_checkpoints")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(token, b"new");
    }

    #[tokio::test]
    async fn stale_or_incomplete_incremental_changes_write_nothing() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO provider_checkpoints (provider, scope_key, token) VALUES ('apple_photos', 'system', ?)")
            .bind(b"current".as_slice()).execute(&pool).await.unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            changes(
                "c3RhbGU=",
                "bmV3",
                &[asset("new/L0/001", "new.heic", None)],
                &[],
            ),
        )
        .unwrap();
        assert!(ingest_changes(&pool, file.path()).await.is_err());
        let sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM asset_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_jobs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((sources, jobs), (0, 0));
        let partial = r#"{"record_type":"changes_header","schema_version":1,"mode":"incremental","checkpoint_before":"Y3VycmVudA=="}"#;
        assert!(parse_changes(partial).is_err());
    }
}
