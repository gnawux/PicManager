use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use crate::web::AppState;
use crate::face::{apply_transform, apply_exif_orientation};

#[derive(Debug, Serialize)]
pub struct PhotoDetail {
    pub id: i64,
    pub path: String,
    pub format: String,
    pub taken_at: Option<String>,
    pub timezone_offset: Option<i64>,
    pub camera: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    pub import_status: String,
}

#[derive(Debug, Serialize)]
pub struct GpsPoint {
    pub id: i64,
    pub taken_at: Option<String>,
    pub gps_lat: f64,
    pub gps_lon: f64,
}

#[derive(Debug, Deserialize, Default)]
pub struct GpsPointsQuery {
    pub country: Option<String>,
    pub state: Option<String>,
    pub city: Option<String>,
}

pub async fn get_gps_points(
    State(state): State<AppState>,
    Query(params): Query<GpsPointsQuery>,
) -> Result<Json<Vec<GpsPoint>>, StatusCode> {
    let has_filter = params.country.is_some() || params.state.is_some() || params.city.is_some();

    let join = if has_filter {
        "JOIN geocache gc \
           ON PRINTF('%.4f', ph.gps_lat) = gc.lat_key \
          AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key"
    } else {
        ""
    };

    let mut conds = vec![
        "ph.import_status = 'imported'".to_owned(),
        "ph.gps_lat IS NOT NULL".to_owned(),
        "ph.gps_lon IS NOT NULL".to_owned(),
    ];
    let mut binds: Vec<String> = vec![];

    if has_filter {
        for (field, val) in [
            ("gc.country", &params.country),
            ("gc.state",   &params.state),
            ("gc.city",    &params.city),
        ] {
            match val {
                None => {}
                Some(v) if v == "__null__" => conds.push(format!("{field} IS NULL")),
                Some(v) => { conds.push(format!("{field} = ?")); binds.push(v.clone()); }
            }
        }
    }

    let where_str = conds.join(" AND ");
    let sql = format!(
        "SELECT ph.id, ph.taken_at, ph.gps_lat, ph.gps_lon \
         FROM photos ph {join} WHERE {where_str} ORDER BY ph.taken_at DESC NULLS LAST"
    );

    let mut q = sqlx::query_as::<_, (i64, Option<String>, f64, f64)>(&sql);
    for b in &binds { q = q.bind(b); }

    let rows = q.fetch_all(&state.pool).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, taken_at, gps_lat, gps_lon)| GpsPoint { id, taken_at, gps_lat, gps_lon })
            .collect(),
    ))
}

pub async fn get_photo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<PhotoDetail>, StatusCode> {
    let row: Option<(i64, String, String, Option<String>, Option<i64>, Option<String>, Option<f64>, Option<f64>, String)> =
        sqlx::query_as(
            "SELECT id, path, format, taken_at, timezone_offset, camera, gps_lat, gps_lon, import_status
             FROM photos WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (id, path, format, taken_at, timezone_offset, camera, gps_lat, gps_lon, import_status) =
        row.ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(PhotoDetail { id, path, format, taken_at, timezone_offset, camera, gps_lat, gps_lon, import_status }))
}

