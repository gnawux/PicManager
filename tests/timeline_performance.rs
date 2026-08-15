use std::time::{Duration, Instant};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use picmanager::{config::Config, web};
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

const CATALOG_SIZE: usize = 100_000;
const PAGE_SIZE: usize = 120;
const WARM_QUERY_BUDGET: Duration = Duration::from_secs(1);

#[tokio::test]
async fn timeline_keeps_a_100k_catalog_within_the_query_budget() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "WITH RECURSIVE seq(x) AS (\
             SELECT 1 UNION ALL SELECT x + 1 FROM seq WHERE x < 100000\
         )\
         INSERT INTO photos (path, sha256, format, taken_at, width, height, import_status)\
         SELECT printf('/library/%06d.jpg', x), printf('sha-%06d', x), 'jpeg',\
                datetime('2020-01-01', printf('+%d minutes', x)), 4032, 3024, 'imported'\
         FROM seq",
    )
    .execute(&pool)
    .await
    .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photos")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count as usize, CATALOG_SIZE);

    let cache = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.thumb_cache_dir = cache.path().to_path_buf();
    let app = web::router(pool, config);

    let _warmup = request_page(app.clone(), "/api/timeline?limit=120").await;
    let started = Instant::now();
    let first = request_page(app.clone(), "/api/timeline?limit=120").await;
    let elapsed = started.elapsed();

    assert_eq!(first["items"].as_array().unwrap().len(), PAGE_SIZE);
    assert_eq!(first["has_more"], true);
    assert!(first["next_cursor"].is_string());
    assert!(
        elapsed < WARM_QUERY_BUDGET,
        "warm 100k timeline query took {elapsed:?}, budget is {WARM_QUERY_BUDGET:?}"
    );

    let cursor = first["next_cursor"].as_str().unwrap();
    let second = request_page(app, &format!("/api/timeline?limit=120&cursor={cursor}")).await;
    assert_eq!(second["items"].as_array().unwrap().len(), PAGE_SIZE);
    assert_eq!(second["has_more"], true);
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
}

async fn request_page(app: axum::Router, uri: &str) -> Value {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}
