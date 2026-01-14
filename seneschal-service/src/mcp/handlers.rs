//! MCP message handlers.
//!
//! Handlers for initialize and tools/list requests.
//!
//! NOTE: Tool definitions are now managed by the unified registry in
//! `crate::tools::registry`. This module converts registry format to MCP format.

use super::{McpError, McpState, McpToolDefinition};
use crate::tools::REGISTRY;

/// Handle initialize request
pub async fn handle_initialize(_state: &McpState) -> Result<serde_json::Value, McpError> {
    Ok(serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": "seneschal-service",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Seneschal Program MCP server for game master assistance, document search, and Foundry VTT integration."
    }))
}

/// Handle tools/list request
///
/// This function retrieves tool definitions from the unified registry
/// and converts them to the MCP format.
pub async fn handle_tools_list(_state: &McpState) -> Result<serde_json::Value, McpError> {
    // Get MCP definitions from the unified registry
    let registry_tools = REGISTRY.mcp_definitions();

    // Convert from registry format to MCP module format
    let tools: Vec<McpToolDefinition> = registry_tools
        .into_iter()
        .map(|t| McpToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.input_schema,
        })
        .collect();

    Ok(serde_json::json!({ "tools": tools }))
}

// Legacy definitions kept for reference during migration.
// TODO: Remove after verifying registry output matches.
#[allow(dead_code)]
fn legacy_tools_list() -> Vec<McpToolDefinition> {
    vec![
        McpToolDefinition {
            name: "document_search".to_string(),
            description: "Search game documents (rulebooks, scenarios) using semantic similarity. Good for conceptual queries like 'how do jump drives work'.".to_string(),
            input_schema: serde_json::json!({
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
            }),
        },
        McpToolDefinition {
            name: "document_search_text".to_string(),
            description: "Search documents using exact keyword matching. Use for specific names, terms, or when semantic search doesn't find what you need. Supports section filtering.".to_string(),
            input_schema: serde_json::json!({
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
            }),
        },
        McpToolDefinition {
            name: "document_get".to_string(),
            description: "Get document metadata or retrieve the full text content of a specific page. Use 'page' parameter to read page content.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "The document ID (get from document_search results)"
                    },
                    "page": {
                        "type": "integer",
                        "description": "Page number to retrieve. If specified, returns the full text content of that page. If omitted, returns document metadata only."
                    }
                },
                "required": ["document_id"]
            }),
        },
        McpToolDefinition {
            name: "traveller_uwp_parse".to_string(),
            description:
                "Parse a Traveller UWP (Universal World Profile) string into detailed world data"
                    .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "uwp": {
                        "type": "string",
                        "description": "UWP string (e.g., 'A867949-C')"
                    }
                },
                "required": ["uwp"]
            }),
        },
        McpToolDefinition {
            name: "traveller_jump_calc".to_string(),
            description: "Calculate jump drive fuel requirements and time".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "distance_parsecs": {
                        "type": "integer",
                        "description": "Distance in parsecs"
                    },
                    "ship_jump_rating": {
                        "type": "integer",
                        "description": "Ship's jump drive rating (1-6)"
                    },
                    "ship_tonnage": {
                        "type": "integer",
                        "description": "Ship's total tonnage"
                    }
                },
                "required": ["distance_parsecs", "ship_jump_rating", "ship_tonnage"]
            }),
        },
        McpToolDefinition {
            name: "traveller_skill_lookup".to_string(),
            description:
                "Look up a Traveller skill's description, characteristic, and specialities"
                    .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Name of the skill"
                    },
                    "speciality": {
                        "type": "string",
                        "description": "Optional speciality"
                    }
                },
                "required": ["skill_name"]
            }),
        },
        McpToolDefinition {
            name: "document_list".to_string(),
            description: "List all available documents (rulebooks, scenarios) with their IDs and titles.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional tags to filter documents"
                    }
                }
            }),
        },
        McpToolDefinition {
            name: "document_find".to_string(),
            description: "Find documents by title (case-insensitive partial match). Returns document IDs and metadata.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "The document title to search for (partial match)"
                    }
                },
                "required": ["title"]
            }),
        },
        McpToolDefinition {
            name: "image_list".to_string(),
            description: "List images from a document. Use document_find first to get the document ID.".to_string(),
            input_schema: serde_json::json!({
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
        },
        McpToolDefinition {
            name: "image_search".to_string(),
            description: "Search for images by description using semantic similarity. Good for finding maps, portraits, deck plans, etc.".to_string(),
            input_schema: serde_json::json!({
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
        },
        McpToolDefinition {
            name: "image_get".to_string(),
            description: "Get detailed information about a specific image by its ID.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "image_id": {
                        "type": "string",
                        "description": "The image ID"
                    }
                },
                "required": ["image_id"]
            }),
        },
        McpToolDefinition {
            name: "image_deliver".to_string(),
            description: "Copy an image to the Foundry VTT assets directory so it can be used in scenes, actors, etc. Returns the FVTT path to use in documents.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "image_id": {
                        "type": "string",
                        "description": "The image ID to deliver"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "Optional: path relative to the assets directory. Default: auto-generated"
                    }
                },
                "required": ["image_id"]
            }),
        },
        McpToolDefinition {
            name: "system_schema".to_string(),
            description: "Get the game system's schema for actors and items.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item"],
                        "description": "Optional: get schema for specific document type"
                    }
                }
            }),
        },
        // ==========================================
        // Traveller Map API Tools
        // ==========================================
        McpToolDefinition {
            name: "traveller_map_search".to_string(),
            description: "Search the Traveller Map for worlds, sectors, or subsectors by name. Returns matching locations with coordinates and basic data.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query - world name, sector name, or subsector name (e.g., 'Regina', 'Spinward Marches')"
                    },
                    "milieu": {
                        "type": "string",
                        "description": "Optional time period/era code (e.g., 'M1105' for 1105 Imperial)"
                    }
                },
                "required": ["query"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_jump_worlds".to_string(),
            description: "Find all worlds within jump range of a specified location. Essential for planning travel routes.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name (e.g., 'Spinward Marches')"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Hex location in XXYY format (e.g., '1910' for Regina)"
                    },
                    "jump": {
                        "type": "integer",
                        "description": "Maximum jump distance in parsecs (1-6 typical)"
                    }
                },
                "required": ["sector", "hex", "jump"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_route".to_string(),
            description: "Calculate the shortest jump route between two locations. Returns worlds along the optimal path.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "start": {
                        "type": "string",
                        "description": "Starting location (e.g., 'Spinward Marches 1910' or 'Regina')"
                    },
                    "end": {
                        "type": "string",
                        "description": "Destination location"
                    },
                    "jump": {
                        "type": "integer",
                        "description": "Ship's jump capability in parsecs (default: 2)"
                    },
                    "wild": {
                        "type": "boolean",
                        "description": "Require wilderness refueling capability"
                    },
                    "imperium_only": {
                        "type": "boolean",
                        "description": "Restrict to Third Imperium worlds"
                    },
                    "no_red_zones": {
                        "type": "boolean",
                        "description": "Avoid TAS Red Zones"
                    }
                },
                "required": ["start", "end"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_world_data".to_string(),
            description: "Get detailed world data including UWP, trade codes, bases, stellar data.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Hex location in XXYY format"
                    }
                },
                "required": ["sector", "hex"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_sector_data".to_string(),
            description: "Get all world data for a sector or subsector.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "subsector": {
                        "type": "string",
                        "description": "Optional subsector (A-P or name)"
                    }
                },
                "required": ["sector"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_coordinates".to_string(),
            description: "Convert sector/hex to world-space coordinates.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Optional hex location"
                    }
                },
                "required": ["sector"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_list_sectors".to_string(),
            description: "List all known sectors in Charted Space.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "milieu": {
                        "type": "string",
                        "description": "Optional time period filter"
                    }
                }
            }),
        },
        McpToolDefinition {
            name: "traveller_map_poster_url".to_string(),
            description: "Generate a URL for a sector or subsector map image.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "subsector": {
                        "type": "string",
                        "description": "Optional subsector"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["poster", "print", "atlas", "candy", "draft", "fasa", "terminal", "mongoose"],
                        "description": "Visual style"
                    }
                },
                "required": ["sector"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_jump_map_url".to_string(),
            description: "Generate a URL for a jump range map centered on a world.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Center hex location"
                    },
                    "jump": {
                        "type": "integer",
                        "description": "Jump range to display"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["poster", "print", "atlas", "candy", "draft", "fasa", "terminal", "mongoose"],
                        "description": "Visual style"
                    }
                },
                "required": ["sector", "hex", "jump"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_save_poster".to_string(),
            description: "Download a sector/subsector map and save to FVTT assets. Returns the FVTT path.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "subsector": {
                        "type": "string",
                        "description": "Optional subsector (A-P or name)"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["poster", "print", "atlas", "candy", "draft", "fasa", "terminal", "mongoose"],
                        "description": "Visual style"
                    },
                    "scale": {
                        "type": "integer",
                        "description": "Pixels per parsec (default: 64)"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "Optional custom path relative to assets"
                    }
                },
                "required": ["sector"]
            }),
        },
        McpToolDefinition {
            name: "traveller_map_save_jump_map".to_string(),
            description: "Download a jump map and save to FVTT assets. Returns the FVTT path.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Center hex location"
                    },
                    "jump": {
                        "type": "integer",
                        "description": "Jump range to display"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["poster", "print", "atlas", "candy", "draft", "fasa", "terminal", "mongoose"],
                        "description": "Visual style"
                    },
                    "scale": {
                        "type": "integer",
                        "description": "Pixels per parsec (default: 64)"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "Optional custom path relative to assets"
                    }
                },
                "required": ["sector", "hex", "jump"]
            }),
        },
        // ==========================================
        // External FVTT Tools (require GM WebSocket connection)
        // ==========================================
        McpToolDefinition {
            name: "fvtt_read".to_string(),
            description: "Read a Foundry VTT document. Requires GM WebSocket connection. Document types: actor (characters, NPCs), item (equipment), journal_entry (notes), scene (maps), rollable_table (random tables).".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of document to read"
                    },
                    "document_id": {
                        "type": "string",
                        "description": "The document ID"
                    }
                },
                "required": ["document_type", "document_id"]
            }),
        },
        McpToolDefinition {
            name: "fvtt_write".to_string(),
            description: "Create or modify a Foundry VTT document. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of document"
                    },
                    "operation": {
                        "type": "string",
                        "enum": ["create", "update", "delete"],
                        "description": "The operation to perform"
                    },
                    "data": {
                        "type": "object",
                        "description": "The document data"
                    }
                },
                "required": ["document_type", "operation", "data"]
            }),
        },
        McpToolDefinition {
            name: "fvtt_query".to_string(),
            description: "Query Foundry VTT documents with filters. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of document to query"
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
            }),
        },
        McpToolDefinition {
            name: "dice_roll".to_string(),
            description: "Roll dice using FVTT's dice system. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
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
            }),
        },
        McpToolDefinition {
            name: "fvtt_assets_browse".to_string(),
            description: "Browse files in Foundry VTT's file system. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to browse (e.g., 'assets', 'assets/seneschal/tokens')"
                    },
                    "source": {
                        "type": "string",
                        "enum": ["data", "public", "s3"],
                        "description": "File source to browse (default: 'data')"
                    },
                    "extensions": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter by file extensions"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "List files in subdirectories (default: false)"
                    }
                }
            }),
        },
        McpToolDefinition {
            name: "image_describe".to_string(),
            description: "Get a vision model description of an FVTT image. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "image_path": {
                        "type": "string",
                        "description": "FVTT path to the image"
                    },
                    "context": {
                        "type": "string",
                        "description": "Context about what the image is for"
                    },
                    "force_refresh": {
                        "type": "boolean",
                        "description": "Bypass cache and regenerate (default: false)"
                    }
                },
                "required": ["image_path"]
            }),
        },
        // Scene CRUD
        McpToolDefinition {
            name: "create_scene".to_string(),
            description: "Create a Foundry VTT scene with a background image. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name for the scene" },
                    "image_path": { "type": "string", "description": "Path to the background image" },
                    "width": { "type": "integer", "description": "Scene width in pixels" },
                    "height": { "type": "integer", "description": "Scene height in pixels" },
                    "grid_size": { "type": "integer", "description": "Grid size in pixels (default: 100)" },
                    "folder": { "type": "string", "description": "Folder to place the scene in" }
                },
                "required": ["name", "image_path"]
            }),
        },
        McpToolDefinition {
            name: "get_scene".to_string(),
            description: "Get a Foundry VTT scene by ID. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scene_id": { "type": "string", "description": "The scene's document ID" }
                },
                "required": ["scene_id"]
            }),
        },
        McpToolDefinition {
            name: "update_scene".to_string(),
            description: "Update an existing Foundry VTT scene. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scene_id": { "type": "string", "description": "The scene's document ID" },
                    "name": { "type": "string", "description": "New name" },
                    "image_path": { "type": "string", "description": "New background image" },
                    "width": { "type": "integer", "description": "Scene width" },
                    "height": { "type": "integer", "description": "Scene height" },
                    "grid_size": { "type": "integer", "description": "Grid size" },
                    "folder": { "type": ["string", "null"], "description": "Folder name to move to, or null to move to root" },
                    "data": { "type": "object", "description": "Additional scene data" }
                },
                "required": ["scene_id"]
            }),
        },
        McpToolDefinition {
            name: "delete_scene".to_string(),
            description: "Delete a Foundry VTT scene. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "scene_id": { "type": "string", "description": "The scene's document ID" }
                },
                "required": ["scene_id"]
            }),
        },
        McpToolDefinition {
            name: "list_scenes".to_string(),
            description: "List scenes in Foundry VTT. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filter by name" },
                    "folder": { "type": "string", "description": "Filter by folder" },
                    "active": { "type": "boolean", "description": "Filter to active scene only" },
                    "limit": { "type": "integer", "description": "Maximum results (default 20)" }
                }
            }),
        },
        // Actor CRUD
        McpToolDefinition {
            name: "create_actor".to_string(),
            description: "Create a Foundry VTT actor. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the actor" },
                    "actor_type": { "type": "string", "description": "Type (e.g., 'character', 'npc')" },
                    "img": { "type": "string", "description": "Portrait image path" },
                    "data": { "type": "object", "description": "Actor system data" },
                    "folder": { "type": "string", "description": "Folder to place actor in" }
                },
                "required": ["name", "actor_type"]
            }),
        },
        McpToolDefinition {
            name: "get_actor".to_string(),
            description: "Get a Foundry VTT actor by ID. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "actor_id": { "type": "string", "description": "The actor's document ID" }
                },
                "required": ["actor_id"]
            }),
        },
        McpToolDefinition {
            name: "update_actor".to_string(),
            description: "Update an existing Foundry VTT actor. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "actor_id": { "type": "string", "description": "The actor's document ID" },
                    "name": { "type": "string", "description": "New name" },
                    "img": { "type": "string", "description": "New portrait" },
                    "folder": { "type": ["string", "null"], "description": "Folder name to move to, or null to move to root" },
                    "data": { "type": "object", "description": "Actor data to update" }
                },
                "required": ["actor_id"]
            }),
        },
        McpToolDefinition {
            name: "delete_actor".to_string(),
            description: "Delete a Foundry VTT actor. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "actor_id": { "type": "string", "description": "The actor's document ID" }
                },
                "required": ["actor_id"]
            }),
        },
        McpToolDefinition {
            name: "list_actors".to_string(),
            description: "List actors in Foundry VTT. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filter by name" },
                    "actor_type": { "type": "string", "description": "Filter by type" },
                    "folder": { "type": "string", "description": "Filter by folder" },
                    "limit": { "type": "integer", "description": "Maximum results (default 20)" }
                }
            }),
        },
        // Item CRUD
        McpToolDefinition {
            name: "create_item".to_string(),
            description: "Create a Foundry VTT item. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the item" },
                    "item_type": { "type": "string", "description": "Type (e.g., 'weapon', 'armor')" },
                    "img": { "type": "string", "description": "Item image path" },
                    "data": { "type": "object", "description": "Item system data" },
                    "folder": { "type": "string", "description": "Folder to place item in" }
                },
                "required": ["name", "item_type"]
            }),
        },
        McpToolDefinition {
            name: "get_item".to_string(),
            description: "Get a Foundry VTT item by ID. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "item_id": { "type": "string", "description": "The item's document ID" }
                },
                "required": ["item_id"]
            }),
        },
        McpToolDefinition {
            name: "update_item".to_string(),
            description: "Update an existing Foundry VTT item. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "item_id": { "type": "string", "description": "The item's document ID" },
                    "name": { "type": "string", "description": "New name" },
                    "img": { "type": "string", "description": "New image" },
                    "folder": { "type": ["string", "null"], "description": "Folder name to move to, or null to move to root" },
                    "data": { "type": "object", "description": "Item data to update" }
                },
                "required": ["item_id"]
            }),
        },
        McpToolDefinition {
            name: "delete_item".to_string(),
            description: "Delete a Foundry VTT item. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "item_id": { "type": "string", "description": "The item's document ID" }
                },
                "required": ["item_id"]
            }),
        },
        McpToolDefinition {
            name: "list_items".to_string(),
            description: "List items in Foundry VTT. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filter by name" },
                    "item_type": { "type": "string", "description": "Filter by type" },
                    "folder": { "type": "string", "description": "Filter by folder" },
                    "limit": { "type": "integer", "description": "Maximum results (default 20)" }
                }
            }),
        },
        // Journal CRUD
        McpToolDefinition {
            name: "create_journal".to_string(),
            description: "Create a Foundry VTT journal entry. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the journal" },
                    "content": { "type": "string", "description": "HTML content for first page" },
                    "img": { "type": "string", "description": "Cover/header image" },
                    "pages": { "type": "array", "description": "Array of page objects", "items": { "type": "object" } },
                    "folder": { "type": "string", "description": "Folder to place journal in" }
                },
                "required": ["name"]
            }),
        },
        McpToolDefinition {
            name: "get_journal".to_string(),
            description: "Get a Foundry VTT journal by ID. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": { "type": "string", "description": "The journal's document ID" }
                },
                "required": ["journal_id"]
            }),
        },
        McpToolDefinition {
            name: "update_journal".to_string(),
            description: "Update an existing Foundry VTT journal. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": { "type": "string", "description": "The journal's document ID" },
                    "name": { "type": "string", "description": "New name" },
                    "folder": { "type": ["string", "null"], "description": "Folder name to move to, or null to move to root" },
                    "content": { "type": "string", "description": "New HTML content" },
                    "pages": { "type": "array", "description": "Updated pages", "items": { "type": "object" } }
                },
                "required": ["journal_id"]
            }),
        },
        McpToolDefinition {
            name: "delete_journal".to_string(),
            description: "Delete a Foundry VTT journal. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": { "type": "string", "description": "The journal's document ID" }
                },
                "required": ["journal_id"]
            }),
        },
        McpToolDefinition {
            name: "list_journals".to_string(),
            description: "List journals in Foundry VTT. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filter by name" },
                    "folder": { "type": "string", "description": "Filter by folder" },
                    "limit": { "type": "integer", "description": "Maximum results (default 20)" }
                }
            }),
        },
        // Journal Page CRUD
        McpToolDefinition {
            name: "add_journal_page".to_string(),
            description: "Add a new page to an existing journal. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": { "type": "string", "description": "The journal's document ID" },
                    "name": { "type": "string", "description": "Name of the new page" },
                    "page_type": { "type": "string", "enum": ["text", "image"], "description": "Type of page" },
                    "content": { "type": "string", "description": "HTML content for text pages" },
                    "src": { "type": "string", "description": "Image path for image pages" },
                    "sort": { "type": "integer", "description": "Sort order value" }
                },
                "required": ["journal_id", "name", "page_type"]
            }),
        },
        McpToolDefinition {
            name: "update_journal_page".to_string(),
            description: "Update a specific page in a journal. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": { "type": "string", "description": "The journal's document ID" },
                    "page_id": { "type": "string", "description": "The page's document ID" },
                    "name": { "type": "string", "description": "New name" },
                    "content": { "type": "string", "description": "New HTML content" },
                    "src": { "type": "string", "description": "New image path" },
                    "sort": { "type": "integer", "description": "New sort order" }
                },
                "required": ["journal_id", "page_id"]
            }),
        },
        McpToolDefinition {
            name: "delete_journal_page".to_string(),
            description: "Delete a specific page from a journal. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": { "type": "string", "description": "The journal's document ID" },
                    "page_id": { "type": "string", "description": "The page's document ID" }
                },
                "required": ["journal_id", "page_id"]
            }),
        },
        McpToolDefinition {
            name: "list_journal_pages".to_string(),
            description: "List all pages in a journal. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": { "type": "string", "description": "The journal's document ID" }
                },
                "required": ["journal_id"]
            }),
        },
        McpToolDefinition {
            name: "reorder_journal_pages".to_string(),
            description: "Bulk reorder pages in a journal by specifying the desired page order. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": { "type": "string", "description": "The journal's document ID" },
                    "page_order": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of page IDs in the desired display order (first ID will appear first)"
                    }
                },
                "required": ["journal_id", "page_order"]
            }),
        },
        // Rollable Table CRUD
        McpToolDefinition {
            name: "create_rollable_table".to_string(),
            description: "Create a Foundry VTT rollable table. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name of the table" },
                    "formula": { "type": "string", "description": "Dice formula (e.g., '1d6', '2d6')" },
                    "results": { "type": "array", "description": "Array of result objects", "items": { "type": "object" } },
                    "img": { "type": "string", "description": "Table image" },
                    "description": { "type": "string", "description": "Table description" },
                    "folder": { "type": "string", "description": "Folder to place table in" }
                },
                "required": ["name", "formula", "results"]
            }),
        },
        McpToolDefinition {
            name: "get_rollable_table".to_string(),
            description: "Get a Foundry VTT rollable table by ID. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "table_id": { "type": "string", "description": "The table's document ID" }
                },
                "required": ["table_id"]
            }),
        },
        McpToolDefinition {
            name: "update_rollable_table".to_string(),
            description: "Update an existing Foundry VTT rollable table. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "table_id": { "type": "string", "description": "The table's document ID" },
                    "name": { "type": "string", "description": "New name" },
                    "formula": { "type": "string", "description": "New dice formula" },
                    "folder": { "type": ["string", "null"], "description": "Folder name to move to, or null to move to root" },
                    "results": { "type": "array", "description": "Updated results", "items": { "type": "object" } }
                },
                "required": ["table_id"]
            }),
        },
        McpToolDefinition {
            name: "delete_rollable_table".to_string(),
            description: "Delete a Foundry VTT rollable table. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "table_id": { "type": "string", "description": "The table's document ID" }
                },
                "required": ["table_id"]
            }),
        },
        McpToolDefinition {
            name: "list_rollable_tables".to_string(),
            description: "List rollable tables in Foundry VTT. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filter by name" },
                    "folder": { "type": "string", "description": "Filter by folder" },
                    "limit": { "type": "integer", "description": "Maximum results (default 20)" }
                }
            }),
        },
        // Folder Management
        McpToolDefinition {
            name: "list_folders".to_string(),
            description: "List folders for a document type in Foundry VTT. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of documents the folders contain"
                    },
                    "parent_folder": { "type": "string", "description": "Filter to child folders" }
                },
                "required": ["document_type"]
            }),
        },
        McpToolDefinition {
            name: "create_folder".to_string(),
            description: "Create a new folder in Foundry VTT. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Folder name" },
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of documents this folder will contain"
                    },
                    "parent_folder": { "type": "string", "description": "Parent folder name" },
                    "color": { "type": "string", "description": "Folder color as hex code" }
                },
                "required": ["name", "document_type"]
            }),
        },
        McpToolDefinition {
            name: "update_folder".to_string(),
            description: "Update a folder's properties. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "folder_id": { "type": "string", "description": "ID of the folder" },
                    "name": { "type": "string", "description": "New name" },
                    "parent_folder": { "type": "string", "description": "New parent folder" },
                    "color": { "type": "string", "description": "New color" }
                },
                "required": ["folder_id"]
            }),
        },
        McpToolDefinition {
            name: "delete_folder".to_string(),
            description: "Delete a folder. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "folder_id": { "type": "string", "description": "ID of the folder" },
                    "delete_contents": { "type": "boolean", "description": "Also delete documents inside (default: false)" }
                },
                "required": ["folder_id"]
            }),
        },
        // User and Ownership Management
        McpToolDefinition {
            name: "list_users".to_string(),
            description: "List all users in the Foundry VTT world. Requires GM WebSocket connection.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "include_inactive": { "type": "boolean", "description": "Include users who are not currently online (default: true)" }
                }
            }),
        },
        McpToolDefinition {
            name: "update_ownership".to_string(),
            description: "Update ownership permissions for a document. Requires GM WebSocket connection. Permission levels: 0=NONE, 1=LIMITED, 2=OBSERVER, 3=OWNER.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "document_type": {
                        "type": "string",
                        "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                        "description": "Type of document"
                    },
                    "document_id": { "type": "string", "description": "The document's ID" },
                    "ownership": {
                        "type": "object",
                        "description": "Ownership mapping. Keys are user IDs or 'default' for base permissions. Values are permission levels: 0=NONE, 1=LIMITED, 2=OBSERVER, 3=OWNER.",
                        "additionalProperties": { "type": "integer", "minimum": 0, "maximum": 3 }
                    }
                },
                "required": ["document_type", "document_id", "ownership"]
            }),
        },
    ]
}
