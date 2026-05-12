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

// ── Activities API tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn get_activities_empty_returns_200() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/api/activities").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total"], 0);
    assert!(json["activities"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_activity_not_found_returns_404() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::builder().uri("/api/activities/9999").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_activity_track_after_insert() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    sqlx::query(
        "INSERT INTO activities (id, sha256, source_path, file_format, activity_type, import_status) \
         VALUES (1, 'abc', '/tmp/a.gpx', 'gpx', 'running', 'imported')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO activity_track_points (activity_id, ts, lat, lon) VALUES (1,'2024-06-15T10:00:00Z',39.9,116.4)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let response = app
        .oneshot(Request::builder().uri("/api/activities/1/track").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["original_count"], 1);
    assert_eq!(json["points"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn get_activity_photos_empty_when_no_photos() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    sqlx::query(
        "INSERT INTO activities (id, sha256, source_path, file_format, activity_type, \
         start_time, end_time, import_status) \
         VALUES (1,'abc','/tmp/a.gpx','gpx','running','2024-06-15T10:00:00Z','2024-06-15T11:00:00Z','imported')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let response = app
        .oneshot(
            Request::builder().uri("/api/activities/1/photos").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["photos"].as_array().unwrap().is_empty());
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
async fn merge_parent_into_child_promotes_child() {
    // G → P → C  (G top-level, P child of G, C child of P)
    // Merging P (source) into C (target) should promote C to G's level.
    let (_app, pool, tmp) = test_app_with_pool().await;

    let g_id: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('G') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let p_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name, parent_id) VALUES ('P', ?) RETURNING id",
    )
    .bind(g_id).fetch_one(&pool).await.unwrap();
    let c_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name, parent_id) VALUES ('C', ?) RETURNING id",
    )
    .bind(p_id).fetch_one(&pool).await.unwrap();

    // Give P a face so we can verify it moves to C
    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES ('/x.jpg','sh1','jpeg','imported') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let face_id: i64 = sqlx::query_scalar(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?,0,0,50,50,0.9) RETURNING id",
    ).bind(photo_id).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
        .bind(p_id).bind(face_id).execute(&pool).await.unwrap();

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app = picmanager::web::router(pool.clone(), config);

    let body = serde_json::json!({ "source_id": p_id, "target_id": c_id });
    let resp = app
        .oneshot(Request::builder().uri("/api/people/merge").method("POST")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // P must be gone
    let p_exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people WHERE id = ?")
        .bind(p_id).fetch_one(&pool).await.unwrap();
    assert_eq!(p_exists, 0);

    // C must be promoted to G's level
    let c_parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM people WHERE id = ?")
        .bind(c_id).fetch_one(&pool).await.unwrap();
    assert_eq!(c_parent, Some(g_id), "C should be promoted to G's level");

    // P's face must have moved to C
    let fc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM person_faces WHERE person_id = ?")
        .bind(c_id).fetch_one(&pool).await.unwrap();
    assert_eq!(fc, 1);
}

#[tokio::test]
async fn merge_parent_into_child_reparents_siblings() {
    // G → P → {C, D}  (D is sibling of C under P)
    // After merging P→C: C promoted to G's level, D becomes child of C.
    let (_app, pool, tmp) = test_app_with_pool().await;

    let g_id: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('G') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let p_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name, parent_id) VALUES ('P', ?) RETURNING id",
    )
    .bind(g_id).fetch_one(&pool).await.unwrap();
    let c_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name, parent_id) VALUES ('C', ?) RETURNING id",
    )
    .bind(p_id).fetch_one(&pool).await.unwrap();
    let d_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name, parent_id) VALUES ('D', ?) RETURNING id",
    )
    .bind(p_id).fetch_one(&pool).await.unwrap();

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app = picmanager::web::router(pool.clone(), config);

    let body = serde_json::json!({ "source_id": p_id, "target_id": c_id });
    let resp = app
        .oneshot(Request::builder().uri("/api/people/merge").method("POST")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // C promoted to G's level
    let c_parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM people WHERE id = ?")
        .bind(c_id).fetch_one(&pool).await.unwrap();
    assert_eq!(c_parent, Some(g_id));

    // D re-parented to C
    let d_parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM people WHERE id = ?")
        .bind(d_id).fetch_one(&pool).await.unwrap();
    assert_eq!(d_parent, Some(c_id), "D (sibling) should be re-parented under C");
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
async fn get_photo_faces_includes_person_info() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Insert a photo, a face, a person, and link them
    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES ('/f.jpg','sha_f28','jpeg','imported') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let face_id: i64 = sqlx::query_scalar(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?,10,10,60,60,0.9) RETURNING id",
    ).bind(photo_id).fetch_one(&pool).await.unwrap();
    let person_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name) VALUES ('Alice') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
        .bind(person_id).bind(face_id).execute(&pool).await.unwrap();

    let resp = app
        .oneshot(Request::builder()
            .uri(&format!("/api/photos/{photo_id}/faces"))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["person_id"].as_i64().unwrap(), person_id);
    assert_eq!(arr[0]["person_name"].as_str().unwrap(), "Alice");
}

