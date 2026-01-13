//! MCP tool call handler.
//!
//! Handles execution of individual tool calls from MCP clients.

use crate::config::AssetsAccess;
use crate::ingestion::IngestionService;
use crate::search::format_search_results_for_llm;
use crate::tools::{SearchFilters, TagMatch, TravellerTool};

use super::{McpError, McpState};

/// Handle tools/call request
pub async fn handle_tool_call(
    state: &McpState,
    params: Option<serde_json::Value>,
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

    let result = match name {
        "document_search" => execute_document_search(state, &arguments, gm_role).await?,
        "document_search_text" => execute_document_search_text(state, &arguments, gm_role)?,
        "document_get" => execute_document_get(state, &arguments, gm_role)?,
        "document_list" => execute_document_list(state, &arguments, gm_role)?,
        "document_find" => execute_document_find(state, &arguments, gm_role)?,
        "image_list" => execute_image_list(state, &arguments, gm_role)?,
        "image_search" => execute_image_search(state, &arguments, gm_role).await?,
        "image_get" => execute_image_get(state, &arguments, gm_role)?,
        "image_deliver" => execute_image_deliver(state, &arguments, gm_role)?,
        "system_schema" => execute_system_schema(&arguments)?,
        "traveller_uwp_parse" => execute_traveller_uwp_parse(&arguments)?,
        "traveller_jump_calc" => execute_traveller_jump_calc(&arguments)?,
        "traveller_skill_lookup" => execute_traveller_skill_lookup(&arguments)?,
        _ => {
            return Err(McpError {
                code: -32601,
                message: format!("Unknown tool: {}", name),
            });
        }
    };

    Ok(result)
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
    match state.service.config.fvtt.check_assets_access() {
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
