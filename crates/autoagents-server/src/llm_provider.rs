//! LLM provider factory — creates an [`Arc<dyn LLMProvider>`] from
//! [`crate::config::ModelProvider`] entries, reading API keys from the
//! environment variables named in the config.
//!
//! Supported backends: MiniMax, DeepSeek.
//! GLM (Zhipu) will follow when the upstream `autoagents-llm` crate is updated.

use std::sync::Arc;

use autoagents_llm::LLMProvider;
use autoagents_llm::builder::LLMBuilder;

use crate::config::ModelProvider;

/// Build a single LLM provider from a [`ModelProvider`] config entry.
///
/// Returns `Err` if the named env var is unset/empty, or the provider is
/// unknown.
pub fn build_llm(cfg: &ModelProvider) -> Result<Arc<dyn LLMProvider>, String> {
    let api_key = std::env::var(&cfg.api_key_env).unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err(format!(
            "API key env var '{}' is not set or empty",
            cfg.api_key_env
        ));
    }

    match cfg.provider.as_str() {
        "minimax" => {
            use autoagents_llm::backends::minimax::MiniMax;
            let mut b = LLMBuilder::<MiniMax>::new().api_key(api_key);
            if let Some(ref url) = cfg.base_url {
                b = b.base_url(url);
            }
            b = b.model(&cfg.model);
            let provider: Arc<MiniMax> = b.build().map_err(|e| format!("MiniMax: {e}"))?;
            Ok(provider as Arc<dyn LLMProvider>)
        }
        "deepseek" => {
            use autoagents_llm::backends::deepseek::DeepSeek;
            let mut b = LLMBuilder::<DeepSeek>::new().api_key(api_key);
            if let Some(ref url) = cfg.base_url {
                b = b.base_url(url);
            }
            b = b.model(&cfg.model);
            let provider: Arc<DeepSeek> = b.build().map_err(|e| format!("DeepSeek: {e}"))?;
            Ok(provider as Arc<dyn LLMProvider>)
        }
        "glm" => Err(
            "GLM (Zhipu) backend is not yet available in the server crate; \
             it will be added when autoagents-llm publishes a GLM backend or \
             we add one here. For now, use MiniMax or DeepSeek."
                .to_string(),
        ),
        other => Err(format!(
            "unknown provider '{other}' — expected minimax, deepseek, or glm"
        )),
    }
}

/// Build the primary LLM for the given agent from the model config.
///
/// Returns `Err` if the agent has no entry in the config or the primary
/// provider's API key is not set.
pub fn build_primary_llm(
    agent_name: &str,
    models: &crate::config::ModelConfig,
) -> Result<Arc<dyn LLMProvider>, String> {
    let entry = models
        .agents
        .iter()
        .find(|a| a.name == agent_name)
        .ok_or_else(|| format!("no model config for agent '{agent_name}'"))?;
    build_llm(&entry.primary)
}
