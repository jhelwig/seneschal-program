//! MCP message handlers.
//!
//! Handlers for initialize and tools/list requests.

use super::{McpError, McpState, McpToolDefinition};

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
pub async fn handle_tools_list(_state: &McpState) -> Result<serde_json::Value, McpError> {
    let tools = vec![
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
    ];

    Ok(serde_json::json!({ "tools": tools }))
}
