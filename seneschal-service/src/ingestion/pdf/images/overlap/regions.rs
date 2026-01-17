//! Content region detection for PDF pages.
//!
//! This module extracts bounding boxes for text, paths, and images from PDF pages
//! using pdfium-render. These regions are used to detect when images overlap with
//! other content.

use pdfium_render::prelude::*;
use tracing::{debug, trace};

use super::super::{ImageInfo, Rectangle};

/// A detected content region on a page
#[derive(Debug, Clone)]
pub struct ContentRegion {
    pub bounds: Rectangle,
}

/// Information about an image found via pdfium
#[derive(Debug, Clone)]
pub struct PdfiumImageInfo {
    /// Bounding box in page coordinates (pdfium coordinate system)
    pub bounds: Rectangle,
    /// Pixel width of the image
    pub width: i32,
    /// Pixel height of the image
    pub height: i32,
}

/// Convert pdfium PdfRect to our Rectangle type
pub(super) fn pdf_rect_to_rectangle(rect: &PdfRect) -> Rectangle {
    Rectangle {
        x1: rect.left().value as f64,
        y1: rect.bottom().value as f64,
        x2: rect.right().value as f64,
        y2: rect.top().value as f64,
    }
}

/// Check if two rectangles intersect
pub(super) fn rectangles_intersect(a: &Rectangle, b: &Rectangle) -> bool {
    !(a.x2 < b.x1 || b.x2 < a.x1 || a.y2 < b.y1 || b.y2 < a.y1)
}

/// Check if two rectangles are adjacent (touching or nearly touching).
///
/// Uses a small tolerance (1 point = ~0.35mm) to handle floating-point
/// precision issues and near-touching cases.
pub fn rectangles_adjacent(a: &Rectangle, b: &Rectangle) -> bool {
    const ADJACENCY_TOLERANCE: f64 = 1.0; // 1 PDF point tolerance

    // Expand rectangle `a` by the tolerance and check if it intersects `b`
    let expanded_a = Rectangle {
        x1: a.x1 - ADJACENCY_TOLERANCE,
        y1: a.y1 - ADJACENCY_TOLERANCE,
        x2: a.x2 + ADJACENCY_TOLERANCE,
        y2: a.y2 + ADJACENCY_TOLERANCE,
    };
    rectangles_intersect(&expanded_a, b)
}

/// Compute the intersection of two rectangles, returning None if they don't overlap.
pub(super) fn intersect_rectangles(a: &Rectangle, b: &Rectangle) -> Option<Rectangle> {
    let x1 = a.x1.max(b.x1);
    let y1 = a.y1.max(b.y1);
    let x2 = a.x2.min(b.x2);
    let y2 = a.y2.min(b.y2);

    // Check if there's a valid intersection (positive area)
    if x1 < x2 && y1 < y2 {
        Some(Rectangle { x1, y1, x2, y2 })
    } else {
        None
    }
}

/// Compute the union of multiple rectangles (axis-aligned bounding box)
pub(super) fn compute_union(rects: &[Rectangle]) -> Option<Rectangle> {
    if rects.is_empty() {
        return None;
    }

    let mut result = rects[0];
    for rect in &rects[1..] {
        result.x1 = result.x1.min(rect.x1);
        result.y1 = result.y1.min(rect.y1);
        result.x2 = result.x2.max(rect.x2);
        result.y2 = result.y2.max(rect.y2);
    }
    Some(result)
}

/// Extract text bounding boxes from a page.
///
/// Merges adjacent characters into line-level regions to reduce
/// the number of regions we need to check for overlap.
pub fn extract_text_regions(page: &PdfPage) -> Vec<ContentRegion> {
    let mut regions = Vec::new();

    let text = match page.text() {
        Ok(t) => t,
        Err(e) => {
            debug!("Failed to get page text: {}", e);
            return regions;
        }
    };

    // Get all character bounds
    let chars = text.chars();
    let mut current_line: Option<Rectangle> = None;
    let mut last_y: Option<f64> = None;

    for char_result in chars.iter() {
        let bounds = match char_result.tight_bounds() {
            Ok(b) => pdf_rect_to_rectangle(&b),
            Err(_) => continue,
        };

        // Check if this character is on a new line (significant y change)
        let is_new_line = match last_y {
            Some(y) => (bounds.y1 - y).abs() > 5.0, // 5 points threshold
            None => false,
        };

        if is_new_line {
            // Save the current line and start a new one
            if let Some(line) = current_line.take() {
                regions.push(ContentRegion { bounds: line });
            }
        }

        // Extend or start the current line
        current_line = Some(match current_line {
            Some(line) => Rectangle {
                x1: line.x1.min(bounds.x1),
                y1: line.y1.min(bounds.y1),
                x2: line.x2.max(bounds.x2),
                y2: line.y2.max(bounds.y2),
            },
            None => bounds,
        });

        last_y = Some(bounds.y1);
    }

    // Don't forget the last line
    if let Some(line) = current_line {
        regions.push(ContentRegion { bounds: line });
    }

    regions
}

