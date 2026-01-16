//! Unified tool registry for both Ollama (agentic loop) and MCP server.
//!
//! This module provides a single source of truth for all tool definitions.
//! Tool names are derived from enum variants via strum, eliminating any
//! possibility of name mismatches between different consumers.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use super::ToolLocation;

/// All tool names as an exhaustive enum.
///
/// Adding a new tool requires:
/// 1. Add variant here
/// 2. Register metadata in the appropriate definitions module
/// 3. Add handler in execution.rs (compile error if missing due to exhaustive match)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, Serialize, Deserialize)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ToolName {
    // ==========================================
    // Document tools (Internal)
    // ==========================================
    DocumentSearch,
    DocumentSearchText,
    DocumentGet,
    DocumentList,
    DocumentFind,
    DocumentUpdate,

    // ==========================================
    // Image tools (Internal)
    // ==========================================
    ImageList,
    ImageSearch,
    ImageGet,
    ImageDeliver,

    // ==========================================
    // Traveller tools (Internal)
    // ==========================================
    TravellerUwpParse,
    TravellerJumpCalc,
    TravellerSkillLookup,

    // ==========================================
    // Traveller Map API tools (Internal)
    // ==========================================
    TravellerMapSearch,
    TravellerMapJumpWorlds,
    TravellerMapRoute,
    TravellerMapWorldData,
    TravellerMapSectorData,
    TravellerMapCoordinates,
    TravellerMapListSectors,
    TravellerMapPosterUrl,
    TravellerMapJumpMapUrl,
    TravellerMapSavePoster,
    TravellerMapSaveJumpMap,

    // ==========================================
    // System tools (External - requires FVTT)
    // ==========================================
    SystemSchema,

    // ==========================================
    // Generic FVTT tools (External)
    // ==========================================
    FvttRead,
    FvttWrite,
    FvttQuery,
    DiceRoll,

    // ==========================================
    // Asset tools (External)
    // ==========================================
    FvttAssetsBrowse,
    ImageDescribe,

    // ==========================================
    // Folder management (External)
    // ==========================================
    ListFolders,
    CreateFolder,
    UpdateFolder,
    DeleteFolder,

    // ==========================================
    // Scene CRUD (External)
    // ==========================================
    CreateScene,
    GetScene,
    UpdateScene,
    DeleteScene,
    ListScenes,

    // ==========================================
    // Actor CRUD (External)
    // ==========================================
    CreateActor,
    GetActor,
    UpdateActor,
    DeleteActor,
    ListActors,

    // ==========================================
    // Actor Embedded Item CRUD (External)
    // ==========================================
    AddActorItem,
    GetActorItem,
    UpdateActorItem,
    DeleteActorItem,
    ListActorItems,

    // ==========================================
    // Item CRUD (External)
    // ==========================================
    CreateItem,
    GetItem,
    UpdateItem,
    DeleteItem,
    ListItems,

    // ==========================================
    // Journal CRUD (External)
    // ==========================================
    CreateJournal,
    GetJournal,
    UpdateJournal,
    DeleteJournal,
    ListJournals,

    // ==========================================
    // Journal Page CRUD (External)
    // ==========================================
    AddJournalPage,
    UpdateJournalPage,
    DeleteJournalPage,
    ListJournalPages,
    ReorderJournalPages,

    // ==========================================
    // Rollable Table CRUD (External)
    // ==========================================
    CreateRollableTable,
    GetRollableTable,
    UpdateRollableTable,
    DeleteRollableTable,
    ListRollableTables,

    // ==========================================
    // User and Ownership Management (External)
    // ==========================================
    ListUsers,
    UpdateOwnership,

    // ==========================================
    // Compendium Pack Tools (External)
    // ==========================================
    ListCompendiumPacks,
    BrowseCompendiumPack,
    SearchCompendiumPacks,
    ImportFromCompendium,
    ExportToCompendium,

    // ==========================================
    // MCP-specific Tools (Internal)
    // ==========================================
    ToolSearch,
}

/// Metadata for a tool definition.
///
/// The tool name string is derived from the `name` enum variant via strum,
/// ensuring it's impossible to have a mismatch between the enum and the string.
#[derive(Debug, Clone)]
pub struct ToolMetadata {
    /// Tool identifier - string representation derived via strum Display
    pub name: ToolName,

    /// Where the tool executes (backend or FVTT client)
    pub location: ToolLocation,

    /// Whether to expose via Ollama (agentic loop)
    pub ollama_enabled: bool,

    /// Whether to expose via MCP server
    pub mcp_enabled: bool,

    /// Tool description (shared between Ollama and MCP)
    pub description: &'static str,

    /// Optional suffix for MCP description (e.g., "Requires GM WebSocket connection")
    pub mcp_suffix: Option<&'static str>,

