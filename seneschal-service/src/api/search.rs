//! Search API endpoints.
//!
//! Handlers for semantic and text search operations.

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::I18nError;
use crate::tools::{SearchFilters, TagMatch};

use super::AppState;

/// Search request
#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub user_role: u8,
    pub limit: Option<usize>,
    pub tags: Option<Vec<String>>,
    pub tags_match: Option<String>,
}

/// Search response
#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultDto>,
}

/// Search result data transfer object
#[derive(Serialize)]
pub struct SearchResultDto {
    pub chunk_id: String,
    pub document_id: String,
    pub content: String,
    pub section_title: Option<String>,
    pub page_number: Option<i32>,
    pub similarity: f32,
}

/// Perform semantic search across documents
pub async fn search_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, I18nError> {
    let filters = if request.tags.is_some() || request.tags_match.is_some() {
        Some(SearchFilters {
            tags: request.tags.unwrap_or_default(),
            tags_match: match request.tags_match.as_deref() {
                Some("all") => TagMatch::All,
                _ => TagMatch::Any,
            },
        })
    } else {
        None
    };

    let results = state
        .service
        .search(
            &request.query,
            request.user_role,
            request.limit.unwrap_or(10),
            filters,
        )
        .await
        .map_err(|e| state.i18n_error(e))?;

    Ok(Json(SearchResponse {
        results: results
            .into_iter()
            .map(|r| SearchResultDto {
                chunk_id: r.chunk.id,
                document_id: r.chunk.document_id,
                content: r.chunk.content,
                section_title: r.chunk.section_title,
                page_number: r.chunk.page_number,
                similarity: r.similarity,
            })
            .collect(),
    }))
}
