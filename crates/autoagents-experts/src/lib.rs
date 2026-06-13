//! Expert agents for the personal assistant.
//!
//! Each expert is a specialized agent with a focused tool set and a
//! specific permission ceiling. They are designed to be spawned by the
//! supervisor agent and report results back via the topic pub/sub system.

mod coding;
mod ops;
mod path_policy;
pub mod redact;
pub mod sandbox;
mod tools;

pub use coding::CodingAgent;
pub use ops::OpsAgent;

use autoagents_core::agent::AgentDeriveT;
use autoagents_tool_auth::{AuditEntry, PermissionLevel, ToolAuthInterceptor};
use std::sync::Arc;

/// Record an audit entry for a tool execution (accountability trail).
///
/// `result` is a short tag such as `"allowed"` or `"blocked"`. Timestamps are
/// Unix seconds (no extra dependency); the entry is written via the shared
/// interceptor's audit logger.
pub(crate) fn record_audit(
    auth: &ToolAuthInterceptor,
    agent: &str,
    tool: &str,
    level: PermissionLevel,
    args: &str,
    result: &str,
) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    auth.audit(AuditEntry {
        timestamp: format!("{secs}"),
        agent_name: agent.into(),
        tool_name: tool.into(),
        permission_level: level,
        command_or_args: args.into(),
        result: result.into(),
        user_confirmed: false,
    });
}

/// Shared context that all expert agents receive when spawned.
pub struct ExpertContext {
    /// The tool authorization interceptor for permission checks.
    pub auth: Arc<ToolAuthInterceptor>,
    /// Working directory for file operations.
    pub working_dir: String,
    /// How strictly the coding agent's shell must be confined.
    pub sandbox: sandbox::SandboxPolicy,
}

/// Trait for expert agents that can be dispatched by the supervisor.
#[async_trait::async_trait]
pub trait ExpertAgent: AgentDeriveT + Send + Sync {
    /// The type of agent (used for routing).
    fn agent_type(&self) -> &'static str;

    /// Get the expert's permission ceiling.
    fn max_permission_level(&self) -> autoagents_tool_auth::PermissionLevel;

    /// Initialize the expert with context.
    async fn init(&mut self, ctx: Arc<ExpertContext>);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_audit_writes_entry() {
        let path = "/tmp/pa-test-audit-record.log";
        let _ = std::fs::remove_file(path);
        let mut cfg = autoagents_tool_auth::ToolAuthConfig::default();
        cfg.audit_log_path = path.into();
        let auth = ToolAuthInterceptor::new(cfg).expect("interceptor");

        record_audit(
            &auth,
            "coding",
            "shell_execute",
            PermissionLevel::System,
            "ls -la",
            "allowed",
        );

        let content = std::fs::read_to_string(path).expect("audit log written");
        assert!(content.contains("shell_execute"), "missing tool: {content}");
        assert!(content.contains("allowed"), "missing result: {content}");
        assert!(content.contains("coding"), "missing agent: {content}");
    }
}
