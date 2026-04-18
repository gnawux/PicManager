use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Gif,
    WebP,
    Heic,
    Arw,
    Unknown,
}

impl ImageFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::WebP => "webp",
            Self::Heic => "heic",
            Self::Arw => "arw",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoMeta {
    pub format: ImageFormat,
    pub taken_at: Option<NaiveDateTime>,
    pub camera: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_as_str() {
        assert_eq!(ImageFormat::Jpeg.as_str(), "jpeg");
        assert_eq!(ImageFormat::Arw.as_str(), "arw");
    }

    #[test]
    fn unknown_not_supported() {
        assert!(!ImageFormat::Unknown.is_supported());
        assert!(ImageFormat::Jpeg.is_supported());
    }
}