#[tokio::test]
async fn get_photo_faces_person_null_when_unassigned() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES ('/g.jpg','sha_g28','jpeg','imported') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?,5,5,40,40,0.8) RETURNING id",
    ).bind(photo_id).fetch_one(&pool).await.unwrap();

    let resp = app
        .oneshot(Request::builder()
            .uri(&format!("/api/photos/{photo_id}/faces"))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert!(arr[0]["person_id"].is_null(), "unassigned face should have null person_id");
    assert!(arr[0]["person_name"].is_null());
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
async fn get_people_tree_includes_cover_face_id() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pa: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('Alice') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES ('/t.jpg','sha_t','jpeg','imported') RETURNING id",
    ).fetch_one(&pool).await.unwrap();
    let face_id: i64 = sqlx::query_scalar(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence) VALUES (?,0,0,50,50,0.9) RETURNING id",
    ).bind(photo_id).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
        .bind(pa).bind(face_id).execute(&pool).await.unwrap();

    let resp = app
        .oneshot(Request::builder().uri("/api/people/tree").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let people = json["people"].as_array().unwrap();
    assert_eq!(people.len(), 1);
    assert_eq!(
        people[0]["cover_face_id"].as_i64().unwrap(),
        face_id,
        "tree node should expose cover_face_id"
    );
}

#[tokio::test]
async fn get_people_tree_cover_face_id_null_when_no_face() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    sqlx::query("INSERT INTO people (name) VALUES ('Bob')")
        .execute(&pool).await.unwrap();

    let resp = app
        .oneshot(Request::builder().uri("/api/people/tree").body(Body::empty()).unwrap())
        .await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let people = json["people"].as_array().unwrap();
    assert_eq!(people.len(), 1);
    assert!(people[0]["cover_face_id"].is_null(), "should be null when no face");
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

// ── helpers for embedding tests ───────────────────────────────────────────────

fn unit_emb(dim: usize, hot: usize) -> Vec<u8> {
    let mut v = vec![0.0f32; dim];
    v[hot] = 1.0;
    picmanager::face::embedder::encode_embedding(&v)
}

async fn insert_photo_plain(pool: &SqlitePool, suffix: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status) VALUES (?,?,  'jpeg', 'imported') RETURNING id"
    )
    .bind(format!("/p_{suffix}.jpg"))
    .bind(format!("sha_{suffix}"))
    .fetch_one(pool).await.unwrap()
}

async fn insert_face_emb(pool: &SqlitePool, photo_id: i64, emb: &[u8]) -> i64 {
    insert_face_emb_with_conf(pool, photo_id, emb, 0.95).await
}

async fn insert_face_emb_with_conf(pool: &SqlitePool, photo_id: i64, emb: &[u8], conf: f32) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence, embedding) \
         VALUES (?, 0, 0, 50, 50, ?, ?) RETURNING id"
    )
    .bind(photo_id)
    .bind(conf)
    .bind(emb)
    .fetch_one(pool).await.unwrap()
}

async fn create_named_person(pool: &SqlitePool, name: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO people (name) VALUES (?) RETURNING id")
        .bind(name)
        .fetch_one(pool).await.unwrap()
}

async fn create_unnamed_person(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("INSERT INTO people DEFAULT VALUES RETURNING id")
        .fetch_one(pool).await.unwrap()
}

async fn link_face(pool: &SqlitePool, person_id: i64, face_id: i64) {
    sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
        .bind(person_id).bind(face_id)
        .execute(pool).await.unwrap();
}

// ── people list confidence filter tests ──────────────────────────────────────

