//! Main document processing pipeline.

use std::sync::Arc;

use tracing::{debug, error, info, warn};

use crate::db::{Document, ProcessingStatus};
use crate::error::ServiceError;
use crate::error::format_error_chain_ref;
use crate::service::SeneschalService;
use crate::websocket::DocumentProgressUpdate;

impl SeneschalService {
    /// Process a single document (called by the worker)
    /// This method is resumable - it checks what's already been done and continues from there.
    pub(crate) async fn process_document(&self, document: &Document) {
        let doc_id = &document.id;
        let title = &document.title;

        // Register cancellation token for this document
        let cancel_token = self.register_processing_token(doc_id);

        let file_path = match &document.file_path {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                error!(doc_id = %doc_id, "Document has no file path");
                if let Err(e) = self.db.update_document_processing_status(
                    doc_id,
                    ProcessingStatus::Failed,
                    Some("Document has no file path"),
                ) {
                    warn!(doc_id = %doc_id, error = %e, "Failed to update status to failed");
                }
                self.broadcast_document_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    None,
                    Some("Document has no file path"),
                );
                self.unregister_processing_token(doc_id);
                return;
            }
        };

        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();

        // Extract vision model from metadata if present
        let vision_model = document
            .metadata
            .as_ref()
            .and_then(|m| m.get("vision_model"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        info!(doc_id = %doc_id, "Resuming/starting document processing");

        // Step 1: Check if chunks exist, if not extract text and create chunks
        if self.check_cancellation(doc_id, &cancel_token).is_err() {
            info!(doc_id = %doc_id, "Document processing cancelled before chunking");
            self.unregister_processing_token(doc_id);
            return;
        }

        let existing_chunk_count = self.db.get_chunk_count(doc_id).unwrap_or_else(|e| {
            debug!(doc_id = %doc_id, error = %e, "Failed to get chunk count");
            0
        });
        if existing_chunk_count == 0 {
            info!(doc_id = %doc_id, "Extracting text and creating chunks");
            if let Err(e) = self.db.update_document_progress(doc_id, "chunking", 0, 1) {
                warn!(doc_id = %doc_id, phase = "chunking", error = %e, "Failed to update progress");
            }
            self.broadcast_document_progress(
                doc_id,
                "processing",
                Some("chunking"),
                Some(0),
                Some(1),
                None,
            );

            let chunks = match self.ingestion.process_document_with_id(
                &file_path,
                doc_id,
                title,
                document.access_level,
                document.tags.clone(),
            ) {
                Ok(chunks) => chunks,
                Err(e) => {
                    error!(doc_id = %doc_id, error = %e, "Document text extraction failed");
                    if let Err(update_err) = self.db.update_document_processing_status(
                        doc_id,
                        ProcessingStatus::Failed,
                        Some(&e.to_string()),
                    ) {
                        warn!(
                            doc_id = %doc_id,
                            original_error = %e,
                            update_error = %update_err,
                            "Failed to mark document as failed"
                        );
                    }
                    self.broadcast_document_progress(
                        doc_id,
                        "failed",
                        None,
                        None,
                        None,
                        Some(&e.to_string()),
                    );
                    self.unregister_processing_token(doc_id);
                    return;
                }
            };

            // Save chunks
            for chunk in &chunks {
                if let Err(e) = self.db.insert_chunk(chunk) {
                    warn!(chunk_id = %chunk.id, error = %e, "Failed to save chunk");
                }
            }

            info!(doc_id = %doc_id, chunks = chunks.len(), "Chunks created");
        } else {
            info!(doc_id = %doc_id, chunks = existing_chunk_count, "Chunks already exist, skipping text extraction");
        }

        // Step 2: Index chunks that don't have embeddings yet
        if self.check_cancellation(doc_id, &cancel_token).is_err() {
            info!(doc_id = %doc_id, "Document processing cancelled before embedding");
            self.unregister_processing_token(doc_id);
            return;
        }

        let chunks_to_embed = match self.db.get_chunks_without_embeddings(doc_id) {
            Ok(chunks) => chunks,
            Err(e) => {
                error!(doc_id = %doc_id, error = %e, "Failed to get chunks without embeddings");
                let error_msg = format!("Failed to query chunks: {}", e);
                if let Err(update_err) = self.db.update_document_processing_status(
                    doc_id,
                    ProcessingStatus::Failed,
                    Some(&error_msg),
                ) {
                    warn!(
                        doc_id = %doc_id,
                        original_error = %e,
                        update_error = %update_err,
                        "Failed to mark document as failed"
                    );
                }
                self.broadcast_document_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    None,
                    Some(&error_msg),
                );
                self.unregister_processing_token(doc_id);
                return;
            }
        };

        if !chunks_to_embed.is_empty() {
            let total_chunks = self.db.get_chunk_count(doc_id).unwrap_or_else(|e| {
                debug!(doc_id = %doc_id, error = %e, "Failed to get total chunk count");
                0
            });
            let already_embedded = total_chunks - chunks_to_embed.len();
            info!(
                doc_id = %doc_id,
                remaining = chunks_to_embed.len(),
                already_embedded = already_embedded,
                total = total_chunks,
                "Generating embeddings for remaining chunks"
            );
            if let Err(e) = self.db.update_document_progress(
                doc_id,
                "embedding",
                already_embedded,
                total_chunks,
            ) {
                warn!(doc_id = %doc_id, phase = "embedding", error = %e, "Failed to update progress");
            }
            self.broadcast_document_progress(
                doc_id,
                "processing",
                Some("embedding"),
                Some(already_embedded),
                Some(total_chunks),
                None,
            );

            // Clone Arc references for use in progress callback
            let db_for_progress = Arc::clone(&self.db);
            let ws_manager_for_progress = Arc::clone(&self.ws_manager);
            let doc_id_for_progress = doc_id.to_string();
            let cancel_token_for_progress = cancel_token.clone();

            let result = self
                .search
                .index_chunks_with_progress_cancellable(
                    &chunks_to_embed,
                    &cancel_token_for_progress,
                    |progress, _total| {
                        let current = already_embedded + progress;
                        if let Err(e) = db_for_progress.update_document_progress(
                            &doc_id_for_progress,
                            "embedding",
                            current,
                            total_chunks,
                        ) {
                            tracing::warn!(doc_id = %doc_id_for_progress, error = %e, "Failed to update embedding progress");
                        }

                        // Broadcast progress update
                        let chunk_count = db_for_progress
                            .get_chunk_count(&doc_id_for_progress)
                            .unwrap_or_else(|e| {
                                tracing::debug!(doc_id = %doc_id_for_progress, error = %e, "Failed to get chunk count for progress");
                                0
                            });
                        let image_count = db_for_progress
                            .get_image_count(&doc_id_for_progress)
                            .unwrap_or_else(|e| {
                                tracing::debug!(doc_id = %doc_id_for_progress, error = %e, "Failed to get image count for progress");
                                0
                            });
                        ws_manager_for_progress.broadcast_document_update(DocumentProgressUpdate {
                            document_id: doc_id_for_progress.clone(),
                            status: "processing".to_string(),
                            phase: Some("embedding".to_string()),
                            progress: Some(current),
                            total: Some(total_chunks),
                            error: None,
                            chunk_count,
                            image_count,
                        });
                    },
                )
                .await;

            if let Err(e) = result {
                // Check if it was a cancellation - don't log as error
                if matches!(
                    &e,
                    ServiceError::Processing(crate::error::ProcessingError::Cancelled { .. })
                ) {
                    info!(doc_id = %doc_id, "Document processing cancelled during embedding");
                    self.unregister_processing_token(doc_id);
                    return;
                }

                error!(doc_id = %doc_id, error = %e, "Failed to index chunks");
                let error_msg = format!("Embedding generation failed: {}", e);
                if let Err(update_err) = self.db.update_document_processing_status(
                    doc_id,
                    ProcessingStatus::Failed,
                    Some(&error_msg),
                ) {
                    warn!(
                        doc_id = %doc_id,
                        original_error = %e,
                        update_error = %update_err,
                        "Failed to mark document as failed"
                    );
                }
                self.broadcast_document_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    None,
                    Some(&error_msg),
                );
                self.unregister_processing_token(doc_id);
                return;
            }
        } else {
            info!(doc_id = %doc_id, "All chunks already have embeddings");
        }

        // Step 3: Extract images from PDFs if not already done
        if self.check_cancellation(doc_id, &cancel_token).is_err() {
            info!(doc_id = %doc_id, "Document processing cancelled before image extraction");
            self.unregister_processing_token(doc_id);
            return;
        }

        let extension = std::path::Path::new(&filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if extension == "pdf" {
            // Check if images already exist
            let existing_images = self.db.get_document_images(doc_id).unwrap_or_else(|e| {
                debug!(doc_id = %doc_id, error = %e, "Failed to get existing images");
                Vec::new()
            });
            let mut image_count = existing_images.len();

            if image_count == 0 {
                info!(doc_id = %doc_id, "Extracting images from PDF");
                if let Err(e) = self
                    .db
                    .update_document_progress(doc_id, "extracting_images", 0, 1)
                {
                    warn!(doc_id = %doc_id, phase = "extracting_images", error = %e, "Failed to update progress");
                }
                self.broadcast_document_progress(
                    doc_id,
                    "processing",
                    Some("extracting_images"),
                    Some(0),
                    Some(1),
                    None,
                );
                match self.ingestion.extract_pdf_images(&file_path, doc_id) {
                    Ok(images) => {
                        image_count = images.len();
                        for image in &images {
                            if let Err(e) = self.db.insert_document_image(image) {
                                warn!(
                                    image_id = %image.id,
                                    error = %format_error_chain_ref(&e),
                                    "Failed to save document image to database"
                                );
                            }
                        }
                        info!(doc_id = %doc_id, images = image_count, "Images extracted");
                    }
                    Err(e) => {
                        warn!(doc_id = %doc_id, error = %format_error_chain_ref(&e), "Failed to extract images from PDF");
                    }
                }
            } else {
                info!(doc_id = %doc_id, images = image_count, "Images already exist, skipping extraction");
            }

            // Queue for captioning if vision model is specified and there are images to caption
            if vision_model.is_some() && image_count > 0 {
                let images_to_caption = self
                    .db
                    .get_images_without_descriptions(doc_id)
                    .unwrap_or_else(|e| {
                        debug!(doc_id = %doc_id, error = %e, "Failed to get images without descriptions");
                        Vec::new()
                    });

                if !images_to_caption.is_empty() {
                    info!(
                        doc_id = %doc_id,
                        images = images_to_caption.len(),
                        "Queueing document for image captioning"
                    );
                    if let Err(e) = self.db.set_captioning_pending(doc_id) {
                        warn!(doc_id = %doc_id, error = %e, "Failed to queue document for captioning");
                    }
                } else {
                    info!(doc_id = %doc_id, "All images already captioned");
                }
            }
        }

        // Update document with final counts and status
        let total_chunks = self.db.get_chunk_count(doc_id).unwrap_or_else(|e| {
            debug!(doc_id = %doc_id, error = %e, "Failed to get final chunk count");
            0
        });
        let total_images = self
            .db
            .get_document_images(doc_id)
            .map(|i| i.len())
            .unwrap_or_else(|e| {
                debug!(doc_id = %doc_id, error = %e, "Failed to get final image count");
                0
            });
        if let Err(e) = self.db.clear_document_progress(doc_id) {
            warn!(doc_id = %doc_id, error = %e, "Failed to clear progress");
        }
        if let Err(e) =
            self.db
                .update_document_processing_status(doc_id, ProcessingStatus::Completed, None)
        {
            // This one is more serious - document completed but status not updated
            error!(doc_id = %doc_id, error = %e, "Failed to mark document as completed");
        }

        // Broadcast completion
        self.broadcast_document_progress(doc_id, "completed", None, None, None, None);

        // Unregister cancellation token
        self.unregister_processing_token(doc_id);

        info!(
            doc_id = %doc_id,
            title = %title,
            chunks = total_chunks,
            images = total_images,
            "Document processing complete"
        );
    }
}
