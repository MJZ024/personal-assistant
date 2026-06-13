//! Supervisor agent — the central coordinator for the personal assistant.
//!
//! Responsibilities:
//! - Intent classification (new task vs follow-up)
//! - Task decomposition and expert dispatch
//! - Session context management with long-term memory
//! - Queue dispatch (wake on completion, not just heartbeat)
//! - Permission relay to user via Feishu

mod dispatch;
mod intent;
mod session;

pub use dispatch::{DispatchResult, ExpertDispatcher};
pub use intent::{Intent, IntentClassifier};
pub use session::{Session, SessionManager};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use autoagents_memory::{Database, Heartbeat, HeartbeatConfig, TaskRecord, TaskStatus};

/// Configuration for the supervisor agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorConfig {
    pub max_concurrent_tasks: usize,
    pub heartbeat_interval_secs: u64,
    pub wal_checkpoint_interval_secs: u64,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 2,
            heartbeat_interval_secs: 300,
            wal_checkpoint_interval_secs: 3600,
        }
    }
}

/// The supervisor agent maintains overall state and orchestrates all experts.
pub struct Supervisor {
    config: SupervisorConfig,
    database: Arc<Database>,
    session_manager: SessionManager,
    dispatcher: ExpertDispatcher,
    /// How many tasks are currently executing.
    active_count: usize,
    /// Queue of pending task IDs waiting for a slot.
    pending_queue: Vec<String>,
}

impl Supervisor {
    pub fn new(config: SupervisorConfig, database: Arc<Database>) -> Self {
        Self {
            config,
            database,
            session_manager: SessionManager::new(),
            dispatcher: ExpertDispatcher::new(),
            active_count: 0,
            pending_queue: Vec::new(),
        }
    }

    /// Process an incoming user message.
    /// Returns the response that should be sent back to the user.
    pub async fn handle_message(
        &mut self,
        user_id: &str,
        message: &str,
    ) -> Result<SupervisorResponse, SupervisorError> {
        // 1. Check if this is a follow-up to an existing task
        let session = self.session_manager.get_or_create_session(user_id);
        let existing_task = session.get_active_task().cloned();

        // 2. Classify intent
        let intent = self
            .classify_intent(message, existing_task.as_ref())
            .await?;

        match intent {
            Intent::NewTask {
                task_type,
                description,
                priority,
            } => {
                self.dispatch_new_task(user_id, &task_type, &description, priority)
                    .await
            }
            Intent::FollowUp { task_id, message } => {
                self.handle_follow_up(&task_id, &message).await
            }
            Intent::Command { command } => self.handle_command(user_id, &command).await,
            Intent::Query { question } => self.handle_query(&question).await,
        }
    }

    /// Classify the user's intent (delegates to LLM via IntentClassifier).
    async fn classify_intent(
        &self,
        message: &str,
        active_task: Option<&TaskRecord>,
    ) -> Result<Intent, SupervisorError> {
        IntentClassifier::classify(message, active_task)
            .await
            .map_err(|e| SupervisorError::IntentError(e))
    }

    /// Dispatch a new task to an expert agent.
    async fn dispatch_new_task(
        &mut self,
        user_id: &str,
        task_type: &str,
        description: &str,
        priority: i32,
    ) -> Result<SupervisorResponse, SupervisorError> {
        let task_id = Uuid::new_v4().to_string();

        // Create task record
        let task = TaskRecord {
            id: task_id.clone(),
            description: description.to_string(),
            status: TaskStatus::Pending,
            priority,
            agent_type: Some(task_type.to_string()),
            parent_task_id: None,
            progress_pct: 0,
            blocking_reason: None,
            next_action: Some("Waiting for dispatch slot".into()),
            result_summary: None,
            created_at: String::new(),
            updated_at: String::new(),
            completed_at: None,
        };
        self.database.upsert_task(&task)?;

        // Check concurrency limit
        if self.active_count >= self.config.max_concurrent_tasks {
            // Queue the task
            self.pending_queue.push(task_id.clone());
            return Ok(SupervisorResponse {
                message: format!(
                    "收到任务「{}」，当前有 {} 个任务正在执行，已加入队列等待。",
                    description, self.active_count
                ),
                task_id: Some(task_id.clone()),
                task_type: Some(task_type.to_string()),
                description: Some(description.to_string()),
                requires_confirmation: false,
            });
        }

        // Dispatch immediately
        self.start_task_execution(task_id.clone(), description, task_type)
            .await?;

        Ok(SupervisorResponse {
            message: format!(
                "收到任务「{}」，已分配给{}Agent开始处理。",
                description,
                task_type_to_display(task_type)
            ),
            task_id: Some(task_id),
            task_type: Some(task_type.to_string()),
            description: Some(description.to_string()),
            requires_confirmation: false,
        })
    }

