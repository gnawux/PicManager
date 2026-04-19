use reqwest::Client;
use sqlx::SqlitePool;
use std::time::Duration;

use crate::error::Result;

const GEO_COORD_PRECISION: usize = 4; // ≈11 m precision at equator

fn coord_key(v: f64) -> String {
    format!("{:.prec$}", v, prec = GEO_COORD_PRECISION)
}

/// Group all imported photos with GPS coordinates into per-city location albums.
/// Uses OSM Nominatim for reverse geocoding, with a local geocache to avoid
/// redundant requests and to respect the 1 req/s rate limit.
pub async fn group_by_location(pool: &SqlitePool) -> Result<()> {
    let photos: Vec<(i64, f64, f64)> = sqlx::query_as(
        "SELECT id, gps_lat, gps_lon FROM photos
         WHERE import_status = 'imported' AND gps_lat IS NOT NULL AND gps_lon IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    if photos.is_empty() {
        return Ok(());
    }

    let client = Client::builder()
        .user_agent("PicManager/0.1 (family photo manager)")
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| Client::new());

    let mut need_rate_limit = false;
    for (photo_id, lat, lon) in photos {
        let city = cached_or_fetch(pool, &client, lat, lon, &mut need_rate_limit).await;
        let Some(city) = city else { continue };
        ensure_location_album(pool, photo_id, &city).await?;
    }
    Ok(())
}

/// Returns a city name for the given coordinates.
/// Checks the geocache first; falls back to the Nominatim API on a cache miss.
/// `need_rate_limit` is set to true after an actual API call is made so the
/// caller can sleep before the next call.
async fn cached_or_fetch(
    pool: &SqlitePool,
    client: &Client,
    lat: f64,
    lon: f64,
    need_rate_limit: &mut bool,
) -> Option<String> {
    let lat_key = coord_key(lat);
    let lon_key = coord_key(lon);

    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT city FROM geocache WHERE lat_key = ? AND lon_key = ?",
    )
    .bind(&lat_key)
    .bind(&lon_key)
    .fetch_optional(pool)
    .await
    .ok()?;

    if let Some((city,)) = row {
        return city; // cache hit (city may be None = previously failed)
    }

    // Cache miss — respect Nominatim's 1 req/s policy
    if *need_rate_limit {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let city = nominatim_lookup(client, lat, lon).await;
    *need_rate_limit = true;

    let _ = sqlx::query(
        "INSERT OR IGNORE INTO geocache (lat_key, lon_key, city) VALUES (?, ?, ?)",
    )
    .bind(&lat_key)
    .bind(&lon_key)
    .bind(&city)
    .execute(pool)
    .await;

    city
}

async fn nominatim_lookup(client: &Client, lat: f64, lon: f64) -> Option<String> {
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?lat={lat}&lon={lon}&format=json&zoom=10"
    );
    let resp: serde_json::Value = client.get(&url).send().await.ok()?.json().await.ok()?;
    let addr = resp.get("address")?;
    for field in &["city", "town", "village", "county", "state"] {
        if let Some(name) = addr.get(*field).and_then(|v| v.as_str()) {
            return Some(name.to_owned());
        }
    }
    None
}

async fn ensure_location_album(pool: &SqlitePool, photo_id: i64, city: &str) -> Result<()> {
    let album_id: i64 = {
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM albums WHERE name = ? AND kind = 'location'",
        )
        .bind(city)
        .fetch_optional(pool)
        .await?;

        match existing {
            Some((id,)) => id,
            None => sqlx::query("INSERT INTO albums (name, kind) VALUES (?, 'location')")
                .bind(city)
                .execute(pool)
                .await?
                .last_insert_rowid(),
        }
    };

    sqlx::query("INSERT OR IGNORE INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
        .bind(photo_id)
        .bind(album_id)
        .execute(pool)
        .await?;

    Ok(())
}

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

    async fn insert_photo(pool: &SqlitePool, path: &str, lat: Option<f64>, lon: Option<f64>) -> i64 {
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, gps_lat, gps_lon, import_status)
             VALUES (?, ?, 'jpeg', ?, ?, 'imported')",
        )
        .bind(path)
        .bind(path)
        .bind(lat)
        .bind(lon)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
    }

    async fn seed_geocache(pool: &SqlitePool, lat: f64, lon: f64, city: Option<&str>) {
        sqlx::query(
            "INSERT INTO geocache (lat_key, lon_key, city) VALUES (?, ?, ?)",
        )
        .bind(coord_key(lat))
        .bind(coord_key(lon))
        .bind(city)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn skips_photos_without_gps() {
        let pool = test_pool().await;
        insert_photo(&pool, "/a.jpg", None, None).await;

        group_by_location(&pool).await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn creates_album_from_cached_city() {
        let pool = test_pool().await;
        let lat = 35.6762;
        let lon = 139.6503;
        insert_photo(&pool, "/tokyo.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon, Some("Tokyo")).await;

        group_by_location(&pool).await.unwrap();

        let albums: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM albums WHERE kind = 'location'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].0, "Tokyo");

        let links: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photo_albums")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(links.0, 1);
    }

    #[tokio::test]
    async fn two_photos_same_city_one_album() {
        let pool = test_pool().await;
        let lat = 35.6762;
        let lon = 139.6503;
        insert_photo(&pool, "/a.jpg", Some(lat), Some(lon)).await;
        insert_photo(&pool, "/b.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon, Some("Tokyo")).await;

        group_by_location(&pool).await.unwrap();

        let albums: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(albums.0, 1, "one city → one album");

        let links: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photo_albums")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(links.0, 2, "both photos linked to the album");
    }

    #[tokio::test]
    async fn two_photos_different_cities_two_albums() {
        let pool = test_pool().await;
        insert_photo(&pool, "/tokyo.jpg", Some(35.6762), Some(139.6503)).await;
        insert_photo(&pool, "/paris.jpg", Some(48.8566), Some(2.3522)).await;
        seed_geocache(&pool, 35.6762, 139.6503, Some("Tokyo")).await;
        seed_geocache(&pool, 48.8566, 2.3522, Some("Paris")).await;

        group_by_location(&pool).await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 2);
    }

    #[tokio::test]
    async fn is_idempotent() {
        let pool = test_pool().await;
        let lat = 51.5074;
        let lon = -0.1278;
        insert_photo(&pool, "/london.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon, Some("London")).await;

        group_by_location(&pool).await.unwrap();
        group_by_location(&pool).await.unwrap();

        let links: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photo_albums")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(links.0, 1, "idempotent: no duplicate album associations");

        let albums: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(albums.0, 1, "idempotent: no duplicate albums");
    }

    #[tokio::test]
    async fn cached_null_city_skips_photo() {
        let pool = test_pool().await;
        let lat = 0.0;
        let lon = 0.0;
        insert_photo(&pool, "/unknown.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon, None).await; // cached as "failed"

        group_by_location(&pool).await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0, "null city in cache → no album created");
    }
}
