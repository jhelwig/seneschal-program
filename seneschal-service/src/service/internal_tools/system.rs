//! System-related internal tools.

use crate::service::SeneschalService;
use crate::tools::{ToolCall, ToolResult};

impl SeneschalService {
    pub(crate) fn tool_system_schema(&self, call: &ToolCall) -> ToolResult {
        // Return a placeholder schema - in reality this would come from FVTT
        let schema = serde_json::json!({
            "system": "mgt2e",
            "actorTypes": ["traveller", "npc", "creature", "spacecraft", "vehicle", "world"],
            "itemTypes": ["weapon", "armour", "skill", "term", "equipment"],
            "note": "For detailed schema, query the FVTT client directly"
        });
        ToolResult::success(call.id.clone(), schema)
    }
}
