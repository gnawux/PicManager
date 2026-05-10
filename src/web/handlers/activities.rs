use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::activities::{importer, rdp};
use crate::web::AppState;

const RDP_THRESHOLD: usize = 7200;
const RDP_EPSILON: f64 = 1e-5; // ~1 m in degrees
const PHOTO_MAX_DISTANCE_M: f64 = 500.0;

#[derive(Debug, Deserialize)]
pub struct ActivityQuery {
    #[serde(rename = "type")]
    activity_type: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_per_page")]
    per_page: u32,
}

fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 50 }

#[derive(Debug, Serialize)]
pub struct ActivityItem {
    pub id: i64,
    pub title: Option<String>,
    pub activity_type: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub duration_seconds: Option<i64>,
    pub distance_meters: Option<f64>,
    pub elevation_gain_meters: Option<f64>,
    pub avg_heart_rate: Option<i64>,
    pub max_heart_rate: Option<i64>,
    pub calories: Option<i64>,
    pub device: Option<String>,
    pub file_format: String,
    /// Sensors JSON parsed from DB — only populated by `get_activity` (detail view).
    pub sensors: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ActivityList {
    pub activities: Vec<ActivityItem>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Serialize)]
pub struct TrackPointItem {
    pub ts: String,
    pub lat: f64,
    pub lon: f64,
    pub elevation: Option<f64>,
    pub heart_rate: Option<i64>,
    pub cadence: Option<i64>,
    pub speed: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct TrackResponse {
    pub points: Vec<TrackPointItem>,
    pub original_count: usize,
    pub downsampled: bool,
}

#[derive(Debug, Serialize)]
pub struct PhotoItem {
    pub id: i64,
    pub path: String,
    pub format: String,
    pub taken_at: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PhotosResponse {
    pub photos: Vec<PhotoItem>,
}

pub async fn list_activities(
    State(state): State<AppState>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<ActivityList>, StatusCode> {
    let offset = (q.page.saturating_sub(1)) as i64 * q.per_page as i64;
    let limit = q.per_page as i64;

    let (total, rows) = if let Some(ref t) = q.activity_type {
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM activities WHERE import_status='imported' AND activity_type=?",
        )
        .bind(t)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows: Vec<(i64, Option<String>, String, Option<String>, Option<String>, Option<i64>, Option<f64>, Option<f64>, Option<i64>, Option<i64>, Option<i64>, Option<String>, String)> =
            sqlx::query_as(
                "SELECT id, title, activity_type, start_time, end_time, duration_seconds,
                 distance_meters, elevation_gain_meters, avg_heart_rate, max_heart_rate,
                 calories, device, file_format
                 FROM activities WHERE import_status='imported' AND activity_type=?
                 ORDER BY start_time DESC NULLS LAST LIMIT ? OFFSET ?",
            )
            .bind(t)
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        (total.0, rows)
    } else {
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM activities WHERE import_status='imported'",
        )
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows: Vec<(i64, Option<String>, String, Option<String>, Option<String>, Option<i64>, Option<f64>, Option<f64>, Option<i64>, Option<i64>, Option<i64>, Option<String>, String)> =
            sqlx::query_as(
                "SELECT id, title, activity_type, start_time, end_time, duration_seconds,
                 distance_meters, elevation_gain_meters, avg_heart_rate, max_heart_rate,
                 calories, device, file_format
                 FROM activities WHERE import_status='imported'
                 ORDER BY start_time DESC NULLS LAST LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        (total.0, rows)
    };

    let activities = rows
        .into_iter()
        .map(|(id, title, activity_type, start_time, end_time, duration_seconds, distance_meters, elevation_gain_meters, avg_heart_rate, max_heart_rate, calories, device, file_format)| {
            ActivityItem {
                id,
                title,
                activity_type,
                start_time,
                end_time,
                duration_seconds,
                distance_meters,
                elevation_gain_meters,
                avg_heart_rate,
                max_heart_rate,
                calories,
                device,
                file_format,
                sensors: None,
            }
        })
        .collect();

    Ok(Json(ActivityList { activities, total, page: q.page, per_page: q.per_page }))
}

pub async fn get_activity(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<ActivityItem>, StatusCode> {
    let row: Option<(i64, Option<String>, String, Option<String>, Option<String>, Option<i64>, Option<f64>, Option<f64>, Option<i64>, Option<i64>, Option<i64>, Option<String>, String, Option<String>)> =
        sqlx::query_as(
            "SELECT id, title, activity_type, start_time, end_time, duration_seconds,
             distance_meters, elevation_gain_meters, avg_heart_rate, max_heart_rate,
             calories, device, file_format, sensors
             FROM activities WHERE id=? AND import_status='imported'",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (id, title, activity_type, start_time, end_time, duration_seconds, distance_meters, elevation_gain_meters, avg_heart_rate, max_heart_rate, calories, device, file_format, sensors_json) =
        row.ok_or(StatusCode::NOT_FOUND)?;

    let sensors = sensors_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    Ok(Json(ActivityItem {
        id,
        title,
        activity_type,
        start_time,
        end_time,
        duration_seconds,
        distance_meters,
        elevation_gain_meters,
        avg_heart_rate,
        max_heart_rate,
        calories,
        device,
        file_format,
        sensors,
    }))
}

pub async fn get_activity_track(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TrackResponse>, StatusCode> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM activities WHERE id=?)")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    let rows: Vec<(String, f64, f64, Option<f64>, Option<i64>, Option<i64>, Option<f64>)> =
        sqlx::query_as(
            "SELECT ts, lat, lon, elevation, heart_rate, cadence, speed
             FROM activity_track_points WHERE activity_id=? ORDER BY ts ASC",
        )
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let original_count = rows.len();

    let points = if original_count > RDP_THRESHOLD {
        let coords: Vec<(f64, f64)> = rows.iter().map(|(_, lat, lon, ..)| (*lat, *lon)).collect();
        let kept_indices = rdp::simplify(&coords, RDP_EPSILON);
        kept_indices
            .into_iter()
            .map(|i| {
                let (ts, lat, lon, elevation, heart_rate, cadence, speed) = &rows[i];
                TrackPointItem {
                    ts: ts.clone(),
                    lat: *lat,
                    lon: *lon,
                    elevation: *elevation,
                    heart_rate: *heart_rate,
                    cadence: *cadence,
                    speed: *speed,
                }
            })
            .collect()
    } else {
        rows.into_iter()
            .map(|(ts, lat, lon, elevation, heart_rate, cadence, speed)| TrackPointItem {
                ts, lat, lon, elevation, heart_rate, cadence, speed,
            })
            .collect()
    };

    Ok(Json(TrackResponse {
        downsampled: original_count > RDP_THRESHOLD,
        original_count,
        points,
    }))
}

pub async fn get_activity_photos(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<PhotosResponse>, StatusCode> {
    let activity: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT start_time, end_time FROM activities WHERE id=?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (start_time, end_time) = activity.ok_or(StatusCode::NOT_FOUND)?;

    let (start_time, end_time) = match (start_time, end_time) {
        (Some(s), Some(e)) => (s, e),
        _ => return Ok(Json(PhotosResponse { photos: vec![] })),
    };

    // Fetch track points for distance filtering
    let track_points: Vec<(f64, f64)> =
        sqlx::query_as("SELECT lat, lon FROM activity_track_points WHERE activity_id=? ORDER BY ts")
            .bind(id)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch photos in the time window with GPS.
    // Convert photo local time to UTC using timezone_offset before comparing with
    // activity start/end (stored as RFC3339 UTC). Raw string comparison fails because
    // photos.taken_at uses space separator ("YYYY-MM-DD HH:MM:SS") while activities
    // use T separator ("YYYY-MM-DDTHH:MM:SS+00:00"), and space (32) < T (84).
    let candidate_photos: Vec<(i64, String, String, Option<String>, Option<f64>, Option<f64>)> =
        sqlx::query_as(
            "SELECT id, path, format, taken_at, gps_lat, gps_lon
             FROM photos
             WHERE datetime(taken_at, CAST(-COALESCE(timezone_offset, 0) AS TEXT) || ' minutes')
                     >= datetime(?)
               AND datetime(taken_at, CAST(-COALESCE(timezone_offset, 0) AS TEXT) || ' minutes')
                     <= datetime(?)
               AND gps_lat IS NOT NULL AND gps_lon IS NOT NULL
               AND import_status = 'imported'
             ORDER BY taken_at",
        )
        .bind(&start_time)
        .bind(&end_time)
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let photos = candidate_photos
        .into_iter()
        .filter(|(_, _, _, _, lat, lon)| {
            if let (Some(lat), Some(lon)) = (lat, lon) {
                if track_points.is_empty() {
                    return false;
                }
                let min_dist = track_points
                    .iter()
                    .map(|(tlat, tlon)| haversine_m(*lat, *lon, *tlat, *tlon))
                    .fold(f64::INFINITY, f64::min);
                min_dist <= PHOTO_MAX_DISTANCE_M
            } else {
                false
            }
        })
        .map(|(id, path, format, taken_at, gps_lat, gps_lon)| PhotoItem {
            id,
            path,
            format,
            taken_at,
            gps_lat,
            gps_lon,
        })
        .collect();

    Ok(Json(PhotosResponse { photos }))
}

#[derive(Debug, Deserialize)]
pub struct TrimRequest {
    pub start_time: String,
    pub end_time: String,
}

pub async fn trim_activity(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<TrimRequest>,
) -> Result<Json<ActivityItem>, StatusCode> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM activities WHERE id=? AND import_status='imported')",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    // Delete points outside [start_time, end_time]; use datetime() to normalise format.
    sqlx::query(
        "DELETE FROM activity_track_points \
         WHERE activity_id=? AND (datetime(ts) < datetime(?) OR datetime(ts) > datetime(?))",
    )
    .bind(id)
    .bind(&req.start_time)
    .bind(&req.end_time)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Re-read remaining points to recalculate stats.
    let pts: Vec<(String, f64, f64, Option<f64>, Option<i64>)> = sqlx::query_as(
        "SELECT ts, lat, lon, elevation, heart_rate \
         FROM activity_track_points WHERE activity_id=? ORDER BY ts ASC",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let new_start = pts.first().map(|(ts, ..)| ts.clone());
    let new_end   = pts.last().map(|(ts, ..)| ts.clone());

    let duration_seconds: Option<i64> = match (&new_start, &new_end) {
        (Some(s), Some(e)) => {
            let s = chrono::DateTime::parse_from_rfc3339(s).ok();
            let e = chrono::DateTime::parse_from_rfc3339(e).ok();
            s.zip(e).map(|(s, e)| (e - s).num_seconds())
        }
        _ => None,
    };

    let distance_meters: Option<f64> = if pts.len() >= 2 {
        let d: f64 = pts.windows(2)
            .map(|w| haversine_m(w[0].1, w[0].2, w[1].1, w[1].2))
            .sum();
        if d > 0.0 { Some(d) } else { None }
    } else {
        None
    };

    let elevation_gain_meters: Option<f64> = {
        let gain: f64 = pts.windows(2)
            .filter_map(|w| match (w[0].3, w[1].3) {
                (Some(e1), Some(e2)) if e2 > e1 => Some(e2 - e1),
                _ => None,
            })
            .sum();
        if gain > 0.0 { Some(gain) } else { None }
    };

    let hr_vals: Vec<i64> = pts.iter().filter_map(|(_, _, _, _, hr)| *hr).collect();
    let avg_heart_rate = if hr_vals.is_empty() { None } else {
        Some(hr_vals.iter().sum::<i64>() / hr_vals.len() as i64)
    };
    let max_heart_rate = hr_vals.iter().copied().max();

    sqlx::query(
        "UPDATE activities SET start_time=?, end_time=?, duration_seconds=?,
         distance_meters=?, elevation_gain_meters=?, avg_heart_rate=?, max_heart_rate=?
         WHERE id=?",
    )
    .bind(&new_start)
    .bind(&new_end)
    .bind(duration_seconds)
    .bind(distance_meters)
    .bind(elevation_gain_meters)
    .bind(avg_heart_rate)
    .bind(max_heart_rate)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    get_activity(State(state), Path(id)).await
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let (lat1, lat2) = (lat1.to_radians(), lat2.to_radians());
    let dlon = (lon2 - lon1).to_radians();
    let dlat = lat2 - lat1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}

// ── Merge ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MergeRequest {
    pub ids: Vec<i64>,
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MergeErrorBody {
    error: &'static str,
}

fn merge_err(status: StatusCode, code: &'static str) -> (StatusCode, Json<MergeErrorBody>) {
    (status, Json(MergeErrorBody { error: code }))
}

type ActivityRow = (
    i64, Option<String>, String,
    Option<String>, Option<String>,
    Option<i64>, Option<f64>, Option<f64>,
    Option<i64>, Option<i64>, Option<i64>,
    Option<String>, String,
);

pub async fn merge_activities(
    State(state): State<AppState>,
    Json(req): Json<MergeRequest>,
) -> Result<Json<ActivityItem>, (StatusCode, Json<MergeErrorBody>)> {
    if req.ids.len() < 2 {
        return Err(merge_err(StatusCode::BAD_REQUEST, "too_few"));
    }

    let placeholders = req.ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, title, activity_type, start_time, end_time, duration_seconds,
         distance_meters, elevation_gain_meters, avg_heart_rate, max_heart_rate,
         calories, device, file_format
         FROM activities WHERE id IN ({placeholders}) AND import_status = 'imported'"
    );
    let mut q = sqlx::query_as::<_, ActivityRow>(&sql);
    for id in &req.ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(&state.pool).await
        .map_err(|_| merge_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error"))?;

    if rows.len() != req.ids.len() {
        return Err(merge_err(StatusCode::NOT_FOUND, "not_found"));
    }

    // Validate same activity type
    let first_type = &rows[0].2;
    if rows.iter().any(|r| &r.2 != first_type) {
        return Err(merge_err(StatusCode::BAD_REQUEST, "type_mismatch"));
    }

    // Validate all have start/end times
    if rows.iter().any(|r| r.3.is_none() || r.4.is_none()) {
        return Err(merge_err(StatusCode::BAD_REQUEST, "missing_times"));
    }

    // Sort by start_time and check for overlaps
    let mut sorted = rows;
    sorted.sort_by(|a, b| a.3.cmp(&b.3));
    for w in sorted.windows(2) {
        if w[0].4.as_deref() > w[1].3.as_deref() {
            return Err(merge_err(StatusCode::BAD_REQUEST, "time_overlap"));
        }
    }

    let activity_type = sorted[0].2.clone();
    let start_time = sorted.first().and_then(|r| r.3.clone());
    let end_time = sorted.last().and_then(|r| r.4.clone());

    let duration_seconds: Option<i64> = {
        let s: i64 = sorted.iter().filter_map(|r| r.5).sum();
        if s > 0 { Some(s) } else { None }
    };
    let distance_meters: Option<f64> = {
        let s: f64 = sorted.iter().filter_map(|r| r.6).sum();
        if s > 0.0 { Some(s) } else { None }
    };
    let elevation_gain_meters: Option<f64> = {
        let s: f64 = sorted.iter().filter_map(|r| r.7).sum();
        if s > 0.0 { Some(s) } else { None }
    };
    let avg_heart_rate: Option<i64> = {
        let (mut wsum, mut wdur) = (0i64, 0i64);
        for r in &sorted {
            if let (Some(dur), Some(hr)) = (r.5, r.8) {
                wsum += dur * hr;
                wdur += dur;
            }
        }
        if wdur > 0 { Some(wsum / wdur) } else { None }
    };
    let max_heart_rate: Option<i64> = sorted.iter().filter_map(|r| r.9).max();
    let calories: Option<i64> = {
        let s: i64 = sorted.iter().filter_map(|r| r.10).sum();
        if s > 0 { Some(s) } else { None }
    };
    let device: Option<String> = sorted.iter().find_map(|r| r.11.clone());

    // sha256 derived from sorted source IDs — unique, avoids re-merge of same set
    let mut sorted_ids: Vec<i64> = req.ids.clone();
    sorted_ids.sort_unstable();
    let sha_input = format!(
        "merged:{}",
        sorted_ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    );
    let sha256 = hex::encode(Sha256::digest(sha_input.as_bytes()));

    // GPS of the first track point (for auto-title city lookup)
    let first_id = sorted[0].0;
    let first_gps: Option<(f64, f64)> = sqlx::query_as(
        "SELECT lat, lon FROM activity_track_points WHERE activity_id = ? ORDER BY ts LIMIT 1",
    )
    .bind(first_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let title = match req.title.filter(|t| !t.is_empty()) {
        Some(t) => Some(t),
        None => importer::generate_title(
            &state.pool, &activity_type, start_time.as_deref(),
            distance_meters, first_gps,
        ).await,
    };

    let mut tx = state.pool.begin().await
        .map_err(|_| merge_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error"))?;

    let new_id: i64 = sqlx::query_scalar(
        "INSERT INTO activities \
         (sha256, source_path, file_format, title, activity_type, \
          start_time, end_time, duration_seconds, distance_meters, elevation_gain_meters, \
          avg_heart_rate, max_heart_rate, calories, device, import_status) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,'imported') RETURNING id",
    )
    .bind(&sha256)
    .bind("merged")
    .bind("merged")
    .bind(&title)
    .bind(&activity_type)
    .bind(&start_time)
    .bind(&end_time)
    .bind(duration_seconds)
    .bind(distance_meters)
    .bind(elevation_gain_meters)
    .bind(avg_heart_rate)
    .bind(max_heart_rate)
    .bind(calories)
    .bind(&device)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| merge_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error"))?;

    // Re-parent all track points to the new merged activity
    let update_pts = format!(
        "UPDATE activity_track_points SET activity_id = ? WHERE activity_id IN ({placeholders})"
    );
    let mut q = sqlx::query(&update_pts).bind(new_id);
    for id in &req.ids { q = q.bind(id); }
    q.execute(&mut *tx).await
        .map_err(|_| merge_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error"))?;

    // Soft-delete the source activities
    let soft_del = format!(
        "UPDATE activities SET import_status = 'merged' WHERE id IN ({placeholders})"
    );
    let mut q = sqlx::query(&soft_del);
    for id in &req.ids { q = q.bind(id); }
    q.execute(&mut *tx).await
        .map_err(|_| merge_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error"))?;

    tx.commit().await
        .map_err(|_| merge_err(StatusCode::INTERNAL_SERVER_ERROR, "db_error"))?;

    get_activity(State(state), Path(new_id)).await
        .map_err(|s| merge_err(s, "db_error"))
}
