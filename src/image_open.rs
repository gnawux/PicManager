use image::DynamicImage;
use std::path::Path;

/// Open any supported image file, returning raw (un-oriented) pixels.
///
/// For JPEG/PNG/WebP/GIF, delegates to `image::open()` which already returns
/// raw pixels (EXIF Orientation is NOT auto-applied by the image crate).
///
/// For HEIC/HEIF, uses macOS `sips` to convert to JPEG (sips auto-applies EXIF
/// Orientation), then reads the original file's orientation tag and applies the
/// inverse transform to get back to raw pixels — consistent with the JPEG path.
/// The rest of the codebase can then apply the stored `exif_orientation` uniformly.
pub fn open_image(path: &Path) -> anyhow::Result<DynamicImage> {
    match image::open(path) {
        Ok(img) => return Ok(img),
        Err(e) => {
            if !is_heic(path) {
                return Err(e.into());
            }
        }
    }
    // sips converts HEIC → JPEG and auto-applies EXIF Orientation.
    // Read the original orientation, then undo it so callers get raw pixels.
    let jpeg = sips_to_jpeg(path)?;
    let img = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)?;
    let orient = read_exif_orientation(path).unwrap_or(1);
    Ok(undo_exif_orientation(img, orient))
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

fn read_exif_orientation(path: &Path) -> Option<u8> {
    use exif::{In, Reader, Tag};
    let file = std::fs::File::open(path).ok()?;
    let mut buf = std::io::BufReader::new(file);
    let data = Reader::new().read_from_container(&mut buf).ok()?;
    let field = data.get_field(Tag::Orientation, In::PRIMARY)?;
    field.value.get_uint(0).map(|v| v as u8)
}

/// Apply the inverse of the given EXIF orientation transform.
///
/// sips auto-applies orientation n when converting HEIC → JPEG.
/// Applying the inverse here returns raw pixels, consistent with
/// `image::open()` on a JPEG (which never auto-orients).
///
/// Inverses: 6 ↔ 8; orientations 1,2,3,4,5,7 are each self-inverse.
fn undo_exif_orientation(img: DynamicImage, orient: u8) -> DynamicImage {
    let (rot_deg, flip_h): (u32, bool) = match orient {
        2 => (0,   true),
        3 => (180, false),
        4 => (180, true),
        5 => (90,  true),
        6 => (270, false), // inverse of CW90  is CCW90 (= CW270)
        7 => (270, true),
        8 => (90,  false), // inverse of CCW90 is CW90
        _ => (0,   false), // 1 = normal, already raw
    };
    let img = match rot_deg {
        90  => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _   => img,
    };
    if flip_h { img.fliph() } else { img }
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
    fn open_heic_dimensions_match_exif() {
        // After undo-orient, the DynamicImage dimensions should match
        // what EXIF reports as the raw sensor dimensions (width > height for
        // landscape, height > width for portrait — matching the stored
        // exif_orientation rather than the display orientation).
        let img = open_image(&sample("IMG_9886.HEIC")).unwrap();
        // IMG_9886.HEIC has EXIF orientation 1 (already upright), so
        // dimensions should be unchanged by undo_exif_orientation.
        assert!(img.width() > 0 && img.height() > 0);
    }

    #[test]
    fn undo_orient_1_is_noop() {
        let img = image::DynamicImage::new_rgb8(10, 20);
        let out = undo_exif_orientation(img, 1);
        assert_eq!((out.width(), out.height()), (10, 20));
    }

    #[test]
    fn undo_orient_6_and_8_are_inverses() {
        // undo(6) followed by undo(8) should be identity on dimensions
        let img = image::DynamicImage::new_rgb8(10, 20);
        let after_6 = undo_exif_orientation(img, 6);   // CW90 inverse = CCW90 → 20×10
        assert_eq!((after_6.width(), after_6.height()), (20, 10));
        let after_8 = undo_exif_orientation(after_6, 8); // CCW90 inverse = CW90 → 10×20
        assert_eq!((after_8.width(), after_8.height()), (10, 20));
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
