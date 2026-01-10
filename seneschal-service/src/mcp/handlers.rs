//! MCP message handlers.
//!
//! Handlers for initialize and tools/list requests.

use super::{McpError, McpState, McpToolDefinition};

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
pub async fn handle_tools_list(_state: &McpState) -> Result<serde_json::Value, McpError> {
    let tools = vec![
        McpToolDefinition {
            name: "document_search".to_string(),
            description: "Search game documents (rulebooks, scenarios) using semantic similarity. Good for conceptual queries like 'how do jump drives work'.".to_string(),
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
            name: "document_search_text".to_string(),
            description: "Search documents using exact keyword matching. Use for specific names, terms, or when semantic search doesn't find what you need. Supports section filtering.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keywords to search for (exact matching)"
                    },
                    "section": {
                        "type": "string",
                        "description": "Optional: filter to content within this section (e.g., 'Adventure 1')"
                    },
                    "document_id": {
                        "type": "string",
                        "description": "Optional: limit search to a specific document"
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
            description: "Get document metadata or retrieve the full text content of a specific page. Use 'page' parameter to read page content.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "The document ID (get from document_search results)"
                    },
                    "page": {
                        "type": "integer",
                        "description": "Page number to retrieve. If specified, returns the full text content of that page. If omitted, returns document metadata only."
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
