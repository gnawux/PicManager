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
        let person_id: i64 =
            sqlx::query_scalar("INSERT INTO people DEFAULT VALUES RETURNING id")
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
}
