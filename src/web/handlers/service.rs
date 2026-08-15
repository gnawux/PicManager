use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ServiceContract {
    pub api_version: &'static str,
    pub service_version: &'static str,
    pub minimum_client_api: &'static str,
    pub local_trusted_only: bool,
    pub library_fingerprint: String,
    pub capabilities: &'static [&'static str],
}

pub async fn get_service_contract(
    State(state): State<crate::web::AppState>,
) -> Json<ServiceContract> {
    Json(ServiceContract {
        api_version: "v1",
        service_version: env!("CARGO_PKG_VERSION"),
        minimum_client_api: "v1",
        local_trusted_only: true,
        library_fingerprint: library_fingerprint(&state.config.library_path),
        capabilities: &[
            "durable_jobs",
            "worker_metrics",
            "health",
            "diagnostics",
            "apple_inventory",
            "filesystem_recovery",
        ],
    })
}

fn library_fingerprint(path: &Path) -> String {
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    hex::encode(Sha256::digest(normalized.to_string_lossy().as_bytes()))
}

pub async fn get_worker_metrics(
    State(state): State<crate::web::AppState>,
) -> Result<Json<crate::jobs::JobMetrics>, StatusCode> {
    crate::jobs::metrics(&state.pool)
        .await
        .map(Json)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_fingerprint_is_stable_and_path_specific() {
        let first = library_fingerprint(Path::new("/tmp/picmanager-library-a"));
        assert_eq!(
            first,
            library_fingerprint(Path::new("/tmp/picmanager-library-a"))
        );
        assert_ne!(
            first,
            library_fingerprint(Path::new("/tmp/picmanager-library-b"))
        );
        assert_eq!(first.len(), 64);
    }
}
