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

    // Channel: agent task → TUI (sends final result)
    let (result_tx, mut result_rx) = mpsc::channel::<String>(4);
    // Channel: agent progress → TUI (tool names per turn, unbounded to
    // avoid backpressure blocking the agent loop)
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    loop {
        tokio::select! {
            // ── crossterm events ──
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        if handle_event(event, &mut app, &supervisor,
                                       &llm, &expert_ctx, &progress_tx, &result_tx).await? {
                            break; // /quit or Ctrl+D
                        }
                    }
                    _ => break,
                }
            }

            // ── Agent result ──
            Some(text) = result_rx.recv() => {
                if let Some(msg) = app.last_assistant_mut() {
                    if !text.is_empty() {
                        msg.push_text(&text);
                    }
                }
                app.agent_running = false;
                app.status = format!("{} | idle", app.model_desc);
            }

            // ── Agent progress (tool calls per turn) ──
            Some(tool_names) = progress_rx.recv() => {
                for name in tool_names.split(", ") {
                    if !name.is_empty() && name != "thinking…" {
                        if let Some(msg) = app.last_assistant_mut() {
                            msg.push_tool(name, true, "");
                        }
                    }
                }
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
    progress_tx: &tokio::sync::mpsc::UnboundedSender<String>,
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
                return handle_enter(app, supervisor, llm, expert_ctx, progress_tx, result_tx)
                    .await;
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
    progress_tx: &tokio::sync::mpsc::UnboundedSender<String>,
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
                app.begin_assistant(&ttype);

                let llm_clone = Arc::clone(llm);
                let ctx_clone = Arc::clone(expert_ctx);
                let tx = result_tx.clone();
                let ptx = progress_tx.clone();
                let ttype_owned = ttype.to_string();
                let desc_owned = desc.to_string();

                tokio::spawn(async move {
                    let outcome = crate::runner::run_expert_with_progress(
                        &ttype_owned,
                        &desc_owned,
                        llm_clone,
                        ctx_clone,
                        ptx,
                    )
                    .await;
                    let _ = tx
                        .send(match outcome {
                            Ok(text) => text,
                            Err(e) => format!("Agent error: {e}"),
                        })
                        .await;
                });
            } else {
                // Direct response (Query/Command) → show immediately
                let mut msg = crate::tui::app::Message::assistant(
                    "supervisor",
                    &app.model_desc.split(" · ").last().unwrap_or("?"),
                );
                msg.push_text(&resp.message);
                app.messages.push(msg);
            }
        }
        Err(e) => {
            let mut msg = crate::tui::app::Message::assistant("system", "error");
            msg.parts.push(crate::tui::app::Part::System {
                text: format!("Error: {e}"),
            });
            app.messages.push(msg);
        }
    }

    Ok(false)
}
