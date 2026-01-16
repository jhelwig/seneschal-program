//! MCP message handlers.
//!
//! Handlers for initialize and tools/list requests.
//!
//! NOTE: Tool definitions are now managed by the unified registry in
//! `crate::tools::registry`. This module converts registry format to MCP format.

use super::{McpError, McpState, McpToolDefinition};
use crate::tools::REGISTRY;

/// Handle initialize request
pub async fn handle_initialize(_state: &McpState) -> Result<serde_json::Value, McpError> {
    Ok(serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": "seneschal-service",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Seneschal Program MCP server for game master assistance, document search, and Foundry VTT integration."
    }))
}

/// Handle tools/list request
///
/// This function retrieves tool definitions from the unified registry
/// and converts them to the MCP format.
pub async fn handle_tools_list(_state: &McpState) -> Result<serde_json::Value, McpError> {
    // Get MCP definitions from the unified registry
    let registry_tools = REGISTRY.mcp_definitions();

    // Convert from registry format to MCP module format
    let tools: Vec<McpToolDefinition> = registry_tools
        .into_iter()
        .map(|t| McpToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.input_schema,
            defer_loading: t.defer_loading,
            category: t.category,
        })
        .collect();

    Ok(serde_json::json!({ "tools": tools }))
}
