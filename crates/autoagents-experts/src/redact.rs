//! Best-effort secret redaction for tool outputs.
//!
//! The sandbox and [`crate::path_policy`] are the primary defence — they stop
//! the agents from reaching `/opt/personal-assistant/.env`, `~/.ssh`, etc. at
//! all. But a project the coding agent legitimately works on may contain its
//! own secrets (a checked-in `.env`, a config with `api_key:`). This module
//! masks obvious secret patterns in tool output *before* it enters the LLM
//! context, so a value the agent is allowed to read is not echoed back to the
//! LLM provider or the Feishu chat.
//!
//! Redaction is intentionally conservative: it targets well-known
//! secret-bearing shapes (`KEY=…`, `Bearer …`, `sk-…`) and leaves everything
//! else untouched. It cannot catch every secret format — never rely on it as
//! the only layer.

use regex::Regex;
use std::sync::OnceLock;

/// Compiled (pattern, replacement) rules. Compiled once, reused.
fn rules() -> &'static [(Regex, &'static str)] {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        vec![
            // `KEY=value`, `api_key: value`, `PASSWORD=…` — case-insensitive.
            // Captures the key + separator so only the value is masked.
            // The secret word must end the token (no trailing letters), so
            // `KEYBOARD=` / `MY_KEYWORD=` are left alone.
            (
                Regex::new(
                    r"(?i)((?:[A-Z0-9_]*(?:KEY|TOKEN|SECRET|PASSWORD|PASSWD|CREDENTIAL))\s*[:=]\s*)\S+",
                )
                .unwrap(),
                "${1}[REDACTED]",
            ),
            // `Bearer <token>`
            (
                Regex::new(r"(?i)(Bearer\s+)\S+").unwrap(),
                "${1}[REDACTED]",
            ),
            // OpenAI-style keys `sk-…`
            (
                Regex::new(r"sk-[A-Za-z0-9_-]{20,}").unwrap(),
                "sk-[REDACTED]",
            ),
        ]
    })
}

/// Mask known secret patterns in `input`, returning a new string.
///
/// Replaces the *value* only, preserving the key/separator so context stays
/// readable (e.g. `GLM_KEY=abc123` ⟶ `GLM_KEY=[REDACTED]`).
pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for (re, repl) in rules() {
        out = re.replace_all(&out, *repl).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_env_style_secret_values() {
        let out = redact_secrets("GLM_KEY=abc123 DEEPSEEK_KEY=xyz");
        assert!(out.contains("GLM_KEY=[REDACTED]"));
        assert!(out.contains("DEEPSEEK_KEY=[REDACTED]"));
        assert!(!out.contains("abc123"));
        assert!(!out.contains("xyz"));
    }

    #[test]
    fn masks_yaml_style_and_case_insensitive() {
        let out = redact_secrets("api_key: secret456\npassword: hunter2");
        assert!(out.contains("api_key: [REDACTED]"));
        assert!(out.contains("password: [REDACTED]"));
        assert!(!out.contains("secret456"));
        assert!(!out.contains("hunter2"));
    }

    #[test]
    fn masks_bearer_and_openai_keys() {
        let out = redact_secrets("Authorization: Bearer eyJhb.x.y\nsk-proj-1234567890abcdefghij");
        assert!(out.contains("Bearer [REDACTED]"));
        assert!(out.contains("sk-[REDACTED]"));
        assert!(!out.contains("eyJhb"));
        assert!(!out.contains("1234567890abcdefghij"));
    }

    #[test]
    fn leaves_non_secret_text_untouched() {
        let out = redact_secrets("the quick brown fox KEYBOARD=mechanical count=42");
        assert!(out.contains("the quick brown fox"));
        assert!(out.contains("KEYBOARD=mechanical"));
        assert!(out.contains("count=42"));
    }

    #[test]
    fn handles_empty_and_no_match() {
        assert_eq!(redact_secrets(""), "");
        assert_eq!(redact_secrets("plain text, no secrets"), "plain text, no secrets");
    }
}
