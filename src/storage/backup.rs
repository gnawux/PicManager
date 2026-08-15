use std::path::{Path, PathBuf};

use serde::Serialize;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct BackupReport {
    pub path: PathBuf,
    pub bytes: u64,
    pub integrity: String,
}

pub async fn create_backup(
    pool: &SqlitePool,
    directory: &Path,
    retention: usize,
) -> Result<BackupReport> {
    std::fs::create_dir_all(directory)?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let final_path = directory.join(format!("picmanager-{stamp}.db"));
    let temporary = directory.join(format!(".picmanager-{stamp}.pending"));
    if temporary.exists() || final_path.exists() {
        return Err(AppError::Metadata("backup filename collision".into()));
    }
    let path = temporary.to_string_lossy().into_owned();
    if let Err(error) = sqlx::query("VACUUM INTO ?").bind(&path).execute(pool).await {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    let integrity = match verify_backup(&temporary).await {
        Ok(report) => report.integrity,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
    };
    std::fs::rename(&temporary, &final_path)?;
    prune_backups(directory, retention.max(1), Some(&final_path))?;
    Ok(BackupReport {
        bytes: std::fs::metadata(&final_path)?.len(),
        path: final_path,
        integrity,
    })
}

pub async fn verify_backup(path: &Path) -> Result<BackupReport> {
    if !path.is_file() {
        return Err(AppError::NotFound(format!("backup {}", path.display())));
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await?;
    if integrity != "ok" {
        return Err(AppError::Metadata(format!(
            "backup integrity check failed: {integrity}"
        )));
    }
    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await?;
    pool.close().await;
    Ok(BackupReport {
        path: path.to_path_buf(),
        bytes: std::fs::metadata(path)?.len(),
        integrity,
    })
}

pub async fn restore_backup(backup: &Path, target: &Path) -> Result<BackupReport> {
    verify_backup(backup).await?;
    if target.exists() {
        return Err(AppError::Metadata(format!(
            "restore target already exists: {}",
            target.display()
        )));
    }
    let parent = target
        .parent()
        .ok_or_else(|| AppError::Metadata("restore target has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.restore-pending",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    if temporary.exists() {
        return Err(AppError::Metadata(format!(
            "restore staging file already exists: {}",
            temporary.display()
        )));
    }
    std::fs::copy(backup, &temporary)?;
    if let Err(error) = verify_backup(&temporary).await {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, target)?;
    verify_backup(target).await
}

pub fn list_backups(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut backups = std::fs::read_dir(directory)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("picmanager-") && name.ends_with(".db"))
        })
        .collect::<Vec<_>>();
    backups.sort();
    backups.reverse();
    Ok(backups)
}

fn prune_backups(directory: &Path, retention: usize, protected: Option<&Path>) -> Result<()> {
    for path in list_backups(directory)?.into_iter().skip(retention) {
        if protected != Some(path.as_path()) {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::connect_with_settings;
    use std::time::Duration;

    #[tokio::test]
    async fn backup_is_verified_retained_and_restorable() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.db");
        let url = format!("sqlite:{}", source.display());
        let pool = connect_with_settings(&url, 2, Duration::from_secs(2))
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status) \
             VALUES ('photo.jpg', 'backup-photo', 'jpeg', 'imported')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let backups = directory.path().join("backups");
        let first = create_backup(&pool, &backups, 1).await.unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
        let second = create_backup(&pool, &backups, 1).await.unwrap();
        assert_eq!(first.integrity, "ok");
        assert_eq!(list_backups(&backups).unwrap(), vec![second.path.clone()]);

        let restored = directory.path().join("restored.db");
        let report = restore_backup(&second.path, &restored).await.unwrap();
        assert_eq!(report.integrity, "ok");
        assert!(restore_backup(&second.path, &restored).await.is_err());
        let restored_url = format!("sqlite:{}", restored.display());
        let restored_pool = connect_with_settings(&restored_url, 1, Duration::from_secs(2))
            .await
            .unwrap();
        let photos: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photos")
            .fetch_one(&restored_pool)
            .await
            .unwrap();
        assert_eq!(photos, 1);
    }
}
