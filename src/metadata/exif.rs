use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use exif::{In, Reader, Tag};
use std::io::BufReader;
use std::fs::File;
use std::path::Path;
use super::types::PhotoMeta;
use crate::error::{AppError, Result};

pub fn extract_from_file(path: &Path) -> Result<PhotoMeta> {
    let fmt = detect_format(path)?;
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let exif = Reader::new().read_from_container(&mut reader).ok();

    let taken_at = exif.as_ref().and_then(parse_datetime);
    let camera = exif.as_ref().and_then(parse_camera);
    let (gps_lat, gps_lon) = exif.as_ref().map(parse_gps).unwrap_or((None, None));
    let exif_orientation = exif.as_ref().map(parse_orientation).unwrap_or(1);
    let timezone_offset = exif.as_ref().and_then(parse_timezone_offset);

    Ok(PhotoMeta { format: fmt, taken_at, camera, gps_lat, gps_lon, exif_orientation, timezone_offset })
}

/// Re-read only the timezone offset from a file already in the library.
/// Returns None if the file has no EXIF timezone info or can't be read.
pub fn read_timezone_offset(path: &Path) -> Option<i32> {
    let file = File::open(path).ok()?;
    let exif = Reader::new().read_from_container(&mut BufReader::new(file)).ok()?;
    parse_timezone_offset(&exif)
}

/// Read EXIF Orientation tag; returns 1 (normal) if absent or invalid.
fn parse_orientation(exif: &exif::Exif) -> u8 {
    let field = match exif.get_field(Tag::Orientation, In::PRIMARY) {
        Some(f) => f,
        None => return 1,
    };
    if let exif::Value::Short(ref v) = field.value {
        let val = v.first().copied().unwrap_or(1);
        if (1..=8).contains(&val) { val as u8 } else { 1 }
    } else {
        1
    }
}

fn detect_format(path: &Path) -> Result<super::types::ImageFormat> {
    use std::io::Read;
    let mut header = [0u8; 12];
    let mut f = File::open(path)?;
    let n = f.read(&mut header)?;
    let fmt = super::format::detect(path, &header[..n]);
    if !fmt.is_supported() {
        return Err(AppError::UnsupportedFormat(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_string(),
        ));
    }
    Ok(fmt)
}

/// Try EXIF time fields in priority order:
/// DateTimeOriginal → DateTimeDigitized → GPS DateStamp+TimeStamp → DateTime
fn parse_datetime(exif: &exif::Exif) -> Option<NaiveDateTime> {
    parse_datetime_tag(exif, Tag::DateTimeOriginal)
        .or_else(|| parse_datetime_tag(exif, Tag::DateTimeDigitized))
        .or_else(|| parse_gps_datetime(exif))
        .or_else(|| parse_datetime_tag(exif, Tag::DateTime))
}

fn parse_datetime_tag(exif: &exif::Exif, tag: Tag) -> Option<NaiveDateTime> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let s = field.display_value().to_string();
    NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").ok()
}

/// Parse GPS DateStamp ("YYYY:MM:DD") + TimeStamp (three Rationals: H, M, S).
fn parse_gps_datetime(exif: &exif::Exif) -> Option<NaiveDateTime> {
    let date_field = exif.get_field(Tag::GPSDateStamp, In::PRIMARY)?;
    let date_str = date_field.display_value().to_string();
    // kamadak-exif shows GPSDateStamp as "YYYY:MM:DD" (with quotes stripped by display_value)
    let date_str = date_str.trim_matches('"');
    let date = NaiveDate::parse_from_str(date_str, "%Y:%m:%d")
        .or_else(|_| NaiveDate::parse_from_str(date_str, "%Y-%m-%d"))
        .ok()?;

    let time_field = exif.get_field(Tag::GPSTimeStamp, In::PRIMARY)?;
    let time = parse_gps_time(time_field)?;

    Some(NaiveDateTime::new(date, time))
}

fn parse_gps_time(field: &exif::Field) -> Option<NaiveTime> {
    use exif::Value;
    if let Value::Rational(ref r) = field.value
        && r.len() >= 3
    {
        let h = r[0].to_f64() as u32;
        let m = r[1].to_f64() as u32;
        let s = r[2].to_f64() as u32;
        return NaiveTime::from_hms_opt(h, m, s);
    }
    None
}

