//! Tool definitions and types for the agentic loop.
//!
//! This module contains:
//! - Core tool types (ToolCall, ToolResult)
//! - Access level definitions
//! - Tool classification (internal vs external)
//! - Submodules for tool definitions and game-specific tools

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod definitions;
pub mod traveller;

pub use definitions::{OllamaToolDefinition, get_ollama_tool_definitions};
pub use traveller::TravellerTool;

/// Access levels aligned with FVTT user roles
/// Values correspond to minimum required role to access
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Default,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum AccessLevel {
    Player = 1,    // Anyone with at least Player role
    Trusted = 2,   // Trusted players and above
    Assistant = 3, // Assistant GMs (may need scenario prep materials)
    #[default]
    GmOnly = 4, // Full GM only
}

impl AccessLevel {
    /// Check if this access level is accessible by a user with the given role
    pub fn accessible_by(&self, user_role: u8) -> bool {
        user_role >= *self as u8
    }

    /// Convert from u8, defaulting to GmOnly for invalid values
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => AccessLevel::Player,
            2 => AccessLevel::Trusted,
            3 => AccessLevel::Assistant,
            _ => AccessLevel::GmOnly,
        }
    }
}

/// Tag matching strategy for search filters
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TagMatch {
    #[default]
    Any, // Any of the specified tags
    All, // All of the specified tags
}

/// Search filters
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SearchFilters {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tags_match: TagMatch,
}

/// Tool call from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool: String,
    pub args: serde_json::Value,
}

/// Tool result to return to the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    #[serde(flatten)]
    pub outcome: ToolOutcome,
}

/// Tool execution outcome
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolOutcome {
    Success { result: serde_json::Value },
    Error { error: String },
}

impl ToolResult {
    pub fn success(tool_call_id: String, result: serde_json::Value) -> Self {
        Self {
            tool_call_id,
            outcome: ToolOutcome::Success { result },
        }
    }

    pub fn error(tool_call_id: String, error: String) -> Self {
        Self {
            tool_call_id,
            outcome: ToolOutcome::Error { error },
        }
    }
}

/// Classify whether a tool is internal (backend-only) or external (requires client)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolLocation {
    Internal,
    External,
}

pub fn classify_tool(tool_name: &str) -> ToolLocation {
    match tool_name {
        // Internal tools - executed by the backend
        "document_search"
        | "document_search_text"
        | "document_get"
        | "document_list"
        | "document_find"
        | "image_list"
        | "image_search"
        | "image_get"
        | "image_deliver"
        | "system_schema"
        | "traveller_uwp_parse"
        | "traveller_jump_calc"
        | "traveller_skill_lookup" => ToolLocation::Internal,

        // Generic FVTT tools
        "fvtt_read" | "fvtt_write" | "fvtt_query" | "dice_roll" => ToolLocation::External,

        // Asset browsing and image description (two-phase external)
        "fvtt_assets_browse" | "image_describe" => ToolLocation::External,

        // Folder management
        "list_folders" | "create_folder" | "update_folder" | "delete_folder" => {
            ToolLocation::External
        }

        // Scene CRUD
        "create_scene" | "get_scene" | "update_scene" | "delete_scene" | "list_scenes" => {
            ToolLocation::External
        }

        // Actor CRUD
        "create_actor" | "get_actor" | "update_actor" | "delete_actor" | "list_actors" => {
            ToolLocation::External
        }

        // Item CRUD
        "create_item" | "get_item" | "update_item" | "delete_item" | "list_items" => {
            ToolLocation::External
        }

        // Journal CRUD
        "create_journal"
        | "get_journal"
        | "update_journal"
        | "delete_journal"
        | "list_journals"
        // Journal Page CRUD
        | "add_journal_page"
        | "update_journal_page"
        | "delete_journal_page"
        | "list_journal_pages" => ToolLocation::External,

        // Rollable Table CRUD
        "create_rollable_table"
        | "get_rollable_table"
        | "update_rollable_table"
        | "delete_rollable_table"
        | "list_rollable_tables" => ToolLocation::External,

        // Unknown tools go to client for safety
        _ => ToolLocation::External,
    }
}
