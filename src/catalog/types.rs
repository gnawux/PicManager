use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceProvider {
    LegacyLocal,
    LocalImport,
    ApplePhotos,
    GooglePhotos,
    GoogleTakeout,
}

impl SourceProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyLocal => "legacy_local",
            Self::LocalImport => "local_import",
            Self::ApplePhotos => "apple_photos",
            Self::GooglePhotos => "google_photos",
            Self::GoogleTakeout => "google_takeout",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Discovered,
    Queued,
    Downloading,
    Downloaded,
    Importing,
    Ready,
    Failed,
    Excluded,
    Missing,
}

impl SourceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Downloaded => "downloaded",
            Self::Importing => "importing",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Excluded => "excluded",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariantRole {
    Original,
    Current,
    Imported,
    Preview,
    Thumbnail,
    RawCompanion,
}

impl VariantRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Current => "current",
            Self::Imported => "imported",
            Self::Preview => "preview",
            Self::Thumbnail => "thumbnail",
            Self::RawCompanion => "raw_companion",
        }
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Asset {
    pub id: i64,
    pub photo_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AssetSource {
    pub id: i64,
    pub asset_id: Option<i64>,
    pub provider: String,
    pub external_id: Option<String>,
    pub original_filename: Option<String>,
    pub media_type: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub taken_at: Option<String>,
    pub metadata_json: Option<String>,
    pub sync_status: String,
    pub exclusion_reason: Option<String>,
    pub last_error: Option<String>,
    pub last_seen_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AssetVariant {
    pub id: i64,
    pub asset_id: i64,
    pub source_id: Option<i64>,
    pub role: String,
    pub path: Option<String>,
    pub content_sha256: Option<String>,
    pub mime_type: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub byte_size: Option<i64>,
    pub generation_key: Option<String>,
    pub is_primary: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_catalog_values_match_database_contract() {
        assert_eq!(SourceProvider::ApplePhotos.as_str(), "apple_photos");
        assert_eq!(SourceStatus::Downloading.as_str(), "downloading");
        assert_eq!(VariantRole::RawCompanion.as_str(), "raw_companion");
    }

    #[test]
    fn enums_serialize_as_database_values() {
        assert_eq!(
            serde_json::to_string(&SourceProvider::GoogleTakeout).unwrap(),
            "\"google_takeout\""
        );
        assert_eq!(
            serde_json::to_string(&VariantRole::Imported).unwrap(),
            "\"imported\""
        );
    }
}
