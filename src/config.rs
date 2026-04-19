use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub library_path: PathBuf,
    pub db_path: PathBuf,
    pub host: String,
    pub port: u16,
    pub thumb_size: u32,
}

/// Subset that can be overridden via config file.
#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    library_path: Option<PathBuf>,
    host: Option<String>,
    port: Option<u16>,
    thumb_size: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        let library_path = default_library_path();
        let db_path = library_path.join("picmanager.db");
        Self {
            library_path,
            db_path,
            host: "127.0.0.1".to_string(),
            port: 8080,
            thumb_size: 300,
        }
    }
}

impl Config {
    /// Load defaults, then overlay values from `~/.config/picmanager/config.toml` if present.
    pub fn load() -> Self {
        let mut cfg = Self::default();
        if let Some(file_cfg) = load_file_config() {
            if let Some(p) = file_cfg.library_path {
                cfg.library_path = p.clone();
                cfg.db_path = p.join("picmanager.db");
            }
            if let Some(h) = file_cfg.host   { cfg.host = h; }
            if let Some(p) = file_cfg.port   { cfg.port = p; }
            if let Some(s) = file_cfg.thumb_size { cfg.thumb_size = s; }
        }
        cfg
    }

    pub fn db_url(&self) -> String {
        format!("sqlite:{}", self.db_path.display())
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn load_file_config() -> Option<FileConfig> {
    let path = dirs::config_dir()?.join("picmanager/config.toml");
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

fn default_library_path() -> PathBuf {
    dirs::picture_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("PicManager")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn default_port_is_8080() {
        assert_eq!(Config::default().port, 8080);
    }

    #[test]
    fn default_library_path_ends_with_picmanager() {
        let cfg = Config::default();
        assert_eq!(cfg.library_path.file_name().unwrap(), "PicManager");
    }

    #[test]
    fn db_url_starts_with_sqlite() {
        assert!(Config::default().db_url().starts_with("sqlite:"));
    }

    #[test]
    fn bind_addr_format() {
        assert_eq!(Config::default().bind_addr(), "127.0.0.1:8080");
    }

    #[test]
    fn db_path_is_inside_library_path() {
        let cfg = Config::default();
        assert!(cfg.db_path.starts_with(&cfg.library_path));
    }

    #[test]
    fn file_config_overrides_port_and_host() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "port = 9090\nhost = \"0.0.0.0\"").unwrap();
        let text = std::fs::read_to_string(f.path()).unwrap();
        let fc: FileConfig = toml::from_str(&text).unwrap();
        assert_eq!(fc.port, Some(9090));
        assert_eq!(fc.host.as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn file_config_overrides_thumb_size() {
        let text = "thumb_size = 512\n";
        let fc: FileConfig = toml::from_str(text).unwrap();
        assert_eq!(fc.thumb_size, Some(512));
    }

    #[test]
    fn empty_file_config_keeps_defaults() {
        let fc: FileConfig = toml::from_str("").unwrap();
        assert!(fc.port.is_none());
        assert!(fc.host.is_none());
    }
}
