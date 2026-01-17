//! Basic Traveller internal tools (UWP parsing, jump calculation, skill lookup).

use crate::service::SeneschalService;
use crate::tools::{ToolCall, ToolResult, TravellerTool};

impl SeneschalService {
    pub(crate) fn tool_traveller_uwp_parse(&self, call: &ToolCall) -> ToolResult {
        let uwp = call.args.get("uwp").and_then(|v| v.as_str()).unwrap_or("");
        let tool = TravellerTool::ParseUwp {
            uwp: uwp.to_string(),
        };

        match tool.execute() {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) fn tool_traveller_jump_calc(&self, call: &ToolCall) -> ToolResult {
        let distance = call
            .args
            .get("distance_parsecs")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u8;
        let rating = call
            .args
            .get("ship_jump_rating")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u8;
        let tonnage = call
            .args
            .get("ship_tonnage")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as u32;

        let tool = TravellerTool::JumpCalculation {
            distance_parsecs: distance,
            ship_jump_rating: rating,
            ship_tonnage: tonnage,
        };

        match tool.execute() {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }

    pub(crate) fn tool_traveller_skill_lookup(&self, call: &ToolCall) -> ToolResult {
        let skill = call
            .args
            .get("skill_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let speciality = call
            .args
            .get("speciality")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let tool = TravellerTool::SkillLookup {
            skill_name: skill.to_string(),
            speciality,
        };

        match tool.execute() {
            Ok(result) => ToolResult::success(call.id.clone(), result),
            Err(e) => ToolResult::error(call.id.clone(), e),
        }
    }
}
