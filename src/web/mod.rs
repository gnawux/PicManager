pub mod handlers;

use axum::{
    Router,
    routing::{get, post},
};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use crate::config::Config;
use handlers::{
    import::{start_import, get_import_status, ImportStatus},
    photos::{list_photos, get_thumb},
};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
    pub import_status: Arc<Mutex<ImportStatus>>,
}

pub fn router(pool: SqlitePool, config: Config) -> Router {
    let state = AppState {
        pool,
        config,
        import_status: Arc::new(Mutex::new(ImportStatus::default())),
    };

    Router::new()
        .route("/api/photos", get(list_photos))
        .route("/api/photos/{id}/thumb", get(get_thumb))
        .route("/api/import", post(start_import))
        .route("/api/import/status", get(get_import_status))
        .with_state(state)
}

pub async fn serve(pool: SqlitePool, config: Config) -> anyhow::Result<()> {
    let addr = config.bind_addr();
    let app = router(pool, config);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Web 服务启动：http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
