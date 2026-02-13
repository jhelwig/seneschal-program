//! HTTP API for the Seneschal service.
//!
//! This module provides the REST API endpoints for:
//! - Health and metrics monitoring
//! - Document management
//! - Image management
//! - Search functionality
//! - WebSocket connections

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, WebSocketUpgrade},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::RuntimeConfig;
use crate::error::{I18nError, ServiceError};
use crate::service::SeneschalService;
use crate::websocket::{WebSocketManager, handle_ws_connection};

pub mod documents;
pub mod images;
pub mod search;
pub mod settings;
use documents::{
    delete_document_handler, delete_document_images_handler, get_document_handler,
    list_documents_handler, reextract_document_images_handler, update_document_handler,
    upload_document_handler,
};
use images::{
    delete_image_handler, deliver_image_handler, get_document_images_handler,
    get_image_data_handler, get_image_handler, list_images_handler, search_images_handler,
};
use search::search_handler;
use settings::{get_settings_handler, update_settings_handler};

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
pub fn router(service: Arc<SeneschalService>, runtime_config: &RuntimeConfig) -> Router {
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
    let max_body_size = runtime_config.dynamic().limits.max_document_size_bytes as usize;

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
        // Settings endpoints
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler));

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
