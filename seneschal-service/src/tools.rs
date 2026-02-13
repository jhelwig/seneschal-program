//! Tool definitions and types for MCP.
//!
//! This module contains:
//! - Access level definitions
//! - Tool classification (internal vs external)
//! - Unified tool registry for MCP
//! - Submodules for tool definitions and game-specific tools

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod registry;
pub mod tool_defs;
pub mod traveller;
pub mod traveller_map;
pub mod traveller_worlds;

pub use registry::REGISTRY;
pub use traveller::TravellerTool;
pub use traveller_map::{TravellerMapClient, TravellerMapTool};
pub use traveller_worlds::{CustomWorldParams, TravellerWorldsClient};

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

/// Classify whether a tool is internal (backend-only) or external (requires client)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolLocation {
    Internal,
    External,
}

/// Classify whether a tool is internal (backend-only) or external (requires FVTT client).
///
/// This function delegates to the unified tool registry, which serves
/// as the single source of truth for tool classification.
pub fn classify_tool(tool_name: &str) -> ToolLocation {
    REGISTRY.classify(tool_name)
}
