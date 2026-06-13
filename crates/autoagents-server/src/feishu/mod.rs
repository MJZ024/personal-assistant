//! Feishu Bot integration layer.
//!
//! Handles:
//! - Event subscription verification (URL challenge)
//! - Message receive (text, file, image)
//! - Message send (text, file)
//! - File upload/download via Feishu API

mod api;
pub mod events;
mod security;
mod types;

pub use api::FeishuClient;
pub use events::EventCallback;
pub use types::*;
