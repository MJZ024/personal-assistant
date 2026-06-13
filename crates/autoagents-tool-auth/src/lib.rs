//! Tool permission framework for AutoAgents.
//!
//! Provides a four-level permission system (Safe/Write/System/Destructive)
//! that intercepts tool calls before execution, enforcing agent-specific
//! permission caps and optional user confirmation for high-risk operations.
//!
//! Also includes Shell command AST analysis with regex fallback for
//! variable expansion scenarios that static analysis cannot handle.

mod audit;
mod permissions;
mod shell;

pub use audit::{AuditEntry, AuditLogger};
pub use permissions::{
    AgentPermissionConfig, PermissionCheck, PermissionLevel, PermissionResult, ToolPermission,
};
pub use shell::{ShellAnalyzer, ShellDangerLevel};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for the tool authorization system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuthConfig {
    /// Agent-specific permission caps (agent_name -> max allowed level).
    pub agent_permissions: HashMap<String, PermissionLevel>,
    /// Shell command blacklist patterns (regex).
    pub shell_blacklist_patterns: Vec<String>,
    /// Shell command whitelist patterns (regex, overrides blacklist).
    pub shell_whitelist_patterns: Vec<String>,
    /// Path where audit logs are stored.
    pub audit_log_path: String,
}

impl Default for ToolAuthConfig {
    fn default() -> Self {
        let mut agent_permissions = HashMap::new();
        agent_permissions.insert("coding".into(), PermissionLevel::System);
        agent_permissions.insert("ops".into(), PermissionLevel::Destructive);
        agent_permissions.insert("document".into(), PermissionLevel::Write);
        agent_permissions.insert("information".into(), PermissionLevel::Safe);
        agent_permissions.insert("knowledge".into(), PermissionLevel::Write);
        agent_permissions.insert("supervisor".into(), PermissionLevel::System);

        Self {
            agent_permissions,
            shell_blacklist_patterns: vec![
                r"rm\s+(-[rRf]+\s+)*/".into(),
                r"mkfs\.".into(),
                r"dd\s+if=".into(),
                r">\s*/dev/sd".into(),
                r":\(\)\s*\{\s*:\|:&\s*\}\s*;:".into(), // fork bomb
            ],
            shell_whitelist_patterns: vec![],
            audit_log_path: "/opt/personal-assistant/audit/audit.log".into(),
        }
    }
}

/// Main tool authorization interceptor.
///
/// Called before every tool execution. Checks:
/// 1. Agent's permission cap
/// 2. Tool's declared risk level
/// 3. For shell commands: AST + regex analysis
/// 4. User confirmation for System+ operations
#[derive(Debug)]
pub struct ToolAuthInterceptor {
    config: ToolAuthConfig,
    shell_analyzer: ShellAnalyzer,
    audit_logger: AuditLogger,
}

impl ToolAuthInterceptor {
    pub fn new(config: ToolAuthConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let shell_analyzer = ShellAnalyzer::new(
            config.shell_blacklist_patterns.clone(),
            config.shell_whitelist_patterns.clone(),
        )?;
        let audit_logger = AuditLogger::new(&config.audit_log_path)?;

        Ok(Self {
            config,
            shell_analyzer,
            audit_logger,
        })
    }

    /// Check if an agent is allowed to execute a tool with a given permission level.
    /// Returns PermissionResult with details about whether confirmation is needed.
    pub fn check(
        &self,
        agent_name: &str,
        tool_name: &str,
        tool_level: PermissionLevel,
    ) -> PermissionResult {
        let agent_cap = self
            .config
            .agent_permissions
            .get(agent_name)
            .copied()
            .unwrap_or(PermissionLevel::Safe);

        if tool_level > agent_cap {
            return PermissionResult::Denied {
                reason: format!(
                    "Agent '{}' (cap: {:?}) cannot use tool '{}' (level: {:?})",
                    agent_name, agent_cap, tool_name, tool_level
                ),
            };
        }

        match tool_level {
            PermissionLevel::Safe => PermissionResult::Allowed,
            PermissionLevel::Write => PermissionResult::AllowedWithNotice {
                message: format!("Tool '{}' will write to the system.", tool_name),
            },
            PermissionLevel::System | PermissionLevel::Destructive => {
                PermissionResult::RequiresConfirmation {
                    tool_name: tool_name.to_string(),
                    level: tool_level,
                    message: format!(
                        "Tool '{}' requires {:?} level permission. Please confirm.",
                        tool_name, tool_level
                    ),
                }
            }
        }
    }

    /// Analyze a shell command and return its danger level.
    /// Combines AST analysis with regex-based pattern matching.
    pub fn analyze_shell_command(&self, command: &str) -> (ShellDangerLevel, Option<String>) {
        self.shell_analyzer.analyze(command)
    }

    /// Log an audit entry for a tool execution.
    pub fn audit(&self, entry: AuditEntry) {
        self.audit_logger.log(entry);
    }

    /// Reload configuration from YAML.
    pub fn reload_config(
        &mut self,
        config: ToolAuthConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.shell_analyzer = ShellAnalyzer::new(
            config.shell_blacklist_patterns.clone(),
            config.shell_whitelist_patterns.clone(),
        )?;
        self.config = config;
        Ok(())
    }
}

// AuditEntry is defined in audit.rs and re-exported above.
