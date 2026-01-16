//! Database model structs.
//!
//! This module contains the data structures for database records.

use chrono::{DateTime, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

use crate::tools::AccessLevel;

/// Processing status for documents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingStatus {
    /// Document is being processed (text extraction, embeddings, etc.)
    Processing,
    /// Document processing completed successfully
    Completed,
    /// Document processing failed
    Failed,
}

impl ProcessingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcessingStatus::Processing => "processing",
            ProcessingStatus::Completed => "completed",
            ProcessingStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "processing" => ProcessingStatus::Processing,
            "failed" => ProcessingStatus::Failed,
            _ => ProcessingStatus::Completed,
        }
    }
}

/// Captioning status for document images (separate from document processing)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptioningStatus {
    /// No vision model specified, captioning not requested
    #[default]
    NotRequested,
    /// Queued for captioning
    Pending,
    /// Currently captioning images
    InProgress,
    /// All images have been captioned
    Completed,
    /// Captioning failed
    Failed,
}

impl CaptioningStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CaptioningStatus::NotRequested => "not_requested",
            CaptioningStatus::Pending => "pending",
            CaptioningStatus::InProgress => "in_progress",
            CaptioningStatus::Completed => "completed",
            CaptioningStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pending" => CaptioningStatus::Pending,
            "in_progress" => CaptioningStatus::InProgress,
            "completed" => CaptioningStatus::Completed,
            "failed" => CaptioningStatus::Failed,
            _ => CaptioningStatus::NotRequested,
        }
    }
}

/// Document record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub file_path: Option<String>,
    pub file_hash: Option<String>,
    pub access_level: AccessLevel,
    pub tags: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub processing_status: ProcessingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_error: Option<String>,
    pub chunk_count: usize,
    pub image_count: usize,
    /// Current processing phase (e.g., "chunking", "embedding", "extracting_images")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_phase: Option<String>,
    /// Current progress within the phase
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_progress: Option<usize>,
    /// Total items in the current phase
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processing_total: Option<usize>,
    /// Status of image captioning (separate from document processing)
    #[serde(default)]
    pub captioning_status: CaptioningStatus,
    /// Error message if captioning failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captioning_error: Option<String>,
    /// Current captioning progress (images captioned so far)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captioning_progress: Option<usize>,
    /// Total images to caption
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captioning_total: Option<usize>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Document {
    pub(crate) fn from_row(row: &Row<'_>, tags: Vec<String>) -> Result<Self, rusqlite::Error> {
        let access_level_u8: u8 = row.get(4)?;
        let metadata_str: Option<String> = row.get(5)?;
        let created_at_str: String = row.get(6)?;
        let updated_at_str: String = row.get(7)?;
        let processing_status_str: String = row.get(8)?;
        let processing_error: Option<String> = row.get(9)?;
        let chunk_count: i64 = row.get(10)?;
        let image_count: i64 = row.get(11)?;
        let processing_phase: Option<String> = row.get(12)?;
        let processing_progress: Option<i64> = row.get(13)?;
        let processing_total: Option<i64> = row.get(14)?;
        let captioning_status_str: String = row.get(15)?;
        let captioning_error: Option<String> = row.get(16)?;
        let captioning_progress: Option<i64> = row.get(17)?;
        let captioning_total: Option<i64> = row.get(18)?;

        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            file_path: row.get(2)?,
            file_hash: row.get(3)?,
            access_level: AccessLevel::from_u8(access_level_u8),
            tags,
            metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
            processing_status: ProcessingStatus::from_str(&processing_status_str),
            processing_error,
            chunk_count: chunk_count as usize,
            image_count: image_count as usize,
            processing_phase,
            processing_progress: processing_progress.map(|p| p as usize),
            processing_total: processing_total.map(|t| t as usize),
            captioning_status: CaptioningStatus::from_str(&captioning_status_str),
            captioning_error,
            captioning_progress: captioning_progress.map(|p| p as usize),
            captioning_total: captioning_total.map(|t| t as usize),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

/// Chunk record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub content: String,
    pub chunk_index: i32,
    pub page_number: Option<i32>,
    pub section_title: Option<String>,
    pub access_level: AccessLevel,
    pub tags: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl Chunk {
    pub(crate) fn from_row(row: &Row<'_>, tags: Vec<String>) -> Result<Self, rusqlite::Error> {
        let access_level_u8: u8 = row.get(6)?;
        let metadata_str: Option<String> = row.get(7)?;
        let created_at_str: String = row.get(8)?;

        Ok(Self {
            id: row.get(0)?,
            document_id: row.get(1)?,
            content: row.get(2)?,
            chunk_index: row.get(3)?,
            page_number: row.get(4)?,
            section_title: row.get(5)?,
            access_level: AccessLevel::from_u8(access_level_u8),
            tags,
            metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

/// Conversation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ConversationMessage>,
    pub metadata: Option<ConversationMetadata>,
}

impl Conversation {
    pub(crate) fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        let messages_str: String = row.get(4)?;
        let metadata_str: Option<String> = row.get(5)?;
        let created_at_str: String = row.get(2)?;
        let updated_at_str: String = row.get(3)?;

        Ok(Self {
            id: row.get(0)?,
            user_id: row.get(1)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            messages: serde_json::from_str(&messages_str).unwrap_or_default(),
            metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
        })
    }
}

/// Message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_results: Option<Vec<ToolResultRecord>>,
}

/// Message role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Tool call record for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub tool: String,
    pub args: serde_json::Value,
}

/// Tool result record for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub tool_call_id: String,
    pub result: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Conversation metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationMetadata {
    #[serde(default)]
    pub active_document_ids: Vec<String>,
    #[serde(default)]
    pub active_actor_ids: Vec<String>,
    #[serde(default)]
    pub total_tokens_estimate: u32,
}

/// Image type classification
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageType {
    /// Standard extracted image
    #[default]
    Individual,
    /// Background image (appears on multiple pages, extracted once)
    Background,
    /// Rendered page region for overlapping content
    RegionRender,
}

impl ImageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ImageType::Individual => "individual",
            ImageType::Background => "background",
            ImageType::RegionRender => "region_render",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "background" => ImageType::Background,
            "region_render" => ImageType::RegionRender,
            _ => ImageType::Individual,
        }
    }
}

