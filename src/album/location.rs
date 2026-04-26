use reqwest::Client;
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Duration;

use crate::error::Result;

const GEO_COORD_PRECISION: usize = 4; // ≈11 m precision at equator

fn coord_key(v: f64) -> String {
    format!("{:.prec$}", v, prec = GEO_COORD_PRECISION)
}

/// Returns the count of imported photos that have GPS coordinates but no matching
/// entry in the geocache table (i.e., not yet reverse-geocoded).
pub async fn count_missing_geo(pool: &SqlitePool) -> Result<i64> {
    let n = sqlx::query_scalar(
        "SELECT COUNT(*) FROM photos ph
         WHERE ph.import_status = 'imported'
           AND ph.gps_lat IS NOT NULL
           AND ph.gps_lon IS NOT NULL
           AND NOT EXISTS (
             SELECT 1 FROM geocache gc
             WHERE PRINTF('%.4f', ph.gps_lat) = gc.lat_key
               AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key
           )",
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
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

struct GeoInfo {
    city: Option<String>,
    state: Option<String>,
    county: Option<String>,
    country: Option<String>,
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

    // Read city, state, and country to distinguish permanent failures from transient ones.
    let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT city, state, country FROM geocache WHERE lat_key = ? AND lon_key = ?",
    )
    .bind(&lat_key)
    .bind(&lon_key)
    .fetch_optional(pool)
    .await
    .ok()?;

    if let Some((city, state, country)) = row {
        // All three NULL means Nominatim returned an error or no data during a previous
        // attempt (transient failure) — treat as a cache miss and retry.
        let truly_empty = city.is_none() && state.is_none() && country.is_none();
        if !truly_empty {
            // Complete entry (state set), or a partial result (only country known) — use as-is.
            if state.is_some() || city.is_none() {
                return city;
            }
            // city is set but state is NULL → stale entry written before municipality fix.
            // Fall through to re-geocode and update.
        }
        // truly_empty → fall through to re-geocode
    }

    // Cache miss or stale entry — respect Nominatim's 1 req/s policy
    if *need_rate_limit {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let info = nominatim_lookup(client, lat, lon).await;
    *need_rate_limit = true;

    let city = info.as_ref().and_then(|i| i.city.clone());
    let state = info.as_ref().and_then(|i| i.state.clone());
    let county = info.as_ref().and_then(|i| i.county.clone());
    let country = info.as_ref().and_then(|i| i.country.clone());

    // INSERT OR REPLACE so that stale entries (state was NULL) are updated.
    let _ = sqlx::query(
        "INSERT OR REPLACE INTO geocache (lat_key, lon_key, city, state, county, country)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&lat_key)
    .bind(&lon_key)
    .bind(&city)
    .bind(&state)
    .bind(&county)
    .bind(&country)
    .execute(pool)
    .await;

    city
}

async fn nominatim_lookup(client: &Client, lat: f64, lon: f64) -> Option<GeoInfo> {
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?lat={lat}&lon={lon}&format=json&zoom=10"
    );
    let resp: serde_json::Value = client.get(&url).send().await.ok()?.json().await.ok()?;
    let addr = resp.get("address")?;

    let city = ["city", "town", "village"]
        .iter()
        .find_map(|f| addr.get(*f).and_then(|v| v.as_str()).map(str::to_owned));
    let county = addr.get("county").and_then(|v| v.as_str()).map(str::to_owned);
    let mut state = addr.get("state").and_then(|v| v.as_str()).map(str::to_owned);
    let country = addr.get("country").and_then(|v| v.as_str()).map(str::to_owned);

    // Chinese direct-controlled municipalities (直辖市) have no `state` field in
    // Nominatim — the city IS the province-level entity.  Derive from ISO 3166-2.
    if state.is_none() {
        if let Some(iso) = addr.get("ISO3166-2-lvl4").and_then(|v| v.as_str()) {
            state = cn_municipality_state(iso).map(str::to_owned);
        }
    }

    // When `city`/`town`/`village` is absent (common for municipalities at zoom=10),
    // fall back to `county` (the district-level name, e.g. 西城区).
    let city = city.or_else(|| county.clone());

    if city.is_none() && state.is_none() && country.is_none() {
        return None;
    }
    Some(GeoInfo { city, state, county, country })
}

/// Maps ISO 3166-2 level-4 codes for China's four direct-controlled municipalities
/// to their province-level display name.  Regular provinces already have a `state`
/// field in the Nominatim response, so this only needs to handle the four 直辖市.
fn cn_municipality_state(iso: &str) -> Option<&'static str> {
    match iso {
        "CN-BJ" => Some("北京市"),
        "CN-SH" => Some("上海市"),
        "CN-TJ" => Some("天津市"),
        "CN-CQ" => Some("重庆市"),
        _ => None,
    }
}

/// Like `group_by_location` but restricted to the given photo IDs.
/// Sets `geo_total` before starting and increments `geo_done` after each photo.
/// GPS photos that already have a geocache hit don't count as API calls but
/// still contribute to `geo_done`.
pub async fn group_by_location_scoped(
    pool: &SqlitePool,
    photo_ids: &[i64],
    geo_total: &AtomicUsize,
    geo_done: &AtomicUsize,
) -> Result<()> {
    if photo_ids.is_empty() {
        return Ok(());
    }

    let placeholders = photo_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, gps_lat, gps_lon FROM photos \
         WHERE id IN ({placeholders}) AND gps_lat IS NOT NULL AND gps_lon IS NOT NULL"
    );
    let mut q = sqlx::query_as::<_, (i64, f64, f64)>(&sql);
    for id in photo_ids {
        q = q.bind(id);
    }
    let photos = q.fetch_all(pool).await?;

    geo_total.store(photos.len(), Relaxed);

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
        if let Some(city) = city {
            ensure_location_album(pool, photo_id, &city).await?;
        }
        geo_done.fetch_add(1, Relaxed);
    }
    Ok(())
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

    async fn seed_geocache(pool: &SqlitePool, lat: f64, lon: f64, city: Option<&str>, state: Option<&str>) {
        sqlx::query(
            "INSERT INTO geocache (lat_key, lon_key, city, state) VALUES (?, ?, ?, ?)",
        )
        .bind(coord_key(lat))
        .bind(coord_key(lon))
        .bind(city)
        .bind(state)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn count_missing_geo_ignores_photos_without_gps() {
        let pool = test_pool().await;
        insert_photo(&pool, "/no-gps.jpg", None, None).await;
        assert_eq!(count_missing_geo(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn count_missing_geo_counts_uncached_gps_photos() {
        let pool = test_pool().await;
        let coords: &[(f64, f64, &str)] = &[
            (37.7749, -122.4194, "/sf.jpg"),
            (35.6762, 139.6503, "/tokyo.jpg"),
            (51.5074, -0.1278, "/london.jpg"),
        ];
        for (lat, lon, path) in coords {
            insert_photo(&pool, path, Some(*lat), Some(*lon)).await;
        }
        // Cache only the first one
        seed_geocache(&pool, 37.7749, -122.4194, Some("San Francisco"), Some("California")).await;

        assert_eq!(count_missing_geo(&pool).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn count_missing_geo_returns_zero_when_all_cached() {
        let pool = test_pool().await;
        let lat = 48.8566;
        let lon = 2.3522;
        insert_photo(&pool, "/paris.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon, Some("Paris"), Some("Île-de-France")).await;

        assert_eq!(count_missing_geo(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn count_missing_geo_ignores_deleted_photos() {
        let pool = test_pool().await;
        let lat = 48.8566;
        let lon = 2.3522;
        // Insert a deleted photo with GPS but no geocache
        sqlx::query(
            "INSERT INTO photos (path, sha256, format, import_status, gps_lat, gps_lon) \
             VALUES ('/del.jpg', 'del', 'jpeg', 'deleted', ?, ?)",
        )
        .bind(lat)
        .bind(lon)
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(count_missing_geo(&pool).await.unwrap(), 0);
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
        seed_geocache(&pool, lat, lon, Some("Tokyo"), Some("Tokyo-to")).await;

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
        seed_geocache(&pool, lat, lon, Some("Tokyo"), Some("Tokyo-to")).await;

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
        seed_geocache(&pool, 35.6762, 139.6503, Some("Tokyo"), Some("Tokyo-to")).await;
        seed_geocache(&pool, 48.8566, 2.3522, Some("Paris"), Some("Île-de-France")).await;

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
        seed_geocache(&pool, lat, lon, Some("London"), Some("England")).await;

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
    async fn scoped_geo_only_touches_given_ids() {
        let pool = test_pool().await;
        let id1 = insert_photo(&pool, "/a.jpg", Some(35.6762), Some(139.6503)).await;
        let id2 = insert_photo(&pool, "/b.jpg", Some(48.8566), Some(2.3522)).await;
        seed_geocache(&pool, 35.6762, 139.6503, Some("Tokyo"), Some("Tokyo-to")).await;
        seed_geocache(&pool, 48.8566, 2.3522, Some("Paris"), Some("Île-de-France")).await;

        let total = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        // Only pass id1 — id2 should not get an album.
        group_by_location_scoped(&pool, &[id1], &total, &done).await.unwrap();

        assert_eq!(total.load(Relaxed), 1);
        assert_eq!(done.load(Relaxed), 1);

        let albums: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM albums WHERE kind = 'location' ORDER BY name",
        ).fetch_all(&pool).await.unwrap();
        assert_eq!(albums.len(), 1, "only Tokyo should be created");
        assert_eq!(albums[0].0, "Tokyo");

        // id2 (Paris) must have no album link.
        let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photo_albums WHERE photo_id = ?")
            .bind(id2).fetch_one(&pool).await.unwrap();
        assert_eq!(links, 0);
    }

    #[tokio::test]
    async fn scoped_geo_skips_photos_without_gps() {
        let pool = test_pool().await;
        let id = insert_photo(&pool, "/no-gps.jpg", None, None).await;
        let total = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        group_by_location_scoped(&pool, &[id], &total, &done).await.unwrap();
        assert_eq!(total.load(Relaxed), 0);
        assert_eq!(done.load(Relaxed), 0);
    }

    #[tokio::test]
    async fn all_null_geocache_is_retried_produces_no_album() {
        // All-NULL geocache rows (city=NULL, state=NULL, country=NULL) indicate a transient
        // Nominatim failure and should be retried. In tests, Nominatim has no network, so
        // the retry also returns None and no album is created — but the code path runs.
        let pool = test_pool().await;
        let lat = 0.0;
        let lon = 0.0;
        insert_photo(&pool, "/unknown.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon, None, None).await; // all-NULL → transient failure

        group_by_location(&pool).await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0, "all-null geocache → Nominatim retry → fails in test → no album");
    }

    #[tokio::test]
    async fn partial_geocache_with_country_only_skips_photo() {
        // city=NULL, state=NULL, country=set → Nominatim returned data but no city/state.
        // This is a legitimate (non-transient) result; do not retry.
        let pool = test_pool().await;
        let lat = 22.1969;
        let lon = 113.5408;
        insert_photo(&pool, "/macau.jpg", Some(lat), Some(lon)).await;
        // Seed with country set but no city/state — simulates "country-only" geocache result.
        sqlx::query(
            "INSERT INTO geocache (lat_key, lon_key, city, state, country) VALUES (?, ?, NULL, NULL, ?)",
        )
        .bind(coord_key(lat))
        .bind(coord_key(lon))
        .bind("Macau")
        .execute(&pool)
        .await
        .unwrap();

        group_by_location(&pool).await.unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0, "country-only geocache → treated as permanent → no album");
    }

    #[test]
    fn cn_municipality_state_maps_known_codes() {
        assert_eq!(cn_municipality_state("CN-BJ"), Some("北京市"));
        assert_eq!(cn_municipality_state("CN-SH"), Some("上海市"));
        assert_eq!(cn_municipality_state("CN-TJ"), Some("天津市"));
        assert_eq!(cn_municipality_state("CN-CQ"), Some("重庆市"));
        assert_eq!(cn_municipality_state("CN-GD"), None); // regular province — Nominatim returns state directly
        assert_eq!(cn_municipality_state("US-CA"), None);
    }

    #[tokio::test]
    async fn stale_geocache_entry_is_re_fetched_when_state_null() {
        // Seed an entry with city set but state NULL (pre-fix data for municipalities).
        // group_by_location should detect the stale entry and call Nominatim.
        // In tests, Nominatim call fails → returns None → INSERT OR REPLACE writes all-NULL.
        // The photo therefore gets no album (city becomes NULL after re-fetch fails).
        // This confirms the stale-detection path runs without panicking.
        let pool = test_pool().await;
        let lat = 39.9042;
        let lon = 116.4074;
        insert_photo(&pool, "/beijing.jpg", Some(lat), Some(lon)).await;
        // Seed stale entry: city set, state NULL (no network in tests so re-fetch returns None)
        seed_geocache(&pool, lat, lon, Some("东城区"), None).await;

        // Should not panic; city will become None after failed re-fetch
        group_by_location(&pool).await.unwrap();
    }
}
