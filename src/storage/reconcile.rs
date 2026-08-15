use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sqlx::SqlitePool;

use crate::config::Config;
use crate::error::Result;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReconciliationReport {
    pub missing_photo_files: usize,
    pub missing_variant_files: usize,
    pub invalid_master_pointers: i64,
    pub invalid_display_pointers: i64,
    pub incomplete_filesystem_intents: usize,
    pub recovered_filesystem_intents: usize,
    pub failed_filesystem_intents: usize,
    pub missing_ready_thumbnails: usize,
    pub stale_cache_files: usize,
    pub repaired_records: usize,
    pub removed_cache_files: usize,
}

pub async fn reconcile(
    pool: &SqlitePool,
    config: &Config,
    repair: bool,
) -> Result<ReconciliationReport> {
    let mut report = ReconciliationReport::default();
    let photo_paths: Vec<String> =
        sqlx::query_scalar("SELECT path FROM photos WHERE import_status = 'imported' ORDER BY id")
            .fetch_all(pool)
            .await?;
    report.missing_photo_files = photo_paths
        .iter()
        .filter(|path| !Path::new(path).is_file())
        .count();
    let variant_paths: Vec<String> =
        sqlx::query_scalar("SELECT path FROM asset_variants WHERE path IS NOT NULL ORDER BY id")
            .fetch_all(pool)
            .await?;
    report.missing_variant_files = variant_paths
        .iter()
        .filter(|path| !Path::new(path).is_file())
        .count();
    report.invalid_master_pointers = sqlx::query_scalar(
        "SELECT COUNT(*) FROM assets a JOIN asset_variants v ON v.id = a.master_variant_id \
         WHERE v.asset_id != a.id",
    )
    .fetch_one(pool)
    .await?;
    report.invalid_display_pointers = sqlx::query_scalar(
        "SELECT COUNT(*) FROM assets a JOIN asset_variants v ON v.id = a.display_variant_id \
         WHERE v.asset_id != a.id",
    )
    .fetch_one(pool)
    .await?;
    if repair {
        let master = sqlx::query(
            "UPDATE assets SET master_variant_id = NULL \
             WHERE master_variant_id IN (SELECT v.id FROM asset_variants v WHERE v.asset_id != assets.id)",
        )
        .execute(pool)
        .await?
        .rows_affected();
        let display = sqlx::query(
            "UPDATE assets SET display_variant_id = NULL \
             WHERE display_variant_id IN (SELECT v.id FROM asset_variants v WHERE v.asset_id != assets.id)",
        )
        .execute(pool)
        .await?
        .rows_affected();
        report.repaired_records += (master + display) as usize;
    }

    reconcile_intents(pool, repair, &mut report).await?;
    reconcile_derived(pool, &config.thumb_cache_dir, repair, &mut report).await?;
    Ok(report)
}

async fn reconcile_intents(
    pool: &SqlitePool,
    repair: bool,
    report: &mut ReconciliationReport,
) -> Result<()> {
    let intents: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, target_path, staging_path FROM filesystem_intents \
         WHERE status IN ('planned', 'staged') ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    report.incomplete_filesystem_intents = intents.len();
    if !repair {
        return Ok(());
    }
    for (id, target, staging) in intents {
        let target = PathBuf::from(target);
        let staging = PathBuf::from(staging);
        if target.is_file() {
            mark_intent(pool, id, "committed", None).await?;
            report.recovered_filesystem_intents += 1;
            report.repaired_records += 1;
        } else if staging.is_file() {
            match std::fs::rename(&staging, &target) {
                Ok(()) => {
                    mark_intent(pool, id, "committed", None).await?;
                    report.recovered_filesystem_intents += 1;
                    report.repaired_records += 1;
                }
                Err(error) => {
                    mark_intent(pool, id, "failed", Some(&error.to_string())).await?;
                    report.failed_filesystem_intents += 1;
                }
            }
        } else {
            mark_intent(
                pool,
                id,
                "failed",
                Some("staging and target files are missing"),
            )
            .await?;
            report.failed_filesystem_intents += 1;
        }
    }
    Ok(())
}

