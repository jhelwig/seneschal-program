//! Traveller Worlds tool definitions.
//!
//! Tools for generating world maps using travellerworlds.com via headless browser.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        traveller_worlds_canon_url(),
        traveller_worlds_canon_save(),
        traveller_worlds_custom_url(),
        traveller_worlds_custom_save(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn traveller_worlds_canon_url() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerWorldsCanonUrl,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Generate a travellerworlds.com URL for a canon Traveller world. Fetches world data from Traveller Map API and constructs the URL with all parameters. Useful for previewing before saving.",
        mcp_suffix: None,
        category: "traveller_worlds",
        priority: 3, // Specialized tool
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
                        "description": "Hex coordinate in XXYY format (e.g., '1232' for Walston)"
                    }
                },
                "required": ["sector", "hex"]
            })
        },
    }
}

fn traveller_worlds_canon_save() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerWorldsCanonSave,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Generate a world map SVG for a canon Traveller world using travellerworlds.com and save it to FVTT assets. Fetches world data from Traveller Map API, generates the map via headless browser, and saves the SVG.",
        mcp_suffix: Some("Requires geckodriver running."),
        category: "traveller_worlds",
        priority: 3,
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
                        "description": "Hex coordinate in XXYY format (e.g., '1232' for Walston)"
                    },
                    "target_folder": {
                        "type": "string",
                        "description": "FVTT assets subfolder (default: 'traveller-worlds')"
                    }
                },
                "required": ["sector", "hex"]
            })
        },
    }
}

fn traveller_worlds_custom_url() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerWorldsCustomUrl,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Generate a travellerworlds.com URL for a custom/homebrew world with manual parameters. Useful for previewing before saving.",
        mcp_suffix: None,
        category: "traveller_worlds",
        priority: 3,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "World name"
                    },
                    "uwp": {
                        "type": "string",
                        "description": "Universal World Profile (e.g., 'B434ABD-B')"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Hex coordinate (used for seed if provided)"
                    },
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "seed": {
                        "type": "string",
                        "description": "Random seed for reproducibility (default: hex+hex or random)"
                    },
                    "stellar": {
                        "type": "string",
                        "description": "Stellar data (e.g., 'F7 V M2 V')"
                    },
                    "bases": {
                        "type": "string",
                        "description": "Base codes: N (Naval), S (Scout), W (Way Station), D (Depot)"
                    },
                    "tc": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Trade classifications (e.g., ['In', 'Hi', 'Ht'])"
                    },
                    "travel_zone": {
                        "type": "string",
                        "description": "'A' (Amber), 'R' (Red), or blank for Green"
                    },
                    "pbg": {
                        "type": "string",
                        "description": "Population-Belts-Gas Giants code (3 digits)"
                    }
                },
                "required": ["name", "uwp"]
            })
        },
    }
}

fn traveller_worlds_custom_save() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerWorldsCustomSave,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Generate a world map SVG for a custom/homebrew world using travellerworlds.com and save it to FVTT assets. Uses headless browser to extract the generated SVG.",
        mcp_suffix: Some("Requires geckodriver running."),
        category: "traveller_worlds",
        priority: 3,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "World name"
                    },
                    "uwp": {
                        "type": "string",
                        "description": "Universal World Profile (e.g., 'B434ABD-B')"
                    },
                    "hex": {
                        "type": "string",
                        "description": "Hex coordinate (used for seed if provided)"
                    },
                    "sector": {
                        "type": "string",
                        "description": "Sector name"
                    },
                    "seed": {
                        "type": "string",
                        "description": "Random seed for reproducibility (default: hex+hex or random)"
                    },
                    "stellar": {
                        "type": "string",
                        "description": "Stellar data (e.g., 'F7 V M2 V')"
                    },
                    "bases": {
                        "type": "string",
                        "description": "Base codes: N (Naval), S (Scout), W (Way Station), D (Depot)"
                    },
                    "tc": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Trade classifications (e.g., ['In', 'Hi', 'Ht'])"
                    },
                    "travel_zone": {
                        "type": "string",
                        "description": "'A' (Amber), 'R' (Red), or blank for Green"
                    },
                    "pbg": {
                        "type": "string",
                        "description": "Population-Belts-Gas Giants code (3 digits)"
                    },
                    "target_folder": {
                        "type": "string",
                        "description": "FVTT assets subfolder (default: 'traveller-worlds')"
                    }
                },
                "required": ["name", "uwp"]
            })
        },
    }
}
