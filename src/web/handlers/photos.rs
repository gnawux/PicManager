use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use crate::application::{PhotoMetadataUpdate, RequestContext, ServiceErrorCode};
use crate::web::AppState;
use crate::orientation::{DisplayTransform, OrientationMode};
#[cfg(test)]
use crate::orientation::apply_user_transform;

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
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub sources: Vec<PhotoSourceDetail>,
    pub renditions: PhotoRenditions,
}

#[derive(Debug, Serialize)]
pub struct PhotoSourceDetail {
    pub provider: String,
    pub original_filename: Option<String>,
    pub sync_status: String,
}

#[derive(Debug, Serialize)]
pub struct PhotoRenditions {
    pub display: String,
    pub original: Option<String>,
    pub current: Option<String>,
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
    let row: Option<(i64, String, String, Option<String>, Option<i64>, Option<String>, Option<f64>, Option<f64>, String, Option<i64>, Option<i64>, i64, i64)> =
        sqlx::query_as(
            "SELECT p.id, p.path, p.format, p.taken_at, p.timezone_offset, p.camera, \
                    p.gps_lat, p.gps_lon, p.import_status, COALESCE(dv.width, p.width), \
                    COALESCE(dv.height, p.height), \
                    EXISTS(SELECT 1 FROM asset_variants ov \
                           WHERE ov.asset_id = a.id AND ov.role = 'original' AND ov.path IS NOT NULL), \
                    EXISTS(SELECT 1 FROM asset_variants cv \
                           WHERE cv.asset_id = a.id AND cv.role = 'current' AND cv.path IS NOT NULL) \
             FROM photos p \
             LEFT JOIN assets a ON a.photo_id = p.id \
             LEFT JOIN asset_variants dv ON dv.id = a.display_variant_id \
             WHERE p.id = ?",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (id, path, format, taken_at, timezone_offset, camera, gps_lat, gps_lon, import_status,
        width, height, has_original, has_current) =
        row.ok_or(StatusCode::NOT_FOUND)?;

    let sources = sqlx::query_as::<_, (String, Option<String>, String)>(
        "SELECT provider, original_filename, sync_status FROM asset_sources \
         WHERE asset_id = (SELECT id FROM assets WHERE photo_id = ?) ORDER BY id",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .into_iter()
    .map(|(provider, original_filename, sync_status)| PhotoSourceDetail {
        provider, original_filename, sync_status,
    })
    .collect();
    let base = format!("/api/photos/{id}/file");
    Ok(Json(PhotoDetail {
        id, path, format, taken_at, timezone_offset, camera, gps_lat, gps_lon, import_status,
        width, height, sources,
        renditions: PhotoRenditions {
            display: base.clone(),
            original: (has_original != 0).then(|| format!("{base}?variant=original")),
            current: (has_current != 0).then(|| format!("{base}?variant=current")),
        },
    }))
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
    Extension(context): Extension<RequestContext>,
    Path(id): Path<i64>,
    Json(body): Json<PhotoMetadataUpdate>,
) -> Result<StatusCode, StatusCode> {
    match state.application.metadata().update_one(&context, id, body).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(error) if error.code == ServiceErrorCode::NotFound => Err(StatusCode::NOT_FOUND),
        Err(error) if error.code == ServiceErrorCode::InvalidInput => Err(StatusCode::BAD_REQUEST),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

pub async fn batch_update_photos(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Json(body): Json<BatchUpdateBody>,
) -> Result<Json<BatchUpdateResponse>, StatusCode> {
    let update = PhotoMetadataUpdate {
        taken_at: body.taken_at,
        timezone_offset: body.timezone_offset,
        rotation_delta: body.rotation_delta,
        flip_h_toggle: body.flip_h_toggle,
        flip_v_toggle: body.flip_v_toggle,
    };
    let result = state
        .application
        .metadata()
        .update_many(&context, &body.photo_ids, update)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(BatchUpdateResponse { updated: result.updated }))
}

#[derive(Debug, Deserialize)]
pub struct Pagination {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
    #[serde(default = "default_order")]
    pub order: String,
}

fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 50 }
fn default_order() -> String { "desc".to_string() }

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

