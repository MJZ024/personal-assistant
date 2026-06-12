//! Feishu event subscription handler.

use axum::extract::State;
use axum::response::Json;
use serde_json::Value;

use super::types::*;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub feishu_client: std::sync::Arc<super::api::FeishuClient>,
    pub supervisor: std::sync::Arc<tokio::sync::Mutex<autoagents_supervisor::Supervisor>>,
    pub config: std::sync::Arc<super::FeishuConfig>,
}

/// Feishu event callback endpoint.
///
/// Handles:
/// 1. URL Verification (on first setup): returns the challenge string
/// 2. Event reception: processes incoming messages
pub async fn event_callback(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    // Check if this is a URL verification challenge
    if let Some(challenge) = body.get("challenge").and_then(|v| v.as_str()) {
        let token = body
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Verify the token matches our config
        if token != state.config.verification_token {
            return Json(serde_json::json!({
                "code": 400,
                "msg": "Invalid verification token"
            }));
        }

        return Json(serde_json::json!({
            "challenge": challenge
        }));
    }

    // Process the event
    let header = body.get("header");
    let event_type = header
        .and_then(|h| h.get("event_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let event = body.get("event");
    let sender = event
        .and_then(|e| e.get("sender"))
        .and_then(|s| s.get("sender_id"))
        .and_then(|s| s.get("open_id"))
        .or_else(|| {
            event
                .and_then(|e| e.get("sender"))
                .and_then(|s| s.get("sender_id"))
                .and_then(|s| s.get("user_id"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let message = event.and_then(|e| e.get("message"));

    match event_type {
        "im.message.receive_v1" => {
            handle_incoming_message(
                &state,
                sender,
                message,
            )
            .await;
        }
        _ => {
            log::debug!("Unhandled event type: {}", event_type);
        }
    }

    Json(serde_json::json!({ "code": 0 }))
}

/// Process an incoming message from a user.
async fn handle_incoming_message(
    state: &AppState,
    sender_id: &str,
    message: Option<&Value>,
) {
    let msg = match message {
        Some(m) => m,
        None => {
            log::warn!("Received message event with no message content");
            return;
        }
    };

    let msg_type = msg.get("message_type").and_then(|v| v.as_str()).unwrap_or("text");
    let chat_id = msg.get("chat_id").and_then(|v| v.as_str()).unwrap_or("");
    let message_id = msg.get("message_id").and_then(|v| v.as_str()).unwrap_or("");

    // Parse text content
    let content_text = if msg_type == "text" {
        msg.get("content")
            .and_then(|v| v.as_str())
            .and_then(|c| {
                serde_json::from_str::<Value>(c).ok()
                    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
            })
            .unwrap_or_else(|| "(empty message)".to_string())
    } else if msg_type == "file" {
        "[文件消息]".to_string()
    } else if msg_type == "image" {
        "[图片消息]".to_string()
    } else {
        format!("[{} 消息]", msg_type)
    };

    // Handle commands directly
    if content_text.starts_with('/') {
        handle_command(state, chat_id, &content_text).await;
        return;
    }

    // Route to supervisor
    let mut supervisor = state.supervisor.lock().await;
    match supervisor.handle_message(sender_id, &content_text).await {
        Ok(response) => {
            let reply_text = if let Some(ref task_id) = response.task_id {
                format!("{}\n\n任务ID: {}", response.message, &task_id[..8])
            } else {
                response.message
            };

            let _ = state.feishu_client.send_text_message(chat_id, &reply_text).await;
        }
        Err(e) => {
            log::error!("Supervisor error: {}", e);
            let _ = state
                .feishu_client
                .send_text_message(chat_id, &format!("处理消息时出错: {}", e))
                .await;
        }
    }
}

/// Handle slash commands.
async fn handle_command(
    state: &AppState,
    chat_id: &str,
    command: &str,
) {
    let mut supervisor = state.supervisor.lock().await;
    match supervisor.handle_message("system", command).await {
        Ok(response) => {
            let _ = state.feishu_client.send_text_message(chat_id, &response.message).await;
        }
        Err(e) => {
            let _ = state
                .feishu_client
                .send_text_message(chat_id, &format!("命令执行失败: {}", e))
                .await;
        }
    }
}

/// Trait alias for event callback function.
pub trait EventCallback: Send + Sync {
    fn handle_event(
        &self,
        event_type: &str,
        body: Value,
    ) -> impl std::future::Future<Output = Value> + Send;
}
