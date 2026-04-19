pub mod embed;
pub mod handlers;

use axum::{
    Router,
    routing::{get, post},
};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use crate::config::Config;
use embed::static_handler;
use handlers::{
    albums::{list_albums, list_album_photos, merge_albums},
    dedup::{list_dedup_groups, resolve_group},
    faces::{start_analyze, get_job_status, list_photo_faces},
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
        .route("/api/albums/merge", post(merge_albums))
        .route("/api/faces/analyze", post(start_analyze))
        .route("/api/faces/jobs/{id}", get(get_job_status))
        .route("/api/photos/{id}/faces", get(list_photo_faces))
        .with_state(state)
        .fallback(static_handler)
}

pub async fn serve(pool: SqlitePool, config: Config) -> anyhow::Result<()> {
    let addr = config.bind_addr();
    let app = router(pool, config);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Web 服务启动：http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
