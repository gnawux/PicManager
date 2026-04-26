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
async fn get_photo_detail_returns_full_metadata() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status, taken_at, camera, gps_lat, gps_lon, timezone_offset)
         VALUES ('/tmp/detail.jpg', 'sha_d', 'jpeg', 'imported', '2024-06-15T10:30:00', 'iPhone 15', 37.77, -122.41, 480)
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/photos/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["id"], id);
    assert_eq!(json["camera"], "iPhone 15");
    assert_eq!(json["timezone_offset"], 480);
    assert!((json["gps_lat"].as_f64().unwrap() - 37.77).abs() < 0.01);
}

#[tokio::test]
async fn get_photo_detail_unknown_returns_404() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/api/photos/9999").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_photo_updates_taken_at_and_timezone() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/p.jpg', 'sha_p', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "taken_at": "2024-06-15T10:30:00", "timezone_offset": 480 });
    let response = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/photos/{id}"))
                .method("PATCH")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let (taken_at, tz): (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT taken_at, timezone_offset FROM photos WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(taken_at.as_deref(), Some("2024-06-15T10:30:00"));
    assert_eq!(tz, Some(480));
}

#[tokio::test]
async fn patch_photo_unknown_id_returns_404() {
    let app = test_app().await;
    let body = serde_json::json!({ "timezone_offset": 0 });
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/photos/9999")
                .method("PATCH")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn batch_update_photos_updates_all() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let id1: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/b1.jpg', 'sha_b1', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let id2: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/b2.jpg', 'sha_b2', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let body = serde_json::json!({
        "photo_ids": [id1, id2],
        "timezone_offset": -300
    });
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/photos/batch-update")
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
    assert_eq!(json["updated"], 2);

    for id in [id1, id2] {
        let tz: Option<i64> =
            sqlx::query_scalar("SELECT timezone_offset FROM photos WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(tz, Some(-300));
    }
}

#[tokio::test]
async fn get_photos_gps_points_returns_only_gps_photos() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    sqlx::query(
        "INSERT INTO photos (path, sha256, format, import_status, gps_lat, gps_lon)
         VALUES ('/gps.jpg', 'shgps', 'jpeg', 'imported', 37.77, -122.41)",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/nogps.jpg', 'shnogps', 'jpeg', 'imported')",
    ).execute(&pool).await.unwrap();

    let resp = app
        .oneshot(Request::builder().uri("/api/photos/gps-points").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let pts = json.as_array().unwrap();
    assert_eq!(pts.len(), 1);
    assert!((pts[0]["gps_lat"].as_f64().unwrap() - 37.77).abs() < 0.01);
}

#[tokio::test]
async fn geo_hierarchy_groups_by_country_state_city() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Insert photos with GPS
    let lat1 = 37.7749f64;
    let lon1 = -122.4194f64;
    let lat2 = 34.0522f64;
    let lon2 = -118.2437f64;
    for (p, lat, lon) in [("/sf.jpg", lat1, lon1), ("/la.jpg", lat2, lon2)] {
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status, gps_lat, gps_lon)
             VALUES (?, ?, 'jpeg', 'imported', ?, ?)",
        )
        .bind(p).bind(p).bind(lat).bind(lon)
        .execute(&pool).await.unwrap();
    }

    // Seed geocache with hierarchy
    let fmt = |v: f64| format!("{:.4}", v);
    sqlx::query(
        "INSERT INTO geocache (lat_key, lon_key, city, state, country)
         VALUES (?, ?, 'San Francisco', 'California', 'United States')",
    )
    .bind(fmt(lat1)).bind(fmt(lon1)).execute(&pool).await.unwrap();

    sqlx::query(
        "INSERT INTO geocache (lat_key, lon_key, city, state, country)
         VALUES (?, ?, 'Los Angeles', 'California', 'United States')",
    )
    .bind(fmt(lat2)).bind(fmt(lon2)).execute(&pool).await.unwrap();

    let response = app
        .oneshot(Request::builder().uri("/api/geo/hierarchy").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let countries = json["countries"].as_array().unwrap();
    assert_eq!(countries.len(), 1);
    assert_eq!(countries[0]["name"], "United States");
    assert_eq!(countries[0]["photo_count"], 2);

    let states = countries[0]["states"].as_array().unwrap();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0]["name"], "California");
    assert_eq!(states[0]["photo_count"], 2);

    let cities = states[0]["cities"].as_array().unwrap();
    assert_eq!(cities.len(), 2);
}

