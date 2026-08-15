use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceErrorCode {
    NotFound,
    InvalidInput,
    Conflict,
    Unauthorized,
    Busy,
    Retryable,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceError {
    pub code: ServiceErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

pub type ServiceResult<T> = std::result::Result<T, ServiceError>;

impl ServiceError {
    pub fn new(code: ServiceErrorCode, message: impl Into<String>) -> Self {
        let retryable = matches!(code, ServiceErrorCode::Busy | ServiceErrorCode::Retryable);
        Self {
            code,
            message: message.into(),
            retryable,
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ServiceError {}

impl From<AppError> for ServiceError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::NotFound(message) => Self::new(ServiceErrorCode::NotFound, message),
            AppError::UnsupportedFormat(message) | AppError::Metadata(message) => {
                Self::new(ServiceErrorCode::InvalidInput, message)
            }
            AppError::Database(_) | AppError::Migration(_) => Self::new(
                ServiceErrorCode::Retryable,
                "The catalog operation could not be completed",
            ),
            AppError::Io(_) => Self::new(
                ServiceErrorCode::Retryable,
                "The filesystem operation could not be completed",
            ),
            AppError::ModelNotFound(message) => Self::new(ServiceErrorCode::Conflict, message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_errors_are_retryable_without_exposing_internal_details() {
        let sqlx_error = sqlx::Error::RowNotFound;
        let error = ServiceError::from(AppError::Database(sqlx_error));

        assert_eq!(error.code, ServiceErrorCode::Retryable);
        assert!(error.retryable);
        assert!(!error.message.contains("row"));
    }

    #[test]
    fn structured_details_are_additive() {
        let error = ServiceError::new(ServiceErrorCode::Conflict, "already running")
            .with_details(serde_json::json!({"job_id": 42}));
        assert_eq!(error.details.unwrap()["job_id"], 42);
    }
}