    /// Start executing a task by dispatching to the appropriate expert.
    async fn start_task_execution(
        &mut self,
        task_id: String,
        description: &str,
        task_type: &str,
    ) -> Result<(), SupervisorError> {
        self.active_count += 1;

        // Update task status
        let mut task = self
            .database
            .get_task(&task_id)?
            .ok_or(SupervisorError::TaskNotFound(task_id.clone()))?;
        task.status = TaskStatus::InProgress;
        task.agent_type = Some(task_type.to_string());
        self.database.upsert_task(&task)?;

        self.dispatcher
            .dispatch(&task_id, task_type, description)
            .await?;

        Ok(())
    }

    /// Handle a follow-up message to an existing task.
    async fn handle_follow_up(
        &self,
        task_id: &str,
        message: &str,
    ) -> Result<SupervisorResponse, SupervisorError> {
        let task = self
            .database
            .get_task(task_id)?
            .ok_or(SupervisorError::TaskNotFound(task_id.to_string()))?;

        // Route the follow-up to the expert agent
        self.dispatcher.send_follow_up(task_id, message).await?;

        Ok(SupervisorResponse {
            message: "已收到，正在处理...".to_string(),
            task_id: Some(task_id.to_string()),
            task_type: None,
            description: None,
            requires_confirmation: false,
        })
    }

    /// Handle a slash command (e.g., /status, /reload).
    async fn handle_command(
        &self,
        user_id: &str,
        command: &str,
    ) -> Result<SupervisorResponse, SupervisorError> {
        match command.trim() {
            "/status" => self.get_status(),
            "/help" => Ok(SupervisorResponse {
                message: "可用命令:\n/status - 查看系统状态\n/help - 显示帮助\n/reload - 重新加载配置\n/tasks - 查看当前任务".into(),
                task_id: None,
                task_type: None,
            description: None,
            requires_confirmation: false,
            }),
            "/tasks" => self.list_active_tasks(),
            _ => Ok(SupervisorResponse {
                message: format!("未知命令: {}。输入 /help 查看可用命令。", command),
                task_id: None,
                task_type: None,
            description: None,
            requires_confirmation: false,
            }),
        }
    }

    /// Handle a general query.
    async fn handle_query(&self, question: &str) -> Result<SupervisorResponse, SupervisorError> {
        Ok(SupervisorResponse {
            message: format!("收到你的问题，我来处理: {}", question),
            task_id: None,
            task_type: None,
            description: None,
            requires_confirmation: false,
        })
    }

    /// Get current system status.
    fn get_status(&self) -> Result<SupervisorResponse, SupervisorError> {
        let active_tasks = self.database.get_active_tasks()?;
        let msg = format!(
            "系统状态: 活跃任务 {} 个，排队任务 {} 个，并发上限 {}",
            self.active_count,
            self.pending_queue.len(),
            self.config.max_concurrent_tasks,
        );

        let detail: Vec<String> = active_tasks
            .iter()
            .map(|t| {
                format!(
                    "- [{}] {} ({})",
                    t.status.to_str(),
                    t.description,
                    t.agent_type.as_deref().unwrap_or("unknown")
                )
            })
            .collect();

        Ok(SupervisorResponse {
            message: format!("{}\n\n当前任务:\n{}", msg, detail.join("\n")),
            task_id: None,
            task_type: None,
            description: None,
            requires_confirmation: false,
        })
    }