#[tokio::test]
async fn get_people_empty() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/api/people").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn post_people_cluster_returns_count() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/people/cluster")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["people_created"].is_number());
}

#[tokio::test]
async fn post_people_merge_and_reparent() {
    let (_app, pool, tmp) = test_app_with_pool().await;

    // Insert 2 people and a photo + face
    let pid1: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('A') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let pid2: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('B') RETURNING id")
        .fetch_one(&pool).await.unwrap();

    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES ('/m.jpg','shm','jpeg','imported') RETURNING id"
    ).fetch_one(&pool).await.unwrap();
    let face_id: i64 = sqlx::query_scalar(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?,0,0,50,50,0.9) RETURNING id"
    ).bind(photo_id).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
        .bind(pid2).bind(face_id).execute(&pool).await.unwrap();

    let config = {
        let mut c = picmanager::config::Config::default();
        c.thumb_cache_dir = tmp.path().to_path_buf();
        c
    };
    let app = picmanager::web::router(pool.clone(), config);

    // Merge pid2 into pid1
    let body = serde_json::json!({ "source_id": pid2, "target_id": pid1 });
    let resp = app.clone()
        .oneshot(Request::builder().uri("/api/people/merge").method("POST")
            .header("content-type","application/json")
            .body(Body::from(body.to_string())).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // pid2 should be gone
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people WHERE id = ?")
        .bind(pid2).fetch_one(&pool).await.unwrap();
    assert_eq!(exists, 0);
    // face moved to pid1
    let fc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM person_faces WHERE person_id = ?")
        .bind(pid1).fetch_one(&pool).await.unwrap();
    assert_eq!(fc, 1);

    // Reparent pid1 to itself is not tested (would be cyclic); use a new child
    let child_id: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('C') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let body2 = serde_json::json!({ "new_parent_id": pid1 });
    let resp2 = app
        .oneshot(Request::builder().uri(&format!("/api/people/{child_id}/reparent"))
            .method("POST")
            .header("content-type","application/json")
            .body(Body::from(body2.to_string())).unwrap())
        .await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM people WHERE id = ?")
        .bind(child_id).fetch_one(&pool).await.unwrap();
    assert_eq!(parent, Some(pid1));
}

#[tokio::test]
async fn get_face_thumb_returns_jpeg() {
    let (_app, pool, tmp) = test_app_with_pool().await;

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/with_exif.jpg");
    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES (?,  'shft', 'jpeg', 'imported') RETURNING id"
    ).bind(fixture.to_str().unwrap()).fetch_one(&pool).await.unwrap();
    let face_id: i64 = sqlx::query_scalar(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?,10,10,100,100,0.9) RETURNING id"
    ).bind(photo_id).fetch_one(&pool).await.unwrap();

    let mut config = picmanager::config::Config::default();
    config.thumb_cache_dir = tmp.path().to_path_buf();
    let app = picmanager::web::router(pool.clone(), config);

    let resp = app
        .oneshot(Request::builder().uri(&format!("/api/faces/{face_id}/thumb")).body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8]);

    // Cache file written
    assert!(tmp.path().join(format!("face_{face_id}.jpg")).exists());
}

