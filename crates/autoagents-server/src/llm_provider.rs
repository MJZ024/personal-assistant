//! LLM provider factory — builds an [`Arc<dyn LLMProvider>`] from
//! [`crate::config::ModelProvider`] entries, reading API keys from the
//! environment variables named in the config.
//!
//! All backends that have built-in support in `autoagents-llm` are enabled
//! (features in Cargo.toml).  GLM (Zhipu) is proxied through the OpenAI
//! backend since its API is compatible.

use std::sync::Arc;

use autoagents_llm::LLMProvider;
use autoagents_llm::builder::LLMBuilder;

use crate::config::ModelProvider;

/// Macro to avoid 12 boilerplate branches.
///
/// Each branch is `let provider: Arc<ConcreteType> = …build()?; provider as Arc<dyn LLMProvider>`.
macro_rules! build {
    ($cfg:expr, $key:expr, $type:ty, $label:literal) => {{
        let mut b = LLMBuilder::<$type>::new().api_key($key);
        if let Some(ref u) = $cfg.base_url {
            b = b.base_url(u);
        }
        b = b.model(&$cfg.model);
        let provider: Arc<$type> = b.build().map_err(|e| format!("{}: {e}", $label))?;
        Ok(provider as Arc<dyn LLMProvider>)
    }};
}

/// Build a single LLM provider from a [`ModelProvider`] config entry.
pub fn build_llm(cfg: &ModelProvider) -> Result<Arc<dyn LLMProvider>, String> {
    let api_key = std::env::var(&cfg.api_key_env).unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err(format!(
            "API key env var '{}' is not set or empty",
            cfg.api_key_env
        ));
    }

    // Built-in backends — one per provider string.
    match cfg.provider.as_str() {
        // ── Chinese providers ──
        "minimax" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::minimax::MiniMax,
            "MiniMax"
        ),
        "deepseek" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::deepseek::DeepSeek,
            "DeepSeek"
        ),
        // Zhipu (glm) has an OpenAI-compatible API, so reuse the OpenAI backend.
        "glm" => build_glm(cfg, api_key),
        // ── Global leaders ──
        "openai" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::openai::OpenAI,
            "OpenAI"
        ),
        "anthropic" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::anthropic::Anthropic,
            "Anthropic"
        ),
        // ── Speed / routing / local ──
        "groq" => build!(cfg, api_key, autoagents_llm::backends::groq::Groq, "Groq"),
        "openrouter" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::openrouter::OpenRouter,
            "OpenRouter"
        ),
        "ollama" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::ollama::Ollama,
            "Ollama"
        ),
        // ── Other cloud providers ──
        "google" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::google::Google,
            "Google (Gemini)"
        ),
        "xai" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::xai::XAI,
            "xAI (Grok)"
        ),
        "phind" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::phind::Phind,
            "Phind"
        ),
        "azure_openai" => build!(
            cfg,
            api_key,
            autoagents_llm::backends::azure_openai::AzureOpenAI,
            "Azure OpenAI"
        ),
        // ── Catch-all ──
        other => Err(format!("unknown provider '{other}'")),
    }
}

/// GLM (Zhipu) via the OpenAI-compatible backend.
fn build_glm(cfg: &ModelProvider, api_key: String) -> Result<Arc<dyn LLMProvider>, String> {
    let url = cfg
        .base_url
        .clone()
        .unwrap_or_else(|| "https://open.bigmodel.cn/api/paas/v4/".into());
    let mut b = LLMBuilder::<autoagents_llm::backends::openai::OpenAI>::new()
        .api_key(api_key)
        .base_url(&url)
        .model(&cfg.model);
    // GLM does not require an org id.
    let provider: Arc<autoagents_llm::backends::openai::OpenAI> =
        b.build().map_err(|e| format!("GLM: {e}"))?;
    Ok(provider as Arc<dyn LLMProvider>)
}

/// Build the primary LLM for the given agent from the model config.
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

/// Try the primary provider for `agent_name`, then each fallback in order,
/// returning the first whose API key is set. Falls back to primary's error
/// if nothing works.
pub fn build_any_llm(
    agent_name: &str,
    models: &crate::config::ModelConfig,
) -> Result<Arc<dyn LLMProvider>, String> {
    let entry = models
        .agents
        .iter()
        .find(|a| a.name == agent_name)
        .ok_or_else(|| format!("no model config for agent '{agent_name}'"))?;

    let providers = std::iter::once(&entry.primary).chain(entry.fallbacks.iter());
    for cfg in providers {
        if let Ok(llm) = build_llm(cfg) {
            return Ok(llm);
        }
    }
    // Everything failed — return the primary's error for clarity.
    build_llm(&entry.primary)
}