    let dir = if pag.order == "asc" { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT id, path, format, taken_at, camera, import_status
         FROM photos ORDER BY taken_at {dir} NULLS LAST, id {dir}
         LIMIT ? OFFSET ?"
    );
    let photos: Vec<PhotoRow> = sqlx::query_as(&sql)
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

#[derive(Debug, Deserialize, Default)]
pub struct ThumbQuery {
    pub size: Option<u32>,
}

pub async fn get_thumb(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<i64>,
    Query(query): Query<ThumbQuery>,
) -> Response {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT render_revision FROM photos WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let Some((render_revision,)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let requested_size = query.size.map(|size| size.clamp(128, 2048));
    let cache_path = if let Some(size) = requested_size {
        crate::derived::sized_thumbnail_cache_path(
            &state.config.thumb_cache_dir,
            id,
            render_revision,
            size,
        )
    } else {
        crate::derived::thumbnail_cache_path(&state.config.thumb_cache_dir, id, render_revision)
    };
    if cache_path.exists() {
        return match tokio::fs::read(cache_path).await {
            Ok(bytes) => ([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
    }

    let queued = crate::jobs::handlers::enqueue_thumbnail(
        &state.application,
        &context,
        crate::jobs::handlers::ThumbnailJobPayload {
            photo_id: id,
            render_revision,
            size: requested_size,
        },
    )
    .await;
    let Ok(queued) = queued else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let completed = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let job = crate::jobs::get(&state.pool, queued.job.id).await.ok()?;
            if matches!(job.status.as_str(), "succeeded" | "failed" | "cancelled") {
                return Some(job);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await;
    match completed {
        Ok(Some(job)) if job.status == "succeeded" => match tokio::fs::read(cache_path).await {
            Ok(bytes) => ([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response(),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        Ok(Some(job)) if job.error_code.as_deref() == Some("photo_not_found") => {
            StatusCode::NOT_FOUND.into_response()
        }
        Ok(Some(_)) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct PhotoFileQuery {
    pub variant: Option<String>,
}

pub async fn get_photo_file(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<PhotoFileQuery>,
) -> Response {
    let variant_join = match query.variant.as_deref().unwrap_or("display") {
        "display" => "LEFT JOIN asset_variants sv ON sv.id = a.display_variant_id",
        "original" => "LEFT JOIN asset_variants sv ON sv.id = (\
            SELECT id FROM asset_variants WHERE asset_id = a.id AND role = 'original' \
            AND path IS NOT NULL ORDER BY is_primary DESC, id LIMIT 1)",
        "current" => "LEFT JOIN asset_variants sv ON sv.id = (\
            SELECT id FROM asset_variants WHERE asset_id = a.id AND role = 'current' \
            AND path IS NOT NULL ORDER BY id DESC LIMIT 1)",
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let path_expression = if query.variant.as_deref().unwrap_or("display") == "display" {
        "COALESCE(sv.path, p.path)"
    } else {
        "sv.path"
    };
    let sql = format!(
        "SELECT {path_expression}, sv.mime_type, p.format, p.rotation, p.flip_h, p.flip_v, \
                p.exif_orientation, COALESCE(vr.orientation_mode, 'legacy_unknown'), \
                vr.display_orientation \
         FROM photos p \
         LEFT JOIN assets a ON a.photo_id = p.id \
         {variant_join} \
         LEFT JOIN variant_renditions vr ON vr.variant_id = sv.id \
         WHERE p.id = ?"
    );
    let row: Option<(Option<String>, Option<String>, String, i32, i32, i32, i32, String, Option<i64>)> = sqlx::query_as(&sql)
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let Some((Some(path), variant_mime, format, rotation, flip_h_i, flip_v_i, exif_orient, mode, display_orient)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let flip_h = flip_h_i != 0;
    let flip_v = flip_v_i != 0;
    let is_heic = crate::image_open::is_heic(std::path::Path::new(&path));
    let orientation_mode = OrientationMode::from_catalog(Some(&mode));
    let needs_catalog_bake = orientation_mode == OrientationMode::Metadata
        && display_orient.is_some_and(|value| value != 1);

    // For HEIC: always bake EXIF orientation (read from the original HEIC file, not the
    // sips-output JPEG) into pixels and return a plain JPEG with no EXIF Orientation tag.
    // This prevents the browser from mis-applying an EXIF value that sips may have
    // synthesised from a HEIF IROT box (IROT-derived EXIF=6 on landscape pixels → portrait).
    // For non-HEIC with user-applied transforms: same pixel-baking is required.
    if is_heic || needs_catalog_bake || rotation != 0 || flip_h || flip_v {
        let exif_orient_u8 = exif_orient as u8;
        match tokio::task::spawn_blocking(move || {
            apply_transforms_full(
                &path,
                orientation_mode,
                display_orient.map(|value| value as u8),
                exif_orient_u8,
                rotation,
                flip_h,
                flip_v,
            )
        })
        .await
        {
            Ok(Ok(bytes)) => return ([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response(),
            _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }

    let mime = variant_mime
        .filter(|mime| mime.starts_with("image/"))
        .unwrap_or_else(|| mime_for_path_or_format(&path, &format).to_owned());

    match tokio::fs::read(&path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        Err(_)    => StatusCode::NOT_FOUND.into_response(),
    }
}

fn mime_for_path_or_format(path: &str, format: &str) -> &'static str {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or(format)
        .to_ascii_lowercase();
    match extension.as_str() {
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    }
}

fn apply_transforms_full(
    path: &str,
    mode: OrientationMode,
    display_orientation: Option<u8>,
    exif_orient: u8,
    rotation: i32,
    flip_h: bool,
    flip_v: bool,
) -> anyhow::Result<Vec<u8>> {
    use image::ImageFormat;
    use std::io::Cursor;

    let p = std::path::Path::new(path);
    let img = crate::image_open::open_image(p)?;
    let img = DisplayTransform::new(
        mode, display_orientation, exif_orient, p, rotation, flip_h, flip_v,
    ).apply(img);
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)?;
    Ok(buf)
}

#[cfg(test)]
fn generate_thumb(
    path: &str,
    size: u32,
    mode: OrientationMode,
    display_orientation: Option<u8>,
    exif_orient: u8,
    rotation: i32,
    flip_h: bool,
    flip_v: bool,
) -> anyhow::Result<Vec<u8>> {
    crate::derived::generate_thumbnail(
        path,
        size,
        mode,
        display_orientation,
        exif_orient,
        rotation,
        flip_h,
        flip_v,
    )
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
        let bytes = generate_thumb(f.to_str().unwrap(), 300, OrientationMode::LegacyUnknown, None, 1, 0, false, false).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn generate_thumb_missing_file_returns_error() {
        let result = generate_thumb("/no/such/file.jpg", 300, OrientationMode::LegacyUnknown, None, 1, 0, false, false);
        assert!(result.is_err());
    }

    #[test]
    fn full_transform_obeys_catalog_orientation_mode() {
        let temp = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        image::DynamicImage::new_rgb8(3, 2).save(temp.path()).unwrap();
        let path = temp.path().to_str().unwrap();

        let metadata = apply_transforms_full(
            path,
            OrientationMode::Metadata,
            Some(6),
            1,
            0,
            false,
            false,
        )
        .unwrap();
        let baked = apply_transforms_full(
            path,
            OrientationMode::BakedPixels,
            Some(6),
            8,
            0,
            false,
            false,
        )
        .unwrap();

        let metadata = image::load_from_memory(&metadata).unwrap();
        let baked = image::load_from_memory(&baked).unwrap();
        assert_eq!((metadata.width(), metadata.height()), (2, 3));
        assert_eq!((baked.width(), baked.height()), (3, 2));
    }

    #[test]
    fn apply_transform_rotation_180_is_involutory() {
        let f = fixture("with_exif.jpg");
        let img = image::open(&f).unwrap();
        let rotated = apply_user_transform(img.clone(), 180, false, false);
        let back = apply_user_transform(rotated, 180, false, false);
        assert_eq!(img.width(), back.width());
        assert_eq!(img.height(), back.height());
    }

    #[test]
    fn apply_transform_four_rotations_returns_original_size() {
        let f = fixture("with_exif.jpg");
        let img = image::open(&f).unwrap();
        let (w, h) = (img.width(), img.height());
        let r = apply_user_transform(img, 90, false, false);
        let r = apply_user_transform(r, 90, false, false);
        let r = apply_user_transform(r, 90, false, false);
        let r = apply_user_transform(r, 90, false, false);
        assert_eq!(r.width(), w);
        assert_eq!(r.height(), h);
    }

    fn sample(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/samples")
            .join(name)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn generate_thumb_heic_returns_valid_jpeg() {
        let f = sample("IMG_9886.HEIC");
        let bytes = generate_thumb(f.to_str().unwrap(), 300, OrientationMode::LegacyUnknown, None, 1, 0, false, false).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn generate_thumb_and_apply_transforms_full_heic_consistent_orientation() {
        // Both functions must read orientation from the same source (original HEIC file,
        // not sips output).  A HEIC with HEIF IROT would expose any divergence: sips
        // would give EXIF=6 while the file has EXIF=1, producing different aspect ratios.
        let f = sample("IMG_9886.HEIC");
        let path = f.to_str().unwrap();
        let thumb_bytes = generate_thumb(path, 300, OrientationMode::LegacyUnknown, None, 1, 0, false, false).unwrap();
        let full_bytes = apply_transforms_full(path, OrientationMode::LegacyUnknown, None, 1, 0, false, false).unwrap();

        let thumb = image::load_from_memory(&thumb_bytes).unwrap();
        let full = image::load_from_memory(&full_bytes).unwrap();
        let thumb_landscape = thumb.width() >= thumb.height();
        let full_landscape = full.width() >= full.height();
        assert_eq!(
            thumb_landscape, full_landscape,
            "thumbnail ({}×{}) and full image ({}×{}) have inconsistent orientations",
            thumb.width(), thumb.height(), full.width(), full.height()
        );
    }
}
