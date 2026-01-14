//! MCP (Model Context Protocol) server implementation.
//!
//! This module provides an MCP-compatible interface for external LLM tools
//! to interact with the Seneschal service. Implements the Streamable HTTP
//! transport from the 2025-03-26 specification.

use axum::body::Bytes;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response, Sse, sse::Event},
};
use dashmap::DashMap;
use futures::stream;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::convert::Infallible;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::service::SeneschalService;

pub mod handlers;
pub mod tools;

use handlers::{handle_initialize, handle_tools_list};
use tools::handle_tool_call;

/// Cached tool result with timestamp
pub struct CachedToolResult {
    pub result: serde_json::Value,
    pub created_at: Instant,
}

/// MCP server state
pub struct McpState {
    pub service: Arc<SeneschalService>,
    /// Cache for deduplicating tool calls (key: hash of tool+args, value: cached result)
    pub tool_dedup_cache: DashMap<u64, CachedToolResult>,
}

/// TTL for cached tool results (10 seconds)
pub const TOOL_DEDUP_TTL: Duration = Duration::from_secs(10);

impl McpState {
    /// Generate a dedup cache key from session ID, tool name and arguments
    ///
    /// Including session ID scopes deduplication to a single client, preventing
    /// accidental cross-client result sharing.
    pub fn dedup_key(session_id: Option<&str>, tool: &str, args: &serde_json::Value) -> u64 {
        let mut hasher = DefaultHasher::new();
        if let Some(sid) = session_id {
            sid.hash(&mut hasher);
        }
        tool.hash(&mut hasher);
        // Use canonical JSON string for consistent hashing
        let args_str = serde_json::to_string(args).unwrap_or_default();
        args_str.hash(&mut hasher);
        hasher.finish()
    }

    /// Check cache for a recent result
    pub fn get_cached_result(&self, key: u64) -> Option<serde_json::Value> {
        if let Some(entry) = self.tool_dedup_cache.get(&key)
            && entry.created_at.elapsed() < TOOL_DEDUP_TTL
        {
            return Some(entry.result.clone());
        }
        None
    }

    /// Store a result in the cache
    pub fn cache_result(&self, key: u64, result: serde_json::Value) {
        self.tool_dedup_cache.insert(
            key,
            CachedToolResult {
                result,
                created_at: Instant::now(),
            },
        );
    }

    /// Clean up expired cache entries (call periodically)
    pub fn cleanup_expired_cache(&self) {
        self.tool_dedup_cache
            .retain(|_, v| v.created_at.elapsed() < TOOL_DEDUP_TTL);
    }
}

/// Build the MCP router
///
/// The Streamable HTTP transport uses a single endpoint supporting both
/// GET (for SSE streams) and POST (for JSON-RPC messages).
///
/// Uses fallback to handle both `/mcp` and `/mcp/` paths when nested.
pub fn mcp_router(service: Arc<SeneschalService>) -> Router {
    let state = Arc::new(McpState {
        service,
        tool_dedup_cache: DashMap::new(),
    });

    // Use fallback to handle the root path regardless of trailing slash
    Router::new()
        .fallback(mcp_fallback_handler)
        .with_state(state)
}

/// Fallback handler that dispatches based on HTTP method
async fn mcp_fallback_handler(
    State(state): State<Arc<McpState>>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match method {
        Method::GET => mcp_get_handler(State(state), headers).await,
        Method::POST => {
            // Parse the JSON body
            match serde_json::from_slice::<McpRequest>(&body) {
                Ok(request) => mcp_post_handler(State(state), headers, request).await,
                Err(e) => {
                    warn!(error = %e, "Failed to parse MCP request");
                    let error_response = McpResponse {
                        jsonrpc: "2.0".to_string(),
                        id: serde_json::Value::Null,
                        result: None,
                        error: Some(McpError {
                            code: -32700,
                            message: format!("Parse error: {}", e),
                        }),
                    };
                    (StatusCode::BAD_REQUEST, Json(error_response)).into_response()
                }
            }
        }
        _ => (StatusCode::METHOD_NOT_ALLOWED, "Method not allowed").into_response(),
    }
}

/// Handle GET requests - opens SSE stream for server-initiated messages
///
/// Per the Streamable HTTP spec, GET opens an SSE stream for the server
/// to send notifications and requests to the client. Since we don't
/// currently have server-initiated messages, we keep the stream open
/// with keep-alive pings.
async fn mcp_get_handler(State(_state): State<Arc<McpState>>, headers: HeaderMap) -> Response {
    // Check Accept header
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !accept.contains("text/event-stream") {
        return (
            StatusCode::NOT_ACCEPTABLE,
            "Accept header must include text/event-stream",
        )
            .into_response();
    }

    info!("MCP SSE stream opened");

    // Create an empty stream that stays open via keep-alive
    let stream = stream::pending::<Result<Event, Infallible>>();

    Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(Duration::from_secs(30))
                .text(":ping"),
        )
        .into_response()
}

/// Handle POST requests - processes JSON-RPC messages
///
/// Per the Streamable HTTP spec, POST receives JSON-RPC requests and
/// returns responses. The response is always JSON for our implementation
/// since we don't need streaming responses for tool calls.
async fn mcp_post_handler(
    State(state): State<Arc<McpState>>,
    headers: HeaderMap,
    request: McpRequest,
) -> Response {
    debug!(method = %request.method, "MCP request received");

    // Extract session ID if provided
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let Some(ref sid) = session_id {
        debug!(session_id = %sid, "Request includes session ID");
    }

    let result = match request.method.as_str() {
        "initialize" => {
            info!("MCP client initializing");
            handle_initialize(&state).await
        }
        "notifications/initialized" => {
            // Client acknowledgment - no response needed
            debug!("MCP client initialized notification received");
            Ok(serde_json::json!({}))
        }
        "tools/list" => {
            debug!("MCP tools/list request");
            handle_tools_list(&state).await
        }
        "tools/call" => {
            debug!("MCP tools/call request");
            handle_tool_call(&state, request.params, session_id.as_deref()).await
        }
        "ping" => {
            debug!("MCP ping request");
            Ok(serde_json::json!({}))
        }
        _ => {
            warn!(method = %request.method, "Unknown MCP method");
            Err(McpError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
            })
        }
    };

    // Build response
    let response = match result {
        Ok(data) => McpResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(data),
            error: None,
        },
        Err(error) => McpResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(error),
        },
    };

    // For initialize requests, generate and include session ID
    let mut headers = HeaderMap::new();
    if request.method == "initialize" {
        let session_id = Uuid::new_v4().to_string();
        if let Ok(value) = session_id.parse() {
            headers.insert("mcp-session-id", value);
            debug!(session_id = %session_id, "Generated new MCP session");
        }
    }

    (StatusCode::OK, headers, Json(response)).into_response()
}

// === MCP Protocol Types ===

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct McpRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
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
#[serde(rename_all = "camelCase")]
pub(crate) struct McpToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}
