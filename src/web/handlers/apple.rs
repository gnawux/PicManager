use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::{
    apple::{self, AppleLinkCandidate, AppleSourcePage, AppleSourceView},
    error::AppError,
    web::AppState,
};

#[derive(Debug, Deserialize)]
pub(crate) struct AppleSourceQuery {
    status: Option<String>,
    search: Option<String>,
    before_id: Option<i64>,
    limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RetryResponse {
    job_id: i64,
    source: AppleSourceView,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CandidateQuery {
    before_id: Option<i64>,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReviewRequest {
    accept: bool,
}
#[derive(Debug, Deserialize)] pub(crate) struct AppleExportWorkerRequest { worker_id: String }
#[derive(Debug, Deserialize)] pub(crate) struct AppleExportCommitRequest { worker_id: String, package_path: PathBuf }

#[derive(Debug, Serialize)]
pub(crate) struct ReviewResponse {
    job_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

pub(crate) struct AppleApiError(AppError);

impl From<AppError> for AppleApiError {
    fn from(value: AppError) -> Self {
        Self(value)
    }
}

impl IntoResponse for AppleApiError {
    fn into_response(self) -> Response {
        let (status, code) = match self.0 {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "apple_source_not_found"),
            AppError::Metadata(_) => (StatusCode::CONFLICT, "invalid_source_transition"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        (
            status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code,
                    message: self.0.to_string(),
                },
            }),
        )
            .into_response()
    }
}

pub(crate) async fn list_apple_sources(
    State(state): State<AppState>,
    Query(query): Query<AppleSourceQuery>,
) -> Result<Json<AppleSourcePage>, AppleApiError> {
    Ok(Json(
        apple::list_sources(
            &state.pool,
            query.status.as_deref(),
            query.search.as_deref(),
            query.before_id,
            query.limit.unwrap_or(50),
        )
        .await?,
    ))
}

pub(crate) async fn retry_apple_source(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<RetryResponse>, AppleApiError> {
    let (job_id, source) = apple::retry_source(&state.pool, id).await?;
    Ok(Json(RetryResponse { job_id, source }))
}
pub(crate) async fn claim_apple_export(State(state): State<AppState>, Json(body): Json<AppleExportWorkerRequest>) -> Result<Json<Option<apple::AppleExportClaim>>, AppleApiError> {
    if body.worker_id.is_empty() { return Err(AppError::Metadata("Apple export worker is required".into()).into()); }
    Ok(Json(apple::claim_next_export(&state.pool, &body.worker_id, 300).await?))
}
pub(crate) async fn renew_apple_export(State(state): State<AppState>, Path(item_id): Path<i64>, Json(body): Json<AppleExportWorkerRequest>) -> Result<StatusCode, AppleApiError> {
    apple::renew_export_lease(&state.pool, item_id, &body.worker_id, 300).await?; Ok(StatusCode::NO_CONTENT)
}
pub(crate) async fn commit_apple_export(State(state): State<AppState>, Path(source_id): Path<i64>, Json(body): Json<AppleExportCommitRequest>) -> Result<Json<apple::RenditionCommit>, AppleApiError> {
    let staging = std::fs::canonicalize(state.config.library_path.join(".sync-staging/apple")).map_err(|_| AppError::Metadata("Apple export staging is unavailable".into()))?;
    let package = std::fs::canonicalize(&body.package_path).map_err(|_| AppError::Metadata("Apple export package is unavailable".into()))?;
    if !package.starts_with(&staging) { return Err(AppError::Metadata("Apple export package is outside the managed staging area".into()).into()); }
    Ok(Json(apple::commit_rendition_package_for_lease(&state.pool, source_id, &package, Some(&body.worker_id)).await?))
}

pub(crate) async fn list_apple_candidates(
    State(state): State<AppState>,
    Query(query): Query<CandidateQuery>,
) -> Result<Json<Vec<AppleLinkCandidate>>, AppleApiError> {
    Ok(Json(
        apple::list_link_candidates(&state.pool, query.before_id, query.limit.unwrap_or(50))
            .await?,
    ))
}

pub(crate) async fn review_apple_candidate(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ReviewRequest>,
) -> Result<Json<ReviewResponse>, AppleApiError> {
    Ok(Json(ReviewResponse {
        job_id: apple::review_link(&state.pool, id, body.accept).await?,
    }))
}
