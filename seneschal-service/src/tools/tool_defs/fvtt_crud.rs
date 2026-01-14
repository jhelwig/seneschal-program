//! FVTT CRUD tool definitions for documents (scenes, actors, items, journals, tables).

use std::collections::HashMap;

use crate::tools::{registry::{ToolMetadata, ToolName}, ToolLocation};

/// Suffix added to external tool descriptions for MCP
const EXTERNAL_MCP_SUFFIX: &str = "Requires GM WebSocket connection.";

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    // Scene CRUD
    register_scene_tools(registry);
    // Actor CRUD
    register_actor_tools(registry);
    // Item CRUD
    register_item_tools(registry);
    // Journal CRUD
    register_journal_tools(registry);
    // Journal Page CRUD
    register_journal_page_tools(registry);
    // Rollable Table CRUD
    register_rollable_table_tools(registry);
    // Compendium Pack tools
    register_compendium_tools(registry);
}

// ==========================================
// Scene CRUD
// ==========================================

fn register_scene_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_scene(),
        get_scene(),
        update_scene(),
        delete_scene(),
        list_scenes(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_scene() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateScene,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a Foundry VTT scene with a background image. Use image_deliver first to get the image path. Can create in world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name for the scene"
                },
                "image_path": {
                    "type": "string",
                    "description": "Path to the background image (from image_deliver)"
                },
                "width": {
                    "type": "integer",
                    "description": "Scene width in pixels (default: from image)"
                },
                "height": {
                    "type": "integer",
                    "description": "Scene height in pixels (default: from image)"
                },
                "grid_size": {
                    "type": "integer",
                    "description": "Grid size in pixels (default: 100)"
                },
                "folder": {
                    "type": "string",
                    "description": "Name of folder to place the scene in"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to create in (e.g., 'world.my-scenes'). If omitted, creates in world."
                }
            },
            "required": ["name", "image_path"]
        }),
    }
}

fn get_scene() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetScene,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a Foundry VTT scene by ID. Returns the scene's configuration. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "scene_id": {
                    "type": "string",
                    "description": "The scene's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to read from. If omitted, reads from world."
                }
            },
            "required": ["scene_id"]
        }),
    }
}

fn update_scene() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateScene,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT scene. Can modify name, background, dimensions, grid settings, etc. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "scene_id": {
                    "type": "string",
                    "description": "The scene's document ID"
                },
                "name": {
                    "type": "string",
                    "description": "New name for the scene"
                },
                "image_path": {
                    "type": "string",
                    "description": "New background image path"
                },
                "width": {
                    "type": "integer",
                    "description": "Scene width in pixels"
                },
                "height": {
                    "type": "integer",
                    "description": "Scene height in pixels"
                },
                "grid_size": {
                    "type": "integer",
                    "description": "Grid size in pixels"
                },
                "folder": {
                    "type": ["string", "null"],
                    "description": "Folder name or ID to move the scene to. Use null to move to root level."
                },
                "data": {
                    "type": "object",
                    "description": "Additional scene data to update (advanced)"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to update in. If omitted, updates in world."
                }
            },
            "required": ["scene_id"]
        }),
    }
}

fn delete_scene() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteScene,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a Foundry VTT scene permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "scene_id": {
                    "type": "string",
                    "description": "The scene's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                }
            },
            "required": ["scene_id"]
        }),
    }
}

fn list_scenes() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListScenes,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List scenes in Foundry VTT. Can filter by name pattern. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Filter by name (partial match)"
                },
                "folder": {
                    "type": "string",
                    "description": "Filter by folder name"
                },
                "active": {
                    "type": "boolean",
                    "description": "Filter to only the currently active scene (world only)"
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
        }),
    }
}

// ==========================================
// Actor CRUD
// ==========================================

