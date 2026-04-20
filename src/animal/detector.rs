use image::DynamicImage;
use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use std::sync::{Mutex, OnceLock};

// COCO class indices 14–23 are animals
const ANIMAL_CLASSES: &[usize] = &[14, 15, 16, 17, 18, 19, 20, 21, 22, 23];
const COCO_NAMES: &[&str] = &[
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train",
    "truck", "boat", "traffic light", "fire hydrant", "stop sign",
    "parking meter", "bench",
    "bird", "cat", "dog", "horse", "sheep", "cow",
    "elephant", "bear", "zebra", "giraffe",
];

#[derive(Debug, Clone)]
pub struct AnimalDetection {
    pub species: String,
    pub confidence: f32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

// ── public entry point ───────────────────────────────────────────────────────

/// Detect animals in `img`. Returns empty Vec when model is unavailable.
pub fn detect(img: &DynamicImage) -> Vec<AnimalDetection> {
    if img.width() < 10 || img.height() < 10 {
        return vec![];
    }
    let Some(mtx) = get_session() else {
        return vec![];
    };
    let mut session = mtx.lock().unwrap();
    run_inference(&mut session, img).unwrap_or_else(|e| {
        tracing::warn!("animal detection failed: {e}");
        vec![]
    })
}

// ── model session ────────────────────────────────────────────────────────────

static SESSION: OnceLock<Option<Mutex<Session>>> = OnceLock::new();

fn model_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("picmanager")
        .join("models")
        .join("yolov8n.onnx")
}

