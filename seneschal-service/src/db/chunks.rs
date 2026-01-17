//! Chunk CRUD operations, FTS search, and embedding search.
//!
//! This module contains all chunk-related database operations including
//! insert, search (full-text and semantic), and embedding management.

use chrono::Utc;
use rusqlite::params;

use super::Database;
use super::models::Chunk;
use crate::error::{DatabaseError, ServiceResult};
use crate::tools::AccessLevel;

impl Database {
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
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
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
}

/// Calculate cosine similarity between two vectors
pub(super) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
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
