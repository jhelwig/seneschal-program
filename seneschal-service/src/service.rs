//! Service layer coordinator for the Seneschal Program.
//!
//! This module provides the main `SeneschalService` struct that coordinates
//! all service functionality. The implementation is split across submodules
//! for better organization:
//!
//! - `document_processing`: Document upload, chunking, embedding, captioning
//! - `external_tools`: MCP external tool execution via WebSocket

mod document_processing;
mod external_tools;

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::config::RuntimeConfig;
use crate::db::Database;
use crate::error::ServiceResult;
use crate::i18n::I18n;
use crate::ingestion::IngestionService;
use crate::ollama::OllamaClient;
use crate::search::{SearchResult, SearchService};
use crate::tools::{SearchFilters, TravellerMapClient, TravellerWorldsClient};
use crate::websocket::WebSocketManager;

/// Main service coordinator
pub struct SeneschalService {
    pub runtime_config: Arc<RuntimeConfig>,
    pub db: Arc<Database>,
    pub ollama: Arc<OllamaClient>,
    pub search: Arc<SearchService>,
    pub ingestion: Arc<IngestionService>,
    pub i18n: Arc<I18n>,
    pub ws_manager: Arc<WebSocketManager>,
    /// Client for Traveller Map API
    pub traveller_map_client: TravellerMapClient,
    /// Client for Traveller Worlds (travellerworlds.com) map generation
    pub traveller_worlds_client: TravellerWorldsClient,
    /// Senders for MCP tool results, keyed by request_id ("mcp:{uuid}")
    pub(crate) mcp_tool_result_senders: Arc<DashMap<String, oneshot::Sender<serde_json::Value>>>,
    /// Cancellation tokens for documents currently being processed.
    /// Key: document_id, Value: CancellationToken
    pub(crate) processing_cancellation_tokens: Arc<DashMap<String, CancellationToken>>,
}

impl SeneschalService {
    /// Create a new service instance
    /// Accepts a pre-opened database so that RuntimeConfig can load settings from it
    pub async fn new(db: Arc<Database>, runtime_config: Arc<RuntimeConfig>) -> ServiceResult<Self> {
        info!("Initializing Seneschal Program service");

        // Get current dynamic config
        let dynamic = runtime_config.dynamic();

        // Initialize Ollama client
        let ollama = Arc::new(OllamaClient::new(dynamic.ollama.clone())?);

        // Check Ollama availability
        if ollama.health_check().await? {
            info!(url = %dynamic.ollama.base_url, "Ollama is available");
        } else {
            warn!(url = %dynamic.ollama.base_url, "Ollama is not available");
        }

        // Initialize search service
        let search = Arc::new(
            SearchService::new(db.clone(), &dynamic.embeddings, &dynamic.ollama.base_url).await?,
        );

        // Initialize ingestion service
        let ingestion = Arc::new(IngestionService::new(
            &dynamic.embeddings,
            dynamic.image_extraction.clone(),
            runtime_config.static_config.storage.data_dir.clone(),
        ));

        // Initialize i18n
        let i18n = Arc::new(I18n::new());

        // Initialize WebSocket manager
        let ws_manager = Arc::new(WebSocketManager::new());

        // Initialize Traveller Map API client
        let traveller_map_client = TravellerMapClient::new(
            &dynamic.traveller_map.base_url,
            dynamic.traveller_map.timeout_secs,
        );
        info!(
            url = %dynamic.traveller_map.base_url,
            "Traveller Map API client initialized"
        );

        // Initialize Traveller Worlds client
        let traveller_worlds_client = TravellerWorldsClient::new(
            &dynamic.traveller_worlds.base_url,
            dynamic.traveller_worlds.chrome_path.clone(),
        );
        info!(
            base_url = %dynamic.traveller_worlds.base_url,
            chrome_path = ?dynamic.traveller_worlds.chrome_path,
            "Traveller Worlds client initialized"
        );

        Ok(Self {
            runtime_config,
            db,
            ollama,
            search,
            ingestion,
            i18n,
            ws_manager,
            traveller_map_client,
            traveller_worlds_client,
            mcp_tool_result_senders: Arc::new(DashMap::new()),
            processing_cancellation_tokens: Arc::new(DashMap::new()),
        })
    }

    /// Update settings and hot-reload affected components
    pub async fn update_settings(
        &self,
        updates: std::collections::HashMap<String, serde_json::Value>,
    ) -> ServiceResult<()> {
        // Persist to DB
        self.db.set_settings(updates)?;

        // Reload config from DB
        self.runtime_config.reload_from_db(&self.db)?;

        Ok(())
    }

    /// Search documents
    pub async fn search(
        &self,
        query: &str,
        user_role: u8,
        limit: usize,
        filters: Option<SearchFilters>,
    ) -> ServiceResult<Vec<SearchResult>> {
        self.search.search(query, user_role, limit, filters).await
    }
}
