use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

use crate::i18n::I18n;

/// Main service error type
#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Document not found: {document_id}")]
    DocumentNotFound { document_id: String },

    #[error("Image not found: {image_id}")]
    ImageNotFound { image_id: String },

    #[error("Conversation not found: {conversation_id}")]
    ConversationNotFound { conversation_id: String },

    #[error("Tool call not found: {tool_call_id}")]
    ToolCallNotFound { tool_call_id: String },

    #[error("{0}")]
    Ollama(#[from] OllamaError),

    #[error("Database error")]
    Database(#[from] DatabaseError),

    #[error("Document processing failed")]
    Processing(#[from] ProcessingError),

    #[error("Embedding error")]
    Embedding(#[from] EmbeddingError),

    #[error("Invalid request: {message}")]
    InvalidRequest { message: String },

    #[error("Configuration error: {message}")]
    Config { message: String },

    #[error("Internal error: {message}")]
    Internal { message: String },
}

/// Ollama client errors
#[derive(Error, Debug)]
pub enum OllamaError {
    #[error("Connection failed to Ollama at {url}")]
    Connection {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("Model not found: {model}")]
    ModelNotFound { model: String },

    #[error("Generation failed (status {status}): {message}")]
    Generation { status: u16, message: String },

    #[error("Invalid response from Ollama")]
    InvalidResponse {
        #[source]
        source: serde_json::Error,
    },
}

/// Database errors
#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Database connection failed")]
    Connection(#[source] rusqlite::Error),

    #[error("Query failed")]
    Query(#[source] rusqlite::Error),

    #[error("Migration failed: {message}")]
    Migration { message: String },

    #[error("Serialization failed")]
    Serialization(#[source] serde_json::Error),
}

/// Document processing errors
#[derive(Error, Debug)]
pub enum ProcessingError {
    #[error("Failed to extract text from page {page}")]
    TextExtraction {
        page: u32,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Failed to read EPUB")]
    EpubRead(String),

    #[error("Unsupported file format: {format}")]
    UnsupportedFormat { format: String },

    #[error("File too large: {size} bytes (max {max} bytes)")]
    FileTooLarge { size: u64, max: u64 },

    #[error("IO error")]
    Io(#[source] std::io::Error),
}

/// Embedding errors
#[derive(Error, Debug)]
pub enum EmbeddingError {
    #[error("Model initialization failed: {message}")]
    ModelInit { message: String },

    #[error("Embedding generation failed: {message}")]
    Generation { message: String },
}

/// API error response (matches Axum's built-in JsonRejection format)
#[derive(Serialize)]
pub struct ErrorResponse {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
}

impl ServiceError {
    fn status_code(&self) -> StatusCode {
        match self {
            ServiceError::DocumentNotFound { .. }
            | ServiceError::ImageNotFound { .. }
            | ServiceError::ConversationNotFound { .. }
            | ServiceError::ToolCallNotFound { .. } => StatusCode::NOT_FOUND,
            ServiceError::InvalidRequest { .. } => StatusCode::BAD_REQUEST,
            ServiceError::Ollama(OllamaError::ModelNotFound { .. }) => StatusCode::NOT_FOUND,
            ServiceError::Processing(ProcessingError::UnsupportedFormat { .. }) => {
                StatusCode::UNSUPPORTED_MEDIA_TYPE
            }
            ServiceError::Processing(ProcessingError::FileTooLarge { .. }) => {
                StatusCode::PAYLOAD_TOO_LARGE
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_code(&self) -> &'static str {
        match self {
            ServiceError::DocumentNotFound { .. } => "document_not_found",
            ServiceError::ImageNotFound { .. } => "image_not_found",
            ServiceError::ConversationNotFound { .. } => "conversation_not_found",
            ServiceError::ToolCallNotFound { .. } => "tool_call_not_found",
            ServiceError::Ollama(OllamaError::Connection { .. }) => "ollama_connection",
            ServiceError::Ollama(OllamaError::ModelNotFound { .. }) => "ollama_model_not_found",
            ServiceError::Ollama(OllamaError::Generation { .. }) => "ollama_generation",
            ServiceError::Ollama(OllamaError::InvalidResponse { .. }) => "ollama_invalid_response",
            ServiceError::Database(_) => "database_error",
            ServiceError::Processing(ProcessingError::TextExtraction { .. }) => {
                "text_extraction_error"
            }
            ServiceError::Processing(ProcessingError::EpubRead(_)) => "epub_read_error",
            ServiceError::Processing(ProcessingError::UnsupportedFormat { .. }) => {
                "unsupported_format"
            }
            ServiceError::Processing(ProcessingError::FileTooLarge { .. }) => "file_too_large",
            ServiceError::Processing(ProcessingError::Io(_)) => "io_error",
            ServiceError::Embedding(_) => "embedding_error",
            ServiceError::InvalidRequest { .. } => "invalid_request",
            ServiceError::Config { .. } => "config_error",
            ServiceError::Internal { .. } => "internal_error",
        }
    }

    /// Get a user-friendly translated message
    pub fn user_message(&self, i18n: &I18n, locale: &str) -> String {
        match self {
            ServiceError::DocumentNotFound { document_id } => {
                i18n.format(locale, "error-document-not-found", &[("id", document_id)])
            }
            ServiceError::ConversationNotFound { conversation_id } => i18n.format(
                locale,
                "error-conversation-not-found",
                &[("id", conversation_id)],
            ),
            ServiceError::Internal { .. } => i18n.get(locale, "error-internal", None),
            // For other errors, fall back to the technical message
            _ => self.to_string(),
        }
    }

    /// Convert to an error response with i18n support
    pub fn into_response_with_i18n(self, i18n: &I18n, locale: &str) -> Response {
        let status = self.status_code();
        let code = self.error_code().to_string();
        let message = self.user_message(i18n, locale);

        let response = ErrorResponse {
            message,
            code: Some(code),
            details: None,
            retry_after_secs: None,
        };

        (status, Json(response)).into_response()
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let code = self.error_code().to_string();

        let response = ErrorResponse {
            message: self.to_string(),
            code: Some(code),
            details: None,
            retry_after_secs: None,
        };

        (status, Json(response)).into_response()
    }
}

/// Result type alias for service operations
pub type ServiceResult<T> = Result<T, ServiceError>;

/// Error wrapper with i18n support for API responses
pub struct I18nError {
    pub error: ServiceError,
    pub i18n: std::sync::Arc<I18n>,
    pub locale: String,
}

impl I18nError {
    pub fn new(error: ServiceError, i18n: std::sync::Arc<I18n>, locale: impl Into<String>) -> Self {
        Self {
            error,
            i18n,
            locale: locale.into(),
        }
    }
}

impl IntoResponse for I18nError {
    fn into_response(self) -> Response {
        self.error.into_response_with_i18n(&self.i18n, &self.locale)
    }
}

impl<E: Into<ServiceError>> From<E> for I18nError {
    fn from(error: E) -> Self {
        // This fallback doesn't have i18n, so uses default
        // Real usage should use I18nError::new()
        Self {
            error: error.into(),
            i18n: std::sync::Arc::new(I18n::new()),
            locale: "en".to_string(),
        }
    }
}
