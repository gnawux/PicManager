use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use crate::web::AppState;

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
}

#[derive(Debug, Deserialize)]
pub struct BatchUpdateBody {
    pub photo_ids: Vec<i64>,
    pub taken_at: Option<String>,
    pub timezone_offset: Option<i64>,
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
    Ok(StatusCode::OK)
}

pub async fn batch_update_photos(
    State(state): State<AppState>,
    Json(body): Json<BatchUpdateBody>,
) -> Result<Json<BatchUpdateResponse>, StatusCode> {
    if body.photo_ids.is_empty() {
        return Ok(Json(BatchUpdateResponse { updated: 0 }));
    }
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
        updated += 1;
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
    let row: Option<(String,)> = sqlx::query_as("SELECT path FROM photos WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let Some((path,)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let cache_path = state.config.thumb_cache_dir.join(format!("{id}.jpg"));
    let thumb_size = state.config.thumb_size;

    let result = tokio::task::spawn_blocking(move || {
        if cache_path.exists() {
            std::fs::read(&cache_path).map_err(|e| anyhow::anyhow!(e))
        } else {
            let bytes = generate_thumb(&path, thumb_size)?;
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

fn generate_thumb(path: &str, size: u32) -> anyhow::Result<Vec<u8>> {
    use image::{ImageFormat, ImageReader};
    use std::io::Cursor;

    let img = ImageReader::open(path)?.decode()?;
    let thumb = img.thumbnail(size, size);

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
        let bytes = generate_thumb(f.to_str().unwrap(), 300).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn generate_thumb_missing_file_returns_error() {
        let result = generate_thumb("/no/such/file.jpg", 300);
        assert!(result.is_err());
    }
}
