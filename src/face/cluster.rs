use anyhow::Result;
use sqlx::SqlitePool;

use super::embedder::decode_embedding;

/// DBSCAN clustering on L2-normalised face embeddings.
/// Distance metric: cosine distance = 1 - dot(a, b).
/// Noise points are each returned as a single-element cluster so every face
/// gets a person record.
///
/// Returns groups of face IDs.
pub fn cluster_faces(
    faces: &[(i64, Vec<f32>)],
    eps: f32,
    min_samples: usize,
) -> Vec<Vec<i64>> {
    let n = faces.len();
    if n == 0 {
        return vec![];
    }

    // -1 = unvisited, 0 = noise, >0 = cluster id
    let mut labels: Vec<i32> = vec![-1; n];
    let mut cluster_id: i32 = 0;

    for i in 0..n {
        if labels[i] != -1 {
            continue;
        }
        let neighbors = region_query(faces, i, eps);
        if neighbors.len() < min_samples {
            labels[i] = 0; // noise for now
            continue;
        }
        cluster_id += 1;
        labels[i] = cluster_id;
        let mut seed = neighbors;
        let mut si = 0;
        while si < seed.len() {
            let q = seed[si];
            if labels[q] == 0 {
                labels[q] = cluster_id; // border point
            }
            if labels[q] == -1 {
                labels[q] = cluster_id;
                let q_neighbors = region_query(faces, q, eps);
                if q_neighbors.len() >= min_samples {
                    for &nb in &q_neighbors {
                        if !seed.contains(&nb) {
                            seed.push(nb);
                        }
                    }
                }
            }
            si += 1;
        }
    }

    // Group face IDs by cluster label; noise (0) → individual clusters
    let max_label = cluster_id as usize;
    let mut clusters: Vec<Vec<i64>> = vec![vec![]; max_label + 1];
    for (i, &label) in labels.iter().enumerate() {
        if label > 0 {
            clusters[label as usize].push(faces[i].0);
        } else {
            // noise → single-element cluster
            clusters.push(vec![faces[i].0]);
        }
    }

    clusters.into_iter().filter(|c| !c.is_empty()).collect()
}

// Returns all points within eps of faces[idx], INCLUDING idx itself.
// This matches the DBSCAN convention where min_samples counts the point itself.
fn region_query(faces: &[(i64, Vec<f32>)], idx: usize, eps: f32) -> Vec<usize> {
    let (_, ref a) = faces[idx];
    faces
        .iter()
        .enumerate()
        .filter(|(_, (_, b))| cosine_distance(a, b) <= eps)
        .map(|(j, _)| j)
        .collect()
}

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    1.0 - dot
}

