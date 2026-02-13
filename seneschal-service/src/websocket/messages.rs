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
    /// Send a tool result back (for MCP external tools executed by FVTT client)
    ToolResult {
        conversation_id: String,
        tool_call_id: String,
        result: serde_json::Value,
    },
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
    /// Image captioning progress update (separate from document processing)
    CaptioningProgress {
        document_id: String,
        /// Status: "pending", "in_progress", "completed", "failed"
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Keepalive pong response
    Pong { timestamp: u64 },
    /// Error message
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
    /// Tool call requested (for MCP external tools executed by FVTT client)
    ChatToolCall {
        conversation_id: String,
        id: String,
        tool: String,
        args: serde_json::Value,
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

/// Data for broadcasting captioning progress updates
#[derive(Debug, Clone)]
pub struct CaptioningProgressUpdate {
    pub document_id: String,
    pub status: String,
    pub progress: Option<usize>,
    pub total: Option<usize>,
    pub error: Option<String>,
}

impl From<CaptioningProgressUpdate> for ServerMessage {
    fn from(update: CaptioningProgressUpdate) -> Self {
        ServerMessage::CaptioningProgress {
            document_id: update.document_id,
            status: update.status,
            progress: update.progress,
            total: update.total,
            error: update.error,
        }
    }
}
