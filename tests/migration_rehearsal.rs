use std::{borrow::Cow, path::Path};

use picmanager::migration::{backfill_legacy_local, verify_catalog};
use sqlx::{
    FromRow, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

#[derive(Debug, Clone, PartialEq, FromRow)]
struct PhotoSnapshot {
    id: i64,
    path: String,
    sha256: String,
    phash: Option<String>,
    taken_at: Option<String>,
    gps_lat: Option<f64>,
    gps_lon: Option<f64>,
    camera: Option<String>,
    format: String,
    import_status: String,
    timezone_offset: Option<i64>,
    rotation: i64,
    flip_h: i64,
    flip_v: i64,
    exif_orientation: i64,
    width: Option<i64>,
    height: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
struct LegacySnapshot {
    photos: Vec<PhotoSnapshot>,
    album_links: Vec<(i64, i64)>,
    face_links: Vec<(i64, i64)>,
    person_links: Vec<(i64, i64)>,
    activity_count: i64,
}

#[tokio::test]
async fn copied_legacy_catalog_upgrades_without_changing_existing_records() {
    let fixture = tempfile::tempdir().unwrap();
    let original_path = fixture.path().join("legacy-original.db");
    let copied_path = fixture.path().join("legacy-upgrade-copy.db");
    let media_dir = fixture.path().join("media");
    std::fs::create_dir(&media_dir).unwrap();

    let original = open_database(&original_path).await;
    legacy_migrator().run(&original).await.unwrap();
    seed_realistic_legacy_catalog(&original, &media_dir).await;
    let before = snapshot(&original).await;
    original.close().await;
    std::fs::copy(&original_path, &copied_path).unwrap();

    let upgraded = open_database(&copied_path).await;
    sqlx::migrate!("./migrations").run(&upgraded).await.unwrap();
    let after_schema_upgrade = snapshot(&upgraded).await;
    assert_eq!(
        after_schema_upgrade, before,
        "additive schema migrations changed legacy data"
    );

    let dry_run = backfill_legacy_local(&upgraded, true).await.unwrap();
    assert!(dry_run.dry_run);
    assert_eq!(dry_run.total_photos, before.photos.len() as i64);
    assert_eq!(
        snapshot(&upgraded).await,
        before,
        "dry-run changed legacy data"
    );
    assert_catalog_counts(&upgraded, 0).await;

    let first = backfill_legacy_local(&upgraded, false).await.unwrap();
    assert_eq!(first.created_assets, before.photos.len() as u64);
    assert_eq!(first.created_sources, before.photos.len() as u64);
    assert_eq!(first.created_variants, before.photos.len() as u64);
    assert_eq!(first.created_links, before.photos.len() as u64);
    assert_eq!(
        snapshot(&upgraded).await,
        before,
        "backfill changed legacy data"
    );
    assert_catalog_counts(&upgraded, before.photos.len() as i64).await;

    let verification = verify_catalog(&upgraded, true).await.unwrap();
    assert!(
        verification.is_healthy(),
        "upgraded catalog did not verify cleanly"
    );

    let second = backfill_legacy_local(&upgraded, false).await.unwrap();
    assert_eq!(second.created_assets, 0);
    assert_eq!(second.created_sources, 0);
    assert_eq!(second.created_variants, 0);
    assert_eq!(second.created_links, 0);
    assert_eq!(
        snapshot(&upgraded).await,
        before,
        "idempotent rerun changed legacy data"
    );

    let completed_runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM migration_runs \
         WHERE kind = 'backfill_legacy_local' AND status = 'completed' \
           AND summary_json IS NOT NULL",
    )
    .fetch_one(&upgraded)
    .await
    .unwrap();
    assert_eq!(completed_runs, 2);
    upgraded.close().await;

    let untouched = open_database(&original_path).await;
    let original_version: i64 =
        sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&untouched)
            .await
            .unwrap();
    assert_eq!(
        original_version, 18,
        "source fixture was modified instead of its copy"
    );
    assert_eq!(snapshot(&untouched).await, before);
}

