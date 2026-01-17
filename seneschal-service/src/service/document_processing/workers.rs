//! Background workers for document processing and image captioning.

use std::sync::Arc;

use tracing::{error, info};

use crate::service::SeneschalService;

impl SeneschalService {
    /// Start the document processing worker
    /// This should be called once on server startup
    pub fn start_document_processing_worker(service: Arc<SeneschalService>) {
        tokio::spawn(async move {
            info!("Document processing worker started");
            loop {
                // Check for pending documents
                match service.db.get_next_pending_document() {
                    Ok(Some(doc)) => {
                        info!(doc_id = %doc.id, title = %doc.title, "Processing queued document");
                        service.process_document(&doc).await;
                    }
                    Ok(None) => {
                        // No pending documents, sleep before checking again
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to check for pending documents");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });
    }

    /// Start the image captioning worker
    /// This runs as a separate background task to caption document images without blocking document processing
    pub fn start_captioning_worker(service: Arc<SeneschalService>) {
        tokio::spawn(async move {
            info!("Image captioning worker started");
            loop {
                // Check for documents pending captioning
                match service.db.get_next_pending_captioning_document() {
                    Ok(Some(doc)) => {
                        info!(doc_id = %doc.id, title = %doc.title, "Captioning images for document");
                        service.caption_document_images(&doc).await;
                    }
                    Ok(None) => {
                        // No pending captioning, sleep before checking again
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to check for documents pending captioning");
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    }
                }
            }
        });
    }
}
