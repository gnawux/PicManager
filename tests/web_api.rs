use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use picmanager::{config::Config, web};
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

async fn test_app() -> axum::Router {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let config = Config::default();
    web::router(pool, config)
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
