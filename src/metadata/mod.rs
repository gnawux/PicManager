pub mod exif;
pub mod filename;
pub mod format;
pub mod types;

pub use exif::extract_from_file;
pub use exif::read_timezone_offset;
pub use filename::infer_date;
pub use types::{ImageFormat, PhotoMeta};

use std::path::Path;
use chrono::NaiveDateTime;

/// Update `timezone_offset` for all photos that currently have it as NULL.
/// Reads EXIF from each file and writes the offset back to the DB.
/// Returns `(updated, no_exif_tz, file_missing)`.
pub async fn backfill_timezones(
    pool: &sqlx::SqlitePool,
    dry_run: bool,
) -> crate::error::Result<(usize, usize, usize)> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, path FROM photos WHERE timezone_offset IS NULL AND import_status = 'imported'",
    )
    .fetch_all(pool)
    .await
    .map_err(crate::error::AppError::Database)?;

    let (mut updated, mut no_tz, mut missing) = (0usize, 0usize, 0usize);
    for (id, path_str) in rows {
        let path = Path::new(&path_str);
        if !path.exists() {
            missing += 1;
            continue;
        }
        match read_timezone_offset(path) {
            Some(offset) => {
                if !dry_run {
                    sqlx::query("UPDATE photos SET timezone_offset = ? WHERE id = ?")
                        .bind(offset)
                        .bind(id)
                        .execute(pool)
                        .await
                        .map_err(crate::error::AppError::Database)?;
                }
                updated += 1;
            }
            None => no_tz += 1,
        }
    }
    Ok((updated, no_tz, missing))
}

/// Returns the file modification time as a `NaiveDateTime`, or `None` if unavailable.
pub fn mtime_to_naive_datetime(path: &Path) -> Option<NaiveDateTime> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    chrono::DateTime::from_timestamp(secs as i64, 0).map(|dt| dt.naive_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtime_to_naive_datetime_returns_none_for_missing_file() {
        let result = mtime_to_naive_datetime(Path::new("/nonexistent/path/file.jpg"));
        assert!(result.is_none());
    }

    #[test]
    fn mtime_to_naive_datetime_returns_date_for_existing_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        // Set known mtime: 2023-03-15 00:00:00 UTC = 1678838400
        let known = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_678_838_400);
        filetime::set_file_mtime(tmp.path(), filetime::FileTime::from_system_time(known)).unwrap();

        let result = mtime_to_naive_datetime(tmp.path()).unwrap();
        assert_eq!(result.and_utc().timestamp(), 1_678_838_400);
    }
}
