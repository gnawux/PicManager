use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use picmanager::{
    apple,
    orientation::{DisplayTransform, OrientationMode, apply_exif_orientation},
};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqlitePoolOptions;
use std::path::{Path, PathBuf};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/media_fidelity")
        .join(path)
}

fn marker_image() -> DynamicImage {
    let mut image = RgbImage::new(3, 2);
    for (index, pixel) in image.pixels_mut().enumerate() {
        *pixel = Rgb([(index + 1) as u8, 0, 0]);
    }
    DynamicImage::ImageRgb8(image)
}

fn signature(image: &DynamicImage) -> Vec<Vec<u8>> {
    (0..image.height())
        .map(|y| {
            (0..image.width())
                .map(|x| image.get_pixel(x, y).0[0])
                .collect()
        })
        .collect()
}

#[test]
fn orientation_visual_matrix_matches_reference_signatures() {
    let reference = [
        vec![vec![1, 2, 3], vec![4, 5, 6]],
        vec![vec![3, 2, 1], vec![6, 5, 4]],
        vec![vec![6, 5, 4], vec![3, 2, 1]],
        vec![vec![4, 5, 6], vec![1, 2, 3]],
        vec![vec![1, 4], vec![2, 5], vec![3, 6]],
        vec![vec![4, 1], vec![5, 2], vec![6, 3]],
        vec![vec![6, 3], vec![5, 2], vec![4, 1]],
        vec![vec![3, 6], vec![2, 5], vec![1, 4]],
    ];
    for orientation in 1..=8 {
        assert_eq!(
            signature(&apply_exif_orientation(marker_image(), orientation)),
            reference[(orientation - 1) as usize],
            "EXIF orientation {orientation} diverged from the reference display"
        );
    }
}

#[test]
fn reprocessing_is_idempotent_and_never_mutates_source_bytes() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/with_exif.jpg");
    let before = std::fs::read(&source).unwrap();
    let transform = DisplayTransform::new(
        OrientationMode::Metadata,
        Some(6),
        1,
        &source,
        90,
        true,
        false,
    );
    let first = transform.apply(image::open(&source).unwrap()).to_rgba8();
    let second = transform.apply(image::open(&source).unwrap()).to_rgba8();
    assert_eq!(first, second);
    let after = std::fs::read(&source).unwrap();
    assert_eq!(Sha256::digest(&before), Sha256::digest(&after));
}

#[cfg(target_os = "macos")]
#[test]
fn real_heic_legacy_fallback_is_repeatable_and_read_only() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples/IMG_9886.HEIC");
    let before = std::fs::read(&source).unwrap();
    let transform = DisplayTransform::new(
        OrientationMode::LegacyUnknown,
        None,
        1,
        &source,
        0,
        false,
        false,
    );
    let first = transform
        .apply(picmanager::image_open::open_image(&source).unwrap())
        .to_rgb8();
    let second = transform
        .apply(picmanager::image_open::open_image(&source).unwrap())
        .to_rgb8();
    assert_eq!(first, second);
    assert_eq!(before, std::fs::read(&source).unwrap());
}

#[tokio::test]
async fn apple_inventory_policy_matrix_remains_visible_and_distinct() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let report = apple::ingest_inventory(&pool, &fixture("apple_inventory.ndjson"), false)
        .await
        .unwrap();
    assert_eq!(report.total_assets, 8);
    assert_eq!(report.queued_assets, 7);
    assert_eq!(report.excluded_assets, 1);

    let sources: Vec<(String, Option<String>, String, Option<String>, String)> = sqlx::query_as(
        "SELECT external_id, original_filename, sync_status, exclusion_reason, metadata_json \
         FROM asset_sources WHERE provider = 'apple_photos' ORDER BY external_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(sources.len(), 8);
    assert_eq!(
        sources
            .iter()
            .filter(|(_, name, _, _, _)| name.as_deref() == Some("IMG_0001.JPG"))
            .count(),
        2,
        "duplicate visible filenames must not collapse source identities"
    );
    let raw_only = sources
        .iter()
        .find(|(id, _, _, _, _)| id == "raw-only/L0/001")
        .unwrap();
    assert_eq!(raw_only.2, "excluded");
    assert_eq!(raw_only.3.as_deref(), Some("raw_only"));

    let live: serde_json::Value = serde_json::from_str(
        &sources
            .iter()
            .find(|(id, _, _, _, _)| id == "live-photo/L0/001")
            .unwrap()
            .4,
    )
    .unwrap();
    assert_eq!(live["live_photo"], true);
    assert_eq!(live["resources"].as_array().unwrap().len(), 2);

    let raw_pair: serde_json::Value = serde_json::from_str(
        &sources
            .iter()
            .find(|(id, _, _, _, _)| id == "raw-jpeg/L0/001")
            .unwrap()
            .4,
    )
    .unwrap();
    assert_eq!(raw_pair["resources"].as_array().unwrap().len(), 2);
    let queued_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(queued_items, 7, "RAW-only media stays visible without export work");
}
