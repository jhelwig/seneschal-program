//! Default value functions for DynamicConfig.

use super::schemas::{
    AgenticLoopConfig, EmbeddingsConfig, ImageExtractionConfig, LimitsConfig, McpConfig,
    OllamaConfig, TravellerMapConfig, TravellerWorldsConfig,
};

// ==================== Top-level Section Defaults ====================

pub(crate) fn default_ollama() -> OllamaConfig {
    OllamaConfig {
        base_url: default_ollama_url(),
        default_model: default_model(),
        vision_model: String::new(), // Empty means no image captioning
        temperature: default_temperature(),
        request_timeout_secs: default_request_timeout_secs(),
    }
}

pub(crate) fn default_embeddings() -> EmbeddingsConfig {
    EmbeddingsConfig {
        model: default_embedding_model(),
        chunk_size: default_chunk_size(),
        chunk_overlap: default_chunk_overlap(),
    }
}

pub(crate) fn default_mcp() -> McpConfig {
    McpConfig {
        path: default_mcp_path(),
        enabled: default_mcp_enabled(),
    }
}

pub(crate) fn default_limits() -> LimitsConfig {
    LimitsConfig {
        max_document_size_bytes: default_max_document_size(),
    }
}

pub(crate) fn default_agentic_loop() -> AgenticLoopConfig {
    AgenticLoopConfig {
        tool_call_pause_threshold: default_tool_call_pause_threshold(),
        time_pause_threshold_secs: default_time_pause_threshold_secs(),
        hard_timeout_secs: default_hard_timeout_secs(),
        external_tool_timeout_secs: default_external_tool_timeout_secs(),
    }
}

pub(crate) fn default_image_extraction() -> ImageExtractionConfig {
    ImageExtractionConfig {
        background_area_threshold: default_background_area_threshold(),
        background_min_pages: default_background_min_pages(),
        text_overlap_min_dpi: default_text_overlap_min_dpi(),
    }
}

pub(crate) fn default_traveller_map() -> TravellerMapConfig {
    TravellerMapConfig::default()
}

pub(crate) fn default_traveller_worlds() -> TravellerWorldsConfig {
    TravellerWorldsConfig::default()
}

// ==================== Ollama Defaults ====================

pub(crate) fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

pub(crate) fn default_model() -> String {
    "llama3.2".to_string()
}

pub(crate) fn default_temperature() -> f32 {
    0.7
}

pub(crate) fn default_request_timeout_secs() -> u64 {
    120
}

// ==================== Embeddings Defaults ====================

pub(crate) fn default_embedding_model() -> String {
    "nomic-embed-text".to_string()
}

pub(crate) fn default_chunk_size() -> usize {
    512
}

pub(crate) fn default_chunk_overlap() -> usize {
    64
}

// ==================== MCP Defaults ====================

pub(crate) fn default_mcp_path() -> String {
    "/mcp".to_string()
}

pub(crate) fn default_mcp_enabled() -> bool {
    true
}

// ==================== Limits Defaults ====================

pub(crate) fn default_max_document_size() -> u64 {
    104_857_600 // 100MB
}

// ==================== Agentic Loop Defaults ====================

pub(crate) fn default_tool_call_pause_threshold() -> u32 {
    u32::MAX // Effectively disabled
}

pub(crate) fn default_time_pause_threshold_secs() -> u64 {
    u64::MAX // Effectively disabled
}

pub(crate) fn default_hard_timeout_secs() -> u64 {
    300
}

pub(crate) fn default_external_tool_timeout_secs() -> u64 {
    30
}

// ==================== Image Extraction Defaults ====================

pub(crate) fn default_background_area_threshold() -> f64 {
    0.9
}

pub(crate) fn default_background_min_pages() -> usize {
    2
}

pub(crate) fn default_text_overlap_min_dpi() -> f64 {
    300.0
}

// ==================== Traveller Map Defaults ====================

pub(crate) fn default_traveller_map_url() -> String {
    "https://travellermap.com".to_string()
}

pub(crate) fn default_traveller_map_timeout() -> u64 {
    30
}

// ==================== Traveller Worlds Defaults ====================

pub(crate) fn default_traveller_worlds_url() -> String {
    "http://www.travellerworlds.com".to_string()
}
