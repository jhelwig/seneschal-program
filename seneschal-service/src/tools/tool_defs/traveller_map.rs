//! Traveller Map API tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        traveller_map_search(),
        traveller_map_jump_worlds(),
        traveller_map_route(),
        traveller_map_world_data(),
        traveller_map_sector_data(),
        traveller_map_coordinates(),
        traveller_map_list_sectors(),
        traveller_map_poster_url(),
        traveller_map_jump_map_url(),
        traveller_map_save_poster(),
        traveller_map_save_jump_map(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn traveller_map_search() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapSearch,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Search the Traveller Map for worlds, sectors, or subsectors by name. Returns matching locations with their coordinates and basic data.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query - world name, sector name, or subsector name (e.g., 'Regina', 'Spinward Marches')"
                    },
                    "milieu": {
                        "type": "string",
                        "description": "Optional time period/era code (e.g., 'M1105' for 1105 Imperial, 'M1900' for Milieu 0). Defaults to current era."
                    }
                },
                "required": ["query"]
            })
        },
    }
}

fn traveller_map_jump_worlds() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapJumpWorlds,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Find all worlds within jump range of a specified location. Essential for planning travel routes and finding nearby destinations.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name (e.g., 'Spinward Marches', 'Core')"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Hex location in XXYY format (e.g., '1910' for Regina)"
                    },
                    "jump": {
                        "type": "integer",
                        "description": "Maximum jump distance in parsecs (1-6 typical, up to 12 supported)"
                    }
                },
                "required": ["sector", "hex", "jump"]
            })
        },
    }
}

fn traveller_map_route() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapRoute,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Calculate the shortest jump route between two locations. Returns a list of worlds along the optimal path.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "start": {
                        "type": "string",
                        "description": "Starting location (e.g., 'Spinward Marches 1910' or 'Regina')"
                    },
                    "end": {
                        "type": "string",
                        "description": "Destination location (e.g., 'Spinward Marches 2118' or 'Efate')"
                    },
                    "jump": {
                        "type": "integer",
                        "description": "Ship's jump capability in parsecs (default: 2)"
                    },
                    "wild": {
                        "type": "boolean",
                        "description": "If true, require wilderness refueling capability (unrefined fuel)"
                    },
                    "imperium_only": {
                        "type": "boolean",
                        "description": "If true, restrict route to Third Imperium member worlds"
                    },
                    "no_red_zones": {
                        "type": "boolean",
                        "description": "If true, avoid TAS Red Zone systems"
                    }
                },
                "required": ["start", "end"]
            })
        },
    }
}

fn traveller_map_world_data() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapWorldData,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get detailed world data for a specific location including UWP, trade codes, bases, stellar data, and more. More comprehensive than UWP parsing.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name (e.g., 'Spinward Marches')"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Hex location in XXYY format (e.g., '1910')"
                    }
                },
                "required": ["sector", "hex"]
            })
        },
    }
}

fn traveller_map_sector_data() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapSectorData,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get all world data for a sector or subsector. Returns UWP listings for all worlds in the region.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name (e.g., 'Spinward Marches')"
                    },
                    "subsector": {
                        "type": "string",
                        "description": "Optional subsector (A-P letter or name like 'Regina')"
                    }
                },
                "required": ["sector"]
            })
        },
    }
}

fn traveller_map_coordinates() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapCoordinates,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Convert between location formats - sector/hex to world-space coordinates or vice versa.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Optional hex location in XXYY format"
                    }
                },
                "required": ["sector"]
            })
        },
    }
}

fn traveller_map_list_sectors() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapListSectors,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List all known sectors in Charted Space. Can filter by milieu/era.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "milieu": {
                        "type": "string",
                        "description": "Optional time period filter (e.g., 'M1105')"
                    }
                }
            })
        },
    }
}

fn traveller_map_poster_url() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapPosterUrl,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Generate a URL for a sector or subsector map image. Returns a URL that can be used to display or embed the map.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name (e.g., 'Spinward Marches')"
                    },
                    "subsector": {
                        "type": "string",
                        "description": "Optional subsector (A-P letter or name)"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["poster", "print", "atlas", "candy", "draft", "fasa", "terminal", "mongoose"],
                        "description": "Visual style for the map (default: 'poster')"
                    }
                },
                "required": ["sector"]
            })
        },
    }
}

fn traveller_map_jump_map_url() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapJumpMapUrl,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Generate a URL for a jump range map centered on a specific world. Shows all worlds within jump range.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Center hex location in XXYY format"
                    },
                    "jump": {
                        "type": "integer",
                        "description": "Jump range to display (1-6 typical)"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["poster", "print", "atlas", "candy", "draft", "fasa", "terminal", "mongoose"],
                        "description": "Visual style for the map"
                    }
                },
                "required": ["sector", "hex", "jump"]
            })
        },
    }
}

fn traveller_map_save_poster() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapSavePoster,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Download a sector or subsector map from Traveller Map and save it to FVTT assets. Returns the FVTT path for use in journal entries, scenes, etc.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name (e.g., 'Spinward Marches')"
                    },
                    "subsector": {
                        "type": "string",
                        "description": "Optional subsector (A-P letter or name like 'Regina')"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["poster", "print", "atlas", "candy", "draft", "fasa", "terminal", "mongoose"],
                        "description": "Visual style for the map (default: 'poster')"
                    },
                    "scale": {
                        "type": "integer",
                        "description": "Pixels per parsec (default: 64, higher = larger file)"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "Optional: custom path relative to assets directory"
                    }
                },
                "required": ["sector"]
            })
        },
    }
}

fn traveller_map_save_jump_map() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerMapSaveJumpMap,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Download a jump range map centered on a world and save it to FVTT assets. Returns the FVTT path for use in journal entries, scenes, etc.",
        mcp_suffix: None,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Center hex location in XXYY format"
                    },
                    "jump": {
                        "type": "integer",
                        "description": "Jump range to display (1-6 typical)"
                    },
                    "style": {
                        "type": "string",
                        "enum": ["poster", "print", "atlas", "candy", "draft", "fasa", "terminal", "mongoose"],
                        "description": "Visual style for the map"
                    },
                    "scale": {
                        "type": "integer",
                        "description": "Pixels per parsec (default: 64)"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "Optional: custom path relative to assets directory"
                    }
                },
                "required": ["sector", "hex", "jump"]
            })
        },
    }
}
