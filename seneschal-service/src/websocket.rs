//! WebSocket support for real-time document processing updates and chat
//!
//! This module provides a WebSocket server that allows clients to receive
//! real-time updates about document processing status and bidirectional
//! chat communication without polling.

mod broadcast;
mod handlers;
mod manager;
pub mod messages;

// Re-export public types
pub use handlers::handle_ws_connection;
pub use manager::WebSocketManager;
pub use messages::{CaptioningProgressUpdate, DocumentProgressUpdate, ServerMessage};
