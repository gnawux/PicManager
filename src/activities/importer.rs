use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::io::Read;
use std::path::{Path, PathBuf};
use super::parser::{parse_fit, parse_gpx};

#[derive(Debug, Default)]
pub struct ImportSummary {
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub enum ImportOutcome {
    Imported(i64),
    Skipped,
}

pub fn scan_dir(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file() && {
                let ext = e.path().extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_lowercase());
                matches!(ext.as_deref(), Some("fit") | Some("gpx"))
            }
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub async fn import_one(
    pool: &SqlitePool,
    path: &Path,
    activities_dir: &Path,
) -> Result<ImportOutcome> {
    let sha256 = compute_sha256(path)?;

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM activities WHERE sha256 = ?)",
    )
    .bind(&sha256)
    .fetch_one(pool)
    .await?;

    if exists {
        return Ok(ImportOutcome::Skipped);
    }

    let ext = path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let data = match ext.as_str() {
        "fit" => parse_fit(path)?,
        "gpx" => parse_gpx(path)?,
        _ => anyhow::bail!("unsupported format: {ext}"),
    };

    let year = data.start_time
        .map(|t| t.format("%Y").to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let dest_dir = activities_dir.join(&year);
    std::fs::create_dir_all(&dest_dir)?;

    let filename = path.file_name().unwrap_or_default();
    let dest_path = dest_dir.join(filename);
    let dest_path = if dest_path.exists() {
        let sha_existing = compute_sha256(&dest_path).unwrap_or_default();
        if sha_existing == sha256 {
            dest_path
        } else {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("activity");
            dest_dir.join(format!("{}_{}.{ext}", stem, &sha256[..8]))
        }
    } else {
        dest_path
    };

    std::fs::copy(path, &dest_path)?;

    let source_path = dest_path.to_string_lossy().to_string();
    let start_str = data.start_time.map(|t| t.to_rfc3339());
    let end_str = data.end_time.map(|t| t.to_rfc3339());

    let activity_id: i64 = sqlx::query_scalar(
        "INSERT INTO activities \
         (sha256, source_path, file_format, title, activity_type, \
          start_time, end_time, duration_seconds, distance_meters, elevation_gain_meters, \
          avg_heart_rate, max_heart_rate, calories, device, import_status) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,'imported') RETURNING id",
    )
    .bind(&sha256)
    .bind(&source_path)
    .bind(&data.file_format)
    .bind(&data.title)
    .bind(&data.activity_type)
    .bind(&start_str)
    .bind(&end_str)
    .bind(data.duration_seconds)
    .bind(data.distance_meters)
    .bind(data.elevation_gain_meters)
    .bind(data.avg_heart_rate)
    .bind(data.max_heart_rate)
    .bind(data.calories)
    .bind(&data.device)
    .fetch_one(pool)
    .await?;

    // Insert track points in batches of 500
    for chunk in data.track_points.chunks(500) {
        let placeholders = chunk.iter().map(|_| "(?,?,?,?,?,?,?,?)").collect::<Vec<_>>().join(",");
        let sql = format!(
            "INSERT INTO activity_track_points (activity_id, ts, lat, lon, elevation, heart_rate, cadence, speed) VALUES {placeholders}"
        );
        let mut q = sqlx::query(&sql);
        for pt in chunk {
            q = q
                .bind(activity_id)
                .bind(pt.ts.to_rfc3339())
                .bind(pt.lat)
                .bind(pt.lon)
                .bind(pt.elevation)
                .bind(pt.heart_rate)
                .bind(pt.cadence)
                .bind(pt.speed);
        }
        q.execute(pool).await?;
    }

    // Generate title if FIT/GPX didn't provide one
    if data.title.is_none() {
        let first_gps = data.track_points.first().map(|p| (p.lat, p.lon));
        if let Some(title) = generate_title(
            pool, &data.activity_type, start_str.as_deref(), data.distance_meters, first_gps,
        ).await {
            sqlx::query("UPDATE activities SET title = ? WHERE id = ?")
                .bind(&title)
                .bind(activity_id)
                .execute(pool)
                .await
                .ok();
        }
    }

    Ok(ImportOutcome::Imported(activity_id))
}

