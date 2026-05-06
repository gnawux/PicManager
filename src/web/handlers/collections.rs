use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
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
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO albums (name, kind) VALUES (?, 'curated') RETURNING id",
    )
    .bind(&name)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id, "name": name }))))
}

pub async fn rename_collection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<CollectionName>,
) -> StatusCode {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    let result = sqlx::query(
        "UPDATE albums SET name = ? WHERE id = ? AND kind = 'curated'",
    )
    .bind(&name)
    .bind(id)
    .execute(&state.pool)
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => StatusCode::OK,
        Ok(_) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn delete_collection(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> StatusCode {
    // Verify it's a curated album before deleting
    let exists: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM albums WHERE id = ? AND kind = 'curated'",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or((0,));

    if exists.0 == 0 {
        return StatusCode::NOT_FOUND;
    }

    match sqlx::query("DELETE FROM albums WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn add_photos(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PhotoIds>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify collection exists
    let exists: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM albums WHERE id = ? AND kind = 'curated'")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists.0 == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    let mut added: u64 = 0;
    for photo_id in &body.photo_ids {
        let r = sqlx::query(
            "INSERT OR IGNORE INTO photo_albums (photo_id, album_id) VALUES (?, ?)",
        )
        .bind(photo_id)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        added += r.rows_affected();
    }

    Ok(Json(serde_json::json!({ "added": added })))
}

pub async fn remove_photos(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PhotoIds>,
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

    if body.photo_ids.is_empty() {
        return Ok(Json(serde_json::json!({ "removed": 0 })));
    }

    let placeholders = body.photo_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "DELETE FROM photo_albums WHERE album_id = ? AND photo_id IN ({placeholders})"
    );
    let mut q = sqlx::query(&sql).bind(id);
    for pid in &body.photo_ids {
        q = q.bind(pid);
    }
    let removed = q
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .rows_affected();

    Ok(Json(serde_json::json!({ "removed": removed })))
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
