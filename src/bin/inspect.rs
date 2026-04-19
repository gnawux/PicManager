use exif::{In, Reader, Tag};
use picmanager::metadata::filename::infer_date;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: inspect <file> [file...]");
        std::process::exit(1);
    }
    for path in &args {
        inspect(Path::new(path));
        println!();
    }
}

fn inspect(path: &Path) {
    println!("=== {} ===", path.display());

    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => { println!("  open error: {e}"); return; }
    };
    let exif_result = Reader::new().read_from_container(&mut BufReader::new(file));

    match &exif_result {
        Err(e) => println!("  EXIF read error: {e}"),
        Ok(exif) => {
            println!("--- All EXIF fields ---");
            let mut fields: Vec<_> = exif.fields().collect();
            fields.sort_by_key(|f| format!("{:?}", f.tag));
            for f in fields {
                if f.ifd_num == In::PRIMARY {
                    println!("  {:40} = {}", format!("{:?}", f.tag), f.display_value());
                }
            }
        }
    }

    println!("--- Date inference ---");
    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

    let (date_source, date_value) = if let Ok(exif) = &exif_result {
        probe_exif_date(exif)
    } else {
        (None, None)
    };

    match date_source {
        Some(src) => println!("  Source : EXIF / {src}"),
        None => {
            match infer_date(filename) {
                Some(dt) => println!("  Source : filename / {filename}\n  Date   : {dt}"),
                None => println!("  Source : none → library/unknown/"),
            }
            return;
        }
    }
    if let Some(dt) = date_value {
        println!("  Date   : {dt}");
    }
}

fn probe_exif_date(exif: &exif::Exif) -> (Option<&'static str>, Option<String>) {
    use chrono::NaiveDateTime;

    let candidates: &[(&str, Tag)] = &[
        ("DateTimeOriginal", Tag::DateTimeOriginal),
        ("DateTimeDigitized", Tag::DateTimeDigitized),
        ("DateTime", Tag::DateTime),
    ];

    for (name, tag) in candidates {
        if let Some(field) = exif.get_field(*tag, In::PRIMARY) {
            let s = field.display_value().to_string();
            if NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").is_ok() {
                return (Some(name), Some(s));
            }
        }
    }

    // GPS fallback
    if let (Some(date_f), Some(time_f)) = (
        exif.get_field(Tag::GPSDateStamp, In::PRIMARY),
        exif.get_field(Tag::GPSTimeStamp, In::PRIMARY),
    ) {
        let date_str = date_f.display_value().to_string();
        let date_str = date_str.trim_matches('"');
        use chrono::NaiveDate;
        let date = NaiveDate::parse_from_str(date_str, "%Y:%m:%d")
            .or_else(|_| NaiveDate::parse_from_str(date_str, "%Y-%m-%d"))
            .ok();
        if let Some(d) = date {
            let time_str = time_f.display_value().to_string();
            return (Some("GPS DateStamp+TimeStamp"), Some(format!("{d} (time: {time_str})")));
        }
    }

    (None, None)
}
