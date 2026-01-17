//! Traveller RPG-related MCP tool implementations.

use crate::tools::TravellerTool;

use super::super::McpError;

pub(super) fn execute_traveller_uwp_parse(
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let uwp = arguments.get("uwp").and_then(|v| v.as_str()).unwrap_or("");
    let tool = TravellerTool::ParseUwp {
        uwp: uwp.to_string(),
    };

    match tool.execute() {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) fn execute_traveller_jump_calc(
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let distance = arguments
        .get("distance_parsecs")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u8;
    let rating = arguments
        .get("ship_jump_rating")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u8;
    let tonnage = arguments
        .get("ship_tonnage")
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as u32;

    let tool = TravellerTool::JumpCalculation {
        distance_parsecs: distance,
        ship_jump_rating: rating,
        ship_tonnage: tonnage,
    };

    match tool.execute() {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) fn execute_traveller_skill_lookup(
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let skill = arguments
        .get("skill_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let speciality = arguments
        .get("speciality")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let tool = TravellerTool::SkillLookup {
        skill_name: skill.to_string(),
        speciality,
    };

    match tool.execute() {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

pub(super) fn execute_system_schema(
    _arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    // Return a placeholder schema - in reality this would come from FVTT
    let schema = serde_json::json!({
        "system": "mgt2e",
        "actorTypes": ["traveller", "npc", "creature", "spacecraft", "vehicle", "world"],
        "itemTypes": ["weapon", "armour", "skill", "term", "equipment"],
        "note": "For detailed schema, query the FVTT client directly"
    });

    let text = serde_json::to_string_pretty(&schema).unwrap_or_default();

    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": text
        }]
    }))
}
