use reqwest::Client;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::time::Duration;

use crate::error::Result;

const GEO_COORD_PRECISION: usize = 4; // ≈11 m precision at equator
const PROXIMITY_DEG: f64 = 0.01; // ≈1 km; used to reuse nearby geocache entries
const MAX_CONSECUTIVE_PROVIDER_FAILURES: usize = 3;
const DEFAULT_NOMINATIM_URL: &str = "https://nominatim.openstreetmap.org/reverse";
pub const GEO_NAME_POLICY_REVISION: i64 = 1;
pub const GEO_LANGUAGE_PREFERENCE: &str =
    "zh-CN,zh-Hans,zh-SG,zh-HK,zh-TW,zh-Hant,zh,en-US,en-GB,en";

fn coord_key(v: f64) -> String {
    format!("{:.prec$}", v, prec = GEO_COORD_PRECISION)
}

/// Returns the count of imported photos that have GPS coordinates but whose geocoding
/// is missing or retriable: no geocache entry at all, OR an all-NULL entry (written
/// when Nominatim previously failed transiently — e.g., no network at import time).
pub async fn count_missing_geo(pool: &SqlitePool) -> Result<i64> {
    let n = sqlx::query_scalar(
        "SELECT COUNT(*) FROM photos ph
         WHERE ph.import_status = 'imported'
           AND ph.gps_lat IS NOT NULL
           AND ph.gps_lon IS NOT NULL
           AND (
             NOT EXISTS (
               SELECT 1 FROM geocache gc
               WHERE PRINTF('%.4f', ph.gps_lat) = gc.lat_key
                 AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key
             )
             OR EXISTS (
               SELECT 1 FROM geocache gc
               WHERE PRINTF('%.4f', ph.gps_lat) = gc.lat_key
                 AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key
                 AND gc.city    IS NULL
                 AND gc.state   IS NULL
                 AND gc.country IS NULL
             )
           )",
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}

/// Count imported GPS photos whose cached names predate the current language policy.
pub async fn count_outdated_geo_names(pool: &SqlitePool) -> Result<i64> {
    let n = sqlx::query_scalar(
        "SELECT COUNT(*) FROM photos ph
         JOIN geocache gc
           ON PRINTF('%.4f', ph.gps_lat) = gc.lat_key
          AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key
         WHERE ph.import_status = 'imported'
           AND gc.name_policy_revision < ?",
    )
    .bind(GEO_NAME_POLICY_REVISION)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

/// Group all imported photos with GPS coordinates into per-city location albums.
/// Uses OSM Nominatim for reverse geocoding, with a local geocache to avoid
/// redundant requests and to respect the 1 req/s rate limit.
pub async fn group_by_location(pool: &SqlitePool) -> Result<()> {
    group_by_location_with_progress(pool, Arc::new(GeoProgress::default())).await
}

#[derive(Default)]
pub struct GeoProgress {
    pub total: AtomicUsize,
    pub processed: AtomicUsize,
    pub updated: AtomicUsize,
    pub cache_hits: AtomicUsize,
    pub provider_failures: AtomicUsize,
    pub cancellation_requested: AtomicBool,
}

pub type SharedGeoProgress = Arc<GeoProgress>;

pub async fn group_by_location_with_progress(
    pool: &SqlitePool,
    progress: SharedGeoProgress,
) -> Result<()> {
    group_by_location_with_policy(pool, progress, false).await
}

pub async fn normalize_geo_names_with_progress(
    pool: &SqlitePool,
    progress: SharedGeoProgress,
) -> Result<()> {
    group_by_location_with_policy(pool, progress, true).await
}

async fn group_by_location_with_policy(
    pool: &SqlitePool,
    progress: SharedGeoProgress,
    refresh_names: bool,
) -> Result<()> {
    group_by_location_with_policy_and_provider(
        pool,
        progress,
        refresh_names,
        GeoProvider::nominatim(),
    )
    .await
}

async fn group_by_location_with_policy_and_provider(
    pool: &SqlitePool,
    progress: SharedGeoProgress,
    refresh_names: bool,
    provider: GeoProvider,
) -> Result<()> {
    let sql = if refresh_names {
        "SELECT MIN(ph.gps_lat), MIN(ph.gps_lon) FROM photos ph
         JOIN geocache gc
           ON PRINTF('%.4f', ph.gps_lat) = gc.lat_key
          AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key
         WHERE ph.import_status = 'imported'
           AND ph.gps_lat IS NOT NULL AND ph.gps_lon IS NOT NULL
           AND gc.name_policy_revision < ?
         GROUP BY PRINTF('%.4f', ph.gps_lat), PRINTF('%.4f', ph.gps_lon)
         ORDER BY MIN(ph.gps_lat), MIN(ph.gps_lon)"
    } else {
        "SELECT MIN(gps_lat), MIN(gps_lon) FROM photos
         WHERE import_status = 'imported' AND gps_lat IS NOT NULL AND gps_lon IS NOT NULL
         GROUP BY PRINTF('%.4f', gps_lat), PRINTF('%.4f', gps_lon)
         ORDER BY MIN(gps_lat), MIN(gps_lon)"
    };
    let mut query = sqlx::query_as::<_, (f64, f64)>(sql);
    if refresh_names {
        query = query.bind(GEO_NAME_POLICY_REVISION);
    }
    let coordinates = query.fetch_all(pool).await?;
    progress.total.store(coordinates.len(), Relaxed);

    if coordinates.is_empty() {
        return Ok(());
    }

    let mut need_rate_limit = false;
    let mut session_cache: HashMap<(String, String), Option<String>> = HashMap::new();
    let mut consecutive_provider_failures = 0;
    for (lat, lon) in coordinates {
        if progress.cancellation_requested.load(Relaxed) {
            break;
        }
        let minimum_revision = if refresh_names {
            GEO_NAME_POLICY_REVISION
        } else {
            0
        };
        let outcome = cached_or_fetch(
            pool,
            &provider,
            lat,
            lon,
            minimum_revision,
            &mut need_rate_limit,
            &mut session_cache,
        )
        .await?;
        if outcome.cache_hit {
            progress.cache_hits.fetch_add(1, Relaxed);
        }
        if outcome.provider_failed {
            progress.provider_failures.fetch_add(1, Relaxed);
            consecutive_provider_failures += 1;
            progress.processed.fetch_add(1, Relaxed);
            if consecutive_provider_failures >= MAX_CONSECUTIVE_PROVIDER_FAILURES {
                return Err(crate::error::AppError::Metadata(format!(
                    "geocoding provider unavailable after {consecutive_provider_failures} consecutive requests"
                )));
            }
        } else {
            consecutive_provider_failures = 0;
            if outcome.provider_updated {
                progress.updated.fetch_add(1, Relaxed);
            }
        }
        if let Some(city) = outcome.city {
            ensure_location_album_for_coordinate(pool, lat, lon, &city).await?;
        }
        if !outcome.provider_failed {
            progress.processed.fetch_add(1, Relaxed);
        }
    }
    prune_empty_location_albums(pool).await?;
    Ok(())
}

#[derive(Clone)]
struct GeoInfo {
    city: Option<String>,
    state: Option<String>,
    county: Option<String>,
    country: Option<String>,
}

struct GeoLookupOutcome {
    city: Option<String>,
    cache_hit: bool,
    provider_updated: bool,
    provider_failed: bool,
}

#[derive(Clone)]
struct GeoProvider {
    client: Client,
    reverse_url: String,
    rate_limit: Duration,
}

impl GeoProvider {
    fn nominatim() -> Self {
        Self {
            client: Client::builder()
                .user_agent("PicManager/1.0 (family photo manager)")
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
            reverse_url: DEFAULT_NOMINATIM_URL.to_owned(),
            rate_limit: Duration::from_secs(1),
        }
    }

    #[cfg(test)]
    fn for_test(reverse_url: String, timeout: Duration) -> Self {
        Self {
            client: Client::builder().timeout(timeout).build().unwrap(),
            reverse_url,
            rate_limit: Duration::ZERO,
        }
    }
}

/// Returns a city name for the given coordinates.
/// Checks the in-memory session cache first, then the DB geocache, then Nominatim.
/// `need_rate_limit` is set to true after an actual API call is made so the
/// caller can sleep before the next call.
/// `session_cache` is an in-memory L1 cache scoped to one import/geocoding session;
/// it eliminates DB round-trips for repeated coordinates within the same run.
/// Returns `(city, is_cache_hit)`.
/// `is_cache_hit` is true when the result came from L1/L2/proximity (no Nominatim call).
async fn cached_or_fetch(
    pool: &SqlitePool,
    provider: &GeoProvider,
    lat: f64,
    lon: f64,
    minimum_revision: i64,
    need_rate_limit: &mut bool,
    session_cache: &mut HashMap<(String, String), Option<String>>,
) -> Result<GeoLookupOutcome> {
    let lat_key = coord_key(lat);
    let lon_key = coord_key(lon);

    // L1: in-memory cache — no DB round-trip for coordinates seen earlier this session.
    if let Some(cached) = session_cache.get(&(lat_key.clone(), lon_key.clone())) {
        return Ok(GeoLookupOutcome {
            city: cached.clone(),
            cache_hit: true,
            provider_updated: false,
            provider_failed: false,
        });
    }

    // Read city, state, and country to distinguish permanent failures from transient ones.
    let row: Option<(
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    )> = sqlx::query_as(
        "SELECT city, state, county, country, name_policy_revision
         FROM geocache WHERE lat_key = ? AND lon_key = ?",
    )
    .bind(&lat_key)
    .bind(&lon_key)
    .fetch_optional(pool)
    .await?;

    let mut stale_fallback = None;
    if let Some((city, state, county, country, revision)) = row {
        if revision < minimum_revision {
            if city.is_some() || state.is_some() || county.is_some() || country.is_some() {
                stale_fallback = Some(GeoInfo {
                    city: city.clone(),
                    state: state.clone(),
                    county,
                    country: country.clone(),
                });
            }
        } else {
            // All three NULL means Nominatim returned an error or no data during a previous
            // attempt (transient failure) — treat as a cache miss and retry.
            let truly_empty = city.is_none() && state.is_none() && country.is_none();
            if !truly_empty {
                // Complete entry (state set), or a partial result (only country known) — use as-is.
                if state.is_some() || city.is_none() {
                    session_cache.insert((lat_key, lon_key), city.clone());
                    return Ok(GeoLookupOutcome {
                        city,
                        cache_hit: true,
                        provider_updated: false,
                        provider_failed: false,
                    });
                }
                // city is set but state is NULL → stale entry written before municipality fix.
                // Fall through to re-geocode and update.
            }
            // truly_empty → fall through to re-geocode
        }
    }

    // Proximity lookup: reuse the nearest valid geocache entry within ±PROXIMITY_DEG.
    // Excludes the exact key itself (which may be stale) and all-NULL entries.
    // Avoids a Nominatim API call when a nearby coordinate has already been resolved.
    let nearby: Option<(
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
    )> = sqlx::query_as(
        "SELECT city, state, county, country, name_policy_revision FROM geocache
             WHERE CAST(lat_key AS REAL) BETWEEN ? AND ?
               AND CAST(lon_key AS REAL) BETWEEN ? AND ?
               AND NOT (lat_key = ? AND lon_key = ?)
               AND name_policy_revision >= ?
               AND (city IS NOT NULL OR state IS NOT NULL OR country IS NOT NULL)
             ORDER BY
               (CAST(lat_key AS REAL) - ?) * (CAST(lat_key AS REAL) - ?) +
               (CAST(lon_key AS REAL) - ?) * (CAST(lon_key AS REAL) - ?)
             LIMIT 1",
    )
    .bind(lat - PROXIMITY_DEG)
    .bind(lat + PROXIMITY_DEG)
    .bind(lon - PROXIMITY_DEG)
    .bind(lon + PROXIMITY_DEG)
    .bind(&lat_key)
    .bind(&lon_key)
    .bind(minimum_revision)
    .bind(lat)
    .bind(lat)
    .bind(lon)
    .bind(lon)
    .fetch_optional(pool)
    .await?;

    if let Some((city, state, county, country, source_revision)) = nearby {
        // Write back to exact key so future lookups skip this proximity scan.
        sqlx::query(
            "INSERT OR REPLACE INTO geocache
             (lat_key, lon_key, city, state, county, country, name_policy_revision)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&lat_key)
        .bind(&lon_key)
        .bind(&city)
        .bind(&state)
        .bind(&county)
        .bind(&country)
        .bind(source_revision)
        .execute(pool)
        .await?;
        session_cache.insert((lat_key, lon_key), city.clone());
        return Ok(GeoLookupOutcome {
            city,
            cache_hit: true,
            provider_updated: false,
            provider_failed: false,
        });
    }

    // No exact hit and no nearby hit — call Nominatim (respect 1 req/s policy).
    if *need_rate_limit {
        tokio::time::sleep(provider.rate_limit).await;
    }
    let provider_result = nominatim_lookup(provider, lat, lon).await;
    *need_rate_limit = true;
    let info = match provider_result {
        Ok(info) => info,
        Err(_) => {
            let city = stale_fallback.and_then(|fallback| fallback.city);
            return Ok(GeoLookupOutcome {
                city,
                cache_hit: false,
                provider_updated: false,
                provider_failed: true,
            });
        }
    };
    if info.is_none() {
        if let Some(fallback) = stale_fallback {
            return Ok(GeoLookupOutcome {
                city: fallback.city,
                cache_hit: false,
                provider_updated: false,
                provider_failed: true,
            });
        }
    }

    let city = info.as_ref().and_then(|i| i.city.clone());
    let state = info.as_ref().and_then(|i| i.state.clone());
    let county = info.as_ref().and_then(|i| i.county.clone());
    let country = info.as_ref().and_then(|i| i.country.clone());

    // INSERT OR REPLACE so that stale entries (state was NULL) are updated.
    sqlx::query(
        "INSERT OR REPLACE INTO geocache
         (lat_key, lon_key, city, state, county, country, name_policy_revision)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&lat_key)
    .bind(&lon_key)
    .bind(&city)
    .bind(&state)
    .bind(&county)
    .bind(&country)
    .bind(GEO_NAME_POLICY_REVISION)
    .execute(pool)
    .await?;

    session_cache.insert((lat_key, lon_key), city.clone());
    Ok(GeoLookupOutcome {
        city,
        cache_hit: false,
        provider_updated: true,
        provider_failed: false,
    })
}

/// Keep only the first variant and trim whitespace.
/// Nominatim (or OSM data) sometimes joins script variants with ";" or " / ",
/// e.g. "美国;美國" or "荷兰 / 荷蘭". Take the segment before the first delimiter.
fn first_name(s: String) -> String {
    let end = [s.find(';'), s.find(" / ")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(s.len());
    s[..end].trim().to_owned()
}

async fn nominatim_lookup(
    provider: &GeoProvider,
    lat: f64,
    lon: f64,
) -> std::result::Result<Option<GeoInfo>, reqwest::Error> {
    let url = format!(
        "{}?lat={lat}&lon={lon}&format=jsonv2&zoom=10&addressdetails=1&accept-language={GEO_LANGUAGE_PREFERENCE}",
        provider.reverse_url
    );
    let resp: serde_json::Value = provider
        .client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let Some(addr) = resp.get("address") else {
        return Ok(None);
    };

    let city = ["city", "town", "village"]
        .iter()
        .find_map(|f| addr.get(*f).and_then(|v| v.as_str()).map(str::to_owned))
        .map(first_name);
    let county = addr
        .get("county")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .map(first_name);
    let mut state = addr
        .get("state")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .map(first_name);
    let country = addr
        .get("country")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .map(first_name);

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
        return Ok(None);
    }
    Ok(Some(GeoInfo {
        city,
        state,
        county,
        country,
    }))
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
/// Sets `geo_total` before starting; increments `geo_done` after each photo,
/// `geo_cache_hits` when resolved from cache (no Nominatim call), and
/// `geo_failed` when a Nominatim call returned no city.
pub async fn group_by_location_scoped(
    pool: &SqlitePool,
    photo_ids: &[i64],
    geo_total: &AtomicUsize,
    geo_done: &AtomicUsize,
    geo_cache_hits: &AtomicUsize,
    geo_failed: &AtomicUsize,
) -> Result<()> {
    if photo_ids.is_empty() {
        return Ok(());
    }

    // SQLite limits bind variables to 999; chunk to stay well under that limit.
    const CHUNK: usize = 500;
    let mut photos: Vec<(i64, f64, f64)> = Vec::with_capacity(photo_ids.len());
    for chunk in photo_ids.chunks(CHUNK) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, gps_lat, gps_lon FROM photos \
             WHERE id IN ({placeholders}) AND gps_lat IS NOT NULL AND gps_lon IS NOT NULL"
        );
        let mut q = sqlx::query_as::<_, (i64, f64, f64)>(&sql);
        for id in chunk {
            q = q.bind(id);
        }
        photos.extend(q.fetch_all(pool).await?);
    }
    photos.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap()
            .then(a.2.partial_cmp(&b.2).unwrap())
    });

    geo_total.store(photos.len(), Relaxed);

    if photos.is_empty() {
        return Ok(());
    }

    let client = Client::builder()
        .user_agent("PicManager/0.1 (family photo manager)")
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| Client::new());
    let provider = GeoProvider {
        client,
        reverse_url: DEFAULT_NOMINATIM_URL.to_owned(),
        rate_limit: Duration::from_secs(1),
    };

    let mut need_rate_limit = false;
    let mut session_cache: HashMap<(String, String), Option<String>> = HashMap::new();
    for (photo_id, lat, lon) in photos {
        let outcome = cached_or_fetch(
            pool,
            &provider,
            lat,
            lon,
            0,
            &mut need_rate_limit,
            &mut session_cache,
        )
        .await?;
        if outcome.cache_hit {
            geo_cache_hits.fetch_add(1, Relaxed);
        } else if outcome.city.is_none() {
            geo_failed.fetch_add(1, Relaxed);
        }
        if let Some(city) = outcome.city {
            ensure_location_album(pool, photo_id, &city).await?;
        }
        geo_done.fetch_add(1, Relaxed);
    }
    prune_empty_location_albums(pool).await?;
    Ok(())
}

