use anyhow::Result;
use sqlx::SqlitePool;

use super::embedder::decode_embedding;

/// DBSCAN epsilon (cosine distance threshold).
/// Tighter than the intuitive 0.4 to avoid DBSCAN chaining through bridge embeddings.
pub const EPS: f32 = 0.35;

/// Minimum face detector confidence for a face to participate in the DBSCAN core pass.
/// Low-confidence detections (partial faces, blurry, tiny) often have noisy embeddings
/// that act as bridges between otherwise-distinct clusters, inflating one giant cluster.
/// These faces are post-assigned to the nearest DBSCAN cluster after the main pass.
pub const MIN_CONFIDENCE: f32 = 0.70;

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

pub(crate) fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    1.0 - dot
}

// Pick the face_id with the highest confidence from a set, using preloaded conf map.
fn pick_cover(group: &[i64], conf_map: &std::collections::HashMap<i64, f32>) -> i64 {
    group
        .iter()
        .copied()
        .max_by(|&a, &b| {
            let ca = conf_map.get(&a).copied().unwrap_or(0.0);
            let cb = conf_map.get(&b).copied().unwrap_or(0.0);
            ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(group[0])
}

/// Full rebuild: clear all people/person_faces, re-cluster from scratch.
///
/// Two-phase algorithm to avoid DBSCAN chaining:
///
/// **Phase 1 — DBSCAN core (high-confidence faces only)**
///   Faces with `confidence >= MIN_CONFIDENCE` (0.70) form the DBSCAN input.
///   Low-confidence detections (partial/blurry/tiny faces) have noisy embeddings that
///   act as chain-links between distinct identities; excluding them keeps clusters tight.
///   Noise points from DBSCAN each become their own person record.
///
/// **Phase 2 — post-assign low-confidence faces**
///   For each low-confidence face, compute the minimum cosine distance to every face
///   in every existing person. If the nearest person is within EPS (0.35), assign there;
///   otherwise create an individual person record (same as a DBSCAN noise point).
///
/// Returns the number of DBSCAN clusters created (not counting post-assigned individuals).
pub async fn run_clustering(pool: &SqlitePool) -> Result<usize> {
    // ── 1. Load all faces with embeddings and confidence ──────────────────────
    let rows: Vec<(i64, Vec<u8>, f32)> = sqlx::query_as(
        "SELECT id, embedding, COALESCE(confidence, 0.0) FROM faces WHERE embedding IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let all_faces: Vec<(i64, Vec<f32>, f32)> = rows
        .into_iter()
        .filter_map(|(id, blob, conf)| {
            let emb = decode_embedding(&blob);
            if emb.is_empty() { None } else { Some((id, emb, conf)) }
        })
        .collect();

    let conf_map: std::collections::HashMap<i64, f32> =
        all_faces.iter().map(|(id, _, c)| (*id, *c)).collect();
    let emb_map: std::collections::HashMap<i64, Vec<f32>> =
        all_faces.iter().map(|(id, e, _)| (*id, e.clone())).collect();

    // ── 2. Split by confidence ─────────────────────────────────────────────────
    let mut high_faces: Vec<(i64, Vec<f32>)> = Vec::new();
    let mut low_faces: Vec<(i64, Vec<f32>)> = Vec::new();
    for (id, emb, conf) in &all_faces {
        if *conf >= MIN_CONFIDENCE {
            high_faces.push((*id, emb.clone()));
        } else {
            low_faces.push((*id, emb.clone()));
        }
    }

    // ── 3. Rebuild from scratch ───────────────────────────────────────────────
    sqlx::query("DELETE FROM person_faces").execute(pool).await?;
    sqlx::query("DELETE FROM people").execute(pool).await?;

    // ── 4. Phase 1: DBSCAN on high-confidence faces ───────────────────────────
    let clusters = cluster_faces(&high_faces, EPS, 2);
    let cluster_count = clusters.len();

    // person_id → Vec<(face_id, embedding)> used for phase-2 nearest-neighbour
    let mut person_map: std::collections::HashMap<i64, Vec<(i64, Vec<f32>)>> =
        std::collections::HashMap::new();

    for group in clusters {
        let cover = pick_cover(&group, &conf_map);
        let pid: i64 =
            sqlx::query_scalar("INSERT INTO people (cover_face_id) VALUES (?) RETURNING id")
                .bind(cover)
                .fetch_one(pool)
                .await?;

        let mut pfaces: Vec<(i64, Vec<f32>)> = Vec::new();
        for fid in group {
            sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
                .bind(pid)
                .bind(fid)
                .execute(pool)
                .await?;
            if let Some(emb) = emb_map.get(&fid) {
                pfaces.push((fid, emb.clone()));
            }
        }
        person_map.insert(pid, pfaces);
    }

    // ── 5. Phase 2: post-assign low-confidence faces ──────────────────────────
    for (face_id, emb) in low_faces {
        let best = person_map
            .iter()
            .filter_map(|(&pid, pfaces)| {
                pfaces
                    .iter()
                    .map(|(_, pe)| cosine_distance(&emb, pe))
                    .reduce(f32::min)
                    .map(|d| (d, pid))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        if let Some((dist, pid)) = best {
            if dist < EPS {
                sqlx::query(
                    "INSERT OR IGNORE INTO person_faces (person_id, face_id) VALUES (?, ?)",
                )
                .bind(pid)
                .bind(face_id)
                .execute(pool)
                .await?;
                person_map.entry(pid).or_default().push((face_id, emb));
                continue;
            }
        }

        // Not close enough to any existing person: individual noise person
        let pid: i64 =
            sqlx::query_scalar("INSERT INTO people (cover_face_id) VALUES (?) RETURNING id")
                .bind(face_id)
                .fetch_one(pool)
                .await?;
        sqlx::query("INSERT INTO person_faces (person_id, face_id) VALUES (?, ?)")
            .bind(pid)
            .bind(face_id)
            .execute(pool)
            .await?;
        person_map.insert(pid, vec![(face_id, emb)]);
    }

    Ok(cluster_count)
}

/// Assign unassigned faces to existing people or form new person clusters,
/// without touching any existing `people` / `person_faces` rows.
///
/// Algorithm:
/// 1. Find faces with non-NULL embedding that have no `person_faces` entry.
/// 2. For each, find the nearest existing person (minimum cosine distance over
///    all that person's faces); assign if distance < EPS (0.35).
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

    let mut still_unassigned: Vec<(i64, Vec<f32>)> = Vec::new();

    for (face_id, emb) in unassigned {
        let best = person_map
            .iter()
            .filter_map(|(&pid, pfaces)| {
                pfaces
                    .iter()
                    .map(|(_, pe)| cosine_distance(&emb, pe))
                    .reduce(f32::min)
                    .map(|d| (d, pid))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

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

    // Preload confidence for still_unassigned faces (for cover selection)
    let conf_map: std::collections::HashMap<i64, f32> = {
        let ids: Vec<i64> = still_unassigned.iter().map(|(id, _)| *id).collect();
        if ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT id, COALESCE(confidence, 0.0) FROM faces WHERE id IN ({placeholders})"
            );
            let mut q = sqlx::query_as::<_, (i64, f32)>(&sql);
            for id in &ids {
                q = q.bind(id);
            }
            q.fetch_all(pool)
                .await?
                .into_iter()
                .collect()
        }
    };

    for group in clusters {
        let cover = pick_cover(&group, &conf_map);
        let person_id: i64 =
            sqlx::query_scalar("INSERT INTO people (cover_face_id) VALUES (?) RETURNING id")
                .bind(cover)
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
        let clusters = cluster_faces(&faces, EPS, 2);
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
        let clusters = cluster_faces(&faces, EPS, 2);
        assert_eq!(clusters.len(), 3);
        for c in &clusters {
            assert_eq!(c.len(), 2);
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(cluster_faces(&[], EPS, 2).is_empty());
    }

    #[test]
    fn single_point_is_noise_but_still_returned() {
        let faces = vec![(42i64, unit_vec(4, 0))];
        let clusters = cluster_faces(&faces, EPS, 2);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0], vec![42]);
    }

    #[test]
    fn cosine_distance_identical_is_zero() {
        let v = unit_vec(8, 2);
        assert!((cosine_distance(&v, &v) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_distance_orthogonal_is_one() {
        let a = unit_vec(8, 0);
        let b = unit_vec(8, 1);
        assert!((cosine_distance(&a, &b) - 1.0).abs() < 1e-6);
    }

    // ── DB-level tests ────────────────────────────────────────────────────────

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

    async fn insert_face(
        pool: &sqlx::SqlitePool,
        photo_id: i64,
        emb: &[f32],
        confidence: f32,
    ) -> i64 {
        use crate::face::embedder::encode_embedding;
        sqlx::query_scalar(
            "INSERT INTO faces (photo_id, x, y, width, height, confidence, embedding) \
             VALUES (?, 0, 0, 50, 50, ?, ?) RETURNING id"
        )
        .bind(photo_id)
        .bind(confidence)
        .bind(encode_embedding(emb).as_slice())
        .fetch_one(pool)
        .await
        .unwrap()
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
    async fn run_clustering_groups_similar_faces() {
        use crate::face::embedder::encode_embedding;

        let pool = setup_pool().await;

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

        let min_faces: i64 = sqlx::query_scalar(
            "SELECT MIN(cnt) FROM (SELECT COUNT(*) as cnt FROM person_faces GROUP BY person_id)"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(min_faces, 2);
    }

    /// Low-confidence bridge face must NOT merge two distant high-conf clusters.
    ///
    /// Topology (cosine distances, 512D unit vectors):
    ///   emb_a ←→ emb_bridge: dist ≈ 0.30  (within EPS)
    ///   emb_bridge ←→ emb_b: dist ≈ 0.30  (within EPS)
    ///   emb_a ←→ emb_b: dist = 1.0         (far apart — orthogonal)
    ///
    /// With old eps=0.4 and no confidence filter, DBSCAN would chain A→bridge→B
    /// into one cluster.  With confidence filter, bridge is excluded from DBSCAN
    /// and both A+A_pair and B+B_pair form separate clusters.
    #[tokio::test]
    async fn low_conf_bridge_does_not_merge_distant_clusters() {
        let pool = setup_pool().await;
        for pid in [1i64, 2, 3] { insert_photo(&pool, pid).await; }

        // Two clusters: hot=0 and hot=255 (orthogonal → dist=1.0)
        let emb_a = unit_vec(512, 0);
        let emb_b = unit_vec(512, 255);

        // Bridge: diagonal between a and b. cos(45°) = 1/√2, so dist = 1 - 1/√2 ≈ 0.293
        // We construct it by summing and L2-normalising.
        let mut emb_bridge_raw = vec![0.0f32; 512];
        emb_bridge_raw[0] = 1.0;
        emb_bridge_raw[255] = 1.0;
        let norm: f32 = emb_bridge_raw.iter().map(|x| x * x).sum::<f32>().sqrt();
        let emb_bridge: Vec<f32> = emb_bridge_raw.iter().map(|x| x / norm).collect();

        // dist(emb_a, emb_bridge) ≈ 0.293, dist(emb_b, emb_bridge) ≈ 0.293 — both < EPS(0.35)
        // dist(emb_a, emb_b) = 1.0 — far apart
        let d_ab = cosine_distance(&emb_a, &emb_b);
        let d_a_br = cosine_distance(&emb_a, &emb_bridge);
        assert!(d_ab > EPS, "a and b must be far apart");
        assert!(d_a_br < EPS, "bridge must be within EPS of a");

        // High-conf faces: 2 of cluster A, 2 of cluster B
        insert_face(&pool, 1, &emb_a, 0.95).await;
        insert_face(&pool, 1, &emb_a, 0.92).await;
        insert_face(&pool, 2, &emb_b, 0.95).await;
        insert_face(&pool, 2, &emb_b, 0.90).await;
        // Low-conf bridge face (would chain A+B under old algorithm)
        insert_face(&pool, 3, &emb_bridge, 0.55).await;

        let cluster_count = run_clustering(&pool).await.unwrap();
        assert_eq!(cluster_count, 2, "exactly 2 DBSCAN clusters (bridge excluded from core)");

        let people: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people")
            .fetch_one(&pool).await.unwrap();
        // bridge is within EPS of cluster A (dist≈0.293), so it gets post-assigned there.
        // Total: 2 people (not 3 — the bridge doesn't become its own person).
        assert_eq!(people, 2, "bridge post-assigned to nearest cluster, not separated");

        // Cluster A has 3 faces (2 high-conf + bridge), cluster B has 2.
        // Critical: clusters A and B are NOT merged (no person has all 5 faces).
        let max_faces: i64 = sqlx::query_scalar(
            "SELECT MAX(cnt) FROM (SELECT COUNT(*) as cnt FROM person_faces GROUP BY person_id)"
        ).fetch_one(&pool).await.unwrap();
        assert!(max_faces < 5, "A and B must remain separate clusters");

        let min_faces: i64 = sqlx::query_scalar(
            "SELECT MIN(cnt) FROM (SELECT COUNT(*) as cnt FROM person_faces GROUP BY person_id)"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(min_faces, 2, "cluster B untouched");
    }

    /// Low-confidence face close to an existing person gets post-assigned to it.
    #[tokio::test]
    async fn low_conf_face_post_assigned_to_nearest_person() {
        let pool = setup_pool().await;
        for pid in [1i64, 2] { insert_photo(&pool, pid).await; }

        let emb_a = unit_vec(512, 0);
        // emb_near is very close to emb_a (dist ≈ 0.0 since both point in direction 0)
        let emb_near = unit_vec(512, 0);

        insert_face(&pool, 1, &emb_a, 0.95).await;
        insert_face(&pool, 1, &emb_a, 0.90).await;
        // Low-conf face with same direction — should be assigned to the A cluster
        insert_face(&pool, 2, &emb_near, 0.50).await;

        let cluster_count = run_clustering(&pool).await.unwrap();
        assert_eq!(cluster_count, 1, "one DBSCAN cluster");

        let people: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(people, 1, "low-conf face merged into existing person");

        let face_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM person_faces"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(face_count, 3, "all 3 faces assigned");
    }

    /// Low-confidence face far from all persons becomes its own person record.
    #[tokio::test]
    async fn low_conf_isolated_face_gets_individual_person() {
        let pool = setup_pool().await;
        for pid in [1i64, 2] { insert_photo(&pool, pid).await; }

        let emb_a = unit_vec(512, 0);
        let emb_far = unit_vec(512, 255); // orthogonal to emb_a, dist = 1.0

        insert_face(&pool, 1, &emb_a, 0.95).await;
        insert_face(&pool, 1, &emb_a, 0.90).await;
        insert_face(&pool, 2, &emb_far, 0.50).await;

        run_clustering(&pool).await.unwrap();

        let people: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(people, 2, "isolated low-conf face gets its own person");

        // Confirm every face is assigned exactly once
        let total_assigned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM person_faces")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(total_assigned, 3);
    }

    #[tokio::test]
    async fn incremental_assigns_new_face_to_existing_person() {
        let pool = setup_pool().await;
        insert_photo(&pool, 1).await;
        insert_photo(&pool, 2).await;

        let emb = unit_vec(512, 0);
        let f1 = insert_face(&pool, 1, &emb, 0.9).await;
        let f2 = insert_face(&pool, 1, &emb, 0.9).await;
        let person = create_person(&pool, f1).await;
        assign_face(&pool, person, f1).await;
        assign_face(&pool, person, f2).await;

        let f3 = insert_face(&pool, 2, &emb, 0.9).await;

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

        let f1 = insert_face(&pool, 1, &emb_a, 0.9).await;
        let person = create_person(&pool, f1).await;
        assign_face(&pool, person, f1).await;

        let _f2 = insert_face(&pool, 2, &emb_b, 0.9).await;

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
        let f1 = insert_face(&pool, 1, &unit_vec(512, 0), 0.9).await;
        let p = create_person(&pool, f1).await;
        assign_face(&pool, p, f1).await;

        let new_people = run_incremental_clustering(&pool).await.unwrap();
        assert_eq!(new_people, 0);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM people")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1, "no change");
    }
}
