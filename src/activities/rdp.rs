/// Ramer-Douglas-Peucker simplification.
/// Returns indices into `points` that should be kept.
/// `epsilon` is the perpendicular distance threshold in the same units as coords.
pub fn simplify(points: &[(f64, f64)], epsilon: f64) -> Vec<usize> {
    if points.len() <= 2 {
        return (0..points.len()).collect();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    rdp_recursive(points, 0, points.len() - 1, epsilon, &mut keep);
    keep.iter().enumerate().filter(|(_, k)| **k).map(|(i, _)| i).collect()
}

fn rdp_recursive(points: &[(f64, f64)], start: usize, end: usize, epsilon: f64, keep: &mut Vec<bool>) {
    if end <= start + 1 {
        return;
    }
    let (x1, y1) = points[start];
    let (x2, y2) = points[end];
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len2 = dx * dx + dy * dy;

    let mut max_dist = 0.0f64;
    let mut max_idx = start + 1;

    for i in (start + 1)..end {
        let (px, py) = points[i];
        let dist = if len2 == 0.0 {
            ((px - x1).powi(2) + (py - y1).powi(2)).sqrt()
        } else {
            let t = ((px - x1) * dx + (py - y1) * dy) / len2;
            let t = t.clamp(0.0, 1.0);
            ((px - x1 - t * dx).powi(2) + (py - y1 - t * dy).powi(2)).sqrt()
        };
        if dist > max_dist {
            max_dist = dist;
            max_idx = i;
        }
    }

    if max_dist > epsilon {
        keep[max_idx] = true;
        rdp_recursive(points, start, max_idx, epsilon, keep);
        rdp_recursive(points, max_idx, end, epsilon, keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collinear_points_reduce_to_endpoints() {
        let pts: Vec<(f64, f64)> = (0..10).map(|i| (i as f64, i as f64)).collect();
        let kept = simplify(&pts, 0.01);
        assert_eq!(kept, vec![0, 9]);
    }

    #[test]
    fn single_peak_retained() {
        let pts = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)];
        let kept = simplify(&pts, 0.01);
        assert_eq!(kept, vec![0, 1, 2]);
    }

    #[test]
    fn two_points_unchanged() {
        let pts = vec![(0.0, 0.0), (1.0, 1.0)];
        let kept = simplify(&pts, 1.0);
        assert_eq!(kept, vec![0, 1]);
    }
}
