//! MCP tool call handler.
//!
//! Handles execution of individual tool calls from MCP clients.

use crate::config::AssetsAccess;
use crate::ingestion::IngestionService;
use crate::search::format_search_results_for_llm;
use crate::tools::traveller_map::{JumpMapOptions, PosterOptions};
use crate::tools::{
    SearchFilters, TagMatch, ToolLocation, TravellerMapTool, TravellerTool, classify_tool,
};

use super::tool_search::TOOL_SEARCH_INDEX;
use super::{McpError, McpState};

/// Handle tools/call request
pub async fn handle_tool_call(
    state: &McpState,
    params: Option<serde_json::Value>,
    session_id: Option<&str>,
) -> Result<serde_json::Value, McpError> {
    let params = params.ok_or_else(|| McpError {
        code: -32602,
        message: "Missing params".to_string(),
    })?;

    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError {
            code: -32602,
            message: "Missing tool name".to_string(),
        })?;

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    // MCP clients have GM access (role=4) since MCP has no user context
    let gm_role = 4u8;

    // Classify the tool and route accordingly
    let location = classify_tool(name);

    let result = match location {
        ToolLocation::Internal => {
            // Execute internal tools directly
            execute_internal_tool(state, name, &arguments, gm_role).await?
        }
        ToolLocation::External => {
            // Route external tools through GM WebSocket connection
            execute_external_tool(state, name, arguments, session_id).await?
        }
    };

    Ok(result)
}

/// Execute an internal tool directly on the backend
async fn execute_internal_tool(
    state: &McpState,
    name: &str,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    match name {
        "document_search" => execute_document_search(state, arguments, gm_role).await,
        "document_search_text" => execute_document_search_text(state, arguments, gm_role),
        "document_get" => execute_document_get(state, arguments, gm_role),
        "document_list" => execute_document_list(state, arguments, gm_role),
        "document_find" => execute_document_find(state, arguments, gm_role),
        "document_update" => execute_document_update(state, arguments, gm_role),
        "image_list" => execute_image_list(state, arguments, gm_role),
        "image_search" => execute_image_search(state, arguments, gm_role).await,
        "image_get" => execute_image_get(state, arguments, gm_role),
        "image_deliver" => execute_image_deliver(state, arguments, gm_role),
        "system_schema" => execute_system_schema(arguments),
        "traveller_uwp_parse" => execute_traveller_uwp_parse(arguments),
        "traveller_jump_calc" => execute_traveller_jump_calc(arguments),
        "traveller_skill_lookup" => execute_traveller_skill_lookup(arguments),
        // Traveller Map API tools
        "traveller_map_search" => execute_traveller_map_search(state, arguments).await,
        "traveller_map_jump_worlds" => execute_traveller_map_jump_worlds(state, arguments).await,
        "traveller_map_route" => execute_traveller_map_route(state, arguments).await,
        "traveller_map_world_data" => execute_traveller_map_world_data(state, arguments).await,
        "traveller_map_sector_data" => execute_traveller_map_sector_data(state, arguments).await,
        "traveller_map_coordinates" => execute_traveller_map_coordinates(state, arguments).await,
        "traveller_map_list_sectors" => execute_traveller_map_list_sectors(state, arguments).await,
        "traveller_map_poster_url" => execute_traveller_map_poster_url(state, arguments),
        "traveller_map_jump_map_url" => execute_traveller_map_jump_map_url(state, arguments),
        "traveller_map_save_poster" => execute_traveller_map_save_poster(state, arguments).await,
        "traveller_map_save_jump_map" => {
            execute_traveller_map_save_jump_map(state, arguments).await
        }
        "tool_search" => execute_tool_search(arguments),
        _ => Err(McpError {
            code: -32601,
            message: format!("Unknown internal tool: {}", name),
        }),
    }
}

