//! Scene CRUD tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

use super::EXTERNAL_MCP_SUFFIX;

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_scene(),
        get_scene(),
        update_scene(),
        delete_scene(),
        list_scenes(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_scene() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateScene,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a Foundry VTT scene with a background image. Use image_deliver first to get the image path. Can create in world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name for the scene"
                    },
                    "image_path": {
                        "type": "string",
                        "description": "Path to the background image (from image_deliver)"
                    },
                    "width": {
                        "type": "integer",
                        "description": "Scene width in pixels (default: from image)"
                    },
                    "height": {
                        "type": "integer",
                        "description": "Scene height in pixels (default: from image)"
                    },
                    "grid_size": {
                        "type": "integer",
                        "description": "Grid size in pixels (default: 100)"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Name of folder to place the scene in"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to create in (e.g., 'world.my-scenes'). If omitted, creates in world."
                    }
                },
                "required": ["name", "image_path"]
            })
        },
    }
}

fn get_scene() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetScene,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a Foundry VTT scene by ID. Returns the scene's configuration. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "scene_id": {
                        "type": "string",
                        "description": "The scene's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to read from. If omitted, reads from world."
                    }
                },
                "required": ["scene_id"]
            })
        },
    }
}

fn update_scene() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateScene,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT scene. Can modify name, background, dimensions, grid settings, etc. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "scene_id": {
                        "type": "string",
                        "description": "The scene's document ID"
                    },
                    "name": {
                        "type": "string",
                        "description": "New name for the scene"
                    },
                    "image_path": {
                        "type": "string",
                        "description": "New background image path"
                    },
                    "width": {
                        "type": "integer",
                        "description": "Scene width in pixels"
                    },
                    "height": {
                        "type": "integer",
                        "description": "Scene height in pixels"
                    },
                    "grid_size": {
                        "type": "integer",
                        "description": "Grid size in pixels"
                    },
                    "folder": {
                        "type": ["string", "null"],
                        "description": "Folder name or ID to move the scene to. Use null to move to root level."
                    },
                    "data": {
                        "type": "object",
                        "description": "Additional scene data to update (advanced)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to update in. If omitted, updates in world."
                    }
                },
                "required": ["scene_id"]
            })
        },
    }
}

fn delete_scene() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteScene,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a Foundry VTT scene permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "scene_id": {
                        "type": "string",
                        "description": "The scene's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                    }
                },
                "required": ["scene_id"]
            })
        },
    }
}

fn list_scenes() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListScenes,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List scenes in Foundry VTT. Can filter by name pattern. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Filter by name (partial match)"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Filter by folder name"
                    },
                    "active": {
                        "type": "boolean",
                        "description": "Filter to only the currently active scene (world only)"
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