#[tokio::test]
async fn list_people_hides_unnamed_all_low_confidence() {
    // Unnamed person with only low-confidence faces must not appear in the
    // default active list, but should still appear with status=all.
    let (_app, pool, tmp) = test_app_with_pool().await;

    // Named person with low-confidence face → must appear (name overrides filter)
    let named_id: i64 = sqlx::query_scalar("INSERT INTO people (name) VALUES ('Alice') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let p1 = insert_photo_plain(&pool, "cf1").await;
    link_face(&pool, named_id,
        insert_face_emb_with_conf(&pool, p1, &unit_emb(4, 0), 0.30).await).await;

    // Unnamed person with only low-confidence face → must be hidden
    let low_id: i64 = sqlx::query_scalar("INSERT INTO people DEFAULT VALUES RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let p2 = insert_photo_plain(&pool, "cf2").await;
    link_face(&pool, low_id,
        insert_face_emb_with_conf(&pool, p2, &unit_emb(4, 1), 0.40).await).await;

    // Unnamed person with a high-confidence face → must appear
    let high_id: i64 = sqlx::query_scalar("INSERT INTO people DEFAULT VALUES RETURNING id")
        .fetch_one(&pool).await.unwrap();
    let p3 = insert_photo_plain(&pool, "cf3").await;
    link_face(&pool, high_id,
        insert_face_emb_with_conf(&pool, p3, &unit_emb(4, 2), 0.80).await).await;

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app = picmanager::web::router(pool.clone(), config);

    let resp = app.clone()
        .oneshot(Request::builder().uri("/api/people").body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let arr: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let ids: Vec<i64> = arr.as_array().unwrap()
        .iter().map(|p| p["id"].as_i64().unwrap()).collect();

    assert!(ids.contains(&named_id), "named person must appear regardless of confidence");
    assert!(!ids.contains(&low_id), "unnamed all-low-confidence person must be hidden");
    assert!(ids.contains(&high_id), "unnamed person with high-confidence face must appear");

    // status=all must still return the low-confidence person
    let resp2 = app
        .oneshot(Request::builder().uri("/api/people?status=all").body(Body::empty()).unwrap())
        .await.unwrap();
    let bytes2 = axum::body::to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
    let arr2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap();
    let ids2: Vec<i64> = arr2.as_array().unwrap()
        .iter().map(|p| p["id"].as_i64().unwrap()).collect();
    assert!(ids2.contains(&low_id), "status=all must bypass the confidence filter");
}

// ── merge suggestions tests ───────────────────────────────────────────────────

#[tokio::test]
async fn merge_suggestions_returns_similar_person_first() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Alice: embedding at dim-0
    let alice_id = create_named_person(&pool, "Alice").await;
    let p1 = insert_photo_plain(&pool, "ms1").await;
    let f1 = insert_face_emb(&pool, p1, &unit_emb(8, 0)).await;
    link_face(&pool, alice_id, f1).await;

    // Bob (unnamed, similar to Alice): embedding at dim-0
    let bob_id = create_unnamed_person(&pool).await;
    let p2 = insert_photo_plain(&pool, "ms2").await;
    let f2 = insert_face_emb(&pool, p2, &unit_emb(8, 0)).await;
    link_face(&pool, bob_id, f2).await;

    // Carol (unnamed, dissimilar): embedding at dim-4
    let carol_id = create_unnamed_person(&pool).await;
    let p3 = insert_photo_plain(&pool, "ms3").await;
    let f3 = insert_face_emb(&pool, p3, &unit_emb(8, 4)).await;
    link_face(&pool, carol_id, f3).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{alice_id}/merge-suggestions"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let arr = json.as_array().unwrap();
    assert!(!arr.is_empty(), "should have suggestions");
    // Bob (distance 0) comes before Carol (distance 1)
    assert_eq!(arr[0]["person_id"].as_i64().unwrap(), bob_id);
    assert!(arr[0]["distance"].as_f64().unwrap() < arr[1]["distance"].as_f64().unwrap());
}

#[tokio::test]
async fn merge_suggestions_excludes_self() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let alice_id = create_named_person(&pool, "Alice").await;
    let p1 = insert_photo_plain(&pool, "se1").await;
    let f1 = insert_face_emb(&pool, p1, &unit_emb(8, 0)).await;
    link_face(&pool, alice_id, f1).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{alice_id}/merge-suggestions"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let arr = json.as_array().unwrap();
    // No other people, so empty
    assert!(arr.is_empty());
    // If there were results, none should have person_id == alice_id
    for item in arr {
        assert_ne!(item["person_id"].as_i64().unwrap(), alice_id);
    }
}

#[tokio::test]
async fn merge_suggestions_empty_when_no_embedding() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Person with no faces (no embeddings)
    let pid = create_named_person(&pool, "Ghost").await;

    // Another person with embedding
    let other_id = create_unnamed_person(&pool).await;
    let p2 = insert_photo_plain(&pool, "ne2").await;
    let f2 = insert_face_emb(&pool, p2, &unit_emb(8, 1)).await;
    link_face(&pool, other_id, f2).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/merge-suggestions"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn merge_suggestions_empty_when_only_one_person() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let alice_id = create_named_person(&pool, "Solo").await;
    let p1 = insert_photo_plain(&pool, "op1").await;
    let f1 = insert_face_emb(&pool, p1, &unit_emb(8, 0)).await;
    link_face(&pool, alice_id, f1).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{alice_id}/merge-suggestions"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn merge_suggestions_excludes_low_confidence_cluster() {
    // Bob has an embedding identical to Alice's but every face is below the
    // CENTROID_LOW_CONF (0.70) threshold → its centroid is unreliable and
    // it must NOT appear in Alice's suggestions.
    // Carol is farther away but has normal confidence → should still appear.
    let (app, pool, _tmp) = test_app_with_pool().await;

    let alice_id = create_named_person(&pool, "Alice").await;
    let p_alice = insert_photo_plain(&pool, "lc_alice").await;
    link_face(&pool, alice_id, insert_face_emb(&pool, p_alice, &unit_emb(8, 0)).await).await;

    // Bob: same direction as Alice, but low confidence
    let bob_id = create_named_person(&pool, "Bob").await;
    let p_bob = insert_photo_plain(&pool, "lc_bob").await;
    link_face(&pool, bob_id,
        insert_face_emb_with_conf(&pool, p_bob, &unit_emb(8, 0), 0.40).await).await;

    // Carol: different direction, normal confidence
    let carol_id = create_named_person(&pool, "Carol").await;
    let p_carol = insert_photo_plain(&pool, "lc_carol").await;
    link_face(&pool, carol_id, insert_face_emb(&pool, p_carol, &unit_emb(8, 1)).await).await;

    let resp = app
        .oneshot(Request::builder()
            .uri(&format!("/api/people/{alice_id}/merge-suggestions"))
            .body(Body::empty()).unwrap())
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let arr = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
    let arr = arr.as_array().unwrap();

    let ids: Vec<i64> = arr.iter().map(|s| s["person_id"].as_i64().unwrap()).collect();
    assert!(!ids.contains(&bob_id), "low-confidence cluster must be excluded from suggestions");
    assert!(ids.contains(&carol_id), "normal-confidence cluster must still be suggested");
}

// ── outlier faces tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn outlier_faces_returns_distant_face_first() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Person with 3 faces: 2 close (dim-1) and 1 outlier (dim-5)
    let pid = create_named_person(&pool, "Alice").await;
    let p1 = insert_photo_plain(&pool, "of1").await;
    let f1 = insert_face_emb(&pool, p1, &unit_emb(8, 1)).await;
    link_face(&pool, pid, f1).await;

    let p2 = insert_photo_plain(&pool, "of2").await;
    let f2 = insert_face_emb(&pool, p2, &unit_emb(8, 1)).await;
    link_face(&pool, pid, f2).await;

    let p3 = insert_photo_plain(&pool, "of3").await;
    // Outlier: orthogonal to the centroid at dim-1
    let f3 = insert_face_emb(&pool, p3, &unit_emb(8, 5)).await;
    link_face(&pool, pid, f3).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/outlier-faces"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let arr = json.as_array().unwrap();
    assert!(!arr.is_empty(), "should have at least one outlier");
    // Outlier face (f3) should be first (highest distance)
    assert_eq!(arr[0]["face_id"].as_i64().unwrap(), f3);
    // Distance should be significant (orthogonal = 1.0)
    assert!(arr[0]["distance"].as_f64().unwrap() > 0.2);
}

