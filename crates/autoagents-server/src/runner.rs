//! Agent runner — glues the supervisor's intent classification to the actual
//! expert-agent execution loop, creating a ReActAgent backed by an LLM and
//! equipped with the expert's tool set.
//!
//! This is the bridge between "the bot knows what you want" (supervisor) and
//! "the bot does the work" (expert agent + LLM + tools).
//!
//! It is designed to be called from [crate::feishu::events] after the
//! supervisor has classified the intent and created a task record. The runner
//! spawns the agent as a fire-and-report task: the Feishu callback returns
//! quickly (within the platform's ~3 s deadline), and the result is pushed
//! back later via `FeishuClient::send_text_message`.

use std::sync::Arc;

use autoagents_core::agent::prebuilt::executor::ReActAgent;
use autoagents_core::agent::task::Task;
use autoagents_core::agent::{AgentBuilder, AgentDeriveT, DirectAgent};
use autoagents_llm::LLMProvider;

use autoagents_experts::{CodingAgent, ExpertAgent, ExpertContext, OpsAgent};
use autoagents_tool_auth::ToolAuthInterceptor;

/// An active (or about-to-be-started) agent run.
pub struct RunRequest {
    pub task_id: String,
    pub task_type: String,
    pub description: String,
    pub context: Arc<ExpertContext>,
    pub llm: Arc<dyn LLMProvider>,
}

/// Outcome of an agent run.
pub struct RunOutcome {
    pub task_id: String,
    pub response: String,
    pub success: bool,
}

/// Build an [`ExpertContext`] from a shared auth interceptor + working dir.
pub fn make_expert_context(
    auth: Arc<ToolAuthInterceptor>,
    working_dir: &str,
) -> Arc<ExpertContext> {
    Arc::new(ExpertContext {
        auth,
        working_dir: working_dir.to_string(),
    })
}

/// Spawn an expert agent run in a background tokio task, reporting the result
/// through `on_complete`.
///
/// This returns immediately so the caller (Feishu event handler) can respond
/// within the platform's deadline.
pub fn spawn_expert(req: RunRequest, on_complete: impl FnOnce(RunOutcome) + Send + 'static) {
    tokio::spawn(async move {
        let outcome = run_expert(&req.task_type, &req.description, req.llm, req.context).await;
        let outcome = match outcome {
            Ok(response) => RunOutcome {
                task_id: req.task_id,
                response,
                success: true,
            },
            Err(e) => RunOutcome {
                task_id: req.task_id,
                response: format!("Agent execution failed: {e}"),
                success: false,
            },
        };
        on_complete(outcome);
    });
}

/// Run an expert agent **synchronously** (block the caller until done).
///
/// Used by the REPL so the terminal waits for the result instead of
/// receiving a pushed Feishu message.
pub async fn run_expert_sync(
    task_type: &str,
    description: &str,
    llm: Arc<dyn LLMProvider>,
    context: Arc<ExpertContext>,
) -> Result<String, RunnerError> {
    run_expert(task_type, description, llm, context).await
}

// ── internal dispatch ──

async fn run_expert(
    task_type: &str,
    description: &str,
    llm: Arc<dyn LLMProvider>,
    context: Arc<ExpertContext>,
) -> Result<String, RunnerError> {
    match task_type {
        "coding" => {
            let mut agent = CodingAgent::new();
            agent.init(context).await;
            let react = ReActAgent::new(agent);
            let handle = AgentBuilder::<_, DirectAgent>::new(react)
                .llm(llm)
                .build()
                .await
                .map_err(|e| RunnerError::Build(format!("{e}")))?;
            Ok(handle
                .agent
                .run(Task::new(description))
                .await
                .map_err(|e| RunnerError::Run(format!("{e}")))?
                .into())
        }
        "ops" => {
            let mut agent = OpsAgent::new();
            agent.init(context).await;
            let react = ReActAgent::new(agent);
            let handle = AgentBuilder::<_, DirectAgent>::new(react)
                .llm(llm)
                .build()
                .await
                .map_err(|e| RunnerError::Build(format!("{e}")))?;
            Ok(handle
                .agent
                .run(Task::new(description))
                .await
                .map_err(|e| RunnerError::Run(format!("{e}")))?
                .into())
        }
        other => Err(RunnerError::UnknownTaskType(other.into())),
    }
}

#[derive(Debug)]
pub enum RunnerError {
    UnknownTaskType(String),
    Build(String),
    Run(String),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTaskType(t) => write!(f, "unknown task type '{t}'"),
            Self::Build(e) => write!(f, "agent build failed: {e}"),
            Self::Run(e) => write!(f, "agent run failed: {e}"),
        }
    }
}
