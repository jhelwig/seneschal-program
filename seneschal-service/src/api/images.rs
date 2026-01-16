//! Image API endpoints.
//!
//! Handlers for image listing, searching, retrieval, deletion, and delivery.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::config::AssetsAccess;
use crate::db::{DocumentImage, DocumentImageWithAccess};
use crate::error::{I18nError, ProcessingError, ServiceError};
use crate::ingestion::IngestionService;

use super::AppState;
use super::documents::DeleteResponse;

/// Image listing query parameters
#[derive(Deserialize)]
pub struct ListImagesParams {
    pub user_role: Option<u8>,
    pub document_id: Option<String>,
    pub page_number: Option<i32>,
    pub start_page: Option<i32>,
    pub end_page: Option<i32>,
    pub limit: Option<usize>,
}

/// Response containing a list of images
#[derive(Serialize)]
pub struct ListImagesResponse {
    pub images: Vec<ImageDto>,
}

/// Image data transfer object (with document info)
#[derive(Serialize)]
pub struct ImageDto {
    pub id: String,
    pub document_id: String,
    pub document_title: String,
    pub page_number: i32,
    pub image_index: i32,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub description: Option<String>,
    pub created_at: String,
}

impl From<DocumentImageWithAccess> for ImageDto {
    fn from(img: DocumentImageWithAccess) -> Self {
        Self {
            id: img.image.id,
            document_id: img.image.document_id,
            document_title: img.document_title,
            page_number: img.image.page_number,
            image_index: img.image.image_index,
            mime_type: img.image.mime_type,
            width: img.image.width,
            height: img.image.height,
            description: img.image.description,
            created_at: img.image.created_at.to_rfc3339(),
        }
    }
}

/// Simple image DTO without access control info (for document-specific queries)
#[derive(Serialize)]
pub struct SimpleImageDto {
    pub id: String,
    pub page_number: i32,
    pub image_index: i32,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub description: Option<String>,
    pub created_at: String,
}

impl From<DocumentImage> for SimpleImageDto {
    fn from(img: DocumentImage) -> Self {
        Self {
            id: img.id,
            page_number: img.page_number,
            image_index: img.image_index,
            mime_type: img.mime_type,
            width: img.width,
            height: img.height,
            description: img.description,
            created_at: img.created_at.to_rfc3339(),
        }
    }
}

/// Response containing images for a specific document
#[derive(Serialize)]
pub struct DocumentImagesResponse {
    pub document_id: String,
    pub images: Vec<SimpleImageDto>,
}

/// Image search request
#[derive(Deserialize)]
pub struct SearchImagesRequest {
    pub query: String,
    pub user_role: Option<u8>,
    pub limit: Option<usize>,
}

/// Image search response
#[derive(Serialize)]
pub struct SearchImagesResponse {
    pub images: Vec<SearchImageResult>,
}

/// Image search result with similarity score
#[derive(Serialize)]
pub struct SearchImageResult {
    #[serde(flatten)]
    pub image: ImageDto,
    pub similarity: f32,
}

/// Image delivery request
#[derive(Deserialize)]
pub struct DeliverImageRequest {
    pub target_path: Option<String>,
}

/// Image delivery response
#[derive(Serialize)]
pub struct DeliverImageResponse {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fvtt_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_path: Option<String>,
}

/// List images with optional filters
pub async fn list_images_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListImagesParams>,
) -> Result<Json<ListImagesResponse>, I18nError> {
    let images = state
        .service
        .db
        .list_document_images(
            params.user_role.unwrap_or(4), // Default to GM
            params.document_id.as_deref(),
            params.start_page.or(params.page_number), // page_number as start for backwards compat
            params.end_page.or(params.page_number),   // page_number as end for backwards compat
            params.limit.unwrap_or(100),
        )
        .map_err(|e| state.i18n_error(e))?;

    Ok(Json(ListImagesResponse {
        images: images.into_iter().map(ImageDto::from).collect(),
    }))
}

/// Get all images for a specific document
pub async fn get_document_images_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DocumentImagesResponse>, I18nError> {
    let images = state
        .service
        .get_document_images(&id)
        .map_err(|e| state.i18n_error(e))?;

    Ok(Json(DocumentImagesResponse {
        document_id: id,
        images: images.into_iter().map(SimpleImageDto::from).collect(),
    }))
}

