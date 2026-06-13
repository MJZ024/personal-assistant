//! Feishu event subscription handler.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use bytes::Bytes;
use serde_json::Value;

use super::security::{AuthOutcome, authenticate};
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
/// Every request is authenticated **before** any business logic runs:
/// - In signature mode (`encrypt_key` set) the `X-Lark-Signature` is verified
///   over the raw body and the timestamp is checked for freshness; the payload
///   is then AES-decrypted.
/// - In token mode (`encrypt_key` empty) the payload's `verification_token` is
///   checked.
///
/// Any failure fails closed with HTTP 401. Only authenticated events reach the
/// URL-verification handshake, message parsing, or the sender allowlist.
///
/// Handles:
/// 1. URL Verification (on first setup): returns the challenge string
/// 2. Event reception: processes incoming messages (subject to the allowlist)
pub async fn event_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // ── Authenticity: verify before touching the payload. ──
    let cfg = &state.config;
    let now_secs = chrono::Utc::now().timestamp();
    let outcome = authenticate(
        &cfg.encrypt_key,
        &cfg.verification_token,
        header_str(&headers, "x-lark-signature"),
        header_str(&headers, "x-lark-request-timestamp"),
        header_any(
            &headers,
            &["x-lark-request-nonce", "x-lark-request-request-nonce"],
        ),
        &body,
        now_secs,
    );

    let body_value = match outcome {
        AuthOutcome::Authenticated(v) => v,
        AuthOutcome::Rejected(reason) => {
            log::warn!("Rejected Feishu callback: {}", reason);
            return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        }
    };

    // ── URL verification handshake (now authenticated/decrypted). ──
    if let Some(challenge) = body_value.get("challenge").and_then(|v| v.as_str()) {
        return Json(serde_json::json!({ "challenge": challenge })).into_response();
    }

    // ── Event routing. ──
    let header = body_value.get("header");
    let event_type = header
        .and_then(|h| h.get("event_type"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let event = body_value.get("event");
    let sender = sender_open_id(event).unwrap_or("unknown");

    if event_type == "im.message.receive_v1" {
        // Authorization: even an authenticated Feishu user must be on the
        // allowlist before they can drive command execution.
        if !is_sender_allowed(&cfg.allowed_sender_ids, sender) {
            log::warn!("Ignored message from sender not on allowlist: {}", sender);
            return Json(serde_json::json!({ "code": 0 })).into_response();
        }
        handle_incoming_message(&state, sender, event.and_then(|e| e.get("message"))).await;
    } else {
        log::debug!("Unhandled event type: {}", event_type);
    }

    Json(serde_json::json!({ "code": 0 })).into_response()
}

/// Extract the sender's `open_id` (falling back to `user_id`) from an event.
fn sender_open_id(event: Option<&Value>) -> Option<&str> {
    let id = event?.get("sender")?.get("sender_id")?;
    id.get("open_id")
        .and_then(|v| v.as_str())
        .or_else(|| id.get("user_id").and_then(|v| v.as_str()))
}

/// Return whether `sender` is permitted to issue commands.
///
/// An empty allowlist authorises **nobody** (fail closed).
fn is_sender_allowed(allowed: &[String], sender: &str) -> bool {
    !allowed.is_empty() && allowed.iter().any(|s| s == sender)
}

/// Read a header value as a `&str`, tolerating missing/non-ASCII headers.
fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

/// Read the first present header from a list of candidate names.
///
/// Feishu's docs are inconsistent about the nonce header
/// (`X-Lark-Request-Nonce` vs a doubled `X-Lark-Request-Request-Nonce`). Since
/// the nonce is mixed into the signature, a wrong name silently breaks every
/// signature-mode callback, so accept either spelling.
fn header_any<'a>(headers: &'a HeaderMap, names: &[&str]) -> &'a str {
    for name in names {
        let value = header_str(headers, name);
        if !value.is_empty() {
            return value;
        }
    }
    ""
}

