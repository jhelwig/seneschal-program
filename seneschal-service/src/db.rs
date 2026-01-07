use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

use crate::error::{DatabaseError, ServiceError, ServiceResult};
use crate::tools::AccessLevel;

/// Database manager for SQLite operations
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open or create the database at the given path
    pub fn open(path: &Path) -> ServiceResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ServiceError::Database(DatabaseError::Connection(
                    rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
                ))
            })?;
        }

        let conn = Connection::open(path).map_err(DatabaseError::Connection)?;

        // Enable WAL mode for better concurrency
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(DatabaseError::Query)?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        db.run_migrations()?;

        Ok(db)
    }

    /// Run database migrations
    fn run_migrations(&self) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            -- Documents table
            CREATE TABLE IF NOT EXISTS documents (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                file_path TEXT,
                file_hash TEXT,
                access_level INTEGER NOT NULL DEFAULT 4,
                metadata TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Document tags (many-to-many)
            CREATE TABLE IF NOT EXISTS document_tags (
                document_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (document_id, tag),
                FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_document_tags_tag ON document_tags(tag);

            -- Chunks table
            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                content TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                page_number INTEGER,
                section_title TEXT,
                access_level INTEGER NOT NULL DEFAULT 4,
                metadata TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_document ON chunks(document_id);
            CREATE INDEX IF NOT EXISTS idx_chunks_access ON chunks(access_level);

            -- Chunk tags (inherited from document + chunk-specific)
            CREATE TABLE IF NOT EXISTS chunk_tags (
                chunk_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (chunk_id, tag),
                FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_chunk_tags_tag ON chunk_tags(tag);

            -- Vector embeddings table
            -- Note: sqlite-vec extension provides vector search capabilities
            -- For now we store embeddings as BLOBs and do brute-force search
            -- Can be upgraded to sqlite-vec when available
            CREATE TABLE IF NOT EXISTS chunk_embeddings (
                chunk_id TEXT PRIMARY KEY,
                embedding BLOB NOT NULL,
                FOREIGN KEY (chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
            );

            -- Conversations table
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                messages TEXT NOT NULL DEFAULT '[]',
                metadata TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_conversations_user ON conversations(user_id);
            CREATE INDEX IF NOT EXISTS idx_conversations_updated ON conversations(updated_at);

            -- Document images table
            CREATE TABLE IF NOT EXISTS document_images (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                page_number INTEGER NOT NULL,
                image_index INTEGER NOT NULL,
                internal_path TEXT NOT NULL,
                mime_type TEXT NOT NULL DEFAULT 'image/webp',
                width INTEGER,
                height INTEGER,
                description TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(document_id, page_number, image_index),
                FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_document_images_document ON document_images(document_id);

            -- Vector embeddings for image description search
            CREATE TABLE IF NOT EXISTS document_image_embeddings (
                image_id TEXT PRIMARY KEY,
                embedding BLOB NOT NULL,
                FOREIGN KEY (image_id) REFERENCES document_images(id) ON DELETE CASCADE
            );
        "#,
        )
        .map_err(|e| DatabaseError::Migration {
            message: e.to_string(),
        })?;

        // Add processing status columns (migration for existing databases)
        // SQLite doesn't have IF NOT EXISTS for ALTER TABLE, so we check if columns exist
        let has_processing_status: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('documents') WHERE name='processing_status'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_processing_status {
            conn.execute_batch(
                r#"
                ALTER TABLE documents ADD COLUMN processing_status TEXT NOT NULL DEFAULT 'completed';
                ALTER TABLE documents ADD COLUMN processing_error TEXT;
                ALTER TABLE documents ADD COLUMN chunk_count INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE documents ADD COLUMN image_count INTEGER NOT NULL DEFAULT 0;
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to add processing columns: {}", e),
            })?;
        }

        // Migration: Add progress tracking columns
        let has_processing_phase: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('documents') WHERE name='processing_phase'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_processing_phase {
            conn.execute_batch(
                r#"
                ALTER TABLE documents ADD COLUMN processing_phase TEXT;
                ALTER TABLE documents ADD COLUMN processing_progress INTEGER;
                ALTER TABLE documents ADD COLUMN processing_total INTEGER;
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to add progress tracking columns: {}", e),
            })?;
        }

        Ok(())
    }

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
            INSERT INTO documents (id, title, file_path, file_hash, access_level, metadata, created_at, updated_at, processing_status, processing_error, chunk_count, image_count, processing_phase, processing_progress, processing_total)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
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
                doc.chunk_count as i64,
                doc.image_count as i64,
                doc.processing_phase,
                doc.processing_progress.map(|p| p as i64),
                doc.processing_total.map(|t| t as i64),
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
                "SELECT id, title, file_path, file_hash, access_level, metadata, created_at, updated_at, processing_status, processing_error, chunk_count, image_count, processing_phase, processing_progress, processing_total FROM documents WHERE id = ?1",
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

    /// List all documents with optional access level filter
    pub fn list_documents(&self, max_access_level: Option<u8>) -> ServiceResult<Vec<Document>> {
        let conn = self.conn.lock().unwrap();

        let mut docs = Vec::new();

        if let Some(level) = max_access_level {
            let mut stmt = conn
                .prepare("SELECT id, title, file_path, file_hash, access_level, metadata, created_at, updated_at, processing_status, processing_error, chunk_count, image_count, processing_phase, processing_progress, processing_total FROM documents WHERE access_level <= ?1 ORDER BY title")
                .map_err(DatabaseError::Query)?;
            let rows = stmt
                .query_map(params![level], |row| Document::from_row(row, vec![]))
                .map_err(DatabaseError::Query)?;
            for row in rows {
                docs.push(row.map_err(DatabaseError::Query)?);
            }
        } else {
            let mut stmt = conn
                .prepare("SELECT id, title, file_path, file_hash, access_level, metadata, created_at, updated_at, processing_status, processing_error, chunk_count, image_count, processing_phase, processing_progress, processing_total FROM documents ORDER BY title")
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

    /// Insert a chunk
    pub fn insert_chunk(&self, chunk: &Chunk) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        let metadata_json = chunk
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(DatabaseError::Serialization)?;

        conn.execute(
            r#"
            INSERT INTO chunks (id, document_id, content, chunk_index, page_number, section_title, access_level, metadata, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                chunk.id,
                chunk.document_id,
                chunk.content,
                chunk.chunk_index,
                chunk.page_number,
                chunk.section_title,
                chunk.access_level as u8,
                metadata_json,
                chunk.created_at.to_rfc3339(),
            ],
        )
        .map_err(DatabaseError::Query)?;

        // Insert tags
        for tag in &chunk.tags {
            conn.execute(
                "INSERT OR IGNORE INTO chunk_tags (chunk_id, tag) VALUES (?1, ?2)",
                params![chunk.id, tag],
            )
            .map_err(DatabaseError::Query)?;
        }

        Ok(())
    }

    /// Insert chunk embedding
    pub fn insert_embedding(&self, chunk_id: &str, embedding: &[f32]) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        // Convert f32 slice to bytes
        let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        conn.execute(
            "INSERT OR REPLACE INTO chunk_embeddings (chunk_id, embedding) VALUES (?1, ?2)",
            params![chunk_id, embedding_bytes],
        )
        .map_err(DatabaseError::Query)?;

        Ok(())
    }

    /// Get chunk by ID
    pub fn get_chunk(&self, id: &str) -> ServiceResult<Option<Chunk>> {
        let conn = self.conn.lock().unwrap();

        let chunk = conn
            .query_row(
                "SELECT id, document_id, content, chunk_index, page_number, section_title, access_level, metadata, created_at FROM chunks WHERE id = ?1",
                params![id],
                |row| Chunk::from_row(row, vec![]),
            )
            .optional()
            .map_err(DatabaseError::Query)?;

        if let Some(mut chunk) = chunk {
            // Load tags
            let mut stmt = conn
                .prepare("SELECT tag FROM chunk_tags WHERE chunk_id = ?1")
                .map_err(DatabaseError::Query)?;
            chunk.tags = stmt
                .query_map(params![id], |row| row.get(0))
                .map_err(DatabaseError::Query)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(Some(chunk))
        } else {
            Ok(None)
        }
    }

    /// Search chunks by embedding similarity (brute force for now)
    pub fn search_chunks(
        &self,
        query_embedding: &[f32],
        max_access_level: u8,
        limit: usize,
        tag_filter: Option<&[String]>,
        tag_match_all: bool,
    ) -> ServiceResult<Vec<(Chunk, f32)>> {
        let conn = self.conn.lock().unwrap();

        // Build query based on filters
        let mut sql = String::from(
            r#"
            SELECT c.id, c.document_id, c.content, c.chunk_index, c.page_number,
                   c.section_title, c.access_level, c.metadata, c.created_at, e.embedding
            FROM chunks c
            JOIN chunk_embeddings e ON c.id = e.chunk_id
            WHERE c.access_level <= ?1
            "#,
        );

        if let Some(tags) = tag_filter
            && !tags.is_empty()
        {
            if tag_match_all {
                // All tags must match
                for (i, _) in tags.iter().enumerate() {
                    sql.push_str(&format!(
                        " AND EXISTS (SELECT 1 FROM chunk_tags ct WHERE ct.chunk_id = c.id AND ct.tag = ?{})",
                        i + 2
                    ));
                }
            } else {
                // Any tag matches
                let placeholders: Vec<String> =
                    (0..tags.len()).map(|i| format!("?{}", i + 2)).collect();
                sql.push_str(&format!(
                    " AND EXISTS (SELECT 1 FROM chunk_tags ct WHERE ct.chunk_id = c.id AND ct.tag IN ({}))",
                    placeholders.join(", ")
                ));
            }
        }

        let mut stmt = conn.prepare(&sql).map_err(DatabaseError::Query)?;

        // Build params
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(max_access_level)];
        if let Some(tags) = tag_filter {
            for tag in tags {
                params_vec.push(Box::new(tag.clone()));
            }
        }

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let embedding_bytes: Vec<u8> = row.get(9)?;
                let chunk = Chunk::from_row(row, vec![])?;
                Ok((chunk, embedding_bytes))
            })
            .map_err(DatabaseError::Query)?;

        // Calculate similarities and sort
        let mut results: Vec<(Chunk, f32)> = Vec::new();

        for row in rows {
            let (mut chunk, embedding_bytes) = row.map_err(DatabaseError::Query)?;

            // Convert bytes back to f32 slice
            let embedding: Vec<f32> = embedding_bytes
                .chunks_exact(4)
                .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                .collect();

            // Calculate cosine similarity
            let similarity = cosine_similarity(query_embedding, &embedding);

            // Load tags
            let mut tag_stmt = conn
                .prepare("SELECT tag FROM chunk_tags WHERE chunk_id = ?1")
                .map_err(DatabaseError::Query)?;
            chunk.tags = tag_stmt
                .query_map(params![chunk.id], |row| row.get(0))
                .map_err(DatabaseError::Query)?
                .filter_map(|r| r.ok())
                .collect();

            results.push((chunk, similarity));
        }

        // Sort by similarity (descending)
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top N
        results.truncate(limit);

        Ok(results)
    }

    /// Insert or update a conversation
    pub fn upsert_conversation(&self, conv: &Conversation) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        let messages_json =
            serde_json::to_string(&conv.messages).map_err(DatabaseError::Serialization)?;
        let metadata_json = conv
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(DatabaseError::Serialization)?;

        conn.execute(
            r#"
            INSERT INTO conversations (id, user_id, created_at, updated_at, messages, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                updated_at = excluded.updated_at,
                messages = excluded.messages,
                metadata = excluded.metadata
            "#,
            params![
                conv.id,
                conv.user_id,
                conv.created_at.to_rfc3339(),
                conv.updated_at.to_rfc3339(),
                messages_json,
                metadata_json,
            ],
        )
        .map_err(DatabaseError::Query)?;

        Ok(())
    }

    /// Get a conversation by ID
    pub fn get_conversation(&self, id: &str) -> ServiceResult<Option<Conversation>> {
        let conn = self.conn.lock().unwrap();

        conn.query_row(
            "SELECT id, user_id, created_at, updated_at, messages, metadata FROM conversations WHERE id = ?1",
            params![id],
            Conversation::from_row,
        )
        .optional()
        .map_err(DatabaseError::Query)?
        .map_or(Ok(None), |c| Ok(Some(c)))
    }

    /// List conversations for a user
    pub fn list_conversations(
        &self,
        user_id: &str,
        limit: usize,
    ) -> ServiceResult<Vec<Conversation>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, created_at, updated_at, messages, metadata FROM conversations WHERE user_id = ?1 ORDER BY updated_at DESC LIMIT ?2",
            )
            .map_err(DatabaseError::Query)?;

        let rows = stmt
            .query_map(params![user_id, limit], Conversation::from_row)
            .map_err(DatabaseError::Query)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::Query)
            .map_err(Into::into)
    }

    /// Delete old conversations
    pub fn cleanup_old_conversations(&self, older_than: DateTime<Utc>) -> ServiceResult<usize> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "DELETE FROM conversations WHERE updated_at < ?1",
                params![older_than.to_rfc3339()],
            )
            .map_err(DatabaseError::Query)?;

        Ok(rows)
    }

    /// Delete excess conversations for all users (keeping most recent per user)
    pub fn cleanup_excess_conversations_all(&self, max_per_user: u32) -> ServiceResult<usize> {
        if max_per_user == 0 {
            return Ok(0);
        }

        let conn = self.conn.lock().unwrap();

        // Get all distinct user_ids
        let mut stmt = conn
            .prepare("SELECT DISTINCT user_id FROM conversations")
            .map_err(DatabaseError::Query)?;

        let user_ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(DatabaseError::Query)?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);

        let mut total_deleted = 0;

        for user_id in user_ids {
            let rows = conn
                .execute(
                    r#"
                    DELETE FROM conversations
                    WHERE user_id = ?1 AND id NOT IN (
                        SELECT id FROM conversations
                        WHERE user_id = ?1
                        ORDER BY updated_at DESC
                        LIMIT ?2
                    )
                    "#,
                    params![user_id, max_per_user],
                )
                .map_err(DatabaseError::Query)?;

            total_deleted += rows;
        }

        Ok(total_deleted)
    }

    /// Insert a document image
    pub fn insert_document_image(&self, image: &DocumentImage) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            r#"
            INSERT INTO document_images (id, document_id, page_number, image_index, internal_path, mime_type, width, height, description, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
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
                   d.title, d.access_level
            FROM document_images di
            JOIN documents d ON di.document_id = d.id
            WHERE di.id = ?1
            "#,
            params![id],
            |row| {
                let image = DocumentImage::from_row(row)?;
                let access_level_u8: u8 = row.get(11)?;
                Ok(DocumentImageWithAccess {
                    image,
                    document_title: row.get(10)?,
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
        page_number: Option<i32>,
        limit: usize,
    ) -> ServiceResult<Vec<DocumentImageWithAccess>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            r#"
            SELECT di.id, di.document_id, di.page_number, di.image_index, di.internal_path,
                   di.mime_type, di.width, di.height, di.description, di.created_at,
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
        if page_number.is_some() {
            sql.push_str(&format!(" AND di.page_number = ?{}", param_idx));
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
        if let Some(page) = page_number {
            params_vec.push(Box::new(page));
        }
        params_vec.push(Box::new(limit as i32));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let image = DocumentImage::from_row(row)?;
                let access_level_u8: u8 = row.get(11)?;
                Ok(DocumentImageWithAccess {
                    image,
                    document_title: row.get(10)?,
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
                let access_level_u8: u8 = row.get(11)?;
                let embedding_bytes: Vec<u8> = row.get(12)?;
                Ok((
                    DocumentImageWithAccess {
                        image,
                        document_title: row.get(10)?,
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
                       mime_type, width, height, description, created_at
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

            // Update the document's image count
            conn.execute(
                "UPDATE documents SET image_count = (SELECT COUNT(*) FROM document_images WHERE document_id = ?1), updated_at = datetime('now') WHERE id = ?1",
                params![doc_id],
            )
            .map_err(DatabaseError::Query)?;

            Ok(Some((path, doc_id)))
        } else {
            Ok(None)
        }
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

    /// Update document chunk and image counts
    pub fn update_document_counts(
        &self,
        document_id: &str,
        chunk_count: usize,
        image_count: usize,
    ) -> ServiceResult<bool> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute(
                "UPDATE documents SET chunk_count = ?1, image_count = ?2, updated_at = datetime('now') WHERE id = ?3",
                params![chunk_count as i64, image_count as i64, document_id],
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

    /// Get the next document pending processing (oldest first)
    /// Used by the document processing worker queue
    pub fn get_next_pending_document(&self) -> ServiceResult<Option<Document>> {
        let conn = self.conn.lock().unwrap();

        let doc = conn
            .query_row(
                "SELECT id, title, file_path, file_hash, access_level, metadata, created_at, updated_at, processing_status, processing_error, chunk_count, image_count, processing_phase, processing_progress, processing_total FROM documents WHERE processing_status = 'processing' ORDER BY created_at ASC LIMIT 1",
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

    /// Get chunks for a document that don't have embeddings yet
    /// Used for resumable document processing
    pub fn get_chunks_without_embeddings(&self, document_id: &str) -> ServiceResult<Vec<Chunk>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                r#"
                SELECT c.id, c.document_id, c.content, c.chunk_index, c.page_number,
                       c.section_title, c.access_level, c.metadata, c.created_at
                FROM chunks c
                LEFT JOIN chunk_embeddings ce ON c.id = ce.chunk_id
                WHERE c.document_id = ?1 AND ce.chunk_id IS NULL
                ORDER BY c.chunk_index
                "#,
            )
            .map_err(DatabaseError::Query)?;

        let chunks: Vec<Chunk> = stmt
            .query_map(params![document_id], |row| {
                let access_level_u8: u8 = row.get(6)?;
                let metadata_str: Option<String> = row.get(7)?;
                let created_at_str: String = row.get(8)?;

                Ok(Chunk {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    content: row.get(2)?,
                    chunk_index: row.get(3)?,
                    page_number: row.get(4)?,
                    section_title: row.get(5)?,
                    access_level: AccessLevel::from_u8(access_level_u8),
                    tags: vec![], // Tags loaded separately if needed
                    metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                })
            })
            .map_err(DatabaseError::Query)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(chunks)
    }

    /// Get count of chunks for a document
    pub fn get_chunk_count(&self, document_id: &str) -> ServiceResult<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE document_id = ?1",
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
                       mime_type, width, height, description, created_at
                FROM document_images
                WHERE document_id = ?1 AND (description IS NULL OR description = '')
                ORDER BY page_number, image_index
                "#,
            )
            .map_err(DatabaseError::Query)?;

        let images: Vec<DocumentImage> = stmt
            .query_map(params![document_id], |row| {
                let created_at_str: String = row.get(9)?;
                Ok(DocumentImage {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    page_number: row.get(2)?,
                    image_index: row.get(3)?,
                    internal_path: row.get(4)?,
                    mime_type: row.get(5)?,
                    width: row.get(6)?,
                    height: row.get(7)?,
                    description: row.get(8)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                })
            })
            .map_err(DatabaseError::Query)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(images)
    }
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Document processing status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingStatus {
    /// Document is being processed (text extraction, embeddings, etc.)
    Processing,
    /// Document processing completed successfully
    Completed,
    /// Document processing failed
    Failed,
}

impl ProcessingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessingStatus::Processing => "processing",
            ProcessingStatus::Completed => "completed",
            ProcessingStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "processing" => ProcessingStatus::Processing,
            "failed" => ProcessingStatus::Failed,
            _ => ProcessingStatus::Completed,
        }
    }
}

/// Document record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub file_path: Option<String>,
    pub file_hash: Option<String>,
    pub access_level: AccessLevel,
    pub tags: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub processing_status: ProcessingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_error: Option<String>,
    pub chunk_count: usize,
    pub image_count: usize,
    /// Current processing phase (e.g., "chunking", "embedding", "extracting_images", "captioning")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_phase: Option<String>,
    /// Current progress within the phase
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_progress: Option<usize>,
    /// Total items in the current phase
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_total: Option<usize>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Document {
    fn from_row(row: &Row<'_>, tags: Vec<String>) -> Result<Self, rusqlite::Error> {
        let access_level_u8: u8 = row.get(4)?;
        let metadata_str: Option<String> = row.get(5)?;
        let created_at_str: String = row.get(6)?;
        let updated_at_str: String = row.get(7)?;
        let processing_status_str: String = row.get(8)?;
        let processing_error: Option<String> = row.get(9)?;
        let chunk_count: i64 = row.get(10)?;
        let image_count: i64 = row.get(11)?;
        let processing_phase: Option<String> = row.get(12)?;
        let processing_progress: Option<i64> = row.get(13)?;
        let processing_total: Option<i64> = row.get(14)?;

        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            file_path: row.get(2)?,
            file_hash: row.get(3)?,
            access_level: AccessLevel::from_u8(access_level_u8),
            tags,
            metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
            processing_status: ProcessingStatus::from_str(&processing_status_str),
            processing_error,
            chunk_count: chunk_count as usize,
            image_count: image_count as usize,
            processing_phase,
            processing_progress: processing_progress.map(|p| p as usize),
            processing_total: processing_total.map(|t| t as usize),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

/// Chunk record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub content: String,
    pub chunk_index: i32,
    pub page_number: Option<i32>,
    pub section_title: Option<String>,
    pub access_level: AccessLevel,
    pub tags: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl Chunk {
    fn from_row(row: &Row<'_>, tags: Vec<String>) -> Result<Self, rusqlite::Error> {
        let access_level_u8: u8 = row.get(6)?;
        let metadata_str: Option<String> = row.get(7)?;
        let created_at_str: String = row.get(8)?;

        Ok(Self {
            id: row.get(0)?,
            document_id: row.get(1)?,
            content: row.get(2)?,
            chunk_index: row.get(3)?,
            page_number: row.get(4)?,
            section_title: row.get(5)?,
            access_level: AccessLevel::from_u8(access_level_u8),
            tags,
            metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

/// Conversation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ConversationMessage>,
    pub metadata: Option<ConversationMetadata>,
}

impl Conversation {
    fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        let messages_str: String = row.get(4)?;
        let metadata_str: Option<String> = row.get(5)?;
        let created_at_str: String = row.get(2)?;
        let updated_at_str: String = row.get(3)?;

        Ok(Self {
            id: row.get(0)?,
            user_id: row.get(1)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            messages: serde_json::from_str(&messages_str).unwrap_or_default(),
            metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
        })
    }
}

/// Message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_results: Option<Vec<ToolResultRecord>>,
}

/// Message role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Tool call record for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub tool: String,
    pub args: serde_json::Value,
}

/// Tool result record for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub tool_call_id: String,
    pub result: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Conversation metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationMetadata {
    #[serde(default)]
    pub active_document_ids: Vec<String>,
    #[serde(default)]
    pub active_actor_ids: Vec<String>,
    #[serde(default)]
    pub total_tokens_estimate: u32,
}

/// Document image record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentImage {
    pub id: String,
    pub document_id: String,
    pub page_number: i32,
    pub image_index: i32,
    pub internal_path: String,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl DocumentImage {
    fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        let created_at_str: String = row.get(9)?;

        Ok(Self {
            id: row.get(0)?,
            document_id: row.get(1)?,
            page_number: row.get(2)?,
            image_index: row.get(3)?,
            internal_path: row.get(4)?,
            mime_type: row.get(5)?,
            width: row.get::<_, Option<i32>>(6)?.map(|v| v as u32),
            height: row.get::<_, Option<i32>>(7)?.map(|v| v as u32),
            description: row.get(8)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

/// Document image with parent document info (for access control)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentImageWithAccess {
    #[serde(flatten)]
    pub image: DocumentImage,
    pub document_title: String,
    pub access_level: AccessLevel,
}
