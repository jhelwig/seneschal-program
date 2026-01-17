//! Actor CRUD tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

use super::EXTERNAL_MCP_SUFFIX;

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_actor(),
        get_actor(),
        update_actor(),
        delete_actor(),
        list_actors(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_actor() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateActor,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a Foundry VTT actor (character, NPC, creature, vehicle, etc.). Use system_schema first to understand the actor types and data structure. Can create in world or compendium. Biography/notes fields support cross-document links: @UUID[Type.ID]{Label}.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the actor"
                    },
                    "actor_type": {
                        "type": "string",
                        "description": "Type of actor (e.g., 'character', 'npc', 'creature', 'vehicle' - varies by game system)"
                    },
                    "img": {
                        "type": "string",
                        "description": "Path to the actor's portrait image (use image_deliver first)"
                    },
                    "data": {
                        "type": "object",
                        "description": "Actor system data (stats, attributes, etc. - use system_schema to see structure). To add embedded items, include an 'items' array here with item objects containing 'name', 'type', and 'system' fields."
                    },
                    "folder": {
                        "type": "string",
                        "description": "Name of folder to place the actor in"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to create in. If omitted, creates in world."
                    }
                },
                "required": ["name", "actor_type"]
            })
        },
    }
}

fn get_actor() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetActor,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a Foundry VTT actor by ID. Returns the actor's complete data. Can read from world or compendium.",
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
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to read from. If omitted, reads from world."
                    }
                },
                "required": ["actor_id"]
            })
        },
    }
}

fn update_actor() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateActor,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT actor. Can modify name, image, stats, or any system data. Works with world or compendium (if unlocked). Biography/notes fields support cross-document links: @UUID[Type.ID]{Label}.",
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
                        "description": "New name for the actor"
                    },
                    "img": {
                        "type": "string",
                        "description": "New portrait image path"
                    },
                    "folder": {
                        "type": ["string", "null"],
                        "description": "Folder name or ID to move the actor to. Use null to move to root level."
                    },
                    "data": {
                        "type": "object",
                        "description": "Actor system data to update (stats, attributes, etc.). To add/update embedded items, include an 'items' array here. Items with '_id' update existing items; items without '_id' create new embedded items."
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to update in. If omitted, updates in world."
                    }
                },
                "required": ["actor_id"]
            })
        },
    }
}

fn delete_actor() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteActor,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a Foundry VTT actor permanently. Works with world or compendium (if unlocked).",
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
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                    }
                },
                "required": ["actor_id"]
            })
        },
    }
}

fn list_actors() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListActors,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List actors in Foundry VTT. Can filter by name pattern. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 1, // High priority - most common FVTT query
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Filter by name (partial match)"
                    },
                    "actor_type": {
                        "type": "string",
                        "description": "Filter by actor type (e.g., 'character', 'npc')"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Filter by folder name"
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
