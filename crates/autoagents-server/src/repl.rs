//! REPL mode — terminal-based chat with the personal assistant.
//!
//! Run with `personal-assistant repl`.  Reads one line at a time,
//! routes through the supervisor, and, for new tasks, runs the expert
//! agent **synchronously** (you wait for the result instead of getting a
//! pushed Feishu message).  Uses an in-memory SQLite database so it
//! never conflicts with a concurrently-running Feishu service.
//!
//! Exit with `/quit`, `/exit`, or Ctrl+D.

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use autoagents_memory::{Database, MemoryConfig};
use autoagents_supervisor::Supervisor;
use autoagents_tool_auth::ToolAuthInterceptor;

use crate::config::AppConfig;

/// Start the interactive REPL loop.
pub async fn run(app_config: AppConfig, auth: Arc<ToolAuthInterceptor>) {
    // ── Subsystems (in-memory, no conflict with Feishu service) ──
    let memory = MemoryConfig {
        db_path: ":memory:".to_string(),
        ..Default::default()
    };
    let database = Arc::new(Database::open(&memory).expect("in-memory db"));
    let llm = match crate::llm_provider::build_any_llm("coding", &app_config.models) {
        Ok(llm) => llm,
        Err(e) => {
            eprintln!("Cannot start REPL: {e}");
            return;
        }
    };

    let supervisor = Arc::new(tokio::sync::Mutex::new(
        Supervisor::new(app_config.supervisor.clone(), database).with_chat_llm(llm.clone()),
    ));

    let expert_ctx = crate::runner::make_expert_context(
        auth,
        "/tmp/personal-assistant-workspace",
        autoagents_experts::sandbox::SandboxPolicy::Auto,
    );

    println!("Personal Assistant REPL");
    println!("Model:  coding agent → {}", describe_model(&app_config));
    println!("Type /quit to exit, /help for commands.");
    println!();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        // Prompt
        print!("> ");
        let _ = stdout.flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // Ctrl+D
            Ok(_) => {}
            Err(e) => {
                eprintln!("read error: {e}");
                break;
            }
        }

        let input = line.trim().to_string();
        if input.is_empty() {
            continue;
        }

        // Built-in commands
        if input == "/quit" || input == "/exit" {
            println!("再见。");
            break;
        }
        if input == "/help" {
            println!("Commands:");
            println!("  /quit, /exit  退出");
            println!("  /help         显示此帮助");
            println!("  /status       查看系统状态");
            println!();
            continue;
        }
        if input == "/status" {
            println!("REPL 模式 — 使用内存数据库，独立于飞书服务。");
            println!();
            continue;
        }

        // ── Route through supervisor (same pipeline as Feishu) ──
        let response = {
            let mut sup = supervisor.lock().await;
            sup.handle_message("terminal", &input).await
        };

        match response {
            Ok(resp) => {
                println!("{}\n", resp.message);

                // If a new task was dispatched, run the expert synchronously.
                if let (Some(ttype), Some(desc)) = (resp.task_type, resp.description) {
                    print!("({} agent 处理中...)\n\n", ttype);
                    let _ = stdout.flush();

                    match crate::runner::run_expert_sync(
                        &ttype,
                        &desc,
                        llm.clone(),
                        expert_ctx.clone(),
                    )
                    .await
                    {
                        Ok(result) => {
                            println!("{}", result);
                            println!();
                        }
                        Err(e) => {
                            eprintln!("错误: {e}");
                            println!();
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("错误: {e}\n");
            }
        }
    }
}

fn describe_model(cfg: &AppConfig) -> String {
    cfg.models
        .agents
        .iter()
        .find(|a| a.name == "coding")
        .map(|a| format!("{} · {}", a.primary.provider, a.primary.model))
        .unwrap_or_else(|| "unknown".into())
}
