//! Event loop — multiplexes crossterm input, agent results, and the render tick.

use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use autoagents_supervisor::Supervisor;

use super::app::TuiApp;

/// Run the main event loop. Blocks until the user types /quit or Ctrl+D.
pub async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    mut app: TuiApp,
    supervisor: Arc<tokio::sync::Mutex<Supervisor>>,
    llm: Arc<dyn autoagents_llm::LLMProvider>,
    expert_ctx: Arc<autoagents_experts::ExpertContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50));

    // Channel: agent task → TUI (sends the final result string)
    let (result_tx, mut result_rx) = mpsc::channel::<String>(4);

    loop {
        tokio::select! {
            // ── crossterm events ──
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        if handle_event(event, &mut app, &supervisor,
                                       &llm, &expert_ctx, &result_tx).await? {
                            break; // /quit or Ctrl+D
                        }
                    }
                    _ => break,
                }
            }

            // ── Agent result ──
            Some(msg) = result_rx.recv() => {
                app.push_agent(msg);
                app.agent_running = false;
                app.status = format!("{} | idle", app.model_desc);
            }

            // ── Render tick ──
            _ = tick.tick() => {
                terminal.draw(|f| super::ui::render(f, &app))?;
            }
        }
    }

    Ok(())
}

/// Process a single crossterm event. Returns `Ok(true)` when the user wants to exit.
async fn handle_event(
    event: Event,
    app: &mut TuiApp,
    supervisor: &Arc<tokio::sync::Mutex<Supervisor>>,
    llm: &Arc<dyn autoagents_llm::LLMProvider>,
    expert_ctx: &Arc<autoagents_experts::ExpertContext>,
    result_tx: &mpsc::Sender<String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    match event {
        Event::Key(key) => {
            // Ctrl+D → exit
            if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(true);
            }
            // Esc → exit
            if key.code == KeyCode::Esc {
                return Ok(true);
            }

            if key.code == KeyCode::Enter {
                return handle_enter(app, supervisor, llm, expert_ctx, result_tx).await;
            }

            // Text input (only when agent is not running)
            if !app.agent_running {
                match key.code {
                    KeyCode::Char(c) => {
                        app.input_char(c);
                    }
                    KeyCode::Backspace => {
                        app.input_backspace();
                    }
                    KeyCode::Delete => {
                        app.input_delete();
                    }
                    KeyCode::Left => {
                        app.cursor_left();
                    }
                    KeyCode::Right => {
                        app.cursor_right();
                    }
                    KeyCode::Home => {
                        app.cursor_home();
                    }
                    KeyCode::End => {
                        app.cursor_end();
                    }
                    _ => {}
                }
            }
        }
        Event::Resize(_, _) => {
            app.needs_resize = true;
        }
        _ => {}
    }
    Ok(false)
}

/// User pressed Enter — dispatch the current input.
async fn handle_enter(
    app: &mut TuiApp,
    supervisor: &Arc<tokio::sync::Mutex<Supervisor>>,
    llm: &Arc<dyn autoagents_llm::LLMProvider>,
    expert_ctx: &Arc<autoagents_experts::ExpertContext>,
    result_tx: &mpsc::Sender<String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let text = app.submit_input();
    if text.is_empty() {
        return Ok(false);
    }

    // Built-in commands
    if text == "/quit" || text == "/exit" {
        return Ok(true);
    }

    // ── Route through supervisor ──
    let response = {
        let mut sup = supervisor.lock().await;
        sup.handle_message("terminal", &text).await
    };

    match response {
        Ok(resp) => {
            if let (Some(ttype), Some(desc)) = (resp.task_type, resp.description) {
                // New task → spawn agent in background
                app.status = format!("{} | {} agent running…", app.model_desc, ttype);
                app.agent_running = true;
                app.push_system(&format!("dispatching to {ttype} agent: {desc}"));

                let llm_clone = Arc::clone(llm);
                let ctx_clone = Arc::clone(expert_ctx);
                let tx = result_tx.clone();
                let ttype_owned = ttype.to_string();
                let desc_owned = desc.to_string();

                tokio::spawn(async move {
                    let outcome = crate::runner::run_expert_sync(
                        &ttype_owned,
                        &desc_owned,
                        llm_clone,
                        ctx_clone,
                    )
                    .await;
                    let _ = tx
                        .send(match outcome {
                            Ok(r) => r,
                            Err(e) => format!("Agent error: {e}"),
                        })
                        .await;
                });
            } else {
                // Direct response (Query/Command) → show immediately
                app.push_agent(resp.message);
            }
        }
        Err(e) => {
            app.push_system(&format!("Error: {e}"));
        }
    }

    Ok(false)
}
