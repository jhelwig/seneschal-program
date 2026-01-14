//! Traveller RPG-specific tool definitions.

use std::collections::HashMap;

use crate::tools::{registry::{ToolMetadata, ToolName}, ToolLocation};

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        traveller_uwp_parse(),
        traveller_jump_calc(),
        traveller_skill_lookup(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn traveller_uwp_parse() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerUwpParse,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Parse a Traveller UWP (Universal World Profile) string into detailed world data.",
        mcp_suffix: None,
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "uwp": {
                    "type": "string",
                    "description": "UWP string (e.g., 'A867949-C')"
                }
            },
            "required": ["uwp"]
        }),
    }
}

fn traveller_jump_calc() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerJumpCalc,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Calculate jump drive fuel requirements and time.",
        mcp_suffix: None,
        parameters: || serde_json::json!({
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
    }
}

fn traveller_skill_lookup() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::TravellerSkillLookup,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Look up a Traveller skill's description, characteristic, and specialities.",
        mcp_suffix: None,
        parameters: || serde_json::json!({
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
    }
}
