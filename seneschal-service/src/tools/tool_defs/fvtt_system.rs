//! FVTT system tools (schema, dice, folders, users, assets).

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

/// Suffix added to external tool descriptions for MCP
const EXTERNAL_MCP_SUFFIX: &str = "Requires GM WebSocket connection.";

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        // Core FVTT tools
        system_schema(),
        fvtt_read(),
        fvtt_write(),
        fvtt_query(),
        dice_roll(),
        // Asset tools
        fvtt_assets_browse(),
        image_describe(),
        // Folder management
        list_folders(),
        create_folder(),
        update_folder(),
        delete_folder(),
        // User and ownership
        list_users(),
        update_ownership(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn system_schema() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::SystemSchema,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get the game system's schema for actors and items.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item"],
                        "description": "Optional: get schema for specific document type"
                    }
                }
            })
        },
    }
}

fn fvtt_read() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::FvttRead,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Read a Foundry VTT document. Document types: actor (characters, NPCs, creatures), item (weapons, armor, equipment), journal_entry (notes, handouts), scene (maps/battlemaps where tokens are placed), rollable_table (random tables).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "actor=characters/NPCs, item=equipment, journal_entry=notes, scene=map/battlemap, rollable_table=random table"
                    },
                    "document_id": {
                        "type": "string",
                        "description": "The document ID"
                    }
                },
                "required": ["document_type", "document_id"]
            })
        },
    }
}

fn fvtt_write() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::FvttWrite,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create or modify a Foundry VTT document. Document types: actor (characters, NPCs, creatures), item (weapons, armor, equipment), journal_entry (notes, handouts), scene (maps/battlemaps - use with image_deliver to create maps from PDF images), rollable_table (random tables).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "actor=characters/NPCs, item=equipment, journal_entry=notes, scene=map/battlemap, rollable_table=random table"
                    },
                    "operation": {
                        "type": "string",
                        "enum": ["create", "update", "delete"],
                        "description": "The operation to perform"
                    },
                    "data": {
                        "type": "object",
                        "description": "The document data. For scenes: {name, background: {src: 'path/to/image.webp'}, width, height, grid: {size, type}}"
                    }
                },
                "required": ["document_type", "operation", "data"]
            })
        },
    }
}

fn fvtt_query() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::FvttQuery,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Query Foundry VTT documents with filters. Document types: actor (characters, NPCs), item (equipment), journal_entry (notes), scene (maps/battlemaps), rollable_table (random tables).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "actor=characters/NPCs, item=equipment, journal_entry=notes, scene=map/battlemap, rollable_table=random table"
                    },
                    "filters": {
                        "type": "object",
                        "description": "Query filters (e.g., {name: 'Marcus'})"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results (default 20)"
                    }
                },
                "required": ["document_type"]
            })
        },
    }
}

fn dice_roll() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DiceRoll,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Roll dice using FVTT's dice system. Results are logged to the game.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 3, // Low priority - specialized tool
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "formula": {
                        "type": "string",
                        "description": "Dice formula (e.g., '2d6+2', '1d20')"
                    },
                    "label": {
                        "type": "string",
                        "description": "Optional label for the roll"
                    }
                },
                "required": ["formula"]
            })
        },
    }
}

fn fvtt_assets_browse() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::FvttAssetsBrowse,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Browse files in Foundry VTT's file system. Returns a list of files and directories at the specified path.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to browse (e.g., 'assets', 'assets/seneschal/tokens'). Defaults to root."
                    },
                    "source": {
                        "type": "string",
                        "enum": ["data", "public", "s3"],
                        "description": "File source to browse (default: 'data')"
                    },
                    "extensions": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by file extensions (e.g., ['.webp', '.png', '.jpg'])"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "If true, also list files in subdirectories (default: false)"
                    }
                }
            })
        },
    }
}

fn image_describe() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ImageDescribe,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a detailed vision model description of an image file in FVTT. Uses the configured vision model to analyze the image. Results are cached.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "image_path": {
                        "type": "string",
                        "description": "FVTT path to the image (e.g., 'assets/tokens/guard.webp')"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context about what the image is for (e.g., 'NPC portrait for a tavern encounter')"
                    },
                    "force_refresh": {
                        "type": "boolean",
                        "description": "If true, bypass cache and generate a new description (default: false)"
                    }
                },
                "required": ["image_path"]
            })
        },
    }
}

fn list_folders() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListFolders,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List all folders for a specific document type in Foundry VTT. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of documents the folders contain"
                    },
                    "parent_folder": {
                        "type": "string",
                        "description": "Filter to only show folders inside this parent folder"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to list folders from. If omitted, lists from world."
                    }
                },
                "required": ["document_type"]
            })
        },
    }
}

fn create_folder() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateFolder,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a new folder for organizing documents in Foundry VTT. Can create in world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the folder"
                    },
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of documents this folder will contain"
                    },
                    "parent_folder": {
                        "type": "string",
                        "description": "Name of parent folder for nesting (optional)"
                    },
                    "color": {
                        "type": "string",
                        "description": "Folder color as hex code (e.g., '#ff0000')"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to create folder in. If omitted, creates in world."
                    }
                },
                "required": ["name", "document_type"]
            })
        },
    }
}

fn update_folder() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateFolder,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update a folder's properties (rename, move, or change color). Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "folder_id": {
                        "type": "string",
                        "description": "ID of the folder to update"
                    },
                    "name": {
                        "type": "string",
                        "description": "New name for the folder"
                    },
                    "parent_folder": {
                        "type": ["string", "null"],
                        "description": "New parent folder name or ID (use null to move to root)"
                    },
                    "color": {
                        "type": "string",
                        "description": "New color as hex code"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the folder. If omitted, targets world folder."
                    }
                },
                "required": ["folder_id"]
            })
        },
    }
}

fn delete_folder() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteFolder,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a folder. By default, documents inside are moved to root level. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "folder_id": {
                        "type": "string",
                        "description": "ID of the folder to delete"
                    },
                    "delete_contents": {
                        "type": "boolean",
                        "description": "If true, also delete all documents inside the folder (default: false)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID containing the folder. If omitted, targets world folder."
                    }
                },
                "required": ["folder_id"]
            })
        },
    }
}

fn list_users() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListUsers,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List all users in the Foundry VTT world. Returns user IDs, names, roles, and online status.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "include_inactive": {
                        "type": "boolean",
                        "description": "Include users who are not currently online (default: true)"
                    }
                }
            })
        },
    }
}

fn update_ownership() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateOwnership,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update ownership permissions for a Foundry VTT document. Permission levels: 0=NONE, 1=LIMITED, 2=OBSERVER, 3=OWNER. Use 'default' key to set base permissions for all users.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_system",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of document"
                    },
                    "document_id": {
                        "type": "string",
                        "description": "The document's ID"
                    },
                    "ownership": {
                        "type": "object",
                        "description": "Ownership mapping. Keys are user IDs or 'default'. Values are 0=NONE, 1=LIMITED, 2=OBSERVER, 3=OWNER."
                    }
                },
                "required": ["document_type", "document_id", "ownership"]
            })
        },
    }
}
