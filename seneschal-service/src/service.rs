//! Service layer coordinator for the Seneschal Program.
//!
//! This module provides the main `SeneschalService` struct that coordinates
//! all service functionality. The implementation is split across submodules
//! for better organization:
//!
//! - `agentic_loop`: LLM interaction with tool calling
//! - `chat`: WebSocket chat session management
//! - `document_processing`: Document upload, chunking, embedding, captioning
//! - `external_tools`: MCP external tool execution via WebSocket
//! - `internal_tools`: Internal tool execution (search, traveller tools, etc.)
//! - `prompts`: System prompt building and message formatting
//! - `state`: In-memory state structures

mod agentic_loop;
mod chat;
mod document_processing;
mod external_tools;
mod internal_tools;
mod prompts;
mod state;

pub use state::ActiveRequest;

use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::config::RuntimeConfig;
use crate::db::{Conversation, Database};
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
    pub active_requests: Arc<DashMap<String, ActiveRequest>>,
    pub ws_manager: Arc<WebSocketManager>,
    /// Client for Traveller Map API
    pub traveller_map_client: TravellerMapClient,
    /// Client for Traveller Worlds (travellerworlds.com) map generation
    pub traveller_worlds_client: TravellerWorldsClient,
    /// Senders for tool results, keyed by conversation_id
    pub(crate) tool_result_senders: Arc<DashMap<String, oneshot::Sender<serde_json::Value>>>,
    /// Senders for continue signals, keyed by conversation_id
    pub(crate) continue_senders: Arc<DashMap<String, oneshot::Sender<()>>>,
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
            active_requests: Arc::new(DashMap::new()),
            ws_manager,
            traveller_map_client,
            traveller_worlds_client,
            tool_result_senders: Arc::new(DashMap::new()),
            continue_senders: Arc::new(DashMap::new()),
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

    /// Get conversation history
    pub fn get_conversation(&self, conversation_id: &str) -> ServiceResult<Option<Conversation>> {
        self.db.get_conversation(conversation_id)
    }

    /// List conversations for a user
    pub fn list_conversations(
        &self,
        user_id: &str,
        limit: usize,
    ) -> ServiceResult<Vec<Conversation>> {
        self.db.list_conversations(user_id, limit)
    }

    /// Run conversation cleanup
    pub fn cleanup_conversations(&self) -> ServiceResult<usize> {
        let ttl = self.runtime_config.dynamic().conversation.ttl();
        let cutoff = Utc::now() - chrono::Duration::from_std(ttl).unwrap_or_default();
        self.db.cleanup_old_conversations(cutoff)
    }

    /// Remove excess conversations per user
    pub fn cleanup_excess_conversations(&self, max_per_user: u32) -> ServiceResult<usize> {
        self.db.cleanup_excess_conversations_all(max_per_user)
    }

    /// Clone for spawning tasks
    pub(crate) fn clone_for_task(&self) -> Self {
        Self {
            runtime_config: self.runtime_config.clone(),
            db: self.db.clone(),
            ollama: self.ollama.clone(),
            search: self.search.clone(),
            ingestion: self.ingestion.clone(),
            i18n: self.i18n.clone(),
            active_requests: self.active_requests.clone(),
            ws_manager: self.ws_manager.clone(),
            traveller_map_client: self.traveller_map_client.clone(),
            traveller_worlds_client: self.traveller_worlds_client.clone(),
            tool_result_senders: self.tool_result_senders.clone(),
            continue_senders: self.continue_senders.clone(),
            mcp_tool_result_senders: self.mcp_tool_result_senders.clone(),
            processing_cancellation_tokens: self.processing_cancellation_tokens.clone(),
        }
    }
}
