/// Project a list of (id, 512-D embedding) pairs onto a 2-D plane via PCA.
///
/// Uses power iteration to find the top-2 principal components.
/// Returns `(id, x, y)` with coordinates normalised to `[−1, 1]`.
/// Returns an empty Vec when fewer than 2 distinct points are supplied.
pub fn pca_2d(embeddings: &[(i64, Vec<f32>)]) -> Vec<(i64, f32, f32)> {
    let n = embeddings.len();
    if n < 2 {
        return vec![];
    }
    let dim = embeddings[0].1.len();
    if dim == 0 {
        return vec![];
    }

    // ── 1. centre ────────────────────────────────────────────────────────────
    let mut mean = vec![0f64; dim];
    for (_, v) in embeddings {
        for (m, x) in mean.iter_mut().zip(v.iter()) {
            *m += *x as f64;
        }
    }
    let inv_n = 1.0 / n as f64;
    for m in &mut mean {
        *m *= inv_n;
    }

    let centered: Vec<Vec<f64>> = embeddings
        .iter()
        .map(|(_, v)| v.iter().zip(&mean).map(|(x, m)| *x as f64 - m).collect())
        .collect();

    // ── 2. power iteration for one principal component ────────────────────────
    // Computes the leading eigenvector of X^T X using the matrix-vector product
    // (X^T X) v  =  X^T (X v),  which avoids forming the dim×dim matrix.
    fn power_iter(centered: &[Vec<f64>], init: &[f64], iters: usize) -> Vec<f64> {
        let dim = init.len();
        let mut v: Vec<f64> = init.to_vec();
        for _ in 0..iters {
            // w = X v  (n-vector)
            let w: Vec<f64> = centered.iter().map(|row| dot(row, &v)).collect();
            // v_new = X^T w  (dim-vector)
            let mut v_new = vec![0f64; dim];
            for (row, &wi) in centered.iter().zip(&w) {
                for (vn, &ri) in v_new.iter_mut().zip(row.iter()) {
                    *vn += wi * ri;
                }
            }
            let norm = dot(&v_new, &v_new).sqrt();
            if norm < 1e-12 {
                break;
            }
            v = v_new.iter().map(|x| x / norm).collect();
        }
        v
    }

    // ── 3. deterministic seed vectors ─────────────────────────────────────────
    // Use the first centred data row as the PC1 seed (avoids rand dependency,
    // deterministic given the same input, works as long as the first row is not
    // the zero vector – extremely unlikely for real embeddings).
    let seed1 = {
        let norm = dot(&centered[0], &centered[0]).sqrt();
        if norm < 1e-12 {
            let mut s = vec![0f64; dim];
            s[0] = 1.0;
            s
        } else {
            centered[0].iter().map(|x| x / norm).collect()
        }
    };

    let pc1 = power_iter(&centered, &seed1, 200);

    // ── 4. deflate: remove PC1 component from each row ───────────────────────
    let deflated: Vec<Vec<f64>> = centered
        .iter()
        .map(|row| {
            let proj = dot(row, &pc1);
            row.iter().zip(&pc1).map(|(r, p)| r - proj * p).collect()
        })
        .collect();

    // PC2 seed: first deflated row that is not near-zero
    let seed2 = {
        let mut s = None;
        for row in &deflated {
            let norm = dot(row, row).sqrt();
            if norm > 1e-12 {
                s = Some(row.iter().map(|x| x / norm).collect::<Vec<_>>());
                break;
            }
        }
        s.unwrap_or_else(|| {
            // fallback: unit vector orthogonal to pc1
            let mut v = vec![0f64; dim];
            // pick dimension with smallest |pc1| component
            let idx = pc1
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(1);
            v[idx] = 1.0;
            // Gram-Schmidt orthogonalise against pc1
            let p = dot(&v, &pc1);
            let mut v: Vec<f64> = v.iter().zip(&pc1).map(|(vi, pi)| vi - p * pi).collect();
            let norm = dot(&v, &v).sqrt();
            if norm > 1e-12 {
                for x in &mut v {
                    *x /= norm;
                }
            }
            v
        })
    };

    let pc2 = power_iter(&deflated, &seed2, 200);

    // ── 5. project ────────────────────────────────────────────────────────────
    let coords: Vec<(i64, f64, f64)> = embeddings
        .iter()
        .zip(&centered)
        .map(|((id, _), row)| (*id, dot(row, &pc1), dot(row, &pc2)))
        .collect();

    // ── 6. normalise to [−1, 1] ───────────────────────────────────────────────
    let max_abs = coords
        .iter()
        .flat_map(|(_, x, y)| [x.abs(), y.abs()])
        .fold(0f64, f64::max);

    if max_abs < 1e-12 {
        // All points project to the same location.
        return embeddings.iter().map(|(id, _)| (*id, 0f32, 0f32)).collect();
    }

    coords
        .into_iter()
        .map(|(id, x, y)| (id, (x / max_abs) as f32, (y / max_abs) as f32))
        .collect()
}

