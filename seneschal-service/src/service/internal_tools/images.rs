//! Image-related internal tools.

use crate::config::AssetsAccess;
use crate::ingestion::IngestionService;
use crate::service::SeneschalService;
use crate::service::state::UserContext;
use crate::tools::{ToolCall, ToolResult};

impl SeneschalService {
    pub(crate) fn tool_image_list(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let doc_id = call
            .args
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let start_page = call
            .args
            .get("start_page")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32);
        let end_page = call
            .args
            .get("end_page")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32);
        let limit = call
            .args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        match self.db.list_document_images(
            user_context.role,
            Some(doc_id),
            start_page,
            end_page,
            limit,
        ) {
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

                ToolResult::success(call.id.clone(), serde_json::json!({ "images": image_list }))
            }
            Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
        }
    }

    pub(crate) async fn tool_image_search(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let query = call
            .args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let doc_id = call.args.get("document_id").and_then(|v| v.as_str());
        let limit = call
            .args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        // Generate embedding for the query
        match self.search.embed_text(query).await {
            Ok(embedding) => match self.db.search_images(&embedding, user_context.role, limit) {
                Ok(results) => {
                    // Filter by document_id if specified
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

                    ToolResult::success(call.id.clone(), serde_json::json!({ "images": filtered }))
                }
                Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
            },
            Err(e) => ToolResult::error(
                call.id.clone(),
                format!("Failed to generate embedding: {}", e),
            ),
        }
    }

    pub(crate) fn tool_image_get(&self, call: &ToolCall, user_context: &UserContext) -> ToolResult {
        let image_id = call
            .args
            .get("image_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match self.db.get_document_image(image_id) {
            Ok(Some(img)) => {
                if img.access_level.accessible_by(user_context.role) {
                    ToolResult::success(
                        call.id.clone(),
                        serde_json::json!({
                            "id": img.image.id,
                            "document_id": img.image.document_id,
                            "document_title": img.document_title,
                            "page_number": img.image.page_number,
                            "image_index": img.image.image_index,
                            "width": img.image.width,
                            "height": img.image.height,
                            "description": img.image.description
                        }),
                    )
                } else {
                    ToolResult::error(call.id.clone(), "Access denied".to_string())
                }
            }
            Ok(None) => ToolResult::error(call.id.clone(), "Image not found".to_string()),
            Err(e) => ToolResult::error(call.id.clone(), e.to_string()),
        }
    }

    pub(crate) fn tool_image_deliver(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let image_id = call
            .args
            .get("image_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let target_path = call
            .args
            .get("target_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Get the image
        let img = match self.db.get_document_image(image_id) {
            Ok(Some(img)) => {
                if !img.access_level.accessible_by(user_context.role) {
                    return ToolResult::error(call.id.clone(), "Access denied".to_string());
                }
                img
            }
            Ok(None) => {
                return ToolResult::error(call.id.clone(), "Image not found".to_string());
            }
            Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
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

        // The FVTT path is what FVTT uses to reference the file (prepend assets/)
        let fvtt_path = format!("assets/{}", relative_path);

        // Check assets access mode
        match self.runtime_config.static_config.fvtt.check_assets_access() {
            AssetsAccess::Direct(assets_dir) => {
                // Create target directory
                let full_path = assets_dir.join(&relative_path);
                if let Some(parent) = full_path.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to create directory: {}", e),
                    );
                }

                // Copy file
                if let Err(e) = std::fs::copy(&img.image.internal_path, &full_path) {
                    return ToolResult::error(
                        call.id.clone(),
                        format!("Failed to copy image: {}", e),
                    );
                }

                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": true,
                        "mode": "direct",
                        "fvtt_path": fvtt_path,
                        "message": format!("Image delivered to FVTT assets at {}", fvtt_path)
                    }),
                )
            }
            AssetsAccess::Shuttle => {
                // Cannot directly deliver, return info for client to handle
                ToolResult::success(
                    call.id.clone(),
                    serde_json::json!({
                        "success": false,
                        "mode": "shuttle",
                        "image_id": image_id,
                        "suggested_path": fvtt_path,
                        "message": "Direct delivery not available. Use the FVTT module to fetch and deliver this image."
                    }),
                )
            }
        }
    }
}
