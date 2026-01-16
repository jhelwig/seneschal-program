//! BM25-based tool search for MCP clients.
//!
//! This module provides natural language search over available tools,
//! enabling clients to discover relevant tools based on queries like
//! "create an actor" or "search documents".

use std::sync::LazyLock;

use bm25::{Document, Language, SearchEngine, SearchEngineBuilder};

use crate::tools::registry::REGISTRY;

/// BM25 search index for tool discovery.
pub struct ToolSearchIndex {
    /// BM25 search engine keyed by tool name
    engine: SearchEngine<String>,
}

impl ToolSearchIndex {
    /// Build a new search index from MCP-enabled tools in the registry.
    ///
    /// Each tool is indexed as a document containing:
    /// - Tool name (with underscores converted to spaces)
    /// - Description
    /// - Parameter names and descriptions
    pub fn new() -> Self {
        let mut documents: Vec<Document<String>> = Vec::new();

        // Get all MCP-enabled tools from the registry
        let mcp_tools = REGISTRY.mcp_definitions();

        for tool in mcp_tools {
            // Build searchable content by concatenating:
            // 1. Tool name (with underscores as spaces for better matching)
            // 2. Description
            // 3. Parameter names and descriptions from the input schema
            let name_for_search = tool.name.replace('_', " ");
            let mut content = format!("{} {}", name_for_search, tool.description);

            // Extract parameter info from input_schema
            if let Some(properties) = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
            {
                for (param_name, param_schema) in properties {
                    content.push(' ');
                    content.push_str(&param_name.replace('_', " "));
                    if let Some(desc) = param_schema.get("description").and_then(|d| d.as_str()) {
                        content.push(' ');
                        content.push_str(desc);
                    }
                }
            }

            documents.push(Document {
                id: tool.name.clone(),
                contents: content,
            });
        }

        // Build the BM25 search engine
        let engine: SearchEngine<String> =
            SearchEngineBuilder::with_documents(Language::English, documents).build();

        Self { engine }
    }

    /// Search for tools matching the given natural language query.
    ///
    /// Returns up to `limit` tool names sorted by relevance.
    pub fn search(&self, query: &str, limit: usize) -> Vec<String> {
        if query.is_empty() {
            return Vec::new();
        }

        self.engine
            .search(query, limit)
            .into_iter()
            .map(|result| result.document.id.clone())
            .collect()
    }
}

impl Default for ToolSearchIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Global singleton tool search index.
///
/// Lazily initialized on first access. Since the tool registry is static,
/// this index is built once and reused for all searches.
pub static TOOL_SEARCH_INDEX: LazyLock<ToolSearchIndex> = LazyLock::new(ToolSearchIndex::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_index_builds() {
        // Force index construction
        let _ = &*TOOL_SEARCH_INDEX;
        // If we get here without panic, the index built successfully
    }

    #[test]
    fn test_search_document_tools() {
        let results = TOOL_SEARCH_INDEX.search("search documents", 5);
        assert!(!results.is_empty(), "Should find document-related tools");

        assert!(
            results.contains(&"document_search".to_string()),
            "document_search should be found for 'search documents' query"
        );
    }

    #[test]
    fn test_search_actor_tools() {
        let results = TOOL_SEARCH_INDEX.search("create actor character", 5);
        assert!(!results.is_empty(), "Should find actor-related tools");

        assert!(
            results.contains(&"create_actor".to_string()),
            "create_actor should be found for actor-related query"
        );
    }

    #[test]
    fn test_empty_query() {
        let results = TOOL_SEARCH_INDEX.search("", 5);
        assert!(results.is_empty(), "Empty query should return no results");
    }
}
