use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum LogStatus {
    Imported,
    Skipped,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LogEntry {
    pub path: String,
    pub status: LogStatus,
    pub sha256: Option<String>,
    pub error: Option<String>,
    pub ts: String,
}

pub struct MigrationLog {
    path: PathBuf,
}

impl MigrationLog {
    pub fn open(path: PathBuf) -> Self {
        Self { path }
    }

    /// Returns source paths for entries with status Imported or Skipped.
    pub fn load_done_paths(&self) -> anyhow::Result<HashSet<PathBuf>> {
        if !self.path.exists() {
            return Ok(HashSet::new());
        }
        let file = std::fs::File::open(&self.path)?;
        let mut done = HashSet::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<LogEntry>(line) {
                if entry.status == LogStatus::Imported || entry.status == LogStatus::Skipped {
                    done.insert(PathBuf::from(&entry.path));
                }
            }
        }
        Ok(done)
    }

    /// Appends one entry as a JSON line (atomic write + flush).
    pub fn append(&self, entry: &LogEntry) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(entry)?;
        writeln!(file, "{}", line)?;
        file.flush()?;
        Ok(())
    }
}

pub fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_entry(path: &str, status: LogStatus) -> LogEntry {
        LogEntry {
            path: path.to_owned(),
            status,
            sha256: None,
            error: None,
            ts: "2026-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn log_append_and_load_done_paths() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("import.log");
        let log = MigrationLog::open(log_path);

        log.append(&make_entry("/photos/a.jpg", LogStatus::Imported)).unwrap();
        let done = log.load_done_paths().unwrap();
        assert!(done.contains(&PathBuf::from("/photos/a.jpg")));
        assert_eq!(done.len(), 1);
    }

    #[test]
    fn log_skipped_in_done_paths() {
        let dir = tempdir().unwrap();
        let log = MigrationLog::open(dir.path().join("import.log"));

        log.append(&make_entry("/photos/b.jpg", LogStatus::Skipped)).unwrap();
        let done = log.load_done_paths().unwrap();
        assert!(done.contains(&PathBuf::from("/photos/b.jpg")));
    }

    #[test]
    fn log_failed_not_in_done_paths() {
        let dir = tempdir().unwrap();
        let log = MigrationLog::open(dir.path().join("import.log"));

        log.append(&make_entry("/photos/c.jpg", LogStatus::Failed)).unwrap();
        let done = log.load_done_paths().unwrap();
        assert!(!done.contains(&PathBuf::from("/photos/c.jpg")));
        assert!(done.is_empty());
    }

    #[test]
    fn log_missing_file_returns_empty() {
        let dir = tempdir().unwrap();
        let log = MigrationLog::open(dir.path().join("nonexistent.log"));
        let done = log.load_done_paths().unwrap();
        assert!(done.is_empty());
    }

    #[test]
    fn log_multiple_entries_roundtrip() {
        let dir = tempdir().unwrap();
        let log = MigrationLog::open(dir.path().join("import.log"));

        log.append(&make_entry("/p/a.jpg", LogStatus::Imported)).unwrap();
        log.append(&make_entry("/p/b.jpg", LogStatus::Skipped)).unwrap();
        log.append(&make_entry("/p/c.jpg", LogStatus::Failed)).unwrap();
        log.append(&make_entry("/p/d.jpg", LogStatus::Imported)).unwrap();

        let done = log.load_done_paths().unwrap();
        assert_eq!(done.len(), 3);
        assert!(done.contains(&PathBuf::from("/p/a.jpg")));
        assert!(done.contains(&PathBuf::from("/p/b.jpg")));
        assert!(!done.contains(&PathBuf::from("/p/c.jpg")));
        assert!(done.contains(&PathBuf::from("/p/d.jpg")));
    }
}