fn register_actor_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_actor(),
        get_actor(),
        update_actor(),
        delete_actor(),
        list_actors(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_actor() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateActor,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a Foundry VTT actor (character, NPC, creature, vehicle, etc.). Use system_schema first to understand the actor types and data structure. Can create in world or compendium. Biography/notes fields support cross-document links: @UUID[Type.ID]{Label}.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the actor"
                },
                "actor_type": {
                    "type": "string",
                    "description": "Type of actor (e.g., 'character', 'npc', 'creature', 'vehicle' - varies by game system)"
                },
                "img": {
                    "type": "string",
                    "description": "Path to the actor's portrait image (use image_deliver first)"
                },
                "data": {
                    "type": "object",
                    "description": "Actor system data (stats, attributes, etc. - use system_schema to see structure)"
                },
                "folder": {
                    "type": "string",
                    "description": "Name of folder to place the actor in"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to create in. If omitted, creates in world."
                }
            },
            "required": ["name", "actor_type"]
        }),
    }
}

fn get_actor() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetActor,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a Foundry VTT actor by ID. Returns the actor's complete data. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "actor_id": {
                    "type": "string",
                    "description": "The actor's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to read from. If omitted, reads from world."
                }
            },
            "required": ["actor_id"]
        }),
    }
}

fn update_actor() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateActor,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT actor. Can modify name, image, stats, or any system data. Works with world or compendium (if unlocked). Biography/notes fields support cross-document links: @UUID[Type.ID]{Label}.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "actor_id": {
                    "type": "string",
                    "description": "The actor's document ID"
                },
                "name": {
                    "type": "string",
                    "description": "New name for the actor"
                },
                "img": {
                    "type": "string",
                    "description": "New portrait image path"
                },
                "folder": {
                    "type": ["string", "null"],
                    "description": "Folder name or ID to move the actor to. Use null to move to root level."
                },
                "data": {
                    "type": "object",
                    "description": "Actor system data to update (stats, attributes, etc.)"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to update in. If omitted, updates in world."
                }
            },
            "required": ["actor_id"]
        }),
    }
}

fn delete_actor() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteActor,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a Foundry VTT actor permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "actor_id": {
                    "type": "string",
                    "description": "The actor's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                }
            },
            "required": ["actor_id"]
        }),
    }
}

fn list_actors() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListActors,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List actors in Foundry VTT. Can filter by name pattern. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Filter by name (partial match)"
                },
                "actor_type": {
                    "type": "string",
                    "description": "Filter by actor type (e.g., 'character', 'npc')"
                },
                "folder": {
                    "type": "string",
                    "description": "Filter by folder name"
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
        }),
    }
}

// ==========================================
// Item CRUD
// ==========================================

fn register_item_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_item(),
        get_item(),
        update_item(),
        delete_item(),
        list_items(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateItem,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a Foundry VTT item (weapon, armor, equipment, skill, spell, etc.). Use system_schema first to understand item types. Can create in world or compendium. Description fields support cross-document links: @UUID[Type.ID]{Label}.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the item"
                },
                "item_type": {
                    "type": "string",
                    "description": "Type of item (e.g., 'weapon', 'armor', 'equipment', 'skill' - varies by game system)"
                },
                "img": {
                    "type": "string",
                    "description": "Path to the item's image (use image_deliver first)"
                },
                "data": {
                    "type": "object",
                    "description": "Item system data (damage, weight, cost, etc. - use system_schema to see structure)"
                },
                "folder": {
                    "type": "string",
                    "description": "Name of folder to place the item in"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to create in. If omitted, creates in world."
                }
            },
            "required": ["name", "item_type"]
        }),
    }
}

fn get_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetItem,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a Foundry VTT item by ID. Returns the item's complete data. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "item_id": {
                    "type": "string",
                    "description": "The item's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to read from. If omitted, reads from world."
                }
            },
            "required": ["item_id"]
        }),
    }
}

fn update_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateItem,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT item. Can modify name, image, or any system data. Works with world or compendium (if unlocked). Description fields support cross-document links: @UUID[Type.ID]{Label}.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "item_id": {
                    "type": "string",
                    "description": "The item's document ID"
                },
                "name": {
                    "type": "string",
                    "description": "New name for the item"
                },
                "img": {
                    "type": "string",
                    "description": "New item image path"
                },
                "folder": {
                    "type": ["string", "null"],
                    "description": "Folder name or ID to move the item to. Use null to move to root level."
                },
                "data": {
                    "type": "object",
                    "description": "Item system data to update"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to update in. If omitted, updates in world."
                }
            },
            "required": ["item_id"]
        }),
    }
}

