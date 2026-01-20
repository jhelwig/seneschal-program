//! Page rendering tool definitions.
//!
//! These tools allow rendering specific regions or full pages of uploaded PDF documents
//! at configurable DPI. Useful for extracting vector graphics, diagrams, and other content
//! that isn't captured as individual images during extraction.

use std::collections::HashMap;

use crate::tools::{
    ToolLocation,
    registry::{ToolMetadata, ToolName},
};

pub fn register(registry: &mut HashMap<ToolName, ToolMetadata>) {
    let tools = [render_page_region(), render_full_page()];
    for tool in tools {
        registry.insert(tool.name, tool);
    }
}

fn render_page_region() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::RenderPageRegion,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Render a specific region of a PDF page at configurable DPI. Useful for extracting vector graphics, diagrams, or other content composed from paths rather than embedded images. Returns an image that can be delivered to FVTT with image_deliver. Coordinates are in PDF points (72 points = 1 inch) with origin at bottom-left of page.",
        mcp_suffix: None,
        category: "rendering",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "The document ID (use document_find to get the ID)"
                    },
                    "page": {
                        "type": "integer",
                        "description": "Page number (1-indexed)"
                    },
                    "x1": {
                        "type": "number",
                        "description": "Left edge of region in PDF points (72 points = 1 inch)"
                    },
                    "y1": {
                        "type": "number",
                        "description": "Bottom edge of region in PDF points"
                    },
                    "x2": {
                        "type": "number",
                        "description": "Right edge of region in PDF points"
                    },
                    "y2": {
                        "type": "number",
                        "description": "Top edge of region in PDF points"
                    },
                    "dpi": {
                        "type": "number",
                        "description": "Render DPI (default: 150, use 300 for high-quality prints)"
                    }
                },
                "required": ["document_id", "page", "x1", "y1", "x2", "y2"]
            })
        },
    }
}

fn render_full_page() -> ToolMetadata {
    ToolMetadata {
        name: ToolName::RenderFullPage,
        location: ToolLocation::Internal,
        ollama_enabled: true,
        mcp_enabled: true,
        description: "Render a full PDF page at configurable DPI. Useful for extracting entire pages as images, including all vector graphics and text. Returns an image that can be delivered to FVTT with image_deliver.",
        mcp_suffix: None,
        category: "rendering",
        priority: 2,
        parameters: || {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "document_id": {
                        "type": "string",
                        "description": "The document ID (use document_find to get the ID)"
                    },
                    "page": {
                        "type": "integer",
                        "description": "Page number (1-indexed)"
                    },
                    "dpi": {
                        "type": "number",
                        "description": "Render DPI (default: 150, use 300 for high-quality prints)"
                    }
                },
                "required": ["document_id", "page"]
            })
        },
    }
}
