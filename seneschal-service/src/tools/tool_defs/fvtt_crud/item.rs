//! Item CRUD tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

use super::EXTERNAL_MCP_SUFFIX;

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_item(),
        get_item(),
        get_items(),
        update_item(),
        delete_item(),
        list_items(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateItem,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Create a Foundry VTT item (weapon, armor, equipment, skill, spell, etc.). Use system_schema first to understand item types. Can create in world or compendium. Description fields support cross-document links: @UUID[Type.ID]{Label}.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the item"
                    },
                    "item_type": {
                        "type": "string",
                        "description": "Type of item (e.g., 'weapon', 'armor', 'equipment', 'skill' - varies by game system)"
                    },
                    "img": {
                        "type": "string",
                        "description": "Path to the item's image (use image_deliver first)"
                    },
                    "data": {
                        "type": "object",
                        "description": "Item system data (damage, weight, cost, etc. - use system_schema to see structure)"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Folder name or ID to place the item in"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to create in. If omitted, creates in world."
                    }
                },
                "required": ["name", "item_type"]
            })
        },
    }
}

fn get_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetItem,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Get a Foundry VTT item by ID. Returns the item's complete data. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "item_id": {
                        "type": "string",
                        "description": "The item's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to read from. If omitted, reads from world."
                    }
                },
                "required": ["item_id"]
            })
        },
    }
}

fn get_items() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetItems,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Get multiple items by ID in one call. Returns full item data for each. Maximum 20 items per call.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "item_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of item IDs to retrieve (max 20)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to read from. If omitted, reads from world."
                    }
                },
                "required": ["item_ids"]
            })
        },
    }
}

fn update_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateItem,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT item. Can modify name, image, or any system data. Works with world or compendium (if unlocked). Description fields support cross-document links: @UUID[Type.ID]{Label}.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "item_id": {
                        "type": "string",
                        "description": "The item's document ID"
                    },
                    "name": {
                        "type": "string",
                        "description": "New name for the item"
                    },
                    "img": {
                        "type": "string",
                        "description": "New item image path"
                    },
                    "folder": {
                        "type": ["string", "null"],
                        "description": "Folder name or ID to move the item to. Use null to move to root level."
                    },
                    "data": {
                        "type": "object",
                        "description": "Item system data to update"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to update in. If omitted, updates in world."
                    }
                },
                "required": ["item_id"]
            })
        },
    }
}

fn delete_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteItem,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Delete a Foundry VTT item permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "item_id": {
                        "type": "string",
                        "description": "The item's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                    }
                },
                "required": ["item_id"]
            })
        },
    }
}

fn list_items() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListItems,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "List items in Foundry VTT. Can filter by name pattern or item type. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 1, // High priority - second most common FVTT query
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Filter by name (partial match)"
                    },
                    "item_type": {
                        "type": "string",
                        "description": "Filter by item type (e.g., 'weapon', 'armor')"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Filter by folder name or ID"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results (default 20)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to list from. If omitted, lists from world."
                    }
                }
            })
        },
    }
}
