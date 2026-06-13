//! Personal Assistant — main entry point.
//!
//! Starts the HTTP server with Feishu Bot integration,
//! initializes all subsystems (database, supervisor, heartbeat),
//! and handles graceful shutdown.

mod config;
mod feishu;
mod runner;

use std::sync::Arc;

use axum::{Router, routing::post};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use autoagents_memory::{Database, Heartbeat, HeartbeatConfig, MemoryConfig};
use autoagents_supervisor::Supervisor;
use autoagents_tool_auth::ToolAuthInterceptor;

use config::AppConfig;
use feishu::{FeishuClient, events::AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting Personal Assistant...");

    // Load configuration
    let config_path = std::env::var("ASSISTANT_CONFIG")
        .unwrap_or_else(|_| "/opt/personal-assistant/config.yaml".to_string());

    let app_config = AppConfig::from_file(&config_path)?;
    log::info!("Configuration loaded from {}", config_path);

    // Initialize database
    let database = Arc::new(Database::open(&app_config.memory)?);
    log::info!("Database initialized at {}", app_config.memory.db_path);

    // Initialize tool auth interceptor
    let auth = Arc::new(ToolAuthInterceptor::new(app_config.tool_auth.clone())?);
    log::info!("Tool auth interceptor initialized");

    // Initialize supervisor
    let mut supervisor = Supervisor::new(app_config.supervisor.clone(), database.clone());

    // Recover from any previous crash
    let recovery_msg = supervisor.recover_after_restart().await?;
    log::info!("{}", recovery_msg);

    let supervisor = Arc::new(tokio::sync::Mutex::new(supervisor));

    // Initialize Feishu client
    let feishu_client = Arc::new(FeishuClient::new(app_config.feishu.clone()));

    // Start heartbeat
    let heartbeat_db = database.clone();
    let heartbeat_config = HeartbeatConfig {
        interval_secs: app_config.supervisor.heartbeat_interval_secs,
        wal_checkpoint_interval_secs: app_config.supervisor.wal_checkpoint_interval_secs,
    };
    let supervisor_for_heartbeat = supervisor.clone();
    let heartbeat = Heartbeat::new(heartbeat_db, heartbeat_config).on_tick(move |tasks| {
        log::debug!("Heartbeat: {} active tasks", tasks.len());
    });

    tokio::spawn(async move {
        heartbeat.run().await;
    });

    // Build HTTP router
    let state = AppState {
        feishu_client: feishu_client.clone(),
        supervisor: supervisor.clone(),
        config: Arc::new(app_config.feishu.clone()),
    };

    let app = Router::new()
        .route("/feishu/event", post(feishu::events::event_callback))
        .route("/health", axum::routing::get(|| async { "OK" }))
        .layer(RequestBodyLimitLayer::new(MAX_UPLOAD_SIZE as usize))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Send startup notification
    if let Ok(token) = feishu_client.get_access_token().await {
        log::info!("Feishu connection established");

        // Try to send startup message to a configured chat (if set via env)
        if let Ok(chat_id) = std::env::var("ASSISTANT_NOTIFY_CHAT_ID") {
            let _ = feishu_client
                .send_text_message(&chat_id, &format!("{} {}", recovery_msg, "随时待命！"))
                .await;
        }
    }

    // Start HTTP server
    let listener = tokio::net::TcpListener::bind(&app_config.feishu.listen_addr).await?;
    log::info!("Server listening on {}", app_config.feishu.listen_addr);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Maximum request body size for file uploads.
const MAX_UPLOAD_SIZE: u64 = 25 * 1024 * 1024; // 25MB (slightly above the 20MB check)