fn delete_item() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteItem,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a Foundry VTT item permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "item_id": {
                    "type": "string",
                    "description": "The item's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                }
            },
            "required": ["item_id"]
        }),
    }
}

fn list_items() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListItems,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List items in Foundry VTT. Can filter by name pattern or item type. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Filter by name (partial match)"
                },
                "item_type": {
                    "type": "string",
                    "description": "Filter by item type (e.g., 'weapon', 'armor')"
                },
                "folder": {
                    "type": "string",
                    "description": "Filter by folder name"
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
        }),
    }
}

// ==========================================
// Journal CRUD
// ==========================================

fn register_journal_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_journal(),
        get_journal(),
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
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a Foundry VTT journal for notes, handouts, or lore. Journals can have multiple pages with text or images. Can create in world or compendium. Rich text supports cross-document links: @UUID[Type.ID]{Label} (e.g., @UUID[Actor.abc123]{Guard Captain}). Types: Actor, Item, JournalEntry, Scene, RollTable.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
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
                    "description": "Name of folder to place the journal entry in"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to create in. If omitted, creates in world."
                }
            },
            "required": ["name"]
        }),
    }
}

fn get_journal() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetJournal,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a Foundry VTT journal by ID. Returns the journal's pages and content. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
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
        }),
    }
}

fn update_journal() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateJournal,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT journal. Can modify name, content, or pages. Works with world or compendium (if unlocked). Rich text supports cross-document links: @UUID[Type.ID]{Label} (e.g., @UUID[Actor.abc123]{Guard Captain}). Types: Actor, Item, JournalEntry, Scene, RollTable.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
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
        }),
    }
}

fn delete_journal() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteJournal,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a Foundry VTT journal permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
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
        }),
    }
}

fn list_journals() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListJournals,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List journals in Foundry VTT. Can filter by name pattern. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Filter by name (partial match)"
                },
                "folder": {
                    "type": "string",
                    "description": "Filter by folder name"
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
        }),
    }
}

// ==========================================
// Journal Page CRUD
// ==========================================

fn register_journal_page_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        add_journal_page(),
        update_journal_page(),
        delete_journal_page(),
        list_journal_pages(),
        reorder_journal_pages(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn add_journal_page() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::AddJournalPage,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Add a new page to an existing Foundry VTT journal. Use get_journal first to see existing pages. Works with world or compendium (if unlocked). Rich text supports cross-document links: @UUID[Type.ID]{Label} (e.g., @UUID[Actor.abc123]{Guard Captain}). Types: Actor, Item, JournalEntry, Scene, RollTable.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "journal_id": {
                    "type": "string",
                    "description": "The journal's document ID"
                },
                "name": {
                    "type": "string",
                    "description": "Name of the new page"
                },
                "page_type": {
                    "type": "string",
                    "enum": ["text", "image"],
                    "description": "Type of page: 'text' for HTML content, 'image' for an image page"
                },
                "content": {
                    "type": "string",
                    "description": "HTML content for text pages"
                },
                "src": {
                    "type": "string",
                    "description": "Image path for image pages (use image_deliver first)"
                },
                "sort": {
                    "type": "integer",
                    "description": "Sort order value (optional, appends to end if omitted)"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                }
            },
            "required": ["journal_id", "name", "page_type"]
        }),
    }
}

fn update_journal_page() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateJournalPage,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update a specific page in a Foundry VTT journal. Use get_journal or list_journal_pages first to find page IDs. Works with world or compendium (if unlocked). Rich text supports cross-document links: @UUID[Type.ID]{Label} (e.g., @UUID[Actor.abc123]{Guard Captain}). Types: Actor, Item, JournalEntry, Scene, RollTable.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "journal_id": {
                    "type": "string",
                    "description": "The journal's document ID"
                },
                "page_id": {
                    "type": "string",
                    "description": "The page's document ID"
                },
                "name": {
                    "type": "string",
                    "description": "New name for the page"
                },
                "content": {
                    "type": "string",
                    "description": "New HTML content (for text pages)"
                },
                "src": {
                    "type": "string",
                    "description": "New image path (for image pages)"
                },
                "sort": {
                    "type": "integer",
                    "description": "New sort order value"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                }
            },
            "required": ["journal_id", "page_id"]
        }),
    }
}

