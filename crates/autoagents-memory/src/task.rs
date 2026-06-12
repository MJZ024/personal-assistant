//! Task record and state management.

use serde::{Deserialize, Serialize};

/// Task lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Created but not yet dispatched.
    Pending,
    /// Assigned to an expert agent, actively being worked on.
    InProgress,
    /// Agent completed the work, waiting for user confirmation.
    WaitingConfirmation,
    /// Task is blocked on something (user input, dependency, resource).
    Blocked,
    /// Task completed successfully.
    Completed,
    /// Task failed or was cancelled.
    Failed,
    /// Scheduled recurring task.
    Scheduled,
}

impl TaskStatus {
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::WaitingConfirmation => "waiting_confirmation",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Scheduled => "scheduled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "in_progress" => Self::InProgress,
            "waiting_confirmation" => Self::WaitingConfirmation,
            "blocked" => Self::Blocked,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "scheduled" => Self::Scheduled,
            _ => Self::Pending,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Pending | Self::InProgress | Self::WaitingConfirmation | Self::Blocked | Self::Scheduled)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// Task priority level.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// A record in the tasks table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: i32,
    pub agent_type: Option<String>,
    pub parent_task_id: Option<String>,
    pub progress_pct: i32,
    pub blocking_reason: Option<String>,
    pub next_action: Option<String>,
    pub result_summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

impl TaskRecord {
    /// Create a new pending task.
    pub fn new(id: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            description: description.to_string(),
            status: TaskStatus::Pending,
            priority: 1, // Normal
            agent_type: None,
            parent_task_id: None,
            progress_pct: 0,
            blocking_reason: None,
            next_action: None,
            result_summary: None,
            created_at: String::new(),
            updated_at: String::new(),
            completed_at: None,
        }
    }
}

/// Dependency between two tasks (task X depends on task Y).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDependency {
    pub task_id: String,
    pub depends_on_task_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_to_from_str() {
        assert_eq!(TaskStatus::from_str("pending"), TaskStatus::Pending);
        assert_eq!(TaskStatus::from_str("in_progress"), TaskStatus::InProgress);
        assert_eq!(TaskStatus::from_str("completed"), TaskStatus::Completed);
        assert_eq!(TaskStatus::from_str("failed"), TaskStatus::Failed);
        assert_eq!(TaskStatus::Pending.to_str(), "pending");
        assert_eq!(TaskStatus::InProgress.to_str(), "in_progress");
    }

    #[test]
    fn test_task_status_is_active() {
        assert!(TaskStatus::Pending.is_active());
        assert!(TaskStatus::InProgress.is_active());
        assert!(TaskStatus::WaitingConfirmation.is_active());
        assert!(TaskStatus::Blocked.is_active());
        assert!(TaskStatus::Scheduled.is_active());
        assert!(!TaskStatus::Completed.is_active());
        assert!(!TaskStatus::Failed.is_active());
    }

    #[test]
    fn test_task_status_is_terminal() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(!TaskStatus::Pending.is_terminal());
        assert!(!TaskStatus::InProgress.is_terminal());
    }

    #[test]
    fn test_task_record_new() {
        let task = TaskRecord::new("t1", "test task");
        assert_eq!(task.id, "t1");
        assert_eq!(task.description, "test task");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.priority, 1);
        assert_eq!(task.progress_pct, 0);
    }

    #[test]
    fn test_task_priority_ordering() {
        assert!(TaskPriority::Low < TaskPriority::Normal);
        assert!(TaskPriority::Normal < TaskPriority::High);
        assert!(TaskPriority::High < TaskPriority::Critical);
    }
}
