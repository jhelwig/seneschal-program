//! Image CRUD operations and description caching.
//!
//! This module contains all document image-related database operations including
//! insert, get, list, search, update, delete, and FVTT image description caching.

use rusqlite::{OptionalExtension, params};

use super::Database;
use super::chunks::cosine_similarity;
use super::models::{DocumentImage, DocumentImageWithAccess, FvttImageDescription};
use crate::error::{DatabaseError, ServiceResult};
use crate::tools::AccessLevel;

impl Database {
    /// Insert a document image
    pub fn insert_document_image(&self, image: &DocumentImage) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        let source_pages_json = image
            .source_pages
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(DatabaseError::Serialization)?;

        conn.execute(
            r#"
            INSERT INTO document_images (id, document_id, page_number, image_index, internal_path, mime_type, width, height, description, created_at, source_pages, image_type, source_image_id, has_region_render)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
            params![
                image.id,
                image.document_id,
                image.page_number,
                image.image_index,
                image.internal_path,
                image.mime_type,
                image.width.map(|v| v as i32),
                image.height.map(|v| v as i32),
                image.description,
                image.created_at.to_rfc3339(),
                source_pages_json,
                image.image_type.as_str(),
                image.source_image_id,
                image.has_region_render,
            ],
        )
        .map_err(DatabaseError::Query)?;

