//! Compendium pack tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

use super::EXTERNAL_MCP_SUFFIX;

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        list_compendium_packs(),
        browse_compendium_pack(),
        search_compendium_packs(),
        import_from_compendium(),
        export_to_compendium(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn list_compendium_packs() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListCompendiumPacks,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List available compendium packs. Compendiums store reusable documents (actors, items, journals, etc.) outside the world.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["Actor", "Item", "JournalEntry", "Scene", "RollTable", "Macro", "Playlist"],
                        "description": "Filter by document type stored in the pack"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results (default 50)"
                    }
                }
            })
        },
    }
}

fn browse_compendium_pack() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::BrowseCompendiumPack,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List documents in a compendium pack. Uses lightweight index for fast browsing without loading full documents.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID (e.g., 'dnd5e.monsters', 'world.my-pack')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Filter by document name (partial match)"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Filter by folder name within the pack"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Skip first N results for pagination (default 0)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results (default 50)"
                    }
                },
                "required": ["pack_id"]
            })
        },
    }
}

fn search_compendium_packs() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::SearchCompendiumPacks,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Search for documents across all compendium packs by name.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search term to match against document names"
                    },
                    "document_type": {
                        "type": "string",
                        "enum": ["Actor", "Item", "JournalEntry", "Scene", "RollTable", "Macro", "Playlist"],
                        "description": "Filter to packs containing this document type"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results (default 50)"
                    }
                },
                "required": ["query"]
            })
        },
    }
}

fn import_from_compendium() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ImportFromCompendium,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Import documents from a compendium pack into the world. Creates copies in the world collection.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to import from"
                    },
                    "document_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of document IDs to import. Omit to import all."
                    },
                    "folder": {
                        "type": "string",
                        "description": "World folder name to place imported documents in"
                    },
                    "keep_id": {
                        "type": "boolean",
                        "description": "Preserve original document IDs (default false, can cause conflicts)"
                    },
                    "keep_folders": {
                        "type": "boolean",
                        "description": "Recreate folder structure from compendium (default false)"
                    }
                },
                "required": ["pack_id"]
            })
        },
    }
}

fn export_to_compendium() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ExportToCompendium,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Export documents from the world to a compendium pack. Pack must be unlocked.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to export to (must be unlocked)"
                    },
                    "document_type": {
                        "type": "string",
                        "enum": ["Actor", "Item", "JournalEntry", "Scene", "RollTable", "Macro", "Playlist"],
                        "description": "Type of documents to export"
                    },
                    "document_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of world document IDs to export"
                    },
                    "keep_id": {
                        "type": "boolean",
                        "description": "Preserve document IDs in compendium (default false)"
                    },
                    "keep_folders": {
                        "type": "boolean",
                        "description": "Recreate folder structure in compendium (default false)"
                    }
                },
                "required": ["pack_id", "document_type", "document_ids"]
            })
        },
    }
}
