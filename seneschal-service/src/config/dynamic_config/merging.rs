//! Key-value conversion and DB merging logic for DynamicConfig.

use std::collections::HashMap;

use super::DynamicConfig;

impl DynamicConfig {
    /// Convert config to key-value map for API response
    pub fn to_key_value_map(&self) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();

        // Ollama settings
        map.insert(
            "ollama.base_url".to_string(),
            serde_json::Value::String(self.ollama.base_url.clone()),
        );
        map.insert(
            "ollama.default_model".to_string(),
            serde_json::Value::String(self.ollama.default_model.clone()),
        );
        map.insert(
            "ollama.vision_model".to_string(),
            serde_json::Value::String(self.ollama.vision_model.clone()),
        );
        map.insert(
            "ollama.temperature".to_string(),
            serde_json::json!(self.ollama.temperature),
        );
        map.insert(
            "ollama.request_timeout_secs".to_string(),
            serde_json::json!(self.ollama.request_timeout_secs),
        );

        // Embeddings settings
        map.insert(
            "embeddings.model".to_string(),
            serde_json::Value::String(self.embeddings.model.clone()),
        );
        map.insert(
            "embeddings.chunk_size".to_string(),
            serde_json::json!(self.embeddings.chunk_size),
        );
        map.insert(
            "embeddings.chunk_overlap".to_string(),
            serde_json::json!(self.embeddings.chunk_overlap),
        );

        // MCP settings
        map.insert(
            "mcp.path".to_string(),
            serde_json::Value::String(self.mcp.path.clone()),
        );
        map.insert(
            "mcp.enabled".to_string(),
            serde_json::json!(self.mcp.enabled),
        );

        // Limits settings
        map.insert(
            "limits.max_document_size_bytes".to_string(),
            serde_json::json!(self.limits.max_document_size_bytes),
        );

        // Agentic loop settings
        map.insert(
            "agentic_loop.tool_call_pause_threshold".to_string(),
            serde_json::json!(self.agentic_loop.tool_call_pause_threshold),
        );
        map.insert(
            "agentic_loop.time_pause_threshold_secs".to_string(),
            serde_json::json!(self.agentic_loop.time_pause_threshold_secs),
        );
        map.insert(
            "agentic_loop.hard_timeout_secs".to_string(),
            serde_json::json!(self.agentic_loop.hard_timeout_secs),
        );
        map.insert(
            "agentic_loop.external_tool_timeout_secs".to_string(),
            serde_json::json!(self.agentic_loop.external_tool_timeout_secs),
        );

        // Conversation settings
        map.insert(
            "conversation.ttl_secs".to_string(),
            serde_json::json!(self.conversation.ttl_secs),
        );
        map.insert(
            "conversation.cleanup_interval_secs".to_string(),
            serde_json::json!(self.conversation.cleanup_interval_secs),
        );
        map.insert(
            "conversation.max_per_user".to_string(),
            serde_json::json!(self.conversation.max_per_user),
        );

        // Image extraction settings
        map.insert(
            "image_extraction.background_area_threshold".to_string(),
            serde_json::json!(self.image_extraction.background_area_threshold),
        );
        map.insert(
            "image_extraction.background_min_pages".to_string(),
            serde_json::json!(self.image_extraction.background_min_pages),
        );
        map.insert(
            "image_extraction.text_overlap_min_dpi".to_string(),
            serde_json::json!(self.image_extraction.text_overlap_min_dpi),
        );

        // Traveller Map settings
        map.insert(
            "traveller_map.base_url".to_string(),
            serde_json::Value::String(self.traveller_map.base_url.clone()),
        );
        map.insert(
            "traveller_map.timeout_secs".to_string(),
            serde_json::json!(self.traveller_map.timeout_secs),
        );

        // Traveller Worlds settings
        map.insert(
            "traveller_worlds.base_url".to_string(),
            serde_json::Value::String(self.traveller_worlds.base_url.clone()),
        );
        map.insert(
            "traveller_worlds.chrome_path".to_string(),
            match &self.traveller_worlds.chrome_path {
                Some(path) => serde_json::Value::String(path.clone()),
                None => serde_json::Value::Null,
            },
        );