/// Extract vector path bounding boxes from a page.
///
/// For direct page paths, extracts their bounds normally.
/// For Form XObjects containing paths, uses the Form XObject's overall bounds
/// rather than descending into internal paths (which have unreliable coordinates).
///
/// Form XObjects may contain content spanning multiple pages (e.g., two-page spreads),
/// so their content bounds are intersected with the page bounds to get the visible region.
/// The Form XObject's BBox acts as a clipping region in the PDF, but pdfium returns
/// content bounds rather than the clipped bounds, so we apply the page intersection manually.
pub fn extract_path_regions(page: &PdfPage) -> Vec<ContentRegion> {
    let mut regions = Vec::new();

    // Get page dimensions for clipping oversized regions
    let page_width = page.width().value as f64;
    let page_height = page.height().value as f64;

    // Page bounds (with small tolerance for edge cases)
    let page_bounds = Rectangle {
        x1: 0.0,
        y1: 0.0,
        x2: page_width,
        y2: page_height,
    };

    for object in page.objects().iter() {
        extract_paths_from_object(&object, &mut regions);
    }

    // Intersect all path regions with page bounds to get visible portions
    // This handles Form XObjects that span multiple pages (e.g., two-page spreads)
    regions
        .into_iter()
        .filter_map(|region| {
            intersect_rectangles(&region.bounds, &page_bounds)
                .map(|bounds| ContentRegion { bounds })
        })
        .collect()
}

/// Extract path regions from a page object.
///
/// For direct Path objects on the page, uses their bounds directly.
///
/// For Form XObjects (XObjectForm), uses the form object's overall bounds rather than
/// descending into its children. This is because:
/// - Form XObjects contain their own coordinate system (form-space)
/// - Child object bounds from pdfium are in form-space, not page-space
/// - The form object itself, as a page object, has bounds in page-space
/// - Using the form's overall bounds correctly represents where its content renders
fn extract_paths_from_object(object: &PdfPageObject, regions: &mut Vec<ContentRegion>) {
    match object {
        PdfPageObject::Path(_) => {
            if let Ok(quad_points) = object.bounds() {
                let bounds = quad_points.to_rect();
                regions.push(ContentRegion {
                    bounds: pdf_rect_to_rectangle(&bounds),
                });
            }
        }
        PdfPageObject::XObjectForm(form_obj) => {
            // Use the form object's overall bounds in page-space.
            // This represents where the form's content renders on the page.
            // We don't recurse into children because their bounds are in form-space.
            if let Ok(quad_points) =
                PdfPageObjectCommon::bounds(form_obj as &dyn PdfPageObjectCommon)
            {
                let bounds = quad_points.to_rect();
                trace!(
                    form_bounds = format!(
                        "({:.1},{:.1})-({:.1},{:.1})",
                        bounds.left().value,
                        bounds.bottom().value,
                        bounds.right().value,
                        bounds.top().value
                    ),
                    child_count = form_obj.len(),
                    "Including Form XObject bounds for path overlap detection"
                );
                regions.push(ContentRegion {
                    bounds: pdf_rect_to_rectangle(&bounds),
                });
            }
        }
        _ => {
            // Ignore other object types (text, image, shading, unsupported)
        }
    }
}

/// Extract image bounding boxes from a page using pdfium.
///
/// This provides an alternative source of image positions when poppler
/// returns invalid coordinates (e.g., negative values from non-standard
/// PDF coordinate transforms).
pub fn extract_pdfium_images(page: &PdfPage) -> Vec<PdfiumImageInfo> {
    let mut images = Vec::new();

    for object in page.objects().iter() {
        // Only process image objects with valid bounds
        if let PdfPageObject::Image(image_obj) = &object
            && let Ok(quad_points) = image_obj.bounds()
        {
            let bounds = quad_points.to_rect();

            // Get pixel dimensions from the image object
            let (width, height) = if let Ok(img) = image_obj.get_raw_image() {
                (img.width() as i32, img.height() as i32)
            } else {
                // Estimate from bounds (at 72 DPI)
                let w = (bounds.width().value * 72.0 / 72.0) as i32;
                let h = (bounds.height().value * 72.0 / 72.0) as i32;
                (w.max(1), h.max(1))
            };

            images.push(PdfiumImageInfo {
                bounds: pdf_rect_to_rectangle(&bounds),
                width,
                height,
            });
        }
    }

    debug!(
        image_count = images.len(),
        "Extracted image bounds from pdfium"
    );

    images
}

/// Calculate the effective DPI of an image based on its pixel dimensions and PDF bounds.
pub fn calculate_image_dpi(info: &ImageInfo) -> f64 {
    // PDF coordinates are in points (1/72 inch)
    let width_inches = info.area.width() / 72.0;
    let height_inches = info.area.height() / 72.0;

    if width_inches <= 0.0 || height_inches <= 0.0 {
        return 72.0; // Default fallback
    }

    let dpi_x = info.width as f64 / width_inches;
    let dpi_y = info.height as f64 / height_inches;

    dpi_x.max(dpi_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rectangles_intersect() {
        let a = Rectangle {
            x1: 0.0,
            y1: 0.0,
            x2: 100.0,
            y2: 100.0,
        };
        let b = Rectangle {
            x1: 50.0,
            y1: 50.0,
            x2: 150.0,
            y2: 150.0,
        };
        let c = Rectangle {
            x1: 200.0,
            y1: 200.0,
            x2: 300.0,
            y2: 300.0,
        };

        assert!(rectangles_intersect(&a, &b));
        assert!(!rectangles_intersect(&a, &c));
        assert!(!rectangles_intersect(&b, &c));
    }

    #[test]
    fn test_compute_union() {
        let rects = vec![
            Rectangle {
                x1: 0.0,
                y1: 0.0,
                x2: 100.0,
                y2: 100.0,
            },
            Rectangle {
                x1: 50.0,
                y1: 50.0,
                x2: 150.0,
                y2: 150.0,
            },
        ];

        let union = compute_union(&rects).unwrap();
        assert_eq!(union.x1, 0.0);
        assert_eq!(union.y1, 0.0);
        assert_eq!(union.x2, 150.0);
        assert_eq!(union.y2, 150.0);
    }
}