pub(crate) fn activity_type_zh(ty: &str) -> &str {
    match ty {
        "running"       => "跑步",
        "hiking"        => "徒步",
        "cycling"       => "骑行",
        "walking"       => "步行",
        "trail_running" => "越野跑",
        "swimming"      => "游泳",
        _               => "运动",
    }
}

pub(crate) fn format_distance(meters: f64) -> String {
    if meters < 1000.0 {
        format!("{:.0}m", meters)
    } else {
        format!("{:.1}km", meters / 1000.0)
    }
}

pub(crate) async fn geocache_city(pool: &SqlitePool, lat: f64, lon: f64) -> Option<String> {
    let row: (Option<String>, Option<String>, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT city, county, state, country FROM geocache
             WHERE CAST(lat_key AS REAL) BETWEEN ? AND ?
               AND CAST(lon_key AS REAL) BETWEEN ? AND ?
             ORDER BY
               (CAST(lat_key AS REAL) - ?) * (CAST(lat_key AS REAL) - ?) +
               (CAST(lon_key AS REAL) - ?) * (CAST(lon_key AS REAL) - ?) ASC
             LIMIT 1",
        )
        .bind(lat - 0.01).bind(lat + 0.01)
        .bind(lon - 0.01).bind(lon + 0.01)
        .bind(lat).bind(lat).bind(lon).bind(lon)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()?;

    let (city, county, state, country) = row;
    city.filter(|s| !s.is_empty())
        .or_else(|| county.filter(|s| !s.is_empty()))
        .or_else(|| state.filter(|s| !s.is_empty()))
        .or_else(|| country.filter(|s| !s.is_empty()))
}

pub(crate) async fn generate_title(
    pool: &SqlitePool,
    activity_type: &str,
    start_time: Option<&str>,
    distance_meters: Option<f64>,
    first_gps: Option<(f64, f64)>,
) -> Option<String> {
    let type_label = activity_type_zh(activity_type);
    let date = start_time
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.format("%m-%d").to_string())?;

    let dist = distance_meters.map(format_distance);
    let city = if let Some((lat, lon)) = first_gps {
        geocache_city(pool, lat, lon).await
    } else {
        None
    };

    let title = match (dist.as_deref(), city.as_deref()) {
        (Some(d), Some(c)) => format!("{type_label}-{date}-{d}@{c}"),
        (Some(d), None)    => format!("{type_label}-{date}-{d}"),
        (None, Some(c))    => format!("{type_label}-{date}@{c}"),
        (None, None)       => format!("{type_label}-{date}"),
    };
    Some(title)
}

