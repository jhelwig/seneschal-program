use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State, WebSocketUpgrade},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::{AppConfig, AssetsAccess};
use crate::db::{Document, DocumentImageWithAccess};
use crate::error::{I18nError, ServiceError};
use crate::ingestion::IngestionService;
use crate::service::SeneschalService;
use crate::tools::{AccessLevel, SearchFilters, TagMatch};
use crate::websocket::{WebSocketManager, handle_ws_connection};

/// Application state
pub struct AppState {
    pub service: Arc<SeneschalService>,
    pub start_time: Instant,
    pub ws_manager: Arc<WebSocketManager>,
}

impl AppState {
    /// Create an i18n-aware error from a service error
    pub fn i18n_error(&self, error: ServiceError) -> I18nError {
        I18nError::new(error, self.service.i18n.clone(), "en")
    }
}

/// Build the API router
pub fn router(service: Arc<SeneschalService>, config: &AppConfig) -> Router {
    let ws_manager = service.ws_manager.clone();

    let state = Arc::new(AppState {
        service,
        start_time: Instant::now(),
        ws_manager,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Use the configured max document size for uploads
    let max_body_size = config.limits.max_document_size_bytes as usize;

    let api_routes = Router::new()
        // Model endpoints
        .route("/models", get(models_handler))
        // Document endpoints - with larger body limit for file uploads
        .route("/documents", get(list_documents_handler))
        .route(
            "/documents",
            post(upload_document_handler).layer(DefaultBodyLimit::max(max_body_size)),
        )
        .route("/documents/{id}", get(get_document_handler))
        .route("/documents/{id}", put(update_document_handler))
        .route("/documents/{id}", delete(delete_document_handler))
        .route("/documents/{id}/images", get(get_document_images_handler))
        .route(
            "/documents/{id}/images",
            delete(delete_document_images_handler),
        )
        .route(
            "/documents/{id}/images/extract",
            post(reextract_document_images_handler),
        )
        // Search endpoint
        .route("/search", post(search_handler))
        // Image endpoints
        .route("/images", get(list_images_handler))
        .route("/images/search", post(search_images_handler))
        .route("/images/{id}", get(get_image_handler))
        .route("/images/{id}", delete(delete_image_handler))
        .route("/images/{id}/data", get(get_image_data_handler))
        .route("/images/{id}/deliver", post(deliver_image_handler))
        // Conversation endpoints
        .route("/conversations", get(list_conversations_handler))
        .route("/conversations/{id}", get(get_conversation_handler));

    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/ws", get(ws_handler))
        .nest("/api", api_routes)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// === Health & Metrics ===

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let ollama_healthy = state.service.ollama.health_check().await.unwrap_or(false);

    let status = if ollama_healthy {
        state.service.i18n.get("en", "health-status-healthy", None)
    } else {
        state.service.i18n.format(
            "en",
            "health-status-degraded",
            &[("reason", "Ollama unavailable")],
        )
    };

    Json(HealthResponse {
        status,
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.start_time.elapsed().as_secs(),
        ollama_available: ollama_healthy,
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_seconds: u64,
    ollama_available: bool,
}

async fn metrics_handler() -> impl IntoResponse {
    // Return Prometheus-formatted metrics
    // In a full implementation, use metrics-exporter-prometheus
    let metrics = r#"
# HELP seneschal_requests_total Total number of requests
# TYPE seneschal_requests_total counter
seneschal_requests_total{endpoint="chat"} 0

# HELP seneschal_active_conversations Number of active conversations
# TYPE seneschal_active_conversations gauge
seneschal_active_conversations 0

# HELP seneschal_documents_total Total number of indexed documents
# TYPE seneschal_documents_total gauge
seneschal_documents_total 0
"#;

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        metrics,
    )
}

// === WebSocket ===

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    info!("WebSocket upgrade request received");
    ws.on_upgrade(move |socket| {
        handle_ws_connection(socket, state.ws_manager.clone(), state.service.clone())
    })
}

// === Models ===

async fn models_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::ollama::ModelInfo>>, I18nError> {
    let models = state
        .service
        .ollama
        .list_models()
        .await
        .map_err(|e| state.i18n_error(e))?;
    Ok(Json(models))
}

// === Documents ===

async fn list_documents_handler(
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

#[derive(Deserialize)]
struct ListDocumentsParams {
    user_role: Option<u8>,
}

async fn upload_document_handler(
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

async fn get_document_handler(
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

async fn delete_document_handler(
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

#[derive(Serialize)]
struct DeleteResponse {
    success: bool,
    message: String,
}

/// Update document details (title, access_level, tags)
async fn update_document_handler(
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

#[derive(Deserialize)]
struct UpdateDocumentRequest {
    title: String,
    access_level: String,
    tags: Option<String>,
}

/// Delete all images for a document
async fn delete_document_images_handler(
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

#[derive(Serialize)]
struct DeleteImagesResponse {
    success: bool,
    deleted_count: usize,
    message: String,
}

/// Re-extract images from a document (queues for async processing)
async fn reextract_document_images_handler(
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

#[derive(Deserialize)]
struct ReextractImagesRequest {
    vision_model: Option<String>,
}

#[derive(Serialize)]
struct ReextractImagesResponse {
    success: bool,
    message: String,
}

// === Search ===

async fn search_handler(
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

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    user_role: u8,
    limit: Option<usize>,
    tags: Option<Vec<String>>,
    tags_match: Option<String>,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SearchResultDto>,
}

#[derive(Serialize)]
struct SearchResultDto {
    chunk_id: String,
    document_id: String,
    content: String,
    section_title: Option<String>,
    page_number: Option<i32>,
    similarity: f32,
}

// === Conversations ===

async fn list_conversations_handler(
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

#[derive(Deserialize)]
struct ListConversationsParams {
    user_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct ConversationSummary {
    id: String,
    created_at: String,
    updated_at: String,
    message_count: usize,
}

async fn get_conversation_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<crate::db::Conversation>, I18nError> {
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

// === Images ===

async fn list_images_handler(
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

#[derive(Deserialize)]
struct ListImagesParams {
    user_role: Option<u8>,
    document_id: Option<String>,
    page_number: Option<i32>,
    start_page: Option<i32>,
    end_page: Option<i32>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct ListImagesResponse {
    images: Vec<ImageDto>,
}

#[derive(Serialize)]
struct ImageDto {
    id: String,
    document_id: String,
    document_title: String,
    page_number: i32,
    image_index: i32,
    mime_type: String,
    width: Option<u32>,
    height: Option<u32>,
    description: Option<String>,
    created_at: String,
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
struct SimpleImageDto {
    id: String,
    page_number: i32,
    image_index: i32,
    mime_type: String,
    width: Option<u32>,
    height: Option<u32>,
    description: Option<String>,
    created_at: String,
}

impl From<crate::db::DocumentImage> for SimpleImageDto {
    fn from(img: crate::db::DocumentImage) -> Self {
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

/// Get all images for a specific document
async fn get_document_images_handler(
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

#[derive(Serialize)]
struct DocumentImagesResponse {
    document_id: String,
    images: Vec<SimpleImageDto>,
}

/// Search images by semantic similarity
async fn search_images_handler(
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

#[derive(Deserialize)]
struct SearchImagesRequest {
    query: String,
    user_role: Option<u8>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct SearchImagesResponse {
    images: Vec<SearchImageResult>,
}

#[derive(Serialize)]
struct SearchImageResult {
    #[serde(flatten)]
    image: ImageDto,
    similarity: f32,
}

async fn get_image_handler(
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

async fn delete_image_handler(
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

async fn get_image_data_handler(
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
    let data = std::fs::read(&image.image.internal_path).map_err(|e| {
        state.i18n_error(ServiceError::Processing(crate::error::ProcessingError::Io(
            e,
        )))
    })?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, image.image.mime_type)],
        data,
    )
        .into_response())
}

async fn deliver_image_handler(
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
    match state.service.config.fvtt.check_assets_access() {
        AssetsAccess::Direct(assets_dir) => {
            // Create target directory
            let full_path = assets_dir.join(&relative_path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    state.i18n_error(ServiceError::Processing(crate::error::ProcessingError::Io(
                        e,
                    )))
                })?;
            }

            // Copy file
            std::fs::copy(&image.image.internal_path, &full_path).map_err(|e| {
                state.i18n_error(ServiceError::Processing(crate::error::ProcessingError::Io(
                    e,
                )))
            })?;

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

#[derive(Deserialize)]
struct DeliverImageRequest {
    target_path: Option<String>,
}

#[derive(Serialize)]
struct DeliverImageResponse {
    mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fvtt_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_path: Option<String>,
}
