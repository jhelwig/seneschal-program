//! Journal CRUD tool definitions.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

use super::EXTERNAL_MCP_SUFFIX;

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_journal(),
        get_journal(),
        get_journals(),
        update_journal(),
        delete_journal(),
        list_journals(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_journal() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateJournal,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Create a Foundry VTT journal for notes, handouts, or lore. Journals can have multiple pages with text or images. Can create in world or compendium. Rich text supports cross-document links: @UUID[Type.ID]{Label} (e.g., @UUID[Actor.abc123]{Guard Captain}). Types: Actor, Item, JournalEntry, Scene, RollTable.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the journal entry"
                    },
                    "content": {
                        "type": "string",
                        "description": "HTML content for the first page (simple entries)"
                    },
                    "img": {
                        "type": "string",
                        "description": "Optional cover/header image"
                    },
                    "pages": {
                        "type": "array",
                        "description": "Array of page objects for multi-page journals: [{name, type: 'text'|'image', text: {content}, src}]",
                        "items": {
                            "type": "object"
                        }
                    },
                    "folder": {
                        "type": "string",
                        "description": "Folder name or ID to place the journal entry in"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to create in. If omitted, creates in world."
                    }
                },
                "required": ["name"]
            })
        },
    }
}

fn get_journal() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetJournal,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Get a Foundry VTT journal by ID. Returns journal metadata and page list (without page content). Use get_journal_page or get_journal_pages to retrieve page content. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to read from. If omitted, reads from world."
                    }
                },
                "required": ["journal_id"]
            })
        },
    }
}

fn get_journals() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetJournals,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Get multiple journals by ID in one call. Returns journal metadata and page list (without page content) for each. Maximum 20 journals per call. Use get_journal_page or get_journal_pages to retrieve page content.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of journal IDs to retrieve (max 20)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to read from. If omitted, reads from world."
                    }
                },
                "required": ["journal_ids"]
            })
        },
    }
}

fn update_journal() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateJournal,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT journal. Can modify name, content, or pages. Works with world or compendium (if unlocked). Rich text supports cross-document links: @UUID[Type.ID]{Label} (e.g., @UUID[Actor.abc123]{Guard Captain}). Types: Actor, Item, JournalEntry, Scene, RollTable.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "name": {
                        "type": "string",
                        "description": "New name for the journal"
                    },
                    "content": {
                        "type": "string",
                        "description": "New HTML content (for simple single-page journals)"
                    },
                    "folder": {
                        "type": ["string", "null"],
                        "description": "Folder name or ID to move the journal to. Use null to move to root level."
                    },
                    "pages": {
                        "type": "array",
                        "description": "Updated pages array",
                        "items": {
                            "type": "object"
                        }
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to update in. If omitted, updates in world."
                    }
                },
                "required": ["journal_id"]
            })
        },
    }
}

fn delete_journal() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteJournal,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "Delete a Foundry VTT journal permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "journal_id": {
                        "type": "string",
                        "description": "The journal's document ID"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                    }
                },
                "required": ["journal_id"]
            })
        },
    }
}

fn list_journals() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListJournals,
        location: ToolLocation::External,
        mcp_enabled: true,
        description: "List journals in Foundry VTT. Can filter by name pattern. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        category: "fvtt_crud",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Filter by name (partial match)"
                    },
                    "folder": {
                        "type": "string",
                        "description": "Filter by folder name or ID"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results (default 20)"
                    },
                    "pack_id": {
                        "type": "string",
                        "description": "Compendium pack ID to list from. If omitted, lists from world."
                    }
                }
            })
        },
    }
}