/// Process an incoming message from a user.
async fn handle_incoming_message(state: &AppState, sender_id: &str, message: Option<&Value>) {
    let msg = match message {
        Some(m) => m,
        None => {
            log::warn!("Received message event with no message content");
            return;
        }
    };

    let msg_type = msg
        .get("message_type")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    let chat_id = msg.get("chat_id").and_then(|v| v.as_str()).unwrap_or("");
    let message_id = msg.get("message_id").and_then(|v| v.as_str()).unwrap_or("");

    // Parse text content
    let content_text = if msg_type == "text" {
        msg.get("content")
            .and_then(|v| v.as_str())
            .and_then(|c| {
                serde_json::from_str::<Value>(c).ok().and_then(|v| {
                    v.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                })
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

            let _ = state
                .feishu_client
                .send_text_message(chat_id, &reply_text)
                .await;
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
async fn handle_command(state: &AppState, chat_id: &str, command: &str) {
    let mut supervisor = state.supervisor.lock().await;
    match supervisor.handle_message("system", command).await {
        Ok(response) => {
            let _ = state
                .feishu_client
                .send_text_message(chat_id, &response.message)
                .await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allowlist_authorizes_nobody() {
        assert!(!is_sender_allowed(&[], "ou_anyone"));
    }

    #[test]
    fn allowlist_admits_listed_sender() {
        let allow = vec!["ou_owner".to_string()];
        assert!(is_sender_allowed(&allow, "ou_owner"));
    }

    #[test]
    fn allowlist_rejects_unlisted_sender() {
        let allow = vec!["ou_owner".to_string()];
        assert!(!is_sender_allowed(&allow, "ou_attacker"));
    }

    #[test]
    fn sender_open_id_extracts_open_id() {
        let event: Value = serde_json::json!({
            "sender": { "sender_id": { "open_id": "ou_123", "user_id": "u_456" } }
        });
        assert_eq!(sender_open_id(Some(&event)), Some("ou_123"));
    }

    #[test]
    fn sender_open_id_falls_back_to_user_id() {
        let event: Value = serde_json::json!({
            "sender": { "sender_id": { "user_id": "u_456" } }
        });
        assert_eq!(sender_open_id(Some(&event)), Some("u_456"));
    }

    #[test]
    fn sender_open_id_returns_none_when_missing() {
        assert_eq!(sender_open_id(Some(&serde_json::json!({}))), None);
        assert_eq!(sender_open_id(None), None);
    }
}

#[cfg(test)]
mod router_tests {
    //! End-to-end tests of the event-callback wiring: axum routing, header
    //! extraction, and the fail-closed auth decision — exercised through the
    //! real router without starting a process or calling the LLM (challenge /
    //! unhandled-type paths return before the supervisor is touched).

    use super::super::security::compute_signature;
    use super::{AppState, event_callback};
    use autoagents_memory::{Database, MemoryConfig};
    use autoagents_supervisor::{Supervisor, SupervisorConfig};
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::post;
    use std::sync::Arc;
    use tower::ServiceExt;

    use super::super::FeishuConfig;

    fn test_router(encrypt_key: &str, verification_token: &str) -> Router {
        let config = FeishuConfig {
            verification_token: verification_token.to_string(),
            encrypt_key: encrypt_key.to_string(),
            ..Default::default()
        };
        let memory = MemoryConfig {
            db_path: ":memory:".to_string(),
            ..Default::default()
        };
        let db = Arc::new(Database::open(&memory).expect("db"));
        let supervisor = Arc::new(tokio::sync::Mutex::new(Supervisor::new(
            SupervisorConfig::default(),
            db,
        )));
        let feishu_client = Arc::new(super::super::api::FeishuClient::new(config.clone()));
        let state = AppState {
            feishu_client,
            supervisor,
            config: Arc::new(config),
        };
        Router::new()
            .route("/feishu/event", post(event_callback))
            .with_state(state)
    }

    fn request(headers: &[(&str, &str)], body: &str) -> Request<Body> {
        let mut builder = Request::builder().method(Method::POST).uri("/feishu/event");
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn send(router: Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
        let resp = router.oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.expect("body");
        (status, bytes.to_vec())
    }

    // ── token mode ──

    #[tokio::test]
    async fn forged_event_without_token_is_rejected() {
        let app = test_router("", "secret-token");
        let body = r#"{"event_type":"im.message.receive_v1","event":{}}"#;
        let (status, _) = send(app, request(&[], body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn forged_event_with_wrong_token_is_rejected() {
        let app = test_router("", "secret-token");
        let body = r#"{"token":"WRONG","event":{}}"#;
        let (status, _) = send(app, request(&[], body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_token_challenge_is_accepted_and_echoed() {
        let app = test_router("", "secret-token");
        let body = r#"{"challenge":"hello-123","token":"secret-token"}"#;
        let (status, body) = send(app, request(&[], body)).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["challenge"], "hello-123");
    }

    // ── signature mode ──

    #[tokio::test]
    async fn signature_mode_rejects_missing_signature_header() {
        let app = test_router("encrypt_key_abc", "");
        let body = r#"{"challenge":"x"}"#;
        let (status, _) = send(app, request(&[], body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn signature_mode_rejects_tampered_body() {
        let app = test_router("encrypt_key_abc", "");
        let body = r#"{"challenge":"x"}"#;
        // Signature computed for a DIFFERENT body.
        let ts = chrono::Utc::now().timestamp().to_string();
        let sig = compute_signature(&ts, "n1", "encrypt_key_abc", b"other body");
        let headers = [
            ("x-lark-signature", sig.as_str()),
            ("x-lark-request-timestamp", ts.as_str()),
            ("x-lark-request-nonce", "n1"),
        ];
        let (status, _) = send(app, request(&headers, body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn signature_mode_accepts_doubled_nonce_header_spelling() {
        // Feishu's docs inconsistently show X-Lark-Request-Request-Nonce.
        // The callback must accept either spelling or signature mode silently
        // breaks for some app configurations.
        let app = test_router("encrypt_key_abc", "");
        let body = r#"{"challenge":"nonce-spelling"}"#;
        let ts = chrono::Utc::now().timestamp().to_string();
        let sig = compute_signature(&ts, "nonce-2", "encrypt_key_abc", body.as_bytes());
        let headers = [
            ("x-lark-signature", sig.as_str()),
            ("x-lark-request-timestamp", ts.as_str()),
            ("x-lark-request-request-nonce", "nonce-2"),
        ];
        let (status, resp_body) = send(app, request(&headers, body)).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        assert_eq!(v["challenge"], "nonce-spelling");
    }

    #[tokio::test]
    async fn signature_mode_accepts_valid_signed_challenge() {
        let app = test_router("encrypt_key_abc", "");
        let body = r#"{"challenge":"signed-ok"}"#;
        // Use the current time so the ±300s freshness window passes.
        let ts = chrono::Utc::now().timestamp().to_string();
        let sig = compute_signature(&ts, "nonce-1", "encrypt_key_abc", body.as_bytes());
        let headers = [
            ("x-lark-signature", sig.as_str()),
            ("x-lark-request-timestamp", ts.as_str()),
            ("x-lark-request-nonce", "nonce-1"),
        ];
        let (status, resp_body) = send(app, request(&headers, body)).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        assert_eq!(v["challenge"], "signed-ok");
    }
}