#[tokio::test]
async fn outlier_faces_empty_when_too_few_faces() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pid = create_named_person(&pool, "Loner").await;
    let p1 = insert_photo_plain(&pool, "tf1").await;
    let f1 = insert_face_emb(&pool, p1, &unit_emb(8, 0)).await;
    link_face(&pool, pid, f1).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/outlier-faces"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // Only 1 face with embedding — cannot detect outliers
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn outlier_faces_empty_when_all_close() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pid = create_named_person(&pool, "Tight").await;
    // All faces identical → distance ≈ 0
    for suffix in ["ac1", "ac2", "ac3"] {
        let p = insert_photo_plain(&pool, suffix).await;
        let f = insert_face_emb(&pool, p, &unit_emb(8, 0)).await;
        link_face(&pool, pid, f).await;
    }

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/outlier-faces"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // All identical → no face exceeds the 0.20 distance threshold
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn eject_face_removes_from_person_and_creates_new() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pid = create_named_person(&pool, "Alice").await;
    let p1 = insert_photo_plain(&pool, "ej1").await;
    let f1 = insert_face_emb(&pool, p1, &unit_emb(8, 0)).await;
    link_face(&pool, pid, f1).await;

    let body = serde_json::json!({ "face_id": f1 });
    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/eject-face"))
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let new_pid = json["new_person_id"].as_i64().unwrap();
    assert_ne!(new_pid, pid);

    // Face no longer belongs to original person
    let old_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM person_faces WHERE person_id = ? AND face_id = ?"
    )
    .bind(pid).bind(f1)
    .fetch_one(&pool).await.unwrap();
    assert_eq!(old_count, 0);

    // Face now belongs to new person
    let new_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM person_faces WHERE person_id = ? AND face_id = ?"
    )
    .bind(new_pid).bind(f1)
    .fetch_one(&pool).await.unwrap();
    assert_eq!(new_count, 1);
}

#[tokio::test]
async fn eject_face_404_if_face_not_in_person() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pid = create_named_person(&pool, "Alice").await;
    let body = serde_json::json!({ "face_id": 9999 });
    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/eject-face"))
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}


// ── centroid faces tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn centroid_faces_returns_all_photo_ids_when_few() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pid = create_named_person(&pool, "Alice").await;
    let p1 = insert_photo_plain(&pool, "cfa1").await;
    let f1 = insert_face_emb(&pool, p1, &unit_emb(8, 0)).await;
    link_face(&pool, pid, f1).await;
    let p2 = insert_photo_plain(&pool, "cfa2").await;
    let f2 = insert_face_emb(&pool, p2, &unit_emb(8, 0)).await;
    link_face(&pool, pid, f2).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/centroid-faces"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let photo_ids: Vec<i64> = json["photo_ids"]
        .as_array().unwrap()
        .iter().map(|v| v.as_i64().unwrap()).collect();
    // Only 2 faces (< 50 threshold) → both returned
    assert_eq!(photo_ids.len(), 2);
    assert!(photo_ids.contains(&p1));
    assert!(photo_ids.contains(&p2));
}

#[tokio::test]
async fn centroid_faces_empty_when_no_embeddings() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    let pid = create_named_person(&pool, "Ghost").await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/centroid-faces"))
                .body(Body::empty()).unwrap(),
        )
        .await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["photo_ids"].as_array().unwrap().is_empty());
}

// ── Step 34b: face thumb orientation transform ───────────────────────────────

#[tokio::test]
async fn face_thumb_applies_orientation_rotated_photo() {
    let (_app, pool, tmp) = test_app_with_pool().await;

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/with_exif.jpg");

    // Insert photo with rotation=90 so the effective image is portrait
    let photo_id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status, rotation, flip_h, flip_v, exif_orientation)
         VALUES (?, 'sha_rot90', 'jpeg', 'imported', 90, 0, 0, 1) RETURNING id",
    )
    .bind(fixture.to_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();

    // Face bbox in display (rotated) space: top-left corner
    let face_id: i64 = sqlx::query_scalar(
        "INSERT INTO faces (photo_id, x, y, width, height, confidence)
         VALUES (?, 5, 5, 80, 80, 0.9) RETURNING id",
    )
    .bind(photo_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut config = picmanager::config::Config::default();
    config.thumb_cache_dir = tmp.path().to_path_buf();
    let app = picmanager::web::router(pool.clone(), config);

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/faces/{face_id}/thumb"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "should return valid JPEG");
}

// ── Step 34a: EXIF orientation storage ──────────────────────────────────────

#[tokio::test]
async fn import_stores_exif_orientation() {
    let (_, pool, tmp) = test_app_with_pool().await;
    let lib = tmp.path().join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    picmanager::importer::import_dir(&pool, &fixtures, &lib, true).await.unwrap();

    let rows: Vec<(i32,)> = sqlx::query_as("SELECT exif_orientation FROM photos")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(!rows.is_empty(), "photos should be imported");
    for (orient,) in &rows {
        assert!((1..=8).contains(orient), "exif_orientation must be 1-8, got {orient}");
    }
}

// ── Step 33a: photo rotation API ────────────────────────────────────────────

#[tokio::test]
async fn rotate_single_photo_updates_rotation() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/rot.jpg', 'sha_rot', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "rotation_delta": 90 });
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::OK);

    let (rotation,): (i32,) = sqlx::query_as("SELECT rotation FROM photos WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rotation, 90);
}