        Ok(())
    }

    /// Insert image embedding
    pub fn insert_image_embedding(&self, image_id: &str, embedding: &[f32]) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        conn.execute(
            "INSERT OR REPLACE INTO document_image_embeddings (image_id, embedding) VALUES (?1, ?2)",
            params![image_id, embedding_bytes],
        )
        .map_err(DatabaseError::Query)?;

        Ok(())
    }

    /// Get a document image by ID (with access control info)
    pub fn get_document_image(&self, id: &str) -> ServiceResult<Option<DocumentImageWithAccess>> {
        let conn = self.conn.lock().unwrap();

        conn.query_row(
            r#"
            SELECT di.id, di.document_id, di.page_number, di.image_index, di.internal_path,
                   di.mime_type, di.width, di.height, di.description, di.created_at,
                   di.source_pages, di.image_type, di.source_image_id, di.has_region_render,
                   d.title, d.access_level
            FROM document_images di
            JOIN documents d ON di.document_id = d.id
            WHERE di.id = ?1
            "#,
            params![id],
            |row| {
                let image = DocumentImage::from_row(row)?;
                let access_level_u8: u8 = row.get(15)?;
                Ok(DocumentImageWithAccess {
                    image,
                    document_title: row.get(14)?,
                    access_level: AccessLevel::from_u8(access_level_u8),
                })
            },
        )
        .optional()
        .map_err(DatabaseError::Query)?
        .map_or(Ok(None), |img| Ok(Some(img)))
    }

    /// List document images with optional filters
    pub fn list_document_images(
        &self,
        max_access_level: u8,
        document_id: Option<&str>,
        start_page: Option<i32>,
        end_page: Option<i32>,
        limit: usize,
    ) -> ServiceResult<Vec<DocumentImageWithAccess>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            r#"
            SELECT di.id, di.document_id, di.page_number, di.image_index, di.internal_path,
                   di.mime_type, di.width, di.height, di.description, di.created_at,
                   di.source_pages, di.image_type, di.source_image_id, di.has_region_render,
                   d.title, d.access_level
            FROM document_images di
            JOIN documents d ON di.document_id = d.id
            WHERE d.access_level <= ?1
            "#,
        );

        let mut param_idx = 2;
        if document_id.is_some() {
            sql.push_str(&format!(" AND di.document_id = ?{}", param_idx));
            param_idx += 1;
        }
        if start_page.is_some() {
            sql.push_str(&format!(" AND di.page_number >= ?{}", param_idx));
            param_idx += 1;
        }
        if end_page.is_some() {
            sql.push_str(&format!(" AND di.page_number <= ?{}", param_idx));
            param_idx += 1;
        }

        sql.push_str(&format!(
            " ORDER BY d.title, di.page_number, di.image_index LIMIT ?{}",
            param_idx
        ));

        let mut stmt = conn.prepare(&sql).map_err(DatabaseError::Query)?;

        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(max_access_level)];
        if let Some(doc_id) = document_id {
            params_vec.push(Box::new(doc_id.to_string()));
        }
        if let Some(page) = start_page {
            params_vec.push(Box::new(page));
        }
        if let Some(page) = end_page {
            params_vec.push(Box::new(page));
        }
        params_vec.push(Box::new(limit as i32));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let image = DocumentImage::from_row(row)?;
                let access_level_u8: u8 = row.get(15)?;
                Ok(DocumentImageWithAccess {
                    image,
                    document_title: row.get(14)?,
                    access_level: AccessLevel::from_u8(access_level_u8),
                })
            })
            .map_err(DatabaseError::Query)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::Query)
            .map_err(Into::into)
    }

    /// Search images by description embedding similarity
    pub fn search_images(
        &self,
        query_embedding: &[f32],
        max_access_level: u8,
        limit: usize,
    ) -> ServiceResult<Vec<(DocumentImageWithAccess, f32)>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                r#"
                SELECT di.id, di.document_id, di.page_number, di.image_index, di.internal_path,
                       di.mime_type, di.width, di.height, di.description, di.created_at,
                       di.source_pages, di.image_type, di.source_image_id, di.has_region_render,
                       d.title, d.access_level, e.embedding
                FROM document_images di
                JOIN documents d ON di.document_id = d.id
                JOIN document_image_embeddings e ON di.id = e.image_id
                WHERE d.access_level <= ?1
                "#,
            )
            .map_err(DatabaseError::Query)?;

        let rows = stmt
            .query_map(params![max_access_level], |row| {
                let image = DocumentImage::from_row(row)?;
                let access_level_u8: u8 = row.get(15)?;
                let embedding_bytes: Vec<u8> = row.get(16)?;
                Ok((
                    DocumentImageWithAccess {
                        image,
                        document_title: row.get(14)?,
                        access_level: AccessLevel::from_u8(access_level_u8),
                    },
                    embedding_bytes,
                ))
            })
            .map_err(DatabaseError::Query)?;

        let mut results: Vec<(DocumentImageWithAccess, f32)> = Vec::new();

        for row in rows {
            let (image_with_access, embedding_bytes) = row.map_err(DatabaseError::Query)?;

            let embedding: Vec<f32> = embedding_bytes
                .chunks_exact(4)
                .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                .collect();

            let similarity = cosine_similarity(query_embedding, &embedding);
            results.push((image_with_access, similarity));
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);

        Ok(results)
    }

    /// Get images for a document
    pub fn get_document_images(&self, document_id: &str) -> ServiceResult<Vec<DocumentImage>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, document_id, page_number, image_index, internal_path,
                       mime_type, width, height, description, created_at, source_pages,
                       image_type, source_image_id, has_region_render
                FROM document_images
                WHERE document_id = ?1
                ORDER BY page_number, image_index
                "#,
            )
            .map_err(DatabaseError::Query)?;

        let rows = stmt
            .query_map(params![document_id], DocumentImage::from_row)
            .map_err(DatabaseError::Query)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::Query)
            .map_err(Into::into)
    }

    /// Update image description
    pub fn update_image_description(
        &self,
        image_id: &str,
        description: &str,
    ) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE document_images SET description = ?1 WHERE id = ?2",
                params![description, image_id],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows > 0)
    }

    /// Delete all images for a document (returns the internal paths for file cleanup)
    pub fn delete_document_images(&self, document_id: &str) -> ServiceResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();

        // First get the internal paths so we can delete the files
        let mut stmt = conn
            .prepare("SELECT internal_path FROM document_images WHERE document_id = ?1")
            .map_err(DatabaseError::Query)?;

        let paths: Vec<String> = stmt
            .query_map(params![document_id], |row| row.get(0))
            .map_err(DatabaseError::Query)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::Query)?;

        // Delete the database records (embeddings will cascade delete)
        conn.execute(
            "DELETE FROM document_images WHERE document_id = ?1",
            params![document_id],
        )
        .map_err(DatabaseError::Query)?;

        Ok(paths)
    }

    /// Delete a single image by ID (returns the internal path for file cleanup)
    pub fn delete_image(&self, image_id: &str) -> ServiceResult<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();

        // First get the internal path and document_id so we can delete the file and update counts
        let result: Option<(String, String)> = conn
            .query_row(
                "SELECT internal_path, document_id FROM document_images WHERE id = ?1",
                params![image_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(DatabaseError::Query)?;

        if let Some((path, doc_id)) = result {
            // Delete the database record (embedding will cascade delete)
            conn.execute(
                "DELETE FROM document_images WHERE id = ?1",
                params![image_id],
            )
            .map_err(DatabaseError::Query)?;

            Ok(Some((path, doc_id)))
        } else {
            Ok(None)
        }
    }

    /// Get count of images for a document
    pub fn get_image_count(&self, document_id: &str) -> ServiceResult<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM document_images WHERE document_id = ?1",
                params![document_id],
                |row| row.get(0),
            )
            .map_err(DatabaseError::Query)?;
        Ok(count as usize)
    }

    /// Get images for a document that don't have descriptions yet
    /// Used for resumable image captioning
    pub fn get_images_without_descriptions(
        &self,
        document_id: &str,
    ) -> ServiceResult<Vec<DocumentImage>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, document_id, page_number, image_index, internal_path,
                       mime_type, width, height, description, created_at, source_pages,
                       image_type, source_image_id, has_region_render
                FROM document_images
                WHERE document_id = ?1 AND (description IS NULL OR description = '')
                ORDER BY page_number, image_index
                "#,
            )
            .map_err(DatabaseError::Query)?;

        let images: Vec<DocumentImage> = stmt
            .query_map(params![document_id], DocumentImage::from_row)
            .map_err(DatabaseError::Query)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(images)
    }

    /// Get a cached FVTT image description by path and source
    pub fn get_fvtt_image_description(
        &self,
        image_path: &str,
        source: &str,
    ) -> ServiceResult<Option<FvttImageDescription>> {
        let conn = self.conn.lock().unwrap();

        conn.query_row(
            r#"
            SELECT id, image_path, source, description, embedding, vision_model, width, height, created_at, updated_at
            FROM fvtt_image_descriptions
            WHERE image_path = ?1 AND source = ?2
            "#,
            params![image_path, source],
            FvttImageDescription::from_row,
        )
        .optional()
        .map_err(DatabaseError::Query)
        .map_err(Into::into)
    }

    /// Insert or update a cached FVTT image description
    pub fn upsert_fvtt_image_description(&self, desc: &FvttImageDescription) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        let embedding_blob: Option<Vec<u8>> = desc
            .embedding
            .as_ref()
            .map(|emb| emb.iter().flat_map(|f| f.to_le_bytes()).collect());

        conn.execute(
            r#"
            INSERT INTO fvtt_image_descriptions
                (id, image_path, source, description, embedding, vision_model, width, height, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(image_path, source) DO UPDATE SET
                description = excluded.description,
                embedding = excluded.embedding,
                vision_model = excluded.vision_model,
                width = excluded.width,
                height = excluded.height,
                updated_at = excluded.updated_at
            "#,
            params![
                desc.id,
                desc.image_path,
                desc.source,
                desc.description,
                embedding_blob,
                desc.vision_model,
                desc.width.map(|v| v as i32),
                desc.height.map(|v| v as i32),
                desc.created_at.to_rfc3339(),
                desc.updated_at.to_rfc3339(),
            ],
        )
        .map_err(DatabaseError::Query)?;

        Ok(())
    }
}
