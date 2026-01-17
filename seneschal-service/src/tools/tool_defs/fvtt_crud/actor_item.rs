//! Actor embedded item CRUD tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

use super::EXTERNAL_MCP_SUFFIX;

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        add_actor_item(),
        get_actor_item(),
        update_actor_item(),
        delete_actor_item(),
        list_actor_items(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn add_actor_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::AddActorItem,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Add an embedded item to a Foundry VTT actor. Use this to add equipment, skills, abilities, or other items to characters, NPCs, or other actors. Works with world or compendium actors (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "actor_id": {
                        "type": "string",
                        "description": "The actor's document ID"
                    },
                    "name": {
                        "type": "string",
                        "description": "Name of the item to add"
                    },
                    "item_type": {
                        "type": "string",
                        "description": "Type of item (e.g., 'weapon', 'armor', 'skill' - varies by game system)"
                    },
                    "img": {
                        "type": "string",
                        "description": "Path to the item's image"
                    },
                    "data": {
                        "type": "object",
                        "description": "Item system data (stats, properties, etc. - use system_schema to see structure)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the actor. If omitted, targets world actor."
                    }
                },
                "required": ["actor_id", "name", "item_type"]
            })
        },
    }
}

fn get_actor_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetActorItem,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a specific embedded item from a Foundry VTT actor by item ID. Returns the item's complete data.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "actor_id": {
                        "type": "string",
                        "description": "The actor's document ID"
                    },
                    "item_id": {
                        "type": "string",
                        "description": "The embedded item's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the actor. If omitted, targets world actor."
                    }
                },
                "required": ["actor_id", "item_id"]
            })
        },
    }
}

fn update_actor_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateActorItem,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an embedded item on a Foundry VTT actor. Can modify name, image, or any system data. Works with world or compendium actors (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "actor_id": {
                        "type": "string",
                        "description": "The actor's document ID"
                    },
                    "item_id": {
                        "type": "string",
                        "description": "The embedded item's document ID"
                    },
                    "name": {
                        "type": "string",
                        "description": "New name for the item"
                    },
                    "img": {
                        "type": "string",
                        "description": "New image path for the item"
                    },
                    "data": {
                        "type": "object",
                        "description": "Item system data to update"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the actor. If omitted, targets world actor."
                    }
                },
                "required": ["actor_id", "item_id"]
            })
        },
    }
}

fn delete_actor_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteActorItem,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete an embedded item from a Foundry VTT actor. Works with world or compendium actors (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "actor_id": {
                        "type": "string",
                        "description": "The actor's document ID"
                    },
                    "item_id": {
                        "type": "string",
                        "description": "The embedded item's document ID to delete"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the actor. If omitted, targets world actor."
                    }
                },
                "required": ["actor_id", "item_id"]
            })
        },
    }
}

fn list_actor_items() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListActorItems,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List all embedded items on a Foundry VTT actor. Can filter by item type or name. Returns item IDs, names, types, and images.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "actor_id": {
                        "type": "string",
                        "description": "The actor's document ID"
                    },
                    "item_type": {
                        "type": "string",
                        "description": "Filter by item type (e.g., 'weapon', 'armor')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Filter by name (partial match)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the actor. If omitted, targets world actor."
                    }
                },
                "required": ["actor_id"]
            })
        },
    }
}
