use image::DynamicImage;
use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct FaceRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub confidence: f32,
}

// ── public entry point ───────────────────────────────────────────────────────

/// Detect faces in `img`.  Returns an empty Vec (no panic) when the model is
/// unavailable or inference fails.
pub fn detect(img: &DynamicImage) -> Vec<FaceRegion> {
    if img.width() < 10 || img.height() < 10 {
        return vec![];
    }
    let Some(mtx) = get_session() else {
        return vec![];
    };
    let mut session = mtx.lock().unwrap();
    run_inference(&mut session, img).unwrap_or_else(|e| {
        tracing::warn!("face detection failed: {e}");
        vec![]
    })
}

// ── model session (lazy, process-wide) ──────────────────────────────────────

static SESSION: OnceLock<Option<Mutex<Session>>> = OnceLock::new();

fn model_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("picmanager")
        .join("models")
        .join("face_detector.onnx")
}

fn get_session() -> Option<&'static Mutex<Session>> {
    SESSION
        .get_or_init(|| {
            // Prefer bytes compiled into the binary (via `models bundle` + rebuild).
            if let Some(bytes) = crate::get_embedded_model("face_detector.onnx") {
                match Session::builder().and_then(|mut b| b.commit_from_memory(&bytes)) {
                    Ok(s) => return Some(Mutex::new(s)),
                    Err(e) => tracing::warn!("embedded face detection model failed: {e}"),
                }
            }
            // In test builds skip disk loading — ONNX runtime init can hang in CI.
            // Tests requiring real model inference use #[ignore] explicitly.
            if cfg!(test) {
                return None;
            }
            // Fall back to the on-disk model in the config directory.
            let path = model_path();
            if !path.exists() {
                tracing::warn!(
                    "face detection model not found at {}; skipping detection",
                    path.display()
                );
                return None;
            }
            match Session::builder().and_then(|mut b| b.commit_from_file(&path)) {
                Ok(s) => Some(Mutex::new(s)),
                Err(e) => {
                    tracing::warn!("failed to load face detection model: {e}");
                    None
                }
            }
        })
        .as_ref()
}

// ── inference ────────────────────────────────────────────────────────────────

fn run_inference(
    session: &mut Session,
    img: &DynamicImage,
) -> Result<Vec<FaceRegion>, Box<dyn std::error::Error>> {
    let (orig_w, orig_h) = (img.width() as f32, img.height() as f32);
    let input = preprocess(img);
    let tensor = TensorRef::from_array_view(&input)?;

    let outputs = session.run(ort::inputs!["input" => tensor])?;

    // ultraface-slim-320 outputs: "scores" [1,4420,2], "boxes" [1,4420,4]
    let (_ss, scores_flat) = outputs["scores"].try_extract_tensor::<f32>()?;
    let (_sb, boxes_flat) = outputs["boxes"].try_extract_tensor::<f32>()?;
    let n = 4420usize;

    let mut candidates: Vec<(FaceRegion, f32)> = (0..n)
        .filter_map(|i| {
            let conf = scores_flat[i * 2 + 1];
            if conf < 0.5 {
                return None;
            }
            let x1 = (boxes_flat[i * 4] * orig_w) as i32;
            let y1 = (boxes_flat[i * 4 + 1] * orig_h) as i32;
            let x2 = (boxes_flat[i * 4 + 2] * orig_w) as i32;
            let y2 = (boxes_flat[i * 4 + 3] * orig_h) as i32;
            Some((
                FaceRegion { x: x1, y: y1, width: (x2 - x1).max(1), height: (y2 - y1).max(1), confidence: conf },
                conf,
            ))
        })
        .collect();

    let kept = nms(&candidates, 0.45);
    let mut result: Vec<FaceRegion> = kept.into_iter().map(|i| candidates.remove(i).0).collect();
    result.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    Ok(result)
}

// ── pure helpers (independently testable) ───────────────────────────────────

/// Resize to 320×240, convert to BGR float32 `[1, 3, H, W]`, normalise `(px-127)/128`.
pub(crate) fn preprocess(img: &DynamicImage) -> Array4<f32> {
    let resized = img.resize_exact(320, 240, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let (w, h) = (320usize, 240usize);
    let mut arr = Array4::<f32>::zeros([1, 3, h, w]);
    for y in 0..h {
        for x in 0..w {
            let p = rgb.get_pixel(x as u32, y as u32);
            // ultraface expects BGR channel order
            arr[[0, 0, y, x]] = (p[2] as f32 - 127.0) / 128.0; // B
            arr[[0, 1, y, x]] = (p[1] as f32 - 127.0) / 128.0; // G
            arr[[0, 2, y, x]] = (p[0] as f32 - 127.0) / 128.0; // R
        }
    }
    arr
}

/// Intersection-over-union for two axis-aligned boxes.
pub(crate) fn iou(a: &FaceRegion, b: &FaceRegion) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;

    let ix1 = a.x.max(b.x);
    let iy1 = a.y.max(b.y);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);

    if ix2 <= ix1 || iy2 <= iy1 {
        return 0.0;
    }
    let inter = ((ix2 - ix1) * (iy2 - iy1)) as f32;
    let union = (a.width * a.height + b.width * b.height) as f32 - inter;
    inter / union
}

