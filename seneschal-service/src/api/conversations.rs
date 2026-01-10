//! Conversation API endpoints.
//!
//! Handlers for listing and retrieving chat conversations.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::db::Conversation;
use crate::error::{I18nError, ServiceError};

use super::AppState;

/// Query parameters for listing conversations
#[derive(Deserialize)]
pub struct ListConversationsParams {
    pub user_id: Option<String>,
    pub limit: Option<usize>,
}

/// Summary of a conversation for listing
#[derive(Serialize)]
pub struct ConversationSummary {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

/// List conversations for a user
pub async fn list_conversations_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListConversationsParams>,
) -> Result<Json<Vec<ConversationSummary>>, I18nError> {
    let user_id = params.user_id.ok_or_else(|| {
        state.i18n_error(ServiceError::InvalidRequest {
            message: "user_id is required".to_string(),
        })
    })?;

    let conversations = state
        .service
        .list_conversations(&user_id, params.limit.unwrap_or(20))
        .map_err(|e| state.i18n_error(e))?;

    Ok(Json(
        conversations
            .into_iter()
            .map(|c| ConversationSummary {
                id: c.id,
                created_at: c.created_at.to_rfc3339(),
                updated_at: c.updated_at.to_rfc3339(),
                message_count: c.messages.len(),
            })
            .collect(),
    ))
}

/// Get a specific conversation by ID
pub async fn get_conversation_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Conversation>, I18nError> {
    let conversation = state
        .service
        .get_conversation(&id)
        .map_err(|e| state.i18n_error(e))?
        .ok_or_else(|| {
            state.i18n_error(ServiceError::ConversationNotFound {
                conversation_id: id,
            })
        })?;

    Ok(Json(conversation))
}