#[tokio::test]
async fn rotate_wraps_at_360() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/rot2.jpg', 'sha_rot2', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    // Set rotation to 270 first
    sqlx::query("UPDATE photos SET rotation = 270 WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    let body = serde_json::json!({ "rotation_delta": 90 });
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::OK);

    let (rotation,): (i32,) = sqlx::query_as("SELECT rotation FROM photos WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rotation, 0);
}

#[tokio::test]
async fn flip_h_toggles_twice_returns_original() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/flip.jpg', 'sha_flip', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "flip_h_toggle": true });
    // First toggle
    app.clone()
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
    // Second toggle
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::OK);

    let (flip_h,): (i32,) = sqlx::query_as("SELECT flip_h FROM photos WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(flip_h, 0);
}

#[tokio::test]
async fn rotate_does_not_touch_taken_at() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status, taken_at)
         VALUES ('/tmp/rot3.jpg', 'sha_rot3', 'jpeg', 'imported', '2024-01-01T12:00:00') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "rotation_delta": 180, "flip_v_toggle": true });
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::OK);

    let (taken_at, rotation, flip_v): (Option<String>, i32, i32) =
        sqlx::query_as("SELECT taken_at, rotation, flip_v FROM photos WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(taken_at.as_deref(), Some("2024-01-01T12:00:00"));
    assert_eq!(rotation, 180);
    assert_eq!(flip_v, 1);
}

#[tokio::test]
async fn batch_rotate_updates_all_photos() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    let id1: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/br1.jpg', 'sha_br1', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let id2: i64 = sqlx::query_scalar(
        "INSERT INTO photos (path, sha256, format, import_status)
         VALUES ('/tmp/br2.jpg', 'sha_br2', 'jpeg', 'imported') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "photo_ids": [id1, id2], "rotation_delta": 270 });
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::OK);

    for id in [id1, id2] {
        let (rotation,): (i32,) = sqlx::query_as("SELECT rotation FROM photos WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rotation, 270);
    }
}

// ── collections photo membership tests ───────────────────────────────────────

async fn make_collection(pool: &SqlitePool, name: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO albums (name, kind) VALUES (?, 'curated') RETURNING id",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn add_photos_to_collection_ok() {
    let (app, pool, tmp) = test_app_with_pool().await;
    let cid = make_collection(&pool, "test").await;
    let pid1 = insert_photo_plain(&pool, "ap1").await;
    let pid2 = insert_photo_plain(&pool, "ap2").await;

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app2 = picmanager::web::router(pool.clone(), config);
    let resp = app2
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/collections/{cid}/photos"))
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"photo_ids":[{pid1},{pid2}]}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(obj["added"].as_u64().unwrap(), 2);

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
        .bind(cid).fetch_one(&pool).await.unwrap();
    assert_eq!(count.0, 2);
    let _ = app;
}

#[tokio::test]
async fn add_photos_deduplicates() {
    let (app, pool, tmp) = test_app_with_pool().await;
    let cid = make_collection(&pool, "dedup").await;
    let pid = insert_photo_plain(&pool, "dedup1").await;
    sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
        .bind(pid).bind(cid).execute(&pool).await.unwrap();

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app2 = picmanager::web::router(pool.clone(), config);
    let resp = app2
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/collections/{cid}/photos"))
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"photo_ids":[{pid}]}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(obj["added"].as_u64().unwrap(), 0, "already in collection, nothing added");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
        .bind(cid).fetch_one(&pool).await.unwrap();
    assert_eq!(count.0, 1, "still only one row");
    let _ = app;
}

#[tokio::test]
async fn remove_photos_from_collection_ok() {
    let (app, pool, tmp) = test_app_with_pool().await;
    let cid = make_collection(&pool, "remove").await;
    let pid1 = insert_photo_plain(&pool, "rm1").await;
    let pid2 = insert_photo_plain(&pool, "rm2").await;
    for pid in [pid1, pid2] {
        sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
            .bind(pid).bind(cid).execute(&pool).await.unwrap();
    }

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app2 = picmanager::web::router(pool.clone(), config);
    let resp = app2
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/collections/{cid}/photos"))
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"photo_ids":[{pid1}]}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(obj["removed"].as_u64().unwrap(), 1);

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
        .bind(cid).fetch_one(&pool).await.unwrap();
    assert_eq!(count.0, 1, "one photo still in collection");
    let _ = app;
}

#[tokio::test]
async fn remove_nonexistent_photos_returns_zero() {
    let (app, pool, tmp) = test_app_with_pool().await;
    let cid = make_collection(&pool, "empty").await;
    let pid = insert_photo_plain(&pool, "ne1").await;

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app2 = picmanager::web::router(pool.clone(), config);
    let resp = app2
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/collections/{cid}/photos"))
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"photo_ids":[{pid}]}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(obj["removed"].as_u64().unwrap(), 0);
    let _ = app;
}