#[tokio::test]
async fn get_face_thumb_unknown_returns_404() {
    let app = test_app().await;
    let resp = app
        .oneshot(Request::builder().uri("/api/faces/9999/thumb").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn people_schema_tree_and_faces() {
    let (_app, pool, _tmp) = test_app_with_pool().await;

    // Insert a root person
    let parent_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name) VALUES ('Alice') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Insert a child person
    let child_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name, parent_id) VALUES ('Alice (child)', ?) RETURNING id",
    )
    .bind(parent_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Verify tree relationship
    let got_parent: Option<i64> =
        sqlx::query_scalar("SELECT parent_id FROM people WHERE id = ?")
            .bind(child_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(got_parent, Some(parent_id));

    // Root has no parent
    let root_parent: Option<i64> =
        sqlx::query_scalar("SELECT parent_id FROM people WHERE id = ?")
            .bind(parent_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(root_parent, None);

    // Insert a photo + face, then link to person
    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/pf.jpg', 'sha_pf', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let face_id: i64 = sqlx::query_scalar(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence)
         VALUES (?, 10, 10, 50, 50, 0.99) RETURNING id",
    )
    .bind(photo_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
        .bind(parent_id)
        .bind(face_id)
        .execute(&pool)
        .await
        .unwrap();

    // Duplicate person_face insertion should fail (PRIMARY KEY)
    let dup = sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
        .bind(parent_id)
        .bind(face_id)
        .execute(&pool)
        .await;
    assert!(dup.is_err());

    // Count faces for person
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM person_faces WHERE person_id = ?")
            .bind(parent_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn timezone_offset_roundtrip() {
    let (_app, pool, _tmp) = test_app_with_pool().await;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status, timezone_offset)
         VALUES ('/tmp/tz.jpg', 'shatz', 'jpeg', 'imported', 480) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let tz: Option<i64> = sqlx::query_scalar("SELECT timezone_offset FROM photos WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tz, Some(480));

    // NULL timezone_offset also works
    let id2: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/notz.jpg', 'shanotz', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let tz2: Option<i64> = sqlx::query_scalar("SELECT timezone_offset FROM photos WHERE id = ?")
        .bind(id2)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tz2, None);
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

#[tokio::test]
async fn animals_schema_exists_and_detect_and_save_is_noop_without_model() {
    let (_app, pool, _tmp) = test_app_with_pool().await;

    // Insert a photo to reference
    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/cat.jpg', 'sha_cat', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Without model file, detect_and_save inserts nothing
    let img = image::DynamicImage::new_rgb8(100, 100);
    picmanager::animal::detect_and_save(&pool, photo_id, &img).await;

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM animals WHERE photo_id = ?")
        .bind(photo_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0, "no animals should be saved when model is absent");
}

#[tokio::test]
async fn animals_table_accepts_manual_insert() {
    let (_app, pool, _tmp) = test_app_with_pool().await;

    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/dog.jpg', 'sha_dog', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO animals (photo_id, species, confidence, x, y, width, height)
         VALUES (?, 'dog', 0.92, 10, 20, 100, 150)",
    )
    .bind(photo_id)
    .execute(&pool)
    .await
    .unwrap();

    let species: (String,) = sqlx::query_as("SELECT species FROM animals WHERE photo_id = ?")
        .bind(photo_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(species.0, "dog");
}

#[tokio::test]
async fn get_animals_species_empty() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/animals/species")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_animals_species_groups_by_species() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Insert two photos with animals
    let p1: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/a.jpg', 'sha_a', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool).await.unwrap();
    let p2: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/b.jpg', 'sha_b', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool).await.unwrap();

    sqlx::query(
        "INSERT INTO animals (photo_id, species, confidence, x, y, width, height)
         VALUES (?, 'cat', 0.9, 0, 0, 100, 100), (?, 'cat', 0.8, 0, 0, 100, 100), (?, 'dog', 0.7, 0, 0, 100, 100)",
    )
    .bind(p1).bind(p2).bind(p1)
    .execute(&pool).await.unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/animals/species")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // cat has 2 distinct photos, dog has 1 — cat should be first
    assert_eq!(arr[0]["species"], "cat");
    assert_eq!(arr[0]["photo_count"], 2);
    assert_eq!(arr[0]["chinese"], "猫");
    assert_eq!(arr[1]["species"], "dog");
    assert_eq!(arr[1]["photo_count"], 1);
}

#[tokio::test]
async fn get_animals_species_photos_paginates() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let p1: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/c.jpg', 'sha_c', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO animals (photo_id, species, confidence, x, y, width, height)
         VALUES (?, 'bird', 0.95, 0, 0, 50, 50)",
    )
    .bind(p1).execute(&pool).await.unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/animals/bird/photos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["photos"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn get_photo_animals_returns_detections() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let p1: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/d.jpg', 'sha_d', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO animals (photo_id, species, confidence, x, y, width, height)
         VALUES (?, 'horse', 0.88, 10, 20, 200, 300)",
    )
    .bind(p1).execute(&pool).await.unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/photos/{p1}/animals"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["species"], "horse");
    assert_eq!(arr[0]["x"], 10);
    assert_eq!(arr[0]["y"], 20);
    assert_eq!(arr[0]["width"], 200);
    assert_eq!(arr[0]["height"], 300);
}

// ── Step 22a: people status management ──────────────────────────────────────

fn make_app_with_pool(
    pool: SqlitePool,
    tmp: &tempfile::TempDir,
) -> axum::Router {
    let mut config = picmanager::config::Config::default();
    config.thumb_cache_dir = tmp.path().to_path_buf();
    picmanager::web::router(pool, config)
}

#[tokio::test]
async fn patch_person_name_updates() {
    let (_app, pool, tmp) = test_app_with_pool().await;
    let pid: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('Old') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let app = make_app_with_pool(pool.clone(), &tmp);

    let body = serde_json::json!({ "name": "New Name" });
    let resp = app.oneshot(
        Request::builder()
            .uri(&format!("/api/people/{pid}"))
            .method("PATCH")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let name: Option<String> = sqlx::query_scalar("SELECT name FROM people WHERE id = ?")
        .bind(pid).fetch_one(&pool).await.unwrap();
    assert_eq!(name.as_deref(), Some("New Name"));
}

#[tokio::test]
async fn patch_person_unknown_returns_404() {
    let app = test_app().await;
    let body = serde_json::json!({ "name": "X" });
    let resp = app.oneshot(
        Request::builder()
            .uri("/api/people/9999")
            .method("PATCH")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_people_default_active_only() {
    let (_app, pool, tmp) = test_app_with_pool().await;
    sqlx::query("INSERT INTO people (name, status) VALUES ('Active', 'active')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO people (name, status) VALUES ('Ignored', 'ignored')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO people (name, status) VALUES ('NotPerson', 'not_a_person')")
        .execute(&pool).await.unwrap();
    let app = make_app_with_pool(pool.clone(), &tmp);

    let resp = app.oneshot(
        Request::builder().uri("/api/people").body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "Active");
}

#[tokio::test]
async fn list_people_status_all_includes_all() {
    let (_app, pool, tmp) = test_app_with_pool().await;
    sqlx::query("INSERT INTO people (name, status) VALUES ('A', 'active')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO people (name, status) VALUES ('B', 'ignored')")
        .execute(&pool).await.unwrap();
    let app = make_app_with_pool(pool.clone(), &tmp);

    let resp = app.oneshot(
        Request::builder().uri("/api/people?status=all").body(Body::empty()).unwrap()
    ).await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 2);
}

// ── Step 26a: GET /api/geo/photos ────────────────────────────────────────────

async fn seed_geo_photo(pool: &SqlitePool, path: &str, sha: &str, lat: f64, lon: f64) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status, gps_lat, gps_lon, taken_at)
         VALUES (?, ?, 'jpeg', 'imported', ?, ?, '2024-06-01 10:00:00') RETURNING id",
    )
    .bind(path).bind(sha).bind(lat).bind(lon)
    .fetch_one(pool).await.unwrap()
}