#[derive(Debug, Deserialize)]
pub struct PatchPhotoBody {
    pub taken_at: Option<String>,
    pub timezone_offset: Option<i64>,
    pub rotation_delta: Option<i32>,
    pub flip_h_toggle: Option<bool>,
    pub flip_v_toggle: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct BatchUpdateBody {
    pub photo_ids: Vec<i64>,
    pub taken_at: Option<String>,
    pub timezone_offset: Option<i64>,
    pub rotation_delta: Option<i32>,
    pub flip_h_toggle: Option<bool>,
    pub flip_v_toggle: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct BatchUpdateResponse {
    pub updated: u64,
}

pub async fn patch_photo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchPhotoBody>,
) -> Result<StatusCode, StatusCode> {
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM photos WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    if let Some(ref taken_at) = body.taken_at {
        sqlx::query("UPDATE photos SET taken_at = ? WHERE id = ?")
            .bind(taken_at)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(tz) = body.timezone_offset {
        sqlx::query("UPDATE photos SET timezone_offset = ? WHERE id = ?")
            .bind(tz)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let mut transform_changed = false;
    if let Some(delta) = body.rotation_delta {
        sqlx::query("UPDATE photos SET rotation = ((rotation + ?) % 360 + 360) % 360 WHERE id = ?")
            .bind(delta)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        transform_changed = true;
    }
    if body.flip_h_toggle == Some(true) {
        sqlx::query("UPDATE photos SET flip_h = 1 - flip_h WHERE id = ?")
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        transform_changed = true;
    }
    if body.flip_v_toggle == Some(true) {
        sqlx::query("UPDATE photos SET flip_v = 1 - flip_v WHERE id = ?")
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        transform_changed = true;
    }
    if transform_changed {
        let cache_path = state.config.thumb_cache_dir.join(format!("{id}.jpg"));
        let _ = tokio::fs::remove_file(&cache_path).await;
        // Re-analyze faces in the new display orientation (fire-and-forget).
        let pool2 = state.pool.clone();
        tokio::spawn(async move {
            crate::face::job::reanalyze_one_photo(&pool2, id).await;
        });
    }
    Ok(StatusCode::OK)
}

pub async fn batch_update_photos(
    State(state): State<AppState>,
    Json(body): Json<BatchUpdateBody>,
) -> Result<Json<BatchUpdateResponse>, StatusCode> {
    if body.photo_ids.is_empty() {
        return Ok(Json(BatchUpdateResponse { updated: 0 }));
    }
    let has_transform = body.rotation_delta.is_some()
        || body.flip_h_toggle == Some(true)
        || body.flip_v_toggle == Some(true);
    let mut updated: u64 = 0;
    for &id in &body.photo_ids {
        if let Some(ref taken_at) = body.taken_at {
            sqlx::query("UPDATE photos SET taken_at = ? WHERE id = ?")
                .bind(taken_at)
                .bind(id)
                .execute(&state.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        if let Some(tz) = body.timezone_offset {
            sqlx::query("UPDATE photos SET timezone_offset = ? WHERE id = ?")
                .bind(tz)
                .bind(id)
                .execute(&state.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        if let Some(delta) = body.rotation_delta {
            sqlx::query(
                "UPDATE photos SET rotation = ((rotation + ?) % 360 + 360) % 360 WHERE id = ?",
            )
            .bind(delta)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        if body.flip_h_toggle == Some(true) {
            sqlx::query("UPDATE photos SET flip_h = 1 - flip_h WHERE id = ?")
                .bind(id)
                .execute(&state.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        if body.flip_v_toggle == Some(true) {
            sqlx::query("UPDATE photos SET flip_v = 1 - flip_v WHERE id = ?")
                .bind(id)
                .execute(&state.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        if has_transform {
            let cache_path = state.config.thumb_cache_dir.join(format!("{id}.jpg"));
            let _ = tokio::fs::remove_file(&cache_path).await;
        }
        updated += 1;
    }
    if has_transform {
        let pool2 = state.pool.clone();
        let ids = body.photo_ids.clone();
        tokio::spawn(async move {
            for id in ids {
                crate::face::job::reanalyze_one_photo(&pool2, id).await;
            }
        });
    }
    Ok(Json(BatchUpdateResponse { updated }))
}

#[derive(Debug, Deserialize)]
pub struct Pagination {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 50 }

#[derive(Debug, Serialize)]
pub struct PhotoRow {
    pub id: i64,
    pub path: String,
    pub format: String,
    pub taken_at: Option<String>,
    pub camera: Option<String>,
    pub import_status: String,
}

#[derive(Debug, Serialize)]
pub struct PhotoList {
    pub photos: Vec<PhotoRow>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

pub async fn list_photos(
    State(state): State<AppState>,
    Query(pag): Query<Pagination>,
) -> Result<Json<PhotoList>, StatusCode> {
    let offset = (pag.page.saturating_sub(1)) as i64 * pag.per_page as i64;
    let limit = pag.per_page as i64;

    let total: (i64,) = sqlx::query_as("SELECT active_count FROM photo_stats WHERE id = 1")
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let photos: Vec<PhotoRow> = sqlx::query_as(
        "SELECT id, path, format, taken_at, camera, import_status
         FROM photos ORDER BY taken_at DESC NULLS LAST, id DESC
         LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(id, path, format, taken_at, camera, import_status)| PhotoRow {
        id, path, format, taken_at, camera, import_status,
    })
    .collect();

    Ok(Json(PhotoList {
        photos,
        total: total.0,
        page: pag.page,
        per_page: pag.per_page,
    }))
}

pub async fn get_thumb(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    let row: Option<(String, i32, i32, i32, i32)> = sqlx::query_as(
        "SELECT path, rotation, flip_h, flip_v, exif_orientation FROM photos WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let Some((path, rotation, flip_h, flip_v, exif_orient)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let cache_path = state.config.thumb_cache_dir.join(format!("{id}.jpg"));
    let thumb_size = state.config.thumb_size;

    let result = tokio::task::spawn_blocking(move || {
        if cache_path.exists() {
            std::fs::read(&cache_path).map_err(|e| anyhow::anyhow!(e))
        } else {
            let bytes = generate_thumb(&path, thumb_size, exif_orient as u8, rotation, flip_h != 0, flip_v != 0)?;
            if let Some(parent) = cache_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&cache_path, &bytes)?;
            Ok(bytes)
        }
    })
    .await;

    match result {
        Ok(Ok(bytes)) => ([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn get_photo_file(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT path, format FROM photos WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .unwrap_or(None);

    let Some((path, format)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mime = match format.to_lowercase().as_str() {
        "jpeg" | "jpg" => "image/jpeg",
        "png"          => "image/png",
        "gif"          => "image/gif",
        "webp"         => "image/webp",
        "heic" | "heif"=> "image/heic",
        "tiff" | "tif" => "image/tiff",
        _              => "application/octet-stream",
    };

    match tokio::fs::read(&path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        Err(_)    => StatusCode::NOT_FOUND.into_response(),
    }
}

fn generate_thumb(path: &str, size: u32, exif_orient: u8, rotation: i32, flip_h: bool, flip_v: bool) -> anyhow::Result<Vec<u8>> {
    use image::{ImageFormat, ImageReader};
    use std::io::Cursor;

    let img = ImageReader::open(path)?.decode()?;
    let thumb = img.resize_to_fill(size, size, image::imageops::FilterType::Triangle);
    let thumb = apply_exif_orientation(thumb, exif_orient);
    let thumb = apply_transform(thumb, rotation, flip_h, flip_v);

    let mut buf = Vec::new();
    thumb.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)?;
    Ok(buf)
}

impl sqlx::FromRow<'_, sqlx::sqlite::SqliteRow> for PhotoRow {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> sqlx::Result<Self> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id")?,
            path: row.try_get("path")?,
            format: row.try_get("format")?,
            taken_at: row.try_get("taken_at")?,
            camera: row.try_get("camera")?,
            import_status: row.try_get("import_status")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn generate_thumb_returns_jpeg_bytes() {
        let f = fixture("with_exif.jpg");
        let bytes = generate_thumb(f.to_str().unwrap(), 300, 1, 0, false, false).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn generate_thumb_missing_file_returns_error() {
        let result = generate_thumb("/no/such/file.jpg", 300, 1, 0, false, false);
        assert!(result.is_err());
    }

    #[test]
    fn apply_transform_rotation_180_is_involutory() {
        let f = fixture("with_exif.jpg");
        let img = image::open(&f).unwrap();
        let rotated = apply_transform(img.clone(), 180, false, false);
        let back = apply_transform(rotated, 180, false, false);
        assert_eq!(img.width(), back.width());
        assert_eq!(img.height(), back.height());
    }

    #[test]
    fn apply_transform_four_rotations_returns_original_size() {
        let f = fixture("with_exif.jpg");
        let img = image::open(&f).unwrap();
        let (w, h) = (img.width(), img.height());
        let r = apply_transform(img, 90, false, false);
        let r = apply_transform(r, 90, false, false);
        let r = apply_transform(r, 90, false, false);
        let r = apply_transform(r, 90, false, false);
        assert_eq!(r.width(), w);
        assert_eq!(r.height(), h);
    }
}
