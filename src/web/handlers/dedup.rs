use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use crate::{dedup, web::AppState};

pub async fn list_dedup_groups(
    State(state): State<AppState>,
) -> Result<Json<Vec<dedup::DedupGroup>>, StatusCode> {
    dedup::list_groups(&state.pool)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
pub struct ResolveRequest {
    pub keep: Vec<i64>,
}

pub async fn resolve_group(
    State(state): State<AppState>,
    Path(group_id): Path<i64>,
    Json(req): Json<ResolveRequest>,
) -> StatusCode {
    match dedup::resolve(&state.pool, group_id, &req.keep).await {
        Ok(()) => StatusCode::OK,
        Err(crate::error::AppError::NotFound(_)) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
