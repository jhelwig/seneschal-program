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

use crate::search::format_search_results_for_llm;
use crate::service::SeneschalService;
use crate::tools::{SearchFilters, TagMatch, TravellerTool};

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

async fn handle_initialize(_state: &McpState) -> Result<serde_json::Value, McpError> {
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

async fn handle_tools_list(_state: &McpState) -> Result<serde_json::Value, McpError> {
    let tools = vec![
        McpToolDefinition {
            name: "document_search".to_string(),
            description: "Search game documents (rulebooks, scenarios) for information".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional tags to filter results"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default 10)"
                    }
                },
                "required": ["query"]
            }),
        },
        McpToolDefinition {
            name: "document_get".to_string(),
            description: "Get a specific document or page by ID".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "The document ID"
                    },
                    "page": {
                        "type": "integer",
                        "description": "Optional specific page number"
                    }
                },
                "required": ["document_id"]
            }),
        },
        McpToolDefinition {
            name: "traveller_uwp_parse".to_string(),
            description:
                "Parse a Traveller UWP (Universal World Profile) string into detailed world data"
                    .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "uwp": {
                        "type": "string",
                        "description": "UWP string (e.g., 'A867949-C')"
                    }
                },
                "required": ["uwp"]
            }),
        },
        McpToolDefinition {
            name: "traveller_jump_calc".to_string(),
            description: "Calculate jump drive fuel requirements and time".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "distance_parsecs": {
                        "type": "integer",
                        "description": "Distance in parsecs"
                    },
                    "ship_jump_rating": {
                        "type": "integer",
                        "description": "Ship's jump drive rating (1-6)"
                    },
                    "ship_tonnage": {
                        "type": "integer",
                        "description": "Ship's total tonnage"
                    }
                },
                "required": ["distance_parsecs", "ship_jump_rating", "ship_tonnage"]
            }),
        },
        McpToolDefinition {
            name: "traveller_skill_lookup".to_string(),
            description:
                "Look up a Traveller skill's description, characteristic, and specialities"
                    .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Name of the skill"
                    },
                    "speciality": {
                        "type": "string",
                        "description": "Optional speciality"
                    }
                },
                "required": ["skill_name"]
            }),
        },
    ];

    Ok(serde_json::json!({ "tools": tools }))
}

async fn handle_tool_call(
    state: &McpState,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, McpError> {
    let params = params.ok_or_else(|| McpError {
        code: -32602,
        message: "Missing params".to_string(),
    })?;

    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError {
            code: -32602,
            message: "Missing tool name".to_string(),
        })?;

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    // MCP clients have GM access (role=4) since MCP has no user context
    let gm_role = 4u8;

    let result = match name {
        "document_search" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tags: Vec<String> = arguments
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let limit = arguments
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

            match state.service.search(query, gm_role, limit, filters).await {
                Ok(results) => {
                    let formatted =
                        format_search_results_for_llm(&results, &state.service.i18n, "en");
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": formatted
                        }]
                    })
                }
                Err(e) => {
                    return Err(McpError {
                        code: -32000,
                        message: e.to_string(),
                    });
                }
            }
        }
        "document_get" => {
            let doc_id = arguments
                .get("document_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match state.service.db.get_chunk(doc_id) {
                Ok(Some(chunk)) => {
                    if chunk.access_level.accessible_by(gm_role) {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": chunk.content
                            }]
                        })
                    } else {
                        return Err(McpError {
                            code: -32000,
                            message: "Access denied".to_string(),
                        });
                    }
                }
                Ok(None) => {
                    return Err(McpError {
                        code: -32000,
                        message: "Document not found".to_string(),
                    });
                }
                Err(e) => {
                    return Err(McpError {
                        code: -32000,
                        message: e.to_string(),
                    });
                }
            }
        }
        "traveller_uwp_parse" => {
            let uwp = arguments.get("uwp").and_then(|v| v.as_str()).unwrap_or("");
            let tool = TravellerTool::ParseUwp {
                uwp: uwp.to_string(),
            };

            match tool.execute() {
                Ok(result) => serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&result).unwrap_or_default()
                    }]
                }),
                Err(e) => {
                    return Err(McpError {
                        code: -32000,
                        message: e,
                    });
                }
            }
        }
        "traveller_jump_calc" => {
            let distance = arguments
                .get("distance_parsecs")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u8;
            let rating = arguments
                .get("ship_jump_rating")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u8;
            let tonnage = arguments
                .get("ship_tonnage")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as u32;

            let tool = TravellerTool::JumpCalculation {
                distance_parsecs: distance,
                ship_jump_rating: rating,
                ship_tonnage: tonnage,
            };

            match tool.execute() {
                Ok(result) => serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&result).unwrap_or_default()
                    }]
                }),
                Err(e) => {
                    return Err(McpError {
                        code: -32000,
                        message: e,
                    });
                }
            }
        }
        "traveller_skill_lookup" => {
            let skill = arguments
                .get("skill_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let speciality = arguments
                .get("speciality")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let tool = TravellerTool::SkillLookup {
                skill_name: skill.to_string(),
                speciality,
            };

            match tool.execute() {
                Ok(result) => serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&result).unwrap_or_default()
                    }]
                }),
                Err(e) => {
                    return Err(McpError {
                        code: -32000,
                        message: e,
                    });
                }
            }
        }
        _ => {
            return Err(McpError {
                code: -32601,
                message: format!("Unknown tool: {}", name),
            });
        }
    };

    Ok(result)
}

// MCP Protocol Types

#[derive(Debug, Serialize, Deserialize)]
struct McpRequest {
    jsonrpc: String,
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct McpResponse {
    jsonrpc: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<McpError>,
}

#[derive(Debug, Serialize)]
struct McpError {
    code: i32,
    message: String,
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
struct McpToolDefinition {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}
