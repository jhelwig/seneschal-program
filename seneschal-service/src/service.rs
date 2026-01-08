use chrono::Utc;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::db::{
    Conversation, ConversationMessage, ConversationMetadata, Database, Document, MessageRole,
    ProcessingStatus, ToolCallRecord, ToolResultRecord,
};
use crate::error::{ServiceError, ServiceResult};
use crate::i18n::I18n;
use crate::ingestion::IngestionService;
use crate::ollama::{ChatMessage, ChatRequest, OllamaClient, StreamEvent};
use crate::search::{SearchResult, SearchService, format_search_results_for_llm};
use crate::tools::{
    AccessLevel, SearchFilters, TagMatch, ToolCall, ToolLocation, ToolResult, TravellerTool,
    classify_tool,
};

/// Main service coordinator
pub struct SeneschalService {
    pub config: AppConfig,
    pub db: Arc<Database>,
    pub ollama: Arc<OllamaClient>,
    pub search: Arc<SearchService>,
    pub ingestion: Arc<IngestionService>,
    pub i18n: Arc<I18n>,
    pub active_requests: Arc<DashMap<String, ActiveRequest>>,
}

impl SeneschalService {
    /// Create a new service instance
    pub async fn new(config: AppConfig) -> ServiceResult<Self> {
        info!("Initializing Seneschal Program service");

        // Ensure data directory exists
        std::fs::create_dir_all(&config.storage.data_dir).map_err(|e| ServiceError::Config {
            message: format!("Failed to create data directory: {}", e),
        })?;

        // Initialize database
        let db_path = config.storage.data_dir.join("seneschal.db");
        let db = Arc::new(Database::open(&db_path)?);
        info!(path = %db_path.display(), "Database initialized");

        // Initialize Ollama client
        let ollama = Arc::new(OllamaClient::new(config.ollama.clone())?);

        // Check Ollama availability
        if ollama.health_check().await? {
            info!(url = %config.ollama.base_url, "Ollama is available");
        } else {
            warn!(url = %config.ollama.base_url, "Ollama is not available");
        }

        // Initialize search service
        let search = Arc::new(
            SearchService::new(db.clone(), &config.embeddings, &config.ollama.base_url).await?,
        );

        // Initialize ingestion service
        let ingestion = Arc::new(IngestionService::new(
            &config.embeddings,
            config.storage.data_dir.clone(),
        ));

        // Initialize i18n
        let i18n = Arc::new(I18n::new());

        Ok(Self {
            config,
            db,
            ollama,
            search,
            ingestion,
            i18n,
            active_requests: Arc::new(DashMap::new()),
        })
    }

    /// Process a chat request
    pub async fn chat(&self, request: ChatApiRequest) -> ServiceResult<mpsc::Receiver<SSEEvent>> {
        let (tx, rx) = mpsc::channel(100);

        let conversation_id = request
            .conversation_id
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        // Create or load conversation
        let mut conversation = self
            .db
            .get_conversation(&conversation_id)?
            .unwrap_or_else(|| Conversation {
                id: conversation_id.clone(),
                user_id: request.user_context.user_id.clone(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                messages: vec![],
                metadata: Some(ConversationMetadata::default()),
            });

        // Add user messages to conversation
        for msg in &request.messages {
            let role = match msg.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                "system" => MessageRole::System,
                "tool" => MessageRole::Tool,
                _ => MessageRole::User,
            };
            conversation.messages.push(ConversationMessage {
                role,
                content: msg.content.clone(),
                timestamp: Utc::now(),
                tool_calls: None,
                tool_results: None,
            });
        }

        // Create active request
        let active_request = ActiveRequest {
            user_context: request.user_context.clone(),
            messages: conversation.messages.clone(),
            tool_calls_made: 0,
            pending_external_tool: None,
            paused: false,
            started_at: Instant::now(),
        };

        self.active_requests
            .insert(conversation_id.clone(), active_request);

        // Spawn the agentic loop
        let service = self.clone_for_task();
        let conv_id = conversation_id.clone();
        let user_ctx = request.user_context.clone();
        let model = request.model;
        let tools = request.tools;

        tokio::spawn(async move {
            service
                .run_agentic_loop(conv_id, user_ctx, model, tools, tx)
                .await;
        });

        Ok(rx)
    }

