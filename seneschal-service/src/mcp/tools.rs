//! MCP tool call handler.
//!
//! Handles execution of individual tool calls from MCP clients.

mod document;
mod external;
mod image;
mod traveller;
mod traveller_map;
mod traveller_worlds;

use crate::tools::{ToolLocation, classify_tool};

use super::tool_search::TOOL_SEARCH_INDEX;
use super::{McpError, McpState};

/// Handle tools/call request
pub async fn handle_tool_call(
    state: &McpState,
    params: Option<serde_json::Value>,
    session_id: Option<&str>,
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

    // Classify the tool and route accordingly
    let location = classify_tool(name);

    let result = match location {
        ToolLocation::Internal => {
            // Execute internal tools directly
            execute_internal_tool(state, name, &arguments, gm_role).await?
        }
        ToolLocation::External => {
            // Route external tools through GM WebSocket connection
            external::execute_external_tool(state, name, arguments, session_id).await?
        }
    };

    Ok(result)
}

/// Execute an internal tool directly on the backend
async fn execute_internal_tool(
    state: &McpState,
    name: &str,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    match name {
        // Document tools
        "document_search" => document::execute_document_search(state, arguments, gm_role).await,
        "document_search_text" => document::execute_document_search_text(state, arguments, gm_role),
        "document_get" => document::execute_document_get(state, arguments, gm_role),
        "document_list" => document::execute_document_list(state, arguments, gm_role),
        "document_find" => document::execute_document_find(state, arguments, gm_role),
        "document_update" => document::execute_document_update(state, arguments, gm_role),

        // Image tools
        "image_list" => image::execute_image_list(state, arguments, gm_role),
        "image_search" => image::execute_image_search(state, arguments, gm_role).await,
        "image_get" => image::execute_image_get(state, arguments, gm_role),
        "image_deliver" => image::execute_image_deliver(state, arguments, gm_role),

        // Traveller tools
        "system_schema" => traveller::execute_system_schema(arguments),
        "traveller_uwp_parse" => traveller::execute_traveller_uwp_parse(arguments),
        "traveller_jump_calc" => traveller::execute_traveller_jump_calc(arguments),
        "traveller_skill_lookup" => traveller::execute_traveller_skill_lookup(arguments),

        // Traveller Map API tools
        "traveller_map_search" => {
            traveller_map::execute_traveller_map_search(state, arguments).await
        }
        "traveller_map_jump_worlds" => {
            traveller_map::execute_traveller_map_jump_worlds(state, arguments).await
        }
        "traveller_map_route" => traveller_map::execute_traveller_map_route(state, arguments).await,
        "traveller_map_world_data" => {
            traveller_map::execute_traveller_map_world_data(state, arguments).await
        }
        "traveller_map_sector_data" => {
            traveller_map::execute_traveller_map_sector_data(state, arguments).await
        }
        "traveller_map_coordinates" => {
            traveller_map::execute_traveller_map_coordinates(state, arguments).await
        }
        "traveller_map_list_sectors" => {
            traveller_map::execute_traveller_map_list_sectors(state, arguments).await
        }
        "traveller_map_poster_url" => {
            traveller_map::execute_traveller_map_poster_url(state, arguments)
        }
        "traveller_map_jump_map_url" => {
            traveller_map::execute_traveller_map_jump_map_url(state, arguments)
        }
        "traveller_map_save_poster" => {
            traveller_map::execute_traveller_map_save_poster(state, arguments).await
        }
        "traveller_map_save_jump_map" => {
            traveller_map::execute_traveller_map_save_jump_map(state, arguments).await
        }

        // Traveller Worlds tools
        "traveller_worlds_canon_url" => {
            traveller_worlds::execute_traveller_worlds_canon_url(state, arguments).await
        }
        "traveller_worlds_canon_save" => {
            traveller_worlds::execute_traveller_worlds_canon_save(state, arguments).await
        }
        "traveller_worlds_custom_url" => {
            traveller_worlds::execute_traveller_worlds_custom_url(state, arguments)
        }
        "traveller_worlds_custom_save" => {
            traveller_worlds::execute_traveller_worlds_custom_save(state, arguments).await
        }

        // Tool search
        "tool_search" => execute_tool_search(arguments),

        _ => Err(McpError {
            code: -32601,
            message: format!("Unknown internal tool: {}", name),
        }),
    }
}

/// Execute tool_search - search for tools using natural language.
///
/// Returns tool_reference blocks per the Claude tool search tool specification.
fn execute_tool_search(arguments: &serde_json::Value) -> Result<serde_json::Value, McpError> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .min(10) as usize;

    if query.is_empty() {
        return Err(McpError {
            code: -32602,
            message: "Query parameter is required".to_string(),
        });
    }

    let results = TOOL_SEARCH_INDEX.search(query, limit);

    // Return tool_reference blocks per Claude docs
    let tool_references: Vec<serde_json::Value> = results
        .into_iter()
        .map(|tool_name| {
            serde_json::json!({
                "type": "tool_reference",
                "tool_name": tool_name
            })
        })
        .collect();

    Ok(serde_json::json!({ "content": tool_references }))
}

/// Sanitize a string for use in a filename
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ' ' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}
