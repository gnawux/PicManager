use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub library_path: PathBuf,
    pub db_path: PathBuf,
    pub host: String,
    pub port: u16,
    pub thumb_size: u32,
}

impl Default for Config {
    fn default() -> Self {
        let library_path = dirs_base_path();
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
    pub fn db_url(&self) -> String {
        format!("sqlite:{}", self.db_path.display())
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn dirs_base_path() -> PathBuf {
    dirs::picture_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("PicManager")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_is_8080() {
        let cfg = Config::default();
        assert_eq!(cfg.port, 8080);
    }

    #[test]
    fn default_library_path_ends_with_picmanager() {
        let cfg = Config::default();
        assert_eq!(cfg.library_path.file_name().unwrap(), "PicManager");
    }

    #[test]
    fn db_url_starts_with_sqlite() {
        let cfg = Config::default();
        assert!(cfg.db_url().starts_with("sqlite:"));
    }

    #[test]
    fn bind_addr_format() {
        let cfg = Config::default();
        assert_eq!(cfg.bind_addr(), "127.0.0.1:8080");
    }

    #[test]
    fn db_path_is_inside_library_path() {
        let cfg = Config::default();
        assert!(cfg.db_path.starts_with(&cfg.library_path));
    }
}