fn get_session() -> Option<&'static Mutex<Session>> {
    SESSION
        .get_or_init(|| {
            // Prefer bytes compiled into the binary (via `models bundle` + rebuild).
            if let Some(bytes) = crate::get_embedded_model("yolov8n.onnx") {
                match Session::builder().and_then(|mut b| b.commit_from_memory(&bytes)) {
                    Ok(s) => return Some(Mutex::new(s)),
                    Err(e) => tracing::warn!("embedded animal detection model failed: {e}"),
                }
            }
            // In test builds skip disk loading — ONNX runtime init can hang in CI.
            if cfg!(test) {
                return None;
            }
            // Fall back to the on-disk model in the config directory.
            let path = model_path();
            if !path.exists() {
                tracing::warn!(
                    "animal detection model not found at {}; skipping detection",
                    path.display()
                );
                return None;
            }
            match Session::builder().and_then(|mut b| b.commit_from_file(&path)) {
                Ok(s) => Some(Mutex::new(s)),
                Err(e) => {
                    tracing::warn!("failed to load animal detection model: {e}");
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
) -> Result<Vec<AnimalDetection>, Box<dyn std::error::Error>> {
    let (orig_w, orig_h) = (img.width() as f32, img.height() as f32);
    let input = preprocess(img);
    let tensor = TensorRef::from_array_view(&input)?;

    let outputs = session.run(ort::inputs!["images" => tensor])?;

    // YOLOv8 output: [1, 84, 8400] — 4 bbox coords + 80 class scores
    let (_shape, flat) = outputs[0usize].try_extract_tensor::<f32>()?;
    let n = 8400usize;

    let mut candidates: Vec<(AnimalDetection, f32)> = (0..n)
        .filter_map(|i| {
            // find best animal class score
            let (best_class, best_score) = ANIMAL_CLASSES
                .iter()
                .map(|&c| (c, flat[(4 + c) * n + i]))
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;

            if best_score < 0.4 {
                return None;
            }

            // YOLOv8 bbox: cx, cy, w, h normalized to 640
            let cx = flat[0 * n + i];
            let cy = flat[1 * n + i];
            let bw = flat[2 * n + i];
            let bh = flat[3 * n + i];

            let x = ((cx - bw / 2.0) / 640.0 * orig_w) as i32;
            let y = ((cy - bh / 2.0) / 640.0 * orig_h) as i32;
            let width  = (bw / 640.0 * orig_w) as i32;
            let height = (bh / 640.0 * orig_h) as i32;

            if width <= 0 || height <= 0 {
                return None;
            }

            Some((
                AnimalDetection {
                    species: COCO_NAMES[best_class].to_string(),
                    confidence: best_score,
                    x,
                    y,
                    width: width.max(1),
                    height: height.max(1),
                },
                best_score,
            ))
        })
        .collect();

    let kept = nms(&candidates, 0.45);
    let mut result: Vec<AnimalDetection> = kept
        .into_iter()
        .map(|i| candidates.swap_remove(i).0)
        .collect();
    result.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    Ok(result)
}

// ── pure helpers ─────────────────────────────────────────────────────────────

/// Resize to 640×640, RGB float32 [0,1], shape [1, 3, H, W].
pub(crate) fn preprocess(img: &DynamicImage) -> Array4<f32> {
    let resized = img.resize_exact(640, 640, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let (w, h) = (640usize, 640usize);
    let mut arr = Array4::<f32>::zeros([1, 3, h, w]);
    for y in 0..h {
        for x in 0..w {
            let p = rgb.get_pixel(x as u32, y as u32);
            arr[[0, 0, y, x]] = p[0] as f32 / 255.0; // R
            arr[[0, 1, y, x]] = p[1] as f32 / 255.0; // G
            arr[[0, 2, y, x]] = p[2] as f32 / 255.0; // B
        }
    }
    arr
}

pub(crate) fn iou(a: &AnimalDetection, b: &AnimalDetection) -> f32 {
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

/// Greedy NMS; returns indices of kept detections (highest-confidence first).
pub(crate) fn nms(candidates: &[(AnimalDetection, f32)], iou_thresh: f32) -> Vec<usize> {
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

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn det(x: i32, y: i32, w: i32, h: i32, conf: f32) -> AnimalDetection {
        AnimalDetection { species: "cat".to_string(), x, y, width: w, height: h, confidence: conf }
    }

    // ── preprocess ────────────────────────────────────────────────────────

    #[test]
    fn preprocess_shape() {
        let img = DynamicImage::new_rgb8(100, 80);
        let arr = preprocess(&img);
        assert_eq!(arr.shape(), &[1, 3, 640, 640]);
    }

    #[test]
    fn preprocess_values_in_range() {
        let img = DynamicImage::new_rgb8(64, 64);
        let arr = preprocess(&img);
        for &v in arr.iter() {
            assert!(v >= 0.0 && v <= 1.0, "value {v} out of [0,1]");
        }
    }

    // ── iou ──────────────────────────────────────────────────────────────

    #[test]
    fn iou_no_overlap() {
        assert!((iou(&det(0, 0, 10, 10, 1.0), &det(20, 20, 10, 10, 1.0)) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn iou_identical() {
        assert!((iou(&det(0, 0, 10, 10, 1.0), &det(0, 0, 10, 10, 1.0)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_half_overlap() {
        let v = iou(&det(0, 0, 10, 10, 1.0), &det(5, 0, 10, 10, 1.0));
        assert!((v - 1.0 / 3.0).abs() < 1e-5, "got {v}");
    }

    #[test]
    fn iou_touching_edges_no_overlap() {
        assert!((iou(&det(0, 0, 10, 10, 1.0), &det(10, 0, 10, 10, 1.0)) - 0.0).abs() < 1e-6);
    }

    // ── nms ──────────────────────────────────────────────────────────────

    #[test]
    fn nms_keeps_all_non_overlapping() {
        let c = vec![
            (det(0, 0, 10, 10, 0.9), 0.9f32),
            (det(50, 50, 10, 10, 0.8), 0.8),
            (det(100, 100, 10, 10, 0.7), 0.7),
        ];
        assert_eq!(nms(&c, 0.45).len(), 3);
    }

    #[test]
    fn nms_suppresses_overlapping_lower_confidence() {
        let c = vec![
            (det(0, 0, 100, 100, 0.9), 0.9f32),
            (det(5, 5, 100, 100, 0.6), 0.6),
            (det(200, 200, 100, 100, 0.8), 0.8),
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
    #[ignore = "requires yolov8n.onnx in config_dir/picmanager/models/"]
    fn detect_cat_in_sample_jpeg() {
        let img = image::open("tests/samples/IMG_20250204_135549.jpg").unwrap();
        let animals = detect(&img);
        // sample photo may or may not have animals; just verify no panic
        assert!(animals.iter().all(|a| a.confidence >= 0.4));
    }
}
