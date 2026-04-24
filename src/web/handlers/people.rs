use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use crate::web::AppState;
use crate::web::handlers::photos::{PhotoRow, Pagination};

#[derive(Debug, Serialize)]
pub struct PersonRow {
    pub id: i64,
    pub name: Option<String>,
    pub parent_id: Option<i64>,
    pub cover_face_id: Option<i64>,
    pub face_count: i64,
    pub photo_count: i64,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct PeopleQuery {
    pub status: Option<String>,
    pub name_exact: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchPersonBody {
    pub name: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatchUpdatePeopleBody {
    pub ids: Vec<i64>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct BatchUpdateResponse {
    pub updated: u64,
}

#[derive(Debug, Serialize)]
pub struct PersonPhotos {
    pub photos: Vec<PhotoRow>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Serialize)]
pub struct PersonNode {
    id: i64,
    name: Option<String>,
    children: Vec<PersonNode>,
}

#[derive(Debug, Serialize)]
pub struct PeopleTree {
    pub people: Vec<PersonNode>,
}

#[derive(Debug, Serialize)]
pub struct ClusterResponse {
    pub people_created: usize,
}

#[derive(Debug, Deserialize)]
pub struct MergeBody {
    pub source_id: i64,
    pub target_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct ReparentBody {
    pub new_parent_id: Option<i64>,
}

pub async fn get_person_photos(
    State(state): State<AppState>,
    Path(person_id): Path<i64>,
    Query(pag): Query<Pagination>,
) -> Result<Json<PersonPhotos>, StatusCode> {
    let offset = (pag.page.saturating_sub(1)) as i64 * pag.per_page as i64;
    let limit = pag.per_page as i64;

    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT f.photo_id) FROM person_faces pf
         JOIN faces f ON f.id = pf.face_id WHERE pf.person_id = ?",
    )
    .bind(person_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let photos: Vec<PhotoRow> = sqlx::query_as(
        "SELECT DISTINCT ph.id, ph.path, ph.format, ph.taken_at, ph.camera, ph.import_status
         FROM person_faces pf
         JOIN faces f ON f.id = pf.face_id
         JOIN photos ph ON ph.id = f.photo_id
         WHERE pf.person_id = ?
         ORDER BY ph.taken_at DESC NULLS LAST, ph.id DESC
         LIMIT ? OFFSET ?",
    )
    .bind(person_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(PersonPhotos {
        photos,
        total: total.0,
        page: pag.page,
        per_page: pag.per_page,
    }))
}

pub async fn get_people_tree(
    State(state): State<AppState>,
) -> Result<Json<PeopleTree>, StatusCode> {
    let rows: Vec<(i64, Option<String>, Option<i64>)> =
        sqlx::query_as("SELECT id, name, parent_id FROM people ORDER BY id")
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build tree from flat list
    fn build(
        all: &[(i64, Option<String>, Option<i64>)],
        parent: Option<i64>,
    ) -> Vec<PersonNode> {
        all.iter()
            .filter(|(_, _, p)| *p == parent)
            .map(|(id, name, _)| PersonNode {
                id: *id,
                name: name.clone(),
                children: build(all, Some(*id)),
            })
            .collect()
    }

    Ok(Json(PeopleTree { people: build(&rows, None) }))
}

pub async fn list_people(
    State(state): State<AppState>,
    Query(params): Query<PeopleQuery>,
) -> Result<Json<Vec<PersonRow>>, StatusCode> {
    const BASE: &str = "SELECT p.id, p.name, p.parent_id, p.status,
                    COALESCE(p.cover_face_id,
                        (SELECT pf2.face_id FROM person_faces pf2
                         WHERE pf2.person_id = p.id
                         ORDER BY pf2.face_id LIMIT 1)) AS cover_face_id,
                    COUNT(DISTINCT pf.face_id)       AS face_count,
                    COUNT(DISTINCT f.photo_id)       AS photo_count
             FROM people p
             LEFT JOIN person_faces pf ON pf.person_id = p.id
             LEFT JOIN faces f ON f.id = pf.face_id";

    type Row = (i64, Option<String>, Option<i64>, String, Option<i64>, i64, i64);

    let rows: Vec<Row> = if let Some(name) = &params.name_exact {
        sqlx::query_as(&format!("{BASE} WHERE p.name = ? GROUP BY p.id"))
            .bind(name)
            .fetch_all(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        let status_filter = params.status.as_deref().unwrap_or("active");
        if status_filter == "all" {
            sqlx::query_as(&format!("{BASE} GROUP BY p.id"))
                .fetch_all(&state.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        } else {
            sqlx::query_as(&format!("{BASE} WHERE p.status = ? GROUP BY p.id"))
                .bind(status_filter)
                .fetch_all(&state.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        }
    };

    let people = rows
        .into_iter()
        .map(|(id, name, parent_id, status, cover_face_id, face_count, photo_count)| PersonRow {
            id, name, parent_id, status, cover_face_id, face_count, photo_count,
        })
        .collect();
    Ok(Json(people))
}

pub async fn patch_person(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchPersonBody>,
) -> Result<StatusCode, StatusCode> {
    if body.name.is_none() && body.status.is_none() {
        return Ok(StatusCode::OK);
    }

    let mut affected: u64 = 0;

    if let Some(name) = &body.name {
        let r = sqlx::query("UPDATE people SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        affected = r.rows_affected();
    }

    if let Some(status) = &body.status {
        let r = sqlx::query("UPDATE people SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        affected = r.rows_affected();
    }

    if affected == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::OK)
    }
}

pub async fn batch_update_people(
    State(state): State<AppState>,
    Json(body): Json<BatchUpdatePeopleBody>,
) -> Result<Json<BatchUpdateResponse>, StatusCode> {
    if body.ids.is_empty() {
        return Ok(Json(BatchUpdateResponse { updated: 0 }));
    }

    let placeholders = body.ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("UPDATE people SET status = ? WHERE id IN ({placeholders})");
    let mut q = sqlx::query(&sql).bind(&body.status);
    for id in &body.ids {
        q = q.bind(id);
    }
    let result = q
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(BatchUpdateResponse { updated: result.rows_affected() }))
}

pub async fn cluster_people(
    State(state): State<AppState>,
) -> Result<Json<ClusterResponse>, StatusCode> {
    let count = crate::face::cluster::run_clustering(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ClusterResponse { people_created: count }))
}

pub async fn merge_people(
    State(state): State<AppState>,
    Json(body): Json<MergeBody>,
) -> Result<StatusCode, StatusCode> {
    // Move all person_faces from source to target (skip duplicates)
    sqlx::query(
        "INSERT OR IGNORE INTO person_faces (person_id, face_id)
         SELECT ?, face_id FROM person_faces WHERE person_id = ?",
    )
    .bind(body.target_id)
    .bind(body.source_id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Re-parent any children of source to target
    sqlx::query("UPDATE people SET parent_id = ? WHERE parent_id = ?")
        .bind(body.target_id)
        .bind(body.source_id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Delete source
    sqlx::query("DELETE FROM people WHERE id = ?")
        .bind(body.source_id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

pub async fn reparent_person(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ReparentBody>,
) -> Result<StatusCode, StatusCode> {
    let result = sqlx::query("UPDATE people SET parent_id = ? WHERE id = ?")
        .bind(body.new_parent_id)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if result.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::OK)
    }
}

pub async fn get_face_thumb(
    State(state): State<AppState>,
    Path(face_id): Path<i64>,
) -> Response {
    let row: Option<(i64, i64, i64, i64, String)> = sqlx::query_as(
        "SELECT f.x, f.y, f.width, f.height, p.path
         FROM faces f JOIN photos p ON p.id = f.photo_id
         WHERE f.id = ?",
    )
    .bind(face_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let Some((x, y, w, h, photo_path)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let cache_path = state.config.thumb_cache_dir.join(format!("face_{face_id}.jpg"));

    let result = tokio::task::spawn_blocking(move || {
        if cache_path.exists() {
            return std::fs::read(&cache_path).map_err(|e| anyhow::anyhow!(e));
        }
        let bytes = crop_face(&photo_path, x, y, w, h)?;
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cache_path, &bytes)?;
        Ok(bytes)
    })
    .await;

    match result {
        Ok(Ok(bytes)) => {
            use axum::http::header;
            ([(header::CONTENT_TYPE, "image/jpeg")], bytes).into_response()
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn crop_face(path: &str, x: i64, y: i64, w: i64, h: i64) -> anyhow::Result<Vec<u8>> {
    use image::{ImageFormat, ImageReader};
    use std::io::Cursor;

    let img = ImageReader::open(path)?.decode()?;
    let iw = img.width() as i64;
    let ih = img.height() as i64;

    // Clamp to image bounds
    let cx = x.max(0) as u32;
    let cy = y.max(0) as u32;
    let cw = (w.min(iw - x)).max(1) as u32;
    let ch = (h.min(ih - y)).max(1) as u32;

    let cropped = img.crop_imm(cx, cy, cw, ch);
    let thumb = cropped.thumbnail(160, 160);
    let mut buf = Vec::new();
    thumb.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)?;
    Ok(buf)
}