/// Try OffsetTimeOriginal → OffsetTimeDigitized → OffsetTime EXIF tags.
/// Fallback: infer from (DateTimeOriginal − GPS UTC time), rounded to 30 min.
fn parse_timezone_offset(exif: &exif::Exif) -> Option<i32> {
    parse_offset_tag(exif, Tag::OffsetTimeOriginal)
        .or_else(|| parse_offset_tag(exif, Tag::OffsetTimeDigitized))
        .or_else(|| parse_offset_tag(exif, Tag::OffsetTime))
        .or_else(|| infer_timezone_from_gps(exif))
}

fn parse_offset_tag(exif: &exif::Exif, tag: Tag) -> Option<i32> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let s = field.display_value().to_string();
    parse_offset_string(s.trim_matches('"').trim())
}

/// Parse "+08:00" or "-05:30" into minutes.
fn parse_offset_string(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.len() < 5 { return None; }
    let sign: i32 = match s.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let mut parts = s[1..].splitn(2, ':');
    let hours: i32 = parts.next()?.parse().ok()?;
    let mins: i32 = parts.next()?.parse().ok()?;
    if hours > 14 || mins > 59 { return None; }
    Some(sign * (hours * 60 + mins))
}

/// Infer UTC offset from DateTimeOriginal (local) minus GPS timestamp (UTC).
/// Rounds to the nearest 30 minutes; rejects values outside [-720, 840].
fn infer_timezone_from_gps(exif: &exif::Exif) -> Option<i32> {
    let local_dt = parse_datetime_tag(exif, Tag::DateTimeOriginal)?;
    let gps_utc = parse_gps_datetime(exif)?;
    let diff_min = (local_dt - gps_utc).num_minutes() as i32;
    // Round to nearest 30 min (all valid timezones are multiples of 15/30 min)
    let rounded = ((diff_min + if diff_min >= 0 { 15 } else { -15 }) / 30) * 30;
    if (-720..=840).contains(&rounded) { Some(rounded) } else { None }
}

fn parse_camera(exif: &exif::Exif) -> Option<String> {
    let make = exif
        .get_field(Tag::Make, In::PRIMARY)
        .map(|f| f.display_value().to_string())
        .unwrap_or_default();
    let model = exif
        .get_field(Tag::Model, In::PRIMARY)
        .map(|f| f.display_value().to_string())
        .unwrap_or_default();

    let make = make.trim_matches('"').trim();
    let model = model.trim_matches('"').trim();

    if model.is_empty() {
        return None;
    }
    if make.is_empty() || model.to_lowercase().starts_with(&make.to_lowercase()) {
        Some(model.to_string())
    } else {
        Some(format!("{make} {model}"))
    }
}

fn parse_gps(exif: &exif::Exif) -> (Option<f64>, Option<f64>) {
    let lat = parse_gps_coord(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef);
    let lon = parse_gps_coord(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef);
    (lat, lon)
}

fn parse_gps_coord(exif: &exif::Exif, coord_tag: Tag, ref_tag: Tag) -> Option<f64> {
    let field = exif.get_field(coord_tag, In::PRIMARY)?;
    let degrees = rational_to_decimal(field)?;

    let ref_field = exif.get_field(ref_tag, In::PRIMARY)?;
    let ref_str = ref_field.display_value().to_string();
    let ref_char = ref_str.trim_matches('"').trim().chars().next()?;

    let signed = match ref_char {
        'S' | 'W' => -degrees,
        _ => degrees,
    };
    Some(signed)
}