#[inline]
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_embedding(base: &[f32], dim: usize) -> Vec<f32> {
        let mut v = base.to_vec();
        v.resize(dim, 0.0);
        // L2-normalise so the vectors resemble real arcface output
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            v.iter_mut().for_each(|x| *x /= norm);
        }
        v
    }

    #[test]
    fn pca_two_separated_clusters() {
        // Group A: large positive first component; Group B: large negative first component.
        // After PCA the two groups should separate clearly along PC1.
        let dim = 512usize;
        let mut embs: Vec<(i64, Vec<f32>)> = Vec::new();
        for i in 0..10i64 {
            let mut base = vec![0.0f32; dim];
            base[0] = 10.0 + i as f32 * 0.1;
            embs.push((i, make_embedding(&base, dim)));
        }
        for i in 10..20i64 {
            let mut base = vec![0.0f32; dim];
            base[0] = -10.0 - (i - 10) as f32 * 0.1;
            embs.push((i, make_embedding(&base, dim)));
        }

        let result = pca_2d(&embs);
        assert_eq!(result.len(), 20);

        // All x-coords for group A should have the same sign.
        let sign_a = result[0].1.signum();
        for &(_, x, _) in &result[..10] {
            assert!(
                x.signum() == sign_a || x.abs() < 1e-4,
                "group A point has wrong sign: {x}"
            );
        }
        // Group B should have the opposite sign.
        for &(_, x, _) in &result[10..] {
            assert!(
                x.signum() != sign_a || x.abs() < 1e-4,
                "group B point has wrong sign: {x}"
            );
        }
    }

    #[test]
    fn pca_output_in_range() {
        let dim = 512usize;
        let mut embs: Vec<(i64, Vec<f32>)> = Vec::new();
        for i in 0..30i64 {
            let mut base = vec![0.0f32; dim];
            base[(i as usize) % dim] = 1.0 + i as f32 * 0.3;
            base[((i * 7 + 3) as usize) % dim] = -0.5 - i as f32 * 0.1;
            embs.push((i, make_embedding(&base, dim)));
        }
        let result = pca_2d(&embs);
        for &(_, x, y) in &result {
            assert!(x >= -1.001 && x <= 1.001, "x out of range: {x}");
            assert!(y >= -1.001 && y <= 1.001, "y out of range: {y}");
        }
    }

    #[test]
    fn pca_deterministic() {
        let dim = 512usize;
        let embs: Vec<(i64, Vec<f32>)> = (0..20i64)
            .map(|i| {
                let mut base = vec![0.0f32; dim];
                base[(i as usize * 13) % dim] = 1.0 + i as f32;
                (i, make_embedding(&base, dim))
            })
            .collect();

        let r1 = pca_2d(&embs);
        let r2 = pca_2d(&embs);
        for ((_, x1, y1), (_, x2, y2)) in r1.iter().zip(&r2) {
            assert!((x1 - x2).abs() < 1e-6, "x not deterministic");
            assert!((y1 - y2).abs() < 1e-6, "y not deterministic");
        }
    }

    #[test]
    fn pca_single_point_returns_empty() {
        let embs = vec![(1i64, vec![0.1f32; 512])];
        assert!(pca_2d(&embs).is_empty());
    }

    #[test]
    fn pca_zero_points_returns_empty() {
        let embs: Vec<(i64, Vec<f32>)> = vec![];
        assert!(pca_2d(&embs).is_empty());
    }

    #[test]
    fn pca_all_identical_returns_same_location() {
        // If all points are identical the projection is at (0,0).
        let v = vec![0.5f32; 512];
        let embs: Vec<(i64, Vec<f32>)> = (0..5i64).map(|i| (i, v.clone())).collect();
        let result = pca_2d(&embs);
        // Should return points (may not be empty since n>=2), all at (0,0).
        for &(_, x, y) in &result {
            assert!(x.abs() < 1e-5 && y.abs() < 1e-5, "expected (0,0) got ({x},{y})");
        }
    }
}
