mod models;

pub use models::{
    CaptioningStatus, Chunk, Conversation, ConversationMessage, ConversationMetadata, Document,
    DocumentImage, DocumentImageWithAccess, FvttImageDescription, ImageType, MessageRole,
    ProcessingStatus, ToolCallRecord, ToolResultRecord,
};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
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
                source_pages TEXT,
                image_type TEXT NOT NULL DEFAULT 'individual',
                source_image_id TEXT,
                has_region_render INTEGER NOT NULL DEFAULT 0,
                UNIQUE(document_id, page_number, image_index, image_type),
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

        // Migration: Add source_pages column to document_images
        let has_source_pages: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('document_images') WHERE name='source_pages'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_source_pages {
            conn.execute(
                "ALTER TABLE document_images ADD COLUMN source_pages TEXT",
                [],
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to add source_pages column: {}", e),
            })?;
        }

        // Migration: Add image_type, source_image_id, has_region_render columns to document_images
        // and update the unique constraint to include image_type
        let has_image_type: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('document_images') WHERE name='image_type'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_image_type {
            // Need to recreate the table to update the UNIQUE constraint
            // SQLite doesn't support ALTER TABLE to modify constraints
            conn.execute_batch(
                r#"
                -- Create new table with updated schema
                CREATE TABLE document_images_new (
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
                    source_pages TEXT,
                    image_type TEXT NOT NULL DEFAULT 'individual',
                    source_image_id TEXT,
                    has_region_render INTEGER NOT NULL DEFAULT 0,
                    UNIQUE(document_id, page_number, image_index, image_type),
                    FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
                );

                -- Copy existing data
                INSERT INTO document_images_new (id, document_id, page_number, image_index, internal_path, mime_type, width, height, description, created_at, source_pages)
                SELECT id, document_id, page_number, image_index, internal_path, mime_type, width, height, description, created_at, source_pages
                FROM document_images;

                -- Drop old table
                DROP TABLE document_images;

                -- Rename new table
                ALTER TABLE document_images_new RENAME TO document_images;

                -- Recreate indexes
                CREATE INDEX IF NOT EXISTS idx_document_images_document ON document_images(document_id);
                CREATE INDEX IF NOT EXISTS idx_document_images_source ON document_images(source_image_id);
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to migrate document_images table: {}", e),
            })?;
        }

        // Migration: Drop denormalized chunk_count and image_count columns
        // These are now computed dynamically via SQL subqueries
        let has_chunk_count: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('documents') WHERE name='chunk_count'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if has_chunk_count {
            conn.execute_batch(
                r#"
                ALTER TABLE documents DROP COLUMN chunk_count;
                ALTER TABLE documents DROP COLUMN image_count;
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to drop chunk_count/image_count columns: {}", e),
            })?;
        }

        // Migration: Add fvtt_image_descriptions table for caching on-demand vision descriptions
        let has_fvtt_image_descriptions: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='fvtt_image_descriptions'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_fvtt_image_descriptions {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS fvtt_image_descriptions (
                    id TEXT PRIMARY KEY,
                    image_path TEXT NOT NULL,
                    source TEXT NOT NULL DEFAULT 'data',
                    description TEXT NOT NULL,
                    embedding BLOB,
                    vision_model TEXT NOT NULL,
                    width INTEGER,
                    height INTEGER,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                    UNIQUE(image_path, source)
                );

                CREATE INDEX IF NOT EXISTS idx_fvtt_image_descriptions_path
                    ON fvtt_image_descriptions(image_path);
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to create fvtt_image_descriptions table: {}", e),
            })?;
        }

        // Migration: Add FTS5 virtual table for full-text search
        let has_chunks_fts: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_chunks_fts {
            conn.execute_batch(
                r#"
                -- FTS5 virtual table for full-text search on chunks
                CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                    content,
                    section_title,
                    document_id UNINDEXED,
                    chunk_id UNINDEXED,
                    page_number UNINDEXED,
                    content='chunks',
                    content_rowid='rowid'
                );

                -- Triggers to keep FTS in sync with chunks table
                CREATE TRIGGER IF NOT EXISTS chunks_fts_ai AFTER INSERT ON chunks BEGIN
                    INSERT INTO chunks_fts(rowid, content, section_title, document_id, chunk_id, page_number)
                    VALUES (new.rowid, new.content, new.section_title, new.document_id, new.id, new.page_number);
                END;

                CREATE TRIGGER IF NOT EXISTS chunks_fts_ad AFTER DELETE ON chunks BEGIN
                    INSERT INTO chunks_fts(chunks_fts, rowid, content, section_title, document_id, chunk_id, page_number)
                    VALUES ('delete', old.rowid, old.content, old.section_title, old.document_id, old.id, old.page_number);
                END;

                CREATE TRIGGER IF NOT EXISTS chunks_fts_au AFTER UPDATE ON chunks BEGIN
                    INSERT INTO chunks_fts(chunks_fts, rowid, content, section_title, document_id, chunk_id, page_number)
                    VALUES ('delete', old.rowid, old.content, old.section_title, old.document_id, old.id, old.page_number);
                    INSERT INTO chunks_fts(rowid, content, section_title, document_id, chunk_id, page_number)
                    VALUES (new.rowid, new.content, new.section_title, new.document_id, new.id, new.page_number);
                END;
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to create FTS5 table: {}", e),
            })?;

            // Populate FTS index from existing chunks
            conn.execute(
                "INSERT INTO chunks_fts(rowid, content, section_title, document_id, chunk_id, page_number) SELECT rowid, content, section_title, document_id, id, page_number FROM chunks",
                [],
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to populate FTS5 index: {}", e),
            })?;
        }

        // Migration: Add captioning status columns for separate background captioning
        let has_captioning_status: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('documents') WHERE name='captioning_status'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_captioning_status {
            conn.execute_batch(
                r#"
                ALTER TABLE documents ADD COLUMN captioning_status TEXT NOT NULL DEFAULT 'not_requested';
                ALTER TABLE documents ADD COLUMN captioning_error TEXT;
                ALTER TABLE documents ADD COLUMN captioning_progress INTEGER;
                ALTER TABLE documents ADD COLUMN captioning_total INTEGER;
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to add captioning columns: {}", e),
            })?;

            // Migrate existing documents:
            // 1. Documents currently in captioning phase -> set captioning_status = 'pending', mark document completed
            // 2. Completed documents with vision_model but uncaptioned images -> set captioning_status = 'pending'
            conn.execute_batch(
                r#"
                -- Documents currently in captioning phase: queue them for the new worker
                UPDATE documents
                SET captioning_status = 'pending',
                    processing_status = 'completed',
                    processing_phase = NULL,
                    processing_progress = NULL,
                    processing_total = NULL
                WHERE processing_status = 'processing'
                AND processing_phase = 'captioning';

                -- Completed documents with vision_model but uncaptioned images
                UPDATE documents
                SET captioning_status = 'pending'
                WHERE processing_status = 'completed'
                AND metadata LIKE '%"vision_model":%'
                AND captioning_status = 'not_requested'
                AND id IN (
                    SELECT document_id FROM document_images
                    WHERE description IS NULL OR description = ''
                );
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to migrate existing documents for captioning: {}", e),
            })?;
        }

        // Migration: Add settings table for FVTT-managed backend configuration
        let has_settings_table: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
            > 0;

        if !has_settings_table {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                "#,
            )
            .map_err(|e| DatabaseError::Migration {
                message: format!("Failed to create settings table: {}", e),
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

    /// Get all chunks for a specific page of a document
    pub fn get_chunks_by_page(
        &self,
        document_id: &str,
        page_number: i32,
        max_access_level: u8,
    ) -> ServiceResult<Vec<Chunk>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, document_id, content, chunk_index, page_number, section_title,
                       access_level, metadata, created_at
                FROM chunks
                WHERE document_id = ?1 AND page_number = ?2 AND access_level <= ?3
                ORDER BY chunk_index
                "#,
            )
            .map_err(DatabaseError::Query)?;

        let chunks: Vec<Chunk> = stmt
            .query_map(params![document_id, page_number, max_access_level], |row| {
                Chunk::from_row(row, vec![])
            })
            .map_err(DatabaseError::Query)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(chunks)
    }

    /// Search chunks using full-text search (FTS5)
    pub fn search_chunks_fts(
        &self,
        query: &str,
        section_filter: Option<&str>,
        document_id: Option<&str>,
        max_access_level: u8,
        limit: usize,
    ) -> ServiceResult<Vec<Chunk>> {
        let conn = self.conn.lock().unwrap();

        // Build the FTS query - escape special characters
        let fts_query = query
            .split_whitespace()
            .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");

        // Build the SQL query with optional filters
        let mut sql = String::from(
            r#"
            SELECT c.id, c.document_id, c.content, c.chunk_index, c.page_number,
                   c.section_title, c.access_level, c.metadata, c.created_at
            FROM chunks c
            JOIN chunks_fts fts ON c.id = fts.chunk_id
            WHERE chunks_fts MATCH ?1 AND c.access_level <= ?2
            "#,
        );

        let mut param_idx = 3;
        if section_filter.is_some() {
            sql.push_str(&format!(
                " AND c.section_title LIKE '%' || ?{} || '%'",
                param_idx
            ));
            param_idx += 1;
        }
        if document_id.is_some() {
            sql.push_str(&format!(" AND c.document_id = ?{}", param_idx));
            param_idx += 1;
        }

        sql.push_str(&format!(" ORDER BY bm25(chunks_fts) LIMIT ?{}", param_idx));

        let mut stmt = conn.prepare(&sql).map_err(DatabaseError::Query)?;

        // Build params
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(fts_query), Box::new(max_access_level)];
        if let Some(section) = section_filter {
            params_vec.push(Box::new(section.to_string()));
        }
        if let Some(doc_id) = document_id {
            params_vec.push(Box::new(doc_id.to_string()));
        }
        params_vec.push(Box::new(limit as i32));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut chunks: Vec<Chunk> = stmt
            .query_map(params_refs.as_slice(), |row| Chunk::from_row(row, vec![]))
            .map_err(DatabaseError::Query)?
            .filter_map(|r| r.ok())
            .collect();

        // Load tags for each chunk
        for chunk in &mut chunks {
            let mut tag_stmt = conn
                .prepare("SELECT tag FROM chunk_tags WHERE chunk_id = ?1")
                .map_err(DatabaseError::Query)?;
            chunk.tags = tag_stmt
                .query_map(params![chunk.id], |row| row.get(0))
                .map_err(DatabaseError::Query)?
                .filter_map(|r| r.ok())
                .collect();
        }

        Ok(chunks)
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
        status: models::CaptioningStatus,
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

    // ==================== Settings CRUD ====================

    /// Get all settings as a map
    pub fn get_all_settings(
        &self,
    ) -> ServiceResult<std::collections::HashMap<String, serde_json::Value>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare("SELECT key, value FROM settings")
            .map_err(DatabaseError::Query)?;

        let rows = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let value_str: String = row.get(1)?;
                Ok((key, value_str))
            })
            .map_err(DatabaseError::Query)?;

        let mut settings = std::collections::HashMap::new();
        for row in rows {
            let (key, value_str) = row.map_err(DatabaseError::Query)?;
            if let Ok(value) = serde_json::from_str(&value_str) {
                settings.insert(key, value);
            }
        }

        Ok(settings)
    }

    /// Set multiple settings in a single transaction
    /// Null values delete the setting (revert to default)
    pub fn set_settings(
        &self,
        settings: std::collections::HashMap<String, serde_json::Value>,
    ) -> ServiceResult<()> {
        let conn = self.conn.lock().unwrap();

        for (key, value) in settings {
            if value.is_null() {
                // Null means delete (revert to default)
                conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
                    .map_err(DatabaseError::Query)?;
            } else {
                let value_str =
                    serde_json::to_string(&value).map_err(DatabaseError::Serialization)?;
                conn.execute(
                    "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, datetime('now')) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![key, value_str],
                )
                .map_err(DatabaseError::Query)?;
            }
        }

        Ok(())
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
