//! Database schema migrations.
//!
//! This module contains all database migrations and schema setup.

use rusqlite::Connection;

use crate::error::{DatabaseError, ServiceResult};

/// Run all database migrations.
///
/// This function is called during database initialization to ensure
/// the schema is up to date.
pub(super) fn run_migrations(conn: &Connection) -> ServiceResult<()> {
    // Initial schema setup
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
    run_processing_status_migration(conn)?;
    run_progress_tracking_migration(conn)?;
    run_source_pages_migration(conn)?;
    run_image_type_migration(conn)?;
    run_drop_denormalized_counts_migration(conn)?;
    run_fvtt_image_descriptions_migration(conn)?;
    run_fts5_migration(conn)?;
    run_captioning_status_migration(conn)?;
    run_settings_table_migration(conn)?;
    run_image_type_rename_migration(conn)?;

    Ok(())
}

/// Migration: Add processing status columns to documents table
fn run_processing_status_migration(conn: &Connection) -> ServiceResult<()> {
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

    Ok(())
}

/// Migration: Add progress tracking columns to documents table
fn run_progress_tracking_migration(conn: &Connection) -> ServiceResult<()> {
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

/// Migration: Add source_pages column to document_images
fn run_source_pages_migration(conn: &Connection) -> ServiceResult<()> {
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

    Ok(())
}

/// Migration: Add image_type, source_image_id, has_region_render columns to document_images
/// and update the unique constraint to include image_type
fn run_image_type_migration(conn: &Connection) -> ServiceResult<()> {
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

    Ok(())
}

/// Migration: Drop denormalized chunk_count and image_count columns
/// These are now computed dynamically via SQL subqueries
fn run_drop_denormalized_counts_migration(conn: &Connection) -> ServiceResult<()> {
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

    Ok(())
}

/// Migration: Add fvtt_image_descriptions table for caching on-demand vision descriptions
fn run_fvtt_image_descriptions_migration(conn: &Connection) -> ServiceResult<()> {
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

    Ok(())
}

/// Migration: Add FTS5 virtual table for full-text search
fn run_fts5_migration(conn: &Connection) -> ServiceResult<()> {
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

    Ok(())
}

/// Migration: Add captioning status columns for separate background captioning
fn run_captioning_status_migration(conn: &Connection) -> ServiceResult<()> {
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

    Ok(())
}

/// Migration: Add settings table for FVTT-managed backend configuration
fn run_settings_table_migration(conn: &Connection) -> ServiceResult<()> {
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

/// Migration: Rename image_type 'region_render' to 'render'
fn run_image_type_rename_migration(conn: &Connection) -> ServiceResult<()> {
    conn.execute(
        "UPDATE document_images SET image_type = 'render' WHERE image_type = 'region_render'",
        [],
    )
    .map_err(|e| DatabaseError::Migration {
        message: format!("Failed to rename region_render to render: {}", e),
    })?;

    Ok(())
}
