use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use crate::web::AppState;

#[derive(Debug, Deserialize)]
pub struct AnalyzeRequest {
    #[serde(default)]
    pub photo_ids: Vec<i64>,
    #[serde(default)]
    pub missing_only: bool,
}

#[derive(Debug, Serialize)]
pub struct JobStatusResponse {
    pub id: i64,
    pub status: String,
    pub total: Option<i64>,
    pub processed: i64,
}

#[derive(Debug, Serialize)]
pub struct FaceResponse {
    pub id: i64,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub confidence: f64,
    pub person_id: Option<i64>,
    pub person_name: Option<String>,
}

pub async fn start_analyze(
    State(state): State<AppState>,
    Json(req): Json<AnalyzeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let scope = if req.missing_only {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM photos WHERE import_status = 'imported' \
             AND NOT EXISTS (SELECT 1 FROM faces WHERE faces.photo_id = photos.id)",
        )
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Some(ids)
    } else if req.photo_ids.is_empty() {
        None
    } else {
        Some(req.photo_ids)
    };
    let job_id = crate::face::job::run_job(&state.pool, scope)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "job_id": job_id })))
}

pub async fn get_job_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<JobStatusResponse>, StatusCode> {
    let row = sqlx::query(
        "SELECT id, status, total, processed FROM face_jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(JobStatusResponse {
        id: row.get("id"),
        status: row.get("status"),
        total: row.get("total"),
        processed: row.get("processed"),
    }))
}

pub async fn list_photo_faces(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
) -> Result<Json<Vec<FaceResponse>>, StatusCode> {
    let rows = sqlx::query(
        "SELECT f.id, f.x, f.y, f.width, f.height, f.confidence,
                pf.person_id, p.name AS person_name
         FROM faces f
         LEFT JOIN person_faces pf ON pf.face_id = f.id
         LEFT JOIN people p ON p.id = pf.person_id
         WHERE f.photo_id = ? ORDER BY f.id",
    )
    .bind(photo_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let faces = rows
        .into_iter()
        .map(|r| FaceResponse {
            id: r.get("id"),
            x: r.get("x"),
            y: r.get("y"),
            width: r.get("width"),
            height: r.get("height"),
            confidence: r.get("confidence"),
            person_id: r.get("person_id"),
            person_name: r.get("person_name"),
        })
        .collect();

    Ok(Json(faces))
}
