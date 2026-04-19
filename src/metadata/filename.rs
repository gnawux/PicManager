use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

/// Infer a datetime from a filename (stem only, extension stripped).
///
/// Priority:
/// 1. Pure numeric stem: 10-digit (Unix seconds) or 13-digit (Unix milliseconds)
/// 2. Compact pattern: YYYYMMDD[_-]HHMMSS anywhere in the name
/// 3. Separated date: YYYY[-_]MM[-_]DD anywhere in the name (time defaults to 00:00:00)
///
/// Returns None when nothing matches or the date is invalid.
pub fn infer_date(filename: &str) -> Option<NaiveDateTime> {
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);

    try_unix_timestamp(stem)
        .or_else(|| try_compact_datetime(stem))
        .or_else(|| try_separated_date(stem))
}

fn try_unix_timestamp(stem: &str) -> Option<NaiveDateTime> {
    if !stem.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    match stem.len() {
        10 => {
            let secs: i64 = stem.parse().ok()?;
            let dt: DateTime<Utc> = Utc.timestamp_opt(secs, 0).single()?;
            Some(dt.naive_utc())
        }
        13 => {
            let millis: i64 = stem.parse().ok()?;
            let dt: DateTime<Utc> = Utc.timestamp_millis_opt(millis).single()?;
            Some(dt.naive_utc())
        }
        _ => None,
    }
}

/// Match YYYYMMDD[_-]HHMMSS anywhere in the string.
fn try_compact_datetime(s: &str) -> Option<NaiveDateTime> {
    let bytes = s.as_bytes();
    if bytes.len() < 15 {
        return None;
    }
    for i in 0..=bytes.len() - 15 {
        if !is_digit8(&bytes[i..i + 8]) {
            continue;
        }
        if bytes[i + 8] != b'_' && bytes[i + 8] != b'-' {
            continue;
        }
        if !is_digit6(&bytes[i + 9..i + 15]) {
            continue;
        }
        let date_part = &s[i..i + 8];
        let time_part = &s[i + 9..i + 15];
        if let Some(dt) = parse_compact(date_part, time_part) {
            return Some(dt);
        }
    }
    None
}

fn parse_compact(date8: &str, time6: &str) -> Option<NaiveDateTime> {
    let y: i32 = date8[0..4].parse().ok()?;
    let mo: u32 = date8[4..6].parse().ok()?;
    let d: u32 = date8[6..8].parse().ok()?;
    let h: u32 = time6[0..2].parse().ok()?;
    let m: u32 = time6[2..4].parse().ok()?;
    let sec: u32 = time6[4..6].parse().ok()?;
    let date = NaiveDate::from_ymd_opt(y, mo, d)?;
    let time = NaiveTime::from_hms_opt(h, m, sec)?;
    Some(NaiveDateTime::new(date, time))
}

/// Match YYYY[-_]MM[-_]DD anywhere in the string.
fn try_separated_date(s: &str) -> Option<NaiveDateTime> {
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    for i in 0..=bytes.len() - 10 {
        if !is_digit4(&bytes[i..i + 4]) {
            continue;
        }
        let sep1 = bytes[i + 4];
        if sep1 != b'-' && sep1 != b'_' {
            continue;
        }
        if !is_digit2(&bytes[i + 5..i + 7]) {
            continue;
        }
        let sep2 = bytes[i + 7];
        if sep2 != b'-' && sep2 != b'_' {
            continue;
        }
        if !is_digit2(&bytes[i + 8..i + 10]) {
            continue;
        }
        let y: i32 = s[i..i + 4].parse().ok()?;
        let mo: u32 = s[i + 5..i + 7].parse().ok()?;
        let d: u32 = s[i + 8..i + 10].parse().ok()?;
        if let Some(date) = NaiveDate::from_ymd_opt(y, mo, d) {
            return Some(NaiveDateTime::new(date, NaiveTime::MIN));
        }
    }
    None
}

fn is_digit4(b: &[u8]) -> bool {
    b.len() == 4 && b.iter().all(|c| c.is_ascii_digit())
}
fn is_digit2(b: &[u8]) -> bool {
    b.len() == 2 && b.iter().all(|c| c.is_ascii_digit())
}
fn is_digit6(b: &[u8]) -> bool {
    b.len() == 6 && b.iter().all(|c| c.is_ascii_digit())
}
fn is_digit8(b: &[u8]) -> bool {
    b.len() == 8 && b.iter().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn compact_datetime() {
        assert_eq!(infer_date("IMG_20240615_103000.jpg"), Some(dt("2024-06-15 10:30:00")));
    }

    #[test]
    fn compact_datetime_dash_sep() {
        assert_eq!(infer_date("20240615-103000.jpg"), Some(dt("2024-06-15 10:30:00")));
    }

    #[test]
    fn separated_date_with_suffix() {
        assert_eq!(infer_date("2024-06-15_vacation.jpg"), Some(dt("2024-06-15 00:00:00")));
    }

    #[test]
    fn separated_date_underscore() {
        assert_eq!(infer_date("2024_06_15_vacation.jpg"), Some(dt("2024-06-15 00:00:00")));
    }

    #[test]
    fn unix_seconds() {
        // 1718447400 = 2024-06-15 10:30:00 UTC
        assert_eq!(infer_date("1718447400.jpg"), Some(dt("2024-06-15 10:30:00")));
    }

    #[test]
    fn unix_millis() {
        // 1718447400000 ms = 2024-06-15 10:30:00 UTC
        assert_eq!(infer_date("1718447400000.jpg"), Some(dt("2024-06-15 10:30:00")));
    }

    #[test]
    fn no_date_in_name() {
        assert_eq!(infer_date("DSC_0001.jpg"), None);
    }

    #[test]
    fn invalid_date_rejected() {
        // month 13 is invalid
        assert_eq!(infer_date("20241332_photo.jpg"), None);
    }

    #[test]
    fn invalid_compact_bad_month() {
        assert_eq!(infer_date("20241300_103000.jpg"), None);
    }

    #[test]
    fn invalid_separated_bad_day() {
        assert_eq!(infer_date("2024-06-32_photo.jpg"), None);
    }

    #[test]
    fn no_extension() {
        assert_eq!(infer_date("IMG_20240615_103000"), Some(dt("2024-06-15 10:30:00")));
    }

    #[test]
    fn unix_9_digits_ignored() {
        assert_eq!(infer_date("171844380.jpg"), None);
    }
}
