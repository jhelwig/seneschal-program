//! Page rendering internal tools.
//!
//! These tools render PDF pages or regions and save them as images that can
//! be delivered to FVTT.

use std::path::PathBuf;

use chrono::Utc;
use image::codecs::webp::WebPEncoder;
use image::ImageEncoder;
use tracing::debug;
use uuid::Uuid;

use crate::db::{DocumentImage, ImageType};
use crate::ingestion::pdf::images::region_render::{render_full_page, render_page_region};
use crate::ingestion::pdf::{create_pdfium, images::Rectangle};
use crate::service::SeneschalService;
use crate::service::state::UserContext;
use crate::tools::{ToolCall, ToolResult};

/// Specifies what portion of a page to render.
enum RenderScope {
    FullPage,
    Region(Rectangle),
}

impl SeneschalService {
    pub(crate) fn tool_render_page_region(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let doc_id = call
            .args
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let page = call.args.get("page").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
        let x1 = call.args.get("x1").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y1 = call.args.get("y1").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let x2 = call
            .args
            .get("x2")
            .and_then(|v| v.as_f64())
            .unwrap_or(612.0);
        let y2 = call
            .args
            .get("y2")
            .and_then(|v| v.as_f64())
            .unwrap_or(792.0);
        let dpi = call
            .args
            .get("dpi")
            .and_then(|v| v.as_f64())
            .unwrap_or(150.0);

        let region = Rectangle { x1, y1, x2, y2 };
        self.render_page(call, user_context, doc_id, page, dpi, RenderScope::Region(region))
    }

    pub(crate) fn tool_render_full_page(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
    ) -> ToolResult {
        let doc_id = call
            .args
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let page = call.args.get("page").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
        let dpi = call
            .args
            .get("dpi")
            .and_then(|v| v.as_f64())
            .unwrap_or(150.0);

        self.render_page(call, user_context, doc_id, page, dpi, RenderScope::FullPage)
    }

    fn render_page(
        &self,
        call: &ToolCall,
        user_context: &UserContext,
        doc_id: &str,
        page: i32,
        dpi: f64,
        scope: RenderScope,
    ) -> ToolResult {
        if page < 1 {
            return ToolResult::error(call.id.clone(), "Page number must be >= 1".to_string());
        }

        let doc = match self.db.get_document(doc_id) {
            Ok(Some(d)) => d,
            Ok(None) => {
                return ToolResult::error(call.id.clone(), "Document not found".to_string());
            }
            Err(e) => return ToolResult::error(call.id.clone(), e.to_string()),
        };

        if !doc.access_level.accessible_by(user_context.role) {
            return ToolResult::error(call.id.clone(), "Access denied".to_string());
        }

        let pdf_path = match &doc.file_path {
            Some(p) => PathBuf::from(p),
            None => {
                return ToolResult::error(
                    call.id.clone(),
                    "Document has no associated file".to_string(),
                );
            }
        };

        if !pdf_path.exists() {
            return ToolResult::error(
                call.id.clone(),
                format!("PDF file not found: {}", pdf_path.display()),
            );
        }

        let pdfium = match create_pdfium() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(
                    call.id.clone(),
                    format!("Failed to initialize PDF renderer: {}", e),
                );
            }
        };

        let page_index = (page - 1) as usize;
        let (image, description, filename_prefix) = match &scope {
            RenderScope::Region(region) => {
                debug!(
                    document_id = doc_id,
                    page = page,
                    region = format!(
                        "({:.1},{:.1})-({:.1},{:.1})",
                        region.x1, region.y1, region.x2, region.y2
                    ),
                    dpi = dpi,
                    "Rendering page region"
                );

                let img = match render_page_region(&pdfium, &pdf_path, page_index, region, dpi) {
                    Ok(img) => img,
                    Err(e) => {
                        return ToolResult::error(
                            call.id.clone(),
                            format!("Failed to render page region: {}", e),
                        );
                    }
                };

                let desc = format!(
                    "Rendered region from page {} ({:.0},{:.0})-({:.0},{:.0}) at {} DPI",
                    page, region.x1, region.y1, region.x2, region.y2, dpi
                );

                (img, desc, "region")
            }
            RenderScope::FullPage => {
                debug!(
                    document_id = doc_id,
                    page = page,
                    dpi = dpi,
                    "Rendering full page"
                );

                let img = match render_full_page(&pdfium, &pdf_path, page_index, dpi) {
                    Ok(img) => img,
                    Err(e) => {
                        return ToolResult::error(
                            call.id.clone(),
                            format!("Failed to render full page: {}", e),
                        );
                    }
                };

                let desc = format!("Full page {} render at {} DPI", page, dpi);

                (img, desc, "page")
            }
        };

        let image_id = Uuid::new_v4().to_string();
        let images_dir = self
            .runtime_config
            .static_config
            .storage
            .data_dir
            .join("rendered")
            .join(doc_id);

        if let Err(e) = std::fs::create_dir_all(&images_dir) {
            return ToolResult::error(
                call.id.clone(),
                format!("Failed to create output directory: {}", e),
            );
        }

        let output_path = images_dir.join(format!(
            "{}_p{}_{}.webp",
            filename_prefix,
            page,
            &image_id[..8]
        ));

        let mut output_file = match std::fs::File::create(&output_path) {
            Ok(f) => f,
            Err(e) => {
                return ToolResult::error(
                    call.id.clone(),
                    format!("Failed to create output file: {}", e),
                );
            }
        };

        let encoder = WebPEncoder::new_lossless(&mut output_file);
        if let Err(e) = encoder.write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        ) {
            return ToolResult::error(call.id.clone(), format!("Failed to encode image: {}", e));
        }

        let doc_image = DocumentImage {
            id: image_id.clone(),
            document_id: doc_id.to_string(),
            page_number: page,
            image_index: 0,
            internal_path: output_path.to_string_lossy().to_string(),
            mime_type: "image/webp".to_string(),
            width: Some(image.width()),
            height: Some(image.height()),
            description: Some(description),
            source_pages: None,
            image_type: ImageType::Render,
            source_image_id: None,
            has_region_render: false,
            created_at: Utc::now(),
        };

        if let Err(e) = self.db.insert_document_image(&doc_image) {
            return ToolResult::error(
                call.id.clone(),
                format!("Failed to save image record: {}", e),
            );
        }

        let mut response = serde_json::json!({
            "success": true,
            "image_id": image_id,
            "width": image.width(),
            "height": image.height(),
            "page": page,
            "dpi": dpi,
            "message": format!("Rendered successfully. Use image_deliver with image_id '{}' to copy to FVTT assets.", image_id)
        });

        if let RenderScope::Region(region) = scope {
            response["region"] = serde_json::json!({
                "x1": region.x1,
                "y1": region.y1,
                "x2": region.x2,
                "y2": region.y2
            });
        }

        ToolResult::success(call.id.clone(), response)
    }
}
