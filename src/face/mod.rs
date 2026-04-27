pub mod cluster;
pub mod detector;
pub mod embedder;
pub mod job;

pub use detector::{detect, FaceRegion};
pub use embedder::Embedder;

use image::DynamicImage;
use sqlx::SqlitePool;

/// Apply a rotation (clockwise degrees: 0/90/180/270) and optional flips to an image.
pub(crate) fn apply_transform(
    img: DynamicImage,
    rotation: i32,
    flip_h: bool,
    flip_v: bool,
) -> DynamicImage {
    let img = match (rotation % 360 + 360) % 360 {
        90  => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _   => img,
    };
    let img = if flip_h { img.fliph() } else { img };
    if flip_v { img.flipv() } else { img }
}

/// Apply EXIF Orientation (1–8) to an image, correcting its display orientation.
/// Orientation 1 (normal) is a no-op.
pub(crate) fn apply_exif_orientation(img: DynamicImage, orientation: u8) -> DynamicImage {
    let (rot, flip_h): (i32, bool) = match orientation {
        2 => (0,   true),
        3 => (180, false),
        4 => (180, true),
        5 => (90,  true),
        6 => (90,  false),
        7 => (270, true),
        8 => (270, false),
        _ => (0,   false), // 1 = normal
    };
    apply_transform(img, rot, flip_h, false)
}

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
        let row: Option<(i32, i32, i32, i32)> = sqlx::query_as(
            "SELECT exif_orientation, rotation, flip_h, flip_v FROM photos WHERE id = ?",
        )
        .bind(photo_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        if let Some((exif_orient, db_rot, db_flip_h, db_flip_v)) = row {
            let oriented = apply_exif_orientation(img.clone(), exif_orient as u8);
            apply_transform(oriented, db_rot, db_flip_h != 0, db_flip_v != 0)
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

    #[test]
    fn apply_exif_orientation_1_is_noop() {
        let img = DynamicImage::new_rgb8(100, 60);
        let out = apply_exif_orientation(img, 1);
        assert_eq!(out.width(), 100);
        assert_eq!(out.height(), 60);
    }

    #[test]
    fn apply_exif_orientation_6_rotates_90cw() {
        // EXIF 6 = 90° CW: landscape (W×H) → portrait (H×W)
        let img = DynamicImage::new_rgb8(100, 60);
        let out = apply_exif_orientation(img, 6);
        assert_eq!(out.width(), 60);
        assert_eq!(out.height(), 100);
    }

    #[test]
    fn apply_exif_orientation_8_rotates_270cw() {
        // EXIF 8 = 270° CW: landscape (W×H) → portrait (H×W)
        let img = DynamicImage::new_rgb8(100, 60);
        let out = apply_exif_orientation(img, 8);
        assert_eq!(out.width(), 60);
        assert_eq!(out.height(), 100);
    }

    #[test]
    fn apply_exif_orientation_3_rotates_180() {
        // EXIF 3 = 180°: dimensions unchanged
        let img = DynamicImage::new_rgb8(100, 60);
        let out = apply_exif_orientation(img, 3);
        assert_eq!(out.width(), 100);
        assert_eq!(out.height(), 60);
    }

    #[test]
    fn apply_exif_orientation_unknown_value_is_noop() {
        let img = DynamicImage::new_rgb8(80, 40);
        let out = apply_exif_orientation(img, 9);
        assert_eq!(out.width(), 80);
        assert_eq!(out.height(), 40);
    }

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
