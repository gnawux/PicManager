pub mod detector;

use image::DynamicImage;
use sqlx::SqlitePool;

pub async fn detect_and_save(pool: &SqlitePool, photo_id: i64, img: &DynamicImage) {
    let detections = detector::detect(img);
    for det in detections {
        if let Err(e) = sqlx::query(
            "INSERT INTO animals (photo_id, species, confidence, x, y, width, height)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(photo_id)
        .bind(&det.species)
        .bind(det.confidence)
        .bind(det.x)
        .bind(det.y)
        .bind(det.width)
        .bind(det.height)
        .execute(pool)
        .await
        {
            tracing::warn!("failed to save animal detection for photo {photo_id}: {e}");
        }
    }
}
