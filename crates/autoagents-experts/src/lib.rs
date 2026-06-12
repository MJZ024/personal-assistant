//! Expert agents for the personal assistant.
//!
//! Each expert is a specialized agent with a focused tool set and a
//! specific permission ceiling. They are designed to be spawned by the
//! supervisor agent and report results back via the topic pub/sub system.

mod coding;
mod ops;
mod tools;

pub use coding::CodingAgent;
pub use ops::OpsAgent;

use autoagents_core::agent::AgentDeriveT;
use autoagents_tool_auth::ToolAuthInterceptor;
use std::sync::Arc;

/// Shared context that all expert agents receive when spawned.
pub struct ExpertContext {
    /// The tool authorization interceptor for permission checks.
    pub auth: Arc<ToolAuthInterceptor>,
    /// Working directory for file operations.
    pub working_dir: String,
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
