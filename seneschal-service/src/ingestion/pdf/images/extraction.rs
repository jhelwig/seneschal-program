//! Image extraction from PDF pages using poppler.

use std::collections::HashMap;

use poppler::Document as PopplerDocument;
use tracing::{debug, trace};

use crate::error::ServiceResult;

use super::transforms::{ImageTransform, find_matching_transform};
use super::types::{ImageInfo, Rectangle};

/// Extract image info from all pages using poppler.
pub fn extract_all_image_info(
    doc: &PopplerDocument,
    transforms: &HashMap<usize, Vec<ImageTransform>>,
) -> ServiceResult<Vec<ImageInfo>> {
    let mut all_images = Vec::new();
    let n_pages = doc.n_pages();

    for page_num in 0..n_pages {
        let page = match doc.page(page_num) {
            Some(p) => p,
            None => continue,
        };

        let mappings = page.image_mapping();
        if mappings.is_empty() {
            continue;
        }

        let (page_width, page_height) = page.size();

        for mapping in mappings.iter() {
            // Access the raw mapping data to get image_id and area
            let ptr = mapping.as_ptr();
            let (image_id, raw_area, area) = unsafe {
                let raw = &*ptr;
                let image_id = raw.image_id;
                let raw_area = Rectangle {
                    x1: raw.area.x1,
                    y1: raw.area.y1,
                    x2: raw.area.x2,
                    y2: raw.area.y2,
                };
                // Poppler uses top-left origin (y increases downward), but pdfium
                // uses bottom-left origin (y increases upward). Convert coordinates.
                let area = Rectangle {
                    x1: raw.area.x1,
                    y1: page_height - raw.area.y2, // Convert: old top -> new bottom
                    x2: raw.area.x2,
                    y2: page_height - raw.area.y1, // Convert: old bottom -> new top
                };
                (image_id, raw_area, area)
            };

            trace!(
                page = page_num + 1,
                image_id = image_id,
                raw = format!(
                    "({:.1},{:.1})-({:.1},{:.1})",
                    raw_area.x1, raw_area.y1, raw_area.x2, raw_area.y2
                ),
                converted = format!(
                    "({:.1},{:.1})-({:.1},{:.1})",
                    area.x1, area.y1, area.x2, area.y2
                ),
                page_dims = format!("{:.1}x{:.1}", page_width, page_height),
                "Poppler area"
            );

            // Get the image surface
            let surface = match page.image(image_id) {
                Some(s) => s,
                None => {
                    debug!(
                        page = page_num + 1,
                        image_id = image_id,
                        "Could not get image surface"
                    );
                    continue;
                }
            };

            // Get surface properties using cairo FFI
            let raw_surface = surface.to_raw_none();
            let (format, width, height, stride) = unsafe {
                use cairo::ffi;
                let format = ffi::cairo_image_surface_get_format(raw_surface);
                let width = ffi::cairo_image_surface_get_width(raw_surface);
                let height = ffi::cairo_image_surface_get_height(raw_surface);
                let stride = ffi::cairo_image_surface_get_stride(raw_surface);
                (format, width, height, stride)
            };

            // Determine image type from format
            let (has_alpha, is_grayscale) = match format {
                0 => (true, false),  // ARGB32
                1 => (false, false), // RGB24
                2 => (false, true),  // A8 (grayscale)
                _ => {
                    debug!(
                        page = page_num + 1,
                        image_id = image_id,
                        format = format,
                        "Unknown Cairo format"
                    );
                    continue;
                }
            };

            // Get surface data
            let data_ptr = unsafe {
                use cairo::ffi;
                ffi::cairo_image_surface_get_data(raw_surface)
            };

            if data_ptr.is_null() {
                debug!(
                    page = page_num + 1,
                    image_id = image_id,
                    "Null surface data pointer"
                );
                continue;
            }

            let data_len = (stride * height) as usize;
            let surface_data = unsafe { std::slice::from_raw_parts(data_ptr, data_len).to_vec() };

            // Calculate scale factors
            let bounds_width = area.width();
            let bounds_height = area.height();
            let scale_x = if bounds_width > 0.0 {
                width as f64 / bounds_width
            } else {
                1.0
            };
            let scale_y = if bounds_height > 0.0 {
                height as f64 / bounds_height
            } else {
                1.0
            };

            // Find matching transformation matrix
            let matched_transform = find_matching_transform(
                page_num as usize,
                bounds_width,
                bounds_height,
                &area,
                transforms,
            );

            // Extract transform data
            let (final_area, transform_matrix, crop_pixels, smask_data, smask_width, smask_height) =
                if let Some(ref t) = matched_transform {
                    trace!(
                        page = page_num + 1,
                        image_id = image_id,
                        matrix = ?t.matrix,
                        "Matched image to transformation"
                    );

                    // Use computed_bounds for overlap detection if available and valid
                    // Fall back to poppler area if CTM bounds are clearly wrong
                    // (negative coordinates or outside reasonable page bounds)
                    let base_area = if let Some((x1, y1, x2, y2)) = t.computed_bounds {
                        let ctm_bounds = Rectangle { x1, y1, x2, y2 };
                        // Validate: bounds should be within reasonable page area
                        // Allow some margin (10%) outside page for bleed/transforms
                        let margin = page_width.max(page_height) * 0.1;
                        let is_valid = x1 >= -margin
                            && y1 >= -margin
                            && x2 <= page_width + margin
                            && y2 <= page_height + margin;
                        trace!(
                            page = page_num + 1,
                            image_id = image_id,
                            ctm_bounds = format!("({:.1},{:.1})-({:.1},{:.1})", x1, y1, x2, y2),
                            page_dims = format!("{:.1}x{:.1}", page_width, page_height),
                            margin = format!("{:.1}", margin),
                            is_valid = is_valid,
                            "CTM bounds validation"
                        );
                        if is_valid { ctm_bounds } else { area }
                    } else {
                        area
                    };

                    // Apply clip_rect if present
                    let (final_clipped_area, crop) = if let Some((cx1, cy1, cx2, cy2)) = t.clip_rect
                    {
                        let clipped = Rectangle {
                            x1: base_area.x1.max(cx1),
                            y1: base_area.y1.max(cy1),
                            x2: base_area.x2.min(cx2),
                            y2: base_area.y2.min(cy2),
                        };

                        // Compute pixel crop region
                        let base_width = base_area.width();
                        let base_height = base_area.height();

                        let crop_pixels = if base_width > 0.0 && base_height > 0.0 {
                            let px_per_pt_x = width as f64 / base_width;
                            let px_per_pt_y = height as f64 / base_height;

                            let crop_x =
                                ((clipped.x1 - base_area.x1) * px_per_pt_x).max(0.0) as u32;
                            let crop_y =
                                ((clipped.y1 - base_area.y1) * px_per_pt_y).max(0.0) as u32;
                            let crop_w = (clipped.width() * px_per_pt_x).max(1.0) as u32;
                            let crop_h = (clipped.height() * px_per_pt_y).max(1.0) as u32;

                            // Clamp to image bounds
                            let crop_w = crop_w.min(width as u32 - crop_x);
                            let crop_h = crop_h.min(height as u32 - crop_y);

                            Some((crop_x, crop_y, crop_w, crop_h))
                        } else {
                            None
                        };

                        (clipped, crop_pixels)
                    } else {
                        (base_area, None)
                    };

                    (
                        final_clipped_area,
                        Some(t.matrix),
                        crop,
                        t.smask_data.clone(),
                        t.smask_width,
                        t.smask_height,
                    )
                } else {
                    (area, None, None, None, None, None)
                };

            trace!(
                page = page_num + 1,
                image_id = image_id,
                bounds = format!(
                    "({:.1},{:.1})-({:.1},{:.1})",
                    final_area.x1, final_area.y1, final_area.x2, final_area.y2
                ),
                width = width,
                height = height,
                "Image bounds"
            );

            all_images.push(ImageInfo {
                image_id,
                area: final_area,
                surface_data,
                width,
                height,
                stride,
                has_alpha,
                is_grayscale,
                scale_x,
                scale_y,
                page_number: page_num as usize,
                page_width,
                page_height,
                transform: transform_matrix,
                crop_pixels,
                smask_data,
                smask_width,
                smask_height,
            });
        }
    }

    Ok(all_images)
}
