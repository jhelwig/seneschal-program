//! Image-related MCP tool implementations.

use crate::config::AssetsAccess;
use crate::ingestion::IngestionService;

use super::super::{McpError, McpState};

pub(super) fn execute_image_list(
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

pub(super) async fn execute_image_search(
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

pub(super) fn execute_image_get(
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

pub(super) fn execute_image_deliver(
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