/// Assign unassigned faces to existing people or form new person clusters,
/// without touching any existing `people` / `person_faces` rows.
///
/// Algorithm:
/// 1. Find faces with non-NULL embedding that have no `person_faces` entry.
/// 2. For each, find the nearest existing person (minimum cosine distance over
///    all that person's faces); assign if distance < eps (0.4).
/// 3. Run DBSCAN on remaining unassigned faces → create new person records.
///
/// Returns the number of *new* people created.
pub async fn run_incremental_clustering(pool: &SqlitePool) -> Result<usize> {
    // ── 1. Unassigned faces ────────────────────────────────────────────────
    let rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT f.id, f.embedding FROM faces f
         WHERE f.embedding IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM person_faces pf WHERE pf.face_id = f.id)",
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let unassigned: Vec<(i64, Vec<f32>)> = rows
        .into_iter()
        .filter_map(|(id, blob)| {
            let emb = decode_embedding(&blob);
            if emb.is_empty() { None } else { Some((id, emb)) }
        })
        .collect();

    // ── 2. Existing people's embeddings ────────────────────────────────────
    let existing_rows: Vec<(i64, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT pf.person_id, f.id, f.embedding
         FROM person_faces pf JOIN faces f ON f.id = pf.face_id
         WHERE f.embedding IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    // person_id → list of (face_id, embedding)
    let mut person_map: std::collections::HashMap<i64, Vec<(i64, Vec<f32>)>> =
        std::collections::HashMap::new();
    for (pid, fid, blob) in existing_rows {
        let emb = decode_embedding(&blob);
        if !emb.is_empty() {
            person_map.entry(pid).or_default().push((fid, emb));
        }
    }

    const EPS: f32 = 0.4;
    let mut still_unassigned: Vec<(i64, Vec<f32>)> = Vec::new();

    for (face_id, emb) in unassigned {
        let best = person_map.iter().filter_map(|(&pid, pfaces)| {
            pfaces
                .iter()
                .map(|(_, pe)| cosine_distance(&emb, pe))
                .reduce(f32::min)
                .map(|d| (d, pid))
        }).min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        if let Some((dist, pid)) = best {
            if dist < EPS {
                sqlx::query(
                    "INSERT OR IGNORE INTO person_faces (person_id, face_id) VALUES (?, ?)",
                )
                .bind(pid)
                .bind(face_id)
                .execute(pool)
                .await?;
                // Add to map so subsequent faces can match against it
                person_map.entry(pid).or_default().push((face_id, emb));
                continue;
            }
        }
        still_unassigned.push((face_id, emb));
    }

    // ── 3. DBSCAN on remaining unassigned → new people ────────────────────
    let clusters = cluster_faces(&still_unassigned, EPS, 2);
    let new_count = clusters.len();

    for group in clusters {
        let cover_face_id = {
            let mut best_id = group[0];
            let mut best_conf: f32 = sqlx::query_scalar(
                "SELECT confidence FROM faces WHERE id = ?",
            )
            .bind(best_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0.0);
            for &fid in &group[1..] {
                let conf: f32 = sqlx::query_scalar(
                    "SELECT confidence FROM faces WHERE id = ?",
                )
                .bind(fid)
                .fetch_one(pool)
                .await
                .unwrap_or(0.0);
                if conf > best_conf {
                    best_conf = conf;
                    best_id = fid;
                }
            }
            best_id
        };

        let person_id: i64 =
            sqlx::query_scalar("INSERT INTO people (cover_face_id) VALUES (?) RETURNING id")
                .bind(cover_face_id)
                .fetch_one(pool)
                .await?;
        for face_id in group {
            sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
                .bind(person_id)
                .bind(face_id)
                .execute(pool)
                .await?;
        }
    }

    Ok(new_count)
}

