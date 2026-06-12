//! User preference storage.

use serde::{Deserialize, Serialize};

/// Categories of user preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreferenceCategory {
    /// Code style (indentation, naming, language defaults)
    CodeStyle,
    /// File paths and directory structure preferences
    PathPreference,
    /// Notification preferences (channel, frequency, format)
    Notification,
    /// Tool and workflow preferences
    Workflow,
    /// Server and environment configuration
    ServerConfig,
    /// Custom / uncategorized
    Custom(String),
}

impl PreferenceCategory {
    pub fn to_str(&self) -> String {
        match self {
            Self::CodeStyle => "code_style".into(),
            Self::PathPreference => "path_preference".into(),
            Self::Notification => "notification".into(),
            Self::Workflow => "workflow".into(),
            Self::ServerConfig => "server_config".into(),
            Self::Custom(s) => format!("custom_{}", s),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "code_style" => Self::CodeStyle,
            "path_preference" => Self::PathPreference,
            "notification" => Self::Notification,
            "workflow" => Self::Workflow,
            "server_config" => Self::ServerConfig,
            other => Self::Custom(other.replace("custom_", "")),
        }
    }
}

/// A single user preference entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preference {
    pub category: PreferenceCategory,
    pub key: String,
    pub value: String,
    pub source: Option<String>,
    pub confidence: f64,
    pub created_at: String,
    pub updated_at: String,
}

/// High-level preference store operations.
pub struct PreferenceStore;

impl PreferenceStore {
    /// Extract preferences from a conversation summary using LLM.
    /// This returns a prompt template, not the extraction itself.
    pub fn extraction_prompt() -> &'static str {
        "Based on the conversation above, extract any new or updated user preferences. \
         Focus on: code style, file path conventions, notification preferences, \
         workflow habits, and server configuration details. \
         Output as JSON: { \"preferences\": [{ \"category\": \"...\", \"key\": \"...\", \"value\": \"...\" }] }"
    }

    /// Prompt for checking if compression would lose critical preferences.
    pub fn compression_guard_prompt() -> &'static str {
        "Before compressing this conversation, list any user preferences, \
         pending decisions, or incomplete tasks that MUST be preserved. \
         If nothing critical exists, respond with 'CLEAR_TO_COMPRESS'."
    }
}
