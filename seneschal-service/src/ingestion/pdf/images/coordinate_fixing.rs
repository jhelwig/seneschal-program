//! Coordinate fixing for invalid poppler image bounds.

use std::collections::{HashMap, HashSet};

use tracing::debug;

use super::overlap::PdfiumImageInfo;
use super::types::{ImageInfo, PageBoxes, Rectangle, is_valid_bounds};

/// Fix invalid poppler coordinates using CropBox offset correction.
///
/// Some PDFs have a CropBox that is offset from the MediaBox origin. Poppler
/// returns image positions in MediaBox coordinates, but reports page size as
/// CropBox size. This function detects such pages and applies the CropBox
/// offset to correct the coordinates.
pub fn fix_invalid_image_bounds(
    all_images: &mut [ImageInfo],
    page_pdfium_images: &HashMap<usize, Vec<PdfiumImageInfo>>,
    page_boxes: &HashMap<usize, PageBoxes>,
) {
    // Group images by page
    let mut images_by_page: HashMap<usize, Vec<usize>> = HashMap::new();
    for (idx, info) in all_images.iter().enumerate() {
        images_by_page
            .entry(info.page_number)
            .or_default()
            .push(idx);
    }

    // Process each page
    for (page_num, image_indices) in images_by_page {
        // Check if this page has any invalid bounds
        let has_invalid = image_indices.iter().any(|&idx| {
            let info = &all_images[idx];
            !is_valid_bounds(&info.area, info.page_width, info.page_height)
        });

        if !has_invalid {
            continue;
        }

        // Get page boxes for coordinate correction
        let boxes = page_boxes.get(&page_num);
        let crop_offset = boxes.and_then(|b| b.crop_box.map(|crop| (crop.x1, crop.y1)));

        // Try CropBox offset correction first
        if let Some((offset_x, offset_y)) = crop_offset {
            debug!(
                page = page_num + 1,
                offset_x = format!("{:.1}", offset_x),
                offset_y = format!("{:.1}", offset_y),
                "Applying CropBox offset correction"
            );

            for &idx in &image_indices {
                let info = &all_images[idx];
                if is_valid_bounds(&info.area, info.page_width, info.page_height) {
                    continue;
                }

                // Apply CropBox offset: poppler returns coordinates offset from CropBox origin
                // in the negative direction, so we ADD the offset to bring them into page space
                let corrected = Rectangle {
                    x1: info.area.x1 + offset_x,
                    y1: info.area.y1 + offset_y,
                    x2: info.area.x2 + offset_x,
                    y2: info.area.y2 + offset_y,
                };

                // Check if the correction results in valid bounds
                if is_valid_bounds(&corrected, info.page_width, info.page_height) {
                    debug!(
                        page = page_num + 1,
                        image_id = info.image_id,
                        old_bounds = format!(
                            "({:.1},{:.1})-({:.1},{:.1})",
                            info.area.x1, info.area.y1, info.area.x2, info.area.y2
                        ),
                        new_bounds = format!(
                            "({:.1},{:.1})-({:.1},{:.1})",
                            corrected.x1, corrected.y1, corrected.x2, corrected.y2
                        ),
                        "Applied CropBox offset correction"
                    );
                    all_images[idx].area = corrected;
                }
            }
        }

        // Check if there are still invalid bounds after CropBox correction
        let still_has_invalid = image_indices.iter().any(|&idx| {
            let info = &all_images[idx];
            !is_valid_bounds(&info.area, info.page_width, info.page_height)
        });

        if !still_has_invalid {
            continue;
        }

        // Fall back to pdfium image matching for remaining invalid bounds
        let pdfium_images = match page_pdfium_images.get(&page_num) {
            Some(imgs) => imgs,
            None => {
                // No pdfium images - use full page bounds as last resort
                for &idx in &image_indices {
                    let info = &all_images[idx];
                    if !is_valid_bounds(&info.area, info.page_width, info.page_height) {
                        let page_bounds = Rectangle {
                            x1: 0.0,
                            y1: 0.0,
                            x2: info.page_width,
                            y2: info.page_height,
                        };
                        debug!(
                            page = page_num + 1,
                            image_id = info.image_id,
                            "Using full page bounds as fallback"
                        );
                        all_images[idx].area = page_bounds;
                    }
                }
                continue;
            }
        };

        debug!(
            page = page_num + 1,
            poppler_images = image_indices.len(),
            pdfium_images = pdfium_images.len(),
            "Attempting pdfium image matching fallback"
        );

        // Track which pdfium images have been used
        let mut used_pdfium: HashSet<usize> = HashSet::new();

        // For each image with invalid bounds, try to find a matching pdfium image
        for &idx in &image_indices {
            let info = &all_images[idx];
            if is_valid_bounds(&info.area, info.page_width, info.page_height) {
                continue; // Already fixed
            }

            // Try to find a matching pdfium image by pixel dimensions
            let mut best_match: Option<(usize, f64)> = None;

            for (pdfium_idx, pdfium_img) in pdfium_images.iter().enumerate() {
                if used_pdfium.contains(&pdfium_idx) {
                    continue;
                }

                let w_diff = (pdfium_img.width - info.width).abs() as f64;
                let h_diff = (pdfium_img.height - info.height).abs() as f64;
                let total_diff = w_diff + h_diff;

                if total_diff < 3.0 {
                    match best_match {
                        None => best_match = Some((pdfium_idx, total_diff)),
                        Some((_, best_diff)) if total_diff < best_diff => {
                            best_match = Some((pdfium_idx, total_diff))
                        }
                        _ => {}
                    }
                }
            }

            if let Some((pdfium_idx, _)) = best_match {
                let pdfium_bounds = &pdfium_images[pdfium_idx].bounds;
                debug!(
                    page = page_num + 1,
                    image_id = info.image_id,
                    old_bounds = format!(
                        "({:.1},{:.1})-({:.1},{:.1})",
                        info.area.x1, info.area.y1, info.area.x2, info.area.y2
                    ),
                    new_bounds = format!(
                        "({:.1},{:.1})-({:.1},{:.1})",
                        pdfium_bounds.x1, pdfium_bounds.y1, pdfium_bounds.x2, pdfium_bounds.y2
                    ),
                    "Replaced with pdfium bounds"
                );
                all_images[idx].area = *pdfium_bounds;
                used_pdfium.insert(pdfium_idx);
            } else {
                // Last resort: use full page bounds
                let page_bounds = Rectangle {
                    x1: 0.0,
                    y1: 0.0,
                    x2: info.page_width,
                    y2: info.page_height,
                };
                debug!(
                    page = page_num + 1,
                    image_id = info.image_id,
                    "Using full page bounds as last resort"
                );
                all_images[idx].area = page_bounds;
            }
        }
    }
}
