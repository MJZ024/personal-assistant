//! Application configuration management with YAML loading and hot reload.

use serde::{Deserialize, Serialize};
use std::path::Path;

use autoagents_memory::MemoryConfig;
use autoagents_supervisor::SupervisorConfig;
use autoagents_tool_auth::ToolAuthConfig;

use crate::feishu::FeishuConfig;

/// Maximum file size in bytes for uploads (20MB).
const MAX_UPLOAD_SIZE: u64 = 20 * 1024 * 1024;

/// Root application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub feishu: FeishuConfig,
    pub supervisor: SupervisorConfig,
    pub memory: MemoryConfig,
    pub tool_auth: ToolAuthConfig,
    /// Model assignments and fallback rules for each agent.
    pub models: ModelConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            feishu: FeishuConfig::default(),
            supervisor: SupervisorConfig::default(),
            memory: MemoryConfig::default(),
            tool_auth: ToolAuthConfig::default(),
            models: ModelConfig::default(),
        }
    }
}

impl AppConfig {
    /// Load configuration from a YAML file.
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Path::new(path);
        if !config_path.exists() {
            log::warn!("Config file not found at {}, using defaults", path);
            let config = Self::default();
            let yaml = serde_yaml::to_string(&config)?;
            if let Some(parent) = config_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, yaml)?;
            restrict_file_perms(path);
            return Ok(config);
        }

        let contents = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&contents)?;
        // Re-tighten permissions in case the file was hand-edited world-readable.
        restrict_file_perms(path);
        Ok(config)
    }

    /// Reload configuration from file.
    pub fn reload(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        *self = serde_yaml::from_str(&contents)?;
        log::info!("Configuration reloaded from {}", path);
        Ok(())
    }
}

/// Model assignment configuration for each agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub agents: Vec<AgentModel>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            agents: vec![
                AgentModel {
                    name: "supervisor".to_string(),
                    primary: ModelProvider {
                        provider: "minimax".to_string(),
                        model: "abab6.5s-chat".to_string(),
                        api_key_env: "MINIMAX_KEY".to_string(),
                        base_url: None,
                    },
                    fallbacks: vec![
                        ModelProvider {
                            provider: "deepseek".to_string(),
                            model: "deepseek-chat".to_string(),
                            api_key_env: "DEEPSEEK_KEY".to_string(),
                            base_url: Some("https://api.deepseek.com".to_string()),
                        },
                        ModelProvider {
                            provider: "glm".to_string(),
                            model: "glm-4-flash".to_string(),
                            api_key_env: "GLM_KEY".to_string(),
                            base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
                        },
                    ],
                },
                AgentModel {
                    name: "coding".to_string(),
                    primary: ModelProvider {
                        provider: "minimax".to_string(),
                        model: "minimax-m1".to_string(),
                        api_key_env: "MINIMAX_KEY".to_string(),
                        base_url: None,
                    },
                    fallbacks: vec![ModelProvider {
                        provider: "deepseek".to_string(),
                        model: "deepseek-chat".to_string(),
                        api_key_env: "DEEPSEEK_KEY".to_string(),
                        base_url: Some("https://api.deepseek.com".to_string()),
                    }],
                },
                AgentModel {
                    name: "ops".to_string(),
                    primary: ModelProvider {
                        provider: "minimax".to_string(),
                        model: "abab6.5s-chat".to_string(),
                        api_key_env: "MINIMAX_KEY".to_string(),
                        base_url: None,
                    },
                    fallbacks: vec![ModelProvider {
                        provider: "deepseek".to_string(),
                        model: "deepseek-chat".to_string(),
                        api_key_env: "DEEPSEEK_KEY".to_string(),
                        base_url: Some("https://api.deepseek.com".to_string()),
                    }],
                },
            ],
        }
    }
}

/// Model assignment for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentModel {
    pub name: String,
    pub primary: ModelProvider,
    pub fallbacks: Vec<ModelProvider>,
}

/// Configuration for a single LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProvider {
    pub provider: String,
    pub model: String,
    pub api_key_env: String,
    pub base_url: Option<String>,
}

/// Tighten a file's permissions to owner-only (0600) on Unix.
///
/// The config file holds `app_secret`, `encrypt_key`, and `verification_token`,
/// so it must never be world- or group-readable. Errors are logged, not fatal:
/// the server can still start, but the operator is alerted.
fn restrict_file_perms(path: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            Ok(()) => {}
            Err(e) => log::warn!("Could not tighten permissions on {}: {}", path, e),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}
