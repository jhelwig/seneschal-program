//! Tool definitions for Ollama's function calling format.
//!
//! This module contains the tool definition structures and the function
//! that generates all available tool definitions for the LLM.

use serde::{Deserialize, Serialize};

/// Tool definition for Ollama's tool format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OllamaFunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Get tool definitions in Ollama's format
pub fn get_ollama_tool_definitions() -> Vec<OllamaToolDefinition> {
    vec![
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_search".to_string(),
                description: "Search game documents (rulebooks, scenarios) for information using semantic similarity. Good for conceptual queries like 'how do jump drives work' or 'rules for combat'. Returns relevant text chunks.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional tags to filter results"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default 10)"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_search_text".to_string(),
                description: "Search documents using exact keyword matching. Use this for specific names, terms, or when semantic search doesn't find what you need. Supports filtering by section (e.g., 'Adventure 1'). Good for finding specific characters like 'Anders Casarii' or references within a particular section.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Keywords to search for (exact matching)"
                        },
                        "section": {
                            "type": "string",
                            "description": "Optional: filter to content within this section (e.g., 'Adventure 1')"
                        },
                        "document_id": {
                            "type": "string",
                            "description": "Optional: limit search to a specific document"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default 10)"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_get".to_string(),
                description: "Get document metadata or retrieve the full text content of a specific page. Use 'page' parameter to read page content - this is the primary way to read specific pages from rulebooks and scenarios.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_id": {
                            "type": "string",
                            "description": "The document ID (get from document_list or document_find)"
                        },
                        "page": {
                            "type": "integer",
                            "description": "Page number to retrieve. If specified, returns the full text content of that page. If omitted, returns document metadata only."
                        }
                    },
                    "required": ["document_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_list".to_string(),
                description: "List all available documents (rulebooks, scenarios) with their IDs and titles.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional tags to filter documents"
                        }
                    }
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_find".to_string(),
                description: "Find documents by title (case-insensitive partial match). Returns document IDs and metadata.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "The document title to search for (partial match)"
                        }
                    },
                    "required": ["title"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_list".to_string(),
                description: "List images from a document. Use document_find first to get the document ID, then use this to browse images from specific pages or page ranges.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_id": {
                            "type": "string",
                            "description": "The document ID"
                        },
                        "start_page": {
                            "type": "integer",
                            "description": "Optional: filter to images starting from this page number"
                        },
                        "end_page": {
                            "type": "integer",
                            "description": "Optional: filter to images up to and including this page number"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum images to return (default 20)"
                        }
                    },
                    "required": ["document_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_search".to_string(),
                description: "Search for images by description using semantic similarity. Good for finding maps, portraits, deck plans, etc.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Description of the image to find"
                        },
                        "document_id": {
                            "type": "string",
                            "description": "Optional: limit search to a specific document"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum results (default 10)"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_get".to_string(),
                description: "Get detailed information about a specific image by its ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "image_id": {
                            "type": "string",
                            "description": "The image ID"
                        }
                    },
                    "required": ["image_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_deliver".to_string(),
                description: "Copy an image to the Foundry VTT assets directory so it can be used in scenes, actors, etc. Returns the full FVTT path (starting with 'assets/') to use in documents.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "image_id": {
                            "type": "string",
                            "description": "The image ID to deliver"
                        },
                        "target_path": {
                            "type": "string",
                            "description": "Optional: path relative to the assets directory, e.g., 'seneschal/tokens/guard.webp'. Do NOT include 'assets/' prefix. Default: auto-generated as 'seneschal/{doc_title}/page_{N}.webp'"
                        }
                    },
                    "required": ["image_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "fvtt_read".to_string(),
                description: "Read a Foundry VTT document. Document types: actor (characters, NPCs, creatures), item (weapons, armor, equipment), journal_entry (notes, handouts), scene (maps/battlemaps where tokens are placed), rollable_table (random tables).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                            "description": "actor=characters/NPCs, item=equipment, journal_entry=notes, scene=map/battlemap, rollable_table=random table"
                        },
                        "document_id": {
                            "type": "string",
                            "description": "The document ID"
                        }
                    },
                    "required": ["document_type", "document_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "fvtt_write".to_string(),
                description: "Create or modify a Foundry VTT document. Document types: actor (characters, NPCs, creatures), item (weapons, armor, equipment), journal_entry (notes, handouts), scene (maps/battlemaps - use with image_deliver to create maps from PDF images), rollable_table (random tables).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                            "description": "actor=characters/NPCs, item=equipment, journal_entry=notes, scene=map/battlemap, rollable_table=random table"
                        },
                        "operation": {
                            "type": "string",
                            "enum": ["create", "update", "delete"],
                            "description": "The operation to perform"
                        },
                        "data": {
                            "type": "object",
                            "description": "The document data. For scenes: {name, background: {src: 'path/to/image.webp'}, width, height, grid: {size, type}}"
                        }
                    },
                    "required": ["document_type", "operation", "data"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "fvtt_query".to_string(),
                description: "Query Foundry VTT documents with filters. Document types: actor (characters, NPCs), item (equipment), journal_entry (notes), scene (maps/battlemaps), rollable_table (random tables).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                            "description": "actor=characters/NPCs, item=equipment, journal_entry=notes, scene=map/battlemap, rollable_table=random table"
                        },
                        "filters": {
                            "type": "object",
                            "description": "Query filters (e.g., {name: 'Marcus'})"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum results (default 20)"
                        }
                    },
                    "required": ["document_type"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "create_scene".to_string(),
                description: "Create a Foundry VTT scene with a background image. Use image_deliver first to get the image path.".to_string(),
                parameters: serde_json::json!({
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
                        }
                    },
                    "required": ["name", "image_path"]
                }),
            },
        },
        // Scene CRUD (create_scene above, get/update/delete below)
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "get_scene".to_string(),
                description: "Get a Foundry VTT scene by ID. Returns the scene's configuration including background, dimensions, grid settings, and placed tokens/drawings.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "scene_id": {
                            "type": "string",
                            "description": "The scene's document ID"
                        }
                    },
                    "required": ["scene_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "update_scene".to_string(),
                description: "Update an existing Foundry VTT scene. Can modify name, background, dimensions, grid settings, etc.".to_string(),
                parameters: serde_json::json!({
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
                        "data": {
                            "type": "object",
                            "description": "Additional scene data to update (advanced)"
                        }
                    },
                    "required": ["scene_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "delete_scene".to_string(),
                description: "Delete a Foundry VTT scene permanently.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "scene_id": {
                            "type": "string",
                            "description": "The scene's document ID"
                        }
                    },
                    "required": ["scene_id"]
                }),
            },
        },
        // Actor CRUD
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "create_actor".to_string(),
                description: "Create a Foundry VTT actor (character, NPC, creature, vehicle, etc.). Use system_schema first to understand the actor types and data structure for the current game system.".to_string(),
                parameters: serde_json::json!({
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
                        }
                    },
                    "required": ["name", "actor_type"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "get_actor".to_string(),
                description: "Get a Foundry VTT actor by ID. Returns the actor's complete data including stats, items, and effects.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "actor_id": {
                            "type": "string",
                            "description": "The actor's document ID"
                        }
                    },
                    "required": ["actor_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "update_actor".to_string(),
                description: "Update an existing Foundry VTT actor. Can modify name, image, stats, or any system data.".to_string(),
                parameters: serde_json::json!({
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
                        "data": {
                            "type": "object",
                            "description": "Actor system data to update (stats, attributes, etc.)"
                        }
                    },
                    "required": ["actor_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "delete_actor".to_string(),
                description: "Delete a Foundry VTT actor permanently.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "actor_id": {
                            "type": "string",
                            "description": "The actor's document ID"
                        }
                    },
                    "required": ["actor_id"]
                }),
            },
        },
        // Item CRUD
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "create_item".to_string(),
                description: "Create a Foundry VTT item (weapon, armor, equipment, skill, spell, etc.). Use system_schema first to understand the item types and data structure for the current game system.".to_string(),
                parameters: serde_json::json!({
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
                        }
                    },
                    "required": ["name", "item_type"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "get_item".to_string(),
                description: "Get a Foundry VTT item by ID. Returns the item's complete data including stats and effects.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "item_id": {
                            "type": "string",
                            "description": "The item's document ID"
                        }
                    },
                    "required": ["item_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "update_item".to_string(),
                description: "Update an existing Foundry VTT item. Can modify name, image, or any system data.".to_string(),
                parameters: serde_json::json!({
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
                        "data": {
                            "type": "object",
                            "description": "Item system data to update"
                        }
                    },
                    "required": ["item_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "delete_item".to_string(),
                description: "Delete a Foundry VTT item permanently.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "item_id": {
                            "type": "string",
                            "description": "The item's document ID"
                        }
                    },
                    "required": ["item_id"]
                }),
            },
        },
        // Journal Entry CRUD
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "create_journal_entry".to_string(),
                description: "Create a Foundry VTT journal entry for notes, handouts, or lore. Journal entries can have multiple pages with text or images.".to_string(),
                parameters: serde_json::json!({
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
                        }
                    },
                    "required": ["name"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "get_journal_entry".to_string(),
                description: "Get a Foundry VTT journal entry by ID. Returns the journal's pages and content.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "journal_id": {
                            "type": "string",
                            "description": "The journal entry's document ID"
                        }
                    },
                    "required": ["journal_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "update_journal_entry".to_string(),
                description: "Update an existing Foundry VTT journal entry. Can modify name, content, or pages.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "journal_id": {
                            "type": "string",
                            "description": "The journal entry's document ID"
                        },
                        "name": {
                            "type": "string",
                            "description": "New name for the journal"
                        },
                        "content": {
                            "type": "string",
                            "description": "New HTML content (for simple single-page journals)"
                        },
                        "pages": {
                            "type": "array",
                            "description": "Updated pages array",
                            "items": {
                                "type": "object"
                            }
                        }
                    },
                    "required": ["journal_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "delete_journal_entry".to_string(),
                description: "Delete a Foundry VTT journal entry permanently.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "journal_id": {
                            "type": "string",
                            "description": "The journal entry's document ID"
                        }
                    },
                    "required": ["journal_id"]
                }),
            },
        },
        // Rollable Table CRUD
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "create_rollable_table".to_string(),
                description: "Create a Foundry VTT rollable table for random encounters, loot, events, etc.".to_string(),
                parameters: serde_json::json!({
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
                            "description": "Array of result objects: [{range: [low, high], text, weight, img}]",
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
                        }
                    },
                    "required": ["name", "formula", "results"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "get_rollable_table".to_string(),
                description: "Get a Foundry VTT rollable table by ID. Returns the table's formula and all results.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "table_id": {
                            "type": "string",
                            "description": "The rollable table's document ID"
                        }
                    },
                    "required": ["table_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "update_rollable_table".to_string(),
                description: "Update an existing Foundry VTT rollable table. Can modify name, formula, or results.".to_string(),
                parameters: serde_json::json!({
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
                        "results": {
                            "type": "array",
                            "description": "Updated results array",
                            "items": {
                                "type": "object"
                            }
                        }
                    },
                    "required": ["table_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "delete_rollable_table".to_string(),
                description: "Delete a Foundry VTT rollable table permanently.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "table_id": {
                            "type": "string",
                            "description": "The rollable table's document ID"
                        }
                    },
                    "required": ["table_id"]
                }),
            },
        },
        // List/Query tools for each document type
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "list_actors".to_string(),
                description: "List actors in Foundry VTT. Can filter by name pattern or other properties.".to_string(),
                parameters: serde_json::json!({
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
                        }
                    }
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "list_items".to_string(),
                description: "List items in Foundry VTT. Can filter by name pattern or item type.".to_string(),
                parameters: serde_json::json!({
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
                        }
                    }
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "list_journal_entries".to_string(),
                description: "List journal entries in Foundry VTT. Can filter by name pattern.".to_string(),
                parameters: serde_json::json!({
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
                        }
                    }
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "list_scenes".to_string(),
                description: "List scenes in Foundry VTT. Can filter by name pattern.".to_string(),
                parameters: serde_json::json!({
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
                            "description": "Filter to only the currently active scene"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum results (default 20)"
                        }
                    }
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "list_rollable_tables".to_string(),
                description: "List rollable tables in Foundry VTT. Can filter by name pattern.".to_string(),
                parameters: serde_json::json!({
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
                        }
                    }
                }),
            },
        },
        // Folder management tools
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "list_folders".to_string(),
                description: "List all folders for a specific document type in Foundry VTT.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["Actor", "Item", "JournalEntry", "Scene", "RollTable"],
                            "description": "Type of documents the folders contain"
                        },
                        "parent_folder": {
                            "type": "string",
                            "description": "Filter to only show folders inside this parent folder"
                        }
                    },
                    "required": ["document_type"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "create_folder".to_string(),
                description: "Create a new folder for organizing documents in Foundry VTT.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Name of the folder"
                        },
                        "document_type": {
                            "type": "string",
                            "enum": ["Actor", "Item", "JournalEntry", "Scene", "RollTable"],
                            "description": "Type of documents this folder will contain"
                        },
                        "parent_folder": {
                            "type": "string",
                            "description": "Name of parent folder for nesting (optional)"
                        },
                        "color": {
                            "type": "string",
                            "description": "Folder color as hex code (e.g., '#ff0000')"
                        }
                    },
                    "required": ["name", "document_type"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "update_folder".to_string(),
                description: "Update a folder's properties (rename, move, or change color).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "folder_id": {
                            "type": "string",
                            "description": "ID of the folder to update"
                        },
                        "name": {
                            "type": "string",
                            "description": "New name for the folder"
                        },
                        "parent_folder": {
                            "type": "string",
                            "description": "New parent folder name (use null to move to root)"
                        },
                        "color": {
                            "type": "string",
                            "description": "New color as hex code"
                        }
                    },
                    "required": ["folder_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "delete_folder".to_string(),
                description: "Delete a folder. By default, documents inside are moved to root level.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "folder_id": {
                            "type": "string",
                            "description": "ID of the folder to delete"
                        },
                        "delete_contents": {
                            "type": "boolean",
                            "description": "If true, also delete all documents inside the folder (default: false)"
                        }
                    },
                    "required": ["folder_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "dice_roll".to_string(),
                description: "Roll dice using FVTT's dice system. Results are logged to the game.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formula": {
                            "type": "string",
                            "description": "Dice formula (e.g., '2d6+2', '1d20')"
                        },
                        "label": {
                            "type": "string",
                            "description": "Optional label for the roll"
                        }
                    },
                    "required": ["formula"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "system_schema".to_string(),
                description: "Get the game system's schema for actors and items.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["actor", "item"],
                            "description": "Optional: get schema for specific document type"
                        }
                    }
                }),
            },
        },
        // Traveller-specific tools
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "traveller_uwp_parse".to_string(),
                description: "Parse a Traveller UWP (Universal World Profile) string into detailed world data.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "uwp": {
                            "type": "string",
                            "description": "UWP string (e.g., 'A867949-C')"
                        }
                    },
                    "required": ["uwp"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "traveller_jump_calc".to_string(),
                description: "Calculate jump drive fuel requirements and time.".to_string(),
                parameters: serde_json::json!({
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
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "traveller_skill_lookup".to_string(),
                description: "Look up a Traveller skill's description, characteristic, and specialities.".to_string(),
                parameters: serde_json::json!({
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
            },
        },
        // FVTT Asset Tools
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "fvtt_assets_browse".to_string(),
                description: "Browse files in Foundry VTT's file system. Returns a list of files and directories at the specified path.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to browse (e.g., 'assets', 'assets/seneschal/tokens'). Defaults to root."
                        },
                        "source": {
                            "type": "string",
                            "enum": ["data", "public", "s3"],
                            "description": "File source to browse (default: 'data')"
                        },
                        "extensions": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Filter by file extensions (e.g., ['.webp', '.png', '.jpg'])"
                        },
                        "recursive": {
                            "type": "boolean",
                            "description": "If true, also list files in subdirectories (default: false)"
                        }
                    }
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_describe".to_string(),
                description: "Get a detailed vision model description of an image file in FVTT. Uses the configured vision model to analyze the image. Results are cached.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "image_path": {
                            "type": "string",
                            "description": "FVTT path to the image (e.g., 'assets/tokens/guard.webp')"
                        },
                        "context": {
                            "type": "string",
                            "description": "Optional context about what the image is for (e.g., 'NPC portrait for a tavern encounter')"
                        },
                        "force_refresh": {
                            "type": "boolean",
                            "description": "If true, bypass cache and generate a new description (default: false)"
                        }
                    },
                    "required": ["image_path"]
                }),
            },
        },
    ]
}
