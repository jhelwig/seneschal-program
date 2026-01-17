//! Configuration struct definitions for DynamicConfig sections.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::defaults::{
    default_background_area_threshold, default_background_min_pages, default_text_overlap_min_dpi,
    default_traveller_map_timeout, default_traveller_map_url, default_traveller_worlds_url,
};

/// Ollama LLM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "super::defaults::default_ollama_url")]
    pub base_url: String,

    #[serde(default = "super::defaults::default_model")]
    pub default_model: String,

    /// Vision model for image captioning (e.g., llava, moondream). Empty means no captioning.
    #[serde(default)]
    pub vision_model: String,

    #[serde(default = "super::defaults::default_temperature")]
    pub temperature: f32,

    #[serde(default = "super::defaults::default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

/// Embeddings configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    #[serde(default = "super::defaults::default_embedding_model")]
    pub model: String,

    #[serde(default = "super::defaults::default_chunk_size")]
    pub chunk_size: usize,

    #[serde(default = "super::defaults::default_chunk_overlap")]
    pub chunk_overlap: usize,
}

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "super::defaults::default_mcp_path")]
    pub path: String,

    #[serde(default = "super::defaults::default_mcp_enabled")]
    pub enabled: bool,
}

/// Size limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    #[serde(default = "super::defaults::default_max_document_size")]
    pub max_document_size_bytes: u64,
}

/// Agentic loop configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticLoopConfig {
    /// Tool calls before pause prompt (internal + external combined)
    #[serde(default = "super::defaults::default_tool_call_pause_threshold")]
    pub tool_call_pause_threshold: u32,

    /// Time before pause prompt in seconds
    #[serde(default = "super::defaults::default_time_pause_threshold_secs")]
    pub time_pause_threshold_secs: u64,

    /// Hard timeout in seconds (cannot continue past this)
    #[serde(default = "super::defaults::default_hard_timeout_secs")]
    pub hard_timeout_secs: u64,

    /// Timeout waiting for external tool result from client in seconds
    #[serde(default = "super::defaults::default_external_tool_timeout_secs")]
    pub external_tool_timeout_secs: u64,
}

impl AgenticLoopConfig {
    pub fn time_pause_threshold(&self) -> Duration {
        Duration::from_secs(self.time_pause_threshold_secs)
    }

    pub fn hard_timeout(&self) -> Duration {
        Duration::from_secs(self.hard_timeout_secs)
    }

    pub fn external_tool_timeout(&self) -> Duration {
        Duration::from_secs(self.external_tool_timeout_secs)
    }
}

/// Conversation storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationConfig {
    /// How long to keep conversations before cleanup in seconds
    #[serde(default = "super::defaults::default_conversation_ttl_secs")]
    pub ttl_secs: u64,

    /// Run cleanup every N seconds
    #[serde(default = "super::defaults::default_cleanup_interval_secs")]
    pub cleanup_interval_secs: u64,

    /// Maximum conversations per user (0 = unlimited)
    #[serde(default = "super::defaults::default_max_per_user")]
    pub max_per_user: u32,
}

impl ConversationConfig {
    pub fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl_secs)
    }

    pub fn cleanup_interval(&self) -> Duration {
        Duration::from_secs(self.cleanup_interval_secs)
    }
}

/// Image extraction configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageExtractionConfig {
    /// Threshold for background image detection (0.0-1.0).
    /// Images covering at least this fraction of a page's area are candidates for background detection.
    #[serde(default = "default_background_area_threshold")]
    pub background_area_threshold: f64,

    /// Minimum number of pages an image must appear on to be considered a background.
    #[serde(default = "default_background_min_pages")]
    pub background_min_pages: usize,

    /// Minimum DPI for region renders that include text or vector overlaps.
    #[serde(default = "default_text_overlap_min_dpi")]
    pub text_overlap_min_dpi: f64,
}

impl Default for ImageExtractionConfig {
    fn default() -> Self {
        Self {
            background_area_threshold: default_background_area_threshold(),
            background_min_pages: default_background_min_pages(),
            text_overlap_min_dpi: default_text_overlap_min_dpi(),
        }
    }
}

/// Traveller Map API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TravellerMapConfig {
    /// Base URL for the Traveller Map API
    #[serde(default = "default_traveller_map_url")]
    pub base_url: String,

    /// Request timeout in seconds
    #[serde(default = "default_traveller_map_timeout")]
    pub timeout_secs: u64,
}

impl Default for TravellerMapConfig {
    fn default() -> Self {
        Self {
            base_url: default_traveller_map_url(),
            timeout_secs: default_traveller_map_timeout(),
        }
    }
}

/// Traveller Worlds configuration (travellerworlds.com map generation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TravellerWorldsConfig {
    /// Base URL for Traveller Worlds
    #[serde(default = "default_traveller_worlds_url")]
    pub base_url: String,

    /// Optional path to Chrome/Chromium executable (uses system default if not set)
    #[serde(default)]
    pub chrome_path: Option<String>,
}

impl Default for TravellerWorldsConfig {
    fn default() -> Self {
        Self {
            base_url: default_traveller_worlds_url(),
            chrome_path: None,
        }
    }
}
