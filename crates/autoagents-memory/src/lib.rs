//! Persistent memory layer for the personal assistant.
//!
//! Provides SQLite-backed storage for:
//! - Task state management (status, progress, blocking conditions)
//! - User preferences and learned habits
//! - Audit logs placeholder
//! - Heartbeat scheduler with WAL checkpointing

mod db;
mod heartbeat;
mod task;
mod preferences;

pub use db::Database;
pub use heartbeat::{Heartbeat, HeartbeatConfig};
pub use task::{TaskRecord, TaskStatus, TaskPriority, TaskDependency};
pub use preferences::{Preference, PreferenceCategory, PreferenceStore};

use serde::{Deserialize, Serialize};

/// Top-level configuration for the memory layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub db_path: String,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval_secs: u64,
    /// WAL checkpoint interval in seconds.
    pub wal_checkpoint_interval_secs: u64,
    /// Backup directory for daily snapshots.
    pub backup_dir: String,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            db_path: "/opt/personal-assistant/tasks.db".into(),
            heartbeat_interval_secs: 300, // 5 minutes
            wal_checkpoint_interval_secs: 3600, // 1 hour
            backup_dir: "/opt/personal-assistant/backups".into(),
        }
    }
}
