//! Canonical display-orientation handling for originals, rendered variants, and legacy media.

use image::DynamicImage;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrientationMode {
    /// Pixels are un-oriented and the recorded display orientation must be applied once.
    Metadata,
    /// The producer already baked the intended orientation into the pixels.
    BakedPixels,
    /// Compatibility media whose original file/legacy database value is authoritative.
    LegacyUnknown,
}

impl OrientationMode {
    pub fn from_catalog(value: Option<&str>) -> Self {
        match value {
            Some("metadata") => Self::Metadata,
            Some("baked_pixels") => Self::BakedPixels,
            _ => Self::LegacyUnknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayTransform {
    pub source_orientation: u8,
    pub rotation: i32,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl DisplayTransform {
    pub fn new(
        mode: OrientationMode,
        display_orientation: Option<u8>,
        legacy_orientation: u8,
        path: &Path,
        rotation: i32,
        flip_h: bool,
        flip_v: bool,
    ) -> Self {
        let source_orientation = match mode {
            OrientationMode::Metadata => normalize_orientation(display_orientation.unwrap_or(1)),
            OrientationMode::BakedPixels => 1,
            OrientationMode::LegacyUnknown => {
                // The original HEIC is authoritative: conversion tools may synthesize an
                // EXIF value from IROT without rotating pixels.
                if crate::image_open::is_heic(path) {
                    crate::image_open::read_exif_orientation(path)
                        .map(normalize_orientation)
                        .unwrap_or_else(|| normalize_orientation(legacy_orientation))
                } else {
                    normalize_orientation(legacy_orientation)
                }
            }
        };
        Self {
            source_orientation,
            rotation,
            flip_h,
            flip_v,
        }
    }

    /// Convert raw/source pixels into display space. Source metadata is applied first,
    /// followed by non-destructive user edits.
    pub fn apply(self, image: DynamicImage) -> DynamicImage {
        let image = apply_exif_orientation(image, self.source_orientation);
        apply_user_transform(image, self.rotation, self.flip_h, self.flip_v)
    }
}

fn normalize_orientation(value: u8) -> u8 {
    if (1..=8).contains(&value) { value } else { 1 }
}

pub fn apply_user_transform(
    image: DynamicImage,
    rotation: i32,
    flip_h: bool,
    flip_v: bool,
) -> DynamicImage {
    let image = match rotation.rem_euclid(360) {
        90 => image.rotate90(),
        180 => image.rotate180(),
        270 => image.rotate270(),
        _ => image,
    };
    let image = if flip_h { image.fliph() } else { image };
    if flip_v { image.flipv() } else { image }
}

pub fn apply_exif_orientation(image: DynamicImage, orientation: u8) -> DynamicImage {
    let (rotation, flip_h) = match normalize_orientation(orientation) {
        2 => (0, true),
        3 => (180, false),
        4 => (180, true),
        5 => (90, true),
        6 => (90, false),
        7 => (270, true),
        8 => (270, false),
        _ => (0, false),
    };
    apply_user_transform(image, rotation, flip_h, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgb, RgbImage};

    fn labelled_image() -> DynamicImage {
        let mut image = RgbImage::new(3, 2);
        for (index, pixel) in image.pixels_mut().enumerate() {
            *pixel = Rgb([(index + 1) as u8, 0, 0]);
        }
        DynamicImage::ImageRgb8(image)
    }

    fn labels(image: &DynamicImage) -> Vec<Vec<u8>> {
        (0..image.height())
            .map(|y| {
                (0..image.width())
                    .map(|x| image.get_pixel(x, y).0[0])
                    .collect()
            })
            .collect()
    }

    #[test]
    fn all_exif_orientations_map_pixels_exactly() {
        let expected = [
            vec![vec![1, 2, 3], vec![4, 5, 6]],
            vec![vec![3, 2, 1], vec![6, 5, 4]],
            vec![vec![6, 5, 4], vec![3, 2, 1]],
            vec![vec![4, 5, 6], vec![1, 2, 3]],
            vec![vec![1, 4], vec![2, 5], vec![3, 6]],
            vec![vec![4, 1], vec![5, 2], vec![6, 3]],
            vec![vec![6, 3], vec![5, 2], vec![4, 1]],
            vec![vec![3, 6], vec![2, 5], vec![1, 4]],
        ];
        for orientation in 1..=8 {
            assert_eq!(
                labels(&apply_exif_orientation(labelled_image(), orientation)),
                expected[(orientation - 1) as usize],
                "incorrect pixel map for EXIF orientation {orientation}"
            );
        }
    }

    #[test]
    fn baked_pixels_never_apply_metadata_again() {
        let transform = DisplayTransform::new(
            OrientationMode::BakedPixels,
            Some(6),
            8,
            Path::new("photo.jpg"),
            0,
            false,
            false,
        );
        assert_eq!(transform.source_orientation, 1);
        assert_eq!(
            labels(&transform.apply(labelled_image())),
            vec![vec![1, 2, 3], vec![4, 5, 6]]
        );
    }

    #[test]
    fn metadata_mode_uses_catalog_display_orientation() {
        let transform = DisplayTransform::new(
            OrientationMode::Metadata,
            Some(6),
            8,
            Path::new("photo.jpg"),
            0,
            false,
            false,
        );
        assert_eq!(transform.source_orientation, 6);
    }

    #[test]
    fn source_orientation_precedes_user_edits() {
        let transform = DisplayTransform::new(
            OrientationMode::Metadata,
            Some(6),
            1,
            Path::new("photo.jpg"),
            90,
            false,
            false,
        );
        assert_eq!(
            labels(&transform.apply(labelled_image())),
            vec![vec![6, 5, 4], vec![3, 2, 1]]
        );
    }
}
