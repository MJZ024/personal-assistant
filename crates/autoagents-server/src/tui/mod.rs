//! TUI mode — ratatui-based terminal UI for the personal assistant.
//!
//! Run with `personal-assistant tui`. Falls back to the plain REPL
//! when stdin is not a terminal.

mod app;
mod events;
mod ui;

use std::io::IsTerminal;
use std::sync::Arc;

use autoagents_memory::{Database, MemoryConfig};
use autoagents_supervisor::Supervisor;
use autoagents_tool_auth::ToolAuthInterceptor;

use crate::config::AppConfig;

/// Detect which provider actually has a key set and return a human-readable
/// description. Falls back to the primary config entry if nothing is set.
fn actual_model_desc(cfg: &AppConfig) -> String {
    let agent = match cfg.models.agents.iter().find(|a| a.name == "coding") {
        Some(a) => a,
        None => return "unknown".into(),
    };
    let candidates = std::iter::once(&agent.primary).chain(agent.fallbacks.iter());
    for p in candidates {
        if std::env::var(&p.api_key_env).map_or(false, |v| !v.trim().is_empty()) {
            return format!("{} · {}", p.provider, p.model);
        }
    }
    format!("{} · {}", agent.primary.provider, agent.primary.model)
}

/// Entry point for TUI mode.
pub async fn run_tui(app_config: AppConfig, auth: Arc<ToolAuthInterceptor>) {
    // ── Fallback: piped or non-interactive ──
    if !std::io::stdin().is_terminal() {
        eprintln!("stdin is not a terminal — falling back to REPL mode");
        crate::repl::run(app_config, auth).await;
        return;
    }

    // ── Subsystem setup (same as REPL) ──
    let memory = MemoryConfig {
        db_path: ":memory:".to_string(),
        ..Default::default()
    };
    let database = Arc::new(Database::open(&memory).expect("in-memory db"));

    let llm = match crate::llm_provider::build_any_llm("coding", &app_config.models) {
        Ok(llm) => llm,
        Err(e) => {
            eprintln!("Cannot start TUI: {e}");
            return;
        }
    };

    let supervisor = Arc::new(tokio::sync::Mutex::new(
        Supervisor::new(app_config.supervisor.clone(), database).with_chat_llm(llm.clone()),
    ));

    let expert_ctx = crate::runner::make_expert_context(
        Arc::clone(&auth),
        "/tmp/personal-assistant-workspace",
        autoagents_experts::sandbox::SandboxPolicy::Auto,
    );

    let model_desc = actual_model_desc(&app_config);

    // ── Terminal setup ──
    let mut stdout = std::io::stdout();
    if let Err(e) = crossterm::terminal::enable_raw_mode() {
        eprintln!("Failed to enable raw mode ({e}) — falling back to REPL");
        return crate::repl::run(app_config, auth).await;
    }
    if crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )
    .is_err()
    {
        let _ = crossterm::terminal::disable_raw_mode();
        eprintln!("Failed to enter alternate screen — falling back to REPL");
        return crate::repl::run(app_config, auth).await;
    }

    let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
    let mut terminal = match ratatui::Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture,
            );
            eprintln!("Failed to create terminal ({e}) — falling back to REPL");
            return crate::repl::run(app_config, auth).await;
        }
    };

    // ── Clear any residual content ──
    let _ = terminal.clear();

    // ── Run ──
    let app = app::TuiApp::new(model_desc);
    let result = events::event_loop(&mut terminal, app, supervisor, llm, expert_ctx).await;

    // ── Teardown (always restore, even on error) ──
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
    );

    if let Err(e) = result {
        eprintln!("TUI error: {e}");
    }
}
