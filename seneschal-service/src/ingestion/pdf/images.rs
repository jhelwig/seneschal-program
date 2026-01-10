//! PDF image extraction using Poppler and qpdf.
//!
//! This module handles extracting images from PDF documents and saving them as WebP files.
//! It uses:
//! - poppler-rs for programmatic access to PDF images with position information
//! - qpdf for extracting transformation matrices (CTMs) to correct image orientation

pub mod compositing;

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use chrono::Utc;
use image::ImageEncoder;
use image::codecs::webp::WebPEncoder;
use poppler::Document as PopplerDocument;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::db::DocumentImage;
use crate::error::{ProcessingError, ServiceResult};

use compositing::{
    ImageInfo, Rectangle, build_cross_page_groups, composite_group,
    extract_image_transforms_with_qpdf, find_matching_transform, is_grayscale_rgb_data,
};

/// Extract images from a PDF document and save them as WebP files.
///
/// Returns a list of DocumentImage records (without descriptions - those are added separately).
///
/// Uses poppler-rs for programmatic access to PDF images with position information,
/// allowing proper layer compositing (e.g., character artwork with drop shadows).
/// Uses qpdf to extract transformation matrices (CTMs) and applies them to correct
/// image orientation (rotation, mirroring).
pub fn extract_pdf_images(
    path: &Path,
    document_id: &str,
    images_dir: &Path,
) -> ServiceResult<Vec<DocumentImage>> {
    // Create images directory for this document
    std::fs::create_dir_all(images_dir).map_err(ProcessingError::Io)?;

    // Extract transformation matrices using qpdf first
    let transforms = match extract_image_transforms_with_qpdf(path) {
        Ok(t) => {
            info!(
                document_id = document_id,
                transforms = t.len(),
                "Extracted CTMs from PDF with qpdf"
            );
            t
        }
        Err(e) => {
            warn!(
                document_id = document_id,
                error = %e,
                "Failed to extract CTMs with qpdf, continuing without transforms"
            );
            HashMap::new()
        }
    };

    // Load PDF with poppler
    let canonical_path = path.canonicalize().map_err(ProcessingError::Io)?;
    let uri = format!("file://{}", canonical_path.display());
    let doc =
        PopplerDocument::from_file(&uri, None).map_err(|e| ProcessingError::TextExtraction {
            page: 0,
            source: Box::new(std::io::Error::other(format!(
                "Failed to load PDF with poppler: {}",
                e
            ))),
        })?;

    let mut all_document_images = Vec::new();
    let mut all_page_images: Vec<ImageInfo> = Vec::new();
    let now = Utc::now();
    let n_pages = doc.n_pages();

    info!(
        document_id = document_id,
        pages = n_pages,
        "Extracting images from PDF with poppler"
    );

    for page_num in 0..n_pages {
        let page = match doc.page(page_num) {
            Some(p) => p,
            None => continue,
        };

        let mappings = page.image_mapping();
        if mappings.is_empty() {
            continue;
        }

        // Get page dimensions
        let (page_width, page_height) = page.size();

        // Extract image info from this page
        let mut page_images: Vec<ImageInfo> = Vec::new();

        for mapping in mappings.iter() {
            // Access the raw mapping data to get image_id and area
            let ptr = mapping.as_ptr();
            let (image_id, area) = unsafe {
                let raw = &*ptr;
                let image_id = raw.image_id;
                let area = Rectangle {
                    x1: raw.area.x1,
                    y1: raw.area.y1,
                    x2: raw.area.x2,
                    y2: raw.area.y2,
                };
                (image_id, area)
            };

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
            // CAIRO_FORMAT_ARGB32 = 0
            // CAIRO_FORMAT_RGB24 = 1
            // CAIRO_FORMAT_A8 = 2 (grayscale/alpha-only - typically SMask data)
            let has_alpha = match format {
                0 => true,  // ARGB32 - color with alpha
                1 => false, // RGB24 - color, no alpha
                2 => {
                    // A8 format is alpha-only data, typically used for SMask
                    // These are not standalone images - they provide transparency
                    // for other images and should be applied via SMask extraction
                    trace!(
                        page = page_num + 1,
                        image_id = image_id,
                        "Skipping A8 format image (probable SMask)"
                    );
                    continue;
                }
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

            // Skip grayscale images - these are typically SMask data that Poppler
            // has converted to RGB format. They should not be extracted as standalone
            // images; they're meant to provide transparency for other images.
            if is_grayscale_rgb_data(&surface_data, width, height, stride) {
                trace!(
                    page = page_num + 1,
                    image_id = image_id,
                    size = format!("{}x{}", width, height),
                    "Skipping grayscale image (probable SMask converted to RGB)"
                );
                continue;
            }

            // Calculate scale factors from PDF points to pixels
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

            // Find matching transformation matrix from qpdf using dimension and position matching
            let matched_transform = find_matching_transform(
                page_num as usize,
                bounds_width,
                bounds_height,
                &area,
                &transforms,
            );

            // If we found a transform with computed_bounds (rotation), use those bounds
            // for overlap detection instead of poppler's potentially incorrect bounds
            let (final_area, transform_matrix, crop_pixels, smask_data, smask_width, smask_height) =
                if let Some(ref t) = matched_transform {
                    trace!(
                        page = page_num + 1,
                        image_id = image_id,
                        poppler_bounds = format!("{:.1} x {:.1}", bounds_width, bounds_height),
                        matrix = ?t.matrix,
                        computed_bounds = ?t.computed_bounds,
                        "Assigning transformation matrix to image"
                    );

                    // Use computed_bounds for overlap detection if available
                    // This fixes rotated images whose poppler bounding box is incorrect
                    let base_area = if let Some((x1, y1, x2, y2)) = t.computed_bounds {
                        Rectangle { x1, y1, x2, y2 }
                    } else {
                        area
                    };

                    // Apply clip_rect if present - the visible area is the intersection
                    // of the image bounds and the clipping rectangle
                    let (final_clipped_area, crop) = if let Some((cx1, cy1, cx2, cy2)) = t.clip_rect
                    {
                        let clipped = Rectangle {
                            x1: base_area.x1.max(cx1),
                            y1: base_area.y1.max(cy1),
                            x2: base_area.x2.min(cx2),
                            y2: base_area.y2.min(cy2),
                        };

                        // Compute which pixels of the image correspond to the clipped region
                        // The base_area defines the full image bounds in PDF points
                        // Map the clipped region back to pixel coordinates
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

                            trace!(
                                page = page_num + 1,
                                image_id = image_id,
                                crop_region =
                                    format!("({}, {}) {}x{}", crop_x, crop_y, crop_w, crop_h),
                                image_size = format!("{}x{}", width, height),
                                "Computed pixel crop region from clip_rect"
                            );

                            Some((crop_x, crop_y, crop_w, crop_h))
                        } else {
                            None
                        };

                        trace!(
                            page = page_num + 1,
                            image_id = image_id,
                            original_area = format!(
                                "({:.1},{:.1})-({:.1},{:.1})",
                                area.x1, area.y1, area.x2, area.y2
                            ),
                            computed_bounds = format!(
                                "({:.1},{:.1})-({:.1},{:.1})",
                                base_area.x1, base_area.y1, base_area.x2, base_area.y2
                            ),
                            clip_rect = format!("({:.1},{:.1})-({:.1},{:.1})", cx1, cy1, cx2, cy2),
                            clipped_area = format!(
                                "({:.1},{:.1})-({:.1},{:.1})",
                                clipped.x1, clipped.y1, clipped.x2, clipped.y2
                            ),
                            "Applied clip rect to image bounds"
                        );
                        (clipped, crop_pixels)
                    } else {
                        trace!(
                            page = page_num + 1,
                            image_id = image_id,
                            original_area = format!(
                                "({:.1},{:.1})-({:.1},{:.1})",
                                area.x1, area.y1, area.x2, area.y2
                            ),
                            corrected_area = format!(
                                "({:.1},{:.1})-({:.1},{:.1})",
                                base_area.x1, base_area.y1, base_area.x2, base_area.y2
                            ),
                            "Using CTM-computed bounds for overlap detection (no clip)"
                        );
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

            page_images.push(ImageInfo {
                image_id,
                area: final_area,
                surface_data,
                width,
                height,
                stride,
                has_alpha,
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

        if page_images.is_empty() {
            continue;
        }

        debug!(
            page = page_num + 1,
            images = page_images.len(),
            "Found images on page"
        );

        // Add to collection of all images (grouping happens after all pages are processed)
        all_page_images.extend(page_images);
    }

    // Now group images considering cross-page overlaps
    debug!(
        document_id = document_id,
        total_images = all_page_images.len(),
        "Building cross-page image groups"
    );

    let groups = build_cross_page_groups(&all_page_images);

    debug!(
        document_id = document_id,
        groups = groups.len(),
        "Built image groups (including cross-page)"
    );

    // Track how many images per page for indexing
    let mut page_image_counts: HashMap<usize, usize> = HashMap::new();

    // Composite each group and save
    for group in groups.iter() {
        let composited = match composite_group(&all_page_images, &group.image_indices) {
            Some(img) => img,
            None => continue,
        };

        let width = composited.width();
        let height = composited.height();

        // Skip images that are too small for vision models (need at least 32x32)
        const MIN_IMAGE_SIZE: u32 = 32;
        if width < MIN_IMAGE_SIZE || height < MIN_IMAGE_SIZE {
            debug!(
                page = group.assigned_page + 1,
                width = width,
                height = height,
                "Skipping small image (below {}x{} threshold)",
                MIN_IMAGE_SIZE,
                MIN_IMAGE_SIZE
            );
            continue;
        }

        // Get the image index for this page
        let page_idx = *page_image_counts.get(&group.assigned_page).unwrap_or(&0);
        page_image_counts.insert(group.assigned_page, page_idx + 1);

        // Save as WebP
        let image_id = Uuid::new_v4().to_string();
        let page_display = (group.assigned_page + 1) as i32;
        let webp_filename = format!("page_{}_img_{}.webp", page_display, page_idx);
        let webp_path = images_dir.join(&webp_filename);

        let file = match File::create(&webp_path) {
            Ok(f) => f,
            Err(e) => {
                warn!(
                    page = page_display,
                    group = page_idx,
                    error = %e,
                    "Failed to create image file"
                );
                continue;
            }
        };

        let encoder = WebPEncoder::new_lossless(file);
        if let Err(e) = encoder.write_image(
            composited.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        ) {
            warn!(
                page = page_display,
                group = page_idx,
                error = %e,
                "Failed to encode image as WebP"
            );
            let _ = std::fs::remove_file(&webp_path);
            continue;
        }

        // Collect source pages for this image (may span multiple pages for composites)
        let mut source_pages: Vec<i32> = group
            .image_indices
            .iter()
            .map(|&i| (all_page_images[i].page_number + 1) as i32)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        source_pages.sort();

        // Log if this was a cross-page composite
        let is_cross_page = source_pages.len() > 1;
        if is_cross_page {
            debug!(
                assigned_page = page_display,
                source_pages = ?source_pages,
                layers = group.image_indices.len(),
                "Created cross-page composite"
            );
        }

        all_document_images.push(DocumentImage {
            id: image_id,
            document_id: document_id.to_string(),
            page_number: page_display,
            image_index: page_idx as i32,
            internal_path: webp_path.to_string_lossy().to_string(),
            mime_type: "image/webp".to_string(),
            width: Some(width),
            height: Some(height),
            description: None,
            source_pages: Some(source_pages),
            created_at: now,
        });

        debug!(
            page = page_display,
            group = page_idx,
            layers = group.image_indices.len(),
            width = width,
            height = height,
            "Extracted composited image"
        );
    }

    // Sort by page number then image index for consistent ordering
    all_document_images.sort_by(|a, b| {
        a.page_number
            .cmp(&b.page_number)
            .then(a.image_index.cmp(&b.image_index))
    });

    info!(
        document_id = document_id,
        total_images = all_document_images.len(),
        "Extracted and saved images from PDF"
    );

    Ok(all_document_images)
}