async fn seed_geocache(pool: &SqlitePool, lat: f64, lon: f64, country: Option<&str>, state: Option<&str>, city: Option<&str>) {
    let fmt = |v: f64| format!("{:.4}", v);
    sqlx::query(
        "INSERT INTO geocache (lat_key, lon_key, country, state, city) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(fmt(lat)).bind(fmt(lon)).bind(country).bind(state).bind(city)
    .execute(pool).await.unwrap();
}

#[tokio::test]
async fn geo_photos_by_country() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    seed_geo_photo(&pool, "/a.jpg", "sha_a", 37.7749, -122.4194).await;
    seed_geo_photo(&pool, "/b.jpg", "sha_b", 34.0522, -118.2437).await;
    seed_geo_photo(&pool, "/c.jpg", "sha_c", 48.8566, 2.3522).await;
    seed_geocache(&pool, 37.7749, -122.4194, Some("USA"), Some("California"), Some("San Francisco")).await;
    seed_geocache(&pool, 34.0522, -118.2437, Some("USA"), Some("California"), Some("Los Angeles")).await;
    seed_geocache(&pool, 48.8566, 2.3522, Some("France"), Some("Île-de-France"), Some("Paris")).await;

    let resp = app
        .oneshot(Request::builder().uri("/api/geo/photos?country=USA").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["total"], 2);
    assert_eq!(json["photos"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn geo_photos_by_state() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    seed_geo_photo(&pool, "/sf.jpg", "sha_sf", 37.7749, -122.4194).await;
    seed_geo_photo(&pool, "/la.jpg", "sha_la", 34.0522, -118.2437).await;
    seed_geo_photo(&pool, "/ny.jpg", "sha_ny", 40.7128, -74.0060).await;
    seed_geocache(&pool, 37.7749, -122.4194, Some("USA"), Some("California"), Some("San Francisco")).await;
    seed_geocache(&pool, 34.0522, -118.2437, Some("USA"), Some("California"), Some("Los Angeles")).await;
    seed_geocache(&pool, 40.7128, -74.0060, Some("USA"), Some("New York"), Some("New York City")).await;

    let resp = app
        .oneshot(Request::builder().uri("/api/geo/photos?country=USA&state=California").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["total"], 2);
}

#[tokio::test]
async fn geo_photos_by_city() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    seed_geo_photo(&pool, "/sf.jpg", "sha_sf2", 37.7749, -122.4194).await;
    seed_geo_photo(&pool, "/la.jpg", "sha_la2", 34.0522, -118.2437).await;
    seed_geocache(&pool, 37.7749, -122.4194, Some("USA"), Some("California"), Some("San Francisco")).await;
    seed_geocache(&pool, 34.0522, -118.2437, Some("USA"), Some("California"), Some("Los Angeles")).await;

    let resp = app
        .oneshot(Request::builder().uri("/api/geo/photos?country=USA&state=California&city=San+Francisco").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["total"], 1);
}

#[tokio::test]
async fn geo_photos_null_city() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    seed_geo_photo(&pool, "/known.jpg", "sha_kn", 37.7749, -122.4194).await;
    seed_geo_photo(&pool, "/unknown.jpg", "sha_unk", 34.0522, -118.2437).await;
    seed_geocache(&pool, 37.7749, -122.4194, Some("USA"), Some("California"), Some("San Francisco")).await;
    seed_geocache(&pool, 34.0522, -118.2437, Some("USA"), Some("California"), None).await;

    let resp = app
        .oneshot(Request::builder().uri("/api/geo/photos?country=USA&state=California&city=__null__").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["total"], 1);
    assert_eq!(json["photos"][0]["path"], "/unknown.jpg");
}