/// Update titles for activities that have `title IS NULL`.
/// Returns `(updated, skipped_no_date)`.
pub async fn update_titles(pool: &SqlitePool, dry_run: bool) -> (usize, usize) {
    let rows: Vec<(i64, String, Option<String>, Option<f64>)> = sqlx::query_as(
        "SELECT id, activity_type, start_time, distance_meters
         FROM activities
         WHERE (title IS NULL OR title = 'activity') AND import_status = 'imported'",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let (mut updated, mut skipped) = (0usize, 0usize);
    for (id, activity_type, start_time, distance_meters) in rows {
        let first_gps: Option<(f64, f64)> = sqlx::query_as(
            "SELECT lat, lon FROM activity_track_points WHERE activity_id = ? ORDER BY ts LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .unwrap_or_default();

        match generate_title(pool, &activity_type, start_time.as_deref(), distance_meters, first_gps).await {
            Some(title) => {
                if !dry_run {
                    sqlx::query("UPDATE activities SET title = ? WHERE id = ?")
                        .bind(&title)
                        .bind(id)
                        .execute(pool)
                        .await
                        .ok();
                }
                updated += 1;
            }
            None => skipped += 1,
        }
    }
    (updated, skipped)
}

pub async fn import_dir_activities(
    pool: &SqlitePool,
    dir: &Path,
    activities_dir: &Path,
    dry_run: bool,
) -> ImportSummary {
    let files = scan_dir(dir);
    let total = files.len();
    let mut summary = ImportSummary { total, ..Default::default() };

    if dry_run {
        return summary;
    }

    for path in files {
        match import_one(pool, &path, activities_dir).await {
            Ok(ImportOutcome::Imported(_)) => summary.imported += 1,
            Ok(ImportOutcome::Skipped) => summary.skipped += 1,
            Err(e) => {
                tracing::warn!("Failed to import {:?}: {e}", path);
                summary.failed += 1;
            }
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::TempDir;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    fn make_gpx(filename: &str, dir: &TempDir) -> PathBuf {
        make_gpx_with_date(filename, dir, "2024-06-15T10:00:00Z", "2024-06-15T10:10:00Z")
    }

    fn make_gpx_with_date(filename: &str, dir: &TempDir, t1: &str, t2: &str) -> PathBuf {
        let path = dir.path().join(filename);
        let content = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><name>{filename}</name><type>running</type><trkseg>
    <trkpt lat="39.9" lon="116.4"><ele>50</ele><time>{t1}</time></trkpt>
    <trkpt lat="39.91" lon="116.41"><ele>55</ele><time>{t2}</time></trkpt>
  </trkseg></trk>
</gpx>"#);
        std::fs::write(&path, content.as_bytes()).unwrap();
        path
    }

    /// GPX without <name>, so auto-title generation kicks in.
    fn make_gpx_untitled(filename: &str, dir: &TempDir, t1: &str, t2: &str) -> PathBuf {
        let path = dir.path().join(filename);
        let content = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><type>running</type><trkseg>
    <trkpt lat="39.9" lon="116.4"><ele>50</ele><time>{t1}</time></trkpt>
    <trkpt lat="39.91" lon="116.41"><ele>55</ele><time>{t2}</time></trkpt>
  </trkseg></trk>
</gpx>"#);
        std::fs::write(&path, content.as_bytes()).unwrap();
        path
    }

    #[tokio::test]
    async fn import_gpx_creates_activity() {
        let pool = test_pool().await;
        let src_dir = TempDir::new().unwrap();
        let act_dir = TempDir::new().unwrap();
        let gpx_path = make_gpx("run.gpx", &src_dir);

        let outcome = import_one(&pool, &gpx_path, act_dir.path()).await.unwrap();
        assert!(matches!(outcome, ImportOutcome::Imported(_)));

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn import_dedup_skips_same_file() {
        let pool = test_pool().await;
        let src_dir = TempDir::new().unwrap();
        let act_dir = TempDir::new().unwrap();
        let gpx_path = make_gpx("run.gpx", &src_dir);

        import_one(&pool, &gpx_path, act_dir.path()).await.unwrap();
        let outcome = import_one(&pool, &gpx_path, act_dir.path()).await.unwrap();
        assert!(matches!(outcome, ImportOutcome::Skipped));

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn import_dir_counts_correctly() {
        let pool = test_pool().await;
        let src_dir = TempDir::new().unwrap();
        let act_dir = TempDir::new().unwrap();

        make_gpx_with_date("a.gpx", &src_dir, "2024-06-15T10:00:00Z", "2024-06-15T10:10:00Z");
        make_gpx_with_date("b.gpx", &src_dir, "2024-06-16T08:00:00Z", "2024-06-16T08:30:00Z");
        // write a non-activity file that should be ignored
        std::fs::write(src_dir.path().join("notes.txt"), "ignore me").unwrap();

        let summary = import_dir_activities(
            &pool, src_dir.path(), act_dir.path(), false,
        ).await;

        assert_eq!(summary.total, 2);
        assert_eq!(summary.imported, 2);
        assert_eq!(summary.failed, 0);
    }

    #[tokio::test]
    async fn import_dir_dry_run_imports_nothing() {
        let pool = test_pool().await;
        let src_dir = TempDir::new().unwrap();
        let act_dir = TempDir::new().unwrap();
        make_gpx("run.gpx", &src_dir);

        let summary = import_dir_activities(
            &pool, src_dir.path(), act_dir.path(), true,
        ).await;

        assert_eq!(summary.total, 1);
        assert_eq!(summary.imported, 0);

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
    }

    async fn insert_geocache(pool: &SqlitePool, lat: f64, lon: f64, city: &str, county: &str, state: &str, country: &str) {
        let lat_key = format!("{:.4}", lat);
        let lon_key = format!("{:.4}", lon);
        sqlx::query(
            "INSERT OR IGNORE INTO geocache (lat_key, lon_key, city, county, state, country) VALUES (?,?,?,?,?,?)"
        )
        .bind(lat_key).bind(lon_key).bind(city).bind(county).bind(state).bind(country)
        .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn title_generated_on_import_with_geocache() {
        let pool = test_pool().await;
        let src_dir = TempDir::new().unwrap();
        let act_dir = TempDir::new().unwrap();

        // Insert geocache for Beijing (near GPX track point 39.9, 116.4)
        insert_geocache(&pool, 39.9, 116.4, "北京", "", "北京市", "中国").await;

        let gpx_path = make_gpx_untitled("run.gpx", &src_dir,
            "2024-06-15T02:00:00Z", "2024-06-15T03:00:00Z");
        import_one(&pool, &gpx_path, act_dir.path()).await.unwrap();

        let title: Option<String> = sqlx::query_scalar("SELECT title FROM activities")
            .fetch_one(&pool).await.unwrap();
        let title = title.expect("title should be generated");
        assert!(title.starts_with("跑步-06-15-"), "expected 跑步-06-15-…, got {title}");
        assert!(title.ends_with("@北京"), "expected @北京, got {title}");
    }

    #[tokio::test]
    async fn title_generated_no_gps_no_city() {
        let pool = test_pool().await;
        let src_dir = TempDir::new().unwrap();
        let act_dir = TempDir::new().unwrap();

        // GPX with no track points → no GPS → no city
        let path = src_dir.path().join("no_gps.gpx");
        std::fs::write(&path, r#"<?xml version="1.0"?>
<gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><type>hiking</type><trkseg></trkseg></trk>
</gpx>"#).unwrap();
        import_one(&pool, &path, act_dir.path()).await.unwrap();

        let title: Option<String> = sqlx::query_scalar("SELECT title FROM activities")
            .fetch_one(&pool).await.unwrap();
        // No date either (no track points → no start_time), title may be NULL
        // At minimum should not panic
        let _ = title;
    }

    #[tokio::test]
    async fn title_city_fallback_uses_county_when_city_empty() {
        let pool = test_pool().await;
        let src_dir = TempDir::new().unwrap();
        let act_dir = TempDir::new().unwrap();

        // city is empty → should fall back to county
        insert_geocache(&pool, 39.9, 116.4, "", "朝阳区", "北京市", "中国").await;

        let gpx_path = make_gpx_untitled("run2.gpx", &src_dir,
            "2024-06-15T02:00:00Z", "2024-06-15T03:00:00Z");
        import_one(&pool, &gpx_path, act_dir.path()).await.unwrap();

        let title: Option<String> = sqlx::query_scalar("SELECT title FROM activities")
            .fetch_one(&pool).await.unwrap();
        let title = title.expect("title should be generated");
        assert!(title.ends_with("@朝阳区"), "should use county as fallback, got {title}");
    }

    #[tokio::test]
    async fn update_titles_backfills_existing_activities() {
        let pool = test_pool().await;
        let src_dir = TempDir::new().unwrap();
        let act_dir = TempDir::new().unwrap();

        insert_geocache(&pool, 39.9, 116.4, "北京", "", "北京市", "中国").await;
        let gpx_path = make_gpx_with_date("run.gpx", &src_dir,
            "2024-06-20T05:00:00Z", "2024-06-20T06:00:00Z");
        import_one(&pool, &gpx_path, act_dir.path()).await.unwrap();

        // Clear the generated title to simulate pre-existing NULL
        sqlx::query("UPDATE activities SET title = NULL").execute(&pool).await.unwrap();

        let (updated, _) = update_titles(&pool, false).await;
        assert_eq!(updated, 1);

        let title: Option<String> = sqlx::query_scalar("SELECT title FROM activities")
            .fetch_one(&pool).await.unwrap();
        let title = title.expect("title should be generated");
        assert!(title.contains("06-20"), "date should be in title, got {title}");
        assert!(title.ends_with("@北京"), "city should be in title, got {title}");
    }

    #[test]
    fn scan_dir_finds_fit_and_gpx() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.fit"), "").unwrap();
        std::fs::write(dir.path().join("b.GPX"), "").unwrap();
        std::fs::write(dir.path().join("c.jpg"), "").unwrap();

        let found = scan_dir(dir.path());
        assert_eq!(found.len(), 2);
    }
}