/// Greedy NMS; returns indices of kept boxes (highest-confidence first).
pub(crate) fn nms(candidates: &[(FaceRegion, f32)], iou_thresh: f32) -> Vec<usize> {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|&a, &b| {
        candidates[b].1.partial_cmp(&candidates[a].1).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut suppressed = vec![false; candidates.len()];
    let mut keep = Vec::new();
    for &i in &order {
        if suppressed[i] {
            continue;
        }
        keep.push(i);
        for &j in &order {
            if !suppressed[j] && j != i && iou(&candidates[i].0, &candidates[j].0) > iou_thresh {
                suppressed[j] = true;
            }
        }
    }
    keep
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32, conf: f32) -> FaceRegion {
        FaceRegion { x, y, width: w, height: h, confidence: conf }
    }

    // ── FaceRegion ────────────────────────────────────────────────────────

    #[test]
    fn face_region_fields() {
        let region = r(10, 20, 100, 120, 0.95);
        assert_eq!(region.x, 10);
        assert_eq!(region.y, 20);
        assert_eq!(region.width, 100);
        assert_eq!(region.height, 120);
        assert!((region.confidence - 0.95).abs() < f32::EPSILON);
    }

    // ── iou ──────────────────────────────────────────────────────────────

    #[test]
    fn iou_no_overlap() {
        assert!((iou(&r(0, 0, 10, 10, 1.0), &r(20, 20, 10, 10, 1.0)) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn iou_identical() {
        assert!((iou(&r(0, 0, 10, 10, 1.0), &r(0, 0, 10, 10, 1.0)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_half_overlap() {
        // a=[0,10)×[0,10), b=[5,15)×[0,10) → inter=50, union=150
        let v = iou(&r(0, 0, 10, 10, 1.0), &r(5, 0, 10, 10, 1.0));
        assert!((v - 1.0 / 3.0).abs() < 1e-5, "got {v}");
    }

    #[test]
    fn iou_touching_edges_no_overlap() {
        assert!((iou(&r(0, 0, 10, 10, 1.0), &r(10, 0, 10, 10, 1.0)) - 0.0).abs() < 1e-6);
    }

    // ── nms ──────────────────────────────────────────────────────────────

    #[test]
    fn nms_keeps_all_non_overlapping() {
        let c = vec![
            (r(0, 0, 10, 10, 0.9), 0.9f32),
            (r(50, 50, 10, 10, 0.8), 0.8),
            (r(100, 100, 10, 10, 0.7), 0.7),
        ];
        assert_eq!(nms(&c, 0.45).len(), 3);
    }

    #[test]
    fn nms_suppresses_overlapping_lower_confidence() {
        let c = vec![
            (r(0, 0, 100, 100, 0.9), 0.9f32),
            (r(5, 5, 100, 100, 0.6), 0.6), // large overlap with first
            (r(200, 200, 100, 100, 0.8), 0.8), // no overlap
        ];
        let kept = nms(&c, 0.45);
        assert_eq!(kept.len(), 2);
        assert!(kept.contains(&0));
        assert!(kept.contains(&2));
        assert!(!kept.contains(&1));
    }

    #[test]
    fn nms_empty_input() {
        assert!(nms(&[], 0.45).is_empty());
    }

    // ── detect ───────────────────────────────────────────────────────────

    #[test]
    fn detect_tiny_image_returns_empty() {
        let img = DynamicImage::new_rgb8(4, 4);
        assert!(detect(&img).is_empty());
    }

    #[test]
    #[ignore = "requires face_detector.onnx in config_dir/picmanager/models/"]
    fn detect_faces_in_sample_jpeg() {
        let img = image::open("tests/samples/IMG_20250204_135549.jpg").unwrap();
        let faces = detect(&img);
        assert!(!faces.is_empty(), "expected at least one face");
        assert!(faces[0].confidence >= 0.5);
        assert!(faces[0].width > 0 && faces[0].height > 0);
    }

    #[test]
    #[ignore = "requires face_detector.onnx in config_dir/picmanager/models/"]
    fn detect_blank_image_returns_empty() {
        let img = DynamicImage::new_rgb8(640, 480);
        assert!(detect(&img).is_empty());
    }
}
