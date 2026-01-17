//! Cancellation token management for document processing.

use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::error::{ServiceError, ServiceResult};
use crate::service::SeneschalService;

impl SeneschalService {
    /// Register a cancellation token for a document being processed.
    pub(crate) fn register_processing_token(&self, document_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.processing_cancellation_tokens
            .insert(document_id.to_string(), token.clone());
        token
    }

    /// Cancel processing for a document if in progress.
    pub(crate) fn cancel_document_processing(&self, document_id: &str) -> bool {
        if let Some((_, token)) = self.processing_cancellation_tokens.remove(document_id) {
            token.cancel();
            info!(doc_id = %document_id, "Document processing cancellation triggered");
            true
        } else {
            false
        }
    }

    /// Remove a cancellation token when processing completes normally.
    pub(crate) fn unregister_processing_token(&self, document_id: &str) {
        self.processing_cancellation_tokens.remove(document_id);
    }

    /// Check if processing should continue for a document.
    pub(crate) fn check_cancellation(
        &self,
        document_id: &str,
        token: &CancellationToken,
    ) -> ServiceResult<()> {
        use crate::error::ProcessingError;
        if token.is_cancelled() {
            Err(ServiceError::Processing(ProcessingError::Cancelled {
                document_id: document_id.to_string(),
            }))
        } else {
            Ok(())
        }
    }
}