fn delete_journal_page() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteJournalPage,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a specific page from a Foundry VTT journal. Use get_journal or list_journal_pages first to find page IDs. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "journal_id": {
                    "type": "string",
                    "description": "The journal's document ID"
                },
                "page_id": {
                    "type": "string",
                    "description": "The page's document ID to delete"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                }
            },
            "required": ["journal_id", "page_id"]
        }),
    }
}

fn list_journal_pages() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListJournalPages,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List all pages in a Foundry VTT journal. Returns page IDs, names, types, and sort order. Works with world or compendium journals.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "journal_id": {
                    "type": "string",
                    "description": "The journal's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                }
            },
            "required": ["journal_id"]
        }),
    }
}

fn reorder_journal_pages() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ReorderJournalPages,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Bulk reorder pages in a Foundry VTT journal. Provide an array of page IDs in the desired display order. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "journal_id": {
                    "type": "string",
                    "description": "The journal's document ID"
                },
                "page_order": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Array of page IDs in the desired display order (first ID will appear first)"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID containing the journal. If omitted, targets world journal."
                }
            },
            "required": ["journal_id", "page_order"]
        }),
    }
}

// ==========================================
// Rollable Table CRUD
// ==========================================

fn register_rollable_table_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        create_rollable_table(),
        get_rollable_table(),
        update_rollable_table(),
        delete_rollable_table(),
        list_rollable_tables(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn create_rollable_table() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::CreateRollableTable,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Create a Foundry VTT rollable table for random encounters, loot, events, etc. Can create in world or compendium. Results can be text, document links, or compendium references.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the rollable table"
                },
                "formula": {
                    "type": "string",
                    "description": "Dice formula for the table (e.g., '1d6', '2d6', '1d100')"
                },
                "results": {
                    "type": "array",
                    "description": "Array of result objects. Text: {range: [low, high], text, weight, img}. Document: {type: 'document', document_collection: 'Actor'|'Item'|'JournalEntry'|'Scene'|'RollTable', document_id, range, weight, img, text?} - text is optional, defaults to linked document's name. Compendium: {type: 'compendium', document_collection: 'pack.name', document_id, range, weight, img, text?} - text optional, defaults to compendium entry's name.",
                    "items": {
                        "type": "object"
                    }
                },
                "img": {
                    "type": "string",
                    "description": "Optional table image"
                },
                "description": {
                    "type": "string",
                    "description": "Description of what this table is for"
                },
                "folder": {
                    "type": "string",
                    "description": "Name of folder to place the table in"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to create in. If omitted, creates in world."
                }
            },
            "required": ["name", "formula", "results"]
        }),
    }
}

fn get_rollable_table() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::GetRollableTable,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Get a Foundry VTT rollable table by ID. Returns the table's formula and all results. Can read from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "table_id": {
                    "type": "string",
                    "description": "The rollable table's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to read from. If omitted, reads from world."
                }
            },
            "required": ["table_id"]
        }),
    }
}

fn update_rollable_table() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::UpdateRollableTable,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Update an existing Foundry VTT rollable table. Can modify name, formula, or results. Works with world or compendium (if unlocked). Results can be text, document links, or compendium references.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "table_id": {
                    "type": "string",
                    "description": "The rollable table's document ID"
                },
                "name": {
                    "type": "string",
                    "description": "New name for the table"
                },
                "formula": {
                    "type": "string",
                    "description": "New dice formula"
                },
                "folder": {
                    "type": ["string", "null"],
                    "description": "Folder name or ID to move the table to. Use null to move to root level."
                },
                "results": {
                    "type": "array",
                    "description": "Updated results array. Text: {range: [low, high], text, weight, img}. Document: {type: 'document', document_collection: 'Actor'|'Item'|etc, document_id, range, weight, img, text?} - text optional, defaults to linked document's name. Compendium: {type: 'compendium', document_collection: 'pack.name', document_id, range, weight, img, text?} - text optional, defaults to entry's name.",
                    "items": {
                        "type": "object"
                    }
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to update in. If omitted, updates in world."
                }
            },
            "required": ["table_id"]
        }),
    }
}

