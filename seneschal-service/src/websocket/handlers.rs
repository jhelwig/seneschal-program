//! WebSocket message handlers.
//!
//! Contains the logic for handling incoming WebSocket connections
//! and processing client messages.

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::service::SeneschalService;

use super::manager::WebSocketManager;
use super::messages::{ClientMessage, ServerMessage};

/// Handle a WebSocket connection
///
/// This function is called when a WebSocket connection is established.
/// It manages the connection lifecycle, processes incoming messages,
/// and forwards outgoing messages.
pub async fn handle_ws_connection(
    socket: WebSocket,
    ws_manager: Arc<WebSocketManager>,
    service: Arc<SeneschalService>,
) {
    let session_id = uuid::Uuid::new_v4().to_string();
    info!(session_id = %session_id, "New WebSocket connection");

    // Split the socket into sender and receiver
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Create a channel for sending messages to this connection
    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<ServerMessage>();

    // Add connection to manager
    ws_manager.add_connection(session_id.clone(), msg_tx);

    // Spawn task to forward messages from channel to WebSocket
    let session_id_clone = session_id.clone();
    let send_task = tokio::spawn(async move {
        while let Some(msg) = msg_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    if ws_tx.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to serialize WebSocket message");
                }
            }
        }
        debug!(session_id = %session_id_clone, "WebSocket send task ended");
    });

    // Process incoming messages
    let session_id_for_recv = session_id.clone();
    let ws_manager_for_recv = ws_manager.clone();
    let service_for_recv = service.clone();
    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(Message::Text(text)) => {
                handle_client_message(
                    &session_id_for_recv,
                    &text,
                    ws_manager_for_recv.clone(),
                    service_for_recv.clone(),
                )
                .await;
            }
            Ok(Message::Binary(data)) => {
                // Try to parse binary as JSON text
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    handle_client_message(
                        &session_id_for_recv,
                        &text,
                        ws_manager_for_recv.clone(),
                        service_for_recv.clone(),
                    )
                    .await;
                }
            }
            Ok(Message::Ping(data)) => {
                // axum handles pong automatically, but we can log it
                debug!(session_id = %session_id_for_recv, "Received ping: {:?}", data);
            }
            Ok(Message::Pong(_)) => {
                // Pong received - connection is alive
            }
            Ok(Message::Close(_)) => {
                info!(session_id = %session_id_for_recv, "WebSocket connection closed by client");
                break;
            }
            Err(e) => {
                error!(session_id = %session_id_for_recv, error = %e, "WebSocket error");
                break;
            }
        }
    }

    // Clean up
    ws_manager.remove_connection(&session_id);
    send_task.abort();
    info!(session_id = %session_id, "WebSocket connection closed");
}

