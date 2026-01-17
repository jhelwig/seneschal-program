//! Document-related MCP tool implementations.

use crate::search::format_search_results_for_llm;
use crate::tools::{SearchFilters, TagMatch};

use super::super::{McpError, McpState};

pub(super) async fn execute_document_search(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tags: Vec<String> = arguments
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    let filters = if tags.is_empty() {
        None
    } else {
        Some(SearchFilters {
            tags,
            tags_match: TagMatch::Any,
        })
    };

    match state.service.search(query, gm_role, limit, filters).await {
        Ok(results) => {
            let formatted = format_search_results_for_llm(&results, &state.service.i18n, "en");
            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": formatted
                }]
            }))
        }
        Err(e) => Err(McpError {
            code: -32000,
            message: e.to_string(),
        }),
    }
}

pub(super) fn execute_document_search_text(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let section = arguments.get("section").and_then(|v| v.as_str());
    let document_id = arguments.get("document_id").and_then(|v| v.as_str());
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    match state
        .service
        .db
        .search_chunks_fts(query, section, document_id, gm_role, limit)
    {
        Ok(chunks) => {
            let results: Vec<serde_json::Value> = chunks
                .into_iter()
                .map(|c| {
                    serde_json::json!({
                        "document_id": c.document_id,
                        "page_number": c.page_number,
                        "section_title": c.section_title,
                        "content": c.content,
                    })
                })
                .collect();

            let text = if results.is_empty() {
                format!("No matches found for '{}'", query)
            } else {
                serde_json::to_string_pretty(&results).unwrap_or_default()
            };

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        Err(e) => Err(McpError {
            code: -32000,
            message: e.to_string(),
        }),
    }
}

pub(super) fn execute_document_get(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let doc_id = arguments
        .get("document_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let page_number = arguments
        .get("page")
        .and_then(|v| v.as_i64())
        .map(|p| p as i32);

    if let Some(page) = page_number {
        // Get all chunks for the specified page
        match state.service.db.get_chunks_by_page(doc_id, page, gm_role) {
            Ok(chunks) => {
                if chunks.is_empty() {
                    return Err(McpError {
                        code: -32000,
                        message: format!(
                            "No content found for page {} of document {}",
                            page, doc_id
                        ),
                    });
                }

                // Concatenate all chunk content for the page
                let page_content: String = chunks
                    .iter()
                    .map(|c| c.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");

                Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": page_content
                    }]
                }))
            }
            Err(e) => Err(McpError {
                code: -32000,
                message: e.to_string(),
            }),
        }
    } else {
        // No page specified - return document metadata
        match state.service.db.get_document(doc_id) {
            Ok(Some(doc)) => {
                if doc.access_level.accessible_by(gm_role) {
                    Ok(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Document: {}\nID: {}\nTags: {:?}\nChunks: {}\nImages: {}\n\nUse the 'page' parameter to retrieve content from a specific page.",
                                doc.title, doc.id, doc.tags, doc.chunk_count, doc.image_count
                            )
                        }]
                    }))
                } else {
                    Err(McpError {
                        code: -32000,
                        message: "Access denied".to_string(),
                    })
                }
            }
            Ok(None) => Err(McpError {
                code: -32000,
                message: "Document not found".to_string(),
            }),
            Err(e) => Err(McpError {
                code: -32000,
                message: e.to_string(),
            }),
        }
    }
}

pub(super) fn execute_document_list(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let tags: Vec<String> = arguments
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    match state.service.db.list_documents(Some(gm_role)) {
        Ok(docs) => {
            let filtered: Vec<_> = if tags.is_empty() {
                docs
            } else {
                docs.into_iter()
                    .filter(|d| tags.iter().any(|t| d.tags.contains(t)))
                    .collect()
            };

            let doc_list: Vec<serde_json::Value> = filtered
                .into_iter()
                .map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "title": d.title,
                        "tags": d.tags,
                        "chunk_count": d.chunk_count,
                        "image_count": d.image_count
                    })
                })
                .collect();

            let text = serde_json::to_string_pretty(&serde_json::json!({ "documents": doc_list }))
                .unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        Err(e) => Err(McpError {
            code: -32000,
            message: e.to_string(),
        }),
    }
}

pub(super) fn execute_document_find(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let title_query = arguments
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match state.service.db.list_documents(Some(gm_role)) {
        Ok(docs) => {
            let query_lower = title_query.to_lowercase();
            let matches: Vec<serde_json::Value> = docs
                .into_iter()
                .filter(|d| d.title.to_lowercase().contains(&query_lower))
                .map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "title": d.title,
                        "tags": d.tags,
                        "chunk_count": d.chunk_count,
                        "image_count": d.image_count
                    })
                })
                .collect();

            let result = if matches.is_empty() {
                serde_json::json!({
                    "documents": [],
                    "message": format!("No documents found matching '{}'", title_query)
                })
            } else {
                serde_json::json!({ "documents": matches })
            };

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        Err(e) => Err(McpError {
            code: -32000,
            message: e.to_string(),
        }),
    }
}

pub(super) fn execute_document_update(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let doc_id = arguments
        .get("document_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Get current document
    let current_doc = match state.service.db.get_document(doc_id) {
        Ok(Some(doc)) => doc,
        Ok(None) => {
            return Err(McpError {
                code: -32000,
                message: "Document not found".to_string(),
            });
        }
        Err(e) => {
            return Err(McpError {
                code: -32000,
                message: e.to_string(),
            });
        }
    };

    // Check access
    if !current_doc.access_level.accessible_by(gm_role) {
        return Err(McpError {
            code: -32000,
            message: "Access denied".to_string(),
        });
    }

    // Parse optional updates
    let new_title = arguments
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(&current_doc.title)
        .to_string();

    let new_access_level = arguments
        .get("access_level")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "player" => crate::tools::AccessLevel::Player,
            "trusted" => crate::tools::AccessLevel::Trusted,
            "assistant" => crate::tools::AccessLevel::Assistant,
            _ => crate::tools::AccessLevel::GmOnly,
        })
        .unwrap_or(current_doc.access_level);

    let new_tags: Vec<String> = arguments
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(|| current_doc.tags.clone());

    match state
        .service
        .update_document(doc_id, &new_title, new_access_level, new_tags.clone())
    {
        Ok(true) => {
            let result = serde_json::json!({
                "success": true,
                "document_id": doc_id,
                "updated": {
                    "title": new_title,
                    "access_level": format!("{:?}", new_access_level).to_lowercase(),
                    "tags": new_tags
                }
            });
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        Ok(false) => Err(McpError {
            code: -32000,
            message: "Document not found".to_string(),
        }),
        Err(e) => Err(McpError {
            code: -32000,
            message: e.to_string(),
        }),
    }
}
