//! Permission level definitions and checking logic.

use serde::{Deserialize, Serialize};

/// Four-level tool permission system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PermissionLevel {
    /// Read-only operations, no side effects.
    Safe = 0,
    /// Write operations with reversible side effects.
    Write = 1,
    /// System-level operations requiring user confirmation.
    System = 2,
    /// Destructive operations requiring double confirmation + audit.
    Destructive = 3,
}

impl std::fmt::Display for PermissionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Safe => write!(f, "Safe"),
            Self::Write => write!(f, "Write"),
            Self::System => write!(f, "System"),
            Self::Destructive => write!(f, "Destructive"),
        }
    }
}

/// The result of a permission check.
#[derive(Debug, Clone)]
pub enum PermissionResult {
    /// Allowed immediately, no notification needed.
    Allowed,
    /// Allowed but the user should be notified after execution.
    AllowedWithNotice { message: String },
    /// Requires user confirmation before execution.
    RequiresConfirmation {
        tool_name: String,
        level: PermissionLevel,
        message: String,
    },
    /// Denied — the agent's permission cap is insufficient.
    Denied { reason: String },
}

impl PermissionResult {
    pub fn is_allowed(&self) -> bool {
        matches!(
            self,
            Self::Allowed | Self::AllowedWithNotice { .. } | Self::RequiresConfirmation { .. }
        )
    }

    pub fn needs_confirmation(&self) -> bool {
        matches!(self, Self::RequiresConfirmation { .. })
    }
}

/// Permission configuration for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPermissionConfig {
    pub agent_name: String,
    pub max_level: PermissionLevel,
}

/// Interface for tools to declare their permission level.
pub trait ToolPermission {
    fn permission_level(&self) -> PermissionLevel;
}

/// Permission check helper.
pub trait PermissionCheck {
    fn check_permission(
        &self,
        agent_cap: PermissionLevel,
    ) -> PermissionResult;
}
