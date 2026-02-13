//! Valid setting keys for DynamicConfig.

use std::collections::HashSet;

/// All valid setting keys for DynamicConfig
pub const VALID_SETTING_KEYS: &[&str] = &[
    "ollama.base_url",
    "ollama.default_model",
    "ollama.vision_model",
    "ollama.temperature",
    "ollama.request_timeout_secs",
    "embeddings.model",
    "embeddings.chunk_size",
    "embeddings.chunk_overlap",
    "mcp.path",
    "mcp.enabled",
    "limits.max_document_size_bytes",
    "agentic_loop.tool_call_pause_threshold",
    "agentic_loop.time_pause_threshold_secs",
    "agentic_loop.hard_timeout_secs",
    "agentic_loop.external_tool_timeout_secs",
    "image_extraction.background_area_threshold",
    "image_extraction.background_min_pages",
    "image_extraction.text_overlap_min_dpi",
    "traveller_map.base_url",
    "traveller_map.timeout_secs",
    "traveller_worlds.base_url",
    "traveller_worlds.chrome_path",
];

/// Get all valid setting keys as a HashSet
pub fn valid_keys() -> HashSet<&'static str> {
    VALID_SETTING_KEYS.iter().copied().collect()
}