async fn ensure_location_album_for_coordinate(
    pool: &SqlitePool,
    lat: f64,
    lon: f64,
    city: &str,
) -> Result<()> {
    let lat_key = coord_key(lat);
    let lon_key = coord_key(lon);
    // Resolve the coordinate while no write transaction is open. PRINTF cannot use the
    // GPS indexes and can take a while on a large library; holding SQLite's single writer
    // lock during that scan starves job heartbeats and can make a healthy job lose its lease.
    let photo_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM photos
         WHERE import_status = 'imported'
           AND PRINTF('%.4f', gps_lat) = ?
           AND PRINTF('%.4f', gps_lon) = ?",
    )
    .bind(&lat_key)
    .bind(&lon_key)
    .fetch_all(pool)
    .await?;
    if photo_ids.is_empty() {
        return Ok(());
    }

    let album_id = ensure_location_album_record(pool, city).await?;

    // Keep each atomic relink batch small enough to fit SQLite's bind limit and, more
    // importantly, release the writer lock frequently enough for worker heartbeats.
    const RELINK_BATCH_SIZE: usize = 250;
    for photo_ids in photo_ids.chunks(RELINK_BATCH_SIZE) {
        let mut tx = pool.begin().await?;
        let mut delete =
            QueryBuilder::<Sqlite>::new("DELETE FROM photo_albums WHERE photo_id IN (");
        {
            let mut separated = delete.separated(", ");
            for photo_id in photo_ids {
                separated.push_bind(photo_id);
            }
        }
        delete
            .push(") AND album_id IN (SELECT id FROM albums WHERE kind = 'location' AND id != ")
            .push_bind(album_id)
            .push(")")
            .build()
            .execute(&mut *tx)
            .await?;

        let mut insert =
            QueryBuilder::<Sqlite>::new("INSERT OR IGNORE INTO photo_albums (photo_id, album_id) ");
        insert.push_values(photo_ids, |mut row, photo_id| {
            row.push_bind(photo_id).push_bind(album_id);
        });
        insert.build().execute(&mut *tx).await?;
        tx.commit().await?;
        tokio::task::yield_now().await;
    }
    Ok(())
}