/// Execute an external tool by routing through GM WebSocket connection
///
/// Uses deduplication cache to prevent duplicate tool executions when
/// Claude Desktop retries requests. The session_id scopes deduplication
/// to a single MCP client session.
async fn execute_external_tool(
    state: &McpState,
    name: &str,
    arguments: serde_json::Value,
    session_id: Option<&str>,
) -> Result<serde_json::Value, McpError> {
    // Generate dedup key from session ID, tool name and arguments
    let dedup_key = McpState::dedup_key(session_id, name, &arguments);

    // Check for cached result (duplicate request)
    if let Some(cached) = state.get_cached_result(dedup_key) {
        tracing::info!(
            tool = %name,
            session_id = ?session_id,
            dedup_key = %dedup_key,
            "Returning cached result for duplicate MCP tool call"
        );
        return Ok(cached);
    }

    // Get timeout from config
    let timeout = state
        .service
        .runtime_config
        .dynamic()
        .agentic_loop
        .external_tool_timeout();

    match state
        .service
        .execute_external_tool_mcp(name, arguments, timeout)
        .await
    {
        Ok(result) => {
            // Format result in MCP content format
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            let response = serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            });

            // Cache the result for deduplication
            state.cache_result(dedup_key, response.clone());

            // Periodically clean up expired entries (every ~100 calls on average)
            if rand::random::<u8>() < 3 {
                state.cleanup_expired_cache();
            }

            Ok(response)
        }
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

