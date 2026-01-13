//! PDF image extraction with overlap detection and region rendering.
//!
//! This module extracts images from PDF documents with the following behavior:
//! - Each image is extracted individually (no compositing)
//! - Background images (covering 90%+ of page, appearing on multiple pages) are extracted once
//! - When overlap is detected (with text, paths, or other images), a page region render
//!   is also saved to capture the composited appearance
//!
//! Uses:
//! - poppler-rs for programmatic access to PDF images with position information
//! - qpdf for extracting transformation matrices (CTMs) to correct image orientation
//! - pdfium-render for text/path bounding boxes and page region rendering

pub mod background;
pub mod overlap;
pub mod region_render;
pub mod transforms;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;

use chrono::Utc;
use image::ImageEncoder;
use image::codecs::webp::WebPEncoder;
use poppler::Document as PopplerDocument;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::config::ImageExtractionConfig;
use crate::db::{DocumentImage, ImageType};
use crate::error::{ProcessingError, ServiceResult};

use background::{ImageSignature, detect_backgrounds, is_background};
use overlap::{
    ContentRegion, OverlapGroup, PdfiumImageInfo, calculate_group_region_dpi,
    detect_overlap_groups, extract_path_regions, extract_pdfium_images, extract_text_regions,
};
use region_render::render_page_region;
use transforms::{
    ImageTransform, apply_smask, apply_transform, convert_to_rgba,
    extract_image_transforms_with_qpdf, find_matching_transform, needs_transformation,
};

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