/// Handle a client message
async fn handle_client_message(
    session_id: &str,
    text: &str,
    ws_manager: Arc<WebSocketManager>,
    service: Arc<SeneschalService>,
) {
    let msg: ClientMessage = match serde_json::from_str(text) {
        Ok(msg) => msg,
        Err(e) => {
            warn!(
                session_id = %session_id,
                error = %e,
                text = %text,
                "Failed to parse client message"
            );
            ws_manager.send_to(
                session_id,
                ServerMessage::Error {
                    code: "parse_error".to_string(),
                    message: format!("Failed to parse message: {}", e),
                    recoverable: true,
                },
            );
            return;
        }
    };

    match msg {
        ClientMessage::Auth {
            user_id,
            user_name,
            role,
            session_id: client_session_id,
        } => {
            debug!(
                session_id = %session_id,
                user_id = %user_id,
                user_name = %user_name,
                role = role,
                client_session_id = ?client_session_id,
                "Processing auth message"
            );

            // Authenticate the connection
            ws_manager.authenticate(session_id, user_id.clone(), user_name, role);

            // Send success response
            ws_manager.send_to(
                session_id,
                ServerMessage::AuthResponse {
                    success: true,
                    session_id: session_id.to_string(),
                    message: None,
                },
            );

            info!(
                session_id = %session_id,
                user_id = %user_id,
                "WebSocket connection authenticated"
            );
        }
        ClientMessage::Ping => {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            ws_manager.send_to(session_id, ServerMessage::Pong { timestamp });
        }
        ClientMessage::SubscribeDocuments => {
            ws_manager.set_document_subscription(session_id, true);
            debug!(session_id = %session_id, "Subscribed to document updates");
        }
        ClientMessage::UnsubscribeDocuments => {
            ws_manager.set_document_subscription(session_id, false);
            debug!(session_id = %session_id, "Unsubscribed from document updates");
        }
        ClientMessage::ChatMessage {
            conversation_id,
            message,
            model,
            enabled_tools,
        } => {
            // Check if connection is authenticated
            let conn_info = ws_manager.get_connection_info(session_id);
            let Some((user_id, user_name, role)) = conn_info else {
                ws_manager.send_to(
                    session_id,
                    ServerMessage::ChatError {
                        conversation_id: conversation_id.unwrap_or_default(),
                        message: "Not authenticated".to_string(),
                        recoverable: false,
                    },
                );
                return;
            };

            debug!(
                session_id = %session_id,
                user_id = %user_id,
                conversation_id = ?conversation_id,
                message_preview = %message.chars().take(100).collect::<String>(),
                "Starting WebSocket chat"
            );

            // Start the chat via service
            let conv_id = service
                .start_chat_ws(
                    session_id.to_string(),
                    conversation_id,
                    message,
                    model,
                    enabled_tools,
                    user_id,
                    user_name,
                    role,
                    ws_manager.clone(),
                )
                .await;

            // Send started acknowledgment
            ws_manager.send_to(
                session_id,
                ServerMessage::ChatStarted {
                    conversation_id: conv_id,
                },
            );
        }
        ClientMessage::ToolResult {
            conversation_id,
            tool_call_id,
            result,
        } => {
            debug!(
                session_id = %session_id,
                conversation_id = %conversation_id,
                tool_call_id = %tool_call_id,
                "Received tool result via WebSocket"
            );

            // Route based on conversation_id prefix
            if conversation_id.starts_with("mcp:") {
                // MCP tool result - route to MCP handler
                service
                    .handle_mcp_tool_result(&conversation_id, &tool_call_id, result)
                    .await;
            } else {
                // Regular WebSocket chat tool result
                service
                    .handle_tool_result_ws(&conversation_id, &tool_call_id, result)
                    .await;
            }
        }
        ClientMessage::ContinueChat { conversation_id } => {
            debug!(
                session_id = %session_id,
                conversation_id = %conversation_id,
                "Continuing paused chat"
            );

            service.continue_chat_ws(&conversation_id).await;
        }
        ClientMessage::CancelChat { conversation_id } => {
            debug!(
                session_id = %session_id,
                conversation_id = %conversation_id,
                "Cancelling chat"
            );

            service.cancel_chat_ws(&conversation_id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_deserialization() {
        let auth_json = r#"{"type":"auth","user_id":"user123","user_name":"Test User","role":4,"session_id":null}"#;
        let msg: ClientMessage = serde_json::from_str(auth_json).unwrap();
        match msg {
            ClientMessage::Auth {
                user_id,
                user_name,
                role,
                session_id,
            } => {
                assert_eq!(user_id, "user123");
                assert_eq!(user_name, "Test User");
                assert_eq!(role, 4);
                assert!(session_id.is_none());
            }
            _ => panic!("Expected Auth message"),
        }

        let ping_json = r#"{"type":"ping"}"#;
        let msg: ClientMessage = serde_json::from_str(ping_json).unwrap();
        assert!(matches!(msg, ClientMessage::Ping));

        let sub_json = r#"{"type":"subscribe_documents"}"#;
        let msg: ClientMessage = serde_json::from_str(sub_json).unwrap();
        assert!(matches!(msg, ClientMessage::SubscribeDocuments));

        let unsub_json = r#"{"type":"unsubscribe_documents"}"#;
        let msg: ClientMessage = serde_json::from_str(unsub_json).unwrap();
        assert!(matches!(msg, ClientMessage::UnsubscribeDocuments));

        // Chat messages
        let chat_json = r#"{"type":"chat_message","conversation_id":null,"message":"Hello","model":"llama3.2","enabled_tools":["search"]}"#;
        let msg: ClientMessage = serde_json::from_str(chat_json).unwrap();
        match msg {
            ClientMessage::ChatMessage {
                conversation_id,
                message,
                model,
                enabled_tools,
            } => {
                assert!(conversation_id.is_none());
                assert_eq!(message, "Hello");
                assert_eq!(model, Some("llama3.2".to_string()));
                assert_eq!(enabled_tools, Some(vec!["search".to_string()]));
            }
            _ => panic!("Expected ChatMessage"),
        }

        let tool_result_json = r#"{"type":"tool_result","conversation_id":"conv123","tool_call_id":"tc_0","result":{"success":true}}"#;
        let msg: ClientMessage = serde_json::from_str(tool_result_json).unwrap();
        match msg {
            ClientMessage::ToolResult {
                conversation_id,
                tool_call_id,
                result,
            } => {
                assert_eq!(conversation_id, "conv123");
                assert_eq!(tool_call_id, "tc_0");
                assert_eq!(result["success"], true);
            }
            _ => panic!("Expected ToolResult"),
        }

        let continue_json = r#"{"type":"continue_chat","conversation_id":"conv123"}"#;
        let msg: ClientMessage = serde_json::from_str(continue_json).unwrap();
        assert!(matches!(
            msg,
            ClientMessage::ContinueChat {
                conversation_id
            } if conversation_id == "conv123"
        ));

        let cancel_json = r#"{"type":"cancel_chat","conversation_id":"conv123"}"#;
        let msg: ClientMessage = serde_json::from_str(cancel_json).unwrap();
        assert!(matches!(
            msg,
            ClientMessage::CancelChat {
                conversation_id
            } if conversation_id == "conv123"
        ));
    }

    #[test]
    fn test_server_message_serialization() {
        let auth_response = ServerMessage::AuthResponse {
            success: true,
            session_id: "session123".to_string(),
            message: None,
        };
        let json = serde_json::to_string(&auth_response).unwrap();
        assert!(json.contains(r#""type":"auth_response""#));
        assert!(json.contains(r#""success":true"#));
        assert!(!json.contains("message")); // should be skipped when None

        let progress = ServerMessage::DocumentProgress {
            document_id: "doc123".to_string(),
            status: "processing".to_string(),
            phase: Some("embedding".to_string()),
            progress: Some(50),
            total: Some(100),
            error: None,
            chunk_count: 10,
            image_count: 5,
        };
        let json = serde_json::to_string(&progress).unwrap();
        assert!(json.contains(r#""type":"document_progress""#));
        assert!(json.contains(r#""document_id":"doc123""#));
        assert!(json.contains(r#""phase":"embedding""#));
        assert!(!json.contains("error")); // should be skipped when None

        // Chat messages
        let chat_started = ServerMessage::ChatStarted {
            conversation_id: "conv123".to_string(),
        };
        let json = serde_json::to_string(&chat_started).unwrap();
        assert!(json.contains(r#""type":"chat_started""#));
        assert!(json.contains(r#""conversation_id":"conv123""#));

        let chat_content = ServerMessage::ChatContent {
            conversation_id: "conv123".to_string(),
            text: "Hello world".to_string(),
        };
        let json = serde_json::to_string(&chat_content).unwrap();
        assert!(json.contains(r#""type":"chat_content""#));
        assert!(json.contains(r#""text":"Hello world""#));

        let tool_call = ServerMessage::ChatToolCall {
            conversation_id: "conv123".to_string(),
            id: "tc_0".to_string(),
            tool: "search".to_string(),
            args: serde_json::json!({"query": "test"}),
        };
        let json = serde_json::to_string(&tool_call).unwrap();
        assert!(json.contains(r#""type":"chat_tool_call""#));
        assert!(json.contains(r#""tool":"search""#));

        let turn_complete = ServerMessage::ChatTurnComplete {
            conversation_id: "conv123".to_string(),
            prompt_tokens: Some(100),
            completion_tokens: None,
        };
        let json = serde_json::to_string(&turn_complete).unwrap();
        assert!(json.contains(r#""type":"chat_turn_complete""#));
        assert!(json.contains(r#""prompt_tokens":100"#));
        assert!(!json.contains("completion_tokens")); // should be skipped when None

        let chat_error = ServerMessage::ChatError {
            conversation_id: "conv123".to_string(),
            message: "Something went wrong".to_string(),
            recoverable: false,
        };
        let json = serde_json::to_string(&chat_error).unwrap();
        assert!(json.contains(r#""type":"chat_error""#));
        assert!(json.contains(r#""recoverable":false"#));
    }
}
