//! External tool execution routing for MCP.

use super::super::{McpError, McpState};

/// Execute an external tool by routing through GM WebSocket connection.
///
/// Uses deduplication cache to prevent duplicate tool executions when
/// Claude Desktop retries requests. The session_id scopes deduplication
/// to a single MCP client session.
pub(super) async fn execute_external_tool(
    state: &McpState,
    name: &str,
    arguments: serde_json::Value,
    session_id: Option<&str>,
) -> Result<serde_json::Value, McpError> {
    // Generate dedup key from session ID, tool name and arguments
    let dedup_key = McpState::dedup_key(session_id, name, &arguments);

    // Check for cached result (duplicate request)
    if let Some(cached) = state.get_cached_result(dedup_key) {
        tracing::info!(
            tool = %name,
            session_id = ?session_id,
            dedup_key = %dedup_key,
            "Returning cached result for duplicate MCP tool call"
        );
        return Ok(cached);
    }

    // Get timeout from config
    let timeout = state
        .service
        .runtime_config
        .dynamic()
        .agentic_loop
        .external_tool_timeout();

    match state
        .service
        .execute_external_tool_mcp(name, arguments, timeout)
        .await
    {
        Ok(result) => {
            // Format result in MCP content format
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            let response = serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            });

            // Cache the result for deduplication
            state.cache_result(dedup_key, response.clone());

            // Periodically clean up expired entries (every ~100 calls on average)
            if rand::random::<u8>() < 3 {
                state.cleanup_expired_cache();
            }

            Ok(response)
        }
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}
