//! Broadcast functions for WebSocket updates.
//!
//! Contains functions for broadcasting document progress updates,
//! captioning progress, and other real-time notifications to
//! subscribed clients.

use tracing::debug;

use super::manager::WebSocketManager;
use super::messages::{CaptioningProgressUpdate, DocumentProgressUpdate, ServerMessage};

impl WebSocketManager {
    /// Broadcast a document progress update to all subscribed connections
    pub fn broadcast_document_update(&self, update: DocumentProgressUpdate) {
        let msg: ServerMessage = update.into();
        let mut sent_count = 0;

        for entry in self.connections.iter() {
            let conn = entry.value();
            if conn.authenticated
                && conn.subscribed_to_documents
                && conn.tx.send(msg.clone()).is_ok()
            {
                sent_count += 1;
            }
        }

        if sent_count > 0 {
            debug!(
                sent_count = sent_count,
                "Broadcast document update to connections"
            );
        }
    }

    /// Broadcast a captioning progress update to all subscribed connections
    pub fn broadcast_captioning_update(&self, update: CaptioningProgressUpdate) {
        let msg: ServerMessage = update.into();
        let mut sent_count = 0;

        for entry in self.connections.iter() {
            let conn = entry.value();
            if conn.authenticated
                && conn.subscribed_to_documents
                && conn.tx.send(msg.clone()).is_ok()
            {
                sent_count += 1;
            }
        }

        if sent_count > 0 {
            debug!(
                sent_count = sent_count,
                "Broadcast captioning update to connections"
            );
        }
    }
}
