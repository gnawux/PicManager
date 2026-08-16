use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::application::{RequestContext, ServiceErrorCode};
use crate::web::AppState;

#[derive(Debug, Serialize)]
pub struct CollectionRow {
    pub id: i64,
    pub name: String,
    pub photo_count: i64,
    pub created_at: String,
    pub latest_photo_at: Option<String>,
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
    let rows: Vec<(i64, String, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT a.id, a.name, COUNT(p.id) as photo_count, a.created_at,
                MAX(p.taken_at) as latest_photo_at
         FROM albums a
         LEFT JOIN photo_albums pa ON pa.album_id = a.id
         LEFT JOIN photos p ON p.id = pa.photo_id AND p.import_status = 'imported'
         WHERE a.kind = 'curated'
         GROUP BY a.id
         ORDER BY a.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, name, photo_count, created_at, latest_photo_at)| CollectionRow {
                id, name, photo_count, created_at, latest_photo_at,
            })
            .collect(),
    ))
}

pub async fn create_collection(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Json(body): Json<CollectionName>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let collection = state.application.collections().create(&context, &body.name).await
        .map_err(collection_status)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({
        "id": collection.id, "name": collection.name
    }))))
}

pub async fn rename_collection(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<i64>,
    Json(body): Json<CollectionName>,
) -> StatusCode {
    match state.application.collections().rename(&context, id, &body.name).await {
        Ok(_) => StatusCode::OK,
        Err(error) => collection_status(error),
    }
}

pub async fn delete_collection(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<i64>,
) -> StatusCode {
    match state.application.collections().delete(&context, id).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(error) => collection_status(error),
    }
}

pub async fn add_photos(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<i64>,
    Json(body): Json<PhotoIds>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let change = state.application.collections().add_photos(&context, id, &body.photo_ids)
        .await.map_err(collection_status)?;
    Ok(Json(serde_json::json!({ "added": change.changed })))
}

pub async fn remove_photos(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(id): Path<i64>,
    Json(body): Json<PhotoIds>,
) -> Result<Json<serde_json::Value>, StatusCode> {
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
    let page = pag.page.max(1);
    let per_page = pag.per_page.clamp(1, 200);
    let exists: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM albums WHERE id = ? AND kind = 'curated'")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists.0 == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    let offset = (page - 1) as i64 * per_page as i64;
    let total: (i64,) =
        sqlx::query_as(
            "SELECT COUNT(*) FROM photo_albums pa JOIN photos p ON p.id = pa.photo_id \
             WHERE pa.album_id = ? AND p.import_status = 'imported'",
        )
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let dir = if pag.order == "asc" { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT p.id, p.path, p.taken_at, p.camera
         FROM photos p JOIN photo_albums pa ON pa.photo_id = p.id
         WHERE pa.album_id = ? AND p.import_status = 'imported'
         ORDER BY p.taken_at {dir} NULLS LAST, p.id {dir}
         LIMIT ? OFFSET ?"
    );
    let photos: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(&sql)
    .bind(id)
    .bind(per_page as i64)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "total": total.0,
        "page": page,
        "per_page": per_page,
        "photos": photos.into_iter().map(|(id, path, taken_at, camera)| {
            serde_json::json!({ "id": id, "path": path, "taken_at": taken_at, "camera": camera })
        }).collect::<Vec<_>>()
    })))
}

#[derive(Debug, Deserialize)]
pub struct PhotoIds {
    pub photo_ids: Vec<i64>,
}
