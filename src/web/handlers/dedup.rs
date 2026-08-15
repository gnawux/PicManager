use crate::{
    application::{RequestContext, ServiceErrorCode},
    dedup,
    web::AppState,
};
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use serde::Deserialize;

pub async fn list_dedup_groups(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
) -> Result<Json<Vec<dedup::DedupGroup>>, StatusCode> {
    state
        .application
        .dedup()
        .list(&context)
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
    Extension(context): Extension<RequestContext>,
    Path(group_id): Path<i64>,
    Json(req): Json<ResolveRequest>,
) -> StatusCode {
    match state
        .application
        .dedup()
        .resolve(&context, group_id, &req.keep)
        .await
    {
        Ok(()) => StatusCode::OK,
        Err(error) if error.code == ServiceErrorCode::NotFound => StatusCode::NOT_FOUND,
        Err(error) if error.code == ServiceErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
