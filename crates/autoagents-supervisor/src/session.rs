//! Session and context management for the supervisor.
//!
//! Each user (identified by Feishu user_id) gets a session that maintains
//! their active task context and conversation state.

use std::collections::HashMap;

use autoagents_memory::TaskRecord;

/// Manages user sessions.
pub struct SessionManager {
    sessions: HashMap<String, Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Get or create a session for a user.
    pub fn get_or_create_session(&mut self, user_id: &str) -> &mut Session {
        self.sessions
            .entry(user_id.to_string())
            .or_insert_with(|| Session::new(user_id))
    }

    /// Get an existing session.
    pub fn get_session(&self, user_id: &str) -> Option<&Session> {
        self.sessions.get(user_id)
    }
}

/// A user session tracking their active conversation and task.
pub struct Session {
    pub user_id: String,
    /// The currently active task (for follow-up routing).
    pub active_task: Option<TaskRecord>,
    /// Conversation history for this session.
    pub message_count: usize,
    /// Timestamp of last interaction.
    pub last_interaction: chrono::DateTime<chrono::Local>,
}

impl Session {
    pub fn new(user_id: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            active_task: None,
            message_count: 0,
            last_interaction: chrono::Local::now(),
        }
    }

    /// Set the active task for follow-up routing.
    pub fn set_active_task(&mut self, task: TaskRecord) {
        self.active_task = Some(task);
    }

    /// Get the active task reference.
    pub fn get_active_task(&self) -> Option<&TaskRecord> {
        self.active_task.as_ref()
    }

    /// Record a new message interaction.
    pub fn record_message(&mut self) {
        self.message_count += 1;
        self.last_interaction = chrono::Local::now();
    }
}
