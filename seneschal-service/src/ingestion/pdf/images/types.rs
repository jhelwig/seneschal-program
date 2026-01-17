//! Core types for PDF image extraction.

use pdfium_render::prelude::PdfPage;

/// Rectangle representing image position on a PDF page
#[derive(Debug, Clone, Copy)]
pub struct Rectangle {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl Rectangle {
    pub fn area(&self) -> f64 {
        (self.x2 - self.x1).abs() * (self.y2 - self.y1).abs()
    }

    pub fn width(&self) -> f64 {
        (self.x2 - self.x1).abs()
    }

    pub fn height(&self) -> f64 {
        (self.y2 - self.y1).abs()
    }
}

/// Information about an extracted PDF image
pub struct ImageInfo {
    pub image_id: i32,
    pub area: Rectangle,
    pub surface_data: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub has_alpha: bool,
    pub is_grayscale: bool,
    /// Scale factor from PDF points to pixels (width)
    #[allow(dead_code)]
    pub scale_x: f64,
    /// Scale factor from PDF points to pixels (height)
    #[allow(dead_code)]
    pub scale_y: f64,
    /// Page number (0-indexed)
    pub page_number: usize,
    /// Page width in PDF points
    pub page_width: f64,
    /// Page height in PDF points
    pub page_height: f64,
    /// Transformation matrix from PDF CTM (if found)
    pub transform: Option<[f64; 6]>,
    /// Pixel crop region when a clip_rect was applied (x, y, width, height in pixels)
    pub crop_pixels: Option<(u32, u32, u32, u32)>,
    /// Soft mask (SMask) data for transparency
    pub smask_data: Option<Vec<u8>>,
    /// Width of the SMask image in pixels
    pub smask_width: Option<u32>,
    /// Height of the SMask image in pixels
    pub smask_height: Option<u32>,
}

/// Information about page boundary boxes
pub struct PageBoxes {
    /// The MediaBox (full PDF canvas)
    pub media_box: Option<Rectangle>,
    /// The CropBox (visible area)
    pub crop_box: Option<Rectangle>,
}

/// Check if image bounds are valid (within reasonable page bounds).
pub fn is_valid_bounds(bounds: &Rectangle, page_width: f64, page_height: f64) -> bool {
    // Allow some margin (10%) for bleed/transforms
    let margin = page_width.max(page_height) * 0.1;

    bounds.x1 >= -margin
        && bounds.y1 >= -margin
        && bounds.x2 <= page_width + margin
        && bounds.y2 <= page_height + margin
}

/// Extract page boundary boxes from pdfium
pub fn extract_page_boxes(page: &PdfPage) -> PageBoxes {
    let boundaries = page.boundaries();

    let media_box = boundaries.media().ok().map(|b| {
        let r = b.bounds;
        Rectangle {
            x1: r.left().value as f64,
            y1: r.bottom().value as f64,
            x2: r.right().value as f64,
            y2: r.top().value as f64,
        }
    });

    let crop_box = boundaries.crop().ok().map(|b| {
        let r = b.bounds;
        Rectangle {
            x1: r.left().value as f64,
            y1: r.bottom().value as f64,
            x2: r.right().value as f64,
            y2: r.top().value as f64,
        }
    });

    PageBoxes {
        media_box,
        crop_box,
    }
}
