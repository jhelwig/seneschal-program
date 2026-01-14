//! Image-related tool definitions.

use std::collections::HashMap;

use crate::tools::{registry::{ToolMetadata, ToolName}, ToolLocation};

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        image_list(),
        image_search(),
        image_get(),
        image_deliver(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn image_list() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ImageList,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List images from a document. Use document_find first to get the document ID, then use this to browse images from specific pages or page ranges.",
        mcp_suffix: None,
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "document_id": {
                    "type": "string",
                    "description": "The document ID"
                },
                "start_page": {
                    "type": "integer",
                    "description": "Optional: filter to images starting from this page number"
                },
                "end_page": {
                    "type": "integer",
                    "description": "Optional: filter to images up to and including this page number"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum images to return (default 20)"
                }
            },
            "required": ["document_id"]
        }),
    }
}

fn image_search() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ImageSearch,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Search for images by description using semantic similarity. Good for finding maps, portraits, deck plans, etc.",
        mcp_suffix: None,
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Description of the image to find"
                },
                "document_id": {
                    "type": "string",
                    "description": "Optional: limit search to a specific document"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results (default 10)"
                }
            },
            "required": ["query"]
        }),
    }
}

fn image_get() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ImageGet,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get detailed information about a specific image by its ID.",
        mcp_suffix: None,
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "image_id": {
                    "type": "string",
                    "description": "The image ID"
                }
            },
            "required": ["image_id"]
        }),
    }
}

fn image_deliver() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ImageDeliver,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Copy an image to the Foundry VTT assets directory so it can be used in scenes, actors, etc. Returns the full FVTT path (starting with 'assets/') to use in documents.",
        mcp_suffix: None,
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "image_id": {
                    "type": "string",
                    "description": "The image ID to deliver"
                },
                "target_path": {
                    "type": "string",
                    "description": "Optional: path relative to the assets directory, e.g., 'seneschal/tokens/guard.webp'. Do NOT include 'assets/' prefix. Default: auto-generated as 'seneschal/{doc_title}/page_{N}.webp'"
                }
            },
            "required": ["image_id"]
        }),
    }
}
