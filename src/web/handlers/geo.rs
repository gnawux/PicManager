use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use std::sync::atomic::Ordering;
use crate::web::AppState;

#[derive(Debug, Serialize)]
pub struct CityEntry {
    pub name: String,
    pub photo_count: i64,
}

#[derive(Debug, Serialize)]
pub struct StateEntry {
    pub name: String,
    pub photo_count: i64,
    pub cities: Vec<CityEntry>,
}

#[derive(Debug, Serialize)]
pub struct CountryEntry {
    pub name: String,
    pub photo_count: i64,
    pub states: Vec<StateEntry>,
}

#[derive(Debug, Serialize)]
pub struct GeoHierarchy {
    pub countries: Vec<CountryEntry>,
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
        let country_name = country_opt.unwrap_or_else(|| "Unknown".to_owned());
        let state_name = state_opt.unwrap_or_else(|| "Unknown".to_owned());
        let city_name = city_opt.unwrap_or_else(|| "Unknown".to_owned());

        let country = match countries.iter_mut().find(|c| c.name == country_name) {
            Some(c) => c,
            None => {
                countries.push(CountryEntry { name: country_name.clone(), photo_count: 0, states: vec![] });
                countries.last_mut().unwrap()
            }
        };
        country.photo_count += cnt;

        let st = match country.states.iter_mut().find(|s| s.name == state_name) {
            Some(s) => s,
            None => {
                country.states.push(StateEntry { name: state_name.clone(), photo_count: 0, cities: vec![] });
                country.states.last_mut().unwrap()
            }
        };
        st.photo_count += cnt;
        st.cities.push(CityEntry { name: city_name, photo_count: cnt });
    }

    Ok(Json(GeoHierarchy { countries }))
}

pub async fn start_regeocode(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if state.geo_running.swap(true, Ordering::SeqCst) {
        return Ok(Json(serde_json::json!({"status": "already_running"})));
    }

    // Count photos that will trigger a real Nominatim call:
    // - no geocache entry at all, OR
    // - stale entry (city set but state NULL, e.g. pre-fix direct-controlled municipalities)
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
           )",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|_| {
        state.geo_running.store(false, Ordering::SeqCst);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let pool = state.pool.clone();
    let running = state.geo_running.clone();
    tokio::spawn(async move {
        let _ = crate::album::group_by_location(&pool).await;
        running.store(false, Ordering::SeqCst);
    });

    Ok(Json(serde_json::json!({"status": "started", "count": count})))
}

pub async fn get_regeocode_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({"running": state.geo_running.load(Ordering::SeqCst)}))
}