#[tokio::test]
async fn list_collection_photos_paginated() {
    let (app, pool, tmp) = test_app_with_pool().await;
    let cid = make_collection(&pool, "paged").await;
    for i in 0..5u64 {
        let pid = insert_photo_plain(&pool, &format!("pg{i}")).await;
        sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
            .bind(pid).bind(cid).execute(&pool).await.unwrap();
    }

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app2 = picmanager::web::router(pool.clone(), config);
    let resp = app2
        .oneshot(
            Request::builder()
                .uri(format!("/api/collections/{cid}/photos?page=1&per_page=3"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(obj["total"].as_i64().unwrap(), 5);
    assert_eq!(obj["photos"].as_array().unwrap().len(), 3);
    let _ = app;
}

// ── collections CRUD tests ────────────────────────────────────────────────────

#[tokio::test]
async fn list_collections_returns_empty_initially() {
    let app = test_app().await;
    let resp = app
        .oneshot(Request::builder().uri("/api/collections").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let arr: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(arr.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn create_collection_returns_id_and_name() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/collections")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"夏日回忆"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(obj["id"].as_i64().unwrap() > 0);
    assert_eq!(obj["name"].as_str().unwrap(), "夏日回忆");
}

#[tokio::test]
async fn rename_collection_changes_name() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    let cid: i64 = sqlx::query_scalar(
        "INSERT INTO albums (name, kind) VALUES ('旧名字', 'curated') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = _tmp.path().to_path_buf(); c };
    let app2 = picmanager::web::router(pool.clone(), config);
    let resp = app2
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/collections/{cid}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"新名字"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let name: (String,) = sqlx::query_as("SELECT name FROM albums WHERE id = ?")
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name.0, "新名字");
    let _ = app;
}

#[tokio::test]
async fn delete_collection_removes_album_and_memberships() {
    let (app, pool, tmp) = test_app_with_pool().await;
    let cid: i64 = sqlx::query_scalar(
        "INSERT INTO albums (name, kind) VALUES ('要删除', 'curated') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let pid = insert_photo_plain(&pool, "dc1").await;
    sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
        .bind(pid)
        .bind(cid)
        .execute(&pool)
        .await
        .unwrap();

    let config = { let mut c = picmanager::config::Config::default(); c.thumb_cache_dir = tmp.path().to_path_buf(); c };
    let app2 = picmanager::web::router(pool.clone(), config);
    let resp = app2
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/collections/{cid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE id = ?")
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0, "album record removed");

    let count2: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count2.0, 0, "photo_albums cascade deleted");
    let _ = app;
}

// ── Activity photo association: timezone & format correctness ──────────────

// Helper: insert a minimal photo row and return its id
async fn insert_photo_with_gps(
    pool: &SqlitePool,
    taken_at: &str,
    timezone_offset: Option<i64>,
    lat: f64,
    lon: f64,
) -> i64 {
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO photos (path, sha256, format, taken_at, timezone_offset, gps_lat, gps_lon, import_status) \
         VALUES (?, ?, 'jpeg', ?, ?, ?, ?, 'imported') RETURNING id",
    )
    .bind(format!("/lib/{taken_at}.jpg"))
    .bind(format!("sha{taken_at}"))
    .bind(taken_at)
    .bind(timezone_offset)
    .bind(lat)
    .bind(lon)
    .fetch_one(pool)
    .await
    .unwrap();
    id.0
}

// Helper: insert a minimal activity with start/end time (RFC3339 UTC) and one track point
async fn insert_activity_with_track(
    pool: &SqlitePool,
    start_utc: &str,
    end_utc: &str,
    track_lat: f64,
    track_lon: f64,
) -> i64 {
    let act_id: (i64,) = sqlx::query_as(
        "INSERT INTO activities (sha256, source_path, file_format, activity_type, start_time, end_time, import_status) \
         VALUES (?,'/tmp/a.gpx','gpx','running',?,?,'imported') RETURNING id",
    )
    .bind(format!("sha_act_{start_utc}"))
    .bind(start_utc)
    .bind(end_utc)
    .fetch_one(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO activity_track_points (activity_id, ts, lat, lon) VALUES (?,?,?,?)",
    )
    .bind(act_id.0)
    .bind(start_utc)
    .bind(track_lat)
    .bind(track_lon)
    .execute(pool)
    .await
    .unwrap();

    act_id.0
}

/// 照片 taken_at 为本地时间（UTC+8），activity 时间为 UTC，时区正确转换后应关联
#[tokio::test]
async fn activity_photos_associates_with_local_time_photo() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Activity: UTC 10:00–11:00 on 2024-06-15
    let act_id = insert_activity_with_track(
        &pool,
        "2024-06-15T10:00:00+00:00",
        "2024-06-15T11:00:00+00:00",
        39.9, 116.4,
    ).await;

    // Photo taken at local 18:00 (UTC+8 → UTC 10:00), i.e. inside the activity window
    insert_photo_with_gps(&pool, "2024-06-15 18:00:00", Some(480), 39.9, 116.4).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/activities/{act_id}/photos"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["photos"].as_array().unwrap().len(), 1,
        "photo at local 18:00 (UTC+8) = UTC 10:00 should be associated with 10:00-11:00 UTC activity"
    );
}

/// 照片在活动时间窗口之外（即使在同一天），不应关联
#[tokio::test]
async fn activity_photos_excludes_photo_outside_utc_window() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Activity: UTC 10:00–11:00
    let act_id = insert_activity_with_track(
        &pool,
        "2024-06-15T10:00:00+00:00",
        "2024-06-15T11:00:00+00:00",
        39.9, 116.4,
    ).await;

    // Photo at local 12:00 (UTC+8 → UTC 04:00), outside the activity window
    insert_photo_with_gps(&pool, "2024-06-15 12:00:00", Some(480), 39.9, 116.4).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/activities/{act_id}/photos"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["photos"].as_array().unwrap().is_empty(),
        "photo at local 12:00 (UTC+8) = UTC 04:00 should NOT be in a 10:00-11:00 UTC activity"
    );
}