async fn ensure_location_album_record(pool: &SqlitePool, city: &str) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let album_id = if let Some(id) =
        sqlx::query_scalar("SELECT id FROM albums WHERE name = ? AND kind = 'location'")
            .bind(city)
            .fetch_optional(&mut *tx)
            .await?
    {
        id
    } else {
        sqlx::query("INSERT INTO albums (name, kind) VALUES (?, 'location')")
            .bind(city)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid()
    };
    tx.commit().await?;
    Ok(album_id)
}

async fn ensure_location_album(pool: &SqlitePool, photo_id: i64, city: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    let album_id: i64 = {
        let existing: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM albums WHERE name = ? AND kind = 'location'")
                .bind(city)
                .fetch_optional(&mut *tx)
                .await?;

        match existing {
            Some((id,)) => id,
            None => sqlx::query("INSERT INTO albums (name, kind) VALUES (?, 'location')")
                .bind(city)
                .execute(&mut *tx)
                .await?
                .last_insert_rowid(),
        }
    };

    sqlx::query(
        "DELETE FROM photo_albums
         WHERE photo_id = ? AND album_id IN (
             SELECT id FROM albums WHERE kind = 'location' AND id != ?
         )",
    )
    .bind(photo_id)
    .bind(album_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query("INSERT OR IGNORE INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
        .bind(photo_id)
        .bind(album_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(())
}

async fn prune_empty_location_albums(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "DELETE FROM albums
         WHERE kind = 'location'
           AND NOT EXISTS (SELECT 1 FROM photo_albums pa WHERE pa.album_id = albums.id)",
    )
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

    async fn insert_photo(
        pool: &SqlitePool,
        path: &str,
        lat: Option<f64>,
        lon: Option<f64>,
    ) -> i64 {
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

    async fn seed_geocache(
        pool: &SqlitePool,
        lat: f64,
        lon: f64,
        city: Option<&str>,
        state: Option<&str>,
    ) {
        sqlx::query("INSERT INTO geocache (lat_key, lon_key, city, state) VALUES (?, ?, ?, ?)")
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
        seed_geocache(
            &pool,
            37.7749,
            -122.4194,
            Some("San Francisco"),
            Some("California"),
        )
        .await;

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
    async fn count_missing_geo_counts_all_null_geocache_entries() {
        // all-NULL geocache entry (transient Nominatim failure) must count as "missing"
        // so that fill-missing --geo doesn't skip it.
        let pool = test_pool().await;
        let lat = 48.8566;
        let lon = 2.3522;
        insert_photo(&pool, "/paris.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon, None, None).await; // all-NULL → retriable

        assert_eq!(count_missing_geo(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn count_missing_geo_does_not_count_partial_geocache() {
        // city=NULL but country set → legitimate partial result, not retriable → not counted
        let pool = test_pool().await;
        let lat = 22.1969;
        let lon = 113.5408;
        insert_photo(&pool, "/macau.jpg", Some(lat), Some(lon)).await;
        sqlx::query(
            "INSERT INTO geocache (lat_key, lon_key, city, state, country) VALUES (?, ?, NULL, NULL, ?)",
        )
        .bind(coord_key(lat))
        .bind(coord_key(lon))
        .bind("Macau")
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(count_missing_geo(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn counts_only_names_from_older_language_policies() {
        let pool = test_pool().await;
        let lat = 22.3193;
        let lon = 114.1694;
        insert_photo(&pool, "/hong-kong.jpg", Some(lat), Some(lon)).await;
        seed_geocache(
            &pool,
            lat,
            lon,
            Some("香港 Hong Kong"),
            Some("香港 Hong Kong"),
        )
        .await;
        assert_eq!(count_outdated_geo_names(&pool).await.unwrap(), 1);

        sqlx::query("UPDATE geocache SET name_policy_revision = ?")
            .bind(GEO_NAME_POLICY_REVISION)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(count_outdated_geo_names(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn name_normalization_reuses_current_nearby_names_and_reconciles_albums() {
        let pool = test_pool().await;
        let lat = 22.3193;
        let lon = 114.1694;
        let photo_id = insert_photo(&pool, "/hong-kong.jpg", Some(lat), Some(lon)).await;
        seed_geocache(
            &pool,
            lat,
            lon,
            Some("香港 Hong Kong"),
            Some("香港 Hong Kong"),
        )
        .await;
        seed_geocache(&pool, lat, lon + 0.005, Some("香港"), Some("香港")).await;
        sqlx::query(
            "UPDATE geocache SET country = '中国', name_policy_revision = ?
             WHERE lon_key = ?",
        )
        .bind(GEO_NAME_POLICY_REVISION)
        .bind(coord_key(lon + 0.005))
        .execute(&pool)
        .await
        .unwrap();
        ensure_location_album(&pool, photo_id, "香港 Hong Kong")
            .await
            .unwrap();

        normalize_geo_names_with_progress(&pool, SharedGeoProgress::default())
            .await
            .unwrap();

        let cached: (Option<String>, i64) = sqlx::query_as(
            "SELECT city, name_policy_revision FROM geocache
             WHERE lat_key = ? AND lon_key = ?",
        )
        .bind(coord_key(lat))
        .bind(coord_key(lon))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cached, (Some("香港".to_owned()), GEO_NAME_POLICY_REVISION));

        let albums: Vec<String> = sqlx::query_scalar(
            "SELECT a.name FROM albums a JOIN photo_albums pa ON pa.album_id = a.id
             WHERE pa.photo_id = ? AND a.kind = 'location'",
        )
        .bind(photo_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(albums, vec!["香港"]);
        let old_album_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM albums WHERE kind = 'location' AND name = '香港 Hong Kong'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(old_album_count, 0);
    }

    #[tokio::test]
    async fn name_normalization_processes_each_coordinate_once() {
        let pool = test_pool().await;
        let lat = 22.3193;
        let lon = 114.1694;
        let first = insert_photo(&pool, "/first.jpg", Some(lat), Some(lon)).await;
        let second = insert_photo(&pool, "/second.jpg", Some(lat), Some(lon)).await;
        seed_geocache(
            &pool,
            lat,
            lon,
            Some("香港 Hong Kong"),
            Some("香港 Hong Kong"),
        )
        .await;
        seed_geocache(&pool, lat, lon + 0.005, Some("香港"), Some("香港")).await;
        sqlx::query(
            "UPDATE geocache SET country = '中国', name_policy_revision = ? WHERE lon_key = ?",
        )
        .bind(GEO_NAME_POLICY_REVISION)
        .bind(coord_key(lon + 0.005))
        .execute(&pool)
        .await
        .unwrap();

        let progress = SharedGeoProgress::default();
        normalize_geo_names_with_progress(&pool, progress.clone())
            .await
            .unwrap();

        assert_eq!(progress.total.load(Relaxed), 1);
        assert_eq!(progress.processed.load(Relaxed), 1);
        assert_eq!(progress.cache_hits.load(Relaxed), 1);
        for photo_id in [first, second] {
            let album: String = sqlx::query_scalar(
                "SELECT a.name FROM albums a JOIN photo_albums pa ON pa.album_id = a.id
                 WHERE pa.photo_id = ? AND a.kind = 'location'",
            )
            .bind(photo_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(album, "香港");
        }
    }

    #[tokio::test]
    async fn name_normalization_stops_before_work_when_cancelled() {
        let pool = test_pool().await;
        let lat = 22.3193;
        let lon = 114.1694;
        insert_photo(&pool, "/cancelled.jpg", Some(lat), Some(lon)).await;
        seed_geocache(
            &pool,
            lat,
            lon,
            Some("香港 Hong Kong"),
            Some("香港 Hong Kong"),
        )
        .await;
        let progress = SharedGeoProgress::default();
        progress.cancellation_requested.store(true, Relaxed);

        normalize_geo_names_with_progress(&pool, progress.clone())
            .await
            .unwrap();

        assert_eq!(progress.total.load(Relaxed), 1);
        assert_eq!(progress.processed.load(Relaxed), 0);
        let revision: i64 = sqlx::query_scalar(
            "SELECT name_policy_revision FROM geocache WHERE lat_key = ? AND lon_key = ?",
        )
        .bind(coord_key(lat))
        .bind(coord_key(lon))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(revision, 0);
    }

    #[tokio::test]
    async fn name_normalization_circuit_breaks_provider_failures() {
        use axum::{Router, http::StatusCode, routing::get};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/reverse", get(|| async { StatusCode::BAD_GATEWAY })),
            )
            .await
            .unwrap();
        });
        let pool = test_pool().await;
        for index in 0..5 {
            let lat = 10.0 + f64::from(index);
            let lon = 100.0 + f64::from(index);
            insert_photo(&pool, &format!("/{index}.jpg"), Some(lat), Some(lon)).await;
            seed_geocache(&pool, lat, lon, Some("Old"), Some("Old")).await;
        }
        let progress = SharedGeoProgress::default();
        let provider =
            GeoProvider::for_test(format!("http://{address}/reverse"), Duration::from_secs(1));

        let error =
            group_by_location_with_policy_and_provider(&pool, progress.clone(), true, provider)
                .await
                .unwrap_err();
        server.abort();

        assert!(error.to_string().contains("3 consecutive requests"));
        assert_eq!(progress.total.load(Relaxed), 5);
        assert_eq!(progress.processed.load(Relaxed), 3);
        assert_eq!(progress.provider_failures.load(Relaxed), 3);
        let updated: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM geocache WHERE name_policy_revision = ?")
                .bind(GEO_NAME_POLICY_REVISION)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(updated, 0);
    }

    #[tokio::test]
    async fn name_normalization_collapses_large_duplicate_coordinate_sets() {
        let pool = test_pool().await;
        sqlx::query(
            "WITH RECURSIVE sequence(value) AS (
                 SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 5000
             )
             INSERT INTO photos
                 (path, sha256, format, gps_lat, gps_lon, import_status)
             SELECT '/bulk-' || value || '.jpg', 'bulk-' || value, 'jpeg',
                    CASE WHEN value % 2 = 0 THEN 22.3193 ELSE 31.2304 END,
                    CASE WHEN value % 2 = 0 THEN 114.1694 ELSE 121.4737 END,
                    'imported'
             FROM sequence",
        )
        .execute(&pool)
        .await
        .unwrap();
        seed_geocache(
            &pool,
            22.3193,
            114.1694,
            Some("香港 Hong Kong"),
            Some("香港 Hong Kong"),
        )
        .await;
        seed_geocache(
            &pool,
            31.2304,
            121.4737,
            Some("上海 Shanghai"),
            Some("上海市"),
        )
        .await;
        seed_geocache(&pool, 22.3193, 114.1744, Some("香港"), Some("香港")).await;
        seed_geocache(&pool, 31.2304, 121.4787, Some("上海"), Some("上海市")).await;
        sqlx::query(
            "UPDATE geocache SET country = '中国', name_policy_revision = ?
             WHERE lon_key IN (?, ?)",
        )
        .bind(GEO_NAME_POLICY_REVISION)
        .bind(coord_key(114.1744))
        .bind(coord_key(121.4787))
        .execute(&pool)
        .await
        .unwrap();
        let progress = SharedGeoProgress::default();

        normalize_geo_names_with_progress(&pool, progress.clone())
            .await
            .unwrap();

        assert_eq!(progress.total.load(Relaxed), 2);
        assert_eq!(progress.processed.load(Relaxed), 2);
        assert_eq!(progress.cache_hits.load(Relaxed), 2);
        let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photo_albums")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(links, 5000);
    }

    #[test]
    fn language_policy_prefers_simplified_chinese_then_chinese_then_english() {
        assert_eq!(
            GEO_LANGUAGE_PREFERENCE.split(',').collect::<Vec<_>>(),
            vec![
                "zh-CN", "zh-Hans", "zh-SG", "zh-HK", "zh-TW", "zh-Hant", "zh", "en-US", "en-GB",
                "en"
            ],
        );
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

        let albums: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM albums WHERE kind = 'location'")
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
        let hits = AtomicUsize::new(0);
        let failed = AtomicUsize::new(0);
        // Only pass id1 — id2 should not get an album.
        group_by_location_scoped(&pool, &[id1], &total, &done, &hits, &failed)
            .await
            .unwrap();

        assert_eq!(total.load(Relaxed), 1);
        assert_eq!(done.load(Relaxed), 1);

        let albums: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM albums WHERE kind = 'location' ORDER BY name")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(albums.len(), 1, "only Tokyo should be created");
        assert_eq!(albums[0].0, "Tokyo");

        // id2 (Paris) must have no album link.
        let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photo_albums WHERE photo_id = ?")
            .bind(id2)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(links, 0);
    }

    #[tokio::test]
    async fn scoped_geo_skips_photos_without_gps() {
        let pool = test_pool().await;
        let id = insert_photo(&pool, "/no-gps.jpg", None, None).await;
        let total = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let hits = AtomicUsize::new(0);
        let failed = AtomicUsize::new(0);
        group_by_location_scoped(&pool, &[id], &total, &done, &hits, &failed)
            .await
            .unwrap();
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
        assert_eq!(
            count.0, 0,
            "all-null geocache → Nominatim retry → fails in test → no album"
        );
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
        assert_eq!(
            count.0, 0,
            "country-only geocache → treated as permanent → no album"
        );
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

    // --- proximity cache tests ---

    #[tokio::test]
    async fn proximity_cache_returns_nearby_city() {
        let pool = test_pool().await;
        let lat = 35.0;
        let lon = 139.0;
        insert_photo(&pool, "/p.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon + 0.005, Some("Tokyo"), Some("Tokyo-to")).await;

        group_by_location(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM albums WHERE name = 'Tokyo'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count, 1,
            "proximity cache should create album from nearby entry"
        );
    }

    #[tokio::test]
    async fn proximity_cache_writes_back_exact_key() {
        let pool = test_pool().await;
        let lat = 35.0;
        let lon = 139.0;
        insert_photo(&pool, "/p.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon + 0.005, Some("Tokyo"), Some("Tokyo-to")).await;

        group_by_location(&pool).await.unwrap();

        let city: Option<String> =
            sqlx::query_scalar("SELECT city FROM geocache WHERE lat_key = ? AND lon_key = ?")
                .bind(coord_key(lat))
                .bind(coord_key(lon))
                .fetch_optional(&pool)
                .await
                .unwrap()
                .flatten();
        assert_eq!(
            city.as_deref(),
            Some("Tokyo"),
            "proximity hit should be written to exact key"
        );
        let revision: i64 = sqlx::query_scalar(
            "SELECT name_policy_revision FROM geocache WHERE lat_key = ? AND lon_key = ?",
        )
        .bind(coord_key(lat))
        .bind(coord_key(lon))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            revision, 0,
            "copying a legacy nearby name must not mark it normalized"
        );
    }

    #[tokio::test]
    async fn proximity_cache_ignores_all_null_nearby() {
        // Use ocean coordinates (Gulf of Guinea) so Nominatim returns no city either,
        // making the "no album" assertion reliable regardless of network availability.
        let pool = test_pool().await;
        let lat = 0.0;
        let lon = 0.0;
        insert_photo(&pool, "/p.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon + 0.005, None, None).await; // all-NULL, invalid

        group_by_location(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "all-NULL nearby entry should not be used as proximity cache hit"
        );
    }

    #[tokio::test]
    async fn proximity_cache_out_of_range_not_used() {
        // Ocean coordinates so Nominatim also returns no city, making the assertion robust.
        let pool = test_pool().await;
        let lat = 0.0;
        let lon = 0.0;
        insert_photo(&pool, "/p.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon + 0.011, Some("ACity"), Some("AState")).await; // > 0.01°

        group_by_location(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM albums WHERE kind = 'location'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "entry beyond ±0.01° must not trigger proximity cache"
        );
    }

    #[tokio::test]
    async fn proximity_cache_prefers_closest() {
        let pool = test_pool().await;
        let lat = 35.0;
        let lon = 139.0;
        insert_photo(&pool, "/p.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon + 0.003, Some("Near City"), Some("S1")).await;
        seed_geocache(&pool, lat, lon + 0.008, Some("Far City"), Some("S2")).await;

        group_by_location(&pool).await.unwrap();

        let albums: Vec<String> =
            sqlx::query_scalar("SELECT name FROM albums WHERE kind = 'location' ORDER BY name")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            albums,
            vec!["Near City"],
            "should pick the closest valid entry"
        );
    }

    #[tokio::test]
    async fn proximity_cache_used_for_stale_exact_entry() {
        // Stale exact key (city set, state NULL) should fall through to proximity lookup.
        let pool = test_pool().await;
        let lat = 35.0;
        let lon = 139.0;
        insert_photo(&pool, "/p.jpg", Some(lat), Some(lon)).await;
        seed_geocache(&pool, lat, lon, Some("OldCity"), None).await; // stale: city set, state NULL
        seed_geocache(&pool, lat, lon + 0.005, Some("NewCity"), Some("S")).await;

        group_by_location(&pool).await.unwrap();

        let albums: Vec<String> =
            sqlx::query_scalar("SELECT name FROM albums WHERE kind = 'location' ORDER BY name")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            albums,
            vec!["NewCity"],
            "stale exact entry should fall through to proximity"
        );
    }
}
