use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Running | Self::Cancelled)
                | (Self::Running, Self::Paused | Self::Completed | Self::Failed | Self::Cancelled)
                | (Self::Paused, Self::Queued | Self::Cancelled)
                | (Self::Failed, Self::Queued | Self::Cancelled)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncItemStatus {
    Queued,
    Leased,
    Succeeded,
    Failed,
    Excluded,
    Cancelled,
}

impl SyncItemStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Leased => "leased",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Excluded => "excluded",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Leased | Self::Excluded | Self::Cancelled)
                | (Self::Leased, Self::Queued | Self::Succeeded | Self::Failed | Self::Cancelled)
                | (Self::Failed, Self::Queued | Self::Cancelled)
        )
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Excluded | Self::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_state_machine_allows_retry_but_not_reopen_completed() {
        assert!(JobStatus::Queued.can_transition_to(JobStatus::Running));
        assert!(JobStatus::Running.can_transition_to(JobStatus::Failed));
        assert!(JobStatus::Failed.can_transition_to(JobStatus::Queued));
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Queued));
    }

    #[test]
    fn item_state_machine_supports_lease_recovery() {
        assert!(SyncItemStatus::Queued.can_transition_to(SyncItemStatus::Leased));
        assert!(SyncItemStatus::Leased.can_transition_to(SyncItemStatus::Queued));
        assert!(SyncItemStatus::Leased.can_transition_to(SyncItemStatus::Succeeded));
        assert!(!SyncItemStatus::Succeeded.can_transition_to(SyncItemStatus::Queued));
    }

    #[test]
    fn terminal_item_states_are_explicit() {
        assert!(SyncItemStatus::Succeeded.is_terminal());
        assert!(SyncItemStatus::Excluded.is_terminal());
        assert!(SyncItemStatus::Cancelled.is_terminal());
        assert!(!SyncItemStatus::Failed.is_terminal());
    }
}