/// 照片 GPS 距轨迹超过 500m，即使时间在窗口内也不应关联
#[tokio::test]
async fn activity_photos_excludes_gps_too_far() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    // Activity track near Beijing (39.9, 116.4)
    let act_id = insert_activity_with_track(
        &pool,
        "2024-06-15T10:00:00+00:00",
        "2024-06-15T11:00:00+00:00",
        39.9, 116.4,
    ).await;

    // Photo at correct time but GPS far away (Shanghai)
    insert_photo_with_gps(&pool, "2024-06-15 18:00:00", Some(480), 31.23, 121.47).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/activities/{act_id}/photos"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["photos"].as_array().unwrap().is_empty(),
        "photo 1000km away from track should not be associated"
    );
}

// ── timezone_offset import tests ─────────────────────────────────────────────

#[tokio::test]
async fn import_stores_timezone_offset_from_exif() {
    let (_, pool, tmp) = test_app_with_pool().await;
    let lib = tmp.path().join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    // IMG_9886.HEIC has OffsetTimeOriginal = +08:00 (UTC+8 = 480 minutes)
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/samples/IMG_9886.HEIC");
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::copy(&sample, staging.join("IMG_9886.HEIC")).unwrap();

    picmanager::importer::import_dir(&pool, &staging, &lib, true).await.unwrap();

    let (tz,): (Option<i64>,) = sqlx::query_as("SELECT timezone_offset FROM photos LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tz, Some(480), "iPhone HEIC with +08:00 should store timezone_offset=480");
}

#[tokio::test]
async fn backfill_timezones_updates_null_rows() {
    let (_, pool, tmp) = test_app_with_pool().await;
    let lib = tmp.path().join("lib");
    std::fs::create_dir_all(&lib).unwrap();

    // Insert a photo row with NULL timezone_offset pointing at the HEIC sample
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/samples/IMG_9886.HEIC");
    sqlx::query(
        "INSERT INTO photos (path, sha256, format, import_status, timezone_offset)
         VALUES (?, 'sha_tz_test', 'heic', 'imported', NULL)",
    )
    .bind(sample.to_string_lossy().as_ref())
    .execute(&pool)
    .await
    .unwrap();

    // Verify it starts as NULL
    let (before,): (Option<i64>,) =
        sqlx::query_as("SELECT timezone_offset FROM photos WHERE sha256='sha_tz_test'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(before.is_none(), "should start as NULL");

    // Run backfill
    picmanager::metadata::backfill_timezones(&pool, false).await.unwrap();

    let (after,): (Option<i64>,) =
        sqlx::query_as("SELECT timezone_offset FROM photos WHERE sha256='sha_tz_test'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(after, Some(480), "after backfill should be 480 (UTC+8)");
}

// ── Activity merge tests ────────────────────────────────────────────────────

async fn insert_test_activity(
    pool: &SqlitePool,
    activity_type: &str,
    start_time: &str,
    end_time: &str,
    duration_seconds: i64,
    distance_meters: f64,
    sha_suffix: &str,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO activities \
         (sha256, source_path, file_format, activity_type, start_time, end_time, \
          duration_seconds, distance_meters, import_status) \
         VALUES (?,?,?,?,?,?,?,?,'imported') RETURNING id",
    )
    .bind(format!("sha_{sha_suffix}"))
    .bind("test")
    .bind("gpx")
    .bind(activity_type)
    .bind(start_time)
    .bind(end_time)
    .bind(duration_seconds)
    .bind(distance_meters)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn merge_two_activities_creates_merged_record() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let id1 = insert_test_activity(
        &pool, "running",
        "2025-04-10T06:00:00+00:00", "2025-04-10T06:30:00+00:00",
        1800, 5000.0, "merge1a",
    ).await;
    let id2 = insert_test_activity(
        &pool, "running",
        "2025-04-10T07:00:00+00:00", "2025-04-10T07:20:00+00:00",
        1200, 3000.0, "merge1b",
    ).await;

    let body = serde_json::json!({ "ids": [id1, id2] });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/activities/merge")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let merged: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(merged["activity_type"], "running");
    assert_eq!(merged["duration_seconds"], 3000); // 1800 + 1200
    assert!((merged["distance_meters"].as_f64().unwrap() - 8000.0).abs() < 1.0);

    // Source activities should be soft-deleted
    let (status1,): (String,) =
        sqlx::query_as("SELECT import_status FROM activities WHERE id = ?")
            .bind(id1)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status1, "merged");

    let (status2,): (String,) =
        sqlx::query_as("SELECT import_status FROM activities WHERE id = ?")
            .bind(id2)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status2, "merged");
}

#[tokio::test]
async fn merge_rejects_type_mismatch() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let id1 = insert_test_activity(
        &pool, "running",
        "2025-04-10T06:00:00+00:00", "2025-04-10T06:30:00+00:00",
        1800, 5000.0, "mismatch_a",
    ).await;
    let id2 = insert_test_activity(
        &pool, "hiking",
        "2025-04-10T07:00:00+00:00", "2025-04-10T07:20:00+00:00",
        1200, 3000.0, "mismatch_b",
    ).await;

    let body = serde_json::json!({ "ids": [id1, id2] });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/activities/merge")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["error"], "type_mismatch");
}

