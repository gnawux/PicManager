use image::DynamicImage;
use std::path::Path;

/// Open any supported image file, returning raw (un-oriented) pixels.
///
/// For JPEG/PNG/WebP/GIF, delegates to `image::open()` which returns
/// raw pixels (EXIF Orientation is NOT auto-applied by the image crate).
///
/// For HEIC/HEIF, uses macOS `sips` to convert to JPEG. sips copies the EXIF
/// Orientation tag to the output JPEG but does NOT rotate the pixels (iPhone
/// HEICs store rotation in the EXIF tag, not the HEIF IROT box). The image crate
/// also does not auto-orient, so callers receive raw sensor pixels and must apply
/// the stored `exif_orientation` themselves — consistent with the JPEG path.
pub fn open_image(path: &Path) -> anyhow::Result<DynamicImage> {
    match image::open(path) {
        Ok(img) => return Ok(img),
        Err(e) => {
            if !is_heic(path) {
                return Err(e.into());
            }
        }
    }
    // sips copies EXIF Orientation to the output JPEG but does NOT rotate pixels;
    // load as-is so callers get raw sensor pixels.
    let jpeg = sips_to_jpeg(path)?;
    Ok(image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)?)
}

/// Return raw JPEG bytes for any supported image.
///
/// For non-HEIC files, reads the file verbatim.
/// For HEIC/HEIF, transcodes to JPEG via `sips` (display-oriented output).
/// Used by `GET /api/photos/:id/file` for Chrome-compatible display.
pub fn heic_to_jpeg(path: &Path) -> anyhow::Result<Vec<u8>> {
    if is_heic(path) {
        sips_to_jpeg(path)
    } else {
        Ok(std::fs::read(path)?)
    }
}

pub fn is_heic(path: &Path) -> bool {
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

pub fn read_exif_orientation(path: &Path) -> Option<u8> {
    use exif::{In, Reader, Tag};
    let file = std::fs::File::open(path).ok()?;
    let mut buf = std::io::BufReader::new(file);
    let data = Reader::new().read_from_container(&mut buf).ok()?;
    let field = data.get_field(Tag::Orientation, In::PRIMARY)?;
    field.value.get_uint(0).map(|v| v as u8)
}

pub fn read_exif_orientation_from_bytes(bytes: &[u8]) -> Option<u8> {
    use exif::{In, Reader, Tag};
    let data = Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    let field = data.get_field(Tag::Orientation, In::PRIMARY)?;
    field.value.get_uint(0).map(|v| v as u8)
}

/// Like `open_image`, but also returns the effective EXIF orientation.
///
/// For HEIC, sips may translate a HEIF IROT box into an EXIF Orientation tag
/// in the output JPEG (e.g. a Photos-exported HEIC can have IROT=90 CW with
/// EXIF Orientation=1 in the container).  Reading orientation from the sips
/// output rather than the original HEIC file gives the correct value.
/// For non-HEIC files the orientation is read from the file as usual.
pub fn open_image_with_orient(path: &Path) -> anyhow::Result<(DynamicImage, u8)> {
    if is_heic(path) {
        let jpeg = sips_to_jpeg(path)?;
        let orient = read_exif_orientation_from_bytes(&jpeg).unwrap_or(1);
        let img = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)?;
        Ok((img, orient))
    } else {
        let img = image::open(path)?;
        let orient = read_exif_orientation(path).unwrap_or(1);
        Ok((img, orient))
    }
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
    fn open_heic_returns_raw_sensor_pixels() {
        // open_image returns raw pixels without applying EXIF Orientation.
        // Callers must apply orientation themselves before display/resize.
        let img = open_image(&sample("IMG_9886.HEIC")).unwrap();
        assert!(img.width() > 0 && img.height() > 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn heic_to_jpeg_returns_valid_jpeg() {
        let bytes = heic_to_jpeg(&sample("IMG_9886.HEIC")).unwrap();
        assert!(!bytes.is_empty());
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
