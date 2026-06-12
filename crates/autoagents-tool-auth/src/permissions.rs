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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_allowed_for_all_agents() {
        // Any agent can execute Safe tools
        for cap in &[PermissionLevel::Safe, PermissionLevel::Write, PermissionLevel::System, PermissionLevel::Destructive] {
            assert!(*cap >= PermissionLevel::Safe);
        }
    }

    #[test]
    fn test_write_requires_write_cap() {
        assert!(PermissionLevel::Write >= PermissionLevel::Write);
        assert!(PermissionLevel::System >= PermissionLevel::Write);
        assert!(!(PermissionLevel::Safe >= PermissionLevel::Write));
    }

    #[test]
    fn test_system_requires_system_cap() {
        assert!(!(PermissionLevel::Write >= PermissionLevel::System));
        assert!(PermissionLevel::System >= PermissionLevel::System);
        assert!(PermissionLevel::Destructive >= PermissionLevel::System);
    }

    #[test]
    fn test_destructive_requires_destructive_cap() {
        assert!(!(PermissionLevel::System >= PermissionLevel::Destructive));
        assert!(PermissionLevel::Destructive >= PermissionLevel::Destructive);
    }

    #[test]
    fn test_permission_result_allowed() {
        let r = PermissionResult::Allowed;
        assert!(r.is_allowed());
        assert!(!r.needs_confirmation());
    }

    #[test]
    fn test_permission_result_needs_confirmation() {
        let r = PermissionResult::RequiresConfirmation {
            tool_name: "test".into(),
            level: PermissionLevel::System,
            message: "confirm".into(),
        };
        assert!(r.is_allowed());
        assert!(r.needs_confirmation());
    }

    #[test]
    fn test_permission_result_denied() {
        let r = PermissionResult::Denied { reason: "nope".into() };
        assert!(!r.is_allowed());
        assert!(!r.needs_confirmation());
    }

    #[test]
    fn test_permission_level_display() {
        assert_eq!(format!("{}", PermissionLevel::Safe), "Safe");
        assert_eq!(format!("{}", PermissionLevel::Write), "Write");
        assert_eq!(format!("{}", PermissionLevel::System), "System");
        assert_eq!(format!("{}", PermissionLevel::Destructive), "Destructive");
    }

    #[test]
    fn test_permission_level_ordering() {
        assert!(PermissionLevel::Safe < PermissionLevel::Write);
        assert!(PermissionLevel::Write < PermissionLevel::System);
        assert!(PermissionLevel::System < PermissionLevel::Destructive);
    }
}
