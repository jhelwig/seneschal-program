//! Database module for SQLite operations.
//!
//! This module provides the `Database` struct and all database operations
//! organized into submodules by domain.

mod chunks;
mod conversations;
mod documents;
mod images;
mod migrations;
pub mod models;
mod settings;

pub use models::{
    CaptioningStatus, Chunk, Conversation, ConversationMessage, ConversationMetadata, Document,
    DocumentImage, DocumentImageWithAccess, FvttImageDescription, ImageType, MessageRole,
    ProcessingStatus, ToolCallRecord, ToolResultRecord,
};

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use crate::error::{DatabaseError, ServiceError, ServiceResult};

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

        // Run all migrations
        migrations::run_migrations(&conn)?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        Ok(db)
    }
}
