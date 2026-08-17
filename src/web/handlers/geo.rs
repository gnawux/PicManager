use axum::{extract::{Extension, Query, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use crate::application::RequestContext;
use crate::web::AppState;

#[derive(Debug, Serialize)]
pub struct CityEntry {
    pub name: String,
    pub query_value: String,
    pub photo_count: i64,
}

#[derive(Debug, Serialize)]
pub struct StateEntry {
    pub name: String,
    pub query_value: String,
    pub photo_count: i64,
    pub cities: Vec<CityEntry>,
}

#[derive(Debug, Serialize)]
pub struct CountryEntry {
    pub name: String,
    pub query_value: String,
    pub photo_count: i64,
    pub states: Vec<StateEntry>,
}

#[derive(Debug, Serialize)]
pub struct GeoHierarchy {
    pub countries: Vec<CountryEntry>,
}

fn hierarchy_value(value: Option<String>) -> (String, String) {
    match value {
        Some(value) => (value.clone(), value),
        None => ("Unknown".to_owned(), "__null__".to_owned()),
    }
}

const DEFAULT_CLUSTER_COLUMNS: i64 = 48;
const DEFAULT_CLUSTER_ROWS: i64 = 24;
const MAX_CLUSTER_COLUMNS: i64 = 64;
const MAX_CLUSTER_ROWS: i64 = 32;

#[derive(Debug, Deserialize)]
pub struct GeoClustersQuery {
    #[serde(default = "default_cluster_columns")]
    pub columns: i64,
    #[serde(default = "default_cluster_rows")]
    pub rows: i64,
    pub west: Option<f64>,
    pub east: Option<f64>,
    pub south: Option<f64>,
    pub north: Option<f64>,
}

fn default_cluster_columns() -> i64 { DEFAULT_CLUSTER_COLUMNS }
fn default_cluster_rows() -> i64 { DEFAULT_CLUSTER_ROWS }

#[derive(Debug, Serialize)]
pub struct GeoCluster {
    pub x_bin: i64,
    pub y_bin: i64,
    pub gps_lat: f64,
    pub gps_lon: f64,
    pub photo_count: i64,
    pub representative_photo_id: i64,
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
}

#[derive(Debug, Serialize)]
pub struct GeoClusterPage {
    pub clusters: Vec<GeoCluster>,
    pub total_photos: i64,
}

pub async fn get_geo_clusters(
    State(state): State<AppState>,
    Query(params): Query<GeoClustersQuery>,
) -> Result<Json<GeoClusterPage>, StatusCode> {
    let columns = params.columns.clamp(1, MAX_CLUSTER_COLUMNS);
    let rows = params.rows.clamp(1, MAX_CLUSTER_ROWS);
    let mut west = params.west.unwrap_or(-180.0).clamp(-180.0, 180.0);
    let mut east = params.east.unwrap_or(180.0).clamp(-180.0, 180.0);
    let mut south = params.south.unwrap_or(-90.0).clamp(-90.0, 90.0);
    let mut north = params.north.unwrap_or(90.0).clamp(-90.0, 90.0);
    if east - west < 0.000_001 { west = -180.0; east = 180.0; }
    if north - south < 0.000_001 { south = -90.0; north = 90.0; }
    let result: Vec<(i64, i64, f64, f64, i64, i64)> = sqlx::query_as(
        "WITH valid_photos AS (
             SELECT id, gps_lat, gps_lon,
                    MIN(? - 1, CAST(((gps_lon - ?) / (? - ?)) * ? AS INTEGER)) AS x_bin,
                    MIN(? - 1, CAST(((? - gps_lat) / (? - ?)) * ? AS INTEGER)) AS y_bin
             FROM photos
             WHERE import_status = 'imported'
               AND gps_lat BETWEEN ? AND ?
               AND gps_lon BETWEEN ? AND ?
         )
         SELECT x_bin, y_bin, AVG(gps_lat), AVG(gps_lon), COUNT(*), MIN(id)
         FROM valid_photos
         GROUP BY x_bin, y_bin
         ORDER BY y_bin, x_bin",
    )
    .bind(columns)
    .bind(west)
    .bind(east)
    .bind(west)
    .bind(columns)
    .bind(rows)
    .bind(north)
    .bind(north)
    .bind(south)
    .bind(rows)
    .bind(south)
    .bind(north)
    .bind(west)
    .bind(east)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let lon_step = (east - west) / columns as f64;
    let lat_step = (north - south) / rows as f64;
    let clusters = result.into_iter().map(
        |(x_bin, y_bin, gps_lat, gps_lon, photo_count, representative_photo_id)| GeoCluster {
            x_bin, y_bin, gps_lat, gps_lon, photo_count, representative_photo_id,
            west: west + x_bin as f64 * lon_step,
            east: west + (x_bin + 1) as f64 * lon_step,
            north: north - y_bin as f64 * lat_step,
            south: north - (y_bin + 1) as f64 * lat_step,
        },
    ).collect::<Vec<_>>();
    let total_photos = clusters.iter().map(|cluster| cluster.photo_count).sum();

    Ok(Json(GeoClusterPage { clusters, total_photos }))
}

#[derive(Debug, Deserialize)]
pub struct GeoClusterPhotosQuery {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
}

pub async fn get_geo_cluster_photos(
    State(state): State<AppState>,
    Query(params): Query<GeoClusterPhotosQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !params.west.is_finite() || !params.east.is_finite()
        || !params.south.is_finite() || !params.north.is_finite()
        || params.east <= params.west || params.north <= params.south
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let west = params.west.clamp(-180.0, 180.0);
    let east = params.east.clamp(-180.0, 180.0);
    let south = params.south.clamp(-90.0, 90.0);
    let north = params.north.clamp(-90.0, 90.0);
    let page = params.page.max(1);
    let per_page = params.per_page.clamp(1, 200);
    let offset = (page - 1) * per_page;
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM photos
         WHERE import_status = 'imported'
           AND gps_lat BETWEEN ? AND ? AND gps_lon BETWEEN ? AND ?",
    )
    .bind(south).bind(north).bind(west).bind(east)
    .fetch_one(&state.pool).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let photos: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, path, taken_at, camera FROM photos
         WHERE import_status = 'imported'
           AND gps_lat BETWEEN ? AND ? AND gps_lon BETWEEN ? AND ?
         ORDER BY taken_at DESC NULLS LAST, id DESC LIMIT ? OFFSET ?",
    )
    .bind(south).bind(north).bind(west).bind(east)
    .bind(per_page).bind(offset)
    .fetch_all(&state.pool).await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "total": total, "page": page, "per_page": per_page,
        "photos": photos.into_iter().map(|(id, path, taken_at, camera)| {
            serde_json::json!({ "id": id, "path": path, "taken_at": taken_at, "camera": camera })
        }).collect::<Vec<_>>(),
    })))
}