fn legacy_migrator() -> Migrator {
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 18)
                .cloned()
                .collect(),
        ),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
}

async fn open_database(path: &Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap()
}

async fn seed_realistic_legacy_catalog(pool: &SqlitePool, media_dir: &Path) {
    let photos = [
        (
            101,
            "IMG_2048.HEIC",
            "sha-heic",
            "heic",
            "2023-05-06 08:09:10",
            31.2304,
            121.4737,
            "iPhone 14 Pro",
            4032,
            3024,
            6,
            90,
        ),
        (
            205,
            "DSC_0042.JPG",
            "sha-jpeg",
            "jpeg",
            "2021-11-12 13:14:15",
            35.6762,
            139.6503,
            "Sony ILCE-6400",
            6000,
            4000,
            1,
            0,
        ),
        (
            990,
            "scan-family.png",
            "sha-png",
            "png",
            "1998-02-03 00:00:00",
            0.0,
            0.0,
            "Flatbed scanner",
            2400,
            1600,
            1,
            0,
        ),
    ];
    for (
        id,
        filename,
        sha,
        format,
        taken_at,
        lat,
        lon,
        camera,
        width,
        height,
        orientation,
        rotation,
    ) in photos
    {
        let path = media_dir.join(filename);
        std::fs::write(&path, format!("fixture-{id}")).unwrap();
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, phash, taken_at, gps_lat, gps_lon, camera, \
                 format, import_status, timezone_offset, rotation, flip_h, flip_v, \
                 exif_orientation, width, height) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'imported', 480, ?, 0, 0, ?, ?, ?)",
        )
        .bind(id)
        .bind(path.to_string_lossy().as_ref())
        .bind(sha)
        .bind(format!("phash-{id}"))
        .bind(taken_at)
        .bind(lat)
        .bind(lon)
        .bind(camera)
        .bind(format)
        .bind(rotation)
        .bind(orientation)
        .bind(width)
        .bind(height)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE photo_stats SET active_count = 3 WHERE id = 1")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO albums (id, name, kind) VALUES (12, 'Family', 'manual')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO photo_albums (photo_id, album_id) VALUES (101, 12), (990, 12)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO faces (id, photo_id, x, y, width, height, confidence) \
         VALUES (71, 101, 100, 200, 300, 400, 0.98)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO people (id, name, cover_face_id) VALUES (81, 'Family member', 71)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (81, 71)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO activities (id, sha256, source_path, file_format, activity_type, import_status) \
         VALUES (33, 'activity-sha', '/imports/walk.gpx', 'gpx', 'walking', 'imported')",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn snapshot(pool: &SqlitePool) -> LegacySnapshot {
    LegacySnapshot {
        photos: sqlx::query_as(
            "SELECT id, path, sha256, phash, taken_at, gps_lat, gps_lon, camera, format, \
                    import_status, timezone_offset, rotation, flip_h, flip_v, \
                    exif_orientation, width, height FROM photos ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap(),
        album_links: sqlx::query_as(
            "SELECT photo_id, album_id FROM photo_albums ORDER BY photo_id, album_id",
        )
        .fetch_all(pool)
        .await
        .unwrap(),
        face_links: sqlx::query_as("SELECT id, photo_id FROM faces ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap(),
        person_links: sqlx::query_as(
            "SELECT person_id, face_id FROM person_faces ORDER BY person_id, face_id",
        )
        .fetch_all(pool)
        .await
        .unwrap(),
        activity_count: sqlx::query_scalar("SELECT COUNT(*) FROM activities")
            .fetch_one(pool)
            .await
            .unwrap(),
    }
}

async fn assert_catalog_counts(pool: &SqlitePool, expected: i64) {
    for table in ["assets", "asset_sources", "asset_variants", "asset_links"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(count, expected, "unexpected row count in {table}");
    }
    let preserved: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT a.photo_id, v.path, v.content_sha256 FROM assets a \
         JOIN asset_variants v ON v.asset_id = a.id ORDER BY a.photo_id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(preserved.len() as i64, expected);
}
