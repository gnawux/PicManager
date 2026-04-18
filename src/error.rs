use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("metadata error: {0}")]
    Metadata(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_display() {
        let err = AppError::NotFound("photo 42".to_string());
        assert_eq!(err.to_string(), "not found: photo 42");
    }

    #[test]
    fn unsupported_format_display() {
        let err = AppError::UnsupportedFormat("bmp".to_string());
        assert!(err.to_string().contains("bmp"));
    }

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let app_err = AppError::from(io_err);
        assert!(app_err.to_string().contains("io error"));
    }
}
