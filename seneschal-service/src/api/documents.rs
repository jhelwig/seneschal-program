//! Document API endpoints.
//!
//! Handlers for document CRUD operations including upload, listing,
//! update, delete, and image management.

use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::db::Document;
use crate::error::{I18nError, ServiceError};
use crate::tools::AccessLevel;

use super::AppState;

/// List documents query parameters
#[derive(Deserialize)]
pub struct ListDocumentsParams {
    pub user_role: Option<u8>,
}

/// Response for delete operations
#[derive(Serialize)]
pub struct DeleteResponse {
    pub success: bool,
    pub message: String,
}

/// Request to update document metadata
#[derive(Deserialize)]
pub struct UpdateDocumentRequest {
    pub title: String,
    pub access_level: String,
    pub tags: Option<String>,
}

/// Response for image deletion
#[derive(Serialize)]
pub struct DeleteImagesResponse {
    pub success: bool,
    pub deleted_count: usize,
    pub message: String,
}

/// Request for image re-extraction
#[derive(Deserialize)]
pub struct ReextractImagesRequest {
    pub vision_model: Option<String>,
}

/// Response for image re-extraction
#[derive(Serialize)]
pub struct ReextractImagesResponse {
    pub success: bool,
    pub message: String,
}

/// List all documents accessible by the user
pub async fn list_documents_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListDocumentsParams>,
) -> Result<Json<Vec<Document>>, I18nError> {
    let user_role = params.user_role.unwrap_or(4); // Default to GM access
    let documents = state
        .service
        .list_documents(user_role)
        .map_err(|e| state.i18n_error(e))?;
    Ok(Json(documents))
}

/// Upload a new document
pub async fn upload_document_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Document>, I18nError> {
    let mut file_data: Option<(Vec<u8>, String)> = None;
    let mut title: Option<String> = None;
    let mut access_level = AccessLevel::GmOnly;
    let mut tags: Vec<String> = Vec::new();
    let mut vision_model: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                let filename = field.file_name().unwrap_or("document").to_string();
                let data = field.bytes().await.map_err(|e| {
                    state.i18n_error(ServiceError::InvalidRequest {
                        message: e.to_string(),
                    })
                })?;
                file_data = Some((data.to_vec(), filename));
            }
            "title" => {
                title = Some(field.text().await.map_err(|e| {
                    state.i18n_error(ServiceError::InvalidRequest {
                        message: e.to_string(),
                    })
                })?);
            }
            "access_level" => {
                let level_str = field.text().await.map_err(|e| {
                    state.i18n_error(ServiceError::InvalidRequest {
                        message: e.to_string(),
                    })
                })?;
                access_level = match level_str.as_str() {
                    "player" => AccessLevel::Player,
                    "trusted" => AccessLevel::Trusted,
                    "assistant" => AccessLevel::Assistant,
                    _ => AccessLevel::GmOnly,
                };
            }
            "tags" => {
                let tags_str = field.text().await.map_err(|e| {
                    state.i18n_error(ServiceError::InvalidRequest {
                        message: e.to_string(),
                    })
                })?;
                tags = tags_str.split(',').map(|s| s.trim().to_string()).collect();
            }
            "vision_model" => {
                let model = field.text().await.map_err(|e| {
                    state.i18n_error(ServiceError::InvalidRequest {
                        message: e.to_string(),
                    })
                })?;
                if !model.is_empty() {
                    vision_model = Some(model);
                }
            }
            _ => {}
        }
    }

    let (data, filename) = file_data.ok_or_else(|| {
        state.i18n_error(ServiceError::InvalidRequest {
            message: "No file provided".to_string(),
        })
    })?;

    let title = title.unwrap_or_else(|| filename.clone());

    let document = state
        .service
        .upload_document(&data, &filename, &title, access_level, tags, vision_model)
        .await
        .map_err(|e| state.i18n_error(e))?;

    Ok(Json(document))
}

/// Get a specific document by ID
pub async fn get_document_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Document>, I18nError> {
    let document = state
        .service
        .db
        .get_document(&id)
        .map_err(|e| state.i18n_error(e))?
        .ok_or_else(|| state.i18n_error(ServiceError::DocumentNotFound { document_id: id }))?;

    Ok(Json(document))
}

/// Delete a document
pub async fn delete_document_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DeleteResponse>, I18nError> {
    let deleted = state
        .service
        .delete_document(&id)
        .map_err(|e| state.i18n_error(e))?;

    if deleted {
        Ok(Json(DeleteResponse {
            success: true,
            message: state.service.i18n.get("en", "doc-delete-success", None),
        }))
    } else {
        Err(state.i18n_error(ServiceError::DocumentNotFound { document_id: id }))
    }
}

/// Update document metadata (title, access_level, tags)
pub async fn update_document_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(request): Json<UpdateDocumentRequest>,
) -> Result<Json<Document>, I18nError> {
    // Parse access level string to enum
    let access_level = match request.access_level.as_str() {
        "player" => AccessLevel::Player,
        "trusted" => AccessLevel::Trusted,
        "assistant" => AccessLevel::Assistant,
        "gm_only" => AccessLevel::GmOnly,
        _ => AccessLevel::GmOnly, // Default to GM Only
    };

    // Parse tags from comma-separated string
    let tags: Vec<String> = request
        .tags
        .as_ref()
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let updated = state
        .service
        .update_document(&id, &request.title, access_level, tags)
        .map_err(|e| state.i18n_error(e))?;

    if !updated {
        return Err(state.i18n_error(ServiceError::DocumentNotFound { document_id: id }));
    }

    // Return the updated document
    let document = state
        .service
        .db
        .get_document(&id)
        .map_err(|e| state.i18n_error(e))?
        .ok_or_else(|| state.i18n_error(ServiceError::DocumentNotFound { document_id: id }))?;

    Ok(Json(document))
}

/// Delete all images for a document
pub async fn delete_document_images_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DeleteImagesResponse>, I18nError> {
    let count = state
        .service
        .delete_document_images(&id)
        .map_err(|e| state.i18n_error(e))?;

    Ok(Json(DeleteImagesResponse {
        success: true,
        deleted_count: count,
        message: format!("Deleted {} images", count),
    }))
}

/// Re-extract images from a document (queues for async processing)
pub async fn reextract_document_images_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(request): Json<ReextractImagesRequest>,
) -> Result<Json<ReextractImagesResponse>, I18nError> {
    state
        .service
        .reextract_document_images(&id, request.vision_model)
        .map_err(|e| state.i18n_error(e))?;

    Ok(Json(ReextractImagesResponse {
        success: true,
        message: "Image re-extraction queued".to_string(),
    }))
}
