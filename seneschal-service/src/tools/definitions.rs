//! Tool definitions for Ollama's function calling format.
//!
//! This module contains the tool definition structures and the function
//! that generates all available tool definitions for the LLM.
//!
//! NOTE: Tool definitions are now managed by the unified registry in
//! `crate::tools::registry`. This module provides the struct types
//! and delegates to the registry for the actual definitions.

use serde::{Deserialize, Serialize};

use super::registry::REGISTRY;

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

/// Get tool definitions in Ollama's format.
///
/// This function delegates to the unified tool registry, which serves
/// as the single source of truth for all tool definitions.
pub fn get_ollama_tool_definitions() -> Vec<OllamaToolDefinition> {
    REGISTRY.ollama_definitions()
}
