use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// An external sensor connected via ANT+ during the activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorInfo {
    pub sensor_type: String,           // "heart_rate", "bike_power", "bike_cadence", etc.
    pub name: Option<String>,          // product_name or garmin_product
    pub manufacturer: Option<String>,
    pub battery_level: Option<i64>,   // 0–100 (not always available)
    pub battery_status: Option<String>, // "good", "ok", "low", "critical"
}

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
    pub sensors: Vec<SensorInfo>,
    pub track_points: Vec<TrackPoint>,
}

/// Intermediate accumulator for a single device_info record.
#[derive(Debug, Default, Clone)]
struct DeviceRaw {
    source_type: Option<String>,
    manufacturer: Option<String>,
    garmin_product: Option<String>,
    product_name: Option<String>,
    software_version: Option<f64>,
    antplus_device_type: Option<String>,
    battery_status: Option<String>,
    battery_level: Option<i64>,
}

fn merge_device_raw(dst: &mut DeviceRaw, src: DeviceRaw) {
    // Prefer the first non-None value for each field.
    macro_rules! take {
        ($field:ident) => {
            if dst.$field.is_none() { dst.$field = src.$field; }
        };
    }
    take!(source_type);
    take!(manufacturer);
    take!(garmin_product);
    take!(product_name);
    take!(software_version);
    take!(antplus_device_type);
    take!(battery_status);
    take!(battery_level);
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
        sensors: vec![],
        track_points,
    })
}

pub fn parse_fit(path: &Path) -> Result<ActivityData> {
    let mut file = std::fs::File::open(path).context("open fit")?;
    let records = fitparser::from_reader(&mut file).context("parse fit")?;

    let mut activity_type = "other".to_string();
    let title: Option<String> = None;
    let mut duration_seconds: Option<i64> = None;
    let mut distance_meters: Option<f64> = None;
    let mut avg_heart_rate: Option<i64> = None;
    let mut max_heart_rate: Option<i64> = None;
    let mut calories: Option<i64> = None;
    // device_index → DeviceRaw (accumulate across duplicate device_info messages)
    let mut devices: std::collections::HashMap<String, DeviceRaw> = std::collections::HashMap::new();
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
                // Activity.event is always "activity" (a FIT protocol enum), not a user title.
                // FIT files don't embed user-editable names; skip this field.
            }
            fitparser::profile::MesgNum::DeviceInfo => {
                let mut idx = String::new();
                let mut raw = DeviceRaw::default();
                for field in record.fields() {
                    match field.name() {
                        "device_index" => {
                            idx = match field.value() {
                                fitparser::Value::String(s) => s.clone(),
                                fitparser::Value::UInt8(0) => "creator".to_string(),
                                fitparser::Value::UInt8(n) => n.to_string(),
                                v => format!("{v:?}"),
                            };
                        }
                        "source_type" => {
                            if let fitparser::Value::String(s) = field.value() {
                                raw.source_type = Some(s.clone());
                            }
                        }
                        "manufacturer" => {
                            if let fitparser::Value::String(s) = field.value() {
                                raw.manufacturer = Some(s.clone());
                            }
                        }
                        "garmin_product" => {
                            if let fitparser::Value::String(s) = field.value() {
                                if !s.is_empty() { raw.garmin_product = Some(s.clone()); }
                            }
                        }
                        "product_name" => {
                            if let fitparser::Value::String(s) = field.value() {
                                if !s.is_empty() { raw.product_name = Some(s.clone()); }
                            }
                        }
                        "software_version" => {
                            raw.software_version = value_as_f64(field.value());
                        }
                        "antplus_device_type" => {
                            if let fitparser::Value::String(s) = field.value() {
                                if !s.is_empty() { raw.antplus_device_type = Some(s.clone()); }
                            }
                        }
                        "battery_status" => {
                            if let fitparser::Value::String(s) = field.value() {
                                raw.battery_status = Some(s.clone());
                            }
                        }
                        "battery_level" => {
                            raw.battery_level = value_as_f64(field.value()).map(|f| f as i64);
                        }
                        _ => {}
                    }
                }
                if !idx.is_empty() {
                    let entry = devices.entry(idx).or_insert_with(DeviceRaw::default);
                    merge_device_raw(entry, raw);
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

    // Extract main device name from "creator" (device_index 0) entry.
    // Prefer product_name > garmin_product; append firmware version if available.
    let creator = devices.get("creator")
        .or_else(|| devices.values().find(|d| {
            d.source_type.as_deref() == Some("local") && d.garmin_product.is_some()
        }));
    let device = creator.and_then(|d| {
        let base = d.product_name.clone()
            .or_else(|| d.garmin_product.as_ref().map(|p| format_product_name(p, d.manufacturer.as_deref())));
        base.map(|b| match d.software_version {
            Some(v) => format!("{b} (v{v:.2})"),
            None => b,
        })
    });

    // Collect external ANT+ sensors (deduplicated by device_index already).
    let mut sensors: Vec<SensorInfo> = devices.values()
        .filter(|d| d.source_type.as_deref() == Some("antplus"))
        .filter(|d| d.antplus_device_type.is_some())
        .map(|d| {
            let name = d.product_name.clone()
                .or_else(|| d.garmin_product.as_ref().map(|p| format_product_name(p, d.manufacturer.as_deref())));
            SensorInfo {
                sensor_type: d.antplus_device_type.clone().unwrap_or_default(),
                name,
                manufacturer: d.manufacturer.clone(),
                battery_level: d.battery_level,
                battery_status: d.battery_status.clone(),
            }
        })
        .collect();
    // Stable ordering: sort by sensor_type so output is deterministic.
    sensors.sort_by(|a, b| a.sensor_type.cmp(&b.sensor_type));

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
        sensors,
        track_points,
    })
}