        map
    }

    /// Apply DB settings as overrides to this config
    pub fn merge_from_db(&mut self, db_settings: &HashMap<String, serde_json::Value>) {
        for (key, value) in db_settings {
            self.apply_setting(key, value);
        }
    }

    /// Apply a single setting value
    fn apply_setting(&mut self, key: &str, value: &serde_json::Value) {
        match key {
            // Ollama settings
            "ollama.base_url" => {
                if let Some(v) = value.as_str() {
                    self.ollama.base_url = v.to_string();
                }
            }
            "ollama.default_model" => {
                if let Some(v) = value.as_str() {
                    self.ollama.default_model = v.to_string();
                }
            }
            "ollama.vision_model" => {
                if let Some(v) = value.as_str() {
                    self.ollama.vision_model = v.to_string();
                }
            }
            "ollama.temperature" => {
                if let Some(v) = value.as_f64() {
                    self.ollama.temperature = v as f32;
                }
            }
            "ollama.request_timeout_secs" => {
                if let Some(v) = value.as_u64() {
                    self.ollama.request_timeout_secs = v;
                }
            }

            // Embeddings settings
            "embeddings.model" => {
                if let Some(v) = value.as_str() {
                    self.embeddings.model = v.to_string();
                }
            }
            "embeddings.chunk_size" => {
                if let Some(v) = value.as_u64() {
                    self.embeddings.chunk_size = v as usize;
                }
            }
            "embeddings.chunk_overlap" => {
                if let Some(v) = value.as_u64() {
                    self.embeddings.chunk_overlap = v as usize;
                }
            }

            // MCP settings
            "mcp.path" => {
                if let Some(v) = value.as_str() {
                    self.mcp.path = v.to_string();
                }
            }
            "mcp.enabled" => {
                if let Some(v) = value.as_bool() {
                    self.mcp.enabled = v;
                }
            }

            // Limits settings
            "limits.max_document_size_bytes" => {
                if let Some(v) = value.as_u64() {
                    self.limits.max_document_size_bytes = v;
                }
            }

            // Agentic loop settings
            "agentic_loop.tool_call_pause_threshold" => {
                if let Some(v) = value.as_u64() {
                    self.agentic_loop.tool_call_pause_threshold = v as u32;
                }
            }
            "agentic_loop.time_pause_threshold_secs" => {
                if let Some(v) = value.as_u64() {
                    self.agentic_loop.time_pause_threshold_secs = v;
                }
            }
            "agentic_loop.hard_timeout_secs" => {
                if let Some(v) = value.as_u64() {
                    self.agentic_loop.hard_timeout_secs = v;
                }
            }
            "agentic_loop.external_tool_timeout_secs" => {
                if let Some(v) = value.as_u64() {
                    self.agentic_loop.external_tool_timeout_secs = v;
                }
            }

            // Conversation settings
            "conversation.ttl_secs" => {
                if let Some(v) = value.as_u64() {
                    self.conversation.ttl_secs = v;
                }
            }
            "conversation.cleanup_interval_secs" => {
                if let Some(v) = value.as_u64() {
                    self.conversation.cleanup_interval_secs = v;
                }
            }
            "conversation.max_per_user" => {
                if let Some(v) = value.as_u64() {
                    self.conversation.max_per_user = v as u32;
                }
            }

            // Image extraction settings
            "image_extraction.background_area_threshold" => {
                if let Some(v) = value.as_f64() {
                    self.image_extraction.background_area_threshold = v;
                }
            }
            "image_extraction.background_min_pages" => {
                if let Some(v) = value.as_u64() {
                    self.image_extraction.background_min_pages = v as usize;
                }
            }
            "image_extraction.text_overlap_min_dpi" => {
                if let Some(v) = value.as_f64() {
                    self.image_extraction.text_overlap_min_dpi = v;
                }
            }

            // Traveller Map settings
            "traveller_map.base_url" => {
                if let Some(v) = value.as_str() {
                    self.traveller_map.base_url = v.to_string();
                }
            }
            "traveller_map.timeout_secs" => {
                if let Some(v) = value.as_u64() {
                    self.traveller_map.timeout_secs = v;
                }
            }

            // Traveller Worlds settings
            "traveller_worlds.base_url" => {
                if let Some(v) = value.as_str() {
                    self.traveller_worlds.base_url = v.to_string();
                }
            }
            "traveller_worlds.chrome_path" => {
                if value.is_null() {
                    self.traveller_worlds.chrome_path = None;
                } else if let Some(v) = value.as_str() {
                    self.traveller_worlds.chrome_path = Some(v.to_string());
                }
            }

            _ => {
                tracing::warn!(key = %key, "Unknown setting key in merge_from_db");
            }
        }
    }
}