pub async fn get_geo_hierarchy(
    State(state): State<AppState>,
) -> Result<Json<GeoHierarchy>, StatusCode> {
    // Join photos with geocache via coordinate keys, group by hierarchy levels.
    // PRINTF('%.4f', ...) matches the coord_key() format used by location.rs.
    let rows: Vec<(Option<String>, Option<String>, Option<String>, i64)> = sqlx::query_as(
        "SELECT gc.country, gc.state, gc.city, COUNT(DISTINCT ph.id) AS cnt
         FROM photos ph
         JOIN geocache gc
           ON PRINTF('%.4f', ph.gps_lat) = gc.lat_key
          AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key
         WHERE ph.import_status = 'imported'
           AND ph.gps_lat IS NOT NULL
         GROUP BY gc.country, gc.state, gc.city
         ORDER BY gc.country, gc.state, gc.city",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build nested structure
    let mut countries: Vec<CountryEntry> = Vec::new();

    for (country_opt, state_opt, city_opt, cnt) in rows {
        let (country_name, country_query_value) = hierarchy_value(country_opt);
        let (state_name, state_query_value) = hierarchy_value(state_opt);
        let (city_name, city_query_value) = hierarchy_value(city_opt);

        let country = match countries.iter_mut().find(|c| c.query_value == country_query_value) {
            Some(c) => c,
            None => {
                countries.push(CountryEntry {
                    name: country_name.clone(),
                    query_value: country_query_value,
                    photo_count: 0,
                    states: vec![],
                });
                countries.last_mut().unwrap()
            }
        };
        country.photo_count += cnt;

        let st = match country.states.iter_mut().find(|s| s.query_value == state_query_value) {
            Some(s) => s,
            None => {
                country.states.push(StateEntry {
                    name: state_name.clone(),
                    query_value: state_query_value,
                    photo_count: 0,
                    cities: vec![],
                });
                country.states.last_mut().unwrap()
            }
        };
        st.photo_count += cnt;
        st.cities.push(CityEntry {
            name: city_name,
            query_value: city_query_value,
            photo_count: cnt,
        });
    }

    for country in &mut countries {
        for state in &mut country.states {
            state.cities.sort_by(|left, right| {
                right.photo_count.cmp(&left.photo_count).then_with(|| left.name.cmp(&right.name))
            });
        }
        country.states.sort_by(|left, right| {
            right.photo_count.cmp(&left.photo_count).then_with(|| left.name.cmp(&right.name))
        });
    }
    countries.sort_by(|left, right| {
        right.photo_count.cmp(&left.photo_count).then_with(|| left.name.cmp(&right.name))
    });

    Ok(Json(GeoHierarchy { countries }))
}

#[derive(Debug, Deserialize)]
pub struct GeoPhotosQuery {
    pub country: Option<String>,
    pub state: Option<String>,
    pub city: Option<String>,
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
}
fn default_page() -> i64 { 1 }
fn default_per_page() -> i64 { 50 }

pub async fn get_geo_photos(
    State(state): State<AppState>,
    Query(params): Query<GeoPhotosQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let page = params.page.max(1);
    let per_page = params.per_page.clamp(1, 200);
    let offset = (page - 1) * per_page;

    let join = "FROM photos ph
                JOIN geocache gc
                  ON PRINTF('%.4f', ph.gps_lat) = gc.lat_key
                 AND PRINTF('%.4f', ph.gps_lon) = gc.lon_key";

    // Build dynamic WHERE conditions
    let mut conds = vec![
        "ph.import_status = 'imported'".to_owned(),
        "ph.gps_lat IS NOT NULL".to_owned(),
    ];
    let mut binds: Vec<String> = vec![];

    for (field, val) in [("gc.country", &params.country), ("gc.state", &params.state), ("gc.city", &params.city)] {
        match val {
            None => {}
            Some(v) if v == "__null__" => conds.push(format!("{field} IS NULL")),
            Some(v) => {
                conds.push(format!("{field} = ?"));
                binds.push(v.clone());
            }
        }
    }

    let where_clause = conds.join(" AND ");

    // COUNT
    let count_sql = format!("SELECT COUNT(DISTINCT ph.id) {join} WHERE {where_clause}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds { count_q = count_q.bind(b); }
    let total: i64 = count_q
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // LIST
    let list_sql = format!(
        "SELECT ph.id, ph.path, ph.taken_at, ph.camera {join} WHERE {where_clause}
         ORDER BY ph.taken_at DESC NULLS LAST, ph.id DESC
         LIMIT ? OFFSET ?"
    );
    let mut list_q = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>)>(&list_sql);
    for b in &binds { list_q = list_q.bind(b); }
    list_q = list_q.bind(per_page).bind(offset);
    let photos = list_q
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "total": total,
        "page": page,
        "per_page": per_page,
        "photos": photos.into_iter().map(|(id, path, taken_at, camera)| {
            serde_json::json!({ "id": id, "path": path, "taken_at": taken_at, "camera": camera })
        }).collect::<Vec<_>>()
    })))
}