/// Re-cluster all faces with non-NULL embeddings, writing results to
/// the `people` / `person_faces` tables (full replacement each run).
/// Returns the number of people created.
pub async fn run_clustering(pool: &SqlitePool) -> Result<usize> {
    // Load all faces with embeddings
    let rows: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT id, embedding FROM faces WHERE embedding IS NOT NULL")
            .fetch_all(pool)
            .await?;

    let faces: Vec<(i64, Vec<f32>)> = rows
        .into_iter()
        .filter_map(|(id, blob)| {
            let emb = decode_embedding(&blob);
            if emb.is_empty() { None } else { Some((id, emb)) }
        })
        .collect();

    // Clear existing clustering
    sqlx::query("DELETE FROM person_faces").execute(pool).await?;
    sqlx::query("DELETE FROM people").execute(pool).await?;

    let clusters = cluster_faces(&faces, 0.4, 2);
    let count = clusters.len();

    for group in clusters {
        // Pick the face with highest detection confidence as the cover.
        let cover_face_id: i64 = {
            let mut best_id = group[0];
            let mut best_conf: f32 = sqlx::query_scalar(
                "SELECT confidence FROM faces WHERE id = ?",
            )
            .bind(best_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0.0);
            for &fid in &group[1..] {
                let conf: f32 = sqlx::query_scalar(
                    "SELECT confidence FROM faces WHERE id = ?",
                )
                .bind(fid)
                .fetch_one(pool)
                .await
                .unwrap_or(0.0);
                if conf > best_conf {
                    best_conf = conf;
                    best_id = fid;
                }
            }
            best_id
        };

        let person_id: i64 =
            sqlx::query_scalar("INSERT INTO people (cover_face_id) VALUES (?) RETURNING id")
                .bind(cover_face_id)
                .fetch_one(pool)
                .await?;
        for face_id in group {
            sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
                .bind(person_id)
                .bind(face_id)
                .execute(pool)
                .await?;
        }
    }

    Ok(count)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_vec(dim: usize, hot: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; dim];
        v[hot] = 1.0;
        v
    }

    /// Two embeddings close together (cosine dist ≈ 0) and one far away.
    #[test]
    fn two_close_one_far_gives_two_groups() {
        let faces = vec![
            (1i64, unit_vec(8, 0)), // cluster A
            (2i64, unit_vec(8, 0)), // cluster A (identical → dist 0)
            (3i64, unit_vec(8, 4)), // noise (far from both)
        ];
        let clusters = cluster_faces(&faces, 0.4, 2);
        assert_eq!(clusters.len(), 2);
        let sizes: Vec<usize> = {
            let mut s: Vec<usize> = clusters.iter().map(|c| c.len()).collect();
            s.sort();
            s
        };
        assert_eq!(sizes, vec![1, 2]);
    }

    /// Three clearly separated groups each with 2 members.
    #[test]
    fn three_distinct_groups() {
        let faces = vec![
            (1i64, unit_vec(8, 0)),
            (2i64, unit_vec(8, 0)),
            (3i64, unit_vec(8, 3)),
            (4i64, unit_vec(8, 3)),
            (5i64, unit_vec(8, 6)),
            (6i64, unit_vec(8, 6)),
        ];
        let clusters = cluster_faces(&faces, 0.4, 2);
        assert_eq!(clusters.len(), 3);
        for c in &clusters {
            assert_eq!(c.len(), 2);
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(cluster_faces(&[], 0.4, 2).is_empty());
    }

    #[test]
    fn single_point_is_noise_but_still_returned() {
        let faces = vec![(42i64, unit_vec(4, 0))];
        let clusters = cluster_faces(&faces, 0.4, 2);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0], vec![42]);
    }

    #[test]
    fn cosine_distance_identical_is_zero() {
        let v = unit_vec(8, 2);
        assert!((cosine_distance(&v, &v) - 0.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn run_clustering_groups_similar_faces() {
        use sqlx::sqlite::SqlitePoolOptions;
        use crate::face::embedder::encode_embedding;

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Insert 2 photos and 4 faces: 2 pairs with identical embeddings
        for pid in [1i64, 2] {
            sqlx::query(
                "INSERT INTO photos (id, path, sha256, format, import_status) VALUES (?, ?, ?, 'jpeg', 'imported')"
            )
            .bind(pid).bind(format!("/p{pid}")).bind(format!("sha{pid}"))
            .execute(&pool).await.unwrap();
        }

        let emb_a = encode_embedding(&unit_vec(512, 0));
        let emb_b = encode_embedding(&unit_vec(512, 256));
        let embs = [&emb_a, &emb_a, &emb_b, &emb_b];
        let photo_ids = [1i64, 1, 2, 2];
        for (emb, &pid) in embs.iter().zip(photo_ids.iter()) {
            sqlx::query(
                "INSERT INTO faces (photo_id, x, y, width, height, confidence, embedding) \
                 VALUES (?, 0, 0, 50, 50, 0.9, ?)"
            )
            .bind(pid).bind(emb.as_slice())
            .execute(&pool).await.unwrap();
        }

        let count = run_clustering(&pool).await.unwrap();
        assert_eq!(count, 2, "two distinct embedding clusters");

        let people_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM people").fetch_one(&pool).await.unwrap();
        assert_eq!(people_count, 2);

        // Each person has 2 faces
        let min_faces: i64 = sqlx::query_scalar(
            "SELECT MIN(cnt) FROM (SELECT COUNT(*) as cnt FROM person_faces GROUP BY person_id)"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(min_faces, 2);
    }

    #[test]
    fn cosine_distance_orthogonal_is_one() {
        let a = unit_vec(8, 0);
        let b = unit_vec(8, 1);
        assert!((cosine_distance(&a, &b) - 1.0).abs() < 1e-6);
    }

    async fn setup_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn insert_photo(pool: &sqlx::SqlitePool, id: i64) {
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) \
             VALUES (?, ?, ?, 'jpeg', 'imported')"
        )
        .bind(id).bind(format!("/p{id}")).bind(format!("sha{id}"))
        .execute(pool).await.unwrap();
    }

    async fn insert_face_with_emb(pool: &sqlx::SqlitePool, photo_id: i64, emb: &[f32]) -> i64 {
        use crate::face::embedder::encode_embedding;
        sqlx::query_scalar(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence, embedding) \
             VALUES (?, 0, 0, 50, 50, 0.9, ?) RETURNING id"
        )
        .bind(photo_id).bind(encode_embedding(emb).as_slice())
        .fetch_one(pool).await.unwrap()
    }

    async fn assign_face(pool: &sqlx::SqlitePool, person_id: i64, face_id: i64) {
        sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
            .bind(person_id).bind(face_id)
            .execute(pool).await.unwrap();
    }

    async fn create_person(pool: &sqlx::SqlitePool, cover_face_id: i64) -> i64 {
        sqlx::query_scalar("INSERT INTO people (cover_face_id) VALUES (?) RETURNING id")
            .bind(cover_face_id)
            .fetch_one(pool).await.unwrap()
    }

    #[tokio::test]
    async fn incremental_assigns_new_face_to_existing_person() {
        let pool = setup_pool().await;
        insert_photo(&pool, 1).await;
        insert_photo(&pool, 2).await;

        let emb = unit_vec(512, 0);
        // Two faces already clustered into a person
        let f1 = insert_face_with_emb(&pool, 1, &emb).await;
        let f2 = insert_face_with_emb(&pool, 1, &emb).await;
        let person = create_person(&pool, f1).await;
        assign_face(&pool, person, f1).await;
        assign_face(&pool, person, f2).await;

        // A new, unassigned face with the same embedding
        let f3 = insert_face_with_emb(&pool, 2, &emb).await;

        let new_people = run_incremental_clustering(&pool).await.unwrap();
        assert_eq!(new_people, 0, "no new person should be created");

        let people_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(people_count, 1, "still only one person");

        let assigned: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM person_faces WHERE face_id = ?")
                .bind(f3).fetch_one(&pool).await.unwrap();
        assert_eq!(assigned, 1, "f3 should now be assigned to the existing person");
    }

    #[tokio::test]
    async fn incremental_creates_new_person_for_distant_face() {
        let pool = setup_pool().await;
        insert_photo(&pool, 1).await;
        insert_photo(&pool, 2).await;

        let emb_a = unit_vec(512, 0);
        let emb_b = unit_vec(512, 255); // orthogonal → cosine dist = 1.0

        let f1 = insert_face_with_emb(&pool, 1, &emb_a).await;
        let person = create_person(&pool, f1).await;
        assign_face(&pool, person, f1).await;

        // New face very different from existing person
        let _f2 = insert_face_with_emb(&pool, 2, &emb_b).await;

        let new_people = run_incremental_clustering(&pool).await.unwrap();
        assert_eq!(new_people, 1, "one new person should be created");

        let people_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(people_count, 2);
    }

    #[tokio::test]
    async fn incremental_noop_when_no_unassigned_faces() {
        let pool = setup_pool().await;
        insert_photo(&pool, 1).await;
        let f1 = insert_face_with_emb(&pool, 1, &unit_vec(512, 0)).await;
        let p = create_person(&pool, f1).await;
        assign_face(&pool, p, f1).await;

        let new_people = run_incremental_clustering(&pool).await.unwrap();
        assert_eq!(new_people, 0);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1, "no change");
    }
}
