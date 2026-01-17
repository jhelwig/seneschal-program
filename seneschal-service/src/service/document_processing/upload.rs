//! Document upload and hash backfill functionality.

use tracing::{debug, info, warn};

use crate::db::{CaptioningStatus, Document, ProcessingStatus};
use crate::error::{ServiceError, ServiceResult};
use crate::ingestion::hash::compute_content_hash;
use crate::service::SeneschalService;
use crate::tools::AccessLevel;

impl SeneschalService {
    /// Upload a document and enqueue it for processing
    ///
    /// This method saves the file and creates a document record with "processing"
    /// status. The document processing worker will pick it up and process it.
    /// Clients should poll the document status for completion.
    pub async fn upload_document(
        &self,
        content: &[u8],
        filename: &str,
        title: &str,
        access_level: AccessLevel,
        tags: Vec<String>,
        vision_model: Option<String>,
    ) -> ServiceResult<Document> {
        // Check file size
        let max_size = self.runtime_config.dynamic().limits.max_document_size_bytes;
        if content.len() as u64 > max_size {
            return Err(ServiceError::Processing(
                crate::error::ProcessingError::FileTooLarge {
                    size: content.len() as u64,
                    max: max_size,
                },
            ));
        }

        // Compute content hash for duplicate detection
        let file_hash = compute_content_hash(content);

        // Generate document ID
        let doc_id = uuid::Uuid::new_v4().to_string();

        // Save file to permanent storage immediately
        let docs_dir = self
            .runtime_config
            .static_config
            .storage
            .data_dir
            .join("documents");
        std::fs::create_dir_all(&docs_dir)
            .map_err(|e| ServiceError::Processing(crate::error::ProcessingError::Io(e)))?;

        let permanent_path = docs_dir.join(format!("{}_{}", doc_id, filename));
        std::fs::write(&permanent_path, content)
            .map_err(|e| ServiceError::Processing(crate::error::ProcessingError::Io(e)))?;

        // Store vision model in metadata if provided
        let metadata = vision_model.map(|vm| serde_json::json!({ "vision_model": vm }));

        // Create document record with "processing" status
        let now = chrono::Utc::now();
        let document = Document {
            id: doc_id.clone(),
            title: title.to_string(),
            file_path: Some(permanent_path.to_string_lossy().to_string()),
            file_hash: Some(file_hash),
            access_level,
            tags: tags.clone(),
            metadata,
            processing_status: ProcessingStatus::Processing,
            processing_error: None,
            chunk_count: 0,
            image_count: 0,
            processing_phase: Some("queued".to_string()),
            processing_progress: None,
            processing_total: None,
            captioning_status: CaptioningStatus::NotRequested,
            captioning_error: None,
            captioning_progress: None,
            captioning_total: None,
            created_at: now,
            updated_at: now,
        };

        // Save document to database (enqueue for processing)
        self.db.insert_document(&document)?;

        info!(
            doc_id = %doc_id,
            title = %title,
            "Document uploaded and queued for processing"
        );

        Ok(document)
    }

    /// Backfill file_hash for existing documents that don't have one.
    ///
    /// This runs once on startup to populate hashes for documents uploaded
    /// before hash computation was added. Enables duplicate detection for
    /// auto-import functionality.
    pub async fn backfill_document_hashes(&self) -> ServiceResult<usize> {
        use crate::ingestion::hash::compute_file_hash;
        use std::path::Path;

        let docs = self.db.get_documents_without_hash()?;
        if docs.is_empty() {
            return Ok(0);
        }

        info!(count = docs.len(), "Backfilling document hashes");

        let mut backfilled = 0;
        for doc in docs {
            let Some(ref file_path) = doc.file_path else {
                // Shouldn't happen due to the query, but skip if no path
                continue;
            };

            let path = Path::new(file_path);
            if !path.exists() {
                warn!(
                    doc_id = %doc.id,
                    file_path = %file_path,
                    "Document file not found, skipping hash backfill"
                );
                continue;
            }

            match compute_file_hash(path) {
                Ok(hash) => {
                    if let Err(e) = self.db.update_document_hash(&doc.id, &hash) {
                        warn!(
                            doc_id = %doc.id,
                            error = %e,
                            "Failed to update document hash"
                        );
                    } else {
                        debug!(doc_id = %doc.id, hash = %hash, "Backfilled document hash");
                        backfilled += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        doc_id = %doc.id,
                        file_path = %file_path,
                        error = %e,
                        "Failed to compute hash for document"
                    );
                }
            }
        }

        Ok(backfilled)
    }
}
