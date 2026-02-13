//! Document-related tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        document_search(),
        document_search_text(),
        document_get(),
        document_list(),
        document_find(),
        document_update(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn document_search() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DocumentSearch,
        location: ToolLocation::Internal,
        mcp_enabled: true,
        description: "Search game documents (rulebooks, scenarios) for information using semantic similarity. Good for conceptual queries like 'how do jump drives work' or 'rules for combat'. Returns relevant text chunks.",
        mcp_suffix: None,
        category: "document",
        priority: 1, // High priority - core RAG functionality
        parameters: || {
            serde_json::json!({
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
            })
        },
    }
}

fn document_search_text() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DocumentSearchText,
        location: ToolLocation::Internal,
        mcp_enabled: true,
        description: "Search documents using exact keyword matching. Use this for specific names, terms, or when semantic search doesn't find what you need. Supports filtering by section (e.g., 'Adventure 1'). Good for finding specific characters like 'Anders Casarii' or references within a particular section.",
        mcp_suffix: None,
        category: "document",
        priority: 2,
        parameters: || {
            serde_json::json!({
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
            })
        },
    }
}

fn document_get() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DocumentGet,
        location: ToolLocation::Internal,
        mcp_enabled: true,
        description: "Get document metadata or retrieve the full text content of a specific page. Use 'page' parameter to read page content - this is the primary way to read specific pages from rulebooks and scenarios.",
        mcp_suffix: None,
        category: "document",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "The document ID (get from document_list or document_find)"
                    },
                    "page": {
                        "type": "integer",
                        "description": "Page number to retrieve. If specified, returns the full text content of that page. If omitted, returns document metadata only."
                    }
                },
                "required": ["document_id"]
            })
        },
    }
}

fn document_list() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DocumentList,
        location: ToolLocation::Internal,
        mcp_enabled: true,
        description: "List all available documents (rulebooks, scenarios) with their IDs and titles.",
        mcp_suffix: None,
        category: "document",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional tags to filter documents"
                    }
                }
            })
        },
    }
}

fn document_find() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DocumentFind,
        location: ToolLocation::Internal,
        mcp_enabled: true,
        description: "Find documents by title (case-insensitive partial match). Returns document IDs and metadata.",
        mcp_suffix: None,
        category: "document",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "The document title to search for (partial match)"
                    }
                },
                "required": ["title"]
            })
        },
    }
}

fn document_update() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DocumentUpdate,
        location: ToolLocation::Internal,
        mcp_enabled: true,
        description: "Update document metadata (title, access level, and/or tags). Use document_list or document_find to get document IDs first. Tags are replaced entirely - provide all desired tags.",
        mcp_suffix: None,
        category: "document",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "The unique identifier of the document to update"
                    },
                    "title": {
                        "type": "string",
                        "description": "New title for the document"
                    },
                    "access_level": {
                        "type": "string",
                        "enum": ["player", "trusted", "assistant", "gm_only"],
                        "description": "Who can access this document"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Tags for categorizing (replaces all existing tags)"
                    }
                },
                "required": ["document_id"]
            })
        },
    }
}
