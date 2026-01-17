//! Document CRUD operations.
//!
//! This module contains all document-related database operations including
//! insert, get, list, delete, and hash management.

use rusqlite::{OptionalExtension, params};

use super::Database;
use super::models::{CaptioningStatus, Document, ProcessingStatus};
use crate::error::{DatabaseError, ServiceResult};
use crate::tools::AccessLevel;

impl Database {
    /// Insert a new document
    pub fn insert_document(&self, doc: &Document) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        let metadata_json = doc
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(DatabaseError::Serialization)?;

        conn.execute(
            r#"
            INSERT INTO documents (id, title, file_path, file_hash, access_level, metadata, created_at, updated_at, processing_status, processing_error, processing_phase, processing_progress, processing_total, captioning_status, captioning_error, captioning_progress, captioning_total)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            "#,
            params![
                doc.id,
                doc.title,
                doc.file_path,
                doc.file_hash,
                doc.access_level as u8,
                metadata_json,
                doc.created_at.to_rfc3339(),
                doc.updated_at.to_rfc3339(),
                doc.processing_status.as_str(),
                doc.processing_error,
                doc.processing_phase,
                doc.processing_progress.map(|p| p as i64),
                doc.processing_total.map(|t| t as i64),
                doc.captioning_status.as_str(),
                doc.captioning_error,
                doc.captioning_progress.map(|p| p as i64),
                doc.captioning_total.map(|t| t as i64),
            ],
        )
        .map_err(DatabaseError::Query)?;

        // Insert tags
        for tag in &doc.tags {
            conn.execute(
                "INSERT OR IGNORE INTO document_tags (document_id, tag) VALUES (?1, ?2)",
                params![doc.id, tag],
            )
            .map_err(DatabaseError::Query)?;
        }

