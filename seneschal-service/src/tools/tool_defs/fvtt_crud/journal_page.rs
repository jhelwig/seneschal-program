//! Journal page CRUD tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

use super::EXTERNAL_MCP_SUFFIX;

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        add_journal_page(),
        get_journal_page(),
        get_journal_pages(),
        update_journal_page(),
        delete_journal_page(),
        list_journal_pages(),
        reorder_journal_pages(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn add_journal_page() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::AddJournalPage,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Add a new page to an existing Foundry VTT journal. Use get_journal first to see existing pages. Works with world or compendium (if unlocked). Rich text supports cross-document links: @UUID[Type.ID]{Label} (e.g., @UUID[Actor.abc123]{Guard Captain}). Types: Actor, Item, JournalEntry, Scene, RollTable.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "name": {
                        "type": "string",
                        "description": "Name of the new page"
                    },
                    "page_type": {
                        "type": "string",
                        "enum": ["text", "image"],
                        "description": "Type of page: 'text' for HTML content, 'image' for an image page"
                    },
                    "content": {
                        "type": "string",
                        "description": "HTML content for text pages"
                    },
                    "src": {
                        "type": "string",
                        "description": "Image path for image pages (use image_deliver first)"
                    },
                    "sort": {
                        "type": "integer",
                        "description": "Sort order value (optional, appends to end if omitted)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                    }
                },
                "required": ["journal_id", "name", "page_type"]
            })
        },
    }
}

fn get_journal_page() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetJournalPage,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Get a specific page from a Foundry VTT journal. Returns full page data including content. Use list_journal_pages or get_journal first to find page IDs.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "page_id": {
                        "type": "string",
                        "description": "The page's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                    }
                },
                "required": ["journal_id", "page_id"]
            })
        },
    }
}

fn get_journal_pages() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetJournalPages,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Get multiple pages from a Foundry VTT journal in one call. Returns full page data including content for each page. Maximum 20 pages per call.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "page_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of page IDs to retrieve (max 20)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                    }
                },
                "required": ["journal_id", "page_ids"]
            })
        },
    }
}

fn update_journal_page() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateJournalPage,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Update a specific page in a Foundry VTT journal. Use get_journal or list_journal_pages first to find page IDs. Works with world or compendium (if unlocked). Rich text supports cross-document links: @UUID[Type.ID]{Label} (e.g., @UUID[Actor.abc123]{Guard Captain}). Types: Actor, Item, JournalEntry, Scene, RollTable.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "page_id": {
                        "type": "string",
                        "description": "The page's document ID"
                    },
                    "name": {
                        "type": "string",
                        "description": "New name for the page"
                    },
                    "content": {
                        "type": "string",
                        "description": "New HTML content (for text pages)"
                    },
                    "src": {
                        "type": "string",
                        "description": "New image path (for image pages)"
                    },
                    "sort": {
                        "type": "integer",
                        "description": "New sort order value"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                    }
                },
                "required": ["journal_id", "page_id"]
            })
        },
    }
}

fn delete_journal_page() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteJournalPage,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Delete a specific page from a Foundry VTT journal. Use get_journal or list_journal_pages first to find page IDs. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "page_id": {
                        "type": "string",
                        "description": "The page's document ID to delete"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                    }
                },
                "required": ["journal_id", "page_id"]
            })
        },
    }
}

fn list_journal_pages() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListJournalPages,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "List all pages in a Foundry VTT journal. Returns page IDs, names, types, and sort order. Works with world or compendium journals.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                    }
                },
                "required": ["journal_id"]
            })
        },
    }
}

fn reorder_journal_pages() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ReorderJournalPages,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Bulk reorder pages in a Foundry VTT journal. Provide an array of page IDs in the desired display order. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "page_order": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of page IDs in the desired display order (first ID will appear first)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                    }
                },
                "required": ["journal_id", "page_order"]
            })
        },
    }
}