#[tokio::test]
async fn geo_photos_pagination() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    for i in 0..5u32 {
        let lat = 37.0 + i as f64 * 0.001;
        seed_geo_photo(&pool, &format!("/p{i}.jpg"), &format!("sha_pg{i}"), lat, -122.0).await;
        seed_geocache(&pool, lat, -122.0, Some("USA"), Some("California"), Some("TestCity")).await;
    }

    let resp = app
        .oneshot(Request::builder().uri("/api/geo/photos?country=USA&state=California&city=TestCity&page=1&per_page=2").body(Body::empty()).unwrap())
        .await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["total"], 5);
    assert_eq!(json["photos"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn patch_person_status_ignored_hides_from_list() {
    let (_app, pool, tmp) = test_app_with_pool().await;
    let pid: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('Alice') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let app = make_app_with_pool(pool.clone(), &tmp);

    let body = serde_json::json!({ "status": "ignored" });
    let resp = app.clone().oneshot(
        Request::builder()
            .uri(&format!("/api/people/{pid}"))
            .method("PATCH")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp2 = app.oneshot(
        Request::builder().uri("/api/people").body(Body::empty()).unwrap()
    ).await.unwrap();
    let bytes = axum::body::to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn batch_update_people_status() {
    let (_app, pool, tmp) = test_app_with_pool().await;
    let pid1: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('P1') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let pid2: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('P2') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let _pid3: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('P3') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let app = make_app_with_pool(pool.clone(), &tmp);

    let body = serde_json::json!({ "ids": [pid1, pid2], "status": "not_a_person" });
    let resp = app.oneshot(
        Request::builder()
            .uri("/api/people/batch-update")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["updated"], 2);

    let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people WHERE status='active'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(active, 1);
}

#[tokio::test]
async fn people_name_exact_search() {
    let (_app, pool, tmp) = test_app_with_pool().await;
    sqlx::query("INSERT INTO people (name) VALUES ('Zhang San')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO people (name) VALUES ('Zhang San')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO people (name) VALUES ('Li Si')").execute(&pool).await.unwrap();
    let app = make_app_with_pool(pool.clone(), &tmp);

    let resp = app.oneshot(
        Request::builder()
            .uri("/api/people?name_exact=Zhang+San")
            .body(Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn get_person_photos_includes_descendant_photos() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Build a 3-level tree: A → B → C
    let pa: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('A') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let pb: i64 = sqlx::query_scalar("INSERT INTO people (name, parent_id) VALUES ('B', ?) RETURNING id")
        .bind(pa).fetch_one(&pool).await.unwrap();
    let pc: i64 = sqlx::query_scalar("INSERT INTO people (name, parent_id) VALUES ('C', ?) RETURNING id")
        .bind(pb).fetch_one(&pool).await.unwrap();

    // One photo + face per person
    for (label, sha, person_id) in [("pA", "sha27a1", pa), ("pB", "sha27a2", pb), ("pC", "sha27a3", pc)] {
        let photo_id: i64 = sqlx::query_scalar(
            "INSERT INTO photos (path, sha256, format, import_status) VALUES (?, ?, 'jpeg', 'imported') RETURNING id",
        )
        .bind(format!("/{label}.jpg")).bind(sha)
        .fetch_one(&pool).await.unwrap();
        let face_id: i64 = sqlx::query_scalar(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?,0,0,50,50,0.9) RETURNING id",
        )
        .bind(photo_id).fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
            .bind(person_id).bind(face_id).execute(&pool).await.unwrap();
    }

    let resp = app
        .oneshot(Request::builder()
            .uri(&format!("/api/people/{pa}?per_page=50"))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["total"], 3, "should include photos from A, B, and C");
    assert_eq!(json["photos"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn get_person_photos_pagination_with_descendants() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pa: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('PA') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let pb: i64 = sqlx::query_scalar("INSERT INTO people (name, parent_id) VALUES ('PB', ?) RETURNING id")
        .bind(pa).fetch_one(&pool).await.unwrap();

    for (i, person_id) in [(1, pa), (2, pa), (3, pb)] {
        let photo_id: i64 = sqlx::query_scalar(
            "INSERT INTO photos (path, sha256, format, import_status) VALUES (?, ?, 'jpeg', 'imported') RETURNING id",
        )
        .bind(format!("/pg{i}.jpg")).bind(format!("sha27b{i}"))
        .fetch_one(&pool).await.unwrap();
        let face_id: i64 = sqlx::query_scalar(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?,0,0,50,50,0.9) RETURNING id",
        )
        .bind(photo_id).fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
            .bind(person_id).bind(face_id).execute(&pool).await.unwrap();
    }

    // Page 1: 2 photos
    let resp1 = app.clone()
        .oneshot(Request::builder()
            .uri(&format!("/api/people/{pa}?per_page=2&page=1"))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    let bytes1 = axum::body::to_bytes(resp1.into_body(), usize::MAX).await.unwrap();
    let j1: serde_json::Value = serde_json::from_slice(&bytes1).unwrap();
    assert_eq!(j1["total"], 3);
    assert_eq!(j1["photos"].as_array().unwrap().len(), 2);

    // Page 2: 1 photo
    let resp2 = app
        .oneshot(Request::builder()
            .uri(&format!("/api/people/{pa}?per_page=2&page=2"))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    let bytes2 = axum::body::to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let j2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap();
    assert_eq!(j2["photos"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn get_albums_latest_photo_at_null_when_no_photos() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    sqlx::query("INSERT INTO albums (name, kind) VALUES ('2024-06', 'time')")
        .execute(&pool).await.unwrap();

    let resp = app
        .oneshot(Request::builder().uri("/api/albums").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert!(arr[0]["latest_photo_at"].is_null(), "empty album should have null latest_photo_at");
}

#[tokio::test]
async fn get_albums_latest_photo_at_is_most_recent() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let album_id: i64 = sqlx::query_scalar(
        "INSERT INTO albums (name, kind) VALUES ('2024-06', 'time') RETURNING id",
    )
    .fetch_one(&pool).await.unwrap();

    let p1: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status, taken_at)
         VALUES ('/a.jpg', 'sha_a1', 'jpeg', 'imported', '2024-06-01 10:00:00') RETURNING id",
    )
    .fetch_one(&pool).await.unwrap();

    let p2: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status, taken_at)
         VALUES ('/b.jpg', 'sha_b1', 'jpeg', 'imported', '2024-06-30 18:00:00') RETURNING id",
    )
    .fetch_one(&pool).await.unwrap();

    sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
        .bind(p1).bind(album_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
        .bind(p2).bind(album_id).execute(&pool).await.unwrap();

    let resp = app
        .oneshot(Request::builder().uri("/api/albums").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0]["latest_photo_at"].as_str().unwrap(),
        "2024-06-30 18:00:00",
        "should return the latest taken_at among all photos in the album"
    );
}