fn delete_rollable_table() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::DeleteRollableTable,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Delete a Foundry VTT rollable table permanently. Works with world or compendium (if unlocked).",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "table_id": {
                    "type": "string",
                    "description": "The rollable table's document ID"
                },
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to delete from. If omitted, deletes from world."
                }
            },
            "required": ["table_id"]
        }),
    }
}

fn list_rollable_tables() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListRollableTables,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List rollable tables in Foundry VTT. Can filter by name pattern. Lists from world or compendium.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Filter by name (partial match)"
                },
                "folder": {
                    "type": "string",
                    "description": "Filter by folder name"
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
        }),
    }
}

// ==========================================
// Compendium Pack Tools
// ==========================================

fn register_compendium_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [
        list_compendium_packs(),
        browse_compendium_pack(),
        search_compendium_packs(),
        import_from_compendium(),
        export_to_compendium(),
    ];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn list_compendium_packs() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ListCompendiumPacks,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List available compendium packs. Compendiums store reusable documents (actors, items, journals, etc.) outside the world.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "document_type": {
                    "type": "string",
                    "enum": ["Actor", "Item", "JournalEntry", "Scene", "RollTable", "Macro", "Playlist"],
                    "description": "Filter by document type stored in the pack"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results (default 50)"
                }
            }
        }),
    }
}

fn browse_compendium_pack() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::BrowseCompendiumPack,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "List documents in a compendium pack. Uses lightweight index for fast browsing without loading full documents.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID (e.g., 'dnd5e.monsters', 'world.my-pack')"
                },
                "name": {
                    "type": "string",
                    "description": "Filter by document name (partial match)"
                },
                "folder": {
                    "type": "string",
                    "description": "Filter by folder name within the pack"
                },
                "offset": {
                    "type": "integer",
                    "description": "Skip first N results for pagination (default 0)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results (default 50)"
                }
            },
            "required": ["pack_id"]
        }),
    }
}

fn search_compendium_packs() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::SearchCompendiumPacks,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Search for documents across all compendium packs by name.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search term to match against document names"
                },
                "document_type": {
                    "type": "string",
                    "enum": ["Actor", "Item", "JournalEntry", "Scene", "RollTable", "Macro", "Playlist"],
                    "description": "Filter to packs containing this document type"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results (default 50)"
                }
            },
            "required": ["query"]
        }),
    }
}

fn import_from_compendium() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ImportFromCompendium,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Import documents from a compendium pack into the world. Creates copies in the world collection.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to import from"
                },
                "document_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Array of document IDs to import. Omit to import all."
                },
                "folder": {
                    "type": "string",
                    "description": "World folder name to place imported documents in"
                },
                "keep_id": {
                    "type": "boolean",
                    "description": "Preserve original document IDs (default false, can cause conflicts)"
                },
                "keep_folders": {
                    "type": "boolean",
                    "description": "Recreate folder structure from compendium (default false)"
                }
            },
            "required": ["pack_id"]
        }),
    }
}

fn export_to_compendium() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::ExportToCompendium,
        location: ToolLocation::External,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Export documents from the world to a compendium pack. Pack must be unlocked.",
        mcp_suffix: Some(EXTERNAL_MCP_SUFFIX),
        parameters: || serde_json::json!({
            "type": "object",
            "properties": {
                "pack_id": {
                    "type": "string",
                    "description": "Compendium pack ID to export to (must be unlocked)"
                },
                "document_type": {
                    "type": "string",
                    "enum": ["Actor", "Item", "JournalEntry", "Scene", "RollTable", "Macro", "Playlist"],
                    "description": "Type of documents to export"
                },
                "document_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Array of world document IDs to export"
                },
                "keep_id": {
                    "type": "boolean",
                    "description": "Preserve document IDs in compendium (default false)"
                },
                "keep_folders": {
                    "type": "boolean",
                    "description": "Recreate folder structure in compendium (default false)"
                }
            },
            "required": ["pack_id", "document_type", "document_ids"]
        }),
    }
}
