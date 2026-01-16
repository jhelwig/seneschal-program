mod state;

pub use state::{ActiveRequest, PendingToolCall, UserContext};

use base64::Engine;
use chrono::Utc;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::{AssetsAccess, RuntimeConfig};
use crate::db::{
    CaptioningStatus, Conversation, ConversationMessage, ConversationMetadata, Database, Document,
    FvttImageDescription, MessageRole, ProcessingStatus, ToolCallRecord, ToolResultRecord,
};
use crate::error::{ServiceError, ServiceResult, format_error_chain_ref};
use crate::i18n::I18n;
use crate::ingestion::IngestionService;
use crate::ollama::{
    ChatMessage, ChatRequest, OllamaClient, OllamaFunctionCall, OllamaToolCall, StreamEvent,
};
use crate::search::{SearchResult, SearchService, format_search_results_for_llm};
use crate::tools::{
    AccessLevel, SearchFilters, TagMatch, ToolCall, ToolLocation, ToolResult, TravellerMapClient,
    TravellerMapTool, TravellerTool, classify_tool,
};
use crate::websocket::{DocumentProgressUpdate, ServerMessage, WebSocketManager};

/// Sanitize a string for use in a filename (for Traveller Map assets)
fn sanitize_map_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ' ' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

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
    /// Senders for tool results, keyed by conversation_id
    tool_result_senders: Arc<DashMap<String, oneshot::Sender<serde_json::Value>>>,
    /// Senders for continue signals, keyed by conversation_id
    continue_senders: Arc<DashMap<String, oneshot::Sender<()>>>,
    /// Senders for MCP tool results, keyed by request_id ("mcp:{uuid}")
    mcp_tool_result_senders: Arc<DashMap<String, oneshot::Sender<serde_json::Value>>>,
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
            tool_result_senders: Arc::new(DashMap::new()),
            continue_senders: Arc::new(DashMap::new()),
            mcp_tool_result_senders: Arc::new(DashMap::new()),
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

        // Note: Some settings may require reinitializing clients (e.g., ollama URL change)
        // For now, components read config fresh each request where needed
        // Full client reinitialization can be added in future if needed

        Ok(())
    }

    /// Run the agentic loop with WebSocket output
    async fn run_agentic_loop_ws(
        &self,
        conversation_id: String,
        user_context: UserContext,
        model: Option<String>,
        enabled_tools: Option<Vec<String>>,
        session_id: String,
        ws_manager: Arc<WebSocketManager>,
    ) {
        let dynamic_config = self.runtime_config.dynamic();
        let loop_config = dynamic_config.agentic_loop.clone();

        // Get model's context length (do this once at the start)
        let model_name = model
            .as_deref()
            .unwrap_or(&dynamic_config.ollama.default_model);
        let context_length = self.ollama.get_model_context_length(model_name).await;
        debug!(
            model = %model_name,
            context_length = ?context_length,
            "Fetched model context length"
        );

        loop {
            // Check if we should stop
            let active_request = match self.active_requests.get(&conversation_id) {
                Some(r) => r.clone(),
                None => {
                    debug!(
                        conversation_id = %conversation_id,
                        "Active request not found, stopping loop"
                    );
                    break;
                }
            };

            debug!(
                conversation_id = %conversation_id,
                tool_calls_made = active_request.tool_calls_made,
                paused = active_request.paused,
                pending_external_tool = active_request.pending_external_tool.is_some(),
                message_count = active_request.messages.len(),
                "WebSocket agentic loop iteration starting"
            );

            // Check hard timeout
            if active_request.started_at.elapsed() > loop_config.hard_timeout() {
                ws_manager.send_to(
                    &session_id,
                    ServerMessage::ChatError {
                        conversation_id: conversation_id.clone(),
                        message: self.i18n.get("en", "error-timeout", None),
                        recoverable: false,
                    },
                );
                break;
            }

            // Check pause conditions
            if active_request.tool_calls_made >= loop_config.tool_call_pause_threshold
                && !active_request.paused
            {
                info!(
                    conversation_id = %conversation_id,
                    tool_calls_made = active_request.tool_calls_made,
                    threshold = loop_config.tool_call_pause_threshold,
                    "Tool call limit reached, pausing loop"
                );

                self.active_requests
                    .entry(conversation_id.clone())
                    .and_modify(|r| r.paused = true);

                ws_manager.send_to(
                    &session_id,
                    ServerMessage::ChatPaused {
                        conversation_id: conversation_id.clone(),
                        reason: "tool_limit".to_string(),
                        tool_calls_made: active_request.tool_calls_made,
                        elapsed_seconds: active_request.started_at.elapsed().as_secs(),
                        message: self.i18n.format(
                            "en",
                            "chat-pause-tool-limit",
                            &[("count", &active_request.tool_calls_made.to_string())],
                        ),
                    },
                );

                // Wait for continue signal
                let (tx, rx) = oneshot::channel();
                self.continue_senders.insert(conversation_id.clone(), tx);

                match tokio::time::timeout(loop_config.hard_timeout(), rx).await {
                    Ok(Ok(())) => {
                        debug!(conversation_id = %conversation_id, "Continue signal received");
                        continue;
                    }
                    _ => {
                        debug!(conversation_id = %conversation_id, "Continue timeout or cancelled");
                        break;
                    }
                }
            }

            if active_request.started_at.elapsed() > loop_config.time_pause_threshold()
                && !active_request.paused
            {
                self.active_requests
                    .entry(conversation_id.clone())
                    .and_modify(|r| r.paused = true);

                let elapsed = active_request.started_at.elapsed().as_secs();
                ws_manager.send_to(
                    &session_id,
                    ServerMessage::ChatPaused {
                        conversation_id: conversation_id.clone(),
                        reason: "time_limit".to_string(),
                        tool_calls_made: active_request.tool_calls_made,
                        elapsed_seconds: elapsed,
                        message: self.i18n.format(
                            "en",
                            "chat-pause-time-limit",
                            &[("seconds", &elapsed.to_string())],
                        ),
                    },
                );

                // Wait for continue signal
                let (tx, rx) = oneshot::channel();
                self.continue_senders.insert(conversation_id.clone(), tx);

                match tokio::time::timeout(loop_config.hard_timeout(), rx).await {
                    Ok(Ok(())) => {
                        debug!(conversation_id = %conversation_id, "Continue signal received");
                        continue;
                    }
                    _ => {
                        debug!(conversation_id = %conversation_id, "Continue timeout or cancelled");
                        break;
                    }
                }
            }

            // Build messages for Ollama
            let ollama_messages = self.build_ollama_messages(&active_request, &user_context);

            // Determine if tools should be enabled
            let tools_enabled = enabled_tools.as_ref().is_none_or(|t| !t.is_empty());

            debug!(
                conversation_id = %conversation_id,
                enabled_tools = ?enabled_tools,
                tools_enabled = tools_enabled,
                message_count = ollama_messages.len(),
                tool_calls_made = active_request.tool_calls_made,
                "Building chat request for Ollama"
            );

            // Call Ollama
            let chat_request = ChatRequest {
                model: model.clone(),
                messages: ollama_messages,
                temperature: None,
                num_ctx: context_length,
                enable_tools: tools_enabled,
            };

            let mut stream = match self.ollama.chat_stream(chat_request).await {
                Ok(s) => s,
                Err(e) => {
                    error!(error = %e, "Ollama chat failed");
                    ws_manager.send_to(
                        &session_id,
                        ServerMessage::ChatError {
                            conversation_id: conversation_id.clone(),
                            message: e.to_string(),
                            recoverable: false,
                        },
                    );
                    break;
                }
            };

            let mut accumulated_content = String::new();
            let mut tool_calls = Vec::new();
            let mut done = false;
            let mut final_usage = (None, None);

            // Process stream events
            while let Some(event) = stream.recv().await {
                match event {
                    StreamEvent::Content(text) => {
                        accumulated_content.push_str(&text);
                        ws_manager.send_to(
                            &session_id,
                            ServerMessage::ChatContent {
                                conversation_id: conversation_id.clone(),
                                text,
                            },
                        );
                    }
                    StreamEvent::ToolCall(call) => {
                        debug!(
                            conversation_id = %conversation_id,
                            tool_call_id = %call.id,
                            tool_name = %call.tool,
                            "Tool call received from LLM"
                        );
                        tool_calls.push(call);
                    }
                    StreamEvent::Done {
                        prompt_eval_count,
                        eval_count,
                        ..
                    } => {
                        let content_preview: String =
                            accumulated_content.chars().take(500).collect();
                        debug!(
                            conversation_id = %conversation_id,
                            prompt_tokens = ?prompt_eval_count,
                            completion_tokens = ?eval_count,
                            tool_call_count = tool_calls.len(),
                            content_length = accumulated_content.len(),
                            content_preview = %content_preview,
                            "LLM response complete"
                        );
                        done = true;
                        final_usage = (prompt_eval_count, eval_count);
                    }
                    StreamEvent::Error(e) => {
                        warn!(
                            conversation_id = %conversation_id,
                            error = %e,
                            "LLM stream error received"
                        );
                        ws_manager.send_to(
                            &session_id,
                            ServerMessage::ChatError {
                                conversation_id: conversation_id.clone(),
                                message: e,
                                recoverable: true,
                            },
                        );
                        done = true;
                    }
                }

                if done {
                    break;
                }
            }

            // Process tool calls
            if !tool_calls.is_empty() {
                // Add assistant message with tool calls to conversation
                self.active_requests
                    .entry(conversation_id.clone())
                    .and_modify(|r| {
                        r.messages.push(ConversationMessage {
                            role: MessageRole::Assistant,
                            content: accumulated_content.clone(),
                            timestamp: Utc::now(),
                            tool_calls: Some(
                                tool_calls
                                    .iter()
                                    .map(|tc| ToolCallRecord {
                                        id: tc.id.clone(),
                                        tool: tc.tool.clone(),
                                        args: tc.args.clone(),
                                    })
                                    .collect(),
                            ),
                            tool_results: None,
                        });
                    });

                for call in &tool_calls {
                    let location = classify_tool(&call.tool);

                    match location {
                        ToolLocation::Internal => {
                            // Send status update
                            ws_manager.send_to(
                                &session_id,
                                ServerMessage::ChatToolStatus {
                                    conversation_id: conversation_id.clone(),
                                    tool_call_id: call.id.clone(),
                                    message: self.i18n.format(
                                        "en",
                                        "chat-executing-tool",
                                        &[("tool", &call.tool)],
                                    ),
                                },
                            );

                            let result = self.execute_internal_tool(call, &user_context).await;

                            // Log tool result
                            match &result.outcome {
                                crate::tools::ToolOutcome::Success { result: res } => {
                                    debug!(
                                        conversation_id = %conversation_id,
                                        tool_call_id = %call.id,
                                        tool_name = %call.tool,
                                        result_preview = %format!("{:.200}", res.to_string()),
                                        "Internal tool execution succeeded"
                                    );
                                }
                                crate::tools::ToolOutcome::Error { error } => {
                                    warn!(
                                        conversation_id = %conversation_id,
                                        tool_call_id = %call.id,
                                        tool_name = %call.tool,
                                        error = %error,
                                        "Internal tool execution failed"
                                    );
                                }
                            }

                            // Add tool result to conversation
                            self.active_requests
                                .entry(conversation_id.clone())
                                .and_modify(|r| {
                                    r.tool_calls_made += 1;
                                    r.messages.push(ConversationMessage {
                                        role: MessageRole::Tool,
                                        content: serde_json::to_string(&result).unwrap_or_default(),
                                        timestamp: Utc::now(),
                                        tool_calls: None,
                                        tool_results: Some(vec![ToolResultRecord {
                                            tool_call_id: call.id.clone(),
                                            result: match &result.outcome {
                                                crate::tools::ToolOutcome::Success { result } => {
                                                    result.clone()
                                                }
                                                crate::tools::ToolOutcome::Error { error } => {
                                                    serde_json::json!({ "error": error })
                                                }
                                            },
                                            error: match &result.outcome {
                                                crate::tools::ToolOutcome::Error { error } => {
                                                    Some(error.clone())
                                                }
                                                _ => None,
                                            },
                                        }]),
                                    });
                                });

                            ws_manager.send_to(
                                &session_id,
                                ServerMessage::ChatToolResult {
                                    conversation_id: conversation_id.clone(),
                                    tool_call_id: call.id.clone(),
                                    tool: call.tool.clone(),
                                    summary: self.i18n.format(
                                        "en",
                                        "chat-tool-complete",
                                        &[("tool", &call.tool)],
                                    ),
                                },
                            );
                        }
                        ToolLocation::External => {
                            // Request external tool execution from client
                            debug!(
                                conversation_id = %conversation_id,
                                tool_call_id = %call.id,
                                tool_name = %call.tool,
                                tool_args = %call.args,
                                "Sending external tool call to client via WebSocket"
                            );

                            self.active_requests
                                .entry(conversation_id.clone())
                                .and_modify(|r| {
                                    r.pending_external_tool = Some(PendingToolCall {
                                        id: call.id.clone(),
                                        tool: call.tool.clone(),
                                        args: call.args.clone(),
                                        sent_at: Instant::now(),
                                    });
                                    r.tool_calls_made += 1;
                                });

                            ws_manager.send_to(
                                &session_id,
                                ServerMessage::ChatToolCall {
                                    conversation_id: conversation_id.clone(),
                                    id: call.id.clone(),
                                    tool: call.tool.clone(),
                                    args: call.args.clone(),
                                },
                            );

                            // Wait for tool result via oneshot channel
                            let (tx, rx) = oneshot::channel();
                            self.tool_result_senders.insert(conversation_id.clone(), tx);

                            match tokio::time::timeout(loop_config.external_tool_timeout(), rx)
                                .await
                            {
                                Ok(Ok(_result)) => {
                                    debug!(
                                        conversation_id = %conversation_id,
                                        tool_call_id = %call.id,
                                        "External tool result received"
                                    );
                                    // Result is already added to messages by handle_tool_result_ws
                                }
                                Ok(Err(_)) => {
                                    warn!(
                                        conversation_id = %conversation_id,
                                        tool_call_id = %call.id,
                                        "External tool channel closed (client disconnected?)"
                                    );
                                    ws_manager.send_to(
                                        &session_id,
                                        ServerMessage::ChatError {
                                            conversation_id: conversation_id.clone(),
                                            message:
                                                "Client disconnected while waiting for tool result"
                                                    .to_string(),
                                            recoverable: false,
                                        },
                                    );
                                    // Clean up and exit
                                    self.active_requests.remove(&conversation_id);
                                    return;
                                }
                                Err(_) => {
                                    error!(
                                        conversation_id = %conversation_id,
                                        tool = %call.tool,
                                        "External tool call timed out"
                                    );
                                    ws_manager.send_to(
                                        &session_id,
                                        ServerMessage::ChatError {
                                            conversation_id: conversation_id.clone(),
                                            message: format!(
                                                "External tool '{}' timed out waiting for response",
                                                call.tool
                                            ),
                                            recoverable: false,
                                        },
                                    );
                                    // Clean up and exit
                                    self.active_requests.remove(&conversation_id);
                                    return;
                                }
                            }
                        }
                    }
                }

                // After processing all tool calls, continue the loop
                continue;
            }

            // No tool calls - add assistant message and finish
            if !accumulated_content.is_empty() {
                self.active_requests
                    .entry(conversation_id.clone())
                    .and_modify(|r| {
                        r.messages.push(ConversationMessage {
                            role: MessageRole::Assistant,
                            content: accumulated_content.clone(),
                            timestamp: Utc::now(),
                            tool_calls: None,
                            tool_results: None,
                        });
                    });
            }

            // Send turn complete
            ws_manager.send_to(
                &session_id,
                ServerMessage::ChatTurnComplete {
                    conversation_id: conversation_id.clone(),
                    prompt_tokens: final_usage.0,
                    completion_tokens: final_usage.1,
                },
            );
            break;
        }

        // Save conversation to database
        if let Some(req) = self.active_requests.get(&conversation_id) {
            let conversation = Conversation {
                id: conversation_id.clone(),
                user_id: req.user_context.user_id.clone(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                messages: req.messages.clone(),
                metadata: Some(ConversationMetadata::default()),
            };

            if let Err(e) = self.db.upsert_conversation(&conversation) {
                error!(error = %e, "Failed to save conversation");
            }
        }

        // Clean up active request
        self.active_requests.remove(&conversation_id);
        self.tool_result_senders.remove(&conversation_id);
        self.continue_senders.remove(&conversation_id);
    }

    /// Build Ollama messages from conversation
    fn build_ollama_messages(
        &self,
        request: &ActiveRequest,
        user_context: &UserContext,
    ) -> Vec<ChatMessage> {
        let mut messages = vec![ChatMessage::system(self.build_system_prompt(user_context))];

        // Log all source messages before building
        debug!(
            source_message_count = request.messages.len(),
            "Building Ollama messages from conversation"
        );
        for (i, msg) in request.messages.iter().enumerate() {
            let tool_call_count = msg.tool_calls.as_ref().map(|tc| tc.len()).unwrap_or(0);
            debug!(
                index = i,
                role = ?msg.role,
                content_length = msg.content.len(),
                content_preview = %msg.content.chars().take(100).collect::<String>(),
                tool_call_count = tool_call_count,
                "Source message {}", i
            );
        }

        for msg in &request.messages {
            let chat_msg = match msg.role {
                MessageRole::User => ChatMessage::user(&msg.content),
                MessageRole::Assistant => {
                    if let Some(tool_calls) = &msg.tool_calls {
                        ChatMessage::assistant_with_tool_calls(
                            &msg.content,
                            tool_calls
                                .iter()
                                .map(|tc| OllamaToolCall {
                                    function: OllamaFunctionCall {
                                        name: tc.tool.clone(),
                                        arguments: tc.args.clone(),
                                    },
                                })
                                .collect(),
                        )
                    } else {
                        ChatMessage::assistant(&msg.content)
                    }
                }
                MessageRole::System => ChatMessage::system(&msg.content),
                MessageRole::Tool => ChatMessage::tool(&msg.content),
            };
            messages.push(chat_msg);
        }

        messages
    }

    /// Build system prompt from template
    fn build_system_prompt(&self, user_context: &UserContext) -> String {
        const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("../prompts/system.txt");

        let is_gm = user_context.is_gm();
        let role_name = if is_gm { "Game Master" } else { "Player" };
        let character = user_context.character_id.as_deref().unwrap_or("None");

        SYSTEM_PROMPT_TEMPLATE
            .replace("{user_name}", &user_context.user_name)
            .replace("{role_name}", role_name)
            .replace("{character}", character)
    }

    /// Execute an internal tool
    async fn execute_internal_tool(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        match call.tool.as_str() {
            "document_search" => {
                let query = call
                    .args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let tags: Vec<String> = call
                    .args
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let limit = call
                    .args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;

                let filters = if tags.is_empty() {
                    None
                } else {
                    Some(SearchFilters {
                        tags,
                        tags_match: TagMatch::Any,
                    })
                };

                match self
                    .search
                    .search(query, user_context.role, limit, filters)
                    .await
                {
                    Ok(results) => {
                        let formatted = format_search_results_for_llm(&results, &self.i18n, "en");
                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({ "results": formatted }),
                        )
                    }
                    Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                }
            }
            "document_search_text" => {
                let query = call
                    .args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let section = call.args.get("section").and_then(|v| v.as_str());
                let document_id = call.args.get("document_id").and_then(|v| v.as_str());
                let limit = call
                    .args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;

                match self.db.search_chunks_fts(
                    query,
                    section,
                    document_id,
                    user_context.role,
                    limit,
                ) {
                    Ok(chunks) => {
                        let results: Vec<serde_json::Value> = chunks
                            .into_iter()
                            .map(|c| {
                                serde_json::json!({
                                    "document_id": c.document_id,
                                    "page_number": c.page_number,
                                    "section_title": c.section_title,
                                    "content": c.content,
                                })
                            })
                            .collect();

                        if results.is_empty() {
                            ToolResult::success(
                                call.id.clone(),
                                serde_json::json!({
                                    "results": [],
                                    "message": format!("No matches found for '{}'", query)
                                }),
                            )
                        } else {
                            ToolResult::success(
                                call.id.clone(),
                                serde_json::json!({ "results": results }),
                            )
                        }
                    }
                    Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                }
            }
            "document_get" => {
                let doc_id = call
                    .args
                    .get("document_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let page_number = call
                    .args
                    .get("page")
                    .and_then(|v| v.as_i64())
                    .map(|p| p as i32);

                if let Some(page) = page_number {
                    // Get all chunks for the specified page
                    match self.db.get_chunks_by_page(doc_id, page, user_context.role) {
                        Ok(chunks) => {
                            if chunks.is_empty() {
                                ToolResult::error(
                                    call.id.clone(),
                                    format!(
                                        "No content found for page {} of document {}",
                                        page, doc_id
                                    ),
                                )
                            } else {
                                // Concatenate all chunk content for the page
                                let page_content: String = chunks
                                    .iter()
                                    .map(|c| c.content.as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n\n");

                                ToolResult::success(
                                    call.id.clone(),
                                    serde_json::json!({
                                        "document_id": doc_id,
                                        "page": page,
                                        "content": page_content,
                                        "chunk_count": chunks.len()
                                    }),
                                )
                            }
                        }
                        Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                    }
                } else {
                    // No page specified - return document metadata
                    match self.db.get_document(doc_id) {
                        Ok(Some(doc)) => {
                            if doc.access_level.accessible_by(user_context.role) {
                                ToolResult::success(
                                    call.id.clone(),
                                    serde_json::json!({
                                        "id": doc.id,
                                        "title": doc.title,
                                        "tags": doc.tags,
                                        "chunk_count": doc.chunk_count,
                                        "image_count": doc.image_count,
                                        "note": "Use the 'page' parameter to retrieve content from a specific page"
                                    }),
                                )
                            } else {
                                ToolResult::error(call.id.clone(), "Access denied".to_string())
                            }
                        }
                        Ok(None) => {
                            ToolResult::error(call.id.clone(), "Document not found".to_string())
                        }
                        Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                    }
                }
            }
            "document_list" => {
                let tags: Vec<String> = call
                    .args
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                match self.db.list_documents(Some(user_context.role)) {
                    Ok(docs) => {
                        // Filter by tags if specified
                        let filtered: Vec<_> = if tags.is_empty() {
                            docs
                        } else {
                            docs.into_iter()
                                .filter(|d| tags.iter().any(|t| d.tags.contains(t)))
                                .collect()
                        };

                        // Return simplified list with just id, title, tags
                        let doc_list: Vec<serde_json::Value> = filtered
                            .into_iter()
                            .map(|d| {
                                serde_json::json!({
                                    "id": d.id,
                                    "title": d.title,
                                    "tags": d.tags,
                                    "chunk_count": d.chunk_count,
                                    "image_count": d.image_count
                                })
                            })
                            .collect();

                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({ "documents": doc_list }),
                        )
                    }
                    Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                }
            }
            "document_find" => {
                let title_query = call
                    .args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                match self.db.list_documents(Some(user_context.role)) {
                    Ok(docs) => {
                        let query_lower = title_query.to_lowercase();
                        let matches: Vec<serde_json::Value> = docs
                            .into_iter()
                            .filter(|d| d.title.to_lowercase().contains(&query_lower))
                            .map(|d| {
                                serde_json::json!({
                                    "id": d.id,
                                    "title": d.title,
                                    "tags": d.tags,
                                    "chunk_count": d.chunk_count,
                                    "image_count": d.image_count
                                })
                            })
                            .collect();

                        if matches.is_empty() {
                            ToolResult::success(
                                call.id.clone(),
                                serde_json::json!({
                                    "documents": [],
                                    "message": format!("No documents found matching '{}'", title_query)
                                }),
                            )
                        } else {
                            ToolResult::success(
                                call.id.clone(),
                                serde_json::json!({ "documents": matches }),
                            )
                        }
                    }
                    Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                }
            }
            "image_list" => {
                let doc_id = call
                    .args
                    .get("document_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let start_page = call
                    .args
                    .get("start_page")
                    .and_then(|v| v.as_i64())
                    .map(|p| p as i32);
                let end_page = call
                    .args
                    .get("end_page")
                    .and_then(|v| v.as_i64())
                    .map(|p| p as i32);
                let limit = call
                    .args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20) as usize;

                match self.db.list_document_images(
                    user_context.role,
                    Some(doc_id),
                    start_page,
                    end_page,
                    limit,
                ) {
                    Ok(images) => {
                        let image_list: Vec<_> = images
                            .into_iter()
                            .map(|img| {
                                serde_json::json!({
                                    "id": img.image.id,
                                    "page_number": img.image.page_number,
                                    "image_index": img.image.image_index,
                                    "width": img.image.width,
                                    "height": img.image.height,
                                    "description": img.image.description
                                })
                            })
                            .collect();

                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({ "images": image_list }),
                        )
                    }
                    Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                }
            }
            "image_search" => {
                let query = call
                    .args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let doc_id = call.args.get("document_id").and_then(|v| v.as_str());
                let limit = call
                    .args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;

                // Generate embedding for the query
                match self.search.embed_text(query).await {
                    Ok(embedding) => {
                        match self.db.search_images(&embedding, user_context.role, limit) {
                            Ok(results) => {
                                // Filter by document_id if specified
                                let filtered: Vec<_> = results
                                    .into_iter()
                                    .filter(|(img, _)| {
                                        doc_id.is_none_or(|d| img.image.document_id == d)
                                    })
                                    .map(|(img, score)| {
                                        serde_json::json!({
                                            "id": img.image.id,
                                            "document_id": img.image.document_id,
                                            "document_title": img.document_title,
                                            "page_number": img.image.page_number,
                                            "image_index": img.image.image_index,
                                            "description": img.image.description,
                                            "similarity": score
                                        })
                                    })
                                    .collect();

                                ToolResult::success(
                                    call.id.clone(),
                                    serde_json::json!({ "images": filtered }),
                                )
                            }
                            Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                        }
                    }
                    Err(e) => ToolResult::error(
                        call.id.clone(),
                        format!("Failed to generate embedding: {}", e),
                    ),
                }
            }
            "image_get" => {
                let image_id = call
                    .args
                    .get("image_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                match self.db.get_document_image(image_id) {
                    Ok(Some(img)) => {
                        if img.access_level.accessible_by(user_context.role) {
                            ToolResult::success(
                                call.id.clone(),
                                serde_json::json!({
                                    "id": img.image.id,
                                    "document_id": img.image.document_id,
                                    "document_title": img.document_title,
                                    "page_number": img.image.page_number,
                                    "image_index": img.image.image_index,
                                    "width": img.image.width,
                                    "height": img.image.height,
                                    "description": img.image.description
                                }),
                            )
                        } else {
                            ToolResult::error(call.id.clone(), "Access denied".to_string())
                        }
                    }
                    Ok(None) => ToolResult::error(call.id.clone(), "Image not found".to_string()),
                    Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
                }
            }
            "image_deliver" => {
                let image_id = call
                    .args
                    .get("image_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let target_path = call
                    .args
                    .get("target_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Get the image
                let img = match self.db.get_document_image(image_id) {
                    Ok(Some(img)) => {
                        if !img.access_level.accessible_by(user_context.role) {
                            return ToolResult::error(call.id.clone(), "Access denied".to_string());
                        }
                        img
                    }
                    Ok(None) => {
                        return ToolResult::error(call.id.clone(), "Image not found".to_string());
                    }
                    Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
                };

                // Determine the path relative to the FVTT assets directory
                // This is used for filesystem operations (joining with assets_dir)
                let relative_path = target_path.unwrap_or_else(|| {
                    IngestionService::fvtt_image_path(
                        &img.document_title,
                        img.image.page_number,
                        img.image.description.as_deref(),
                    )
                    .to_string_lossy()
                    .to_string()
                });

                // The FVTT path is what FVTT uses to reference the file (prepend assets/)
                let fvtt_path = format!("assets/{}", relative_path);

                // Check assets access mode
                match self.runtime_config.static_config.fvtt.check_assets_access() {
                    AssetsAccess::Direct(assets_dir) => {
                        // Create target directory
                        let full_path = assets_dir.join(&relative_path);
                        if let Some(parent) = full_path.parent()
                            && let Err(e) = std::fs::create_dir_all(parent)
                        {
                            return ToolResult::error(
                                call.id.clone(),
                                format!("Failed to create directory: {}", e),
                            );
                        }

                        // Copy file
                        if let Err(e) = std::fs::copy(&img.image.internal_path, &full_path) {
                            return ToolResult::error(
                                call.id.clone(),
                                format!("Failed to copy image: {}", e),
                            );
                        }

                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "success": true,
                                "mode": "direct",
                                "fvtt_path": fvtt_path,
                                "message": format!("Image delivered to FVTT assets at {}", fvtt_path)
                            }),
                        )
                    }
                    AssetsAccess::Shuttle => {
                        // Cannot directly deliver, return info for client to handle
                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "success": false,
                                "mode": "shuttle",
                                "image_id": image_id,
                                "suggested_path": fvtt_path,
                                "message": "Direct delivery not available. Use the FVTT module to fetch and deliver this image."
                            }),
                        )
                    }
                }
            }
            "system_schema" => {
                // Return a placeholder schema - in reality this would come from FVTT
                let schema = serde_json::json!({
                    "system": "mgt2e",
                    "actorTypes": ["traveller", "npc", "creature", "spacecraft", "vehicle", "world"],
                    "itemTypes": ["weapon", "armour", "skill", "term", "equipment"],
                    "note": "For detailed schema, query the FVTT client directly"
                });
                ToolResult::success(call.id.clone(), schema)
            }
            "traveller_uwp_parse" => {
                let uwp = call.args.get("uwp").and_then(|v| v.as_str()).unwrap_or("");
                let tool = TravellerTool::ParseUwp {
                    uwp: uwp.to_string(),
                };

                match tool.execute() {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_jump_calc" => {
                let distance = call
                    .args
                    .get("distance_parsecs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u8;
                let rating = call
                    .args
                    .get("ship_jump_rating")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u8;
                let tonnage = call
                    .args
                    .get("ship_tonnage")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100) as u32;

                let tool = TravellerTool::JumpCalculation {
                    distance_parsecs: distance,
                    ship_jump_rating: rating,
                    ship_tonnage: tonnage,
                };

                match tool.execute() {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_skill_lookup" => {
                let skill = call
                    .args
                    .get("skill_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let speciality = call
                    .args
                    .get("speciality")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let tool = TravellerTool::SkillLookup {
                    skill_name: skill.to_string(),
                    speciality,
                };

                match tool.execute() {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            // Traveller Map API tools
            "traveller_map_search" => {
                let query = call
                    .args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let milieu = call.args.get("milieu").and_then(|v| v.as_str());
                let tool = TravellerMapTool::Search {
                    query: query.to_string(),
                    milieu: milieu.map(|s| s.to_string()),
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_jump_worlds" => {
                let sector = call
                    .args
                    .get("sector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
                let jump = call.args.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
                let tool = TravellerMapTool::JumpWorlds {
                    sector: sector.to_string(),
                    hex: hex.to_string(),
                    jump,
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_route" => {
                let start = call
                    .args
                    .get("start")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let end = call.args.get("end").and_then(|v| v.as_str()).unwrap_or("");
                let jump = call.args.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
                let wild = call
                    .args
                    .get("wild")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let imperium_only = call
                    .args
                    .get("imperium_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let no_red_zones = call
                    .args
                    .get("no_red_zones")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let tool = TravellerMapTool::Route {
                    start: start.to_string(),
                    end: end.to_string(),
                    jump,
                    wild,
                    imperium_only,
                    no_red_zones,
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_world_data" => {
                let sector = call
                    .args
                    .get("sector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
                let tool = TravellerMapTool::WorldData {
                    sector: sector.to_string(),
                    hex: hex.to_string(),
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_sector_data" => {
                let sector = call
                    .args
                    .get("sector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let subsector = call.args.get("subsector").and_then(|v| v.as_str());
                let tool = TravellerMapTool::SectorData {
                    sector: sector.to_string(),
                    subsector: subsector.map(|s| s.to_string()),
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_coordinates" => {
                let sector = call
                    .args
                    .get("sector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let hex = call.args.get("hex").and_then(|v| v.as_str());
                let tool = TravellerMapTool::Coordinates {
                    sector: sector.to_string(),
                    hex: hex.map(|s| s.to_string()),
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_list_sectors" => {
                let milieu = call.args.get("milieu").and_then(|v| v.as_str());
                let tool = TravellerMapTool::ListSectors {
                    milieu: milieu.map(|s| s.to_string()),
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_poster_url" => {
                let sector = call
                    .args
                    .get("sector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let subsector = call.args.get("subsector").and_then(|v| v.as_str());
                let style = call.args.get("style").and_then(|v| v.as_str());
                let tool = TravellerMapTool::PosterUrl {
                    sector: sector.to_string(),
                    subsector: subsector.map(|s| s.to_string()),
                    style: style.map(|s| s.to_string()),
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_jump_map_url" => {
                let sector = call
                    .args
                    .get("sector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
                let jump = call.args.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
                let style = call.args.get("style").and_then(|v| v.as_str());
                let tool = TravellerMapTool::JumpMapUrl {
                    sector: sector.to_string(),
                    hex: hex.to_string(),
                    jump,
                    style: style.map(|s| s.to_string()),
                };
                match tool.execute(&self.traveller_map_client).await {
                    Ok(result) => ToolResult::success(call.id.clone(), result),
                    Err(e) => ToolResult::error(call.id.clone(), e),
                }
            }
            "traveller_map_save_poster" => {
                use crate::tools::traveller_map::PosterOptions;

                let sector = call
                    .args
                    .get("sector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let subsector = call.args.get("subsector").and_then(|v| v.as_str());
                let style = call.args.get("style").and_then(|v| v.as_str());
                let scale = call
                    .args
                    .get("scale")
                    .and_then(|v| v.as_u64())
                    .map(|s| s as u32);
                let target_folder = call.args.get("target_folder").and_then(|v| v.as_str());

                let options = PosterOptions {
                    subsector: subsector.map(|s| s.to_string()),
                    style: style.map(|s| s.to_string()),
                    scale,
                    ..Default::default()
                };

                // Download the image
                let (bytes, extension) = match self
                    .traveller_map_client
                    .download_poster(sector, &options)
                    .await
                {
                    Ok(result) => result,
                    Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
                };

                // Generate filename
                let filename = if let Some(ss) = subsector {
                    format!(
                        "{}-{}.{}",
                        sanitize_map_filename(sector),
                        sanitize_map_filename(ss),
                        extension
                    )
                } else {
                    format!("{}.{}", sanitize_map_filename(sector), extension)
                };

                // Determine relative path for FVTT assets
                let folder = target_folder.unwrap_or("traveller-maps");
                let relative_path = format!("{}/{}", folder, filename);
                let fvtt_path = format!("assets/{}", relative_path);

                // Check assets access mode
                match self.runtime_config.static_config.fvtt.check_assets_access() {
                    AssetsAccess::Direct(assets_dir) => {
                        // Create target directory
                        let full_path = assets_dir.join(&relative_path);
                        if let Some(parent) = full_path.parent()
                            && let Err(e) = std::fs::create_dir_all(parent)
                        {
                            return ToolResult::error(
                                call.id.clone(),
                                format!("Failed to create directory: {}", e),
                            );
                        }

                        // Write file
                        if let Err(e) = std::fs::write(&full_path, &bytes) {
                            return ToolResult::error(
                                call.id.clone(),
                                format!("Failed to write image: {}", e),
                            );
                        }

                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "success": true,
                                "mode": "direct",
                                "fvtt_path": fvtt_path,
                                "filename": filename,
                                "size_bytes": bytes.len(),
                                "message": format!("Sector map saved to FVTT assets at {}", fvtt_path)
                            }),
                        )
                    }
                    AssetsAccess::Shuttle => {
                        // Return base64-encoded data for client to handle
                        let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "success": false,
                                "mode": "shuttle",
                                "suggested_path": fvtt_path,
                                "filename": filename,
                                "extension": extension,
                                "size_bytes": bytes.len(),
                                "base64_data": base64_data,
                                "message": "Direct delivery not available. Use the FVTT module to save this image."
                            }),
                        )
                    }
                }
            }
            "traveller_map_save_jump_map" => {
                use crate::tools::traveller_map::JumpMapOptions;

                let sector = call
                    .args
                    .get("sector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let hex = call.args.get("hex").and_then(|v| v.as_str()).unwrap_or("");
                let jump = call.args.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
                let style = call.args.get("style").and_then(|v| v.as_str());
                let scale = call
                    .args
                    .get("scale")
                    .and_then(|v| v.as_u64())
                    .map(|s| s as u32);
                let target_folder = call.args.get("target_folder").and_then(|v| v.as_str());

                let options = JumpMapOptions {
                    style: style.map(|s| s.to_string()),
                    scale,
                    ..Default::default()
                };

                // Download the image
                let (bytes, extension) = match self
                    .traveller_map_client
                    .download_jump_map(sector, hex, jump, &options)
                    .await
                {
                    Ok(result) => result,
                    Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
                };

                // Generate filename
                let filename = format!(
                    "{}-{}-jump{}.{}",
                    sanitize_map_filename(sector),
                    hex,
                    jump,
                    extension
                );

                // Determine relative path for FVTT assets
                let folder = target_folder.unwrap_or("traveller-maps");
                let relative_path = format!("{}/{}", folder, filename);
                let fvtt_path = format!("assets/{}", relative_path);

                // Check assets access mode
                match self.runtime_config.static_config.fvtt.check_assets_access() {
                    AssetsAccess::Direct(assets_dir) => {
                        // Create target directory
                        let full_path = assets_dir.join(&relative_path);
                        if let Some(parent) = full_path.parent()
                            && let Err(e) = std::fs::create_dir_all(parent)
                        {
                            return ToolResult::error(
                                call.id.clone(),
                                format!("Failed to create directory: {}", e),
                            );
                        }

                        // Write file
                        if let Err(e) = std::fs::write(&full_path, &bytes) {
                            return ToolResult::error(
                                call.id.clone(),
                                format!("Failed to write image: {}", e),
                            );
                        }

                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "success": true,
                                "mode": "direct",
                                "fvtt_path": fvtt_path,
                                "filename": filename,
                                "size_bytes": bytes.len(),
                                "message": format!("Jump map saved to FVTT assets at {}", fvtt_path)
                            }),
                        )
                    }
                    AssetsAccess::Shuttle => {
                        // Return base64-encoded data for client to handle
                        let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "success": false,
                                "mode": "shuttle",
                                "suggested_path": fvtt_path,
                                "filename": filename,
                                "extension": extension,
                                "size_bytes": bytes.len(),
                                "base64_data": base64_data,
                                "message": "Direct delivery not available. Use the FVTT module to save this image."
                            }),
                        )
                    }
                }
            }
            _ => ToolResult::error(
                call.id.clone(),
                format!("Unknown internal tool: {}", call.tool),
            ),
        }
    }

    // === WebSocket Chat Methods ===

    /// Start a chat session via WebSocket
    #[allow(clippy::too_many_arguments)]
    pub async fn start_chat_ws(
        &self,
        session_id: String,
        conversation_id: Option<String>,
        message: String,
        model: Option<String>,
        enabled_tools: Option<Vec<String>>,
        user_id: String,
        user_name: Option<String>,
        role: u8,
        ws_manager: Arc<WebSocketManager>,
    ) -> String {
        let conversation_id = conversation_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        // Create or load conversation
        let mut conversation = self
            .db
            .get_conversation(&conversation_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| Conversation {
                id: conversation_id.clone(),
                user_id: user_id.clone(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                messages: vec![],
                metadata: Some(ConversationMetadata::default()),
            });

        // Add user message to conversation
        conversation.messages.push(ConversationMessage {
            role: MessageRole::User,
            content: message.clone(),
            timestamp: Utc::now(),
            tool_calls: None,
            tool_results: None,
        });

        // Build user context
        let user_context = UserContext {
            user_id,
            user_name: user_name.unwrap_or_default(),
            role,
            owned_actor_ids: vec![],
            character_id: None,
        };

        // Create active request
        let active_request = ActiveRequest {
            user_context: user_context.clone(),
            messages: conversation.messages.clone(),
            tool_calls_made: 0,
            pending_external_tool: None,
            paused: false,
            started_at: Instant::now(),
            ws_session_id: Some(session_id.clone()),
        };

        self.active_requests
            .insert(conversation_id.clone(), active_request);

        // Spawn the agentic loop for WebSocket
        let service = self.clone_for_task();
        let conv_id = conversation_id.clone();
        let session = session_id.clone();

        tokio::spawn(async move {
            service
                .run_agentic_loop_ws(
                    conv_id,
                    user_context,
                    model,
                    enabled_tools,
                    session,
                    ws_manager,
                )
                .await;
        });

        conversation_id
    }

    /// Handle external tool result for WebSocket chat
    pub async fn handle_tool_result_ws(
        &self,
        conversation_id: &str,
        tool_call_id: &str,
        result: serde_json::Value,
    ) {
        debug!(
            conversation_id = %conversation_id,
            tool_call_id = %tool_call_id,
            result_preview = %format!("{:.200}", result.to_string()),
            "External tool result received via WebSocket"
        );

        // Get pending tool info and validate
        let pending_info: Option<(String, serde_json::Value)> = {
            let entry = match self.active_requests.get(conversation_id) {
                Some(e) => e,
                None => {
                    warn!(
                        conversation_id = %conversation_id,
                        "Tool result for unknown conversation"
                    );
                    return;
                }
            };

            // Verify the tool call ID matches
            if let Some(ref pending) = entry.pending_external_tool {
                if pending.id != tool_call_id {
                    warn!(
                        conversation_id = %conversation_id,
                        expected = %pending.id,
                        got = %tool_call_id,
                        "Tool call ID mismatch"
                    );
                    return;
                }
                Some((pending.tool.clone(), pending.args.clone()))
            } else {
                warn!(
                    conversation_id = %conversation_id,
                    "No pending external tool"
                );
                return;
            }
        };

        // Process the result - special handling for image_describe (two-phase tool)
        let (tool_name, tool_args) = pending_info.unwrap();
        let final_result = if tool_name == "image_describe" {
            self.process_image_describe_result(&result, &tool_args)
                .await
        } else {
            result.clone()
        };

        // Add tool result to messages
        {
            let mut entry = match self.active_requests.get_mut(conversation_id) {
                Some(e) => e,
                None => return,
            };

            entry.messages.push(ConversationMessage {
                role: MessageRole::Tool,
                content: serde_json::to_string(&final_result).unwrap_or_default(),
                timestamp: Utc::now(),
                tool_calls: None,
                tool_results: Some(vec![ToolResultRecord {
                    tool_call_id: tool_call_id.to_string(),
                    result: final_result.clone(),
                    error: None,
                }]),
            });

            // Clear pending tool
            entry.pending_external_tool = None;
        }

        // Send the result through the oneshot channel to unblock the waiting loop
        if let Some((_, sender)) = self.tool_result_senders.remove(conversation_id) {
            let _ = sender.send(final_result);
        }
    }

    /// Process image_describe tool result - fetch from cache or call vision model
    async fn process_image_describe_result(
        &self,
        raw_result: &serde_json::Value,
        tool_args: &serde_json::Value,
    ) -> serde_json::Value {
        // Check for error in FVTT response
        if let Some(error) = raw_result.get("error") {
            return serde_json::json!({
                "error": error
            });
        }

        let image_path = raw_result
            .get("image_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let force_refresh = tool_args
            .get("force_refresh")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let context = tool_args.get("context").and_then(|v| v.as_str());

        // Check cache first (unless force_refresh)
        if !force_refresh {
            match self.db.get_fvtt_image_description(image_path, "data") {
                Ok(Some(cached)) => {
                    debug!(
                        image_path = %image_path,
                        "Returning cached image description"
                    );
                    return serde_json::json!({
                        "image_path": image_path,
                        "description": cached.description,
                        "cached": true,
                        "width": cached.width,
                        "height": cached.height
                    });
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(error = %e, "Failed to check image description cache");
                }
            }
        }

        // Get vision model from FVTT response
        let vision_model = match raw_result.get("vision_model").and_then(|v| v.as_str()) {
            Some(model) if !model.is_empty() => model,
            _ => {
                return serde_json::json!({
                    "error": "No vision model configured in FVTT settings"
                });
            }
        };

        // Get image data from FVTT response
        let image_data = match raw_result.get("image_data").and_then(|v| v.as_str()) {
            Some(data) => data,
            None => {
                return serde_json::json!({
                    "error": "No image data in FVTT response"
                });
            }
        };

        // Build prompt
        let prompt = if let Some(ctx) = context {
            format!(
                "Describe this image in detail for use in a tabletop RPG. Context: {}",
                ctx
            )
        } else {
            "Describe this image in detail for use in a tabletop RPG. \
             Focus on what the image depicts (characters, creatures, locations, items, maps, etc.) \
             and any text visible in the image. Be concise but descriptive."
                .to_string()
        };

        // Call vision model
        let message = ChatMessage::user_with_image(&prompt, image_data.to_string());
        let description = match self
            .ollama
            .generate_simple(vision_model, vec![message])
            .await
        {
            Ok(desc) => desc,
            Err(e) => {
                error!(error = %e, "Failed to call vision model");
                return serde_json::json!({
                    "error": format!("Vision model error: {}", e)
                });
            }
        };

        // Cache the result (using UUID since upsert handles conflicts via unique constraint)
        let cache_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        if let Err(e) = self
            .db
            .upsert_fvtt_image_description(&FvttImageDescription {
                id: cache_id,
                image_path: image_path.to_string(),
                source: "data".to_string(),
                description: description.clone(),
                embedding: None, // Could generate for semantic search later
                vision_model: vision_model.to_string(),
                width: None,
                height: None,
                created_at: now,
                updated_at: now,
            })
        {
            warn!(error = %e, "Failed to cache image description");
        }

        debug!(
            image_path = %image_path,
            vision_model = %vision_model,
            "Generated and cached new image description"
        );

        serde_json::json!({
            "image_path": image_path,
            "description": description,
            "cached": false
        })
    }

    /// Continue a paused WebSocket chat
    pub async fn continue_chat_ws(&self, conversation_id: &str) {
        debug!(conversation_id = %conversation_id, "Continuing paused WebSocket chat");

        // Update the active request to mark it as not paused
        if let Some(mut entry) = self.active_requests.get_mut(conversation_id) {
            entry.paused = false;
        }

        // Send continue signal through the oneshot channel
        if let Some((_, sender)) = self.continue_senders.remove(conversation_id) {
            let _ = sender.send(());
        }
    }

    /// Cancel an active WebSocket chat
    pub async fn cancel_chat_ws(&self, conversation_id: &str) {
        debug!(conversation_id = %conversation_id, "Cancelling WebSocket chat");

        // Remove the active request - the loop will detect this and stop
        self.active_requests.remove(conversation_id);

        // Clean up any pending channels
        self.tool_result_senders.remove(conversation_id);
        self.continue_senders.remove(conversation_id);
    }

    /// Execute an external tool for an MCP request via WebSocket GM connection
    ///
    /// Routes the tool call through an available GM WebSocket connection and waits
    /// for the result with the specified timeout.
    ///
    /// Returns `Ok(result)` on success, `Err(error_message)` on failure.
    pub async fn execute_external_tool_mcp(
        &self,
        tool: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        // Find an available GM connection
        let session_id = self
            .ws_manager
            .get_any_gm_connection()
            .ok_or_else(|| "No GM connection available to execute FVTT tools".to_string())?;

        // Generate unique request ID for this MCP tool call
        let request_id = format!("mcp:{}", Uuid::new_v4());
        let tool_call_id = format!("tc_{}", Uuid::new_v4().simple());

        debug!(
            request_id = %request_id,
            tool = %tool,
            session_id = %session_id,
            "Routing MCP external tool call to GM WebSocket"
        );

        // Create oneshot channel for the result
        let (tx, rx) = oneshot::channel();
        self.mcp_tool_result_senders.insert(request_id.clone(), tx);

        // Send tool call to GM client
        // Use request_id as conversation_id so client routes result back correctly
        self.ws_manager.send_to(
            &session_id,
            ServerMessage::ChatToolCall {
                conversation_id: request_id.clone(),
                id: tool_call_id.clone(),
                tool: tool.to_string(),
                args,
            },
        );

        // Wait for result with timeout
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => {
                debug!(request_id = %request_id, "MCP tool result received");
                Ok(result)
            }
            Ok(Err(_)) => {
                warn!(request_id = %request_id, "MCP tool channel closed");
                self.mcp_tool_result_senders.remove(&request_id);
                Err("GM client disconnected while processing tool".to_string())
            }
            Err(_) => {
                warn!(request_id = %request_id, tool = %tool, "MCP tool call timed out");
                self.mcp_tool_result_senders.remove(&request_id);
                Err(format!("Tool '{}' timed out", tool))
            }
        }
    }

    /// Handle a tool result for an MCP request
    ///
    /// Called when a GM WebSocket client sends back a tool result for an MCP-initiated
    /// tool call (identified by conversation_id starting with "mcp:").
    pub async fn handle_mcp_tool_result(
        &self,
        request_id: &str,
        _tool_call_id: &str,
        result: serde_json::Value,
    ) {
        debug!(
            request_id = %request_id,
            result_preview = %format!("{:.200}", result.to_string()),
            "MCP external tool result received"
        );

        // Send result through oneshot channel
        if let Some((_, sender)) = self.mcp_tool_result_senders.remove(request_id) {
            let _ = sender.send(result);
        } else {
            warn!(request_id = %request_id, "No pending MCP tool call for result");
        }
    }

    /// Upload a document and enqueue it for processing
    ///
    /// This method saves the file and creates a document record with "processing"
    /// status. The document processing worker will pick it up and process it.
    /// Clients should poll the document status for completion.
    pub async fn upload_document(
        &self,
        content: &[u8],
        filename: &str,
        title: &str,
        access_level: AccessLevel,
        tags: Vec<String>,
        vision_model: Option<String>,
    ) -> ServiceResult<Document> {
        // Check file size
        let max_size = self.runtime_config.dynamic().limits.max_document_size_bytes;
        if content.len() as u64 > max_size {
            return Err(ServiceError::Processing(
                crate::error::ProcessingError::FileTooLarge {
                    size: content.len() as u64,
                    max: max_size,
                },
            ));
        }

        // Generate document ID
        let doc_id = uuid::Uuid::new_v4().to_string();

        // Save file to permanent storage immediately
        let docs_dir = self
            .runtime_config
            .static_config
            .storage
            .data_dir
            .join("documents");
        std::fs::create_dir_all(&docs_dir)
            .map_err(|e| ServiceError::Processing(crate::error::ProcessingError::Io(e)))?;

        let permanent_path = docs_dir.join(format!("{}_{}", doc_id, filename));
        std::fs::write(&permanent_path, content)
            .map_err(|e| ServiceError::Processing(crate::error::ProcessingError::Io(e)))?;

        // Store vision model in metadata if provided
        let metadata = vision_model.map(|vm| serde_json::json!({ "vision_model": vm }));

        // Create document record with "processing" status
        let now = chrono::Utc::now();
        let document = Document {
            id: doc_id.clone(),
            title: title.to_string(),
            file_path: Some(permanent_path.to_string_lossy().to_string()),
            file_hash: None,
            access_level,
            tags: tags.clone(),
            metadata,
            processing_status: ProcessingStatus::Processing,
            processing_error: None,
            chunk_count: 0,
            image_count: 0,
            processing_phase: Some("queued".to_string()),
            processing_progress: None,
            processing_total: None,
            captioning_status: CaptioningStatus::NotRequested,
            captioning_error: None,
            captioning_progress: None,
            captioning_total: None,
            created_at: now,
            updated_at: now,
        };

        // Save document to database (enqueue for processing)
        self.db.insert_document(&document)?;

        info!(
            doc_id = %doc_id,
            title = %title,
            "Document uploaded and queued for processing"
        );

        Ok(document)
    }

    /// Start the document processing worker
    /// This should be called once on server startup
    pub fn start_document_processing_worker(service: Arc<SeneschalService>) {
        tokio::spawn(async move {
            info!("Document processing worker started");
            loop {
                // Check for pending documents
                match service.db.get_next_pending_document() {
                    Ok(Some(doc)) => {
                        info!(doc_id = %doc.id, title = %doc.title, "Processing queued document");
                        service.process_document(&doc).await;
                    }
                    Ok(None) => {
                        // No pending documents, sleep before checking again
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to check for pending documents");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });
    }

    /// Start the image captioning worker
    /// This runs as a separate background task to caption document images without blocking document processing
    pub fn start_captioning_worker(service: Arc<SeneschalService>) {
        tokio::spawn(async move {
            info!("Image captioning worker started");
            loop {
                // Check for documents pending captioning
                match service.db.get_next_pending_captioning_document() {
                    Ok(Some(doc)) => {
                        info!(doc_id = %doc.id, title = %doc.title, "Captioning images for document");
                        service.caption_document_images(&doc).await;
                    }
                    Ok(None) => {
                        // No pending captioning, sleep before checking again
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to check for documents pending captioning");
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    }
                }
            }
        });
    }

    /// Broadcast captioning progress via WebSocket
    fn broadcast_captioning_progress(
        &self,
        document_id: &str,
        status: &str,
        progress: Option<usize>,
        total: Option<usize>,
        error: Option<&str>,
    ) {
        self.ws_manager
            .broadcast_captioning_update(crate::websocket::CaptioningProgressUpdate {
                document_id: document_id.to_string(),
                status: status.to_string(),
                progress,
                total,
                error: error.map(String::from),
            });
    }

    /// Caption images for a single document (called by the captioning worker)
    /// This method is resumable - it only captions images without descriptions
    async fn caption_document_images(&self, document: &Document) {
        let doc_id = &document.id;
        let title = &document.title;

        let file_path = match &document.file_path {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                error!(doc_id = %doc_id, "Document has no file path for captioning");
                let _ = self.db.update_captioning_status(
                    doc_id,
                    CaptioningStatus::Failed,
                    Some("Document has no file path"),
                );
                self.broadcast_captioning_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    Some("Document has no file path"),
                );
                return;
            }
        };

        // Extract vision model from metadata
        let vision_model = match document
            .metadata
            .as_ref()
            .and_then(|m| m.get("vision_model"))
            .and_then(|v| v.as_str())
        {
            Some(model) => model.to_string(),
            None => {
                error!(doc_id = %doc_id, "Document has no vision model specified");
                let _ = self.db.update_captioning_status(
                    doc_id,
                    CaptioningStatus::Failed,
                    Some("No vision model specified"),
                );
                self.broadcast_captioning_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    Some("No vision model specified"),
                );
                return;
            }
        };

        // Get images that need captioning
        let images_to_caption = match self.db.get_images_without_descriptions(doc_id) {
            Ok(images) => images,
            Err(e) => {
                error!(doc_id = %doc_id, error = %e, "Failed to get images for captioning");
                let error_msg = format!("Failed to query images: {}", e);
                let _ = self.db.update_captioning_status(
                    doc_id,
                    CaptioningStatus::Failed,
                    Some(&error_msg),
                );
                self.broadcast_captioning_progress(doc_id, "failed", None, None, Some(&error_msg));
                return;
            }
        };

        if images_to_caption.is_empty() {
            // All images already captioned
            let _ = self
                .db
                .update_captioning_status(doc_id, CaptioningStatus::Completed, None);
            let _ = self.db.clear_captioning_progress(doc_id);
            self.broadcast_captioning_progress(doc_id, "completed", None, None, None);
            info!(doc_id = %doc_id, "All images already captioned");
            return;
        }

        // Mark as in_progress in database BEFORE starting work
        // This prevents the document from being picked up again if the worker restarts
        let _ = self
            .db
            .update_captioning_status(doc_id, CaptioningStatus::InProgress, None);

        let total_images = self
            .db
            .get_image_count(doc_id)
            .unwrap_or(images_to_caption.len());
        let already_captioned = total_images - images_to_caption.len();

        // Update database with initial progress and broadcast
        let _ = self
            .db
            .update_captioning_progress(doc_id, already_captioned, total_images);
        self.broadcast_captioning_progress(
            doc_id,
            "in_progress",
            Some(already_captioned),
            Some(total_images),
            None,
        );

        info!(
            doc_id = %doc_id,
            remaining = images_to_caption.len(),
            already_captioned = already_captioned,
            total = total_images,
            model = %vision_model,
            "Captioning remaining images"
        );

        // Extract page text for context (all unique pages from images to caption)
        let unique_pages: std::collections::HashSet<i32> = images_to_caption
            .iter()
            .flat_map(|img| {
                img.source_pages
                    .clone()
                    .unwrap_or_else(|| vec![img.page_number])
            })
            .collect();

        let page_list: Vec<i32> = unique_pages.into_iter().collect();
        let page_texts = match self.ingestion.extract_pdf_page_text(&file_path, &page_list) {
            Ok(texts) => {
                debug!(
                    doc_id = %doc_id,
                    pages = texts.len(),
                    "Extracted page text for image captioning context"
                );
                texts
            }
            Err(e) => {
                warn!(
                    doc_id = %doc_id,
                    error = %e,
                    "Failed to extract page text, captioning without context"
                );
                std::collections::HashMap::new()
            }
        };

        // Caption each image
        for (i, image) in images_to_caption.iter().enumerate() {
            let current_progress = already_captioned + i + 1;
            let _ = self
                .db
                .update_captioning_progress(doc_id, current_progress, total_images);
            self.broadcast_captioning_progress(
                doc_id,
                "in_progress",
                Some(current_progress),
                Some(total_images),
                None,
            );

            debug!(
                doc_id = %doc_id,
                image_id = %image.id,
                progress = current_progress,
                total = total_images,
                "Captioning image"
            );

            // Build page context for this image
            let mut source_pages = image
                .source_pages
                .clone()
                .unwrap_or_else(|| vec![image.page_number]);
            source_pages.sort();
            let context: String = source_pages
                .iter()
                .filter_map(|p| {
                    page_texts
                        .get(p)
                        .map(|t| format!("--- Page {} ---\n{}", p, t))
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            let page_context = if context.is_empty() {
                None
            } else {
                Some(context.as_str())
            };

            let image_path = std::path::Path::new(&image.internal_path);
            match self
                .caption_image(image_path, &vision_model, title, page_context)
                .await
            {
                Ok(Some(description)) => {
                    if let Err(e) = self.db.update_image_description(&image.id, &description) {
                        warn!(
                            image_id = %image.id,
                            error = %e,
                            "Failed to update image description"
                        );
                    } else {
                        // Generate and store embedding for the description
                        match self.search.embed_text(&description).await {
                            Ok(embedding) => {
                                if let Err(e) =
                                    self.db.insert_image_embedding(&image.id, &embedding)
                                {
                                    warn!(
                                        image_id = %image.id,
                                        error = %e,
                                        "Failed to store image embedding"
                                    );
                                }
                            }
                            Err(e) => {
                                warn!(
                                    image_id = %image.id,
                                    error = %e,
                                    "Failed to generate image embedding"
                                );
                            }
                        }
                        debug!(
                            image_id = %image.id,
                            description_len = description.len(),
                            "Image captioned successfully"
                        );
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        image_id = %image.id,
                        error = %e,
                        "Failed to caption image"
                    );
                }
            }
        }

        // Mark captioning as complete
        let _ = self
            .db
            .update_captioning_status(doc_id, CaptioningStatus::Completed, None);
        let _ = self.db.clear_captioning_progress(doc_id);
        self.broadcast_captioning_progress(doc_id, "completed", None, None, None);

        info!(doc_id = %doc_id, "Image captioning complete");
    }

    /// Broadcast document processing progress via WebSocket
    fn broadcast_document_progress(
        &self,
        document_id: &str,
        status: &str,
        phase: Option<&str>,
        progress: Option<usize>,
        total: Option<usize>,
        error: Option<&str>,
    ) {
        // Compute counts dynamically from the database
        let chunk_count = self.db.get_chunk_count(document_id).unwrap_or(0);
        let image_count = self.db.get_image_count(document_id).unwrap_or(0);

        self.ws_manager
            .broadcast_document_update(DocumentProgressUpdate {
                document_id: document_id.to_string(),
                status: status.to_string(),
                phase: phase.map(String::from),
                progress,
                total,
                error: error.map(String::from),
                chunk_count,
                image_count,
            });
    }

    /// Process a single document (called by the worker)
    /// This method is resumable - it checks what's already been done and continues from there.
    async fn process_document(&self, document: &Document) {
        let doc_id = &document.id;
        let title = &document.title;

        let file_path = match &document.file_path {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                error!(doc_id = %doc_id, "Document has no file path");
                let _ = self.db.update_document_processing_status(
                    doc_id,
                    ProcessingStatus::Failed,
                    Some("Document has no file path"),
                );
                self.broadcast_document_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    None,
                    Some("Document has no file path"),
                );
                return;
            }
        };

        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();

        // Extract vision model from metadata if present
        let vision_model = document
            .metadata
            .as_ref()
            .and_then(|m| m.get("vision_model"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        info!(doc_id = %doc_id, "Resuming/starting document processing");

        // Step 1: Check if chunks exist, if not extract text and create chunks
        let existing_chunk_count = self.db.get_chunk_count(doc_id).unwrap_or(0);
        if existing_chunk_count == 0 {
            info!(doc_id = %doc_id, "Extracting text and creating chunks");
            let _ = self.db.update_document_progress(doc_id, "chunking", 0, 1);
            self.broadcast_document_progress(
                doc_id,
                "processing",
                Some("chunking"),
                Some(0),
                Some(1),
                None,
            );

            let chunks = match self.ingestion.process_document_with_id(
                &file_path,
                doc_id,
                title,
                document.access_level,
                document.tags.clone(),
            ) {
                Ok(chunks) => chunks,
                Err(e) => {
                    error!(doc_id = %doc_id, error = %e, "Document text extraction failed");
                    let _ = self.db.update_document_processing_status(
                        doc_id,
                        ProcessingStatus::Failed,
                        Some(&e.to_string()),
                    );
                    self.broadcast_document_progress(
                        doc_id,
                        "failed",
                        None,
                        None,
                        None,
                        Some(&e.to_string()),
                    );
                    return;
                }
            };

            // Save chunks
            for chunk in &chunks {
                if let Err(e) = self.db.insert_chunk(chunk) {
                    warn!(chunk_id = %chunk.id, error = %e, "Failed to save chunk");
                }
            }

            info!(doc_id = %doc_id, chunks = chunks.len(), "Chunks created");
        } else {
            info!(doc_id = %doc_id, chunks = existing_chunk_count, "Chunks already exist, skipping text extraction");
        }

        // Step 2: Index chunks that don't have embeddings yet
        let chunks_to_embed = match self.db.get_chunks_without_embeddings(doc_id) {
            Ok(chunks) => chunks,
            Err(e) => {
                error!(doc_id = %doc_id, error = %e, "Failed to get chunks without embeddings");
                let error_msg = format!("Failed to query chunks: {}", e);
                let _ = self.db.update_document_processing_status(
                    doc_id,
                    ProcessingStatus::Failed,
                    Some(&error_msg),
                );
                self.broadcast_document_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    None,
                    Some(&error_msg),
                );
                return;
            }
        };

        if !chunks_to_embed.is_empty() {
            let total_chunks = self.db.get_chunk_count(doc_id).unwrap_or(0);
            let already_embedded = total_chunks - chunks_to_embed.len();
            info!(
                doc_id = %doc_id,
                remaining = chunks_to_embed.len(),
                already_embedded = already_embedded,
                total = total_chunks,
                "Generating embeddings for remaining chunks"
            );
            let _ = self.db.update_document_progress(
                doc_id,
                "embedding",
                already_embedded,
                total_chunks,
            );
            self.broadcast_document_progress(
                doc_id,
                "processing",
                Some("embedding"),
                Some(already_embedded),
                Some(total_chunks),
                None,
            );

            // Clone Arc references for use in progress callback
            let db_for_progress = Arc::clone(&self.db);
            let ws_manager_for_progress = Arc::clone(&self.ws_manager);
            let doc_id_for_progress = doc_id.to_string();

            let result = self
                .search
                .index_chunks_with_progress(&chunks_to_embed, |progress, _total| {
                    let current = already_embedded + progress;
                    let _ = db_for_progress.update_document_progress(
                        &doc_id_for_progress,
                        "embedding",
                        current,
                        total_chunks,
                    );

                    // Broadcast progress update
                    let chunk_count = db_for_progress
                        .get_chunk_count(&doc_id_for_progress)
                        .unwrap_or(0);
                    let image_count = db_for_progress
                        .get_image_count(&doc_id_for_progress)
                        .unwrap_or(0);
                    ws_manager_for_progress.broadcast_document_update(DocumentProgressUpdate {
                        document_id: doc_id_for_progress.clone(),
                        status: "processing".to_string(),
                        phase: Some("embedding".to_string()),
                        progress: Some(current),
                        total: Some(total_chunks),
                        error: None,
                        chunk_count,
                        image_count,
                    });
                })
                .await;

            if let Err(e) = result {
                error!(doc_id = %doc_id, error = %e, "Failed to index chunks");
                let error_msg = format!("Embedding generation failed: {}", e);
                let _ = self.db.update_document_processing_status(
                    doc_id,
                    ProcessingStatus::Failed,
                    Some(&error_msg),
                );
                self.broadcast_document_progress(
                    doc_id,
                    "failed",
                    None,
                    None,
                    None,
                    Some(&error_msg),
                );
                return;
            }
        } else {
            info!(doc_id = %doc_id, "All chunks already have embeddings");
        }

        // Step 3: Extract images from PDFs if not already done
        let extension = std::path::Path::new(&filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if extension == "pdf" {
            // Check if images already exist
            let existing_images = self.db.get_document_images(doc_id).unwrap_or_default();
            let mut image_count = existing_images.len();

            if image_count == 0 {
                info!(doc_id = %doc_id, "Extracting images from PDF");
                let _ = self
                    .db
                    .update_document_progress(doc_id, "extracting_images", 0, 1);
                self.broadcast_document_progress(
                    doc_id,
                    "processing",
                    Some("extracting_images"),
                    Some(0),
                    Some(1),
                    None,
                );
                match self.ingestion.extract_pdf_images(&file_path, doc_id) {
                    Ok(images) => {
                        image_count = images.len();
                        for image in &images {
                            if let Err(e) = self.db.insert_document_image(image) {
                                warn!(
                                    image_id = %image.id,
                                    error = %format_error_chain_ref(&e),
                                    "Failed to save document image to database"
                                );
                            }
                        }
                        info!(doc_id = %doc_id, images = image_count, "Images extracted");
                    }
                    Err(e) => {
                        warn!(doc_id = %doc_id, error = %format_error_chain_ref(&e), "Failed to extract images from PDF");
                    }
                }
            } else {
                info!(doc_id = %doc_id, images = image_count, "Images already exist, skipping extraction");
            }

            // Queue for captioning if vision model is specified and there are images to caption
            if vision_model.is_some() && image_count > 0 {
                let images_to_caption = self
                    .db
                    .get_images_without_descriptions(doc_id)
                    .unwrap_or_default();

                if !images_to_caption.is_empty() {
                    info!(
                        doc_id = %doc_id,
                        images = images_to_caption.len(),
                        "Queueing document for image captioning"
                    );
                    let _ = self.db.set_captioning_pending(doc_id);
                } else {
                    info!(doc_id = %doc_id, "All images already captioned");
                }
            }
        }

        // Update document with final counts and status
        let total_chunks = self.db.get_chunk_count(doc_id).unwrap_or(0);
        let total_images = self
            .db
            .get_document_images(doc_id)
            .map(|i| i.len())
            .unwrap_or(0);
        let _ = self.db.clear_document_progress(doc_id);
        let _ =
            self.db
                .update_document_processing_status(doc_id, ProcessingStatus::Completed, None);

        // Broadcast completion
        self.broadcast_document_progress(doc_id, "completed", None, None, None, None);

        info!(
            doc_id = %doc_id,
            title = %title,
            chunks = total_chunks,
            images = total_images,
            "Document processing complete"
        );
    }

    /// List documents
    pub fn list_documents(&self, user_role: u8) -> ServiceResult<Vec<Document>> {
        self.db.list_documents(Some(user_role))
    }

    /// Delete a document
    pub fn delete_document(&self, document_id: &str) -> ServiceResult<bool> {
        self.db.delete_document(document_id)
    }

    /// Update document details (title, access_level, tags)
    pub fn update_document(
        &self,
        document_id: &str,
        title: &str,
        access_level: crate::tools::AccessLevel,
        tags: Vec<String>,
    ) -> ServiceResult<bool> {
        self.db
            .update_document(document_id, title, access_level, tags)
    }

    /// Get images for a document
    pub fn get_document_images(
        &self,
        document_id: &str,
    ) -> ServiceResult<Vec<crate::db::DocumentImage>> {
        self.db.get_document_images(document_id)
    }

    /// Delete all images for a document
    pub fn delete_document_images(&self, document_id: &str) -> ServiceResult<usize> {
        // Get paths and delete from database
        let paths = self.db.delete_document_images(document_id)?;
        let count = paths.len();

        // Delete the image files
        for path in paths {
            if let Err(e) = std::fs::remove_file(&path) {
                warn!(path = %path, error = %e, "Failed to delete image file");
            }
        }

        // Try to remove the images directory for this document
        let images_dir = self
            .runtime_config
            .static_config
            .storage
            .data_dir
            .join("images")
            .join(document_id);
        let _ = std::fs::remove_dir(&images_dir); // Ignore error if not empty or doesn't exist

        info!(document_id = %document_id, count = count, "Deleted document images");
        Ok(count)
    }

    /// Delete a single image by ID
    pub fn delete_image(&self, image_id: &str) -> ServiceResult<bool> {
        // Get path and delete from database
        let result = self.db.delete_image(image_id)?;

        if let Some((path, document_id)) = result {
            // Delete the image file
            if let Err(e) = std::fs::remove_file(&path) {
                warn!(path = %path, error = %e, "Failed to delete image file");
            }

            info!(image_id = %image_id, document_id = %document_id, "Deleted image");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Re-extract images from a document
    /// This queues the document for reprocessing - the actual extraction happens in the background worker
    pub fn reextract_document_images(
        &self,
        document_id: &str,
        vision_model: Option<String>,
    ) -> ServiceResult<()> {
        // Get the document to validate it exists and is a PDF
        let document =
            self.db
                .get_document(document_id)?
                .ok_or_else(|| ServiceError::DocumentNotFound {
                    document_id: document_id.to_string(),
                })?;

        // Get the file path
        let file_path =
            document
                .file_path
                .as_ref()
                .ok_or_else(|| ServiceError::InvalidRequest {
                    message:
                        "Document has no associated file. Re-upload the document to extract images."
                            .to_string(),
                })?;

        // Check if it's a PDF
        let extension = std::path::Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if extension != "pdf" {
            return Err(ServiceError::InvalidRequest {
                message: "Image extraction is only supported for PDF documents".to_string(),
            });
        }

        let doc_path = std::path::Path::new(file_path);
        if !doc_path.exists() {
            return Err(ServiceError::InvalidRequest {
                message:
                    "Original document file not found. Re-upload the document to extract images."
                        .to_string(),
            });
        }

        // Delete existing images first
        self.delete_document_images(document_id)?;

        // Update metadata with vision model if provided
        if vision_model.is_some() {
            let metadata = serde_json::json!({ "vision_model": vision_model });
            let _ = self
                .db
                .update_document_metadata(document_id, Some(metadata));
        }

        // Queue for processing by setting status back to "processing"
        // The worker will skip chunking/embedding (already done) and extract images
        let _ = self
            .db
            .update_document_progress(document_id, "extracting_images", 0, 1);
        let _ = self.db.update_document_processing_status(
            document_id,
            ProcessingStatus::Processing,
            None,
        );

        info!(document_id = %document_id, "Queued document for image re-extraction");
        Ok(())
    }

    /// Caption an image using the specified vision model
    ///
    /// # Arguments
    /// * `image_path` - Path to the image file
    /// * `vision_model` - Name of the vision model to use
    /// * `document_title` - Title of the document containing the image
    /// * `page_context` - Optional text content from the page(s) where the image appears
    pub async fn caption_image(
        &self,
        image_path: &std::path::Path,
        vision_model: &str,
        document_title: &str,
        page_context: Option<&str>,
    ) -> ServiceResult<Option<String>> {
        // Read and encode image as base64
        let image_data = std::fs::read(image_path)
            .map_err(|e| ServiceError::Processing(crate::error::ProcessingError::Io(e)))?;
        let image_base64 = base64::engine::general_purpose::STANDARD.encode(&image_data);

        // Build prompt with document title and optional page context
        // Page context goes at the end for easier truncation if needed
        let base_prompt = format!(
            "Describe this image from the tabletop RPG document \"{}\". \
            Focus on what the image depicts (characters, creatures, locations, items, maps, etc.) \
            and any text visible in the image. Be concise but descriptive. \
            This description will be used to help game masters find relevant images.",
            document_title
        );

        let prompt = if let Some(context) = page_context {
            if context.is_empty() {
                base_prompt
            } else {
                format!(
                    "{}\n\n\
                    The image appears on a page with the following text for additional context:\n\n{}",
                    base_prompt, context
                )
            }
        } else {
            base_prompt
        };

        let message = crate::ollama::ChatMessage::user_with_image(&prompt, image_base64);

        let description = self
            .ollama
            .generate_simple(vision_model, vec![message])
            .await?;

        Ok(Some(description))
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
    fn clone_for_task(&self) -> Self {
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
            tool_result_senders: self.tool_result_senders.clone(),
            continue_senders: self.continue_senders.clone(),
            mcp_tool_result_senders: self.mcp_tool_result_senders.clone(),
        }
    }
}
