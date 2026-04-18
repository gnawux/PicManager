use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::web::AppState;

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
}

pub async fn start_import(
    State(state): State<AppState>,
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

    let pool = state.pool.clone();
    let import_status = state.import_status.clone();
    let dir = std::path::PathBuf::from(req.dir.clone());

    tokio::spawn(async move {
        let result = crate::importer::import_dir(&pool, &dir).await;
        let mut status = import_status.lock().unwrap();
        match result {
            Ok(summary) => {
                status.total = summary.total;
                status.imported = summary.imported;
                status.skipped = summary.skipped;
                status.errors = summary.errors;
            }
            Err(e) => {
                tracing::error!("import failed: {e}");
                status.errors += 1;
            }
        }
        status.running = false;
    });

    Ok(Json(serde_json::json!({ "status": "started", "dir": req.dir })))
}

pub async fn get_import_status(
    State(state): State<AppState>,
    Query(_): Query<std::collections::HashMap<String, String>>,
) -> Json<ImportStatus> {
    let status = state.import_status.lock().unwrap().clone();
    Json(status)
}
