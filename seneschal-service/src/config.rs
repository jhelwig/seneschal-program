use config::{Config, Environment, File};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

/// Main application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_server")]
    pub server: ServerConfig,

    #[serde(default = "default_ollama")]
    pub ollama: OllamaConfig,

    #[serde(default = "default_embeddings")]
    pub embeddings: EmbeddingsConfig,

    #[serde(default = "default_storage")]
    pub storage: StorageConfig,

    #[serde(default)]
    pub fvtt: FvttConfig,

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
}

/// HTTP server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port")]
    pub port: u16,
}

/// Ollama LLM configuration
#[derive(Debug, Clone, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_url")]
    pub base_url: String,

    #[serde(default = "default_model")]
    pub default_model: String,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

/// Embeddings configuration
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingsConfig {
    #[serde(default = "default_embedding_model")]
    pub model: String,

    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,

    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap: usize,
}

/// Storage configuration
#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

/// MCP server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_mcp_path")]
    pub path: String,

    #[serde(default = "default_mcp_enabled")]
    pub enabled: bool,
}

/// Size limits
#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    #[serde(default = "default_max_document_size")]
    pub max_document_size_bytes: u64,
}

/// Agentic loop configuration
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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

// Default value functions
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

impl AppConfig {
    /// Load configuration from layered sources:
    /// 1. Default values (compiled in)
    /// 2. Config file (config.toml, if present)
    /// 3. Environment variables (SENESCHAL__*)
    pub fn load() -> Result<Self, config::ConfigError> {
        Config::builder()
            // Layer config file if present
            .add_source(File::with_name("config").required(false))
            // Layer environment variables (prefix: SENESCHAL__)
            .add_source(
                Environment::with_prefix("SENESCHAL")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?
            .try_deserialize()
    }
}