fn rational_to_decimal(field: &exif::Field) -> Option<f64> {
    use exif::Value;
    if let Value::Rational(ref rationals) = field.value
        && rationals.len() >= 3
    {
        return Some(
            rationals[0].to_f64()
                + rationals[1].to_f64() / 60.0
                + rationals[2].to_f64() / 3600.0,
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn extract_with_exif_jpeg() {
        let meta = extract_from_file(&fixture("with_exif.jpg")).unwrap();
        assert_eq!(meta.format, super::super::types::ImageFormat::Jpeg);
        assert_eq!(meta.taken_at.unwrap().to_string(), "2024-06-15 10:30:00");
        assert_eq!(meta.camera.as_deref(), Some("Apple iPhone 15 Pro"));
        let lat = meta.gps_lat.unwrap();
        let lon = meta.gps_lon.unwrap();
        assert!((lat - 37.7749).abs() < 0.01, "lat={lat}");
        assert!((lon - (-122.4194)).abs() < 0.01, "lon={lon}");
    }

    #[test]
    fn extract_no_exif_jpeg() {
        let meta = extract_from_file(&fixture("no_exif.jpg")).unwrap();
        assert_eq!(meta.format, super::super::types::ImageFormat::Jpeg);
        assert!(meta.taken_at.is_none());
        assert!(meta.camera.is_none());
        assert!(meta.gps_lat.is_none());
    }

    #[test]
    fn extract_gps_from_heic_sample() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/samples/IMG_9886.HEIC");
        let meta = extract_from_file(&path).unwrap();
        let lat = meta.gps_lat.expect("IMG_9886.HEIC should have GPS latitude");
        let lon = meta.gps_lon.expect("IMG_9886.HEIC should have GPS longitude");
        // 39°50'26.05"N, 116°13'4.73"E (Beijing area)
        assert!((lat - 39.8406).abs() < 0.001, "unexpected lat={lat}");
        assert!((lon - 116.2180).abs() < 0.001, "unexpected lon={lon}");
    }

    #[test]
    fn unsupported_format_returns_error() {
        let tmp = tempfile::NamedTempFile::with_suffix(".bmp").unwrap();
        std::fs::write(tmp.path(), b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00").unwrap();
        let result = extract_from_file(tmp.path());
        assert!(matches!(result, Err(AppError::UnsupportedFormat(_))));
    }

    #[test]
    fn exif_orientation_defaults_to_1_when_absent() {
        // no_exif.jpg has no EXIF data at all
        let meta = extract_from_file(&fixture("no_exif.jpg")).unwrap();
        assert_eq!(meta.exif_orientation, 1, "absent orientation should default to 1");
    }

    #[test]
    fn exif_orientation_is_valid_for_fixture_with_exif() {
        // with_exif.jpg is a standard-orientation fixture, should return 1
        let meta = extract_from_file(&fixture("with_exif.jpg")).unwrap();
        assert!((1..=8).contains(&meta.exif_orientation), "orientation must be 1-8");
    }

    // --- timezone offset tests ---

    #[test]
    fn parse_offset_string_positive() {
        assert_eq!(parse_offset_string("+08:00"), Some(480));
        assert_eq!(parse_offset_string("+05:30"), Some(330));
        assert_eq!(parse_offset_string("+00:00"), Some(0));
    }

    #[test]
    fn parse_offset_string_negative() {
        assert_eq!(parse_offset_string("-05:00"), Some(-300));
        assert_eq!(parse_offset_string("-05:30"), Some(-330));
    }

    #[test]
    fn parse_offset_string_invalid() {
        assert_eq!(parse_offset_string(""), None);
        assert_eq!(parse_offset_string("08:00"), None); // missing sign
        assert_eq!(parse_offset_string("+99:00"), None); // out of range
    }

    #[test]
    fn heic_sample_timezone_is_plus8() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/samples/IMG_9886.HEIC");
        let meta = extract_from_file(&path).unwrap();
        assert_eq!(meta.timezone_offset, Some(480), "iPhone Beijing photo should be UTC+8");
    }

    #[test]
    fn jpg_sample_timezone_is_plus8() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/samples/IMG_9844.JPG");
        let meta = extract_from_file(&path).unwrap();
        assert_eq!(meta.timezone_offset, Some(480), "Nikon Z8 Beijing photo should be UTC+8");
    }

    // --- fallback field tests ---

    #[test]
    fn fallback_to_datetime_digitized() {
        let meta = extract_from_file(&fixture("digitized_only.jpg")).unwrap();
        assert_eq!(
            meta.taken_at.unwrap().to_string(),
            "2024-07-20 09:15:00",
            "should fall back to DateTimeDigitized"
        );
    }

    #[test]
    fn fallback_to_gps_datetime() {
        let meta = extract_from_file(&fixture("gps_time_only.jpg")).unwrap();
        let taken = meta.taken_at.unwrap();
        assert_eq!(taken.date().to_string(), "2024-08-10", "GPS date should be used");
        assert_eq!(taken.time().to_string(), "14:30:00", "GPS time should be used");
    }

    #[test]
    fn fallback_to_datetime_tag() {
        let meta = extract_from_file(&fixture("datetime_only.jpg")).unwrap();
        assert_eq!(
            meta.taken_at.unwrap().to_string(),
            "2024-09-05 08:00:00",
            "should fall back to DateTime"
        );
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn dump_exif_field(path: &std::path::Path, tag: exif::Tag) -> String {
    use exif::{In, Reader};
    use std::fs::File;
    use std::io::BufReader;
    let f = File::open(path).unwrap();
    let exif = Reader::new().read_from_container(&mut BufReader::new(f)).unwrap();
    exif.get_field(tag, In::PRIMARY)
        .map(|f| f.display_value().to_string())
        .unwrap_or_else(|| "(absent)".to_string())
}