/// Format a raw FIT product name into a human-readable string.
/// "fenix7" → "Fenix 7", "edge_explore2" → "Edge Explore 2"
fn format_product_name(raw: &str, manufacturer: Option<&str>) -> String {
    let words: Vec<String> = raw.split(|c: char| c == '_' || c == ' ')
        .filter(|s| !s.is_empty())
        .map(|part| {
            // Insert space before digit runs: "fenix7" → "Fenix 7"
            let mut out = String::new();
            let mut prev_alpha = false;
            for (i, c) in part.chars().enumerate() {
                if i == 0 {
                    out.extend(c.to_uppercase());
                } else if c.is_ascii_digit() && prev_alpha {
                    out.push(' ');
                    out.push(c);
                } else {
                    out.push(c);
                }
                prev_alpha = c.is_alphabetic();
            }
            out
        })
        .collect();
    let product = words.join(" ");
    // Prepend manufacturer if it's a recognizable brand and the product doesn't already start with it.
    match manufacturer {
        Some(m) if !m.is_empty() && !m.eq_ignore_ascii_case("garmin") => {
            let brand: String = {
                let mut s = m.to_string();
                if let Some(c) = s.get_mut(0..1) { c.make_ascii_uppercase(); }
                s
            };
            format!("{brand} {product}")
        }
        _ => product,
    }
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

    #[test]
    fn parse_fit_real_extracts_device_and_sensors() {
        let fit_path = std::path::Path::new(
            "/Volumes/PLATEA/Pictures/PicManager/.activities/2025/521040895_ACTIVITY.fit"
        );
        if !fit_path.exists() { return; }
        let data = parse_fit(fit_path).unwrap();
        // Device should be non-empty (Edge Explore 2)
        println!("device: {:?}", data.device);
        assert!(data.device.is_some(), "device should be extracted");
        // Should have at least one external sensor (HR or power)
        println!("sensors: {:?}", data.sensors);
        assert!(!data.sensors.is_empty(), "sensors should be non-empty for this cycling file");
        // Heart rate sensor should be present
        assert!(data.sensors.iter().any(|s| s.sensor_type == "heart_rate"));
    }
