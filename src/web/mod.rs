pub mod embed;
pub mod handlers;

use axum::{
    Router,
    routing::{get, post},
};
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use crate::config::Config;
use embed::static_handler;
use handlers::{
    albums::{list_albums, list_album_photos, merge_albums},
    animals::{list_species, list_species_photos, list_photo_animals},
    dedup::{list_dedup_groups, resolve_group},
    faces::{start_analyze, get_job_status, list_photo_faces},
    geo::{get_geo_hierarchy, start_regeocode, get_regeocode_status},
    people::{list_people, get_person_photos, get_people_tree, cluster_people, merge_people, reparent_person, get_face_thumb, patch_person, batch_update_people},
    import::{start_import, get_import_status, ImportStatus},
    photos::{list_photos, get_thumb, get_photo, get_gps_points, patch_photo, batch_update_photos},
};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
    pub import_status: Arc<Mutex<ImportStatus>>,
    pub geo_running: Arc<AtomicBool>,
}

pub fn router(pool: SqlitePool, config: Config) -> Router {
    std::fs::create_dir_all(&config.thumb_cache_dir).ok();
    let state = AppState {
        pool,
        config,
        import_status: Arc::new(Mutex::new(ImportStatus::default())),
        geo_running: Arc::new(AtomicBool::new(false)),
    };

    Router::new()
        .route("/api/photos", get(list_photos))
        .route("/api/photos/gps-points", get(get_gps_points))
        .route("/api/photos/batch-update", post(batch_update_photos))
        .route("/api/photos/{id}", get(get_photo).patch(patch_photo))
        .route("/api/photos/{id}/thumb", get(get_thumb))
        .route("/api/import", post(start_import))
        .route("/api/import/status", get(get_import_status))
        .route("/api/dedup", get(list_dedup_groups))
        .route("/api/dedup/{group_id}/resolve", post(resolve_group))
        .route("/api/albums", get(list_albums))
        .route("/api/albums/{id}/photos", get(list_album_photos))
        .route("/api/albums/merge", post(merge_albums))
        .route("/api/geo/hierarchy", get(get_geo_hierarchy))
        .route("/api/geo/regeocode", post(start_regeocode))
        .route("/api/geo/regeocode/status", get(get_regeocode_status))
        .route("/api/people", get(list_people))
        .route("/api/people/tree", get(get_people_tree))
        .route("/api/people/cluster", post(cluster_people))
        .route("/api/people/merge", post(merge_people))
        .route("/api/people/batch-update", post(batch_update_people))
        .route("/api/people/{id}", get(get_person_photos).patch(patch_person))
        .route("/api/people/{id}/reparent", post(reparent_person))
        .route("/api/faces/{id}/thumb", get(get_face_thumb))
        .route("/api/animals/species", get(list_species))
        .route("/api/animals/{species}/photos", get(list_species_photos))
        .route("/api/photos/{id}/animals", get(list_photo_animals))
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
