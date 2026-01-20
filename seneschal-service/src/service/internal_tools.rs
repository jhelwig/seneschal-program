//! Internal tool execution for the agentic loop.
//!
//! This module handles execution of all internal tools (tools that run on the
//! backend server rather than in the FVTT client).

mod documents;
mod images;
mod rendering;
mod system;
mod traveller_basic;
mod traveller_map_api;
mod traveller_worlds;

use crate::tools::{ToolCall, ToolResult};

use super::SeneschalService;
use super::state::UserContext;

/// Sanitize a string for use in a filename (for Traveller Map assets)
pub(crate) fn sanitize_map_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ' ' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

impl SeneschalService {
    /// Execute an internal tool
    pub(crate) async fn execute_internal_tool(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        match call.tool.as_str() {
            // Document tools
            "document_search" => self.tool_document_search(call, user_context).await,
            "document_search_text" => self.tool_document_search_text(call, user_context),
            "document_get" => self.tool_document_get(call, user_context),
            "document_list" => self.tool_document_list(call, user_context),
            "document_find" => self.tool_document_find(call, user_context),
            "document_update" => self.tool_document_update(call, user_context),

            // Image tools
            "image_list" => self.tool_image_list(call, user_context),
            "image_search" => self.tool_image_search(call, user_context).await,
            "image_get" => self.tool_image_get(call, user_context),
            "image_deliver" => self.tool_image_deliver(call, user_context),

            // Page rendering tools
            "render_page_region" => self.tool_render_page_region(call, user_context),
            "render_full_page" => self.tool_render_full_page(call, user_context),

            // System tools
            "system_schema" => self.tool_system_schema(call),

            // Basic Traveller tools
            "traveller_uwp_parse" => self.tool_traveller_uwp_parse(call),
            "traveller_jump_calc" => self.tool_traveller_jump_calc(call),
            "traveller_skill_lookup" => self.tool_traveller_skill_lookup(call),

            // Traveller Map API tools
            "traveller_map_search" => self.tool_traveller_map_search(call).await,
            "traveller_map_jump_worlds" => self.tool_traveller_map_jump_worlds(call).await,
            "traveller_map_route" => self.tool_traveller_map_route(call).await,
            "traveller_map_world_data" => self.tool_traveller_map_world_data(call).await,
            "traveller_map_sector_data" => self.tool_traveller_map_sector_data(call).await,
            "traveller_map_coordinates" => self.tool_traveller_map_coordinates(call).await,
            "traveller_map_list_sectors" => self.tool_traveller_map_list_sectors(call).await,
            "traveller_map_poster_url" => self.tool_traveller_map_poster_url(call).await,
            "traveller_map_jump_map_url" => self.tool_traveller_map_jump_map_url(call).await,
            "traveller_map_save_poster" => self.tool_traveller_map_save_poster(call).await,
            "traveller_map_save_jump_map" => self.tool_traveller_map_save_jump_map(call).await,

            // Traveller Worlds tools
            "traveller_worlds_canon_url" => self.tool_traveller_worlds_canon_url(call).await,
            "traveller_worlds_canon_save" => self.tool_traveller_worlds_canon_save(call).await,
            "traveller_worlds_custom_url" => self.tool_traveller_worlds_custom_url(call),
            "traveller_worlds_custom_save" => self.tool_traveller_worlds_custom_save(call).await,

            _ => ToolResult::error(
                call.id.clone(),
                format!("Unknown internal tool: {}", call.tool),
            ),
        }
    }
}
