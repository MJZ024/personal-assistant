//! Type definitions for Feishu Bot API.

use serde::{Deserialize, Serialize};

/// Configuration for Feishu Bot integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuConfig {
    /// Feishu App ID from the developer console.
    pub app_id: String,
    /// Feishu App Secret.
    pub app_secret: String,
    /// Verification token for event subscriptions.
    ///
    /// Required in *token mode* (when `encrypt_key` is unset). In that mode the
    /// token is the only thing proving a callback came from Feishu, so it must
    /// be kept secret and the listener must not be exposed to untrusted
    /// networks without TLS. Prefer configuring `encrypt_key` (signature mode).
    pub verification_token: String,
    /// Feishu Encrypt Key (a.k.a. Encryption Key) from the developer console.
    ///
    /// When set, Feishu signs every callback with `X-Lark-Signature` and
    /// AES-256-CBC encrypts the payload. This is the strongly recommended mode:
    /// it defeats request forgery and tampering even over plaintext HTTP.
    /// When empty, the bot falls back to the weaker `verification_token` check.
    pub encrypt_key: String,
    /// Allowlist of Feishu sender `open_id` values permitted to issue commands.
    ///
    /// Empty means **nobody** is authorised (fail closed). Populate it with the
    /// owner's `open_id` so that even an authenticated Feishu user who merely
    /// messages the bot cannot trigger command execution.
    pub allowed_sender_ids: Vec<String>,
    /// The bot name for display purposes.
    pub bot_name: String,
    /// Max file size in bytes for user-uploaded files (default 20MB).
    pub max_upload_size: u64,
    /// Feishu API base URL.
    pub api_base_url: String,
    /// HTTP server listen address and port.
    ///
    /// Defaults to the loopback interface: expose the callback through a
    /// TLS-terminating reverse proxy (nginx/Caddy) rather than binding the
    /// plaintext handler directly to a public interface.
    pub listen_addr: String,
}

impl Default for FeishuConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            app_secret: String::new(),
            verification_token: String::new(),
            encrypt_key: String::new(),
            allowed_sender_ids: Vec::new(),
            bot_name: "个人助理".to_string(),
            max_upload_size: 20 * 1024 * 1024, // 20MB
            api_base_url: "https://open.feishu.cn/open-apis".to_string(),
            listen_addr: "127.0.0.1:8080".to_string(),
        }
    }
}

/// Event types from Feishu.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum FeishuEvent {
    #[serde(rename = "url_verification")]
    UrlVerification { challenge: String },
    #[serde(rename = "im.message.receive_v1")]
    MessageReceive {
        sender: Option<SenderInfo>,
        message: Option<MessageContent>,
    },
}

/// Sender information in a Feishu event.
#[derive(Debug, Clone, Deserialize)]
pub struct SenderInfo {
    pub sender_id: Option<SenderId>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SenderId {
    pub open_id: Option<String>,
    pub user_id: Option<String>,
}

/// Message content from Feishu.
#[derive(Debug, Clone, Deserialize)]
pub struct MessageContent {
    pub message_id: Option<String>,
    pub chat_id: Option<String>,
    pub message_type: Option<String>,
    pub content: Option<String>,
    pub file_key: Option<String>,
    pub image_key: Option<String>,
}

/// Header structure for Feishu event callbacks.
#[derive(Debug, Clone, Deserialize)]
pub struct FeishuEventHeader {
    #[serde(rename = "event_type")]
    pub event_type: String,
    #[serde(rename = "tenant_key")]
    pub tenant_key: Option<String>,
}

/// Full Feishu event request body.
#[derive(Debug, Clone, Deserialize)]
pub struct FeishuEventBody {
    pub schema: Option<String>,
    pub header: Option<FeishuEventHeader>,
    pub event: Option<FeishuEvent>,
    pub challenge: Option<String>,
    pub token: Option<String>,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

/// Format of message content for Feishu API send.
#[derive(Debug, Clone, Serialize)]
pub struct TextContent {
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeishuSendMessage {
    #[serde(rename = "receive_id")]
    pub receive_id: String,
    #[serde(rename = "msg_type")]
    pub msg_type: String,
    pub content: String,
    #[serde(rename = "uuid")]
    pub msg_uuid: Option<String>,
}

/// Access token response from Feishu.
#[derive(Debug, Clone, Deserialize)]
pub struct TenantAccessTokenResponse {
    pub code: i32,
    pub msg: String,
    #[serde(rename = "tenant_access_token")]
    pub token: Option<String>,
    pub expire: Option<i64>,
}

/// Send message response.
#[derive(Debug, Clone, Deserialize)]
pub struct SendMessageResponse {
    pub code: i32,
    pub msg: String,
    pub data: Option<SendMessageData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendMessageData {
    pub message_id: Option<String>,
}

/// Parsed user message from Feishu.
#[derive(Debug, Clone)]
pub struct UserMessage {
    pub sender_id: String,
    pub chat_id: String,
    pub message_id: String,
    pub content: String,
    pub message_type: MessageType,
    pub file_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    Text,
    File,
    Image,
    Unknown,
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::File => write!(f, "file"),
            Self::Image => write!(f, "image"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}
