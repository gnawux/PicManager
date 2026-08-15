use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use picmanager::{config::Config, web};
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

async fn app() -> axum::Router {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let cache = tempfile::tempdir().unwrap().keep();
    let mut config = Config::default();
    config.library_path = cache.clone();
    config.db_path = cache.join("picmanager.db");
    config.thumb_cache_dir = cache.join(".thumbs");
    web::router(pool, config)
}

#[tokio::test]
async fn legacy_import_status_contract_remains_available_during_service_refactor() {
    let response = app()
        .await
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
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["running"], false);
    for field in ["total", "imported", "skipped", "errors", "source_dir"] {
        assert!(
            json.get(field).is_some(),
            "missing compatibility field {field}"
        );
    }
}

#[tokio::test]
async fn durable_task_contract_keeps_additive_list_and_detail_shapes() {
    let app = app().await;
    let list = request(app.clone(), "/api/tasks").await;
    assert_eq!(list["tasks"], serde_json::json!([]));
    assert!(list.get("next_before_id").is_some());

    let missing = app
        .oneshot(
            Request::builder()
                .uri("/api/tasks/404")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(missing.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"]["code"], "task_not_found");
    assert!(json["error"]["message"].is_string());
}

#[tokio::test]
async fn unknown_api_routes_do_not_fall_through_to_the_embedded_application() {
    let response = app()
        .await
        .oneshot(
            Request::builder()
                .uri("/api/phase4/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

async fn request(app: axum::Router, uri: &str) -> Value {
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

#[tokio::test]
async fn api_responses_return_a_valid_request_correlation_id() {
    let app = app().await;
    let generated = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/import/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(generated.headers().get("x-request-id").is_some());

    let supplied = app
        .oneshot(
            Request::builder()
                .uri("/api/import/status")
                .header("x-request-id", "mac-app:health_42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        supplied.headers().get("x-request-id").unwrap(),
        "mac-app:health_42"
    );
}
