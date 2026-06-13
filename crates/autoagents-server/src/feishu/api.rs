//! Feishu API client for sending messages, uploading files, etc.

use reqwest::Client;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::types::*;

/// API client for Feishu Open Platform.
pub struct FeishuClient {
    client: Client,
    config: FeishuConfig,
    token_cache: Mutex<TokenCache>,
}

struct TokenCache {
    token: Option<String>,
    expires_at: Instant,
}

impl FeishuClient {
    pub fn new(config: FeishuConfig) -> Self {
        Self {
            client: Client::new(),
            config,
            token_cache: Mutex::new(TokenCache {
                token: None,
                expires_at: Instant::now(),
            }),
        }
    }

    /// Get a tenant access token (with caching).
    pub async fn get_access_token(&self) -> Result<String, String> {
        {
            let cache = self.token_cache.lock().unwrap();
            if let Some(ref token) = cache.token {
                if cache.expires_at > Instant::now() {
                    return Ok(token.clone());
                }
            }
        }

        let url = format!(
            "{}/auth/v3/tenant_access_token/internal",
            self.config.api_base_url
        );
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await
            .map_err(|e| format!("Token request failed: {}", e))?;

        let body: TenantAccessTokenResponse = resp
            .json()
            .await
            .map_err(|e| format!("Token parse failed: {}", e))?;

        if body.code != 0 {
            return Err(format!("Token API error: {} - {}", body.code, body.msg));
        }

        let token = body.token.ok_or("No token in response")?;
        let expires_in = body.expire.unwrap_or(7200) as u64;

        let mut cache = self.token_cache.lock().unwrap();
        cache.token = Some(token.clone());
        cache.expires_at = Instant::now() + Duration::from_secs(expires_in - 300); // 5 min buffer

        Ok(token)
    }

    /// Send a text message to a chat.
    pub async fn send_text_message(
        &self,
        receive_id: &str,
        content: &str,
    ) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let url = format!(
            "{}/im/v1/messages?receive_id_type=chat_id",
            self.config.api_base_url
        );

        let inner = serde_json::json!({ "text": content });
        let msg = FeishuSendMessage {
            receive_id: receive_id.to_string(),
            msg_type: "text".to_string(),
            content: inner.to_string(),
            msg_uuid: Some(uuid::Uuid::new_v4().to_string()),
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&msg)
            .send()
            .await
            .map_err(|e| format!("Send message failed: {}", e))?;

        let body: SendMessageResponse = resp
            .json()
            .await
            .map_err(|e| format!("Response parse failed: {}", e))?;

        if body.code != 0 {
            return Err(format!("Send message error: {} - {}", body.code, body.msg));
        }

        Ok(body.data.and_then(|d| d.message_id).unwrap_or_default())
    }

    /// Reply to a message in thread.
    pub async fn reply_to_message(
        &self,
        message_id: &str,
        content: &str,
    ) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let url = format!(
            "{}/im/v1/messages/{}/reply",
            self.config.api_base_url, message_id
        );

        let inner = serde_json::json!({ "text": content });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "content": inner.to_string(),
                "msg_type": "text",
            }))
            .send()
            .await
            .map_err(|e| format!("Reply failed: {}", e))?;

        let body: SendMessageResponse = resp
            .json()
            .await
            .map_err(|e| format!("Response parse failed: {}", e))?;

        if body.code != 0 {
            return Err(format!("Reply error: {} - {}", body.code, body.msg));
        }

        Ok(body.data.and_then(|d| d.message_id).unwrap_or_default())
    }

    /// Send a file message.
    pub async fn send_file(&self, chat_id: &str, file_key: &str) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let url = format!(
            "{}/im/v1/messages?receive_id_type=chat_id",
            self.config.api_base_url
        );

        let inner = serde_json::json!({ "file_key": file_key });
        let msg = FeishuSendMessage {
            receive_id: chat_id.to_string(),
            msg_type: "file".to_string(),
            content: inner.to_string(),
            msg_uuid: Some(uuid::Uuid::new_v4().to_string()),
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&msg)
            .send()
            .await
            .map_err(|e| format!("Send file failed: {}", e))?;

        let body: SendMessageResponse = resp.json().await.map_err(|e| e.to_string())?;
        if body.code != 0 {
            return Err(format!("Send file error: {}", body.msg));
        }

        Ok(body.data.and_then(|d| d.message_id).unwrap_or_default())
    }

    /// Download a file from Feishu.
    pub async fn download_file(&self, message_id: &str, file_key: &str) -> Result<Vec<u8>, String> {
        let token = self.get_access_token().await?;
        let url = format!(
            "{}/im/v1/messages/{}/resources/{}?type=file",
            self.config.api_base_url, message_id, file_key
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Read bytes failed: {}", e))?;

        // Check size limit
        if bytes.len() as u64 > self.config.max_upload_size {
            return Err(format!(
                "File too large: {} bytes (max: {} bytes)",
                bytes.len(),
                self.config.max_upload_size
            ));
        }

        Ok(bytes.to_vec())
    }

    /// Download an image from Feishu.
    pub async fn download_image(
        &self,
        message_id: &str,
        image_key: &str,
    ) -> Result<Vec<u8>, String> {
        let token = self.get_access_token().await?;
        let url = format!(
            "{}/im/v1/messages/{}/resources/{}?type=image",
            self.config.api_base_url, message_id, image_key
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Read bytes failed: {}", e))?;
        Ok(bytes.to_vec())
    }
}
