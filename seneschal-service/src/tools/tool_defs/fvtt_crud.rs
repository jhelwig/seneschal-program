//! FVTT CRUD tool definitions for documents (scenes, actors, items, journals, tables).

mod actor;
mod actor_item;
mod compendium;
mod item;
mod journal;
mod journal_page;
mod rollable_table;
mod scene;

use std::collections::HashMap;

use crate::tools::registry::{ToolMetadata, ToolName};

/// Suffix added to external tool descriptions for MCP
const EXTERNAL_MCP_SUFFIX: &str = "Requires GM WebSocket connection.";

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    scene::register(registry);
    actor::register(registry);
    actor_item::register(registry);
    item::register(registry);
    journal::register(registry);
    journal_page::register(registry);
    rollable_table::register(registry);
    compendium::register(registry);
}
