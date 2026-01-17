//! Document and image CRUD operations.

use tracing::{info, warn};

use crate::db::{Document, ProcessingStatus};
use crate::error::{ServiceError, ServiceResult};
use crate::service::SeneschalService;
use crate::tools::AccessLevel;

impl SeneschalService {
    /// List documents
    pub fn list_documents(&self, user_role: u8) -> ServiceResult<Vec<Document>> {
        self.db.list_documents(Some(user_role))
    }

    /// Delete a document
    pub fn delete_document(&self, document_id: &str) -> ServiceResult<bool> {
        // Cancel any in-progress processing first
        let was_processing = self.cancel_document_processing(document_id);
        if was_processing {
            info!(doc_id = %document_id, "Cancelled in-progress processing for deleted document");
        }

        self.db.delete_document(document_id)
    }

    /// Update document details (title, access_level, tags)
    pub fn update_document(
        &self,
        document_id: &str,
        title: &str,
        access_level: AccessLevel,
        tags: Vec<String>,
    ) -> ServiceResult<bool> {
        self.db
            .update_document(document_id, title, access_level, tags)
    }

    /// Get images for a document
    pub fn get_document_images(
        &self,
        document_id: &str,
    ) -> ServiceResult<Vec<crate::db::DocumentImage>> {
        self.db.get_document_images(document_id)
    }

    /// Delete all images for a document
    pub fn delete_document_images(&self, document_id: &str) -> ServiceResult<usize> {
        // Get paths and delete from database
        let paths = self.db.delete_document_images(document_id)?;
        let count = paths.len();

        // Delete the image files
        for path in paths {
            if let Err(e) = std::fs::remove_file(&path) {
                warn!(path = %path, error = %e, "Failed to delete image file");
            }
        }

        // Try to remove the images directory for this document
        let images_dir = self
            .runtime_config
            .static_config
            .storage
            .data_dir
            .join("images")
            .join(document_id);
        let _ = std::fs::remove_dir(&images_dir); // Ignore error if not empty or doesn't exist

        info!(document_id = %document_id, count = count, "Deleted document images");
        Ok(count)
    }

    /// Delete a single image by ID
    pub fn delete_image(&self, image_id: &str) -> ServiceResult<bool> {
        // Get path and delete from database
        let result = self.db.delete_image(image_id)?;

        if let Some((path, document_id)) = result {
            // Delete the image file
            if let Err(e) = std::fs::remove_file(&path) {
                warn!(path = %path, error = %e, "Failed to delete image file");
            }

            info!(image_id = %image_id, document_id = %document_id, "Deleted image");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Re-extract images from a document
    pub fn reextract_document_images(
        &self,
        document_id: &str,
        vision_model: Option<String>,
    ) -> ServiceResult<()> {
        // Get the document to validate it exists and is a PDF
        let document =
            self.db
                .get_document(document_id)?
                .ok_or_else(|| ServiceError::DocumentNotFound {
                    document_id: document_id.to_string(),
                })?;

        // Get the file path
        let file_path =
            document
                .file_path
                .as_ref()
                .ok_or_else(|| ServiceError::InvalidRequest {
                    message:
                        "Document has no associated file. Re-upload the document to extract images."
                            .to_string(),
                })?;

        // Check if it's a PDF
        let extension = std::path::Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if extension != "pdf" {
            return Err(ServiceError::InvalidRequest {
                message: "Image extraction is only supported for PDF documents".to_string(),
            });
        }

        let doc_path = std::path::Path::new(file_path);
        if !doc_path.exists() {
            return Err(ServiceError::InvalidRequest {
                message:
                    "Original document file not found. Re-upload the document to extract images."
                        .to_string(),
            });
        }

        // Delete existing images first
        self.delete_document_images(document_id)?;

        // Update metadata with vision model if provided
        if vision_model.is_some() {
            let metadata = serde_json::json!({ "vision_model": vision_model });
            let _ = self
                .db
                .update_document_metadata(document_id, Some(metadata));
        }

        // Queue for processing by setting status back to "processing"
        let _ = self
            .db
            .update_document_progress(document_id, "extracting_images", 0, 1);
        let _ = self.db.update_document_processing_status(
            document_id,
            ProcessingStatus::Processing,
            None,
        );

        info!(document_id = %document_id, "Queued document for image re-extraction");
        Ok(())
    }
}