    /// List active tasks.
    fn list_active_tasks(&self) -> Result<SupervisorResponse, SupervisorError> {
        let tasks = self.database.get_active_tasks()?;
        if tasks.is_empty() {
            return Ok(SupervisorResponse {
                message: "当前没有活跃任务。".into(),
                task_id: None,
                task_type: None,
                description: None,
                requires_confirmation: false,
            });
        }

        let lines: Vec<String> = tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "{}. [{}] {} (进度: {}%)",
                    i + 1,
                    t.id[..8].to_string(),
                    t.description,
                    t.progress_pct
                )
            })
            .collect();

        Ok(SupervisorResponse {
            message: format!("当前任务:\n{}", lines.join("\n")),
            task_id: None,
            task_type: None,
            description: None,
            requires_confirmation: false,
        })
    }

    /// Called when an expert agent completes a task.
    /// Updates the task record and triggers queue dispatch.
    pub async fn on_expert_task_complete(
        &mut self,
        task_id: &str,
        result: &str,
        success: bool,
    ) -> Result<(), SupervisorError> {
        self.active_count = self.active_count.saturating_sub(1);

        let mut task = self
            .database
            .get_task(task_id)?
            .ok_or(SupervisorError::TaskNotFound(task_id.to_string()))?;

        task.status = if success {
            TaskStatus::Completed
        } else {
            TaskStatus::Failed
        };
        task.result_summary = Some(result.to_string());
        self.database.upsert_task(&task)?;

        // Dispatch next queued task immediately
        self.dispatch_queue().await?;

        Ok(())
    }

    /// Dispatch the next task from the pending queue.
    /// Called both on heartbeat and on expert completion (immediate).
    pub async fn dispatch_queue(&mut self) -> Result<(), SupervisorError> {
        while self.active_count < self.config.max_concurrent_tasks {
            if let Some(task_id) = self.pending_queue.pop() {
                let task = self
                    .database
                    .get_task(&task_id)?
                    .ok_or_else(|| SupervisorError::TaskNotFound(task_id.clone()))?;

                let task_type = task
                    .agent_type
                    .clone()
                    .unwrap_or_else(|| "coding".to_string());
                self.start_task_execution(task_id, &task.description, &task_type)
                    .await?;
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Recovery: check if in-progress tasks still have live tokio tasks.
    /// Called on startup after crash.
    pub async fn recover_after_restart(&mut self) -> Result<String, SupervisorError> {
        let in_progress = self.database.get_in_progress_tasks()?;
        let pending = self.database.get_pending_tasks()?;

        let interrupted = in_progress.len();
        let waiting = pending.len();

        // Reset in-progress tasks to pending for re-dispatch
        for mut task in in_progress {
            task.status = TaskStatus::Pending;
            task.blocking_reason = Some("Interrupted by restart, will re-dispatch.".into());
            self.database.upsert_task(&task)?;
            self.pending_queue.push(task.id);
        }

        // Dispatch queued tasks
        self.dispatch_queue().await?;

        Ok(format!(
            "我回来了。发现 {} 个中断的任务已重新排队，{} 个等待中的任务。",
            interrupted, waiting
        ))
    }
}

fn task_type_to_display(task_type: &str) -> &str {
    match task_type {
        "coding" => "编程",
        "ops" => "运维",
        "document" => "文档",
        "information" => "信息",
        "knowledge" => "知识库",
        _ => "通用",
    }
}

/// Response from the supervisor to be sent back to the user.
#[derive(Debug, Clone)]
pub struct SupervisorResponse {
    pub message: String,
    pub task_id: Option<String>,
    /// Classified task type ("coding", "ops", …) — set for NewTask.
    pub task_type: Option<String>,
    /// Original user description — set for NewTask.
    pub description: Option<String>,
    pub requires_confirmation: bool,
}

/// Errors that can occur in supervisor operations.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] Box<dyn std::error::Error + Send + Sync>),

    #[error("Dispatch error: {0}")]
    DispatchError(String),

    #[error("Intent classification error: {0}")]
    IntentError(String),
}

impl From<rusqlite::Error> for SupervisorError {
    fn from(e: rusqlite::Error) -> Self {
        Self::DatabaseError(Box::new(e))
    }
}
