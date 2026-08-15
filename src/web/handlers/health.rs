use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::storage::HealthReport;
use crate::web::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct HealthQuery {
    #[serde(default)]
    deep: bool,
}

pub async fn get_health(
    State(state): State<AppState>,
    Query(query): Query<HealthQuery>,
) -> Result<Json<HealthReport>, StatusCode> {
    crate::storage::health_report(&state.pool, &state.config, query.deep)
        .await
        .map(Json)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
