use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    error::AppError,
    jobs::{self, Job},
    sync::{self, SyncJob, SyncJobDetail},
    web::AppState,
};

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    provider: Option<String>,
    status: Option<String>,
    kind: Option<String>,
    source: Option<String>,
    before_id: Option<i64>,
    limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSummary {
    id: i64,
    kind: String,
    provider: Option<String>,
    status: String,
    checkpoint_before: Option<Vec<u8>>,
    checkpoint_after: Option<Vec<u8>>,
    total_items: i64,
    completed_items: i64,
    failed_items: i64,
    error: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    updated_at: String,
    source: &'static str,
    progress_stage: Option<String>,
    attempt_count: Option<i64>,
    max_attempts: Option<i64>,
    correlation_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskList {
    tasks: Vec<TaskSummary>,
    next_before_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TaskDetail {
    #[serde(flatten)]
    task: TaskSummary,
    items: Vec<serde_json::Value>,
    result: Option<serde_json::Value>,
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
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let include_sync = query.source.as_deref() != Some("application");
    let include_application = query.source.as_deref() != Some("sync") && query.provider.is_none();
    let mut tasks = Vec::new();
    if include_sync {
        let before = query.before_id.filter(|id| *id > 0);
        tasks.extend(
            sync::list_jobs(
                &state.pool,
                query.provider.as_deref(),
                query.status.as_deref(),
                before,
                limit,
            )
            .await?
            .into_iter()
            .filter(|job| query.kind.as_ref().is_none_or(|kind| &job.kind == kind))
            .map(sync_summary),
        );
    }
    if include_application {
        let before = query.before_id.filter(|id| *id < 0).map(|id| -id);
        tasks.extend(
            jobs::list(
                &state.pool,
                query.status.as_deref(),
                query.kind.as_deref(),
                before,
                limit,
            )
            .await?
            .into_iter()
            .map(application_summary),
        );
    }
    tasks.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    tasks.truncate(limit as usize);
    let next_before_id = tasks.last().map(|task| task.id);
    Ok(Json(TaskList {
        tasks,
        next_before_id,
    }))
}

pub(crate) async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskDetail>, TaskApiError> {
    if id < 0 {
        return Ok(Json(application_detail(&state, -id).await?));
    }
    Ok(Json(sync_detail(sync::get_job(&state.pool, id).await?)))
}

pub(crate) async fn retry_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskDetail>, TaskApiError> {
    if id < 0 {
        jobs::retry(&state.pool, -id).await?;
        return Ok(Json(application_detail(&state, -id).await?));
    }
    Ok(Json(sync_detail(sync::retry_job(&state.pool, id).await?)))
}

pub(crate) async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskDetail>, TaskApiError> {
    if id < 0 {
        jobs::request_cancel(&state.pool, -id).await?;
        return Ok(Json(application_detail(&state, -id).await?));
    }
    Ok(Json(sync_detail(sync::cancel_job(&state.pool, id).await?)))
}

fn sync_summary(job: SyncJob) -> TaskSummary {
    TaskSummary {
        id: job.id,
        kind: job.kind,
        provider: job.provider,
        status: job.status,
        checkpoint_before: job.checkpoint_before,
        checkpoint_after: job.checkpoint_after,
        total_items: job.total_items,
        completed_items: job.completed_items,
        failed_items: job.failed_items,
        error: job.error,
        created_at: job.created_at,
        started_at: job.started_at,
        finished_at: job.finished_at,
        updated_at: job.updated_at,
        source: "sync",
        progress_stage: None,
        attempt_count: None,
        max_attempts: None,
        correlation_id: None,
    }
}

fn application_summary(job: Job) -> TaskSummary {
    TaskSummary {
        id: -job.id,
        kind: job.kind,
        provider: None,
        status: job.status.clone(),
        checkpoint_before: None,
        checkpoint_after: None,
        total_items: job.progress_total.unwrap_or(0),
        completed_items: job.progress_completed,
        failed_items: i64::from(job.status == "failed"),
        error: job.error_message,
        created_at: job.created_at,
        started_at: job.started_at,
        finished_at: job.finished_at,
        updated_at: job.updated_at,
        source: "application",
        progress_stage: job.progress_stage,
        attempt_count: Some(job.attempt_count),
        max_attempts: Some(job.max_attempts),
        correlation_id: job.correlation_id,
    }
}

fn sync_detail(detail: SyncJobDetail) -> TaskDetail {
    TaskDetail {
        task: sync_summary(detail.job),
        items: detail
            .items
            .into_iter()
            .map(|item| serde_json::to_value(item).expect("sync item is serializable"))
            .collect(),
        result: None,
    }
}

async fn application_detail(state: &AppState, id: i64) -> Result<TaskDetail, AppError> {
    let job = jobs::get(&state.pool, id).await?;
    let result = job
        .result()
        .map_err(|error| AppError::Metadata(error.to_string()))?;
    let attempts: Vec<(
        i64,
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, attempt_number, worker_id, status, error_code, error_message, \
                    started_at, finished_at \
             FROM application_job_attempts WHERE job_id = ? ORDER BY attempt_number",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let public_id = -id;
    let items = attempts
        .into_iter()
        .map(
            |(id, attempt, worker, status, code, message, started, finished)| {
                serde_json::json!({
                    "id": id,
                    "job_id": public_id,
                    "external_id": format!("attempt-{attempt}"),
                    "operation": "attempt",
                    "status": status,
                    "attempt_count": attempt,
                    "max_attempts": attempt,
                    "last_error": message,
                    "worker_id": worker,
                    "error_code": code,
                    "started_at": started,
                    "finished_at": finished,
                })
            },
        )
        .collect();
    Ok(TaskDetail {
        task: application_summary(job),
        items,
        result,
    })
}
