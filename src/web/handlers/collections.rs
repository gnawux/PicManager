use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::application::{CallerKind, ServiceErrorCode};
use crate::web::AppState;

#[derive(Debug, Serialize)]
pub struct CollectionRow {
    pub id: i64,
    pub name: String,
    pub photo_count: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CollectionName {
    pub name: String,
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

pub async fn list_collections(
    State(state): State<AppState>,
) -> Result<Json<Vec<CollectionRow>>, StatusCode> {
    let rows: Vec<(i64, String, i64, String)> = sqlx::query_as(
        "SELECT a.id, a.name, COUNT(pa.photo_id) as photo_count, a.created_at
         FROM albums a
         LEFT JOIN photo_albums pa ON pa.album_id = a.id
         WHERE a.kind = 'curated'
         GROUP BY a.id
         ORDER BY a.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, name, photo_count, created_at)| CollectionRow {
                id, name, photo_count, created_at,
            })
            .collect(),
    ))
}

pub async fn create_collection(
    State(state): State<AppState>,
    Json(body): Json<CollectionName>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let context = state.application.request_context(CallerKind::LocalWeb);
    let collection = state.application.collections().create(&context, &body.name).await
        .map_err(collection_status)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({
        "id": collection.id, "name": collection.name
    }))))
}

pub async fn rename_collection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<CollectionName>,
) -> StatusCode {
    let context = state.application.request_context(CallerKind::LocalWeb);
    match state.application.collections().rename(&context, id, &body.name).await {
        Ok(_) => StatusCode::OK,
        Err(error) => collection_status(error),
    }
}

pub async fn delete_collection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> StatusCode {
    let context = state.application.request_context(CallerKind::LocalWeb);
    match state.application.collections().delete(&context, id).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(error) => collection_status(error),
    }
}

pub async fn add_photos(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PhotoIds>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let context = state.application.request_context(CallerKind::LocalWeb);
    let change = state.application.collections().add_photos(&context, id, &body.photo_ids)
        .await.map_err(collection_status)?;
    Ok(Json(serde_json::json!({ "added": change.changed })))
}

pub async fn remove_photos(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PhotoIds>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let context = state.application.request_context(CallerKind::LocalWeb);
    let change = state.application.collections().remove_photos(&context, id, &body.photo_ids)
        .await.map_err(collection_status)?;
    Ok(Json(serde_json::json!({ "removed": change.changed })))
}

fn collection_status(error: crate::application::ServiceError) -> StatusCode {
    match error.code {
        ServiceErrorCode::NotFound => StatusCode::NOT_FOUND,
        ServiceErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn list_collection_photos(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(pag): Query<Pagination>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let exists: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM albums WHERE id = ? AND kind = 'curated'")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists.0 == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    let offset = (pag.page.saturating_sub(1)) as i64 * pag.per_page as i64;
    let total: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
            .bind(id)
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
    .bind(id)
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

#[derive(Debug, Deserialize)]
pub struct PhotoIds {
    pub photo_ids: Vec<i64>,
}
