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
mod coordinate_fixing;
mod extraction;
mod image_saving;
pub mod overlap;
pub mod region_render;
pub mod transforms;
mod types;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::Utc;
use poppler::Document as PopplerDocument;
use tracing::{debug, info, trace, warn};

use crate::config::ImageExtractionConfig;
use crate::db::{DocumentImage, ImageType};
use crate::error::{ProcessingError, ServiceResult};

use background::{ImageSignature, detect_backgrounds, is_background};
use coordinate_fixing::fix_invalid_image_bounds;
use extraction::extract_all_image_info;
use image_saving::{save_group_region_render, save_individual_image};
use overlap::{
    ContentRegion, PdfiumImageInfo, calculate_group_region_dpi, detect_overlap_groups,
    extract_path_regions, extract_pdfium_images, extract_text_regions,
};
use region_render::render_page_region;
use transforms::extract_image_transforms_with_qpdf;
use types::extract_page_boxes;

pub use types::{ImageInfo, PageBoxes, Rectangle};

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
        renders = results
            .iter()
            .filter(|r| r.image_type == ImageType::Render)
            .count(),
        "Extracted and saved images from PDF"
    );

    Ok(results)
}
