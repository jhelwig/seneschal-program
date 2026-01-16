//! Tool definitions organized by category.
//!
//! Each submodule defines tools for a specific category and provides
//! a registration function that adds them to the registry.

mod document;
mod fvtt_crud;
mod fvtt_system;
mod image;
mod mcp;
mod traveller;
mod traveller_map;

use std::collections::HashMap;

use super::registry::{ToolMetadata, ToolName};

/// Register all tools from all categories into the registry.
pub fn register_all_tools(registry: &mut HashMap<ToolName, ToolMetadata>) {
    document::register(registry);
    image::register(registry);
    traveller::register(registry);
    traveller_map::register(registry);
    fvtt_system::register(registry);
    fvtt_crud::register(registry);
    mcp::register(registry);
}