#[tokio::test]
async fn merge_rejects_time_overlap() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let id1 = insert_test_activity(
        &pool, "running",
        "2025-04-10T06:00:00+00:00", "2025-04-10T06:40:00+00:00",
        2400, 5000.0, "overlap_a",
    ).await;
    // Starts before id1 ends → overlap
    let id2 = insert_test_activity(
        &pool, "running",
        "2025-04-10T06:30:00+00:00", "2025-04-10T07:00:00+00:00",
        1800, 3000.0, "overlap_b",
    ).await;

    let body = serde_json::json!({ "ids": [id1, id2] });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/activities/merge")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["error"], "time_overlap");
}

#[tokio::test]
async fn merge_migrates_track_points() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let id1 = insert_test_activity(
        &pool, "running",
        "2025-04-10T06:00:00+00:00", "2025-04-10T06:30:00+00:00",
        1800, 5000.0, "pts_a",
    ).await;
    let id2 = insert_test_activity(
        &pool, "running",
        "2025-04-10T07:00:00+00:00", "2025-04-10T07:20:00+00:00",
        1200, 3000.0, "pts_b",
    ).await;

    // Insert track points for both
    sqlx::query(
        "INSERT INTO activity_track_points (activity_id, ts, lat, lon) VALUES (?,?,?,?),(?,?,?,?)"
    )
    .bind(id1).bind("2025-04-10T06:00:00+00:00").bind(39.9).bind(116.4)
    .bind(id2).bind("2025-04-10T07:00:00+00:00").bind(39.91).bind(116.41)
    .execute(&pool)
    .await
    .unwrap();

    let body = serde_json::json!({ "ids": [id1, id2] });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/activities/merge")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let merged: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let new_id = merged["id"].as_i64().unwrap();

    let (pt_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM activity_track_points WHERE activity_id = ?")
            .bind(new_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pt_count, 2, "both track points should be migrated to merged activity");
}

// ── embedding-map tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn embedding_map_returns_404_for_unknown_person() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/people/9999/embedding-map")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn embedding_map_returns_empty_for_person_with_no_faces() {
    let (app, pool, _tmp) = test_app_with_pool().await;
    let pid = create_named_person(&pool, "Ghost").await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/embedding-map"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["points"].as_array().unwrap().is_empty());
    assert_eq!(json["total"].as_i64().unwrap(), 0);
}

#[tokio::test]
async fn embedding_map_returns_points_with_required_fields() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pid = create_named_person(&pool, "Alice").await;
    let p1 = insert_photo_plain(&pool, "em1").await;
    let p2 = insert_photo_plain(&pool, "em2").await;
    // Two clearly different unit vectors so PCA can produce a meaningful result
    let f1 = insert_face_emb(&pool, p1, &unit_emb(16, 0)).await;
    let f2 = insert_face_emb(&pool, p2, &unit_emb(16, 8)).await;
    link_face(&pool, pid, f1).await;
    link_face(&pool, pid, f2).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/embedding-map"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let points = json["points"].as_array().unwrap();
    assert_eq!(points.len(), 2);
    assert_eq!(json["total"].as_i64().unwrap(), 2);

    for pt in points {
        assert!(pt["face_id"].is_i64(), "missing face_id");
        assert!(pt["photo_id"].is_i64(), "missing photo_id");
        assert!(pt["person_id"].is_i64(), "missing person_id");
        assert!(pt["x"].is_f64(), "missing x");
        assert!(pt["y"].is_f64(), "missing y");
        assert!(pt["confidence"].is_f64(), "missing confidence");
        let x = pt["x"].as_f64().unwrap();
        let y = pt["y"].as_f64().unwrap();
        assert!(x >= -1.001 && x <= 1.001, "x out of range: {x}");
        assert!(y >= -1.001 && y <= 1.001, "y out of range: {y}");
    }
}

#[tokio::test]
async fn embedding_map_includes_child_person_faces() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let parent_id = create_named_person(&pool, "Parent").await;
    let child_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (name, parent_id) VALUES ('Child', ?) RETURNING id",
    )
    .bind(parent_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    // One face for parent, one for child
    let p1 = insert_photo_plain(&pool, "ec1").await;
    let p2 = insert_photo_plain(&pool, "ec2").await;
    let f1 = insert_face_emb(&pool, p1, &unit_emb(16, 0)).await;
    let f2 = insert_face_emb(&pool, p2, &unit_emb(16, 8)).await;
    link_face(&pool, parent_id, f1).await;
    link_face(&pool, child_id, f2).await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{parent_id}/embedding-map"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let points = json["points"].as_array().unwrap();

    // Both faces returned
    assert_eq!(points.len(), 2, "should include parent and child faces");

    let person_ids: Vec<i64> = points.iter().map(|p| p["person_id"].as_i64().unwrap()).collect();
    assert!(person_ids.contains(&parent_id), "parent face must appear");
    assert!(person_ids.contains(&child_id), "child face must appear");
}

#[tokio::test]
async fn embedding_map_coordinates_in_range_with_many_faces() {
    let (app, pool, _tmp) = test_app_with_pool().await;

    let pid = create_named_person(&pool, "Many").await;
    for i in 0..20i64 {
        let p = insert_photo_plain(&pool, &format!("mr{i}")).await;
        // Vary the hot dimension across the 16-dim space
        let f = insert_face_emb(&pool, p, &unit_emb(16, (i as usize) % 16)).await;
        link_face(&pool, pid, f).await;
    }

    let resp = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/people/{pid}/embedding-map"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let points = json["points"].as_array().unwrap();
    assert_eq!(points.len(), 20);

    for pt in points {
        let x = pt["x"].as_f64().unwrap();
        let y = pt["y"].as_f64().unwrap();
        assert!(x >= -1.001 && x <= 1.001, "x={x} out of range");
        assert!(y >= -1.001 && y <= 1.001, "y={y} out of range");
    }
}