    /// Run the agentic loop
    async fn run_agentic_loop(
        &self,
        conversation_id: String,
        user_context: UserContext,
        model: Option<String>,
        enabled_tools: Option<Vec<String>>,
        tx: mpsc::Sender<SSEEvent>,
    ) {
        let loop_config = &self.config.agentic_loop;

        loop {
            // Check if we should stop
            let active_request = match self.active_requests.get(&conversation_id) {
                Some(r) => r.clone(),
                None => {
                    debug!("Active request not found, stopping loop");
                    break;
                }
            };

            // Check hard timeout
            if active_request.started_at.elapsed() > loop_config.hard_timeout() {
                let _ = tx
                    .send(SSEEvent::Error {
                        message: self.i18n.get("en", "error-timeout", None),
                        recoverable: false,
                    })
                    .await;
                break;
            }

            // Check pause conditions
            if active_request.tool_calls_made >= loop_config.tool_call_pause_threshold
                && !active_request.paused
            {
                self.active_requests
                    .entry(conversation_id.clone())
                    .and_modify(|r| r.paused = true);

                let _ = tx
                    .send(SSEEvent::Pause {
                        reason: PauseReason::ToolLimit,
                        tool_calls_made: active_request.tool_calls_made,
                        elapsed_seconds: active_request.started_at.elapsed().as_secs(),
                        message: self.i18n.format(
                            "en",
                            "chat-pause-tool-limit",
                            &[("count", &active_request.tool_calls_made.to_string())],
                        ),
                    })
                    .await;

                // Wait for continue signal or timeout
                // For now, we'll just stop. In a real implementation,
                // we'd wait for POST /api/chat/continue
                break;
            }

            if active_request.started_at.elapsed() > loop_config.time_pause_threshold()
                && !active_request.paused
            {
                self.active_requests
                    .entry(conversation_id.clone())
                    .and_modify(|r| r.paused = true);

                let elapsed = active_request.started_at.elapsed().as_secs();
                let _ = tx
                    .send(SSEEvent::Pause {
                        reason: PauseReason::TimeLimit,
                        tool_calls_made: active_request.tool_calls_made,
                        elapsed_seconds: elapsed,
                        message: self.i18n.format(
                            "en",
                            "chat-pause-time-limit",
                            &[("seconds", &elapsed.to_string())],
                        ),
                    })
                    .await;
                break;
            }

            // Send thinking indicator
            let _ = tx.send(SSEEvent::Thinking).await;

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
                num_ctx: None,
                enable_tools: tools_enabled,
            };

            let mut stream = match self.ollama.chat_stream(chat_request).await {
                Ok(s) => s,
                Err(e) => {
                    error!(error = %e, "Ollama chat failed");
                    let _ = tx
                        .send(SSEEvent::Error {
                            message: e.to_string(),
                            recoverable: false,
                        })
                        .await;
                    break;
                }
            };

            let mut accumulated_content = String::new();
            let mut tool_calls = Vec::new();
            let mut done = false;

            // Process stream events
            while let Some(event) = stream.recv().await {
                match event {
                    StreamEvent::Content(text) => {
                        accumulated_content.push_str(&text);
                        let _ = tx.send(SSEEvent::Content { text }).await;
                    }
                    StreamEvent::ToolCall(call) => {
                        tool_calls.push(call);
                    }
                    StreamEvent::Done {
                        prompt_eval_count,
                        eval_count,
                        ..
                    } => {
                        done = true;
                        // If we have tool calls, process them
                        if !tool_calls.is_empty() {
                            for call in &tool_calls {
                                let location = classify_tool(&call.tool);

                                match location {
                                    ToolLocation::Internal => {
                                        // Execute internal tool
                                        let _ = tx
                                            .send(SSEEvent::ToolStatus {
                                                message: self.i18n.format(
                                                    "en",
                                                    "chat-executing-tool",
                                                    &[("tool", &call.tool)],
                                                ),
                                            })
                                            .await;

                                        let result =
                                            self.execute_internal_tool(call, &user_context).await;

                                        // Add tool result to conversation
                                        self.active_requests.entry(conversation_id.clone()).and_modify(|r| {
                                            r.tool_calls_made += 1;
                                            r.messages.push(ConversationMessage {
                                                role: MessageRole::Tool,
                                                content: serde_json::to_string(&result).unwrap_or_default(),
                                                timestamp: Utc::now(),
                                                tool_calls: None,
                                                tool_results: Some(vec![ToolResultRecord {
                                                    tool_call_id: call.id.clone(),
                                                    result: match &result.outcome {
                                                        crate::tools::ToolOutcome::Success { result } => result.clone(),
                                                        crate::tools::ToolOutcome::Error { error } => serde_json::json!({ "error": error }),
                                                    },
                                                    error: match &result.outcome {
                                                        crate::tools::ToolOutcome::Error { error } => Some(error.clone()),
                                                        _ => None,
                                                    },
                                                }]),
                                            });
                                        });

                                        let _ = tx
                                            .send(SSEEvent::ToolResult {
                                                id: call.id.clone(),
                                                tool: call.tool.clone(),
                                                summary: self.i18n.format(
                                                    "en",
                                                    "chat-tool-complete",
                                                    &[("tool", &call.tool)],
                                                ),
                                            })
                                            .await;
                                    }
                                    ToolLocation::External => {
                                        // Request external tool execution from client
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

                                        let _ = tx
                                            .send(SSEEvent::ToolCall {
                                                id: call.id.clone(),
                                                tool: call.tool.clone(),
                                                args: call.args.clone(),
                                            })
                                            .await;

                                        // Wait for tool result (handled by API endpoint)
                                        // For now, we'll break and wait for the client
                                        // In a full implementation, we'd use channels
                                    }
                                }
                            }

                            // If we had tool calls, continue the loop
                            if !tool_calls
                                .iter()
                                .any(|c| classify_tool(&c.tool) == ToolLocation::External)
                            {
                                continue;
                            }
                        }

                        // No tool calls or external tools pending - we're done
                        if !accumulated_content.is_empty() {
                            // Add assistant message to conversation
                            self.active_requests
                                .entry(conversation_id.clone())
                                .and_modify(|r| {
                                    r.messages.push(ConversationMessage {
                                        role: MessageRole::Assistant,
                                        content: accumulated_content.clone(),
                                        timestamp: Utc::now(),
                                        tool_calls: if tool_calls.is_empty() {
                                            None
                                        } else {
                                            Some(
                                                tool_calls
                                                    .iter()
                                                    .map(|c| ToolCallRecord {
                                                        id: c.id.clone(),
                                                        tool: c.tool.clone(),
                                                        args: c.args.clone(),
                                                    })
                                                    .collect(),
                                            )
                                        },
                                        tool_results: None,
                                    });
                                });
                        }

                        let _ = tx
                            .send(SSEEvent::Done {
                                usage: Some(Usage {
                                    prompt_tokens: prompt_eval_count.unwrap_or(0),
                                    completion_tokens: eval_count.unwrap_or(0),
                                }),
                            })
                            .await;
                    }
                    StreamEvent::Error(e) => {
                        let _ = tx
                            .send(SSEEvent::Error {
                                message: e,
                                recoverable: true,
                            })
                            .await;
                        done = true;
                    }
                }

                if done {
                    break;
                }
            }

            // If we got content without tool calls, we're done
            if !accumulated_content.is_empty() && tool_calls.is_empty() {
                break;
            }

            // If we have pending external tools, check for timeout
            if let Some(req) = self.active_requests.get(&conversation_id)
                && let Some(ref pending) = req.pending_external_tool
            {
                if pending.sent_at.elapsed() > loop_config.external_tool_timeout() {
                    error!(
                        tool = %pending.tool,
                        args = %pending.args,
                        "External tool call timed out"
                    );
                    let _ = tx
                        .send(SSEEvent::Error {
                            message: format!(
                                "External tool '{}' timed out waiting for response",
                                pending.tool
                            ),
                            recoverable: false,
                        })
                        .await;
                    break;
                }
                // Wait for external tool result (will be provided via API)
                break;
            }

            // No more work to do
            if tool_calls.is_empty() {
                break;
            }
        }

