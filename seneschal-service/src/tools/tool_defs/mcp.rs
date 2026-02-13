//! MCP-specific tool definitions.
//!
//! These tools are only exposed via MCP and provide meta-functionality
//! for tool discovery and search.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [tool_search()];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn tool_search() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ToolSearch,
        location: ToolLocation::Internal,
        mcp_enabled: true,
        description: "Search for available tools using natural language. Returns tool references for discovered capabilities. Use this to find the right tool for a task.",
        mcp_suffix: None,
        category: "mcp",
        priority: 0, // Never defer - always available for discovery
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language query describing what you want to do (e.g., 'create an actor', 'search documents', 'roll dice')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default 5, max 10)"
                    }
                },
                "required": ["query"]
            })
        },
    }
}
