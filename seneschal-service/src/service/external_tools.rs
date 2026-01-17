//! External tool execution via WebSocket for MCP requests.

use std::time::Duration;

use tracing::{debug, warn};
use uuid::Uuid;

use crate::websocket::ServerMessage;

use super::SeneschalService;

impl SeneschalService {
    /// Execute an external tool via a GM WebSocket connection (for MCP requests).
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
        use tokio::sync::oneshot;

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
            if sender.send(result).is_err() {
                debug!(request_id = %request_id, "MCP tool result channel closed - receiver likely timed out");
            }
        } else {
            warn!(request_id = %request_id, "No pending MCP tool call for result");
        }
    }
}
