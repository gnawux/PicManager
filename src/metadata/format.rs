use std::path::Path;
use super::types::ImageFormat;

/// magic bytes で形式を判定、拡張子を補助的に使用
pub fn detect(path: &Path, header: &[u8]) -> ImageFormat {
    if header.len() >= 12 {
        // JPEG: FF D8 FF
        if header.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return ImageFormat::Jpeg;
        }
        // PNG: 89 50 4E 47 0D 0A 1A 0A
        if header.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            return ImageFormat::Png;
        }
        // GIF: 47 49 46 38
        if header.starts_with(b"GIF8") {
            return ImageFormat::Gif;
        }
        // WebP: RIFF????WEBP
        if header.starts_with(b"RIFF") && &header[8..12] == b"WEBP" {
            return ImageFormat::WebP;
        }
        // TIFF (ARW base): 49 49 2A 00 or 4D 4D 00 2A
        if (header.starts_with(&[0x49, 0x49, 0x2A, 0x00])
            || header.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]))
            && is_arw_by_ext(path)
        {
            return ImageFormat::Arw;
        }
    }
    // HEIC/HEIF: ftyp box（offset 4）
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        let brand = &header[8..12];
        if matches!(brand, b"heic" | b"heix" | b"mif1" | b"msf1") {
            return ImageFormat::Heic;
        }
    }
    ImageFormat::Unknown
}

fn is_arw_by_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("arw"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    #[test]
    fn detect_jpeg() {
        let header = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(detect(&path("a.jpg"), &header), ImageFormat::Jpeg);
    }

    #[test]
    fn detect_png() {
        let header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        assert_eq!(detect(&path("a.png"), &header), ImageFormat::Png);
    }

    #[test]
    fn detect_gif() {
        let header = *b"GIF89a\x01\x00\x01\x00\x00\x00";
        assert_eq!(detect(&path("a.gif"), &header), ImageFormat::Gif);
    }

    #[test]
    fn detect_webp() {
        let mut header = *b"RIFF\x00\x00\x00\x00WEBP";
        assert_eq!(detect(&path("a.webp"), &header), ImageFormat::WebP);
        // 少し変えても検出できる
        header[4] = 0x10;
        assert_eq!(detect(&path("a.webp"), &header), ImageFormat::WebP);
    }

    #[test]
    fn detect_unknown() {
        let header = [0x00u8; 12];
        assert_eq!(detect(&path("a.bmp"), &header), ImageFormat::Unknown);
    }

    #[test]
    fn detect_tiff_without_arw_ext_is_unknown() {
        let header = [0x49, 0x49, 0x2A, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(detect(&path("a.tiff"), &header), ImageFormat::Unknown);
    }

    #[test]
    fn detect_arw() {
        let header = [0x49, 0x49, 0x2A, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(detect(&path("a.ARW"), &header), ImageFormat::Arw);
    }
}
