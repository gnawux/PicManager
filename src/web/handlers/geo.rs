use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
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
