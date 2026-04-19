pub mod handlers;

use axum::{
    Router,
    routing::{get, post},
};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tower_http::services::ServeDir;
use crate::config::Config;
use handlers::{
    albums::{list_albums, list_album_photos},
    dedup::{list_dedup_groups, resolve_group},
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
        .route("/api/dedup", get(list_dedup_groups))
        .route("/api/dedup/{group_id}/resolve", post(resolve_group))
        .route("/api/albums", get(list_albums))
        .route("/api/albums/{id}/photos", get(list_album_photos))
        .with_state(state)
        .fallback_service(ServeDir::new("frontend").append_index_html_on_directories(true))
}

pub async fn serve(pool: SqlitePool, config: Config) -> anyhow::Result<()> {
    let addr = config.bind_addr();
    let app = router(pool, config);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Web 服务启动：http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
