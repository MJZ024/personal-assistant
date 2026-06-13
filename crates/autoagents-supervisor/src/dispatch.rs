//! Task dispatching to expert agents.

use std::collections::HashMap;
use std::sync::Arc;

use autoagents_tool_auth::ToolAuthInterceptor;

use super::SupervisorError;

/// Manages expert agent instances and routes tasks to them.
pub struct ExpertDispatcher {
    /// Active expert instances (task_id -> expert agent).
    // In practice, these would be agent handles with tokio tasks.
    active_experts: HashMap<String, ExpertHandle>,
    /// Shared tool auth interceptor.
    auth: Option<Arc<ToolAuthInterceptor>>,
}

/// A handle to a running expert agent.
struct ExpertHandle {
    task_id: String,
    agent_type: String,
    // In full implementation: tokio::task::JoinHandle<()>, channel sender, etc.
}

impl ExpertDispatcher {
    pub fn new() -> Self {
        Self {
            active_experts: HashMap::new(),
            auth: None,
        }
    }

    /// Set the tool auth interceptor for permission checks.
    pub fn with_auth(mut self, auth: Arc<ToolAuthInterceptor>) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Dispatch a task to the appropriate expert agent.
    pub async fn dispatch(
        &mut self,
        task_id: &str,
        task_type: &str,
        description: &str,
    ) -> Result<(), SupervisorError> {
        let handle = ExpertHandle {
            task_id: task_id.to_string(),
            agent_type: task_type.to_string(),
        };

        self.active_experts.insert(task_id.to_string(), handle);

        log::info!(
            "Dispatched task {} ({}) to {} expert",
            task_id,
            description,
            task_type
        );

        Ok(())
    }

    /// Send a follow-up message to a running expert.
    pub async fn send_follow_up(
        &self,
        task_id: &str,
        message: &str,
    ) -> Result<(), SupervisorError> {
        if !self.active_experts.contains_key(task_id) {
            return Err(SupervisorError::DispatchError(format!(
                "No active expert for task {}",
                task_id
            )));
        }

        log::info!("Sent follow-up to task {}: {}", task_id, message);

        Ok(())
    }

    /// Clean up a completed expert instance.
    pub fn cleanup(&mut self, task_id: &str) {
        self.active_experts.remove(task_id);
    }
}

/// Result of a dispatched task.
#[derive(Debug, Clone)]
pub struct DispatchResult {
    pub task_id: String,
    pub expert_type: String,
    pub output: String,
    pub success: bool,
}
