//! Tool definitions organized by category.
//!
//! Each submodule defines tools for a specific category and provides
//! a registration function that adds them to the registry.

mod document;
mod fvtt_crud;
mod fvtt_system;
mod image;
mod mcp;
mod rendering;
mod traveller;
mod traveller_map;
mod traveller_worlds;

use std::collections::HashMap;

use super::registry::{ToolMetadata, ToolName};

/// Register all tools from all categories into the registry.
pub fn register_all_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    document::register(registry);
    image::register(registry);
    rendering::register(registry);
    traveller::register(registry);
    traveller_map::register(registry);
    traveller_worlds::register(registry);
    fvtt_system::register(registry);
    fvtt_crud::register(registry);
    mcp::register(registry);
}