        // Save conversation to database
        if let Some(req) = self.active_requests.get(&conversation_id) {
            let conversation = Conversation {
                id: conversation_id.clone(),
                user_id: req.user_context.user_id.clone(),
                created_at: Utc::now(), // Should be preserved from original
                updated_at: Utc::now(),
                messages: req.messages.clone(),
                metadata: Some(ConversationMetadata::default()),
            };

            if let Err(e) = self.db.upsert_conversation(&conversation) {
                error!(error = %e, "Failed to save conversation");
            }
        }

        // Only clean up active request if there's no pending external tool
        // If there's a pending external tool, the request will be cleaned up
        // after the tool result is processed
        let has_pending_tool = self
            .active_requests
            .get(&conversation_id)
            .is_some_and(|r| r.pending_external_tool.is_some());

        if !has_pending_tool {
            self.active_requests.remove(&conversation_id);
        }
    }

    /// Build Ollama messages from conversation
    fn build_ollama_messages(
        &self,
        request: &ActiveRequest,
        user_context: &UserContext,
    ) -> Vec<ChatMessage> {
        let mut messages = vec![ChatMessage::system(self.build_system_prompt(user_context))];

        for msg in &request.messages {
            let chat_msg = match msg.role {
                MessageRole::User => ChatMessage::user(&msg.content),
                MessageRole::Assistant => ChatMessage::assistant(&msg.content),
                MessageRole::System => ChatMessage::system(&msg.content),
                MessageRole::Tool => ChatMessage::tool(&msg.content),
            };
            messages.push(chat_msg);
        }

        messages
    }

    /// Build system prompt
    fn build_system_prompt(&self, user_context: &UserContext) -> String {
        let is_gm = user_context.is_gm();

        format!(
            r#"You are the Seneschal Program, an AI assistant for tabletop roleplaying game masters using Foundry VTT.

User Information:
- Name: {}
- Role: {} ({})
- Character: {}

Your capabilities:
1. Search and retrieve information from game rulebooks and documents
2. Read and modify Foundry VTT game data (actors, items, journals, etc.)
3. Roll dice using FVTT's dice system
4. Parse and explain game-specific data (like Traveller UWPs)

Guidelines:
- Be helpful and concise
- When referencing rules, cite the source (book/page if available)
- Respect the user's role - {} can only access what they have permission to see
- Use appropriate tools to look up information rather than guessing
- For Mongoose Traveller 2e: You understand UWP format, skills, characteristics, and game mechanics

When asked about rules or game content, use document_search to find relevant information before answering.
"#,
            user_context.user_name,
            user_context.role,
            if is_gm { "Game Master" } else { "Player" },
            user_context
                .character_id
                .as_ref()
                .unwrap_or(&"None".to_string()),
            if is_gm { "GMs" } else { "Players" }
        )
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
            "document_get" => {
                let doc_id = call
                    .args
                    .get("document_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                match self.db.get_chunk(doc_id) {
                    Ok(Some(chunk)) => {
                        if chunk.access_level.accessible_by(user_context.role) {
                            ToolResult::success(
                                call.id.clone(),
                                serde_json::to_value(&chunk).unwrap_or_default(),
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
            _ => ToolResult::error(
                call.id.clone(),
                format!("Unknown internal tool: {}", call.tool),
            ),
        }
    }

    /// Handle external tool result from client and continue the agentic loop
    pub async fn handle_tool_result(
        &self,
        conversation_id: &str,
        tool_call_id: &str,
        result: serde_json::Value,
    ) -> ServiceResult<mpsc::Receiver<SSEEvent>> {
        // Get the active request and validate
        let (user_context, model, enabled_tools) = {
            let mut entry = self
                .active_requests
                .get_mut(conversation_id)
                .ok_or_else(|| ServiceError::ConversationNotFound {
                    conversation_id: conversation_id.to_string(),
                })?;

            // Verify the tool call ID matches
            if let Some(ref pending) = entry.pending_external_tool {
                if pending.id != tool_call_id {
                    return Err(ServiceError::ToolCallNotFound {
                        tool_call_id: tool_call_id.to_string(),
                    });
                }
            } else {
                return Err(ServiceError::ToolCallNotFound {
                    tool_call_id: tool_call_id.to_string(),
                });
            }

            // Add tool result to messages
            entry.messages.push(ConversationMessage {
                role: MessageRole::Tool,
                content: serde_json::to_string(&result).unwrap_or_default(),
                timestamp: Utc::now(),
                tool_calls: None,
                tool_results: Some(vec![ToolResultRecord {
                    tool_call_id: tool_call_id.to_string(),
                    result,
                    error: None,
                }]),
            });

            // Clear pending tool
            entry.pending_external_tool = None;

            // Extract what we need to continue the loop
            (
                entry.user_context.clone(),
                None::<String>,
                None::<Vec<String>>,
            )
        };

        // Create a new channel for the continuation
        let (tx, rx) = mpsc::channel(100);

        // Spawn the continuation of the agentic loop
        let service = self.clone_for_task();
        let conv_id = conversation_id.to_string();

        tokio::spawn(async move {
            service
                .run_agentic_loop(conv_id, user_context, model, enabled_tools, tx)
                .await;
        });

        Ok(rx)
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
        if content.len() as u64 > self.config.limits.max_document_size_bytes {
            return Err(ServiceError::Processing(
                crate::error::ProcessingError::FileTooLarge {
                    size: content.len() as u64,
                    max: self.config.limits.max_document_size_bytes,
                },
            ));
        }

        // Generate document ID
        let doc_id = uuid::Uuid::new_v4().to_string();

        // Save file to permanent storage immediately
        let docs_dir = self.config.storage.data_dir.join("documents");
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
                let _ = self.db.update_document_processing_status(
                    doc_id,
                    ProcessingStatus::Failed,
                    Some(&format!("Failed to query chunks: {}", e)),
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

            if let Err(e) = self.search.index_chunks(&chunks_to_embed).await {
                error!(doc_id = %doc_id, error = %e, "Failed to index chunks");
                let _ = self.db.update_document_processing_status(
                    doc_id,
                    ProcessingStatus::Failed,
                    Some(&format!("Embedding generation failed: {}", e)),
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
                match self.ingestion.extract_pdf_images(&file_path, doc_id) {
                    Ok(images) => {
                        image_count = images.len();
                        for image in &images {
                            if let Err(e) = self.db.insert_document_image(image) {
                                warn!(
                                    image_id = %image.id,
                                    error = %e,
                                    "Failed to save document image to database"
                                );
                            }
                        }
                        info!(doc_id = %doc_id, images = image_count, "Images extracted");
                    }
                    Err(e) => {
                        warn!(doc_id = %doc_id, error = %e, "Failed to extract images from PDF");
                    }
                }
            } else {
                info!(doc_id = %doc_id, images = image_count, "Images already exist, skipping extraction");
            }

            // Step 4: Caption images that don't have descriptions yet
            if let Some(ref model) = vision_model {
                let images_to_caption = self
                    .db
                    .get_images_without_descriptions(doc_id)
                    .unwrap_or_default();

                if !images_to_caption.is_empty() {
                    let total_images = image_count;
                    let already_captioned = total_images - images_to_caption.len();
                    info!(
                        doc_id = %doc_id,
                        remaining = images_to_caption.len(),
                        already_captioned = already_captioned,
                        total = total_images,
                        model = %model,
                        "Captioning remaining images"
                    );

                    // Extract page text for context
                    // Collect all unique pages from images to caption
                    let unique_pages: std::collections::HashSet<i32> = images_to_caption
                        .iter()
                        .flat_map(|img| {
                            img.source_pages
                                .clone()
                                .unwrap_or_else(|| vec![img.page_number])
                        })
                        .collect();

                    let page_list: Vec<i32> = unique_pages.into_iter().collect();
                    let page_texts = match self
                        .ingestion
                        .extract_pdf_page_text(&file_path, &page_list)
                    {
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

                    for (i, image) in images_to_caption.iter().enumerate() {
                        let current_progress = already_captioned + i + 1;
                        let _ = self.db.update_document_progress(
                            doc_id,
                            "captioning",
                            current_progress,
                            total_images,
                        );
                        info!(
                            doc_id = %doc_id,
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
                            .caption_image(image_path, model, title, page_context)
                            .await
                        {
                            Ok(Some(description)) => {
                                if let Err(e) =
                                    self.db.update_image_description(&image.id, &description)
                                {
                                    warn!(
                                        image_id = %image.id,
                                        error = %e,
                                        "Failed to update image description"
                                    );
                                } else {
                                    // Generate and store embedding for the description
                                    match self.search.embed_text(&description).await {
                                        Ok(embedding) => {
                                            if let Err(e) = self
                                                .db
                                                .insert_image_embedding(&image.id, &embedding)
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

                    info!(doc_id = %doc_id, "Image captioning complete");
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
        let _ = self
            .db
            .update_document_counts(doc_id, total_chunks, total_images);
        let _ = self.db.clear_document_progress(doc_id);
        let _ =
            self.db
                .update_document_processing_status(doc_id, ProcessingStatus::Completed, None);

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
            .config
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
        let image_base64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image_data);

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
        let ttl = self.config.conversation.ttl();
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
            config: self.config.clone(),
            db: self.db.clone(),
            ollama: self.ollama.clone(),
            search: self.search.clone(),
            ingestion: self.ingestion.clone(),
            i18n: self.i18n.clone(),
            active_requests: self.active_requests.clone(),
        }
    }
}

/// User context from FVTT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserContext {
    pub user_id: String,
    pub user_name: String,
    pub role: u8, // CONST.USER_ROLES: 0=None, 1=Player, 2=Trusted, 3=Assistant, 4=GM
    #[serde(default)]
    pub owned_actor_ids: Vec<String>,
    pub character_id: Option<String>,
}

impl UserContext {
    pub fn is_gm(&self) -> bool {
        self.role >= 4
    }
}

/// Chat API request
#[derive(Debug, Clone, Deserialize)]
pub struct ChatApiRequest {
    pub model: Option<String>,
    pub messages: Vec<ApiMessage>,
    pub user_context: UserContext,
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub stream: bool,
}

/// API message
#[derive(Debug, Clone, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: String,
}

/// SSE Event types
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SSEEvent {
    /// Currently processing
    Thinking,
    /// Streaming text from LLM
    Content { text: String },
    /// External tool request
    ToolCall {
        id: String,
        tool: String,
        args: serde_json::Value,
    },
    /// Internal tool progress
    ToolStatus { message: String },
    /// Internal tool completed
    ToolResult {
        id: String,
        tool: String,
        summary: String,
    },
    /// Loop limit reached
    Pause {
        reason: PauseReason,
        tool_calls_made: u32,
        elapsed_seconds: u64,
        message: String,
    },
    /// Error occurred
    Error { message: String, recoverable: bool },
    /// Request completed
    Done { usage: Option<Usage> },
}

/// Pause reason
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    ToolLimit,
    TimeLimit,
}

/// Token usage
#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// Active request state (in-memory only)
#[derive(Debug, Clone)]
pub struct ActiveRequest {
    pub user_context: UserContext,
    pub messages: Vec<ConversationMessage>,
    pub tool_calls_made: u32,
    pub pending_external_tool: Option<PendingToolCall>,
    pub paused: bool,
    pub started_at: Instant,
}

/// Pending external tool call
#[derive(Debug, Clone)]
pub struct PendingToolCall {
    pub id: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub sent_at: Instant,
}