async fn execute_document_search(
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

fn execute_document_search_text(
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

fn execute_document_get(
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

fn execute_traveller_uwp_parse(
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let uwp = arguments.get("uwp").and_then(|v| v.as_str()).unwrap_or("");
    let tool = TravellerTool::ParseUwp {
        uwp: uwp.to_string(),
    };

    match tool.execute() {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

fn execute_traveller_jump_calc(
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let distance = arguments
        .get("distance_parsecs")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u8;
    let rating = arguments
        .get("ship_jump_rating")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as u8;
    let tonnage = arguments
        .get("ship_tonnage")
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as u32;

    let tool = TravellerTool::JumpCalculation {
        distance_parsecs: distance,
        ship_jump_rating: rating,
        ship_tonnage: tonnage,
    };

    match tool.execute() {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

fn execute_traveller_skill_lookup(
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let skill = arguments
        .get("skill_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let speciality = arguments
        .get("speciality")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let tool = TravellerTool::SkillLookup {
        skill_name: skill.to_string(),
        speciality,
    };

    match tool.execute() {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

fn execute_document_list(
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

fn execute_document_find(
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

fn execute_document_update(
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

fn execute_image_list(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let doc_id = arguments
        .get("document_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let start_page = arguments
        .get("start_page")
        .and_then(|v| v.as_i64())
        .map(|p| p as i32);
    let end_page = arguments
        .get("end_page")
        .and_then(|v| v.as_i64())
        .map(|p| p as i32);
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    match state
        .service
        .db
        .list_document_images(gm_role, Some(doc_id), start_page, end_page, limit)
    {
        Ok(images) => {
            let image_list: Vec<_> = images
                .into_iter()
                .map(|img| {
                    serde_json::json!({
                        "id": img.image.id,
                        "page_number": img.image.page_number,
                        "image_index": img.image.image_index,
                        "width": img.image.width,
                        "height": img.image.height,
                        "description": img.image.description
                    })
                })
                .collect();

            let text = serde_json::to_string_pretty(&serde_json::json!({ "images": image_list }))
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

async fn execute_image_search(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let doc_id = arguments.get("document_id").and_then(|v| v.as_str());
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    // Generate embedding for the query
    let embedding = state
        .service
        .search
        .embed_text(query)
        .await
        .map_err(|e| McpError {
            code: -32000,
            message: format!("Failed to generate embedding: {}", e),
        })?;

    match state.service.db.search_images(&embedding, gm_role, limit) {
        Ok(results) => {
            let filtered: Vec<_> = results
                .into_iter()
                .filter(|(img, _)| doc_id.is_none_or(|d| img.image.document_id == d))
                .map(|(img, score)| {
                    serde_json::json!({
                        "id": img.image.id,
                        "document_id": img.image.document_id,
                        "document_title": img.document_title,
                        "page_number": img.image.page_number,
                        "image_index": img.image.image_index,
                        "description": img.image.description,
                        "similarity": score
                    })
                })
                .collect();

            let text = serde_json::to_string_pretty(&serde_json::json!({ "images": filtered }))
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

fn execute_image_get(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let image_id = arguments
        .get("image_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match state.service.db.get_document_image(image_id) {
        Ok(Some(img)) => {
            if img.access_level.accessible_by(gm_role) {
                let result = serde_json::json!({
                    "id": img.image.id,
                    "document_id": img.image.document_id,
                    "document_title": img.document_title,
                    "page_number": img.image.page_number,
                    "image_index": img.image.image_index,
                    "width": img.image.width,
                    "height": img.image.height,
                    "description": img.image.description
                });

                let text = serde_json::to_string_pretty(&result).unwrap_or_default();

                Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": text
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
            message: "Image not found".to_string(),
        }),
        Err(e) => Err(McpError {
            code: -32000,
            message: e.to_string(),
        }),
    }
}

fn execute_image_deliver(
    state: &McpState,
    arguments: &serde_json::Value,
    gm_role: u8,
) -> Result<serde_json::Value, McpError> {
    let image_id = arguments
        .get("image_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let target_path = arguments
        .get("target_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Get the image
    let img = match state.service.db.get_document_image(image_id) {
        Ok(Some(img)) => {
            if !img.access_level.accessible_by(gm_role) {
                return Err(McpError {
                    code: -32000,
                    message: "Access denied".to_string(),
                });
            }
            img
        }
        Ok(None) => {
            return Err(McpError {
                code: -32000,
                message: "Image not found".to_string(),
            });
        }
        Err(e) => {
            return Err(McpError {
                code: -32000,
                message: e.to_string(),
            });
        }
    };

    // Determine the path relative to the FVTT assets directory
    let relative_path = target_path.unwrap_or_else(|| {
        IngestionService::fvtt_image_path(
            &img.document_title,
            img.image.page_number,
            img.image.description.as_deref(),
        )
        .to_string_lossy()
        .to_string()
    });

    // The FVTT path is what FVTT uses to reference the file
    let fvtt_path = format!("assets/{}", relative_path);

    // Check assets access mode
    match state
        .service
        .runtime_config
        .static_config
        .fvtt
        .check_assets_access()
    {
        AssetsAccess::Direct(assets_dir) => {
            // Create target directory
            let full_path = assets_dir.join(&relative_path);
            if let Some(parent) = full_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to create directory: {}", e),
                });
            }

            // Copy file
            if let Err(e) = std::fs::copy(&img.image.internal_path, &full_path) {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to copy image: {}", e),
                });
            }

            let result = serde_json::json!({
                "success": true,
                "mode": "direct",
                "fvtt_path": fvtt_path,
                "message": format!("Image delivered to FVTT assets at {}", fvtt_path)
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        AssetsAccess::Shuttle => {
            let result = serde_json::json!({
                "success": false,
                "mode": "shuttle",
                "image_id": image_id,
                "suggested_path": fvtt_path,
                "message": "Direct delivery not available. Use the FVTT module to fetch and deliver this image."
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
    }
}

fn execute_system_schema(_arguments: &serde_json::Value) -> Result<serde_json::Value, McpError> {
    // Return a placeholder schema - in reality this would come from FVTT
    let schema = serde_json::json!({
        "system": "mgt2e",
        "actorTypes": ["traveller", "npc", "creature", "spacecraft", "vehicle", "world"],
        "itemTypes": ["weapon", "armour", "skill", "term", "equipment"],
        "note": "For detailed schema, query the FVTT client directly"
    });

    let text = serde_json::to_string_pretty(&schema).unwrap_or_default();

    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": text
        }]
    }))
}

// ==========================================
// Traveller Map API Tool Implementations
// ==========================================

async fn execute_traveller_map_search(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let milieu = arguments.get("milieu").and_then(|v| v.as_str());

    let tool = TravellerMapTool::Search {
        query: query.to_string(),
        milieu: milieu.map(|s| s.to_string()),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

async fn execute_traveller_map_jump_worlds(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");
    let jump = arguments.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;

    let tool = TravellerMapTool::JumpWorlds {
        sector: sector.to_string(),
        hex: hex.to_string(),
        jump,
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

async fn execute_traveller_map_route(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let start = arguments
        .get("start")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let end = arguments.get("end").and_then(|v| v.as_str()).unwrap_or("");
    let jump = arguments.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
    let wild = arguments
        .get("wild")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let imperium_only = arguments
        .get("imperium_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let no_red_zones = arguments
        .get("no_red_zones")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let tool = TravellerMapTool::Route {
        start: start.to_string(),
        end: end.to_string(),
        jump,
        wild,
        imperium_only,
        no_red_zones,
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

async fn execute_traveller_map_world_data(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");

    let tool = TravellerMapTool::WorldData {
        sector: sector.to_string(),
        hex: hex.to_string(),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

async fn execute_traveller_map_sector_data(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let subsector = arguments.get("subsector").and_then(|v| v.as_str());

    let tool = TravellerMapTool::SectorData {
        sector: sector.to_string(),
        subsector: subsector.map(|s| s.to_string()),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

async fn execute_traveller_map_coordinates(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str());

    let tool = TravellerMapTool::Coordinates {
        sector: sector.to_string(),
        hex: hex.map(|s| s.to_string()),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

async fn execute_traveller_map_list_sectors(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let milieu = arguments.get("milieu").and_then(|v| v.as_str());

    let tool = TravellerMapTool::ListSectors {
        milieu: milieu.map(|s| s.to_string()),
    };

    match tool.execute(&state.service.traveller_map_client).await {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

fn execute_traveller_map_poster_url(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let subsector = arguments.get("subsector").and_then(|v| v.as_str());
    let style = arguments.get("style").and_then(|v| v.as_str());

    let tool = TravellerMapTool::PosterUrl {
        sector: sector.to_string(),
        subsector: subsector.map(|s| s.to_string()),
        style: style.map(|s| s.to_string()),
    };

    // This is a synchronous operation (just URL generation)
    let result = futures::executor::block_on(tool.execute(&state.service.traveller_map_client));

    match result {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

fn execute_traveller_map_jump_map_url(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");
    let jump = arguments.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
    let style = arguments.get("style").and_then(|v| v.as_str());

    let tool = TravellerMapTool::JumpMapUrl {
        sector: sector.to_string(),
        hex: hex.to_string(),
        jump,
        style: style.map(|s| s.to_string()),
    };

    // This is a synchronous operation (just URL generation)
    let result = futures::executor::block_on(tool.execute(&state.service.traveller_map_client));

    match result {
        Ok(result) => Ok(serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&result).unwrap_or_default()
            }]
        })),
        Err(e) => Err(McpError {
            code: -32000,
            message: e,
        }),
    }
}

async fn execute_traveller_map_save_poster(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let subsector = arguments.get("subsector").and_then(|v| v.as_str());
    let style = arguments.get("style").and_then(|v| v.as_str());
    let scale = arguments
        .get("scale")
        .and_then(|v| v.as_u64())
        .map(|s| s as u32);
    let target_path = arguments.get("target_path").and_then(|v| v.as_str());

    // Download the image
    let options = PosterOptions {
        subsector: subsector.map(|s| s.to_string()),
        style: style.map(|s| s.to_string()),
        scale,
        ..Default::default()
    };

    let (bytes, extension) = state
        .service
        .traveller_map_client
        .download_poster(sector, &options)
        .await
        .map_err(|e| McpError {
            code: -32000,
            message: e.to_string(),
        })?;

    // Generate filename
    let filename = if let Some(ss) = subsector {
        format!(
            "traveller-map/{}-{}.{}",
            sanitize_filename(sector),
            sanitize_filename(ss),
            extension
        )
    } else {
        format!("traveller-map/{}.{}", sanitize_filename(sector), extension)
    };
    let relative_path = target_path.map(|s| s.to_string()).unwrap_or(filename);

    // The FVTT path is what FVTT uses to reference the file
    let fvtt_path = format!("assets/{}", relative_path);

    // Save to FVTT assets
    match state
        .service
        .runtime_config
        .static_config
        .fvtt
        .check_assets_access()
    {
        AssetsAccess::Direct(assets_dir) => {
            let full_path = assets_dir.join(&relative_path);
            if let Some(parent) = full_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to create directory: {}", e),
                });
            }

            if let Err(e) = std::fs::write(&full_path, &bytes) {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to write image: {}", e),
                });
            }

            let result = serde_json::json!({
                "success": true,
                "mode": "direct",
                "fvtt_path": fvtt_path,
                "size_bytes": bytes.len(),
                "message": format!("Poster map saved to {}", fvtt_path)
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        AssetsAccess::Shuttle => {
            // Can't directly write - need to shuttle via the FVTT module
            let result = serde_json::json!({
                "success": false,
                "mode": "shuttle",
                "suggested_path": fvtt_path,
                "message": "Direct asset writing not available. FVTT assets directory not configured or not writable."
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
    }
}

async fn execute_traveller_map_save_jump_map(
    state: &McpState,
    arguments: &serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    let sector = arguments
        .get("sector")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hex = arguments.get("hex").and_then(|v| v.as_str()).unwrap_or("");
    let jump = arguments.get("jump").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
    let style = arguments.get("style").and_then(|v| v.as_str());
    let scale = arguments
        .get("scale")
        .and_then(|v| v.as_u64())
        .map(|s| s as u32);
    let target_path = arguments.get("target_path").and_then(|v| v.as_str());

    // Download the image
    let options = JumpMapOptions {
        style: style.map(|s| s.to_string()),
        scale,
        ..Default::default()
    };

    let (bytes, extension) = state
        .service
        .traveller_map_client
        .download_jump_map(sector, hex, jump, &options)
        .await
        .map_err(|e| McpError {
            code: -32000,
            message: e.to_string(),
        })?;

    // Generate filename
    let filename = format!(
        "traveller-map/{}-{}-jump{}.{}",
        sanitize_filename(sector),
        hex,
        jump,
        extension
    );
    let relative_path = target_path.map(|s| s.to_string()).unwrap_or(filename);

    // The FVTT path is what FVTT uses to reference the file
    let fvtt_path = format!("assets/{}", relative_path);

    // Save to FVTT assets
    match state
        .service
        .runtime_config
        .static_config
        .fvtt
        .check_assets_access()
    {
        AssetsAccess::Direct(assets_dir) => {
            let full_path = assets_dir.join(&relative_path);
            if let Some(parent) = full_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to create directory: {}", e),
                });
            }

            if let Err(e) = std::fs::write(&full_path, &bytes) {
                return Err(McpError {
                    code: -32000,
                    message: format!("Failed to write image: {}", e),
                });
            }

            let result = serde_json::json!({
                "success": true,
                "mode": "direct",
                "fvtt_path": fvtt_path,
                "size_bytes": bytes.len(),
                "message": format!("Jump map saved to {}", fvtt_path)
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
        AssetsAccess::Shuttle => {
            let result = serde_json::json!({
                "success": false,
                "mode": "shuttle",
                "suggested_path": fvtt_path,
                "message": "Direct asset writing not available. FVTT assets directory not configured or not writable."
            });

            let text = serde_json::to_string_pretty(&result).unwrap_or_default();

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }
    }
}

/// Sanitize a string for use in a filename
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ' ' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

// ==========================================
// MCP Tool Search Implementation
// ==========================================

/// Execute tool_search - search for tools using natural language.
///
/// Returns tool_reference blocks per the Claude tool search tool specification.
fn execute_tool_search(arguments: &serde_json::Value) -> Result<serde_json::Value, McpError> {
    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .min(10) as usize;

    if query.is_empty() {
        return Err(McpError {
            code: -32602,
            message: "Query parameter is required".to_string(),
        });
    }

    let results = TOOL_SEARCH_INDEX.search(query, limit);

    // Return tool_reference blocks per Claude docs
    let tool_references: Vec<serde_json::Value> = results
        .into_iter()
        .map(|tool_name| {
            serde_json::json!({
                "type": "tool_reference",
                "tool_name": tool_name
            })
        })
        .collect();

    Ok(serde_json::json!({ "content": tool_references }))
}
