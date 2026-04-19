use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use picmanager::{config::Config, web};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use tower::ServiceExt;

async fn test_app() -> axum::Router {
    let (app, _, _tmp) = test_app_with_pool().await;
    app
}

async fn test_app_with_pool() -> (axum::Router, SqlitePool, tempfile::TempDir) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.thumb_cache_dir = tmp.path().to_path_buf();
    let app = web::router(pool.clone(), config);
    (app, pool, tmp)
}

#[tokio::test]
async fn get_photos_empty_returns_200() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/photos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 0);
    assert!(json["photos"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_import_status_returns_200() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/import/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["running"], false);
}

#[tokio::test]
async fn get_thumb_unknown_id_returns_404() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/photos/9999/thumb")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn start_import_returns_started() {
    let app = test_app().await;
    let body = serde_json::json!({ "dir": "/nonexistent/path" });
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/import")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "started");
}

#[tokio::test]
async fn get_albums_returns_200() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/api/albums").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_album_photos_unknown_returns_404() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/api/albums/9999/photos").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_faces_analyze_returns_job_id() {
    let app = test_app().await;
    let body = serde_json::json!({ "photo_ids": [] });
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/faces/analyze")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["job_id"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn get_faces_job_not_found_returns_404() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/faces/jobs/9999")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_photo_faces_empty_returns_empty_array() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/photos/9999/faces")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn frontend_index_is_served() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    // ServeDir serves index.html; 200 means the file exists and routing works
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_thumb_generates_and_caches() {
    let (app, pool, tmp) = test_app_with_pool().await;

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/with_exif.jpg");
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES (?, 'sha1', 'jpeg', 'imported') RETURNING id",
    )
    .bind(fixture.to_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/photos/{id}/thumb"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    // JPEG magic bytes
    assert_eq!(&body[..2], &[0xFF, 0xD8]);
    // Cache file written
    assert!(tmp.path().join(format!("{id}.jpg")).exists());
}

#[tokio::test]
async fn get_thumb_serves_from_cache() {
    let (app, pool, tmp) = test_app_with_pool().await;

    // Write a sentinel JPEG to the cache dir before any real request
    let fake_jpeg = b"\xFF\xD8\xFF\xE0sentinel";
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES ('/no/such/file.jpg', 'sha2', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    std::fs::write(tmp.path().join(format!("{id}.jpg")), fake_jpeg).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/photos/{id}/thumb"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body.as_ref(), fake_jpeg);
}
