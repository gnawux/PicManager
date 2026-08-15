use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    error::AppError,
    sync::{self, SyncJob, SyncJobDetail},
    web::AppState,
};

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    provider: Option<String>,
    status: Option<String>,
    before_id: Option<i64>,
    limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct TaskList {
    tasks: Vec<SyncJob>,
    next_before_id: Option<i64>,
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

pub(crate) struct TaskApiError(AppError);

impl From<AppError> for TaskApiError {
    fn from(value: AppError) -> Self {
        Self(value)
    }
}

impl IntoResponse for TaskApiError {
    fn into_response(self) -> Response {
        let (status, code) = match self.0 {
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "task_not_found"),
            AppError::Metadata(_) => (StatusCode::CONFLICT, "invalid_task_transition"),
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

pub(crate) async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<TaskList>, TaskApiError> {
    let tasks = sync::list_jobs(
        &state.pool,
        query.provider.as_deref(),
        query.status.as_deref(),
        query.before_id,
        query.limit.unwrap_or(30),
    )
    .await?;
    let next_before_id = tasks.last().map(|task| task.id);
    Ok(Json(TaskList {
        tasks,
        next_before_id,
    }))
}

pub(crate) async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<SyncJobDetail>, TaskApiError> {
    Ok(Json(sync::get_job(&state.pool, id).await?))
}

pub(crate) async fn retry_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<SyncJobDetail>, TaskApiError> {
    Ok(Json(sync::retry_job(&state.pool, id).await?))
}

pub(crate) async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<SyncJobDetail>, TaskApiError> {
    Ok(Json(sync::cancel_job(&state.pool, id).await?))
}
