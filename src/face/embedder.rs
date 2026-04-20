use crate::error::AppError;
use crate::face::FaceRegion;
use image::DynamicImage;
use ndarray::Array4;
use ort::session::Session;
use ort::value::TensorRef;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

// ── global session (loaded at most once per process) ─────────────────────────

static SESSION: OnceLock<Option<Mutex<Session>>> = OnceLock::new();

fn model_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("picmanager")
        .join("models")
        .join("arcface_mobilenetv1.onnx")
}

fn get_session() -> Option<&'static Mutex<Session>> {
    SESSION
        .get_or_init(|| {
            // Prefer bytes compiled into the binary (via `models bundle` + rebuild).
            if let Some(bytes) = crate::get_embedded_model("arcface_mobilenetv1.onnx") {
                match (|| -> ort::Result<Session> {
                    Session::builder()?
                        .with_execution_providers([ort::ep::coreml::CoreML::default().build()])?
                        .commit_from_memory(&bytes)
                })() {
                    Ok(s) => return Some(Mutex::new(s)),
                    Err(e) => tracing::warn!("embedded embedder model failed: {e}"),
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
                    "face embedding model not found at {}; embeddings will be skipped",
                    path.display()
                );
                return None;
            }
            match (|| -> ort::Result<Session> {
                Session::builder()?
                    .with_execution_providers([ort::ep::coreml::CoreML::default().build()])?
                    .commit_from_file(&path)
            })() {
                Ok(s) => Some(Mutex::new(s)),
                Err(e) => {
                    tracing::warn!("failed to load face embedding model: {e}");
                    None
                }
            }
        })
        .as_ref()
}

// ── public API ────────────────────────────────────────────────────────────────

/// Zero-cost handle that confirms the embedding session is ready.
/// The underlying ONNX session is a process-wide singleton loaded at most once.
pub struct Embedder;

impl Embedder {
    /// Returns `Ok(Embedder)` if the model is available (embedded or on disk).
    /// The model is loaded at most once per process regardless of how many times
    /// `load` is called.
    pub fn load(_model_path: &Path) -> crate::error::Result<Self> {
        if get_session().is_some() {
            Ok(Self)
        } else {
            Err(AppError::ModelNotFound("arcface model unavailable".into()))
        }
    }

    pub fn extract(&self, img: &DynamicImage, region: &FaceRegion) -> crate::error::Result<Vec<f32>> {
        let Some(mtx) = get_session() else {
            return Err(AppError::ModelNotFound("embedder not loaded".into()));
        };
        let input = preprocess(img, region);
        let tensor = TensorRef::from_array_view(&input)
            .map_err(|e| AppError::ModelNotFound(e.to_string()))?;
        let mut session = mtx
            .lock()
            .map_err(|_| AppError::ModelNotFound("embedder session mutex poisoned".into()))?;
        let outputs = session
            .run(ort::inputs!["data" => tensor])
            .map_err(|e| AppError::ModelNotFound(e.to_string()))?;
        let (_shape, raw) = outputs[0usize]
            .try_extract_tensor::<f32>()
            .map_err(|e| AppError::ModelNotFound(e.to_string()))?;
        let embedding = l2_normalize(raw);
        Ok(embedding)
    }
}

// ── pure helpers ─────────────────────────────────────────────────────────────

/// Crop region with 20% padding, resize to 112×112, normalise to [-1, 1].
pub(crate) fn preprocess(img: &DynamicImage, region: &FaceRegion) -> Array4<f32> {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let pad_x = (region.width as f32 * 0.2) as i32;
    let pad_y = (region.height as f32 * 0.2) as i32;
    let x1 = (region.x - pad_x).max(0) as u32;
    let y1 = (region.y - pad_y).max(0) as u32;
    let x2 = (region.x + region.width + pad_x).min(iw) as u32;
    let y2 = (region.y + region.height + pad_y).min(ih) as u32;
    let w = (x2 - x1).max(1);
    let h = (y2 - y1).max(1);

    let cropped = img.crop_imm(x1, y1, w, h);
    let resized = cropped.resize_exact(112, 112, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let mut arr = Array4::<f32>::zeros([1, 3, 112, 112]);
    for y in 0..112usize {
        for x in 0..112usize {
            let p = rgb.get_pixel(x as u32, y as u32);
            arr[[0, 0, y, x]] = (p[0] as f32 - 127.5) / 127.5;
            arr[[0, 1, y, x]] = (p[1] as f32 - 127.5) / 127.5;
            arr[[0, 2, y, x]] = (p[2] as f32 - 127.5) / 127.5;
        }
    }
    arr
}

pub(crate) fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-10 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

/// Encode a f32 slice to little-endian bytes.
pub fn encode_embedding(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Decode little-endian bytes back to f32 Vec.
pub fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::FaceRegion;

    fn dummy_region() -> FaceRegion {
        FaceRegion { x: 10, y: 10, width: 80, height: 80, confidence: 0.9 }
    }

    #[test]
    fn l2_normalize_unit_vector() {
        let v = vec![3.0f32, 4.0];
        let n = l2_normalize(&v);
        let norm: f32 = n.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "norm={norm}");
        assert!((n[0] - 0.6).abs() < 1e-6);
        assert!((n[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_zero_vector() {
        let v = vec![0.0f32; 4];
        let n = l2_normalize(&v);
        assert_eq!(n, v);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let original: Vec<f32> = (0..512).map(|i| i as f32 * 0.001).collect();
        let blob = encode_embedding(&original);
        assert_eq!(blob.len(), 512 * 4);
        let decoded = decode_embedding(&blob);
        for (a, b) in original.iter().zip(decoded.iter()) {
            assert_eq!(a.to_bits(), b.to_bits(), "mismatch at value {a}");
        }
    }

    #[test]
    fn preprocess_output_shape() {
        let img = DynamicImage::new_rgb8(200, 200);
        let arr = preprocess(&img, &dummy_region());
        assert_eq!(arr.shape(), &[1, 3, 112, 112]);
    }

    #[test]
    fn preprocess_values_in_range() {
        let img = DynamicImage::new_rgb8(200, 200);
        let arr = preprocess(&img, &dummy_region());
        for &v in arr.iter() {
            assert!(v >= -1.0 && v <= 1.0, "value out of range: {v}");
        }
    }

    #[test]
    fn preprocess_clamps_region_to_image() {
        // Region extends beyond image bounds; should not panic
        let img = DynamicImage::new_rgb8(100, 100);
        let big = FaceRegion { x: 80, y: 80, width: 60, height: 60, confidence: 0.9 };
        let arr = preprocess(&img, &big);
        assert_eq!(arr.shape(), &[1, 3, 112, 112]);
    }

    #[test]
    #[ignore = "requires arcface_mobilenetv1.onnx in config_dir/picmanager/models/"]
    fn extract_returns_512d_l2_normalized() {
        let model_path = dirs::config_dir()
            .unwrap()
            .join("picmanager")
            .join("models")
            .join("arcface_mobilenetv1.onnx");
        let embedder = Embedder::load(&model_path).unwrap();
        let img = image::open("tests/samples/IMG_20250204_135549.jpg").unwrap();
        let region = FaceRegion { x: 100, y: 50, width: 200, height: 200, confidence: 0.95 };
        let emb = embedder.extract(&img, &region).unwrap();
        assert_eq!(emb.len(), 512);
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "L2 norm={norm}");
    }
}
