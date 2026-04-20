pub mod cluster;
pub mod detector;
pub mod embedder;
pub mod job;

pub use detector::{detect, FaceRegion};
pub use embedder::Embedder;

use image::DynamicImage;
use sqlx::SqlitePool;

/// Detect faces in `img`, persist them to the `faces` table, and (if the
/// embedding model is available) fill in 512-D embeddings.  All failures
/// are warned — never propagated.
pub async fn analyze_one(pool: &SqlitePool, photo_id: i64, img: &DynamicImage) {
    let faces = detector::detect(img);
    if faces.is_empty() {
        return;
    }

    let mut face_ids: Vec<i64> = Vec::new();
    for face in &faces {
        match sqlx::query_scalar(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence) \
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(photo_id)
        .bind(face.x)
        .bind(face.y)
        .bind(face.width)
        .bind(face.height)
        .bind(face.confidence)
        .fetch_one(pool)
        .await
        {
            Ok(id) => face_ids.push(id),
            Err(e) => tracing::warn!("failed to persist face for photo {photo_id}: {e}"),
        }
    }

    let emb_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("picmanager/models/arcface_mobilenetv1.onnx");

    let emb = match embedder::Embedder::load(&emb_path) {
        Ok(e) => e,
        Err(_) => return,
    };

    for (i, face) in faces.iter().enumerate() {
        let Some(&face_id) = face_ids.get(i) else { continue };
        match emb.extract(img, face) {
            Ok(vec) => {
                let blob = embedder::encode_embedding(&vec);
                if let Err(e) = sqlx::query(
                    "UPDATE faces SET embedding = ?, embed_model = 'arcface-mobilenet-v1' \
                     WHERE id = ?",
                )
                .bind(&blob)
                .bind(face_id)
                .execute(pool)
                .await
                {
                    tracing::warn!("failed to store embedding for face {face_id}: {e}");
                }
            }
            Err(e) => tracing::warn!("embedding failed for face {face_id}: {e}"),
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn analyze_one_blank_image_inserts_nothing() {
        let pool = test_pool().await;
        // Insert a dummy photo row so FK is satisfied
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) VALUES (1, 'x', 'abc', 'jpeg', 'imported')"
        )
        .execute(&pool)
        .await
        .unwrap();

        let img = DynamicImage::new_rgb8(640, 480);
        analyze_one(&pool, 1, &img).await;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM faces WHERE photo_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "no model → detect returns [] → no rows inserted");
    }

    #[tokio::test]
    #[ignore = "requires face_detector.onnx in config_dir/picmanager/models/"]
    async fn analyze_one_real_photo_inserts_faces() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) VALUES (1, 'x', 'abc', 'jpeg', 'imported')"
        )
        .execute(&pool)
        .await
        .unwrap();

        let img = image::open("tests/samples/IMG_20250204_135549.jpg").unwrap();
        analyze_one(&pool, 1, &img).await;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM faces WHERE photo_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(count >= 1, "expected at least one face row");
    }
}
