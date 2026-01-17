//! Dynamic configuration that can be updated at runtime via API.
//! DB values override config file/env defaults.

mod defaults;
mod keys;
mod merging;
mod schemas;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub use schemas::{
    AgenticLoopConfig, ConversationConfig, EmbeddingsConfig, ImageExtractionConfig, LimitsConfig,
    McpConfig, OllamaConfig, TravellerMapConfig, TravellerWorldsConfig,
};

use defaults::{
    default_agentic_loop, default_conversation, default_embeddings, default_image_extraction,
    default_limits, default_mcp, default_ollama, default_traveller_map, default_traveller_worlds,
};

/// Dynamic configuration that can be updated at runtime via API
/// DB values override config file/env defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicConfig {
    #[serde(default = "default_ollama")]
    pub ollama: OllamaConfig,

    #[serde(default = "default_embeddings")]
    pub embeddings: EmbeddingsConfig,

    #[serde(default = "default_mcp")]
    pub mcp: McpConfig,

    #[serde(default = "default_limits")]
    pub limits: LimitsConfig,

    #[serde(default = "default_agentic_loop")]
    pub agentic_loop: AgenticLoopConfig,

    #[serde(default = "default_conversation")]
    pub conversation: ConversationConfig,

    #[serde(default = "default_image_extraction")]
    pub image_extraction: ImageExtractionConfig,

    #[serde(default = "default_traveller_map")]
    pub traveller_map: TravellerMapConfig,

    #[serde(default = "default_traveller_worlds")]
    pub traveller_worlds: TravellerWorldsConfig,
}

impl DynamicConfig {
    /// Get all valid setting keys
    pub fn valid_keys() -> HashSet<&'static str> {
        keys::valid_keys()
    }
}