/// Extract images from a PDF document and save them as WebP files.
///
/// Returns a list of DocumentImage records (without descriptions - those are added separately).
///
/// The extraction process:
/// 1. Extract all images with poppler and match to qpdf CTMs for orientation
/// 2. Detect background images that appear across multiple pages
/// 3. Extract text and path bounding boxes with pdfium-render
/// 4. For each image:
///    - If background: extract once, skip duplicates, no overlap check
///    - If non-background: extract, check overlaps, render region if needed
pub fn extract_pdf_images(
    path: &Path,
    document_id: &str,
    images_dir: &Path,
    config: &ImageExtractionConfig,
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

    // Load PDF with poppler for image extraction
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

    // Load PDF with pdfium for text/path extraction and region rendering
    let pdfium = super::create_pdfium()?;
    let pdfium_doc =
        pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| ProcessingError::TextExtraction {
                page: 0,
                source: Box::new(std::io::Error::other(format!(
                    "Failed to load PDF with pdfium: {}",
                    e
                ))),
            })?;

    let n_pages = doc.n_pages();
    info!(
        document_id = document_id,
        pages = n_pages,
        "Extracting images from PDF"
    );

    // Phase 1: Extract all image info from all pages
    let mut all_images = extract_all_image_info(&doc, &transforms)?;

    if all_images.is_empty() {
        info!(document_id = document_id, "No images found in PDF");
        return Ok(vec![]);
    }

    info!(
        document_id = document_id,
        total_images = all_images.len(),
        "Found images in PDF"
    );

    // Phase 2: Detect background images
    let background_signatures = detect_backgrounds(&all_images, config);

    info!(
        document_id = document_id,
        background_signatures = background_signatures.len(),
        "Detected background image signatures"
    );

    // Phase 3: Extract text, path, and image regions per page using pdfium
    let mut page_text_regions: HashMap<usize, Vec<ContentRegion>> = HashMap::new();
    let mut page_path_regions: HashMap<usize, Vec<ContentRegion>> = HashMap::new();
    let mut page_pdfium_images: HashMap<usize, Vec<PdfiumImageInfo>> = HashMap::new();
    let mut page_boxes: HashMap<usize, PageBoxes> = HashMap::new();

    for page_num in 0..pdfium_doc.pages().len() {
        if let Ok(page) = pdfium_doc.pages().get(page_num) {
            let text_regions = extract_text_regions(&page);
            let path_regions = extract_path_regions(&page);
            let pdfium_images = extract_pdfium_images(&page);
            let boxes = extract_page_boxes(&page);

            trace!(
                page = page_num + 1,
                text_regions = text_regions.len(),
                path_regions = path_regions.len(),
                pdfium_images = pdfium_images.len(),
                media_box = boxes
                    .media_box
                    .map(|b| format!("({:.1},{:.1})-({:.1},{:.1})", b.x1, b.y1, b.x2, b.y2)),
                crop_box = boxes
                    .crop_box
                    .map(|b| format!("({:.1},{:.1})-({:.1},{:.1})", b.x1, b.y1, b.x2, b.y2)),
                "Extracted content regions from page"
            );

            page_text_regions.insert(page_num as usize, text_regions);
            page_path_regions.insert(page_num as usize, path_regions);
            page_pdfium_images.insert(page_num as usize, pdfium_images);
            page_boxes.insert(page_num as usize, boxes);
        }
    }

    // Phase 3b: Fix invalid poppler coordinates using pdfium fallback
    fix_invalid_image_bounds(&mut all_images, &page_pdfium_images, &page_boxes);

    // Phase 4: Save all individual images first
    let mut results = Vec::new();
    let mut page_image_counts: HashMap<usize, usize> = HashMap::new();
    let mut extracted_backgrounds: HashSet<ImageSignature> = HashSet::new();
    let now = Utc::now();

    // Map from global image index to saved DocumentImage id
    let mut saved_image_ids: HashMap<usize, String> = HashMap::new();

    for (image_idx, image_info) in all_images.iter().enumerate() {
        let is_bg = is_background(image_info, &background_signatures);
        let signature = ImageSignature::from_image(image_info);

        // Skip duplicate background images (only extract once)
        if is_bg {
            if extracted_backgrounds.contains(&signature) {
                debug!(
                    page = image_info.page_number + 1,
                    image_id = image_info.image_id,
                    "Skipping duplicate background image"
                );
                continue;
            }
            extracted_backgrounds.insert(signature);
        }

        // Get image index for this page
        let page_idx = *page_image_counts.get(&image_info.page_number).unwrap_or(&0);
        page_image_counts.insert(image_info.page_number, page_idx + 1);

        let page_display = (image_info.page_number + 1) as i32;

        // Save individual image
        let individual_image = match save_individual_image(
            image_info,
            images_dir,
            document_id,
            page_display,
            page_idx,
            if is_bg {
                ImageType::Background
            } else {
                ImageType::Individual
            },
            now,
        )? {
            Some(img) => img,
            None => continue, // Image was intentionally skipped (e.g., too small)
        };

        saved_image_ids.insert(image_idx, individual_image.id.clone());
        results.push(individual_image);
    }

    // Phase 5: Detect overlap groups and create region renders
    // Group images by page
    let mut images_by_page: HashMap<usize, Vec<usize>> = HashMap::new();
    for (idx, info) in all_images.iter().enumerate() {
        images_by_page
            .entry(info.page_number)
            .or_default()
            .push(idx);
    }

    // Helper to check if an index is a background
    let is_background_idx =
        |idx: usize| -> bool { is_background(&all_images[idx], &background_signatures) };

    // Track overlap group counter per page for naming
    let mut page_group_counts: HashMap<usize, usize> = HashMap::new();

    for (page_num, page_image_indices) in images_by_page.iter() {
        let text_regions = page_text_regions
            .get(page_num)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let path_regions = page_path_regions
            .get(page_num)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        // Detect overlap groups for this page
        let overlap_groups = detect_overlap_groups(
            page_image_indices,
            &all_images,
            text_regions,
            path_regions,
            is_background_idx,
        );

        // Create one region render per overlap group
        for group in overlap_groups {
            // Get group index for this page
            let group_idx = *page_group_counts.get(page_num).unwrap_or(&0);
            page_group_counts.insert(*page_num, group_idx + 1);

            let page_display = (*page_num + 1) as i32;

            // Calculate DPI for this group
            let region_dpi =
                calculate_group_region_dpi(&group, &all_images, config.text_overlap_min_dpi);

            debug!(
                page = page_display,
                group_idx = group_idx,
                images = group.image_indices.len(),
                has_text_overlap = group.has_text_overlap,
                has_path_overlap = group.has_path_overlap,
                region_dpi = format!("{:.1}", region_dpi),
                region = format!(
                    "({:.1},{:.1})-({:.1},{:.1})",
                    group.combined_region.x1,
                    group.combined_region.y1,
                    group.combined_region.x2,
                    group.combined_region.y2
                ),
                "Rendering region for overlap group"
            );

            match render_page_region(&pdfium, path, *page_num, &group.combined_region, region_dpi) {
                Ok(region_image) => {
                    // Find the first saved image in this group to use as source_image_id
                    let source_image_id = group
                        .image_indices
                        .iter()
                        .find_map(|&idx| saved_image_ids.get(&idx))
                        .cloned();

                    // Save region render
                    match save_group_region_render(
                        &region_image,
                        images_dir,
                        document_id,
                        page_display,
                        group_idx,
                        source_image_id.as_deref(),
                        &group,
                        now,
                    ) {
                        Ok(region_doc_image) => {
                            // Mark all images in this group as having a region render
                            for &idx in &group.image_indices {
                                if let Some(img_id) = saved_image_ids.get(&idx)
                                    && let Some(doc_img) =
                                        results.iter_mut().find(|r| &r.id == img_id)
                                {
                                    doc_img.has_region_render = true;
                                }
                            }
                            results.push(region_doc_image);
                        }
                        Err(e) => {
                            warn!(
                                page = page_display,
                                group_idx = group_idx,
                                error = %e,
                                "Failed to save region render"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        page = page_display,
                        group_idx = group_idx,
                        error = %e,
                        "Failed to render page region"
                    );
                }
            }
        }
    }

    // Sort by page number then image index for consistent ordering
    results.sort_by(|a, b| {
        a.page_number
            .cmp(&b.page_number)
            .then(a.image_index.cmp(&b.image_index))
    });

    info!(
        document_id = document_id,
        total_images = results.len(),
        individual = results
            .iter()
            .filter(|r| r.image_type == ImageType::Individual)
            .count(),
        background = results
            .iter()
            .filter(|r| r.image_type == ImageType::Background)
            .count(),
        region_renders = results
            .iter()
            .filter(|r| r.image_type == ImageType::RegionRender)
            .count(),
        "Extracted and saved images from PDF"
    );

    Ok(results)
}

/// Extract image info from all pages using poppler.
fn extract_all_image_info(
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

/// Check if image bounds are valid (within reasonable page bounds).
fn is_valid_bounds(bounds: &Rectangle, page_width: f64, page_height: f64) -> bool {
    // Allow some margin (10%) for bleed/transforms
    let margin = page_width.max(page_height) * 0.1;

    bounds.x1 >= -margin
        && bounds.y1 >= -margin
        && bounds.x2 <= page_width + margin
        && bounds.y2 <= page_height + margin
}

/// Information about page boundary boxes
struct PageBoxes {
    /// The MediaBox (full PDF canvas)
    media_box: Option<Rectangle>,
    /// The CropBox (visible area)
    crop_box: Option<Rectangle>,
}

/// Extract page boundary boxes from pdfium
fn extract_page_boxes(page: &pdfium_render::prelude::PdfPage) -> PageBoxes {
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

/// Fix invalid poppler coordinates using CropBox offset correction.
///
/// Some PDFs have a CropBox that is offset from the MediaBox origin. Poppler
/// returns image positions in MediaBox coordinates, but reports page size as
/// CropBox size. This function detects such pages and applies the CropBox
/// offset to correct the coordinates.
fn fix_invalid_image_bounds(
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

/// Save an individual image to disk.
///
/// Returns `Ok(Some(image))` if saved successfully, `Ok(None)` if the image
/// was intentionally skipped (e.g., too small), or `Err` for actual failures.
fn save_individual_image(
    info: &ImageInfo,
    images_dir: &Path,
    document_id: &str,
    page_number: i32,
    image_index: usize,
    image_type: ImageType,
    created_at: chrono::DateTime<Utc>,
) -> ServiceResult<Option<DocumentImage>> {
    // Convert to RGBA
    let mut img = convert_to_rgba(info);

    // Apply SMask if present
    if let (Some(smask_data), Some(smask_width), Some(smask_height)) =
        (&info.smask_data, info.smask_width, info.smask_height)
    {
        apply_smask(&mut img, smask_data, smask_width, smask_height);
    }

    // Apply transformation if needed
    let img = if let Some(ref matrix) = info.transform {
        if needs_transformation(matrix) {
            apply_transform(&img, matrix)
        } else {
            img
        }
    } else {
        img
    };

    // Apply pixel crop if needed
    let img = if let Some((crop_x, crop_y, crop_w, crop_h)) = info.crop_pixels {
        image::imageops::crop_imm(&img, crop_x, crop_y, crop_w, crop_h).to_image()
    } else {
        img
    };

    let width = img.width();
    let height = img.height();

    // Skip images that are too small (not an error, just not useful)
    const MIN_IMAGE_SIZE: u32 = 32;
    if width < MIN_IMAGE_SIZE || height < MIN_IMAGE_SIZE {
        debug!(
            page = page_number,
            image_index = image_index,
            width = width,
            height = height,
            "Skipping image: too small"
        );
        return Ok(None);
    }

    // Save as WebP
    let image_id = Uuid::new_v4().to_string();
    let webp_filename = format!("page_{}_img_{}.webp", page_number, image_index);
    let webp_path = images_dir.join(&webp_filename);

    let file = File::create(&webp_path).map_err(|e| ProcessingError::TextExtraction {
        page: page_number as u32,
        source: Box::new(e),
    })?;

    let encoder = WebPEncoder::new_lossless(file);
    encoder
        .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| ProcessingError::TextExtraction {
            page: page_number as u32,
            source: Box::new(std::io::Error::other(format!(
                "Failed to encode WebP: {}",
                e
            ))),
        })?;

    debug!(
        page = page_number,
        image_index = image_index,
        width = width,
        height = height,
        image_type = ?image_type,
        "Saved individual image"
    );

    Ok(Some(DocumentImage {
        id: image_id,
        document_id: document_id.to_string(),
        page_number,
        image_index: image_index as i32,
        internal_path: webp_path.to_string_lossy().to_string(),
        mime_type: "image/webp".to_string(),
        width: Some(width),
        height: Some(height),
        description: None,
        source_pages: Some(vec![page_number]),
        image_type,
        source_image_id: None,
        has_region_render: false,
        created_at,
    }))
}

/// Save a region render for an overlap group to disk.
#[allow(clippy::too_many_arguments)]
fn save_group_region_render(
    image: &image::RgbaImage,
    images_dir: &Path,
    document_id: &str,
    page_number: i32,
    group_index: usize,
    source_image_id: Option<&str>,
    group: &OverlapGroup,
    created_at: chrono::DateTime<Utc>,
) -> ServiceResult<DocumentImage> {
    let width = image.width();
    let height = image.height();

    let image_id = Uuid::new_v4().to_string();
    // Name using group index to indicate this is a grouped region render
    let webp_filename = format!("page_{}_group_{}_region.webp", page_number, group_index);
    let webp_path = images_dir.join(&webp_filename);

    let file = File::create(&webp_path).map_err(|e| ProcessingError::TextExtraction {
        page: page_number as u32,
        source: Box::new(e),
    })?;

    let encoder = WebPEncoder::new_lossless(file);
    encoder
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| ProcessingError::TextExtraction {
            page: page_number as u32,
            source: Box::new(std::io::Error::other(format!(
                "Failed to encode region WebP: {}",
                e
            ))),
        })?;

    debug!(
        page = page_number,
        group_index = group_index,
        images_in_group = group.image_indices.len(),
        width = width,
        height = height,
        "Saved overlap group region render"
    );

    Ok(DocumentImage {
        id: image_id,
        document_id: document_id.to_string(),
        page_number,
        image_index: group_index as i32,
        internal_path: webp_path.to_string_lossy().to_string(),
        mime_type: "image/webp".to_string(),
        width: Some(width),
        height: Some(height),
        description: None,
        source_pages: Some(vec![page_number]),
        image_type: ImageType::RegionRender,
        source_image_id: source_image_id.map(String::from),
        has_region_render: false,
        created_at,
    })
}
