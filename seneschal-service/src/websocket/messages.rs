//! WebSocket message types.
//!
//! Defines the client-to-server and server-to-client message formats
//! for WebSocket communication.

use serde::{Deserialize, Serialize};

/// Messages sent from client to server
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Authenticate the connection with user information
    Auth {
        user_id: String,
        user_name: String,
        role: u8,
        session_id: Option<String>,
    },
    /// Keepalive ping
    Ping,
    /// Subscribe to document processing updates
    SubscribeDocuments,
    /// Unsubscribe from document processing updates
    UnsubscribeDocuments,
    /// Start a new chat or continue an existing conversation
    ChatMessage {
        /// None = start new conversation, Some = continue existing
        conversation_id: Option<String>,
        /// The user's message
        message: String,
        /// Model to use (optional, uses default if not specified)
        model: Option<String>,
        /// Which tools to enable (optional, uses all if not specified)
        enabled_tools: Option<Vec<String>>,
    },
    /// Send a tool result back to the agentic loop
    ToolResult {
        conversation_id: String,
        tool_call_id: String,
        result: serde_json::Value,
    },
    /// Continue a paused chat (after tool limit reached)
    ContinueChat { conversation_id: String },
    /// Cancel an active chat
    CancelChat { conversation_id: String },
}

/// Messages sent from server to client
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Response to authentication attempt
    AuthResponse {
        success: bool,
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Document processing progress update
    DocumentProgress {
        document_id: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        chunk_count: usize,
        image_count: usize,
    },
    /// Keepalive pong response
    Pong { timestamp: u64 },
    /// Error message
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
    /// Chat conversation started
    ChatStarted { conversation_id: String },
    /// Streaming chat content
    ChatContent {
        conversation_id: String,
        text: String,
    },
    /// Tool call requested (for external tools executed by client)
    ChatToolCall {
        conversation_id: String,
        id: String,
        tool: String,
        args: serde_json::Value,
    },
    /// Status update for an internal tool being executed
    ChatToolStatus {
        conversation_id: String,
        tool_call_id: String,
        message: String,
    },
    /// Result from an internal tool execution
    ChatToolResult {
        conversation_id: String,
        tool_call_id: String,
        tool: String,
        summary: String,
    },
    /// Chat paused due to limits (tool calls, time, etc.)
    ChatPaused {
        conversation_id: String,
        reason: String,
        tool_calls_made: u32,
        elapsed_seconds: u64,
        message: String,
    },
    /// Chat turn completed
    ChatTurnComplete {
        conversation_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt_tokens: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        completion_tokens: Option<u32>,
    },
    /// Chat error
    ChatError {
        conversation_id: String,
        message: String,
        recoverable: bool,
    },
}

/// Data for broadcasting document progress updates
#[derive(Debug, Clone)]
pub struct DocumentProgressUpdate {
    pub document_id: String,
    pub status: String,
    pub phase: Option<String>,
    pub progress: Option<usize>,
    pub total: Option<usize>,
    pub error: Option<String>,
    pub chunk_count: usize,
    pub image_count: usize,
}

impl From<DocumentProgressUpdate> for ServerMessage {
    fn from(update: DocumentProgressUpdate) -> Self {
        ServerMessage::DocumentProgress {
            document_id: update.document_id,
            status: update.status,
            phase: update.phase,
            progress: update.progress,
            total: update.total,
            error: update.error,
            chunk_count: update.chunk_count,
            image_count: update.image_count,
        }
    }
}
