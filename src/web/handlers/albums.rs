use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::{album, web::AppState};

#[derive(Debug, Serialize)]
pub struct AlbumRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub photo_count: i64,
    pub latest_photo_at: Option<String>,
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

pub async fn list_albums(
    State(state): State<AppState>,
) -> Result<Json<Vec<AlbumRow>>, StatusCode> {
    let rows: Vec<(i64, String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT a.id, a.name, a.kind,
                COUNT(pa.photo_id) as photo_count,
                MAX(p.taken_at) as latest_photo_at
         FROM albums a
         LEFT JOIN photo_albums pa ON pa.album_id = a.id
         LEFT JOIN photos p ON p.id = pa.photo_id
         GROUP BY a.id ORDER BY a.kind, a.name",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, name, kind, photo_count, latest_photo_at)| AlbumRow {
                id, name, kind, photo_count, latest_photo_at,
            })
            .collect(),
    ))
}

pub async fn list_album_photos(
    State(state): State<AppState>,
    Path(album_id): Path<i64>,
    Query(pag): Query<Pagination>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let exists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE id = ?")
        .bind(album_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists.0 == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    let offset = (pag.page.saturating_sub(1)) as i64 * pag.per_page as i64;
    let total: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
            .bind(album_id)
            .fetch_one(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let dir = if pag.order == "asc" { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT p.id, p.path, p.taken_at, p.camera
         FROM photos p JOIN photo_albums pa ON pa.photo_id = p.id
         WHERE pa.album_id = ?
         ORDER BY p.taken_at {dir} NULLS LAST, p.id {dir}
         LIMIT ? OFFSET ?"
    );
    let photos: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(&sql)
    .bind(album_id)
    .bind(pag.per_page as i64)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "total": total.0,
        "page": pag.page,
        "per_page": pag.per_page,
        "photos": photos.into_iter().map(|(id, path, taken_at, camera)| {
            serde_json::json!({ "id": id, "path": path, "taken_at": taken_at, "camera": camera })
        }).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
pub struct MergeRequest {
    pub source: i64,
    pub target: i64,
}

pub async fn merge_albums(
    State(state): State<AppState>,
    Json(req): Json<MergeRequest>,
) -> StatusCode {
    match album::merge(&state.pool, req.source, req.target).await {
        Ok(()) => StatusCode::OK,
        Err(crate::error::AppError::NotFound(_)) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
