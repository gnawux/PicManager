use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::web::AppState;

#[derive(Debug, Serialize)]
pub struct SpeciesEntry {
    pub species: String,
    pub chinese: String,
    pub photo_count: i64,
}

#[derive(Debug, Serialize)]
pub struct AnimalRow {
    pub id: i64,
    pub species: String,
    pub confidence: f64,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Deserialize)]
pub struct Pagination {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

fn default_page() -> u32 { 1 }
fn default_per_page() -> u32 { 50 }

#[derive(Debug, Serialize)]
pub struct PhotoList {
    pub photos: Vec<PhotoItem>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Serialize)]
pub struct PhotoItem {
    pub id: i64,
    pub path: String,
    pub format: String,
    pub taken_at: Option<String>,
}

fn chinese_name(species: &str) -> &'static str {
    match species {
        "bird"     => "鸟",
        "cat"      => "猫",
        "dog"      => "狗",
        "horse"    => "马",
        "sheep"    => "羊",
        "cow"      => "牛",
        "elephant" => "象",
        "bear"     => "熊",
        "zebra"    => "斑马",
        "giraffe"  => "长颈鹿",
        _          => "动物",
    }
}

pub async fn list_species(
    State(state): State<AppState>,
) -> Result<Json<Vec<SpeciesEntry>>, StatusCode> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT species, COUNT(DISTINCT photo_id) AS photo_count
         FROM animals GROUP BY species ORDER BY photo_count DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        rows.into_iter()
            .map(|(species, photo_count)| {
                let chinese = chinese_name(&species).to_string();
                SpeciesEntry { species, chinese, photo_count }
            })
            .collect(),
    ))
}

pub async fn list_species_photos(
    State(state): State<AppState>,
    Path(species): Path<String>,
    Query(pag): Query<Pagination>,
) -> Result<Json<PhotoList>, StatusCode> {
    let offset = (pag.page.saturating_sub(1)) as i64 * pag.per_page as i64;
    let limit = pag.per_page as i64;

    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT p.id) FROM photos p
         JOIN animals a ON a.photo_id = p.id WHERE a.species = ?",
    )
    .bind(&species)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let photos: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
        "SELECT DISTINCT p.id, p.path, p.format, p.taken_at
         FROM photos p JOIN animals a ON a.photo_id = p.id
         WHERE a.species = ?
         ORDER BY p.taken_at DESC NULLS LAST, p.id DESC
         LIMIT ? OFFSET ?",
    )
    .bind(&species)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(PhotoList {
        photos: photos
            .into_iter()
            .map(|(id, path, format, taken_at)| PhotoItem { id, path, format, taken_at })
            .collect(),
        total: total.0,
        page: pag.page,
        per_page: pag.per_page,
    }))
}

pub async fn list_photo_animals(
    State(state): State<AppState>,
    Path(photo_id): Path<i64>,
) -> Result<Json<Vec<AnimalRow>>, StatusCode> {
    let rows: Vec<(i64, String, f64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT id, species, confidence, x, y, width, height
         FROM animals WHERE photo_id = ? ORDER BY confidence DESC",
    )
    .bind(photo_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(
        rows.into_iter()
            .map(|(id, species, confidence, x, y, width, height)| AnimalRow {
                id, species, confidence, x, y, width, height,
            })
            .collect(),
    ))
}
