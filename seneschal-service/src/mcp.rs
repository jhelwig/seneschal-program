//! MCP (Model Context Protocol) server implementation.
//!
//! This module provides an MCP-compatible interface for external LLM tools
//! to interact with the Seneschal service.

use axum::{
    Json, Router,
    extract::State,
    response::{Sse, sse::Event},
    routing::get,
};
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

use crate::service::SeneschalService;

pub mod handlers;
pub mod tools;

use handlers::{handle_initialize, handle_tools_list};
use tools::handle_tool_call;

/// MCP server state
pub struct McpState {
    pub service: Arc<SeneschalService>,
}

/// Build the MCP router
pub fn mcp_router(service: Arc<SeneschalService>) -> Router {
    let state = Arc::new(McpState { service });

    Router::new()
        .route("/", get(mcp_sse_handler))
        .route("/messages", axum::routing::post(mcp_message_handler))
        .with_state(state)
}

/// MCP SSE handler - implements the MCP protocol over SSE
async fn mcp_sse_handler(
    State(_state): State<Arc<McpState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    info!("MCP client connected");

    // Send server info as first event
    let server_info = McpServerInfo {
        protocol_version: "2024-11-05".to_string(),
        capabilities: McpCapabilities {
            tools: Some(McpToolsCapability { list_changed: false }),
            resources: None,
            prompts: None,
        },
        server_info: McpImplementation {
            name: "seneschal-service".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        instructions: Some(
            "Seneschal Program MCP server for game master assistance, document search, and Foundry VTT integration.".to_string()
        ),
    };

    let info_json = serde_json::to_string(&McpMessage::ServerInfo(server_info)).unwrap_or_default();

    let stream = stream::once(async move { Ok::<_, Infallible>(Event::default().data(info_json)) });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("ping"),
    )
}

/// MCP message handler - handles JSON-RPC style requests
async fn mcp_message_handler(
    State(state): State<Arc<McpState>>,
    Json(request): Json<McpRequest>,
) -> Json<McpResponse> {
    debug!(method = %request.method, "MCP request received");

    let result = match request.method.as_str() {
        "initialize" => handle_initialize(&state).await,
        "tools/list" => handle_tools_list(&state).await,
        "tools/call" => handle_tool_call(&state, request.params).await,
        _ => Err(McpError {
            code: -32601,
            message: format!("Method not found: {}", request.method),
        }),
    };

    match result {
        Ok(data) => Json(McpResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(data),
            error: None,
        }),
        Err(error) => Json(McpResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(error),
        }),
    }
}

// === MCP Protocol Types ===

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct McpRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum McpMessage {
    ServerInfo(McpServerInfo),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpServerInfo {
    protocol_version: String,
    capabilities: McpCapabilities,
    server_info: McpImplementation,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
}

#[derive(Debug, Serialize)]
struct McpCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<McpToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resources: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompts: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpToolsCapability {
    list_changed: bool,
}

#[derive(Debug, Serialize)]
struct McpImplementation {
    name: String,
    version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}
