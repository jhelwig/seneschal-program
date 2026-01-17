//! Document-related internal tools.

use crate::search::format_search_results_for_llm;
use crate::service::SeneschalService;
use crate::service::state::UserContext;
use crate::tools::{AccessLevel, SearchFilters, TagMatch, ToolCall, ToolResult};

impl SeneschalService {
    pub(crate) async fn tool_document_search(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let query = call
            .args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tags: Vec<String> = call
            .args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let limit = call
            .args
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

        match self
            .search
            .search(query, user_context.role, limit, filters)
            .await
        {
            Ok(results) => {
                let formatted = format_search_results_for_llm(&results, &self.i18n, "en");
                ToolResult::success(call.id.clone(), serde_json::json!({ "results": formatted }))
            }
            Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
        }
    }

    pub(crate) fn tool_document_search_text(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let query = call
            .args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let section = call.args.get("section").and_then(|v| v.as_str());
        let document_id = call.args.get("document_id").and_then(|v| v.as_str());
        let limit = call
            .args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        match self
            .db
            .search_chunks_fts(query, section, document_id, user_context.role, limit)
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

                if results.is_empty() {
                    ToolResult::success(
                        call.id.clone(),
                        serde_json::json!({
                            "results": [],
                            "message": format!("No matches found for '{}'", query)
                        }),
                    )
                } else {
                    ToolResult::success(call.id.clone(), serde_json::json!({ "results": results }))
                }
            }
            Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
        }
    }

    pub(crate) fn tool_document_get(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let doc_id = call
            .args
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let page_number = call
            .args
            .get("page")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32);

        if let Some(page) = page_number {
            // Get all chunks for the specified page
            match self.db.get_chunks_by_page(doc_id, page, user_context.role) {
                Ok(chunks) => {
                    if chunks.is_empty() {
                        ToolResult::error(
                            call.id.clone(),
                            format!("No content found for page {} of document {}", page, doc_id),
                        )
                    } else {
                        // Concatenate all chunk content for the page
                        let page_content: String = chunks
                            .iter()
                            .map(|c| c.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n\n");

                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "document_id": doc_id,
                                "page": page,
                                "content": page_content,
                                "chunk_count": chunks.len()
                            }),
                        )
                    }
                }
                Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
            }
        } else {
            // No page specified - return document metadata
            match self.db.get_document(doc_id) {
                Ok(Some(doc)) => {
                    if doc.access_level.accessible_by(user_context.role) {
                        ToolResult::success(
                            call.id.clone(),
                            serde_json::json!({
                                "id": doc.id,
                                "title": doc.title,
                                "tags": doc.tags,
                                "chunk_count": doc.chunk_count,
                                "image_count": doc.image_count,
                                "note": "Use the 'page' parameter to retrieve content from a specific page"
                            }),
                        )
                    } else {
                        ToolResult::error(call.id.clone(), "Access denied".to_string())
                    }
                }
                Ok(None) => ToolResult::error(call.id.clone(), "Document not found".to_string()),
                Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
            }
        }
    }

    pub(crate) fn tool_document_list(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let tags: Vec<String> = call
            .args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        match self.db.list_documents(Some(user_context.role)) {
            Ok(docs) => {
                // Filter by tags if specified
                let filtered: Vec<_> = if tags.is_empty() {
                    docs
                } else {
                    docs.into_iter()
                        .filter(|d| tags.iter().any(|t| d.tags.contains(t)))
                        .collect()
                };

                // Return simplified list with just id, title, tags
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

                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({ "documents": doc_list }),
                )
            }
            Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
        }
    }

    pub(crate) fn tool_document_find(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let title_query = call
            .args
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match self.db.list_documents(Some(user_context.role)) {
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

                if matches.is_empty() {
                    ToolResult::success(
                        call.id.clone(),
                        serde_json::json!({
                            "documents": [],
                            "message": format!("No documents found matching '{}'", title_query)
                        }),
                    )
                } else {
                    ToolResult::success(
                        call.id.clone(),
                        serde_json::json!({ "documents": matches }),
                    )
                }
            }
            Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
        }
    }

    pub(crate) fn tool_document_update(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let doc_id = call
            .args
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Get current document to fill in unchanged fields
        let current_doc = match self.db.get_document(doc_id) {
            Ok(Some(doc)) => doc,
            Ok(None) => {
                return ToolResult::error(call.id.clone(), "Document not found".to_string());
            }
            Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
        };

        // Check access - user must have access to the document
        if !current_doc.access_level.accessible_by(user_context.role) {
            return ToolResult::error(call.id.clone(), "Access denied".to_string());
        }

        // Parse optional updates, falling back to current values
        let new_title = call
            .args
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(&current_doc.title)
            .to_string();

        let new_access_level = call
            .args
            .get("access_level")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "player" => AccessLevel::Player,
                "trusted" => AccessLevel::Trusted,
                "assistant" => AccessLevel::Assistant,
                _ => AccessLevel::GmOnly,
            })
            .unwrap_or(current_doc.access_level);

        let new_tags: Vec<String> = call
            .args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_else(|| current_doc.tags.clone());

        match self.update_document(doc_id, &new_title, new_access_level, new_tags.clone()) {
            Ok(true) => ToolResult::success(
                call.id.clone(),
                serde_json::json!({
                    "success": true,
                    "document_id": doc_id,
                    "updated": {
                        "title": new_title,
                        "access_level": format!("{:?}", new_access_level).to_lowercase(),
                        "tags": new_tags
                    }
                }),
            ),
            Ok(false) => ToolResult::error(call.id.clone(), "Document not found".to_string()),
            Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
        }
    }
}
