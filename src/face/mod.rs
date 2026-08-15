pub mod cluster;
pub mod detector;
pub mod embedder;
pub mod job;
pub mod pca;

pub use detector::{detect, FaceRegion};
pub use embedder::Embedder;

use image::DynamicImage;
use sqlx::SqlitePool;
use crate::orientation::{DisplayTransform, OrientationMode};

/// Detect faces in `img`, persist them to the `faces` table, and (if the
/// embedding model is available) fill in 512-D embeddings.  All failures
/// are warned — never propagated.
/// Returns the number of faces detected and persisted.
///
/// The image is pre-processed with the photo's effective orientation
/// (EXIF Orientation + DB rotation/flip) so that face coordinates are
/// stored in display space.
pub async fn analyze_one(pool: &SqlitePool, photo_id: i64, img: &DynamicImage) -> usize {
    // Fetch orientation data and build an effectively-oriented image.
    let oriented = {
        let row: Option<(i32, i32, i32, i32, String, Option<i64>, String)> = sqlx::query_as(
            "SELECT p.exif_orientation, p.rotation, p.flip_h, p.flip_v, \
                    COALESCE(vr.orientation_mode, 'legacy_unknown'), \
                    vr.display_orientation, COALESCE(dv.path, p.path) \
             FROM photos p \
             LEFT JOIN assets a ON a.photo_id = p.id \
             LEFT JOIN asset_variants dv ON dv.id = a.display_variant_id \
             LEFT JOIN variant_renditions vr ON vr.variant_id = dv.id \
             WHERE p.id = ?",
        )
        .bind(photo_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        if let Some((exif_orient, db_rot, db_flip_h, db_flip_v, mode, display_orient, path)) = row {
            DisplayTransform::new(
                OrientationMode::from_catalog(Some(&mode)),
                display_orient.map(|value| value as u8),
                exif_orient as u8,
                std::path::Path::new(&path),
                db_rot,
                db_flip_h != 0,
                db_flip_v != 0,
            )
            .apply(img.clone())
        } else {
            img.clone()
        }
    };

    let (faces, embeddings): (Vec<FaceRegion>, Vec<Option<Vec<f32>>>) =
        tokio::task::spawn_blocking(move || {
            let faces = detector::detect(&oriented);
            if faces.is_empty() {
                return (vec![], vec![]);
            }
            let emb = embedder::Embedder::load(std::path::Path::new("")).ok();
            let embeddings = faces
                .iter()
                .map(|face| emb.as_ref().and_then(|e| e.extract(&oriented, face).ok()))
                .collect();
            (faces, embeddings)
        })
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("face analysis task panicked for photo {photo_id}: {e}");
            (vec![], vec![])
        });

    if faces.is_empty() {
        return 0;
    }

    let face_ids = save_faces(pool, photo_id, &faces).await;

    for (i, maybe_emb) in embeddings.into_iter().enumerate() {
        let Some(&face_id) = face_ids.get(i) else { continue };
        let Some(emb_vec) = maybe_emb else { continue };
        let blob = embedder::encode_embedding(&emb_vec);
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

    face_ids.len()
}

pub(crate) async fn save_faces(pool: &SqlitePool, photo_id: i64, faces: &[FaceRegion]) -> Vec<i64> {
    let mut face_ids = Vec::new();
    for face in faces {
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
    face_ids
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;
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
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) VALUES (1, 'x', 'abc', 'jpeg', 'imported')"
        )
        .execute(&pool)
        .await
        .unwrap();

        let img = DynamicImage::new_rgb8(640, 480);
        let n = analyze_one(&pool, 1, &img).await;
        assert_eq!(n, 0, "blank image → analyze_one should return 0 faces");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM faces WHERE photo_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "blank image → no faces detected → no rows inserted");
    }

    #[tokio::test]
    async fn analyze_one_real_photo_inserts_faces() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) VALUES (1, 'x', 'abc', 'jpeg', 'imported')"
        )
        .execute(&pool)
        .await
        .unwrap();

        let img = image::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/samples/IMG_9844.JPG"),
        )
        .unwrap();
        let img_owned = img.clone();
        let faces = tokio::task::spawn_blocking(move || {
            let model_path = dirs::config_dir()
                .unwrap()
                .join("picmanager/models/face_detector.onnx");
            let mut session = ort::session::Session::builder()
                .unwrap()
                .with_execution_providers([ort::ep::coreml::CoreML::default().build()])
                .unwrap()
                .commit_from_file(&model_path)
                .unwrap();
            detector::run_inference(&mut session, &img_owned).unwrap()
        })
        .await
        .unwrap();

        save_faces(&pool, 1, &faces).await;

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM faces WHERE photo_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(count >= 1, "expected at least one face row, got {count}");
    }
}
