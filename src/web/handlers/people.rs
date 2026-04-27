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
    cover_face_id: Option<i64>,
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

#[derive(Debug, Deserialize)]
pub struct CreatePersonBody {
    pub name: Option<String>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreatePersonResponse {
    pub id: i64,
}

#[derive(Debug, Deserialize)]
pub struct TransferFacesBody {
    pub target_person_id: i64,
    pub photo_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct TransferFacesResponse {
    pub faces_moved: u64,
}

#[derive(Debug, Deserialize)]
pub struct LiftPersonBody {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct LiftPersonResponse {
    pub new_person_id: i64,
}

pub async fn get_person_photos(
    State(state): State<AppState>,
    Path(person_id): Path<i64>,
    Query(pag): Query<Pagination>,
) -> Result<Json<PersonPhotos>, StatusCode> {
    let offset = (pag.page.saturating_sub(1)) as i64 * pag.per_page as i64;
    let limit = pag.per_page as i64;

    let total: (i64,) = sqlx::query_as(
        "WITH RECURSIVE subtree(id) AS (
           SELECT id FROM people WHERE id = ?
           UNION ALL
           SELECT p.id FROM people p JOIN subtree s ON p.parent_id = s.id
         )
         SELECT COUNT(DISTINCT f.photo_id)
         FROM person_faces pf
         JOIN faces f ON f.id = pf.face_id
         JOIN subtree s ON pf.person_id = s.id",
    )
    .bind(person_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let photos: Vec<PhotoRow> = sqlx::query_as(
        "WITH RECURSIVE subtree(id) AS (
           SELECT id FROM people WHERE id = ?
           UNION ALL
           SELECT p.id FROM people p JOIN subtree s ON p.parent_id = s.id
         )
         SELECT DISTINCT ph.id, ph.path, ph.format, ph.taken_at, ph.camera, ph.import_status
         FROM person_faces pf
         JOIN faces f ON f.id = pf.face_id
         JOIN photos ph ON ph.id = f.photo_id
         JOIN subtree s ON pf.person_id = s.id
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
    let rows: Vec<(i64, Option<String>, Option<i64>, Option<i64>)> =
        sqlx::query_as(
            "SELECT id, name, parent_id,
                    COALESCE(cover_face_id,
                        (SELECT pf.face_id FROM person_faces pf
                         WHERE pf.person_id = people.id ORDER BY pf.face_id LIMIT 1))
             FROM people ORDER BY id",
        )
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build tree from flat list
    fn build(
        all: &[(i64, Option<String>, Option<i64>, Option<i64>)],
        parent: Option<i64>,
    ) -> Vec<PersonNode> {
        all.iter()
            .filter(|(_, _, p, _)| *p == parent)
            .map(|(id, name, _, cover_face_id)| PersonNode {
                id: *id,
                name: name.clone(),
                cover_face_id: *cover_face_id,
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

pub async fn incremental_cluster_people(
    State(state): State<AppState>,
) -> Result<Json<ClusterResponse>, StatusCode> {
    let count = crate::face::cluster::run_incremental_clustering(&state.pool)
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
    let thumb = cropped.resize_to_fill(160, 160, image::imageops::FilterType::Triangle);
    let mut buf = Vec::new();
    thumb.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)?;
    Ok(buf)
}

pub async fn create_person(
    State(state): State<AppState>,
    Json(body): Json<CreatePersonBody>,
) -> Result<Json<CreatePersonResponse>, StatusCode> {
    let result = sqlx::query(
        "INSERT INTO people (name, parent_id, status) VALUES (?, ?, 'active')",
    )
    .bind(&body.name)
    .bind(body.parent_id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(CreatePersonResponse { id: result.last_insert_rowid() }))
}

pub async fn transfer_faces(
    State(state): State<AppState>,
    Path(source_id): Path<i64>,
    Json(body): Json<TransferFacesBody>,
) -> Result<Json<TransferFacesResponse>, StatusCode> {
    if body.photo_ids.is_empty() {
        return Ok(Json(TransferFacesResponse { faces_moved: 0 }));
    }

    let placeholders = body.photo_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

    // Copy matching face records to target person
    let insert_sql = format!(
        "INSERT OR IGNORE INTO person_faces (person_id, face_id)
         SELECT ?, f.id FROM faces f
         JOIN person_faces pf ON pf.face_id = f.id
         WHERE pf.person_id = ? AND f.photo_id IN ({placeholders})"
    );
    let mut q = sqlx::query(&insert_sql).bind(body.target_person_id).bind(source_id);
    for id in &body.photo_ids { q = q.bind(id); }
    q.execute(&state.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Remove from source person
    let delete_sql = format!(
        "DELETE FROM person_faces WHERE person_id = ? AND face_id IN (
           SELECT f.id FROM faces f WHERE f.photo_id IN ({placeholders})
         )"
    );
    let mut q = sqlx::query(&delete_sql).bind(source_id);
    for id in &body.photo_ids { q = q.bind(id); }
    let result = q.execute(&state.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Set cover_face_id for target if currently null
    sqlx::query(
        "UPDATE people SET cover_face_id = (
           SELECT pf.face_id FROM person_faces pf WHERE pf.person_id = ? ORDER BY pf.face_id LIMIT 1
         ) WHERE id = ? AND cover_face_id IS NULL",
    )
    .bind(body.target_person_id)
    .bind(body.target_person_id)
    .execute(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TransferFacesResponse { faces_moved: result.rows_affected() }))
}

pub async fn delete_person(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let face_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM person_faces WHERE person_id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if face_count.0 > 0 {
        return Err(StatusCode::CONFLICT);
    }

    let result = sqlx::query("DELETE FROM people WHERE id = ?")
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

// ── merge suggestions ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MergeSuggestion {
    pub person_id: i64,
    pub name: Option<String>,
    pub cover_face_id: Option<i64>,
    pub photo_count: i64,
    pub face_count: i64,
    pub distance: f32,
}

#[derive(Debug, Deserialize)]
pub struct MergeSuggestionsQuery {
    pub limit: Option<i64>,
}

pub async fn get_merge_suggestions(
    State(state): State<AppState>,
    Path(target_id): Path<i64>,
    Query(params): Query<MergeSuggestionsQuery>,
) -> Result<Json<Vec<MergeSuggestion>>, StatusCode> {
    use crate::face::embedder::decode_embedding;
    use crate::face::cluster::cosine_distance;

    let limit = params.limit.unwrap_or(5).max(1).min(20);

    // Load target person's face IDs + embeddings + confidence
    let target_rows: Vec<(i64, Vec<u8>, f32)> = sqlx::query_as(
        "SELECT f.id, f.embedding, COALESCE(f.confidence, 0.0) FROM person_faces pf
         JOIN faces f ON f.id = pf.face_id
         WHERE pf.person_id = ? AND f.embedding IS NOT NULL",
    )
    .bind(target_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if target_rows.is_empty() {
        return Ok(Json(vec![]));
    }

    let target_centroid = {
        let faces: Vec<(i64, Vec<f32>, f32)> = target_rows
            .iter()
            .map(|(id, b, c)| (*id, decode_embedding(b), *c))
            .collect();
        let (c, _) = compute_refined_centroid(&faces);
        c
    };

    // Load all other people's face IDs + embeddings + confidence in one pass
    let all_emb_rows: Vec<(i64, i64, Vec<u8>, f32)> = sqlx::query_as(
        "SELECT pf.person_id, f.id, f.embedding, COALESCE(f.confidence, 0.0)
         FROM person_faces pf
         JOIN faces f ON f.id = pf.face_id
         JOIN people p ON p.id = pf.person_id
         WHERE pf.person_id != ? AND p.status = 'active' AND f.embedding IS NOT NULL",
    )
    .bind(target_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if all_emb_rows.is_empty() {
        return Ok(Json(vec![]));
    }

    // Group (face_id, embedding, confidence) by person_id and compute refined centroid per person
    let mut person_faces: std::collections::HashMap<i64, Vec<(i64, Vec<f32>, f32)>> =
        std::collections::HashMap::new();
    for (pid, fid, bytes, conf) in &all_emb_rows {
        person_faces
            .entry(*pid)
            .or_default()
            .push((*fid, decode_embedding(bytes), *conf));
    }

    // Compute distances using refined centroids
    let mut scored: Vec<(i64, f32)> = person_faces
        .into_iter()
        .map(|(pid, faces)| {
            let (centroid, _) = compute_refined_centroid(&faces);
            let dist = cosine_distance(&target_centroid, &centroid);
            (pid, dist)
        })
        .collect();
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit as usize);

    if scored.is_empty() {
        return Ok(Json(vec![]));
    }

    // Fetch metadata for the top candidates
    let ids: Vec<i64> = scored.iter().map(|(id, _)| *id).collect();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT p.id, p.name,
                COALESCE(p.cover_face_id,
                    (SELECT pf2.face_id FROM person_faces pf2
                     WHERE pf2.person_id = p.id ORDER BY pf2.face_id LIMIT 1)) AS cover_face_id,
                COUNT(DISTINCT pf.face_id) AS face_count,
                COUNT(DISTINCT f.photo_id) AS photo_count
         FROM people p
         LEFT JOIN person_faces pf ON pf.person_id = p.id
         LEFT JOIN faces f ON f.id = pf.face_id
         WHERE p.id IN ({placeholders})
         GROUP BY p.id"
    );
    let mut q = sqlx::query(&sql);
    for id in &ids { q = q.bind(id); }
    let meta_rows = q
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build map: person_id → (name, cover_face_id, face_count, photo_count)
    let mut meta_map: std::collections::HashMap<i64, (Option<String>, Option<i64>, i64, i64)> =
        std::collections::HashMap::new();
    for row in meta_rows {
        use sqlx::Row;
        let pid: i64 = row.get(0);
        let name: Option<String> = row.get(1);
        let cover_face_id: Option<i64> = row.get(2);
        let face_count: i64 = row.get(3);
        let photo_count: i64 = row.get(4);
        meta_map.insert(pid, (name, cover_face_id, face_count, photo_count));
    }

    let suggestions = scored
        .into_iter()
        .filter_map(|(pid, dist)| {
            meta_map.remove(&pid).map(|(name, cover_face_id, face_count, photo_count)| {
                MergeSuggestion { person_id: pid, name, cover_face_id, photo_count, face_count, distance: dist }
            })
        })
        .collect();

    Ok(Json(suggestions))
}

// 50 张以上取最近 40%，避免口罩/侧脸 embedding 拉偏质心
const REFINE_THRESHOLD: usize = 50;
const REFINE_PCT: f32 = 0.40;
// 优先用高置信度人脸计算质心（排除暗光/畸变照片）
const CENTROID_HIGH_CONF: f32 = 0.85;
const CENTROID_LOW_CONF: f32 = 0.70;
const CENTROID_MIN_CONF_FACES: usize = 10;

fn centroid_from_embs(embs: &[Vec<f32>]) -> Vec<f32> {
    use crate::face::embedder::l2_normalize;
    if embs.is_empty() { return vec![]; }
    let dim = embs[0].len();
    let mut sum = vec![0.0f32; dim];
    for e in embs { for (s, x) in sum.iter_mut().zip(e.iter()) { *s += x; } }
    l2_normalize(&sum)
}

/// Returns (centroid, selected_face_ids).
/// Step 1: prefer faces with confidence ≥ 0.85 (fallback to ≥ 0.70, then all).
/// Step 2: for 50+ remaining faces, re-compute from the closest 40% to a rough centroid.
pub(crate) fn compute_refined_centroid(faces: &[(i64, Vec<f32>, f32)]) -> (Vec<f32>, Vec<i64>) {
    use crate::face::cluster::cosine_distance;
    if faces.is_empty() { return (vec![], vec![]); }

    // Step 1: confidence-based pre-filter
    let filter_conf = |min: f32| -> Vec<(i64, &Vec<f32>)> {
        faces.iter()
            .filter(|(_, _, c)| *c >= min)
            .map(|(id, emb, _)| (*id, emb))
            .collect()
    };
    let candidates: Vec<(i64, &Vec<f32>)> = {
        let high = filter_conf(CENTROID_HIGH_CONF);
        if high.len() >= CENTROID_MIN_CONF_FACES { high }
        else {
            let mid = filter_conf(CENTROID_LOW_CONF);
            if mid.len() >= CENTROID_MIN_CONF_FACES { mid }
            else { faces.iter().map(|(id, emb, _)| (*id, emb)).collect() }
        }
    };

    // Step 2: geometric refinement on confidence-filtered candidates
    let all_embs: Vec<Vec<f32>> = candidates.iter().map(|(_, e)| (*e).clone()).collect();
    let rough = centroid_from_embs(&all_embs);

    let keep = if candidates.len() > REFINE_THRESHOLD {
        ((candidates.len() as f32 * REFINE_PCT) as usize).max(1)
    } else {
        candidates.len()
    };

    if keep == candidates.len() {
        return (rough, candidates.iter().map(|(id, _)| *id).collect());
    }

    let mut sorted: Vec<(i64, &Vec<f32>, f32)> = candidates
        .iter()
        .map(|(id, emb)| (*id, *emb, cosine_distance(*emb, &rough)))
        .collect();
    sorted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    let sel = &sorted[..keep];
    let sel_embs: Vec<Vec<f32>> = sel.iter().map(|(_, e, _)| (*e).clone()).collect();
    let sel_ids: Vec<i64> = sel.iter().map(|(id, _, _)| *id).collect();
    (centroid_from_embs(&sel_embs), sel_ids)
}

#[cfg(test)]
mod centroid_tests {
    use super::*;

    fn uv(dim: usize, hot: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; dim];
        v[hot] = 1.0;
        v
    }

    // helper: (id, unit-vector at `hot`, confidence)
    fn face(id: i64, hot: usize, conf: f32) -> (i64, Vec<f32>, f32) {
        (id, uv(4, hot), conf)
    }

    #[test]
    fn uses_all_when_few() {
        let faces: Vec<(i64, Vec<f32>, f32)> = (0..10i64).map(|i| face(i, 0, 1.0)).collect();
        let (centroid, selected) = compute_refined_centroid(&faces);
        assert_eq!(selected.len(), 10);
        assert!(!centroid.is_empty());
    }

    #[test]
    fn filters_outliers_when_many() {
        // 60 faces: 40 close (dim-0), 20 far (dim-1), all high-confidence
        let mut faces: Vec<(i64, Vec<f32>, f32)> = (0..40i64).map(|i| face(i, 0, 1.0)).collect();
        faces.extend((40..60i64).map(|i| face(i, 1, 1.0)));
        let (centroid, selected) = compute_refined_centroid(&faces);
        // 40% of 60 = 24
        assert_eq!(selected.len(), 24);
        assert!(selected.iter().all(|&id| id < 40));
        assert!(centroid[0] > centroid[1]);
    }

    #[test]
    fn prefers_high_conf_faces() {
        // 15 high-conf faces pointing dim-0; 20 low-conf faces pointing dim-1
        let mut faces: Vec<(i64, Vec<f32>, f32)> =
            (0..15i64).map(|i| face(i, 0, 0.90)).collect();
        faces.extend((15..35i64).map(|i| face(i, 1, 0.50)));
        let (centroid, selected) = compute_refined_centroid(&faces);
        // Only high-conf faces used (ids 0-14)
        assert!(selected.iter().all(|&id| id < 15));
        // Centroid should point toward dim-0
        assert!(centroid[0] > centroid[1]);
    }

    #[test]
    fn falls_back_to_low_conf_when_too_few_high() {
        // 5 high-conf (< CENTROID_MIN_CONF_FACES=10), 12 mid-conf, 10 low-conf
        let mut faces: Vec<(i64, Vec<f32>, f32)> =
            (0..5i64).map(|i| face(i, 0, 0.90)).collect();   // high: 5  → not enough
        faces.extend((5..17i64).map(|i| face(i, 0, 0.75)));  // mid: 12  → enough
        faces.extend((17..27i64).map(|i| face(i, 1, 0.50))); // low: 10  (different direction)
        let (centroid, selected) = compute_refined_centroid(&faces);
        // Should use high + mid (ids 0-16), not the low-conf group
        assert!(selected.iter().all(|&id| id < 17));
        assert!(centroid[0] > centroid[1]);
    }

    #[test]
    fn falls_back_to_all_when_too_few_conf() {
        // Only 3 faces total, all low confidence — must use all
        let faces: Vec<(i64, Vec<f32>, f32)> = vec![
            face(0, 0, 0.40), face(1, 0, 0.50), face(2, 0, 0.60),
        ];
        let (_, selected) = compute_refined_centroid(&faces);
        assert_eq!(selected.len(), 3);
    }
}

// ── outlier faces ─────────────────────────────────────────────────────────────

const OUTLIER_MIN_DISTANCE: f32 = 0.50;

#[derive(Debug, Serialize)]
pub struct OutlierFace {
    pub face_id: i64,
    pub photo_id: i64,
    pub distance: f32,
    pub confidence: f32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Deserialize)]
pub struct OutlierFacesQuery {
    pub limit: Option<i64>,
}

pub async fn get_outlier_faces(
    State(state): State<AppState>,
    Path(person_id): Path<i64>,
    Query(params): Query<OutlierFacesQuery>,
) -> Result<Json<Vec<OutlierFace>>, StatusCode> {
    use crate::face::embedder::decode_embedding;
    use crate::face::cluster::cosine_distance;

    let limit = params.limit.unwrap_or(5).max(1).min(20);

    // Load all faces for this person that have embeddings
    let rows: Vec<(i64, i64, Vec<u8>, f32, i32, i32, i32, i32)> = sqlx::query_as(
        "SELECT f.id, f.photo_id, f.embedding, COALESCE(f.confidence, 0.0),
                f.x, f.y, f.width, f.height
         FROM person_faces pf
         JOIN faces f ON f.id = pf.face_id
         WHERE pf.person_id = ? AND f.embedding IS NOT NULL",
    )
    .bind(person_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Need at least 2 faces to detect outliers
    if rows.len() < 2 {
        return Ok(Json(vec![]));
    }

    let faces_for_centroid: Vec<(i64, Vec<f32>, f32)> = rows
        .iter()
        .map(|(face_id, _, b, conf, ..)| (*face_id, decode_embedding(b), *conf))
        .collect();
    let (centroid, _) = compute_refined_centroid(&faces_for_centroid);
    let embs: Vec<Vec<f32>> = faces_for_centroid.into_iter().map(|(_, e, _)| e).collect();

    let mut scored: Vec<OutlierFace> = rows
        .iter()
        .zip(embs.iter())
        .map(|((face_id, photo_id, _, conf, x, y, w, h), emb)| {
            let dist = cosine_distance(emb, &centroid);
            OutlierFace {
                face_id: *face_id,
                photo_id: *photo_id,
                distance: dist,
                confidence: *conf,
                x: *x,
                y: *y,
                width: *w,
                height: *h,
            }
        })
        .filter(|o| o.distance > OUTLIER_MIN_DISTANCE)
        .collect();

    scored.sort_by(|a, b| b.distance.partial_cmp(&a.distance).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit as usize);
    Ok(Json(scored))
}

// ── centroid faces ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CentroidFacesResponse {
    pub photo_ids: Vec<i64>,
    pub emb_count: usize,
    pub centroid_size: usize,
    pub min_dist: f32,
    pub p25_dist: f32,
    pub median_dist: f32,
    pub p75_dist: f32,
    pub max_dist: f32,
}

pub async fn get_centroid_faces(
    State(state): State<AppState>,
    Path(person_id): Path<i64>,
) -> Result<Json<CentroidFacesResponse>, StatusCode> {
    use crate::face::cluster::cosine_distance;
    use crate::face::embedder::decode_embedding;

    let rows: Vec<(i64, i64, Vec<u8>, f32)> = sqlx::query_as(
        "SELECT f.id, f.photo_id, f.embedding, COALESCE(f.confidence, 0.0)
         FROM person_faces pf
         JOIN faces f ON f.id = pf.face_id
         WHERE pf.person_id = ? AND f.embedding IS NOT NULL",
    )
    .bind(person_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let emb_count = rows.len();
    if rows.is_empty() {
        return Ok(Json(CentroidFacesResponse {
            photo_ids: vec![],
            emb_count: 0,
            centroid_size: 0,
            min_dist: 0.0,
            p25_dist: 0.0,
            median_dist: 0.0,
            p75_dist: 0.0,
            max_dist: 0.0,
        }));
    }

    let faces: Vec<(i64, Vec<f32>, f32)> = rows
        .iter()
        .map(|(face_id, _, bytes, conf)| (*face_id, decode_embedding(bytes), *conf))
        .collect();

    let (centroid, selected_ids) = compute_refined_centroid(&faces);

    // Compute distances from all faces to the refined centroid
    let mut all_dists: Vec<f32> = faces
        .iter()
        .map(|(_, emb, _)| cosine_distance(&centroid, emb))
        .collect();
    all_dists.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let n = all_dists.len();
    let pct = |p: f32| all_dists[((p * n as f32) as usize).min(n - 1)];

    let face_to_photo: std::collections::HashMap<i64, i64> =
        rows.iter().map(|(fid, pid, _, _)| (*fid, *pid)).collect();

    let photo_ids: Vec<i64> = selected_ids
        .iter()
        .filter_map(|fid| face_to_photo.get(fid).copied())
        .collect();

    Ok(Json(CentroidFacesResponse {
        photo_ids,
        emb_count,
        centroid_size: selected_ids.len(),
        min_dist: all_dists[0],
        p25_dist: pct(0.25),
        median_dist: pct(0.50),
        p75_dist: pct(0.75),
        max_dist: *all_dists.last().unwrap(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct EjectFaceBody {
    pub face_id: i64,
}

#[derive(Debug, Serialize)]
pub struct EjectFaceResponse {
    pub new_person_id: i64,
}

pub async fn eject_face(
    State(state): State<AppState>,
    Path(person_id): Path<i64>,
    Json(body): Json<EjectFaceBody>,
) -> Result<Json<EjectFaceResponse>, StatusCode> {
    // Verify the face belongs to this person
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM person_faces WHERE person_id = ? AND face_id = ?",
    )
    .bind(person_id)
    .bind(body.face_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if count.0 == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    // Remove from current person
    sqlx::query("DELETE FROM person_faces WHERE person_id = ? AND face_id = ?")
        .bind(person_id)
        .bind(body.face_id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create a new unnamed person for this face
    let new_person_id: i64 = sqlx::query_scalar(
        "INSERT INTO people (cover_face_id, status) VALUES (?, 'active') RETURNING id",
    )
    .bind(body.face_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
        .bind(new_person_id)
        .bind(body.face_id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(EjectFaceResponse { new_person_id }))
}

pub async fn lift_person(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<LiftPersonBody>,
) -> Result<Json<LiftPersonResponse>, StatusCode> {
    let mut tx = state.pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create new parent inheriting current person's parent_id
    let insert = sqlx::query(
        "INSERT INTO people (name, parent_id, status)
         VALUES (?, (SELECT parent_id FROM people WHERE id = ?), 'active')",
    )
    .bind(&body.name)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let new_parent_id = insert.last_insert_rowid();

    // Reparent current person to new parent
    sqlx::query("UPDATE people SET parent_id = ? WHERE id = ?")
        .bind(new_parent_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tx.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LiftPersonResponse { new_person_id: new_parent_id }))
}
