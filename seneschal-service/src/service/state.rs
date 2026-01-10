//! State structures for the Seneschal service.
//!
//! This module contains the in-memory state structures used by the chat system.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::db::ConversationMessage;

/// User context from FVTT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserContext {
    pub user_id: String,
    pub user_name: String,
    pub role: u8, // CONST.USER_ROLES: 0=None, 1=Player, 2=Trusted, 3=Assistant, 4=GM
    #[serde(default)]
    pub owned_actor_ids: Vec<String>,
    pub character_id: Option<String>,
}

impl UserContext {
    pub fn is_gm(&self) -> bool {
        self.role >= 4
    }
}

/// Active request state (in-memory only)
#[derive(Debug, Clone)]
pub struct ActiveRequest {
    pub user_context: UserContext,
    pub messages: Vec<ConversationMessage>,
    pub tool_calls_made: u32,
    pub pending_external_tool: Option<PendingToolCall>,
    pub paused: bool,
    pub started_at: Instant,
    /// WebSocket session ID (if this is a WebSocket chat)
    #[allow(dead_code)]
    pub ws_session_id: Option<String>,
}

/// Pending external tool call
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PendingToolCall {
    pub id: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub sent_at: Instant,
}
