use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    RetryWait,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::RetryWait => "retry_wait",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewJob {
    pub kind: String,
    pub payload_version: i64,
    pub payload: Value,
    pub priority: i64,
    pub max_attempts: i64,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
}

impl NewJob {
    pub fn new(kind: impl Into<String>, payload: Value) -> Self {
        Self {
            kind: kind.into(),
            payload_version: 1,
            payload,
            priority: 0,
            max_attempts: 3,
            correlation_id: None,
            idempotency_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub payload_version: i64,
    pub payload_json: String,
    pub status: String,
    pub priority: i64,
    pub progress_total: Option<i64>,
    pub progress_completed: i64,
    pub progress_stage: Option<String>,
    pub cancel_requested_at: Option<String>,
    pub max_attempts: i64,
    pub attempt_count: i64,
    pub next_run_at: Option<String>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub error_details_json: Option<String>,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub updated_at: String,
}

impl Job {
    pub fn payload(&self) -> serde_json::Result<Value> {
        serde_json::from_str(&self.payload_json)
    }
}

#[derive(Debug, Clone)]
pub struct EnqueueResult {
    pub job: Job,
    pub created: bool,
}

#[derive(Debug, Clone)]
pub struct JobLease {
    pub job: Job,
    pub attempt_id: i64,
    pub worker_id: String,
}

#[derive(Debug, Clone)]
pub struct JobFailure {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
    pub retryable: bool,
}

impl JobFailure {
    pub fn retryable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
            retryable: true,
        }
    }

    pub fn terminal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
            retryable: false,
        }
    }
}