async fn mark_intent(pool: &SqlitePool, id: i64, status: &str, error: Option<&str>) -> Result<()> {
    sqlx::query(
        "UPDATE filesystem_intents SET status = ?, error = ?, \
         completed_at = CASE WHEN ? = 'committed' THEN datetime('now') ELSE completed_at END, \
         updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(error)
    .bind(status)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn reconcile_derived(
    pool: &SqlitePool,
    cache_dir: &Path,
    repair: bool,
    report: &mut ReconciliationReport,
) -> Result<()> {
    let revisions: Vec<(i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT p.id, p.render_revision, d.thumbnail_status FROM photos p \
         LEFT JOIN derived_media_state d ON d.photo_id = p.id \
         WHERE p.import_status = 'imported'",
    )
    .fetch_all(pool)
    .await?;
    let revision_map: HashMap<i64, i64> = revisions
        .iter()
        .map(|(photo_id, revision, _)| (*photo_id, *revision))
        .collect();
    let mut missing_ready = Vec::new();
    for (photo_id, revision, status) in &revisions {
        if status.as_deref() == Some("ready")
            && !crate::derived::thumbnail_cache_path(cache_dir, *photo_id, *revision).is_file()
        {
            missing_ready.push(*photo_id);
        }
    }
    report.missing_ready_thumbnails = missing_ready.len();
    if repair && !missing_ready.is_empty() {
        for photo_id in &missing_ready {
            report.repaired_records += sqlx::query(
                "UPDATE derived_media_state SET thumbnail_status = 'pending', thumbnail_at = NULL, \
                 updated_at = datetime('now') WHERE photo_id = ?",
            )
            .bind(photo_id)
            .execute(pool)
            .await?
            .rows_affected() as usize;
        }
    }
    if !cache_dir.exists() {
        return Ok(());
    }
    let valid_ids: HashSet<i64> = revision_map.keys().copied().collect();
    for entry in std::fs::read_dir(cache_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let stale = if name.contains(".job-") {
            true
        } else if name.starts_with("face_") {
            false
        } else {
            cache_identity(name).is_some_and(|(photo_id, revision)| {
                !valid_ids.contains(&photo_id)
                    || revision_map.get(&photo_id).copied() != Some(revision)
            })
        };
        if stale {
            report.stale_cache_files += 1;
            if repair {
                std::fs::remove_file(&path)?;
                report.removed_cache_files += 1;
            }
        }
    }
    Ok(())
}

fn cache_identity(name: &str) -> Option<(i64, i64)> {
    let stem = name.strip_suffix(".jpg")?;
    let (id, suffix) = stem.split_once('_').unwrap_or((stem, ""));
    let photo_id = id.parse().ok()?;
    let revision = suffix
        .strip_prefix('r')
        .and_then(|suffix| suffix.split('_').next())
        .map_or(Some(0), |value| value.parse().ok())?;
    Some((photo_id, revision))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn repair_recovers_intents_and_resets_missing_derived_cache() {
        let directory = tempfile::tempdir().unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let media = directory.path().join("photo.jpg");
        std::fs::write(&media, b"photo").unwrap();
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status, render_revision) \
             VALUES (1, ?, 'reconcile-photo', 'jpeg', 'imported', 2)",
        )
        .bind(media.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO derived_media_state \
             (photo_id, render_revision, thumbnail_status, face_status) \
             VALUES (1, 2, 'ready', 'ready')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let staging = directory.path().join("staged.jpg");
        let target = directory.path().join("target.jpg");
        std::fs::write(&staging, b"staged").unwrap();
        sqlx::query(
            "INSERT INTO filesystem_intents (kind, target_path, staging_path, status) \
             VALUES ('thumbnail', ?, ?, 'staged')",
        )
        .bind(target.to_string_lossy().as_ref())
        .bind(staging.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
        let mut config = Config::default();
        config.thumb_cache_dir = directory.path().join("cache");
        std::fs::create_dir_all(&config.thumb_cache_dir).unwrap();
        std::fs::write(config.thumb_cache_dir.join("1_r1.jpg"), b"stale").unwrap();

        let report = reconcile(&pool, &config, true).await.unwrap();
        assert_eq!(report.missing_ready_thumbnails, 1);
        assert_eq!(report.recovered_filesystem_intents, 1);
        assert_eq!(report.removed_cache_files, 1);
        assert!(target.is_file());
        let status: String = sqlx::query_scalar(
            "SELECT thumbnail_status FROM derived_media_state WHERE photo_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "pending");
    }
}
