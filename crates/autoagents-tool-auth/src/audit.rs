//! Audit logging for tool executions.

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;

use super::PermissionLevel;

/// Logger for tool execution audit entries.
pub struct AuditLogger {
    writer: Mutex<BufWriter<File>>,
}

impl std::fmt::Debug for AuditLogger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditLogger").finish()
    }
}

impl AuditLogger {
    pub fn new(log_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let path = Path::new(log_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).append(true).open(path)?;

        // The audit log records every command the agents run, so restrict it
        // to the owner (0600 on Unix) even though it is created by the parent.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }

        Ok(Self {
            writer: Mutex::new(BufWriter::new(file)),
        })
    }

    pub fn log(&self, entry: AuditEntry) {
        if let Ok(mut writer) = self.writer.lock() {
            let line = serde_json::to_string(&entry).unwrap_or_default();
            let _ = writeln!(writer, "{}", line);
            let _ = writer.flush();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub agent_name: String,
    pub tool_name: String,
    pub permission_level: PermissionLevel,
    pub command_or_args: String,
    pub result: String,
    pub user_confirmed: bool,
}