/// Document image record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentImage {
    pub id: String,
    pub document_id: String,
    pub page_number: i32,
    pub image_index: i32,
    pub internal_path: String,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub description: Option<String>,
    /// Pages this image spans (for cross-page composites). JSON array stored as TEXT.
    pub source_pages: Option<Vec<i32>>,
    /// Type of image (individual, background, or region render)
    #[serde(default)]
    pub image_type: ImageType,
    /// ID of the source individual image if this is a region render
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_image_id: Option<String>,
    /// Whether this image has an associated region render
    #[serde(default)]
    pub has_region_render: bool,
    pub created_at: DateTime<Utc>,
}

impl DocumentImage {
    pub(crate) fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        let created_at_str: String = row.get(9)?;
        let source_pages_json: Option<String> = row.get(10)?;
        let source_pages = source_pages_json.and_then(|s| serde_json::from_str(&s).ok());
        let image_type_str: String = row.get(11)?;
        let source_image_id: Option<String> = row.get(12)?;
        let has_region_render: bool = row.get(13)?;

        Ok(Self {
            id: row.get(0)?,
            document_id: row.get(1)?,
            page_number: row.get(2)?,
            image_index: row.get(3)?,
            internal_path: row.get(4)?,
            mime_type: row.get(5)?,
            width: row.get::<_, Option<i32>>(6)?.map(|v| v as u32),
            height: row.get::<_, Option<i32>>(7)?.map(|v| v as u32),
            description: row.get(8)?,
            source_pages,
            image_type: ImageType::from_str(&image_type_str),
            source_image_id,
            has_region_render,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}

/// Document image with parent document info (for access control)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentImageWithAccess {
    #[serde(flatten)]
    pub image: DocumentImage,
    pub document_title: String,
    pub access_level: AccessLevel,
}

/// Cached vision model description for an arbitrary FVTT image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FvttImageDescription {
    pub id: String,
    pub image_path: String,
    pub source: String,
    pub description: String,
    pub embedding: Option<Vec<f32>>,
    pub vision_model: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FvttImageDescription {
    pub(crate) fn from_row(row: &Row<'_>) -> Result<Self, rusqlite::Error> {
        let created_at_str: String = row.get(8)?;
        let updated_at_str: String = row.get(9)?;
        let embedding_blob: Option<Vec<u8>> = row.get(4)?;
        let embedding = embedding_blob.map(|blob| {
            blob.chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect()
        });

        Ok(Self {
            id: row.get(0)?,
            image_path: row.get(1)?,
            source: row.get(2)?,
            description: row.get(3)?,
            embedding,
            vision_model: row.get(5)?,
            width: row.get::<_, Option<i32>>(6)?.map(|v| v as u32),
            height: row.get::<_, Option<i32>>(7)?.map(|v| v as u32),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        })
    }
}
