//! Conversation CRUD operations.
//!
//! This module contains all conversation-related database operations including
//! upsert, get, list, and cleanup.

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use super::Database;
use super::models::Conversation;
use crate::error::{DatabaseError, ServiceResult};

impl Database {
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
}
