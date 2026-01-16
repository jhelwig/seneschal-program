use arc_swap::ArcSwap;
use config::{Config, Environment, File};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::db::Database;
use crate::error::ServiceResult;

// ==================== Static Configuration (startup-only) ====================

/// Static configuration that cannot be changed at runtime
/// These settings affect server binding or require restart to change
#[derive(Debug, Clone, Deserialize)]
pub struct StaticConfig {
    #[serde(default = "default_server")]
    pub server: ServerConfig,

    #[serde(default = "default_storage")]
    pub storage: StorageConfig,

    #[serde(default)]
    pub fvtt: FvttConfig,
}

/// HTTP server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port")]
    pub port: u16,
}

/// Storage configuration
#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    /// Optional auto-import directory. When set, files placed here are automatically
    /// imported. Files are moved to processed/ or failed/ subdirectories after import.
    #[serde(default)]
    pub auto_import_dir: Option<PathBuf>,
}

/// FVTT integration configuration
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FvttConfig {
    /// Path to FVTT assets directory (Data/assets). If provided and writable,
    /// images are copied directly. Otherwise, shuttled via API.
    #[serde(default)]
    pub assets_path: Option<PathBuf>,
}

/// Determines how to deliver images to FVTT
#[derive(Debug, Clone)]
pub enum AssetsAccess {
    /// Backend can write directly to FVTT assets directory
    Direct(PathBuf),
    /// Images must be shuttled via API to the module
    Shuttle,
}

impl FvttConfig {
    /// Check if we can write directly to FVTT assets
    pub fn check_assets_access(&self) -> AssetsAccess {
        match &self.assets_path {
            None => AssetsAccess::Shuttle,
            Some(path) => {
                // Test write access by creating the seneschal directory
                let seneschal_dir = path.join("seneschal");
                match std::fs::create_dir_all(&seneschal_dir) {
                    // Return the base assets path, not the seneschal subdir
                    // The fvtt_image_path function includes seneschal/ in its output
                    Ok(_) => AssetsAccess::Direct(path.clone()),
                    Err(e) => {
                        tracing::warn!(
                            path = %seneschal_dir.display(),
                            error = %e,
                            "FVTT assets path not writable, falling back to API shuttle"
                        );
                        AssetsAccess::Shuttle
                    }
                }
            }
        }
    }
}

// ==================== Dynamic Configuration (hot-reloadable) ====================

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

/// Ollama LLM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_url")]
    pub base_url: String,

    #[serde(default = "default_model")]
    pub default_model: String,

    /// Vision model for image captioning (e.g., llava, moondream). Empty means no captioning.
    #[serde(default)]
    pub vision_model: String,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

/// Embeddings configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    #[serde(default = "default_embedding_model")]
    pub model: String,

    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,

    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap: usize,
}

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_mcp_path")]
    pub path: String,

    #[serde(default = "default_mcp_enabled")]
    pub enabled: bool,
}

/// Size limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    #[serde(default = "default_max_document_size")]
    pub max_document_size_bytes: u64,
}

/// Agentic loop configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticLoopConfig {
    /// Tool calls before pause prompt (internal + external combined)
    #[serde(default = "default_tool_call_pause_threshold")]
    pub tool_call_pause_threshold: u32,

    /// Time before pause prompt in seconds
    #[serde(default = "default_time_pause_threshold_secs")]
    pub time_pause_threshold_secs: u64,

    /// Hard timeout in seconds (cannot continue past this)
    #[serde(default = "default_hard_timeout_secs")]
    pub hard_timeout_secs: u64,

    /// Timeout waiting for external tool result from client in seconds
    #[serde(default = "default_external_tool_timeout_secs")]
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
    #[serde(default = "default_conversation_ttl_secs")]
    pub ttl_secs: u64,

    /// Run cleanup every N seconds
    #[serde(default = "default_cleanup_interval_secs")]
    pub cleanup_interval_secs: u64,

    /// Maximum conversations per user (0 = unlimited)
    #[serde(default = "default_max_per_user")]
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

