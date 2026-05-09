use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TrackPoint {
    pub ts: DateTime<Utc>,
    pub lat: f64,
    pub lon: f64,
    pub elevation: Option<f64>,
    pub heart_rate: Option<i64>,
    pub cadence: Option<i64>,
    pub speed: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ActivityData {
    pub file_format: String,
    pub title: Option<String>,
    pub activity_type: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration_seconds: Option<i64>,
    pub distance_meters: Option<f64>,
    pub elevation_gain_meters: Option<f64>,
    pub avg_heart_rate: Option<i64>,
    pub max_heart_rate: Option<i64>,
    pub calories: Option<i64>,
    pub device: Option<String>,
    pub track_points: Vec<TrackPoint>,
}

pub fn parse_gpx(path: &Path) -> Result<ActivityData> {
    let file = std::fs::File::open(path).context("open gpx")?;
    let reader = std::io::BufReader::new(file);
    let gpx_data = gpx::read(reader).context("parse gpx")?;

    let title = gpx_data
        .metadata
        .as_ref()
        .and_then(|m| m.name.clone())
        .or_else(|| gpx_data.tracks.first().and_then(|t| t.name.clone()));

    let activity_type = gpx_data
        .tracks
        .first()
        .and_then(|t| t.type_.clone())
        .map(|s| gpx_type_to_activity(&s))
        .unwrap_or_else(|| "other".to_string());

    let mut track_points: Vec<TrackPoint> = Vec::new();
    for track in &gpx_data.tracks {
        for segment in &track.segments {
            for wpt in &segment.points {
                let pt = wpt.point();
                let lat = pt.y();
                let lon = pt.x();
                // gpx::Time wraps time::OffsetDateTime; format to string and parse via chrono
                let ts_utc = wpt.time.as_ref().and_then(|t| {
                    t.format().ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                });
                if let Some(ts) = ts_utc {
                    track_points.push(TrackPoint {
                        ts,
                        lat,
                        lon,
                        elevation: wpt.elevation,
                        heart_rate: None,
                        cadence: None,
                        speed: wpt.speed,
                    });
                }
            }
        }
    }

    let start_time = track_points.first().map(|p| p.ts);
    let end_time = track_points.last().map(|p| p.ts);
    let duration_seconds = match (start_time, end_time) {
        (Some(s), Some(e)) => Some((e - s).num_seconds()),
        _ => None,
    };
    let distance_meters = compute_distance_m(&track_points);
    let elevation_gain_meters = compute_elevation_gain(&track_points);

    Ok(ActivityData {
        file_format: "gpx".to_string(),
        title,
        activity_type,
        start_time,
        end_time,
        duration_seconds,
        distance_meters,
        elevation_gain_meters,
        avg_heart_rate: None,
        max_heart_rate: None,
        calories: None,
        device: None,
        track_points,
    })
}

pub fn parse_fit(path: &Path) -> Result<ActivityData> {
    let mut file = std::fs::File::open(path).context("open fit")?;
    let records = fitparser::from_reader(&mut file).context("parse fit")?;

    let mut activity_type = "other".to_string();
    let mut title: Option<String> = None;
    let mut duration_seconds: Option<i64> = None;
    let mut distance_meters: Option<f64> = None;
    let mut avg_heart_rate: Option<i64> = None;
    let mut max_heart_rate: Option<i64> = None;
    let mut calories: Option<i64> = None;
    let mut device: Option<String> = None;
    let mut track_points: Vec<TrackPoint> = Vec::new();

    for record in &records {
        match record.kind() {
            fitparser::profile::MesgNum::Session => {
                for field in record.fields() {
                    match field.name() {
                        "sport" => {
                            activity_type = fit_value_to_sport(field.value());
                        }
                        "total_elapsed_time" => {
                            duration_seconds = value_as_f64(field.value()).map(|f| f as i64);
                        }
                        "total_distance" => {
                            distance_meters = value_as_f64(field.value());
                        }
                        "avg_heart_rate" => {
                            avg_heart_rate = value_as_f64(field.value()).map(|f| f as i64);
                        }
                        "max_heart_rate" => {
                            max_heart_rate = value_as_f64(field.value()).map(|f| f as i64);
                        }
                        "total_calories" => {
                            calories = value_as_f64(field.value()).map(|f| f as i64);
                        }
                        _ => {}
                    }
                }
            }
            fitparser::profile::MesgNum::Activity => {
                for field in record.fields() {
                    if field.name() == "event" {
                        if let fitparser::Value::String(s) = field.value() {
                            title = Some(s.clone());
                        }
                    }
                }
            }
            fitparser::profile::MesgNum::DeviceInfo => {
                for field in record.fields() {
                    if field.name() == "product_name" {
                        if let fitparser::Value::String(s) = field.value() {
                            if !s.is_empty() {
                                device = Some(s.clone());
                            }
                        }
                    }
                }
            }
            fitparser::profile::MesgNum::Record => {
                let mut lat: Option<f64> = None;
                let mut lon: Option<f64> = None;
                let mut ts: Option<DateTime<Utc>> = None;
                let mut elevation: Option<f64> = None;
                let mut heart_rate: Option<i64> = None;
                let mut cadence: Option<i64> = None;
                let mut speed: Option<f64> = None;

                for field in record.fields() {
                    match field.name() {
                        "position_lat" => {
                            lat = value_as_f64(field.value())
                                .map(|v| v * (180.0 / 2_147_483_648.0));
                        }
                        "position_long" => {
                            lon = value_as_f64(field.value())
                                .map(|v| v * (180.0 / 2_147_483_648.0));
                        }
                        "timestamp" => {
                            ts = value_as_timestamp(field.value());
                        }
                        "altitude" | "enhanced_altitude" => {
                            if elevation.is_none() {
                                elevation = value_as_f64(field.value());
                            }
                        }
                        "heart_rate" => {
                            heart_rate = value_as_f64(field.value()).map(|v| v as i64);
                        }
                        "cadence" | "fractional_cadence" => {
                            if cadence.is_none() {
                                cadence = value_as_f64(field.value()).map(|v| v as i64);
                            }
                        }
                        "speed" | "enhanced_speed" => {
                            if speed.is_none() {
                                speed = value_as_f64(field.value());
                            }
                        }
                        _ => {}
                    }
                }

                if let (Some(lat), Some(lon), Some(ts)) = (lat, lon, ts) {
                    track_points.push(TrackPoint { ts, lat, lon, elevation, heart_rate, cadence, speed });
                }
            }
            _ => {}
        }
    }

    if track_points.is_empty() && duration_seconds.is_none() {
        anyhow::bail!("no usable data found in FIT file");
    }

    let start_time = track_points.first().map(|p| p.ts);
    let end_time = track_points.last().map(|p| p.ts);
    let elevation_gain_meters = compute_elevation_gain(&track_points);

    if distance_meters.is_none() {
        distance_meters = compute_distance_m(&track_points);
    }

    Ok(ActivityData {
        file_format: "fit".to_string(),
        title,
        activity_type,
        start_time,
        end_time,
        duration_seconds,
        distance_meters,
        elevation_gain_meters,
        avg_heart_rate,
        max_heart_rate,
        calories,
        device,
        track_points,
    })
}

fn fit_value_to_sport(v: &fitparser::Value) -> String {
    match v {
        fitparser::Value::String(s) => gpx_type_to_activity(s),
        fitparser::Value::Enum(n) => fit_sport_code_to_type(*n),
        _ => "other".to_string(),
    }
}

fn gpx_type_to_activity(s: &str) -> String {
    match s.to_lowercase().replace(['-', '_', ' '], "").as_str() {
        "running" | "run" | "9" => "running",
        "cycling" | "biking" | "bike" | "2" => "cycling",
        "hiking" | "hike" | "17" => "hiking",
        "walking" | "walk" | "11" => "walking",
        "trailrunning" | "35" => "trail_running",
        "swimming" | "swim" | "5" => "swimming",
        _ => "other",
    }
    .to_string()
}

fn fit_sport_code_to_type(code: u8) -> String {
    match code {
        1 => "running",
        2 => "cycling",
        5 => "swimming",
        11 => "walking",
        17 => "hiking",
        35 => "trail_running",
        _ => "other",
    }
    .to_string()
}

fn value_as_f64(v: &fitparser::Value) -> Option<f64> {
    match v {
        fitparser::Value::Float32(f) => Some(*f as f64),
        fitparser::Value::Float64(f) => Some(*f),
        fitparser::Value::UInt8(n) => Some(*n as f64),
        fitparser::Value::UInt16(n) => Some(*n as f64),
        fitparser::Value::UInt32(n) => Some(*n as f64),
        fitparser::Value::UInt64(n) => Some(*n as f64),
        fitparser::Value::SInt8(n) => Some(*n as f64),
        fitparser::Value::SInt16(n) => Some(*n as f64),
        fitparser::Value::SInt32(n) => Some(*n as f64),
        fitparser::Value::SInt64(n) => Some(*n as f64),
        _ => None,
    }
}

fn value_as_timestamp(v: &fitparser::Value) -> Option<DateTime<Utc>> {
    match v {
        fitparser::Value::Timestamp(dt) => Some(dt.with_timezone(&Utc)),
        _ => None,
    }
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let (lat1, lat2) = (lat1.to_radians(), lat2.to_radians());
    let dlon = (lon2 - lon1).to_radians();
    let dlat = lat2 - lat1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().asin()
}

fn compute_distance_m(points: &[TrackPoint]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let total: f64 = points
        .windows(2)
        .map(|w| haversine_m(w[0].lat, w[0].lon, w[1].lat, w[1].lon))
        .sum();
    if total > 0.0 { Some(total) } else { None }
}

fn compute_elevation_gain(points: &[TrackPoint]) -> Option<f64> {
    let gain: f64 = points
        .windows(2)
        .filter_map(|w| match (w[0].elevation, w[1].elevation) {
            (Some(e1), Some(e2)) if e2 > e1 => Some(e2 - e1),
            _ => None,
        })
        .sum();
    if gain > 0.0 { Some(gain) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_gpx(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::with_suffix(".gpx").unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    const SAMPLE_GPX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test" xmlns="http://www.topografix.com/GPX/1/1">
  <metadata><name>Morning Run</name></metadata>
  <trk>
    <name>Morning Run</name>
    <type>running</type>
    <trkseg>
      <trkpt lat="39.9000" lon="116.4000">
        <ele>50.0</ele>
        <time>2024-06-15T10:00:00Z</time>
      </trkpt>
      <trkpt lat="39.9010" lon="116.4010">
        <ele>55.0</ele>
        <time>2024-06-15T10:05:00Z</time>
      </trkpt>
      <trkpt lat="39.9020" lon="116.4020">
        <ele>52.0</ele>
        <time>2024-06-15T10:10:00Z</time>
      </trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    #[test]
    fn parse_gpx_basic() {
        let f = write_gpx(SAMPLE_GPX);
        let data = parse_gpx(f.path()).unwrap();
        assert_eq!(data.file_format, "gpx");
        assert_eq!(data.activity_type, "running");
        assert_eq!(data.track_points.len(), 3);
        assert!((data.track_points[0].lat - 39.9).abs() < 1e-4);
        assert!((data.track_points[0].lon - 116.4).abs() < 1e-4);
    }

    #[test]
    fn parse_gpx_title() {
        let f = write_gpx(SAMPLE_GPX);
        let data = parse_gpx(f.path()).unwrap();
        assert_eq!(data.title.as_deref(), Some("Morning Run"));
    }

    #[test]
    fn parse_gpx_duration() {
        let f = write_gpx(SAMPLE_GPX);
        let data = parse_gpx(f.path()).unwrap();
        assert_eq!(data.duration_seconds, Some(600)); // 10 minutes
    }

    #[test]
    fn parse_gpx_elevation_gain() {
        let f = write_gpx(SAMPLE_GPX);
        let data = parse_gpx(f.path()).unwrap();
        // gain: 50→55 = +5m (55→52 is descent, not counted)
        assert_eq!(data.elevation_gain_meters, Some(5.0));
    }

    #[test]
    fn parse_gpx_distance_nonzero() {
        let f = write_gpx(SAMPLE_GPX);
        let data = parse_gpx(f.path()).unwrap();
        assert!(data.distance_meters.unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn haversine_beijing_to_nearby() {
        let d = haversine_m(39.9, 116.4, 39.901, 116.401);
        assert!(d > 100.0 && d < 200.0);
    }

    #[test]
    fn fit_sport_code_running() {
        assert_eq!(fit_sport_code_to_type(1), "running");
    }

    #[test]
    fn fit_sport_code_unknown() {
        assert_eq!(fit_sport_code_to_type(99), "other");
    }
}