pub async fn start_regeocode(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let active = crate::jobs::list(&state.pool, None, Some("geocode"), None, 100)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .any(|job| matches!(job.status.as_str(), "queued" | "running" | "retry_wait"));
    if active {
        return Ok(Json(serde_json::json!({"status": "already_running"})));
    }

    // Count photos that will trigger a real Nominatim call:
    // - no geocache entry at all, OR
    // - stale entry (city set but state NULL, e.g. pre-fix direct-controlled municipalities), OR
    // - all-NULL entry (transient failure during a previous geocoding attempt)
    let count: i64 = sqlx::query_scalar(
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
                 AND gc.city  IS NOT NULL
                 AND gc.state IS NULL
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
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let queued = crate::jobs::handlers::enqueue_geocode(&state.application, &context)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"status": "started", "count": count, "job_id": queued.job.id})))
}

pub async fn get_geo_name_policy(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let outdated_photos = crate::album::location::count_outdated_geo_names(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "revision": crate::album::location::GEO_NAME_POLICY_REVISION,
        "language_preference": crate::album::location::GEO_LANGUAGE_PREFERENCE,
        "outdated_photos": outdated_photos,
    })))
}

pub async fn start_geo_name_normalization(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let active = crate::jobs::list(&state.pool, None, Some("geocode"), None, 100)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .any(|job| matches!(job.status.as_str(), "queued" | "running" | "retry_wait"));
    if active {
        return Ok(Json(serde_json::json!({"status": "already_running"})));
    }

    let count = crate::album::location::count_outdated_geo_names(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if count == 0 {
        return Ok(Json(serde_json::json!({"status": "up_to_date", "count": 0})));
    }
    let queued = crate::jobs::handlers::enqueue_geo_name_normalization(
        &state.application, &context,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "status": "started", "count": count, "job_id": queued.job.id,
    })))
}

pub async fn get_regeocode_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let running = crate::jobs::list(&state.pool, None, Some("geocode"), None, 100)
        .await
        .unwrap_or_default()
        .into_iter()
        .any(|job| matches!(job.status.as_str(), "queued" | "running" | "retry_wait"));
    Json(serde_json::json!({"running": running}))
}
