//! Chat session management for WebSocket-based chat.

use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use tracing::{debug, error, warn};
use uuid::Uuid;

use crate::db::{
    Conversation, ConversationMessage, ConversationMetadata, FvttImageDescription, MessageRole,
    ToolResultRecord,
};
use crate::ollama::ChatMessage;
use crate::websocket::WebSocketManager;

use super::SeneschalService;
use super::state::{ActiveRequest, UserContext};

impl SeneschalService {
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
        if let Some((_, sender)) = self.tool_result_senders.remove(conversation_id)
            && sender.send(final_result).is_err()
        {
            debug!(conversation_id = %conversation_id, "Tool result channel closed - receiver likely timed out");
        }
    }

    /// Process image_describe tool result - fetch from cache or call vision model
    pub(crate) async fn process_image_describe_result(
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
        if let Some((_, sender)) = self.continue_senders.remove(conversation_id)
            && sender.send(()).is_err()
        {
            debug!(conversation_id = %conversation_id, "Continue signal channel closed - receiver likely timed out");
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
}