// ==================== DynamicConfig Settings Keys ====================

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
    "conversation.ttl_secs",
    "conversation.cleanup_interval_secs",
    "conversation.max_per_user",
    "image_extraction.background_area_threshold",
    "image_extraction.background_min_pages",
    "image_extraction.text_overlap_min_dpi",
    "traveller_map.base_url",
    "traveller_map.timeout_secs",
    "traveller_worlds.base_url",
    "traveller_worlds.chrome_path",
];

impl DynamicConfig {
    /// Get all valid setting keys
    pub fn valid_keys() -> HashSet<&'static str> {
        VALID_SETTING_KEYS.iter().copied().collect()
    }

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

// ==================== RuntimeConfig (combines static + dynamic) ====================

/// Runtime configuration manager
/// Combines static config (startup-only) with dynamic config (hot-reloadable via ArcSwap)
pub struct RuntimeConfig {
    /// Static configuration (never changes after startup)
    pub static_config: StaticConfig,
    /// Dynamic configuration (can be hot-reloaded)
    dynamic: ArcSwap<DynamicConfig>,
}

impl RuntimeConfig {
    /// Get current dynamic config snapshot (lock-free read)
    pub fn dynamic(&self) -> arc_swap::Guard<Arc<DynamicConfig>> {
        self.dynamic.load()
    }

    /// Update dynamic config (atomic swap)
    pub fn update_dynamic(&self, new_config: DynamicConfig) {
        self.dynamic.store(Arc::new(new_config));
    }

    /// Load config from all sources with DB overrides
    pub fn load(db: &Database) -> ServiceResult<Self> {
        // Load static config from env/file
        let static_config = load_static_config()?;

        // Load dynamic config defaults from env/file, then apply DB overrides
        let mut dynamic = load_dynamic_config()?;
        let db_settings = db.get_all_settings()?;
        dynamic.merge_from_db(&db_settings);

        Ok(Self {
            static_config,
            dynamic: ArcSwap::from_pointee(dynamic),
        })
    }

    /// Rebuild dynamic config from file/env defaults + DB and swap atomically
    pub fn reload_from_db(&self, db: &Database) -> ServiceResult<()> {
        let mut dynamic = load_dynamic_config()?;
        let db_settings = db.get_all_settings()?;
        dynamic.merge_from_db(&db_settings);
        self.update_dynamic(dynamic);
        Ok(())
    }
}

// ==================== Config Loading Functions ====================

/// Internal struct for loading static fields from config sources
#[derive(Debug, Clone, Deserialize)]
struct StaticConfigLoader {
    #[serde(default = "default_server")]
    pub server: ServerConfig,

    #[serde(default = "default_storage")]
    pub storage: StorageConfig,

    #[serde(default)]
    pub fvtt: FvttConfig,
}

/// Load static configuration from file and env vars
fn load_static_config() -> ServiceResult<StaticConfig> {
    let loader: StaticConfigLoader = Config::builder()
        .add_source(File::with_name("config").required(false))
        .add_source(
            Environment::with_prefix("SENESCHAL")
                .separator("__")
                .try_parsing(true),
        )
        .build()
        .map_err(|e| crate::error::ServiceError::Config {
            message: format!("Failed to build config: {}", e),
        })?
        .try_deserialize()
        .map_err(|e| crate::error::ServiceError::Config {
            message: format!("Failed to deserialize static config: {}", e),
        })?;

    Ok(StaticConfig {
        server: loader.server,
        storage: loader.storage,
        fvtt: loader.fvtt,
    })
}

/// Load dynamic configuration from file and env vars (without DB overrides)
fn load_dynamic_config() -> ServiceResult<DynamicConfig> {
    Config::builder()
        .add_source(File::with_name("config").required(false))
        .add_source(
            Environment::with_prefix("SENESCHAL")
                .separator("__")
                .try_parsing(true),
        )
        .build()
        .map_err(|e| crate::error::ServiceError::Config {
            message: format!("Failed to build config: {}", e),
        })?
        .try_deserialize()
        .map_err(|e| crate::error::ServiceError::Config {
            message: format!("Failed to deserialize dynamic config: {}", e),
        })
}

