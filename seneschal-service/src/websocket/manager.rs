//! WebSocket connection manager.
//!
//! Handles connection lifecycle, authentication, and state tracking
//! for all active WebSocket connections.

use dashmap::DashMap;
use tokio::sync::mpsc;
use tracing::debug;

use super::messages::ServerMessage;

/// State for a single WebSocket connection
pub(crate) struct ConnectionState {
    #[allow(dead_code)] // Kept for debugging/logging
    pub(crate) session_id: String,
    pub(crate) user_id: Option<String>,
    pub(crate) user_name: Option<String>,
    pub(crate) user_role: Option<u8>,
    pub(crate) tx: mpsc::UnboundedSender<ServerMessage>,
    pub(crate) subscribed_to_documents: bool,
    pub(crate) authenticated: bool,
}

/// Manager for all WebSocket connections
///
/// Handles connection lifecycle and message broadcasting.
pub struct WebSocketManager {
    pub(crate) connections: DashMap<String, ConnectionState>,
}

impl Default for WebSocketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketManager {
    /// Create a new WebSocket manager
    pub fn new() -> Self {
        Self {
            connections: DashMap::new(),
        }
    }

    /// Add a new connection
    pub(crate) fn add_connection(
        &self,
        session_id: String,
        tx: mpsc::UnboundedSender<ServerMessage>,
    ) {
        debug!(session_id = %session_id, "Adding WebSocket connection");
        self.connections.insert(
            session_id.clone(),
            ConnectionState {
                session_id,
                user_id: None,
                user_name: None,
                user_role: None,
                tx,
                subscribed_to_documents: false,
                authenticated: false,
            },
        );
    }

    /// Remove a connection
    pub(crate) fn remove_connection(&self, session_id: &str) {
        debug!(session_id = %session_id, "Removing WebSocket connection");
        self.connections.remove(session_id);
    }

    /// Authenticate a connection
    pub(crate) fn authenticate(
        &self,
        session_id: &str,
        user_id: String,
        user_name: String,
        user_role: u8,
    ) -> bool {
        if let Some(mut conn) = self.connections.get_mut(session_id) {
            conn.user_id = Some(user_id);
            conn.user_name = Some(user_name);
            conn.user_role = Some(user_role);
            conn.authenticated = true;
            true
        } else {
            false
        }
    }

    /// Set document subscription status for a connection
    pub(crate) fn set_document_subscription(&self, session_id: &str, subscribed: bool) {
        if let Some(mut conn) = self.connections.get_mut(session_id) {
            conn.subscribed_to_documents = subscribed;
            debug!(
                session_id = %session_id,
                subscribed = subscribed,
                "Updated document subscription"
            );
        }
    }

    /// Send a message to a specific connection
    pub fn send_to(&self, session_id: &str, msg: ServerMessage) {
        if let Some(conn) = self.connections.get(session_id)
            && conn.tx.send(msg).is_err()
        {
            tracing::warn!(session_id = %session_id, "Failed to send message to connection");
        }
    }

    /// Get the number of active connections
    #[allow(dead_code)] // Useful for monitoring/debugging
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Get the number of connections subscribed to documents
    #[allow(dead_code)] // Useful for monitoring/debugging
    pub fn document_subscriber_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|entry| entry.value().authenticated && entry.value().subscribed_to_documents)
            .count()
    }

    /// Get first available authenticated GM connection for MCP routing
    ///
    /// Returns the session_id of an authenticated connection with GM role (4+),
    /// or None if no GM is currently connected.
    pub fn get_any_gm_connection(&self) -> Option<String> {
        for entry in self.connections.iter() {
            let conn = entry.value();
            if conn.authenticated && conn.user_role.is_some_and(|r| r >= 4) {
                return Some(entry.key().clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_manager() {
        let manager = WebSocketManager::new();
        let (tx, _rx) = mpsc::unbounded_channel();

        // Add connection
        manager.add_connection("session1".to_string(), tx);
        assert_eq!(manager.connection_count(), 1);
        assert_eq!(manager.document_subscriber_count(), 0);

        // Authenticate
        manager.authenticate("session1", "user1".to_string(), "User One".to_string(), 4);

        // Subscribe
        manager.set_document_subscription("session1", true);
        assert_eq!(manager.document_subscriber_count(), 1);

        // Unsubscribe
        manager.set_document_subscription("session1", false);
        assert_eq!(manager.document_subscriber_count(), 0);

        // Remove
        manager.remove_connection("session1");
        assert_eq!(manager.connection_count(), 0);
    }
}