        Ok(())
    }

    /// Get a document by ID
    pub fn get_document(&self, id: &str) -> ServiceResult<Option<Document>> {
        let conn = self.conn.lock().unwrap();

        let doc = conn
            .query_row(
                "SELECT d.id, d.title, d.file_path, d.file_hash, d.access_level, d.metadata, d.created_at, d.updated_at, d.processing_status, d.processing_error, \
                 (SELECT COUNT(*) FROM chunks WHERE document_id = d.id) as chunk_count, \
                 (SELECT COUNT(*) FROM document_images WHERE document_id = d.id) as image_count, \
                 d.processing_phase, d.processing_progress, d.processing_total, \
                 d.captioning_status, d.captioning_error, d.captioning_progress, d.captioning_total \
                 FROM documents d WHERE d.id = ?1",
                params![id],
                |row| Document::from_row(row, vec![]),
            )
            .optional()
            .map_err(DatabaseError::Query)?;

        if let Some(mut doc) = doc {
            // Load tags
            let mut stmt = conn
                .prepare("SELECT tag FROM document_tags WHERE document_id = ?1")
                .map_err(DatabaseError::Query)?;
            let tags: Vec<String> = stmt
                .query_map(params![id], |row| row.get(0))
                .map_err(DatabaseError::Query)?
                .filter_map(|r| r.ok())
                .collect();
            doc.tags = tags;
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    }

    /// Delete a document and all related data
    pub fn delete_document(&self, id: &str) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute("DELETE FROM documents WHERE id = ?1", params![id])
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Check if a document with the given file_hash already exists.
    /// Returns the document ID if found.
    pub fn get_document_by_hash(&self, file_hash: &str) -> ServiceResult<Option<String>> {
        let conn = self.conn.lock().unwrap();

        conn.query_row(
            "SELECT id FROM documents WHERE file_hash = ?1 AND processing_status != 'failed'",
            params![file_hash],
            |row| row.get(0),
        )
        .optional()
        .map_err(DatabaseError::Query)
        .map_err(Into::into)
    }

    /// Update a document's file_hash.
    pub fn update_document_hash(&self, document_id: &str, file_hash: &str) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET file_hash = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![file_hash, document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Get all documents without a file_hash (for backfill migration).
    /// Only returns documents with a file_path set (so we can compute the hash).
    pub fn get_documents_without_hash(&self) -> ServiceResult<Vec<Document>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT d.id, d.title, d.file_path, d.file_hash, d.access_level, d.metadata, d.created_at, d.updated_at, d.processing_status, d.processing_error, \
                 (SELECT COUNT(*) FROM chunks WHERE document_id = d.id) as chunk_count, \
                 (SELECT COUNT(*) FROM document_images WHERE document_id = d.id) as image_count, \
                 d.processing_phase, d.processing_progress, d.processing_total, \
                 d.captioning_status, d.captioning_error, d.captioning_progress, d.captioning_total \
                 FROM documents d WHERE d.file_hash IS NULL AND d.file_path IS NOT NULL ORDER BY d.created_at"
            )
            .map_err(DatabaseError::Query)?;

        let rows = stmt
            .query_map([], |row| Document::from_row(row, vec![]))
            .map_err(DatabaseError::Query)?;

        let mut docs = Vec::new();
        for row in rows {
            docs.push(row.map_err(DatabaseError::Query)?);
        }

        Ok(docs)
    }

    /// List all documents with optional access level filter
    pub fn list_documents(&self, max_access_level: Option<u8>) -> ServiceResult<Vec<Document>> {
        let conn = self.conn.lock().unwrap();

        let mut docs = Vec::new();

        if let Some(level) = max_access_level {
            let mut stmt = conn
                .prepare(
                    "SELECT d.id, d.title, d.file_path, d.file_hash, d.access_level, d.metadata, d.created_at, d.updated_at, d.processing_status, d.processing_error, \
                     (SELECT COUNT(*) FROM chunks WHERE document_id = d.id) as chunk_count, \
                     (SELECT COUNT(*) FROM document_images WHERE document_id = d.id) as image_count, \
                     d.processing_phase, d.processing_progress, d.processing_total, \
                     d.captioning_status, d.captioning_error, d.captioning_progress, d.captioning_total \
                     FROM documents d WHERE d.access_level <= ?1 ORDER BY d.title"
                )
                .map_err(DatabaseError::Query)?;
            let rows = stmt
                .query_map(params![level], |row| Document::from_row(row, vec![]))
                .map_err(DatabaseError::Query)?;
            for row in rows {
                docs.push(row.map_err(DatabaseError::Query)?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT d.id, d.title, d.file_path, d.file_hash, d.access_level, d.metadata, d.created_at, d.updated_at, d.processing_status, d.processing_error, \
                     (SELECT COUNT(*) FROM chunks WHERE document_id = d.id) as chunk_count, \
                     (SELECT COUNT(*) FROM document_images WHERE document_id = d.id) as image_count, \
                     d.processing_phase, d.processing_progress, d.processing_total, \
                     d.captioning_status, d.captioning_error, d.captioning_progress, d.captioning_total \
                     FROM documents d ORDER BY d.title"
                )
                .map_err(DatabaseError::Query)?;
            let rows = stmt
                .query_map([], |row| Document::from_row(row, vec![]))
                .map_err(DatabaseError::Query)?;
            for row in rows {
                docs.push(row.map_err(DatabaseError::Query)?);
            }
        }

        // Load tags for each document
        for doc in &mut docs {
            let mut tag_stmt = conn
                .prepare("SELECT tag FROM document_tags WHERE document_id = ?1")
                .map_err(DatabaseError::Query)?;
            doc.tags = tag_stmt
                .query_map(params![doc.id], |row| row.get(0))
                .map_err(DatabaseError::Query)?
                .filter_map(|r| r.ok())
                .collect();
        }

        Ok(docs)
    }

    /// Update document processing status
    pub fn update_document_processing_status(
        &self,
        document_id: &str,
        status: ProcessingStatus,
        error: Option<&str>,
    ) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET processing_status = ?1, processing_error = ?2, updated_at = datetime('now') WHERE id = ?3",
                params![status.as_str(), error, document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Update document processing progress
    pub fn update_document_progress(
        &self,
        document_id: &str,
        phase: &str,
        progress: usize,
        total: usize,
    ) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET processing_phase = ?1, processing_progress = ?2, processing_total = ?3, updated_at = datetime('now') WHERE id = ?4",
                params![phase, progress as i64, total as i64, document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Clear document processing progress (called when processing completes or fails)
    pub fn clear_document_progress(&self, document_id: &str) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET processing_phase = NULL, processing_progress = NULL, processing_total = NULL, updated_at = datetime('now') WHERE id = ?1",
                params![document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Update document metadata
    pub fn update_document_metadata(
        &self,
        document_id: &str,
        metadata: Option<serde_json::Value>,
    ) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let metadata_json = metadata.map(|m| m.to_string());

        let rows = conn
            .execute(
                "UPDATE documents SET metadata = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![metadata_json, document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Update document details (title, access_level, and tags)
    pub fn update_document(
        &self,
        document_id: &str,
        title: &str,
        access_level: AccessLevel,
        tags: Vec<String>,
    ) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        // Update the document's title and access_level
        let rows = conn
            .execute(
                "UPDATE documents SET title = ?1, access_level = ?2, updated_at = datetime('now') WHERE id = ?3",
                params![title, access_level as u8, document_id],
            )
            .map_err(DatabaseError::Query)?;

        if rows == 0 {
            return Ok(false);
        }

        // Delete existing tags
        conn.execute(
            "DELETE FROM document_tags WHERE document_id = ?1",
            params![document_id],
        )
        .map_err(DatabaseError::Query)?;

        // Insert new tags
        for tag in &tags {
            let tag = tag.trim();
            if !tag.is_empty() {
                conn.execute(
                    "INSERT OR IGNORE INTO document_tags (document_id, tag) VALUES (?1, ?2)",
                    params![document_id, tag],
                )
                .map_err(DatabaseError::Query)?;
            }
        }

        Ok(true)
    }

    /// Get the next document pending processing (oldest first)
    /// Used by the document processing worker queue
    pub fn get_next_pending_document(&self) -> ServiceResult<Option<Document>> {
        let conn = self.conn.lock().unwrap();

        let doc = conn
            .query_row(
                "SELECT d.id, d.title, d.file_path, d.file_hash, d.access_level, d.metadata, d.created_at, d.updated_at, d.processing_status, d.processing_error, \
                 (SELECT COUNT(*) FROM chunks WHERE document_id = d.id) as chunk_count, \
                 (SELECT COUNT(*) FROM document_images WHERE document_id = d.id) as image_count, \
                 d.processing_phase, d.processing_progress, d.processing_total, \
                 d.captioning_status, d.captioning_error, d.captioning_progress, d.captioning_total \
                 FROM documents d WHERE d.processing_status = 'processing' ORDER BY d.created_at ASC LIMIT 1",
                [],
                |row| Document::from_row(row, vec![]),
            )
            .optional()
            .map_err(DatabaseError::Query)?;

        if let Some(mut doc) = doc {
            // Load tags
            let mut stmt = conn
                .prepare("SELECT tag FROM document_tags WHERE document_id = ?1")
                .map_err(DatabaseError::Query)?;
            let tags: Vec<String> = stmt
                .query_map(params![doc.id], |row| row.get(0))
                .map_err(DatabaseError::Query)?
                .filter_map(|r| r.ok())
                .collect();
            doc.tags = tags;
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    }

    /// Set a document's captioning status to pending
    /// Called after image extraction when a vision model is specified
    pub fn set_captioning_pending(&self, document_id: &str) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET captioning_status = 'pending', captioning_error = NULL, updated_at = datetime('now') WHERE id = ?1",
                params![document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Get the next document needing captioning (oldest first)
    /// Prioritizes in_progress documents (to resume interrupted work) over pending ones
    /// Used by the captioning worker queue
    pub fn get_next_pending_captioning_document(&self) -> ServiceResult<Option<Document>> {
        let conn = self.conn.lock().unwrap();

        let doc = conn
            .query_row(
                "SELECT d.id, d.title, d.file_path, d.file_hash, d.access_level, d.metadata, d.created_at, d.updated_at, d.processing_status, d.processing_error, \
                 (SELECT COUNT(*) FROM chunks WHERE document_id = d.id) as chunk_count, \
                 (SELECT COUNT(*) FROM document_images WHERE document_id = d.id) as image_count, \
                 d.processing_phase, d.processing_progress, d.processing_total, \
                 d.captioning_status, d.captioning_error, d.captioning_progress, d.captioning_total \
                 FROM documents d WHERE d.captioning_status IN ('in_progress', 'pending') \
                 ORDER BY CASE d.captioning_status WHEN 'in_progress' THEN 0 ELSE 1 END, d.created_at ASC LIMIT 1",
                [],
                |row| Document::from_row(row, vec![]),
            )
            .optional()
            .map_err(DatabaseError::Query)?;

        if let Some(mut doc) = doc {
            // Load tags
            let mut stmt = conn
                .prepare("SELECT tag FROM document_tags WHERE document_id = ?1")
                .map_err(DatabaseError::Query)?;
            let tags: Vec<String> = stmt
                .query_map(params![doc.id], |row| row.get(0))
                .map_err(DatabaseError::Query)?
                .filter_map(|r| r.ok())
                .collect();
            doc.tags = tags;
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    }

    /// Update captioning status
    pub fn update_captioning_status(
        &self,
        document_id: &str,
        status: CaptioningStatus,
        error: Option<&str>,
    ) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET captioning_status = ?1, captioning_error = ?2, updated_at = datetime('now') WHERE id = ?3",
                params![status.as_str(), error, document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Update captioning progress
    pub fn update_captioning_progress(
        &self,
        document_id: &str,
        progress: usize,
        total: usize,
    ) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET captioning_status = 'in_progress', captioning_progress = ?1, captioning_total = ?2, updated_at = datetime('now') WHERE id = ?3",
                params![progress as i64, total as i64, document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Clear captioning progress (called when captioning completes or fails)
    pub fn clear_captioning_progress(&self, document_id: &str) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET captioning_progress = NULL, captioning_total = NULL, updated_at = datetime('now') WHERE id = ?1",
                params![document_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }
}
