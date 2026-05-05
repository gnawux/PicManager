use image::DynamicImage;
use std::path::Path;

/// Open any supported image file.
///
/// Tries the `image` crate first (JPEG/PNG/WebP/GIF).  For HEIC/HEIF files,
/// falls back to `sips` (macOS built-in) to convert to JPEG in a temp file,
/// then decodes that.
pub fn open_image(path: &Path) -> anyhow::Result<DynamicImage> {
    match image::open(path) {
        Ok(img) => return Ok(img),
        Err(e) => {
            if !is_heic(path) {
                return Err(e.into());
            }
        }
    }
    let jpeg = sips_to_jpeg(path)?;
    Ok(image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)?)
}

/// Return raw JPEG bytes for any supported image.
///
/// For non-HEIC files, reads the file verbatim.
/// For HEIC/HEIF, transcodes to JPEG via `sips` and returns the result.
/// Used by `GET /api/photos/:id/file` to ensure Chrome-compatible output.
pub fn heic_to_jpeg(path: &Path) -> anyhow::Result<Vec<u8>> {
    if is_heic(path) {
        sips_to_jpeg(path)
    } else {
        Ok(std::fs::read(path)?)
    }
}

fn is_heic(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .as_deref(),
        Some("heic" | "heif")
    )
}

fn sips_to_jpeg(path: &Path) -> anyhow::Result<Vec<u8>> {
    let tmp = tempfile::Builder::new().suffix(".jpg").tempfile()?;
    let tmp_path = tmp.path().to_path_buf();
    let out = std::process::Command::new("sips")
        .args([
            "-s", "format", "jpeg",
            path.to_str().ok_or_else(|| anyhow::anyhow!("non-UTF8 path"))?,
            "--out",
            tmp_path.to_str().ok_or_else(|| anyhow::anyhow!("non-UTF8 tmp path"))?,
        ])
        .output()?;
    if !out.status.success() {
        anyhow::bail!("sips: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(std::fs::read(&tmp_path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/samples")
            .join(name)
    }

    #[test]
    fn open_jpeg_works() {
        let img = open_image(&sample("IMG_9844.JPG")).unwrap();
        assert!(img.width() > 0 && img.height() > 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn open_heic_via_sips() {
        let img = open_image(&sample("IMG_9886.HEIC")).unwrap();
        assert!(img.width() > 0 && img.height() > 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn heic_to_jpeg_returns_valid_jpeg() {
        let bytes = heic_to_jpeg(&sample("IMG_9886.HEIC")).unwrap();
        assert!(!bytes.is_empty());
        // JPEG magic bytes FF D8 FF
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);
    }

    #[test]
    fn non_heic_heic_to_jpeg_reads_verbatim() {
        let path = sample("IMG_9844.JPG");
        let bytes = heic_to_jpeg(&path).unwrap();
        let expected = std::fs::read(&path).unwrap();
        assert_eq!(bytes, expected);
    }
}
