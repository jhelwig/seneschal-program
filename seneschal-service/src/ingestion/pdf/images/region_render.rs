//! Page region rendering using pdfium-render.
//!
//! This module renders specific regions of PDF pages at high resolution.
//! Used when overlapping content is detected to capture the composited appearance.

use std::path::Path;

use image::{DynamicImage, RgbaImage};
use pdfium_render::prelude::*;
use tracing::debug;

use super::Rectangle;
use crate::error::{ProcessingError, ServiceResult};

/// Render a specific region of a PDF page at the given DPI.
///
/// # Arguments
/// * `pdfium` - The PDFium instance
/// * `pdf_path` - Path to the PDF file
/// * `page_number` - Page number (0-indexed)
/// * `region` - The region to render in PDF points
/// * `dpi` - Target DPI for the render
///
/// # Returns
/// An RGBA image of the rendered region
pub fn render_page_region(
    pdfium: &Pdfium,
    pdf_path: &Path,
    page_number: usize,
    region: &Rectangle,
    dpi: f64,
) -> ServiceResult<RgbaImage> {
    // Load the PDF document
    let document =
        pdfium
            .load_pdf_from_file(pdf_path, None)
            .map_err(|e| ProcessingError::TextExtraction {
                page: page_number as u32,
                source: Box::new(std::io::Error::other(format!(
                    "Failed to load PDF for region render: {}",
                    e
                ))),
            })?;

    // Get the page
    let pages = document.pages();
    let page = pages
        .get(page_number as u16)
        .map_err(|e| ProcessingError::TextExtraction {
            page: page_number as u32,
            source: Box::new(std::io::Error::other(format!(
                "Failed to get page {} for region render: {}",
                page_number, e
            ))),
        })?;

    // Get page dimensions in points
    let page_width_pts = page.width().value as f64;
    let page_height_pts = page.height().value as f64;

    // Calculate full page pixel dimensions at the desired DPI
    let pixels_per_point = dpi / 72.0;
    let full_page_width = (page_width_pts * pixels_per_point).ceil() as i32;
    let full_page_height = (page_height_pts * pixels_per_point).ceil() as i32;

    // Calculate the region's position and size in pixels
    // PDF coordinates have origin at bottom-left, image coordinates at top-left
    let region_left_px = (region.x1 * pixels_per_point).floor() as u32;
    let region_top_px = ((page_height_pts - region.y2) * pixels_per_point).floor() as u32;
    let region_width_px = (region.width() * pixels_per_point).ceil() as u32;
    let region_height_px = (region.height() * pixels_per_point).ceil() as u32;

    // Clamp to page bounds
    let region_left_px = region_left_px.min(full_page_width as u32);
    let region_top_px = region_top_px.min(full_page_height as u32);
    let region_width_px = region_width_px
        .min(full_page_width as u32 - region_left_px)
        .max(1);
    let region_height_px = region_height_px
        .min(full_page_height as u32 - region_top_px)
        .max(1);

    debug!(
        page = page_number,
        region_pts = format!(
            "({:.1},{:.1})-({:.1},{:.1})",
            region.x1, region.y1, region.x2, region.y2
        ),
        region_px = format!(
            "({},{})-({},{})",
            region_left_px,
            region_top_px,
            region_left_px + region_width_px,
            region_top_px + region_height_px
        ),
        dpi = dpi,
        full_page_size = format!("{}x{}", full_page_width, full_page_height),
        output_size = format!("{}x{}", region_width_px, region_height_px),
        "Rendering page region"
    );

    // Render the full page at the desired DPI
    let config = PdfRenderConfig::new()
        .set_target_width(full_page_width)
        .set_target_height(full_page_height);

    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| ProcessingError::TextExtraction {
            page: page_number as u32,
            source: Box::new(std::io::Error::other(format!(
                "Failed to render page: {}",
                e
            ))),
        })?;

    // Use pdfium-render's built-in conversion which handles color format correctly
    let full_image: DynamicImage = bitmap.as_image();
    let full_rgba = full_image.to_rgba8();

    // Crop to the region we want
    let cropped = image::imageops::crop_imm(
        &full_rgba,
        region_left_px,
        region_top_px,
        region_width_px,
        region_height_px,
    )
    .to_image();

    Ok(cropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_dimensions() {
        let region = Rectangle {
            x1: 0.0,
            y1: 0.0,
            x2: 612.0, // 8.5 inches in points
            y2: 792.0, // 11 inches in points
        };

        let dpi = 300.0;
        let pixels_per_point = dpi / 72.0;

        let width = (region.width() * pixels_per_point).ceil() as i32;
        let height = (region.height() * pixels_per_point).ceil() as i32;

        // At 300 DPI, a letter page should be approximately 2550x3300 pixels
        // Allow for floating-point rounding (ceil can round up by 1)
        assert!((width - 2550).abs() <= 1, "width was {}", width);
        assert!((height - 3300).abs() <= 1, "height was {}", height);
    }
}
