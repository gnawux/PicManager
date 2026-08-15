use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ServiceContract {
    pub api_version: &'static str,
    pub service_version: &'static str,
    pub minimum_client_api: &'static str,
    pub local_trusted_only: bool,
    pub capabilities: &'static [&'static str],
}

pub async fn get_service_contract() -> Json<ServiceContract> {
    Json(ServiceContract {
        api_version: "v1",
        service_version: env!("CARGO_PKG_VERSION"),
        minimum_client_api: "v1",
        local_trusted_only: true,
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

pub async fn get_worker_metrics(
    State(state): State<crate::web::AppState>,
) -> Result<Json<crate::jobs::JobMetrics>, StatusCode> {
    crate::jobs::metrics(&state.pool)
        .await
        .map(Json)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
