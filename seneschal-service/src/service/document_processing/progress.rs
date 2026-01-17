//! WebSocket progress broadcast helpers.

use crate::service::SeneschalService;
use crate::websocket::{CaptioningProgressUpdate, DocumentProgressUpdate};

impl SeneschalService {
    /// Broadcast captioning progress via WebSocket
    pub(crate) fn broadcast_captioning_progress(
        &self,
        document_id: &str,
        status: &str,
        progress: Option<usize>,
        total: Option<usize>,
        error: Option<&str>,
    ) {
        self.ws_manager
            .broadcast_captioning_update(CaptioningProgressUpdate {
                document_id: document_id.to_string(),
                status: status.to_string(),
                progress,
                total,
                error: error.map(String::from),
            });
    }

    /// Broadcast document processing progress via WebSocket
    pub(crate) fn broadcast_document_progress(
        &self,
        document_id: &str,
        status: &str,
        phase: Option<&str>,
        progress: Option<usize>,
        total: Option<usize>,
        error: Option<&str>,
    ) {
        // Compute counts dynamically from the database
        let chunk_count = self.db.get_chunk_count(document_id).unwrap_or(0);
        let image_count = self.db.get_image_count(document_id).unwrap_or(0);

        self.ws_manager
            .broadcast_document_update(DocumentProgressUpdate {
                document_id: document_id.to_string(),
                status: status.to_string(),
                phase: phase.map(String::from),
                progress,
                total,
                error: error.map(String::from),
                chunk_count,
                image_count,
            });
    }
}
