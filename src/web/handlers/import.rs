use crate::application::{ImportCommand, RequestContext};
use crate::importer::SharedImportProgress;
use crate::web::AppState;
use axum::{
    Json,
    extract::{Extension, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportStatus {
    pub running: bool,
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
    pub source_dir: Option<String>,
}

pub type SharedImportStatus = Arc<Mutex<ImportStatus>>;

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
    let mut status = state.import_status.lock().unwrap();
    if status.running {
        return Err(StatusCode::CONFLICT);
    }
    *status = ImportStatus {
        running: true,
        source_dir: Some(req.dir.clone()),
        ..Default::default()
    };
    drop(status);

    let application = state.application.clone();
    let import_status = state.import_status.clone();
    let command = ImportCommand::directory(&req.dir, req.copy);

    tokio::spawn(async move {
        let result = application
            .imports()
            .execute(&context, command, SharedImportProgress::default())
            .await;
        let mut status = import_status.lock().unwrap();
        match result {
            Ok(result) => {
                status.total = result.summary.total;
                status.imported = result.summary.imported;
                status.skipped = result.summary.skipped;
                status.errors = result.summary.errors;
            }
            Err(e) => {
                tracing::error!("import failed: {e}");
                status.errors += 1;
            }
        }
        status.running = false;
    });

    Ok(Json(
        serde_json::json!({ "status": "started", "dir": req.dir }),
    ))
}

pub async fn get_import_status(
    State(state): State<AppState>,
    Query(_): Query<std::collections::HashMap<String, String>>,
) -> Json<ImportStatus> {
    let status = state.import_status.lock().unwrap().clone();
    Json(status)
}
