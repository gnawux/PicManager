use serde::{Deserialize, Serialize};

use crate::catalog::VariantRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrientationMode {
    /// Original bytes retain their orientation metadata; consumers apply it once.
    Metadata,
    /// Pixels already match the intended display orientation; consumers apply no transform.
    BakedPixels,
    /// Legacy provenance is insufficient to determine whether orientation was baked.
    LegacyUnknown,
}

impl OrientationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::BakedPixels => "baked_pixels",
            Self::LegacyUnknown => "legacy_unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenditionCandidate {
    pub variant_id: i64,
    pub role: VariantRole,
    pub byte_preserved: bool,
}

pub fn select_master(candidates: &[RenditionCandidate]) -> Option<i64> {
    candidates
        .iter()
        .min_by_key(|candidate| master_rank(candidate))
        .map(|candidate| candidate.variant_id)
}

pub fn select_display(candidates: &[RenditionCandidate]) -> Option<i64> {
    candidates
        .iter()
        .min_by_key(|candidate| display_rank(candidate.role))
        .map(|candidate| candidate.variant_id)
}

pub fn requires_current_rendition(metadata_json: Option<&str>) -> bool {
    metadata_json
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|value| {
            value
                .get("adjusted")
                .and_then(|adjusted| adjusted.as_bool())
        })
        .unwrap_or(false)
}

fn master_rank(candidate: &RenditionCandidate) -> u8 {
    match (candidate.role, candidate.byte_preserved) {
        (VariantRole::Original, true) => 0,
        (VariantRole::Imported, _) => 1,
        (VariantRole::Original, false) => 2,
        (VariantRole::Current, _) => 3,
        (VariantRole::RawCompanion, _) => 4,
        (VariantRole::Preview, _) => 5,
        (VariantRole::Thumbnail, _) => 6,
    }
}

fn display_rank(role: VariantRole) -> u8 {
    match role {
        VariantRole::Current => 0,
        VariantRole::Original => 1,
        VariantRole::Imported => 2,
        VariantRole::Preview => 3,
        VariantRole::RawCompanion => 4,
        VariantRole::Thumbnail => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immutable_original_is_master_while_current_is_displayed() {
        let candidates = [
            RenditionCandidate {
                variant_id: 1,
                role: VariantRole::Original,
                byte_preserved: true,
            },
            RenditionCandidate {
                variant_id: 2,
                role: VariantRole::Current,
                byte_preserved: false,
            },
            RenditionCandidate {
                variant_id: 3,
                role: VariantRole::Imported,
                byte_preserved: false,
            },
        ];
        assert_eq!(select_master(&candidates), Some(1));
        assert_eq!(select_display(&candidates), Some(2));
    }

    #[test]
    fn imported_copy_precedes_current_when_original_is_unavailable() {
        let candidates = [
            RenditionCandidate {
                variant_id: 2,
                role: VariantRole::Current,
                byte_preserved: false,
            },
            RenditionCandidate {
                variant_id: 3,
                role: VariantRole::Imported,
                byte_preserved: false,
            },
        ];
        assert_eq!(select_master(&candidates), Some(3));
        assert_eq!(select_display(&candidates), Some(2));
    }

    #[test]
    fn adjusted_inventory_requires_current_rendition_without_guessing() {
        assert!(requires_current_rendition(Some(r#"{"adjusted":true}"#)));
        assert!(!requires_current_rendition(Some(r#"{"adjusted":false}"#)));
        assert!(!requires_current_rendition(Some("invalid")));
        assert!(!requires_current_rendition(None));
    }
}
