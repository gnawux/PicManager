use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use crate::web::AppState;

#[derive(Debug, Serialize)]
pub struct PersonRow {
    pub id: i64,
    pub name: Option<String>,
    pub parent_id: Option<i64>,
    pub cover_face_id: Option<i64>,
    pub face_count: i64,
    pub photo_count: i64,
}

#[derive(Debug, Serialize)]
pub struct ClusterResponse {
    pub people_created: usize,
}

#[derive(Debug, Deserialize)]
pub struct MergeBody {
    pub source_id: i64,
    pub target_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct ReparentBody {
    pub new_parent_id: Option<i64>,
}

pub async fn list_people(
    State(state): State<AppState>,
) -> Result<Json<Vec<PersonRow>>, StatusCode> {
    let rows: Vec<(i64, Option<String>, Option<i64>, Option<i64>, i64, i64)> =
        sqlx::query_as(
            "SELECT p.id, p.name, p.parent_id, p.cover_face_id,
                    COUNT(DISTINCT pf.face_id)       AS face_count,
                    COUNT(DISTINCT f.photo_id)       AS photo_count
             FROM people p
             LEFT JOIN person_faces pf ON pf.person_id = p.id
             LEFT JOIN faces f ON f.id = pf.face_id
             GROUP BY p.id",
        )
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let people = rows
        .into_iter()
        .map(|(id, name, parent_id, cover_face_id, face_count, photo_count)| PersonRow {
            id, name, parent_id, cover_face_id, face_count, photo_count,
        })
        .collect();
    Ok(Json(people))
}

pub async fn cluster_people(
    State(state): State<AppState>,
) -> Result<Json<ClusterResponse>, StatusCode> {
    let count = crate::face::cluster::run_clustering(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ClusterResponse { people_created: count }))
}

pub async fn merge_people(
    State(state): State<AppState>,
    Json(body): Json<MergeBody>,
) -> Result<StatusCode, StatusCode> {
    // Move all person_faces from source to target (skip duplicates)
    sqlx::query(
        "INSERT OR IGNORE INTO person_faces (person_id, face_id)
         SELECT ?, face_id FROM person_faces WHERE person_id = ?",
    )
    .bind(body.target_id)
    .bind(body.source_id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Re-parent any children of source to target
    sqlx::query("UPDATE people SET parent_id = ? WHERE parent_id = ?")
        .bind(body.target_id)
        .bind(body.source_id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Delete source
    sqlx::query("DELETE FROM people WHERE id = ?")
        .bind(body.source_id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

pub async fn reparent_person(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ReparentBody>,
) -> Result<StatusCode, StatusCode> {
    let result = sqlx::query("UPDATE people SET parent_id = ? WHERE id = ?")
        .bind(body.new_parent_id)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if result.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::OK)
    }
}

pub async fn get_face_thumb(
    State(state): State<AppState>,
    Path(face_id): Path<i64>,
) -> Response {
    let row: Option<(i64, i64, i64, i64, String)> = sqlx::query_as(
        "SELECT f.x, f.y, f.width, f.height, p.path
         FROM faces f JOIN photos p ON p.id = f.photo_id
         WHERE f.id = ?",
    )
    .bind(face_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let Some((x, y, w, h, photo_path)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let cache_path = state.config.thumb_cache_dir.join(format!("face_{face_id}.jpg"));

    let result = tokio::task::spawn_blocking(move || {
        if cache_path.exists() {
            return std::fs::read(&cache_path).map_err(|e| anyhow::anyhow!(e));
        }
        let bytes = crop_face(&photo_path, x, y, w, h)?;
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cache_path, &bytes)?;
        Ok(bytes)
    })
    .await;

    match result {
        Ok(Ok(bytes)) => {
            use axum::http::header;
            ([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response()
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn crop_face(path: &str, x: i64, y: i64, w: i64, h: i64) -> anyhow::Result<Vec<u8>> {
    use image::{ImageFormat, ImageReader};
    use std::io::Cursor;

    let img = ImageReader::open(path)?.decode()?;
    let iw = img.width() as i64;
    let ih = img.height() as i64;

    // Clamp to image bounds
    let cx = x.max(0) as u32;
    let cy = y.max(0) as u32;
    let cw = (w.min(iw - x)).max(1) as u32;
    let ch = (h.min(ih - y)).max(1) as u32;

    let cropped = img.crop_imm(cx, cy, cw, ch);
    let thumb = cropped.thumbnail(160, 160);
    let mut buf = Vec::new();
    thumb.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)?;
    Ok(buf)
}