// ==================== Default Value Functions ====================

fn default_server() -> ServerConfig {
    ServerConfig {
        host: default_host(),
        port: default_port(),
    }
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_ollama() -> OllamaConfig {
    OllamaConfig {
        base_url: default_ollama_url(),
        default_model: default_model(),
        vision_model: String::new(), // Empty means no image captioning
        temperature: default_temperature(),
        request_timeout_secs: default_request_timeout_secs(),
    }
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_model() -> String {
    "llama3.2".to_string()
}

fn default_temperature() -> f32 {
    0.7
}

fn default_request_timeout_secs() -> u64 {
    120
}

fn default_embeddings() -> EmbeddingsConfig {
    EmbeddingsConfig {
        model: default_embedding_model(),
        chunk_size: default_chunk_size(),
        chunk_overlap: default_chunk_overlap(),
    }
}

fn default_embedding_model() -> String {
    "nomic-embed-text".to_string()
}

fn default_chunk_size() -> usize {
    512
}

fn default_chunk_overlap() -> usize {
    64
}

fn default_storage() -> StorageConfig {
    StorageConfig {
        data_dir: default_data_dir(),
        auto_import_dir: None,
    }
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("./data")
}

fn default_mcp() -> McpConfig {
    McpConfig {
        path: default_mcp_path(),
        enabled: default_mcp_enabled(),
    }
}

fn default_mcp_path() -> String {
    "/mcp".to_string()
}

fn default_mcp_enabled() -> bool {
    true
}

fn default_limits() -> LimitsConfig {
    LimitsConfig {
        max_document_size_bytes: default_max_document_size(),
    }
}

fn default_max_document_size() -> u64 {
    104_857_600 // 100MB
}

fn default_agentic_loop() -> AgenticLoopConfig {
    AgenticLoopConfig {
        tool_call_pause_threshold: default_tool_call_pause_threshold(),
        time_pause_threshold_secs: default_time_pause_threshold_secs(),
        hard_timeout_secs: default_hard_timeout_secs(),
        external_tool_timeout_secs: default_external_tool_timeout_secs(),
    }
}

fn default_tool_call_pause_threshold() -> u32 {
    u32::MAX // Effectively disabled
}

fn default_time_pause_threshold_secs() -> u64 {
    u64::MAX // Effectively disabled
}

fn default_hard_timeout_secs() -> u64 {
    300
}

fn default_external_tool_timeout_secs() -> u64 {
    30
}

fn default_conversation() -> ConversationConfig {
    ConversationConfig {
        ttl_secs: default_conversation_ttl_secs(),
        cleanup_interval_secs: default_cleanup_interval_secs(),
        max_per_user: default_max_per_user(),
    }
}

fn default_conversation_ttl_secs() -> u64 {
    7 * 24 * 60 * 60 // 7 days
}

fn default_cleanup_interval_secs() -> u64 {
    24 * 60 * 60 // 24 hours
}

fn default_max_per_user() -> u32 {
    100
}

fn default_image_extraction() -> ImageExtractionConfig {
    ImageExtractionConfig {
        background_area_threshold: default_background_area_threshold(),
        background_min_pages: default_background_min_pages(),
        text_overlap_min_dpi: default_text_overlap_min_dpi(),
    }
}

fn default_background_area_threshold() -> f64 {
    0.9
}

fn default_background_min_pages() -> usize {
    2
}

fn default_text_overlap_min_dpi() -> f64 {
    300.0
}

fn default_traveller_map() -> TravellerMapConfig {
    TravellerMapConfig::default()
}

fn default_traveller_map_url() -> String {
    "https://travellermap.com".to_string()
}

fn default_traveller_map_timeout() -> u64 {
    30
}

fn default_traveller_worlds() -> TravellerWorldsConfig {
    TravellerWorldsConfig::default()
}

fn default_traveller_worlds_url() -> String {
    "http://www.travellerworlds.com".to_string()
}
