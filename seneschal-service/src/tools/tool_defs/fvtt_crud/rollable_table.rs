//! Rollable table CRUD tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

use super::EXTERNAL_MCP_SUFFIX;

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_rollable_table(),
        get_rollable_table(),
        update_rollable_table(),
        delete_rollable_table(),
        list_rollable_tables(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_rollable_table() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateRollableTable,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a Foundry VTT rollable table for random encounters, loot, events, etc. Can create in world or compendium. Results can be text, document links, or compendium references.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the rollable table"
                    },
                    "formula": {
                        "type": "string",
                        "description": "Dice formula for the table (e.g., '1d6', '2d6', '1d100')"
                    },
                    "results": {
                        "type": "array",
                        "description": "Array of result objects. Text: {range: [low, high], text, weight, img}. Document: {type: 'document', document_collection: 'Actor'|'Item'|'JournalEntry'|'Scene'|'RollTable', document_id, range, weight, img, text?} - text is optional, defaults to linked document's name. Compendium: {type: 'compendium', document_collection: 'pack.name', document_id, range, weight, img, text?} - text optional, defaults to compendium entry's name.",
                        "items": {
                            "type": "object"
                        }
                    },
                    "img": {
                        "type": "string",
                        "description": "Optional table image"
                    },
                    "description": {
                        "type": "string",
                        "description": "Description of what this table is for"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Name of folder to place the table in"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to create in. If omitted, creates in world."
                    }
                },
                "required": ["name", "formula", "results"]
            })
        },
    }
}

fn get_rollable_table() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetRollableTable,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a Foundry VTT rollable table by ID. Returns the table's formula and all results. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "table_id": {
                        "type": "string",
                        "description": "The rollable table's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to read from. If omitted, reads from world."
                    }
                },
                "required": ["table_id"]
            })
        },
    }
}

fn update_rollable_table() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateRollableTable,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT rollable table. Can modify name, formula, or results. Works with world or compendium (if unlocked). Results can be text, document links, or compendium references.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "table_id": {
                        "type": "string",
                        "description": "The rollable table's document ID"
                    },
                    "name": {
                        "type": "string",
                        "description": "New name for the table"
                    },
                    "formula": {
                        "type": "string",
                        "description": "New dice formula"
                    },
                    "folder": {
                        "type": ["string", "null"],
                        "description": "Folder name or ID to move the table to. Use null to move to root level."
                    },
                    "results": {
                        "type": "array",
                        "description": "Updated results array. Text: {range: [low, high], text, weight, img}. Document: {type: 'document', document_collection: 'Actor'|'Item'|etc, document_id, range, weight, img, text?} - text optional, defaults to linked document's name. Compendium: {type: 'compendium', document_collection: 'pack.name', document_id, range, weight, img, text?} - text optional, defaults to entry's name.",
                        "items": {
                            "type": "object"
                        }
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to update in. If omitted, updates in world."
                    }
                },
                "required": ["table_id"]
            })
        },
    }
}

fn delete_rollable_table() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteRollableTable,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a Foundry VTT rollable table permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "table_id": {
                        "type": "string",
                        "description": "The rollable table's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                    }
                },
                "required": ["table_id"]
            })
        },
    }
}

fn list_rollable_tables() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListRollableTables,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List rollable tables in Foundry VTT. Can filter by name pattern. Lists from world or compendium.",
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