/// Search images by semantic similarity
pub async fn search_images_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SearchImagesRequest>,
) -> Result<Json<SearchImagesResponse>, I18nError> {
    // Generate embedding for the query
    let embedding = state
        .service
        .search
        .embed_text(&request.query)
        .await
        .map_err(|e| state.i18n_error(e))?;

    // Search images by embedding similarity
    let results = state
        .service
        .db
        .search_images(
            &embedding,
            request.user_role.unwrap_or(4), // Default to GM
            request.limit.unwrap_or(20),
        )
        .map_err(|e| state.i18n_error(e))?;

    Ok(Json(SearchImagesResponse {
        images: results
            .into_iter()
            .map(|(img, score)| SearchImageResult {
                image: ImageDto::from(img),
                similarity: score,
            })
            .collect(),
    }))
}

/// Get a specific image by ID
pub async fn get_image_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ImageDto>, I18nError> {
    let image = state
        .service
        .db
        .get_document_image(&id)
        .map_err(|e| state.i18n_error(e))?
        .ok_or_else(|| {
            state.i18n_error(ServiceError::ImageNotFound {
                image_id: id.clone(),
            })
        })?;

    Ok(Json(ImageDto::from(image)))
}

/// Delete an image
pub async fn delete_image_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DeleteResponse>, I18nError> {
    let deleted = state
        .service
        .delete_image(&id)
        .map_err(|e| state.i18n_error(e))?;

    if deleted {
        Ok(Json(DeleteResponse {
            success: true,
            message: "Image deleted successfully".to_string(),
        }))
    } else {
        Err(state.i18n_error(ServiceError::ImageNotFound { image_id: id }))
    }
}

/// Get raw image data
pub async fn get_image_data_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, I18nError> {
    let image = state
        .service
        .db
        .get_document_image(&id)
        .map_err(|e| state.i18n_error(e))?
        .ok_or_else(|| {
            state.i18n_error(ServiceError::ImageNotFound {
                image_id: id.clone(),
            })
        })?;

    // Read the image file
    let data = std::fs::read(&image.image.internal_path)
        .map_err(|e| state.i18n_error(ServiceError::Processing(ProcessingError::Io(e))))?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, image.image.mime_type)],
        data,
    )
        .into_response())
}

/// Deliver an image to FVTT assets directory
pub async fn deliver_image_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(request): Json<DeliverImageRequest>,
) -> Result<Json<DeliverImageResponse>, I18nError> {
    let image = state
        .service
        .db
        .get_document_image(&id)
        .map_err(|e| state.i18n_error(e))?
        .ok_or_else(|| {
            state.i18n_error(ServiceError::ImageNotFound {
                image_id: id.clone(),
            })
        })?;

    // Determine path relative to FVTT assets directory (for filesystem operations)
    let relative_path = request.target_path.unwrap_or_else(|| {
        IngestionService::fvtt_image_path(
            &image.document_title,
            image.image.page_number,
            image.image.description.as_deref(),
        )
        .to_string_lossy()
        .to_string()
    });

    // The FVTT path is what FVTT uses to reference the file (prepend assets/)
    let fvtt_path = format!("assets/{}", relative_path);

    // Check if we can write directly
    match state
        .service
        .runtime_config
        .static_config
        .fvtt
        .check_assets_access()
    {
        AssetsAccess::Direct(assets_dir) => {
            // Create target directory
            let full_path = assets_dir.join(&relative_path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    state.i18n_error(ServiceError::Processing(ProcessingError::Io(e)))
                })?;
            }

            // Copy file
            std::fs::copy(&image.image.internal_path, &full_path)
                .map_err(|e| state.i18n_error(ServiceError::Processing(ProcessingError::Io(e))))?;

            Ok(Json(DeliverImageResponse {
                mode: "direct".to_string(),
                fvtt_path: Some(fvtt_path),
                image_id: None,
                suggested_path: None,
            }))
        }
        AssetsAccess::Shuttle => Ok(Json(DeliverImageResponse {
            mode: "shuttle".to_string(),
            fvtt_path: None,
            image_id: Some(id),
            suggested_path: Some(fvtt_path),
        })),
    }
}
