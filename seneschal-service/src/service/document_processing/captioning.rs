//! Image captioning functionality.

use base64::Engine;
use tracing::{debug, error, info, warn};

use crate::db::{CaptioningStatus, Document};
use crate::error::{ServiceError, ServiceResult};
use crate::service::SeneschalService;

impl SeneschalService {
    /// Caption images for a single document (called by the captioning worker)
    /// This method is resumable - it only captions images without descriptions
    pub(crate) async fn caption_document_images(&self, document: &Document) {
        let doc_id = &document.id;

        // Register cancellation token for this document
        let cancel_token = self.register_processing_token(doc_id);

        let file_path = match &document.file_path {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                error!(doc_id = %doc_id, "Document has no file path for captioning");
                if let Err(e) = self.db.update_captioning_status(
                    doc_id,
                    CaptioningStatus::Failed,
                    Some("Document has no file path"),
                ) {
                    warn!(doc_id = %doc_id, error = %e, "Failed to update captioning status to failed");
                }
                self.broadcast_captioning_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    Some("Document has no file path"),
                );
                self.unregister_processing_token(doc_id);
                return;
            }
        };

        // Extract vision model from metadata
        let vision_model = match document
            .metadata
            .as_ref()
            .and_then(|m| m.get("vision_model"))
            .and_then(|v| v.as_str())
        {
            Some(model) => model.to_string(),
            None => {
                error!(doc_id = %doc_id, "Document has no vision model specified");
                if let Err(e) = self.db.update_captioning_status(
                    doc_id,
                    CaptioningStatus::Failed,
                    Some("No vision model specified"),
                ) {
                    warn!(doc_id = %doc_id, error = %e, "Failed to update captioning status to failed");
                }
                self.broadcast_captioning_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    Some("No vision model specified"),
                );
                self.unregister_processing_token(doc_id);
                return;
            }
        };

        // Get images that need captioning
        let images_to_caption = match self.db.get_images_without_descriptions(doc_id) {
            Ok(images) => images,
            Err(e) => {
                error!(doc_id = %doc_id, error = %e, "Failed to get images for captioning");
                let error_msg = format!("Failed to query images: {}", e);
                if let Err(update_err) = self.db.update_captioning_status(
                    doc_id,
                    CaptioningStatus::Failed,
                    Some(&error_msg),
                ) {
                    warn!(
                        doc_id = %doc_id,
                        original_error = %e,
                        update_error = %update_err,
                        "Failed to update captioning status to failed"
                    );
                }
                self.broadcast_captioning_progress(doc_id, "failed", None, None, Some(&error_msg));
                self.unregister_processing_token(doc_id);
                return;
            }
        };

        if images_to_caption.is_empty() {
            // All images already captioned
            if let Err(e) =
                self.db
                    .update_captioning_status(doc_id, CaptioningStatus::Completed, None)
            {
                warn!(doc_id = %doc_id, error = %e, "Failed to update captioning status to completed");
            }
            if let Err(e) = self.db.clear_captioning_progress(doc_id) {
                warn!(doc_id = %doc_id, error = %e, "Failed to clear captioning progress");
            }
            self.broadcast_captioning_progress(doc_id, "completed", None, None, None);
            info!(doc_id = %doc_id, "All images already captioned");
            self.unregister_processing_token(doc_id);
            return;
        }

        // Mark as in_progress in database BEFORE starting work
        if let Err(e) = self
            .db
            .update_captioning_status(doc_id, CaptioningStatus::InProgress, None)
        {
            warn!(doc_id = %doc_id, error = %e, "Failed to update captioning status to in_progress");
        }

        let total_images = self.db.get_image_count(doc_id).unwrap_or_else(|e| {
            debug!(doc_id = %doc_id, error = %e, "Failed to get image count");
            images_to_caption.len()
        });
        let already_captioned = total_images - images_to_caption.len();

        // Update database with initial progress and broadcast
        if let Err(e) = self
            .db
            .update_captioning_progress(doc_id, already_captioned, total_images)
        {
            warn!(doc_id = %doc_id, error = %e, "Failed to update initial captioning progress");
        }
        self.broadcast_captioning_progress(
            doc_id,
            "in_progress",
            Some(already_captioned),
            Some(total_images),
            None,
        );

        info!(
            doc_id = %doc_id,
            remaining = images_to_caption.len(),
            already_captioned = already_captioned,
            total = total_images,
            model = %vision_model,
            "Captioning remaining images"
        );

        // Extract page text for context
        let unique_pages: std::collections::HashSet<i32> = images_to_caption
            .iter()
            .flat_map(|img| {
                img.source_pages
                    .clone()
                    .unwrap_or_else(|| vec![img.page_number])
            })
            .collect();

        let page_list: Vec<i32> = unique_pages.into_iter().collect();
        let page_texts = match self.ingestion.extract_pdf_page_text(&file_path, &page_list) {
            Ok(texts) => {
                debug!(
                    doc_id = %doc_id,
                    pages = texts.len(),
                    "Extracted page text for image captioning context"
                );
                texts
            }
            Err(e) => {
                warn!(
                    doc_id = %doc_id,
                    error = %e,
                    "Failed to extract page text, captioning without context"
                );
                std::collections::HashMap::new()
            }
        };

        // Caption each image
        for (i, image) in images_to_caption.iter().enumerate() {
            // Check for cancellation before each image
            if cancel_token.is_cancelled() {
                info!(doc_id = %doc_id, progress = i, "Image captioning cancelled");
                self.unregister_processing_token(doc_id);
                return;
            }

            let current_progress = already_captioned + i + 1;
            if let Err(e) =
                self.db
                    .update_captioning_progress(doc_id, current_progress, total_images)
            {
                warn!(doc_id = %doc_id, error = %e, "Failed to update captioning progress");
            }
            self.broadcast_captioning_progress(
                doc_id,
                "in_progress",
                Some(current_progress),
                Some(total_images),
                None,
            );

            debug!(
                doc_id = %doc_id,
                image_id = %image.id,
                progress = current_progress,
                total = total_images,
                "Captioning image"
            );

            // Build page context for this image
            let mut source_pages = image
                .source_pages
                .clone()
                .unwrap_or_else(|| vec![image.page_number]);
            source_pages.sort();
            let context: String = source_pages
                .iter()
                .filter_map(|p| {
                    page_texts
                        .get(p)
                        .map(|t| format!("--- Page {} ---\n{}", p, t))
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            let page_context = if context.is_empty() {
                None
            } else {
                Some(context.as_str())
            };

            let image_path = std::path::Path::new(&image.internal_path);
            match self
                .caption_image(image_path, &vision_model, &document.title, page_context)
                .await
            {
                Ok(Some(description)) => {
                    if let Err(e) = self.db.update_image_description(&image.id, &description) {
                        warn!(
                            image_id = %image.id,
                            error = %e,
                            "Failed to update image description"
                        );
                    } else {
                        // Generate and store embedding for the description
                        match self.search.embed_text(&description).await {
                            Ok(embedding) => {
                                if let Err(e) =
                                    self.db.insert_image_embedding(&image.id, &embedding)
                                {
                                    warn!(
                                        image_id = %image.id,
                                        error = %e,
                                        "Failed to store image embedding"
                                    );
                                }
                            }
                            Err(e) => {
                                warn!(
                                    image_id = %image.id,
                                    error = %e,
                                    "Failed to generate image embedding"
                                );
                            }
                        }
                        debug!(
                            image_id = %image.id,
                            description_len = description.len(),
                            "Image captioned successfully"
                        );
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        image_id = %image.id,
                        error = %e,
                        "Failed to caption image"
                    );
                }
            }
        }

        // Mark captioning as complete
        if let Err(e) = self
            .db
            .update_captioning_status(doc_id, CaptioningStatus::Completed, None)
        {
            error!(doc_id = %doc_id, error = %e, "Failed to mark captioning as completed");
        }
        if let Err(e) = self.db.clear_captioning_progress(doc_id) {
            warn!(doc_id = %doc_id, error = %e, "Failed to clear captioning progress");
        }
        self.broadcast_captioning_progress(doc_id, "completed", None, None, None);

        // Unregister cancellation token
        self.unregister_processing_token(doc_id);

        info!(doc_id = %doc_id, "Image captioning complete");
    }

    /// Caption an image using the specified vision model
    pub async fn caption_image(
        &self,
        image_path: &std::path::Path,
        vision_model: &str,
        document_title: &str,
        page_context: Option<&str>,
    ) -> ServiceResult<Option<String>> {
        // Read and encode image as base64
        let image_data = std::fs::read(image_path)
            .map_err(|e| ServiceError::Processing(crate::error::ProcessingError::Io(e)))?;
        let image_base64 = base64::engine::general_purpose::STANDARD.encode(&image_data);

        // Build prompt with document title and optional page context
        let base_prompt = format!(
            "Describe this image from the tabletop RPG document \"{}\". \
            Focus on what the image depicts (characters, creatures, locations, items, maps, etc.) \
            and any text visible in the image. Be concise but descriptive. \
            This description will be used to help game masters find relevant images.",
            document_title
        );

        let prompt = if let Some(context) = page_context {
            if context.is_empty() {
                base_prompt
            } else {
                format!(
                    "{}\n\n\
                    The image appears on a page with the following text for additional context:\n\n{}",
                    base_prompt, context
                )
            }
        } else {
            base_prompt
        };

        let message = crate::ollama::ChatMessage::user_with_image(&prompt, image_base64);

        let description = self
            .ollama
            .generate_simple(vision_model, vec![message])
            .await?;

        Ok(Some(description))
    }
}
