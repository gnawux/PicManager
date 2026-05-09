use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::activities::rdp;
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
            }
        })
        .collect();

    Ok(Json(ActivityList { activities, total, page: q.page, per_page: q.per_page }))
}

pub async fn get_activity(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<ActivityItem>, StatusCode> {
    let row: Option<(i64, Option<String>, String, Option<String>, Option<String>, Option<i64>, Option<f64>, Option<f64>, Option<i64>, Option<i64>, Option<i64>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, title, activity_type, start_time, end_time, duration_seconds,
             distance_meters, elevation_gain_meters, avg_heart_rate, max_heart_rate,
             calories, device, file_format
             FROM activities WHERE id=? AND import_status='imported'",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (id, title, activity_type, start_time, end_time, duration_seconds, distance_meters, elevation_gain_meters, avg_heart_rate, max_heart_rate, calories, device, file_format) =
        row.ok_or(StatusCode::NOT_FOUND)?;

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

    // Fetch photos in the time window with GPS
    let candidate_photos: Vec<(i64, String, String, Option<String>, Option<f64>, Option<f64>)> =
        sqlx::query_as(
            "SELECT id, path, format, taken_at, gps_lat, gps_lon
             FROM photos
             WHERE taken_at >= ? AND taken_at <= ?
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

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let (lat1, lat2) = (lat1.to_radians(), lat2.to_radians());
    let dlon = (lon2 - lon1).to_radians();
    let dlat = lat2 - lat1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}
