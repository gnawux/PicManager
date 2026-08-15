use crate::application::{ImportCommand, RequestContext};
use crate::jobs::{self, Job};
use crate::web::AppState;
use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportStatus {
    pub running: bool,
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
    pub source_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub dir: String,
    #[serde(default)]
    pub copy: bool,
}

pub async fn start_import(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let jobs = jobs::list(state.application.pool(), None, Some("import"), None, 100)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if jobs.iter().any(|job| matches!(job.status.as_str(), "queued" | "running" | "retry_wait")) {
        return Err(StatusCode::CONFLICT);
    }
    let command = ImportCommand::directory(&req.dir, req.copy);
    let queued = state.application.imports().enqueue(&context, command)
        .await
        .map_err(|error| {
            tracing::error!("failed to enqueue import: {error}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(
        serde_json::json!({ "status": "started", "dir": req.dir, "job_id": queued.job.id }),
    ))
}

pub async fn get_import_status(
    State(state): State<AppState>,
    Query(_): Query<std::collections::HashMap<String, String>>,
) -> Json<ImportStatus> {
    let latest = jobs::list(state.application.pool(), None, Some("import"), None, 1)
        .await
        .ok()
        .and_then(|mut jobs| jobs.pop());
    Json(latest.as_ref().map(status_from_job).unwrap_or_default())
}

fn status_from_job(job: &Job) -> ImportStatus {
    let command = serde_json::from_str::<ImportCommand>(&job.payload_json).ok();
    let result = job.result_json.as_deref()
        .and_then(|value| serde_json::from_str::<crate::jobs::handlers::ImportJobResult>(value).ok());
    ImportStatus {
        running: matches!(job.status.as_str(), "queued" | "running" | "retry_wait"),
        total: result.as_ref().map_or(job.progress_total.unwrap_or(0).max(0) as usize, |value| value.total),
        imported: result.as_ref().map_or(0, |value| value.imported),
        skipped: result.as_ref().map_or(0, |value| value.skipped),
        errors: result.as_ref().map_or(usize::from(job.status == "failed"), |value| value.errors),
        source_dir: command.map(|value| value.source_dir.to_string_lossy().into_owned()),
    }
}
