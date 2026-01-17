//! Agentic loop for LLM interaction with tool calling.

use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};

use crate::db::{
    Conversation, ConversationMessage, ConversationMetadata, MessageRole, ToolCallRecord,
    ToolResultRecord,
};
use crate::ollama::{ChatRequest, StreamEvent};
use crate::tools::{ToolLocation, ToolOutcome, classify_tool};
use crate::websocket::{ServerMessage, WebSocketManager};

use super::SeneschalService;
use super::state::{PendingToolCall, UserContext};

impl SeneschalService {
    /// Run the agentic loop with WebSocket output
    pub(crate) async fn run_agentic_loop_ws(
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
                                ToolOutcome::Success { result: res } => {
                                    debug!(
                                        conversation_id = %conversation_id,
                                        tool_call_id = %call.id,
                                        tool_name = %call.tool,
                                        result_preview = %format!("{:.200}", res.to_string()),
                                        "Internal tool execution succeeded"
                                    );
                                }
                                ToolOutcome::Error { error } => {
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
                                                ToolOutcome::Success { result } => result.clone(),
                                                ToolOutcome::Error { error } => {
                                                    serde_json::json!({ "error": error })
                                                }
                                            },
                                            error: match &result.outcome {
                                                ToolOutcome::Error { error } => Some(error.clone()),
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
}