    /// Tool category for organizational purposes (e.g., "document", "fvtt_crud", "traveller")
    pub category: &'static str,

    /// Priority for deferred loading (lower = more important)
    /// 0 = always load immediately (tool_search itself)
    /// 1 = high priority (document_search, list_actors, dice_roll)
    /// 2 = normal priority (most tools)
    /// 3 = low priority (specialized tools like traveller_map_*)
    pub priority: u8,

    /// JSON Schema for tool parameters (called lazily to avoid static initialization issues)
    pub parameters: fn() -> serde_json::Value,
}

impl ToolMetadata {
    /// Get the tool name as a string (derived from enum via strum Display)
    #[allow(dead_code)]
    pub fn name_str(&self) -> String {
        self.name.to_string()
    }
}

/// Central registry of all tools.
///
/// Provides methods to generate Ollama and MCP tool definitions from
/// a single source of truth.
pub struct ToolRegistry {
    tools: HashMap<ToolName, ToolMetadata>,
}

impl ToolRegistry {
    /// Build the registry from all registered tool definitions
    pub fn new() -> Self {
        let mut tools = HashMap::new();

        // Register tools from each category module
        super::tool_defs::register_all_tools(&mut tools);

        Self { tools }
    }

    /// Get all tools as Ollama tool definitions
    pub fn ollama_definitions(&self) -> Vec<super::OllamaToolDefinition> {
        self.tools
            .values()
            .filter(|t| t.ollama_enabled)
            .map(|t| super::OllamaToolDefinition {
                tool_type: "function".to_string(),
                function: super::OllamaFunctionDefinition {
                    name: t.name.to_string(),
                    description: t.description.to_string(),
                    parameters: (t.parameters)(),
                },
            })
            .collect()
    }

    /// Get all tools as MCP tool definitions
    pub fn mcp_definitions(&self) -> Vec<McpToolDefinition> {
        self.tools
            .values()
            .filter(|t| t.mcp_enabled)
            .map(|t| {
                let description = match t.mcp_suffix {
                    Some(suffix) => format!("{} {}", t.description, suffix),
                    None => t.description.to_string(),
                };
                // Priority 0-1 tools should not be deferred
                let defer_loading = Some(t.priority > 1);
                McpToolDefinition {
                    name: t.name.to_string(),
                    description,
                    input_schema: (t.parameters)(),
                    defer_loading,
                    category: Some(t.category.to_string()),
                }
            })
            .collect()
    }

    /// Classify a tool by its string name
    ///
    /// Parses the string to a ToolName enum, then looks up in registry.
    /// Unknown tools default to External for safety.
    pub fn classify(&self, name: &str) -> ToolLocation {
        ToolName::from_str(name)
            .ok()
            .and_then(|n| self.tools.get(&n))
            .map(|t| t.location)
            .unwrap_or(ToolLocation::External)
    }

    /// Get metadata by enum variant
    #[allow(dead_code)]
    pub fn get(&self, name: ToolName) -> Option<&ToolMetadata> {
        self.tools.get(&name)
    }

    /// Get metadata by string name
    #[allow(dead_code)]
    pub fn get_by_str(&self, name: &str) -> Option<&ToolMetadata> {
        ToolName::from_str(name)
            .ok()
            .and_then(|n| self.tools.get(&n))
    }

    /// Get the location for a tool by enum variant
    #[allow(dead_code)]
    pub fn location(&self, name: ToolName) -> ToolLocation {
        self.tools
            .get(&name)
            .map(|t| t.location)
            .unwrap_or(ToolLocation::External)
    }

    /// Iterator over all registered tools
    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = (&ToolName, &ToolMetadata)> {
        self.tools.iter()
    }

    /// Number of registered tools
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if registry is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global singleton registry instance
pub static REGISTRY: LazyLock<ToolRegistry> = LazyLock::new(ToolRegistry::new);

/// MCP tool definition structure (for output generation)
#[derive(Debug, Clone, Serialize)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
    /// Hint for clients about whether to defer loading this tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_loading: Option<bool>,
    /// Tool category for organizational purposes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_name_string_conversion() {
        assert_eq!(ToolName::DocumentSearch.to_string(), "document_search");
        assert_eq!(ToolName::FvttRead.to_string(), "fvtt_read");
        assert_eq!(
            ToolName::TravellerMapSearch.to_string(),
            "traveller_map_search"
        );
        assert_eq!(ToolName::CreateActor.to_string(), "create_actor");
    }

    #[test]
    fn test_tool_name_from_string() {
        assert_eq!(
            ToolName::from_str("document_search").unwrap(),
            ToolName::DocumentSearch
        );
        assert_eq!(ToolName::from_str("fvtt_read").unwrap(), ToolName::FvttRead);
        assert!(ToolName::from_str("unknown_tool").is_err());
    }
}
