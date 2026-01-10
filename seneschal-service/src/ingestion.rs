use chrono::Utc;
use image::codecs::webp::WebPEncoder;
use image::{ImageEncoder, RgbaImage};
use pdfium_render::prelude::*;
use poppler::Document as PopplerDocument;
use qpdf::{QPdf, StreamDecodeLevel};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::config::EmbeddingsConfig;
use crate::db::{Chunk, DocumentImage};
use crate::error::{ProcessingError, ServiceError, ServiceResult};
use crate::tools::AccessLevel;

/// Rectangle representing image position on a PDF page
#[derive(Debug, Clone, Copy)]
struct Rectangle {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl Rectangle {
    fn area(&self) -> f64 {
        (self.x2 - self.x1).abs() * (self.y2 - self.y1).abs()
    }

    fn width(&self) -> f64 {
        (self.x2 - self.x1).abs()
    }

    fn height(&self) -> f64 {
        (self.y2 - self.y1).abs()
    }
}

/// Information about an extracted PDF image
struct ImageInfo {
    image_id: i32,
    area: Rectangle,
    surface_data: Vec<u8>,
    width: i32,
    height: i32,
    stride: i32,
    has_alpha: bool,
    /// Scale factor from PDF points to pixels (width)
    scale_x: f64,
    /// Scale factor from PDF points to pixels (height)
    scale_y: f64,
    /// Page number (0-indexed)
    page_number: usize,
    /// Page width in PDF points
    page_width: f64,
    /// Page height in PDF points
    page_height: f64,
    /// Transformation matrix from PDF CTM (if found)
    transform: Option<[f64; 6]>,
    /// Pixel crop region when a clip_rect was applied (x, y, width, height in pixels)
    /// If Some, only this portion of the image should be used during compositing
    crop_pixels: Option<(u32, u32, u32, u32)>,
    /// Soft mask (SMask) data for transparency, if the image has one
    /// This is raw grayscale pixel data where 0 = transparent, 255 = opaque
    smask_data: Option<Vec<u8>>,
    /// Width of the SMask image in pixels
    smask_width: Option<u32>,
    /// Height of the SMask image in pixels
    smask_height: Option<u32>,
}

/// Check if two rectangles overlap by more than a threshold percentage
fn rectangles_overlap(a: &Rectangle, b: &Rectangle, threshold: f64) -> bool {
    let x_overlap = f64::max(0.0, f64::min(a.x2, b.x2) - f64::max(a.x1, b.x1));
    let y_overlap = f64::max(0.0, f64::min(a.y2, b.y2) - f64::max(a.y1, b.y1));
    let overlap_area = x_overlap * y_overlap;
    let smaller_area = a.area().min(b.area());
    if smaller_area <= 0.0 {
        return false;
    }
    overlap_area / smaller_area > threshold
}

/// Group images by overlapping bounding boxes using union-find
/// Note: This function is kept for potential future use in per-page-only grouping scenarios
#[allow(dead_code)]
fn group_by_overlap(images: &[ImageInfo]) -> Vec<Vec<usize>> {
    if images.is_empty() {
        return Vec::new();
    }

    let n = images.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }

    fn union(parent: &mut [usize], i: usize, j: usize) {
        let pi = find(parent, i);
        let pj = find(parent, j);
        if pi != pj {
            parent[pi] = pj;
        }
    }

    // Group images that overlap by more than 70%
    const OVERLAP_THRESHOLD: f64 = 0.7;
    for i in 0..n {
        for j in (i + 1)..n {
            if rectangles_overlap(&images[i].area, &images[j].area, OVERLAP_THRESHOLD) {
                union(&mut parent, i, j);
            }
        }
    }

    // Collect groups
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    // Convert to vec and sort groups by their average position on page (top to bottom, left to right)
    let mut result: Vec<Vec<usize>> = groups.into_values().collect();
    result.sort_by(|a, b| {
        let avg_y_a: f64 = a.iter().map(|&i| images[i].area.y1).sum::<f64>() / a.len() as f64;
        let avg_y_b: f64 = b.iter().map(|&i| images[i].area.y1).sum::<f64>() / b.len() as f64;
        avg_y_a
            .partial_cmp(&avg_y_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    result
}

/// Threshold for within-page overlap grouping
const WITHIN_PAGE_OVERLAP_THRESHOLD: f64 = 0.7;

/// Threshold for cross-page overlap grouping (lower since shadows often only partially overlap)
const CROSS_PAGE_OVERLAP_THRESHOLD: f64 = 0.3;

/// Directions an image can extend beyond page bounds
#[derive(Debug, Default)]
struct OverflowDirections {
    right: bool, // x2 > page_width (check next page)
    left: bool,  // x1 < 0 (check previous page)
    down: bool,  // y2 > page_height (check next page for top-flip)
    up: bool,    // y1 < 0 (check previous page for top-flip)
}

/// Check if an image extends beyond page bounds
fn get_overflow_directions(info: &ImageInfo) -> OverflowDirections {
    OverflowDirections {
        right: info.area.x2 > info.page_width,
        left: info.area.x1 < 0.0,
        down: info.area.y2 > info.page_height,
        up: info.area.y1 < 0.0,
    }
}

/// Transform image bounds to adjacent page's coordinate space
/// Returns the transformed rectangle and whether any part is on the target page
fn transform_to_adjacent_page(
    area: &Rectangle,
    extends_right: bool,
    extends_left: bool,
    extends_down: bool,
    extends_up: bool,
    page_width: f64,
    page_height: f64,
) -> Rectangle {
    let mut transformed = *area;

    if extends_right {
        // Image extends right onto next page: subtract page_width from x coordinates
        transformed.x1 -= page_width;
        transformed.x2 -= page_width;
    } else if extends_left {
        // Image extends left onto previous page: add page_width to x coordinates
        transformed.x1 += page_width;
        transformed.x2 += page_width;
    }

    if extends_down {
        // Image extends down onto next page: subtract page_height from y coordinates
        transformed.y1 -= page_height;
        transformed.y2 -= page_height;
    } else if extends_up {
        // Image extends up onto previous page: add page_height to y coordinates
        transformed.y1 += page_height;
        transformed.y2 += page_height;
    }

    transformed
}

/// A group of images that should be composited together, potentially spanning multiple pages
#[derive(Debug)]
struct ImageGroup {
    /// Indices into the all_images vec
    image_indices: Vec<usize>,
    /// The page this composite should be assigned to (by centroid)
    assigned_page: usize,
}

/// Build groups considering cross-page overlaps
/// Returns groups of image indices and their assigned page numbers
fn build_cross_page_groups(all_images: &[ImageInfo]) -> Vec<ImageGroup> {
    if all_images.is_empty() {
        return Vec::new();
    }

    let n = all_images.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }

    fn union(parent: &mut [usize], i: usize, j: usize) {
        let pi = find(parent, i);
        let pj = find(parent, j);
        if pi != pj {
            parent[pi] = pj;
        }
    }

    // First pass: group within-page overlaps (70% threshold)
    for i in 0..n {
        for j in (i + 1)..n {
            // Only check same-page overlaps in this pass
            if all_images[i].page_number == all_images[j].page_number
                && rectangles_overlap(
                    &all_images[i].area,
                    &all_images[j].area,
                    WITHIN_PAGE_OVERLAP_THRESHOLD,
                )
            {
                union(&mut parent, i, j);
            }
        }
    }

    // Second pass: check cross-page overlaps (30% threshold)
    for i in 0..n {
        let overflow = get_overflow_directions(&all_images[i]);

        // Check if this image extends beyond page bounds
        if !overflow.right && !overflow.left && !overflow.down && !overflow.up {
            continue;
        }

        let img_i = &all_images[i];

        for (j, img_j) in all_images.iter().enumerate() {
            if i == j {
                continue;
            }

            // Check if j is on an adjacent page in the right direction
            let is_next_page = img_j.page_number == img_i.page_number + 1;
            let is_prev_page = img_i.page_number > 0 && img_j.page_number == img_i.page_number - 1;

            let check_horizontal =
                (overflow.right && is_next_page) || (overflow.left && is_prev_page);
            let check_vertical = (overflow.down && is_next_page) || (overflow.up && is_prev_page);

            if !check_horizontal && !check_vertical {
                continue;
            }

            // Transform coordinates to adjacent page's space
            let transformed = transform_to_adjacent_page(
                &img_i.area,
                overflow.right && is_next_page,
                overflow.left && is_prev_page,
                overflow.down && is_next_page,
                overflow.up && is_prev_page,
                img_i.page_width,
                img_i.page_height,
            );

            // Check overlap with lower threshold
            if rectangles_overlap(&transformed, &img_j.area, CROSS_PAGE_OVERLAP_THRESHOLD) {
                union(&mut parent, i, j);
            }
        }
    }

    // Collect groups
    let mut groups_map: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups_map.entry(root).or_default().push(i);
    }

    // Convert to ImageGroup with page assignment by centroid
    let mut result: Vec<ImageGroup> = groups_map
        .into_values()
        .map(|indices| {
            let assigned_page = calculate_centroid_page(all_images, &indices);
            ImageGroup {
                image_indices: indices,
                assigned_page,
            }
        })
        .collect();

    // Sort by assigned page then by position
    result.sort_by(|a, b| {
        a.assigned_page.cmp(&b.assigned_page).then_with(|| {
            let avg_y_a: f64 = a
                .image_indices
                .iter()
                .map(|&i| all_images[i].area.y1)
                .sum::<f64>()
                / a.image_indices.len() as f64;
            let avg_y_b: f64 = b
                .image_indices
                .iter()
                .map(|&i| all_images[i].area.y1)
                .sum::<f64>()
                / b.image_indices.len() as f64;
            avg_y_a
                .partial_cmp(&avg_y_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    result
}

/// Calculate which page a group should be assigned to based on centroid location
fn calculate_centroid_page(all_images: &[ImageInfo], indices: &[usize]) -> usize {
    if indices.is_empty() {
        return 0;
    }

    // For single-page groups, just use that page
    let first_page = all_images[indices[0]].page_number;
    if indices
        .iter()
        .all(|&i| all_images[i].page_number == first_page)
    {
        return first_page;
    }

    // For cross-page groups, calculate centroid in unified coordinate space
    // We'll use the first page as reference and transform other pages' coordinates
    let ref_page = indices
        .iter()
        .map(|&i| all_images[i].page_number)
        .min()
        .unwrap_or(0);
    let ref_page_width = all_images
        .iter()
        .find(|img| img.page_number == ref_page)
        .map(|img| img.page_width)
        .unwrap_or(600.0);

    let mut total_x = 0.0;
    let mut count = 0.0;

    for &idx in indices {
        let img = &all_images[idx];
        let page_offset = (img.page_number - ref_page) as f64 * ref_page_width;

        let center_x = (img.area.x1 + img.area.x2) / 2.0 + page_offset;

        total_x += center_x;
        count += 1.0;
    }

    if count == 0.0 {
        return first_page;
    }

    let centroid_x = total_x / count;

    // Determine which page the centroid falls on
    // centroid_x / page_width gives the page offset from ref_page
    let page_offset = (centroid_x / ref_page_width).floor() as usize;
    ref_page + page_offset
}

/// Check if image data represents a grayscale image (R=G=B for all pixels)
/// This detects SMask images that Poppler has converted to RGB format
/// Returns true if the image appears to be grayscale data
fn is_grayscale_rgb_data(surface_data: &[u8], width: i32, height: i32, stride: i32) -> bool {
    // Sample pixels to check if R=G=B
    // We don't need to check every pixel - sampling is sufficient
    let sample_step = ((width * height) as usize / 100).max(1); // Check ~100 pixels
    // Both ARGB32 and RGB24 use 4 bytes per pixel in Cairo
    let bytes_per_pixel = 4;

    let mut samples_checked = 0;
    let mut grayscale_count = 0;

    for y in (0..height).step_by(sample_step.max(1)) {
        for x in (0..width).step_by(sample_step.max(1)) {
            let offset = (y * stride + x * bytes_per_pixel) as usize;
            if offset + 3 >= surface_data.len() {
                continue;
            }

            // Cairo format: BGRA on little-endian
            let b = surface_data[offset];
            let g = surface_data[offset + 1];
            let r = surface_data[offset + 2];

            samples_checked += 1;

            // Check if R=G=B (within small tolerance for compression artifacts)
            let max_diff = r.abs_diff(g).max(g.abs_diff(b)).max(r.abs_diff(b));
            if max_diff <= 2 {
                grayscale_count += 1;
            }
        }
    }

    // If >95% of sampled pixels are grayscale, consider the whole image grayscale
    samples_checked > 0 && (grayscale_count as f64 / samples_checked as f64) > 0.95
}

/// Convert an ImageInfo to an RGBA image
fn convert_to_rgba(info: &ImageInfo) -> RgbaImage {
    let width = info.width as u32;
    let height = info.height as u32;
    let mut img = RgbaImage::new(width, height);

    if info.has_alpha {
        // ARGB32 (Cairo premultiplied format) -> RGBA
        // Cairo ARGB32 is stored as 32-bit native-endian with alpha in highest byte
        // On little-endian systems: BGRA byte order
        for y in 0..height {
            for x in 0..width {
                let offset = (y as i32 * info.stride + x as i32 * 4) as usize;
                if offset + 3 < info.surface_data.len() {
                    let b = info.surface_data[offset];
                    let g = info.surface_data[offset + 1];
                    let r = info.surface_data[offset + 2];
                    let a = info.surface_data[offset + 3];

                    // Un-premultiply alpha
                    let (r, g, b) = if a > 0 && a < 255 {
                        let alpha_f = a as f32 / 255.0;
                        (
                            (r as f32 / alpha_f).min(255.0) as u8,
                            (g as f32 / alpha_f).min(255.0) as u8,
                            (b as f32 / alpha_f).min(255.0) as u8,
                        )
                    } else {
                        (r, g, b)
                    };

                    img.put_pixel(x, y, image::Rgba([r, g, b, a]));
                }
            }
        }
    } else {
        // RGB24 format -> RGBA
        // Cairo RGB24 is stored as 32-bit with high byte unused: xRGB on big-endian, BGRx on little-endian
        for y in 0..height {
            for x in 0..width {
                let offset = (y as i32 * info.stride + x as i32 * 4) as usize;
                if offset + 3 < info.surface_data.len() {
                    let b = info.surface_data[offset];
                    let g = info.surface_data[offset + 1];
                    let r = info.surface_data[offset + 2];
                    // Ignore byte at offset + 3 (unused)
                    img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
                }
            }
        }
    }

    img
}

/// Apply a soft mask (SMask) to an RGBA image
/// The SMask is grayscale data where 0 = transparent, 255 = opaque
/// This replaces the image's alpha channel with the SMask values
fn apply_smask(img: &mut RgbaImage, smask_data: &[u8], smask_width: u32, smask_height: u32) {
    let img_width = img.width();
    let img_height = img.height();

    // If SMask dimensions match the image, apply directly
    if smask_width == img_width && smask_height == img_height {
        let expected_size = (smask_width * smask_height) as usize;
        if smask_data.len() >= expected_size {
            for y in 0..img_height {
                for x in 0..img_width {
                    let smask_idx = (y * smask_width + x) as usize;
                    let alpha = smask_data[smask_idx];
                    let pixel = img.get_pixel_mut(x, y);
                    pixel[3] = alpha;
                }
            }
        }
    } else {
        // SMask dimensions differ - scale the mask to match image dimensions
        // Create a grayscale image from SMask data and resize it
        let expected_size = (smask_width * smask_height) as usize;
        if smask_data.len() >= expected_size {
            // Create grayscale image from SMask data
            let smask_img: image::GrayImage =
                image::GrayImage::from_raw(smask_width, smask_height, smask_data.to_vec())
                    .unwrap_or_else(|| image::GrayImage::new(1, 1));

            // Resize SMask to match image dimensions
            let scaled_smask = image::imageops::resize(
                &smask_img,
                img_width,
                img_height,
                image::imageops::FilterType::Lanczos3,
            );

            // Apply scaled SMask as alpha channel
            for y in 0..img_height {
                for x in 0..img_width {
                    let alpha = scaled_smask.get_pixel(x, y)[0];
                    let pixel = img.get_pixel_mut(x, y);
                    pixel[3] = alpha;
                }
            }
        }
    }
}

/// Alpha blend two pixels (Porter-Duff "over" operation)
fn alpha_blend(dst: image::Rgba<u8>, src: image::Rgba<u8>) -> image::Rgba<u8> {
    let src_a = src[3] as f32 / 255.0;
    let dst_a = dst[3] as f32 / 255.0;

    // out_a = src_a + dst_a * (1 - src_a)
    let out_a = src_a + dst_a * (1.0 - src_a);

    if out_a <= 0.0 {
        return image::Rgba([0, 0, 0, 0]);
    }

    // out_rgb = (src_rgb * src_a + dst_rgb * dst_a * (1 - src_a)) / out_a
    let blend = |s: u8, d: u8| -> u8 {
        let s_f = s as f32 / 255.0;
        let d_f = d as f32 / 255.0;
        let out = (s_f * src_a + d_f * dst_a * (1.0 - src_a)) / out_a;
        (out * 255.0).clamp(0.0, 255.0) as u8
    };

    image::Rgba([
        blend(src[0], dst[0]),
        blend(src[1], dst[1]),
        blend(src[2], dst[2]),
        (out_a * 255.0) as u8,
    ])
}

/// Scale an image by a given factor using Lanczos3 filter
fn scale_image(img: &RgbaImage, scale: f64) -> RgbaImage {
    if (scale - 1.0).abs() < 0.01 {
        // No significant scaling needed
        return img.clone();
    }

    let new_width = ((img.width() as f64 * scale).ceil() as u32).max(1);
    let new_height = ((img.height() as f64 * scale).ceil() as u32).max(1);

    image::imageops::resize(
        img,
        new_width,
        new_height,
        image::imageops::FilterType::Lanczos3,
    )
}

/// Composite a layer onto a canvas at the given offset
fn composite_over(canvas: &mut RgbaImage, layer: &RgbaImage, offset_x: i32, offset_y: i32) {
    for (ly, row) in layer.rows().enumerate() {
        for (lx, &pixel) in row.enumerate() {
            let cx = lx as i32 + offset_x;
            let cy = ly as i32 + offset_y;
            if cx >= 0 && cy >= 0 && cx < canvas.width() as i32 && cy < canvas.height() as i32 {
                let dst = canvas.get_pixel(cx as u32, cy as u32);
                let blended = alpha_blend(*dst, pixel);
                canvas.put_pixel(cx as u32, cy as u32, blended);
            }
        }
    }
}

/// Calculate the bounding box encompassing all images in a group
fn calculate_group_bounds(images: &[ImageInfo], indices: &[usize]) -> Rectangle {
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for &idx in indices {
        let area = &images[idx].area;
        min_x = min_x.min(area.x1).min(area.x2);
        min_y = min_y.min(area.y1).min(area.y2);
        max_x = max_x.max(area.x1).max(area.x2);
        max_y = max_y.max(area.y1).max(area.y2);
    }

    Rectangle {
        x1: min_x,
        y1: min_y,
        x2: max_x,
        y2: max_y,
    }
}

/// Calculate the bounding box for a cross-page group, transforming coordinates
/// to a unified coordinate space where pages are laid out horizontally.
/// Returns (bounds, ref_page, ref_page_width) for use in offset calculations.
fn calculate_cross_page_bounds(images: &[ImageInfo], indices: &[usize]) -> (Rectangle, usize, f64) {
    // Find the reference page (minimum page number)
    let ref_page = indices
        .iter()
        .map(|&i| images[i].page_number)
        .min()
        .unwrap_or(0);

    let ref_page_width = images
        .iter()
        .find(|img| img.page_number == ref_page)
        .map(|img| img.page_width)
        .unwrap_or(600.0);

    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for &idx in indices {
        let img = &images[idx];
        // Transform x coordinates to unified space by adding page offset
        let page_offset = (img.page_number - ref_page) as f64 * ref_page_width;

        let x1_transformed = img.area.x1 + page_offset;
        let x2_transformed = img.area.x2 + page_offset;

        min_x = min_x.min(x1_transformed).min(x2_transformed);
        min_y = min_y.min(img.area.y1).min(img.area.y2);
        max_x = max_x.max(x1_transformed).max(x2_transformed);
        max_y = max_y.max(img.area.y1).max(img.area.y2);
    }

    (
        Rectangle {
            x1: min_x,
            y1: min_y,
            x2: max_x,
            y2: max_y,
        },
        ref_page,
        ref_page_width,
    )
}

/// Composite a group of overlapping images into a single image.
///
/// Each image has a PDF bounding box (in points) and native pixel dimensions.
/// The composite canvas is sized to encompass all bounding boxes at the highest
/// available resolution (max pixels-per-point). Each image is then scaled to
/// fill its own bounding box at the canvas resolution and placed accordingly.
///
/// For cross-page groups (images spanning multiple pages), coordinates are
/// transformed to a unified space where pages are laid out horizontally.
///
/// Layers are composited back-to-front based on image_id (lower IDs = back layers).
///
/// Transformation matrices (CTMs) are applied to correct image orientation.
fn composite_group(images: &[ImageInfo], indices: &[usize]) -> Option<RgbaImage> {
    if indices.is_empty() {
        return None;
    }

    if indices.len() == 1 {
        // Single image - convert to RGBA, apply SMask if present, crop, then transform
        let info = &images[indices[0]];
        let mut img = convert_to_rgba(info);

        // Apply soft mask (SMask) if present - this sets the alpha channel
        if let (Some(smask_data), Some(smask_w), Some(smask_h)) =
            (&info.smask_data, info.smask_width, info.smask_height)
        {
            trace!(
                page = info.page_number + 1,
                image_id = info.image_id,
                smask_size = format!("{}x{}", smask_w, smask_h),
                image_size = format!("{}x{}", img.width(), img.height()),
                "Applying SMask to single image"
            );
            apply_smask(&mut img, smask_data, smask_w, smask_h);
        }

        // Apply pixel crop if present (from PDF clip_rect)
        if let Some((cx, cy, cw, ch)) = info.crop_pixels {
            trace!(
                page = info.page_number + 1,
                image_id = info.image_id,
                crop = format!("({}, {}) {}x{}", cx, cy, cw, ch),
                original_size = format!("{}x{}", img.width(), img.height()),
                "Applying crop to single image"
            );
            img = image::imageops::crop_imm(&img, cx, cy, cw, ch).to_image();
        }

        // Apply transformation if present and needed
        if let Some(ref matrix) = info.transform
            && needs_transformation(matrix)
        {
            trace!(
                page = info.page_number + 1,
                image_id = info.image_id,
                matrix = ?matrix,
                "Applying transformation to single image"
            );
            img = apply_transform(&img, matrix);
        }

        return Some(img);
    }

    // Check if this is a cross-page group
    let first_page = images[indices[0]].page_number;
    let is_cross_page = indices.iter().any(|&i| images[i].page_number != first_page);

    // Find the maximum scale factor (highest resolution image)
    let max_scale = indices
        .iter()
        .map(|&i| images[i].scale_x.max(images[i].scale_y))
        .fold(0.0_f64, f64::max);

    if max_scale <= 0.0 {
        return None;
    }

    // Calculate bounds in PDF points (with transformation for cross-page groups)
    let (bounds, ref_page, ref_page_width) = if is_cross_page {
        calculate_cross_page_bounds(images, indices)
    } else {
        (calculate_group_bounds(images, indices), first_page, 0.0)
    };

    // Calculate canvas size in pixels using the max scale factor
    let canvas_width = (bounds.width() * max_scale).ceil() as u32;
    let canvas_height = (bounds.height() * max_scale).ceil() as u32;

    if canvas_width == 0 || canvas_height == 0 {
        return None;
    }

    // Create transparent canvas at full resolution
    let mut canvas = RgbaImage::new(canvas_width, canvas_height);

    // Sort indices by image_id ascending (lower IDs drawn first = back layer)
    // This matches the discovery that images are listed in reverse z-order
    let mut sorted_indices = indices.to_vec();
    sorted_indices.sort_by_key(|&idx| images[idx].image_id);

    // Composite each layer (back to front)
    for &idx in &sorted_indices {
        let info = &images[idx];
        let mut layer = convert_to_rgba(info);

        // Apply soft mask (SMask) if present - this sets the alpha channel
        if let (Some(smask_data), Some(smask_w), Some(smask_h)) =
            (&info.smask_data, info.smask_width, info.smask_height)
        {
            trace!(
                page = info.page_number + 1,
                image_id = info.image_id,
                smask_size = format!("{}x{}", smask_w, smask_h),
                layer_size = format!("{}x{}", layer.width(), layer.height()),
                "Applying SMask to layer in composite"
            );
            apply_smask(&mut layer, smask_data, smask_w, smask_h);
        }

        // Apply pixel crop if present (from PDF clip_rect)
        if let Some((cx, cy, cw, ch)) = info.crop_pixels {
            trace!(
                page = info.page_number + 1,
                image_id = info.image_id,
                crop = format!("({}, {}) {}x{}", cx, cy, cw, ch),
                original_size = format!("{}x{}", layer.width(), layer.height()),
                "Applying crop to layer in composite"
            );
            layer = image::imageops::crop_imm(&layer, cx, cy, cw, ch).to_image();
        }

        // Apply transformation if present and needed
        if let Some(ref matrix) = info.transform
            && needs_transformation(matrix)
        {
            trace!(
                page = info.page_number + 1,
                image_id = info.image_id,
                matrix = ?matrix,
                "Applying transformation to layer in composite"
            );
            layer = apply_transform(&layer, matrix);
        }

        // Each image has a native resolution (pixels) and a PDF bounding box (points).
        // To composite correctly, we need to scale each image so it fills its bounding
        // box at the canvas resolution (max_scale pixels per point).
        //
        // scale_factor = (bounding_box_pts * max_scale) / native_pixels
        //              = max_scale / (native_pixels / bounding_box_pts)
        //              = max_scale / layer_scale
        let layer_scale = info.scale_x.max(info.scale_y);
        let scale_factor = max_scale / layer_scale;

        if (scale_factor - 1.0).abs() > 0.01 {
            // Scale image to fill its bounding box at canvas resolution
            layer = scale_image(&layer, scale_factor);
        }

        // For cross-page groups, transform x coordinate to unified space
        let page_offset = if is_cross_page {
            (info.page_number - ref_page) as f64 * ref_page_width
        } else {
            0.0
        };

        // Calculate offset in pixels (convert PDF points to pixels using max_scale)
        let offset_x =
            ((info.area.x1.min(info.area.x2) + page_offset - bounds.x1) * max_scale) as i32;
        let offset_y = ((info.area.y1.min(info.area.y2) - bounds.y1) * max_scale) as i32;

        composite_over(&mut canvas, &layer, offset_x, offset_y);
    }

    Some(canvas)
}

/// Transformation matrix extracted from PDF content stream
/// Represents the CTM (Current Transformation Matrix) applied to an image
#[derive(Debug, Clone)]
struct ImageTransform {
    /// XObject name (e.g., "Im0", "I129") - kept for debugging
    #[allow(dead_code)]
    xobject_name: String,
    /// 6-element transformation matrix [a, b, c, d, e, f]
    /// [a b 0]
    /// [c d 0]
    /// [e f 1]
    matrix: [f64; 6],
    /// Expected width of the transformed image (calculated from CTM)
    /// width = sqrt(a² + b²)
    expected_width: f64,
    /// Expected height of the transformed image (calculated from CTM)
    /// height = sqrt(c² + d²)
    expected_height: f64,
    /// Axis-aligned bounding box computed from CTM (for rotated images)
    /// This gives the TRUE position on the page after transformation
    computed_bounds: Option<(f64, f64, f64, f64)>, // (x1, y1, x2, y2)
    /// Clipping rectangle active when the image was drawn (if any)
    /// This should be used to constrain the visible area of the image
    clip_rect: Option<(f64, f64, f64, f64)>, // (x1, y1, x2, y2)
    /// Soft mask (SMask) data for transparency, if the image has one
    /// This is raw grayscale pixel data where 0 = transparent, 255 = opaque
    smask_data: Option<Vec<u8>>,
    /// Width of the SMask image in pixels
    smask_width: Option<u32>,
    /// Height of the SMask image in pixels
    smask_height: Option<u32>,
}

/// Compute the axis-aligned bounding box from a CTM
/// The CTM transforms a unit square [0,0] to [1,1] to the final position
fn compute_bounds_from_ctm(matrix: &[f64; 6]) -> (f64, f64, f64, f64) {
    let [a, b, c, d, e, f] = *matrix;

    // Transform the four corners of the unit square
    // Corner [0,0] -> (e, f)
    // Corner [1,0] -> (a+e, b+f)
    // Corner [0,1] -> (c+e, d+f)
    // Corner [1,1] -> (a+c+e, b+d+f)
    let corners = [
        (e, f),
        (a + e, b + f),
        (c + e, d + f),
        (a + c + e, b + d + f),
    ];

    let min_x = corners.iter().map(|c| c.0).fold(f64::MAX, f64::min);
    let max_x = corners.iter().map(|c| c.0).fold(f64::MIN, f64::max);
    let min_y = corners.iter().map(|c| c.1).fold(f64::MAX, f64::min);
    let max_y = corners.iter().map(|c| c.1).fold(f64::MIN, f64::max);

    (min_x, min_y, max_x, max_y)
}

/// Extract transformation matrices for images from PDF using qpdf
/// Returns a map of page_num -> Vec<ImageTransform> for matching by dimensions
fn extract_image_transforms_with_qpdf(
    path: &Path,
) -> Result<HashMap<usize, Vec<ImageTransform>>, ProcessingError> {
    use qpdf::{QPdfObjectLike, QPdfObjectType, QPdfStream};

    let pdf = QPdf::read(path).map_err(|e| ProcessingError::TextExtraction {
        page: 0,
        source: Box::new(std::io::Error::other(format!(
            "Failed to load PDF with qpdf: {}",
            e
        ))),
    })?;

    let mut transforms: HashMap<usize, Vec<ImageTransform>> = HashMap::new();
    let pages = pdf
        .get_pages()
        .map_err(|e| ProcessingError::TextExtraction {
            page: 0,
            source: Box::new(std::io::Error::other(format!(
                "Failed to get pages from PDF: {}",
                e
            ))),
        })?;

    let mut total_form_xobjects = 0;
    let mut total_with_ctm = 0;

    for (page_idx, page_dict) in pages.iter().enumerate() {
        // Get the page's Resources dictionary
        let resources = match page_dict.get("/Resources") {
            Some(r) => r,
            None => {
                trace!(page = page_idx + 1, "No /Resources dictionary on page");
                continue;
            }
        };

        // Convert to dictionary to access keys
        let resources_dict: qpdf::QPdfDictionary = resources.into();

        // Get XObject dictionary from Resources
        let xobjects = match resources_dict.get("/XObject") {
            Some(x) => x,
            None => {
                trace!(page = page_idx + 1, "No /XObject dictionary in Resources");
                continue;
            }
        };

        let xobjects_dict: qpdf::QPdfDictionary = xobjects.into();

        // Get all XObject names
        let xobject_keys = xobjects_dict.keys();
        trace!(
            page = page_idx + 1,
            xobjects = xobject_keys.len(),
            "Found XObjects on page"
        );

        // For each Form XObject, extract CTM from its content stream
        for key in xobject_keys {
            let xobject = match xobjects_dict.get(&key) {
                Some(obj) => obj,
                None => continue,
            };

            // Check if it's a stream (Form XObjects are streams)
            if xobject.get_type() != QPdfObjectType::Stream {
                continue;
            }

            // Convert to dictionary to check subtype
            let xobject_stream: QPdfStream = xobject.clone().into();
            let xobject_dict = xobject_stream.get_dictionary();

            // Check if it's a Form XObject (not an Image XObject)
            let subtype = match xobject_dict.get("/Subtype") {
                Some(s) => s.as_name(),
                None => continue,
            };

            if subtype != "/Form" {
                continue;
            }

            total_form_xobjects += 1;

            // Get the content stream data
            let data = match xobject_stream.get_data(StreamDecodeLevel::Generalized) {
                Ok(d) => d,
                Err(e) => {
                    trace!(page = page_idx + 1, xobject = %key, error = %e, "Failed to decode Form XObject stream");
                    continue;
                }
            };

            let content = String::from_utf8_lossy(&data);

            // Parse content stream for all CTM + image draw commands
            // Pattern: [a b c d e f] cm ... /ImN Do (may occur multiple times)
            let found_transforms = parse_content_stream_for_all_ctms(&content);

            // Get Form's nested XObject dictionary to look up SMasks
            let form_xobjects_dict: Option<qpdf::QPdfDictionary> = xobject_dict
                .get("/Resources")
                .and_then(|r| {
                    let r_dict: qpdf::QPdfDictionary = r.into();
                    r_dict.get("/XObject")
                })
                .map(|x| x.into());

            for mut transform in found_transforms {
                total_with_ctm += 1;

                // Try to extract SMask data for this image
                if let Some(ref nested_xobjects) = form_xobjects_dict {
                    let image_name = format!("/{}", transform.xobject_name);
                    if let Some(image_obj) = nested_xobjects.get(&image_name)
                        && image_obj.get_type() == QPdfObjectType::Stream
                    {
                        let image_stream: QPdfStream = image_obj.into();
                        let image_dict = image_stream.get_dictionary();

                        if let Some(smask_ref) = image_dict.get("/SMask") {
                            let smask_id = smask_ref.get_id();
                            let smask_gen = smask_ref.get_generation();

                            if let Some(smask_obj) = pdf.get_object_by_id(smask_id, smask_gen)
                                && smask_obj.get_type() == QPdfObjectType::Stream
                            {
                                let smask_stream: QPdfStream = smask_obj.into();
                                let smask_dict = smask_stream.get_dictionary();

                                // Extract SMask dimensions
                                let width: Option<u32> = smask_dict
                                    .get("/Width")
                                    .and_then(|w| format!("{}", w).parse().ok());
                                let height: Option<u32> = smask_dict
                                    .get("/Height")
                                    .and_then(|h| format!("{}", h).parse().ok());

                                // Extract SMask data
                                if let Ok(smask_data) =
                                    smask_stream.get_data(StreamDecodeLevel::All)
                                {
                                    transform.smask_data = Some(smask_data.to_vec());
                                    transform.smask_width = width;
                                    transform.smask_height = height;

                                    trace!(
                                        page = page_idx + 1,
                                        image = %transform.xobject_name,
                                        smask_width = ?width,
                                        smask_height = ?height,
                                        smask_bytes = transform.smask_data.as_ref().map(|d| d.len()),
                                        "Extracted SMask data for image"
                                    );
                                }
                            }
                        }
                    }
                }

                // Store transforms that indicate rotation/mirroring, have a clip_rect, or have SMask
                let has_rotation = needs_transformation(&transform.matrix);
                let has_clip = transform.clip_rect.is_some();
                let has_smask = transform.smask_data.is_some();

                if has_rotation || has_clip || has_smask {
                    trace!(
                        page = page_idx + 1,
                        form_xobject = %key,
                        image_xobject = %transform.xobject_name,
                        matrix = ?transform.matrix,
                        expected_width = format!("{:.1}", transform.expected_width),
                        expected_height = format!("{:.1}", transform.expected_height),
                        has_rotation = has_rotation,
                        clip_rect = ?transform.clip_rect,
                        has_smask = has_smask,
                        "Found CTM with rotation/mirroring, clip_rect, or SMask in Form XObject"
                    );
                    transforms.entry(page_idx).or_default().push(transform);
                }
            }
        }
    }

    debug!(
        total_form_xobjects = total_form_xobjects,
        form_xobjects_with_ctm = total_with_ctm,
        transforms_with_rotation = transforms.values().map(|v| v.len()).sum::<usize>(),
        pages_with_transforms = transforms.len(),
        "Extracted image transforms with qpdf"
    );

    Ok(transforms)
}

/// Parse a PDF content stream to extract all CTMs and clip rects applied to images
/// Returns a Vec of ImageTransforms, one for each image draw command found
/// Captures the state at the moment of each Do command
fn parse_content_stream_for_all_ctms(content: &str) -> Vec<ImageTransform> {
    // Track graphics state stack for cumulative CTM and clip rect
    let mut ctm_stack: Vec<[f64; 6]> = vec![[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]]; // Identity matrix
    let mut clip_stack: Vec<Option<(f64, f64, f64, f64)>> = vec![None];
    let mut current_ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut current_clip: Option<(f64, f64, f64, f64)> = None;
    // Pending rectangle from 're' command, waiting for 'W' to make it a clip
    let mut pending_rect: Option<(f64, f64, f64, f64)> = None;

    let mut transforms = Vec::new();

    // Tokenize the content stream
    let tokens: Vec<&str> = content.split_whitespace().collect();
    let mut i = 0;

    while i < tokens.len() {
        let token = tokens[i];

        match token {
            "q" => {
                // Save graphics state (including clip)
                ctm_stack.push(current_ctm);
                clip_stack.push(current_clip);
            }
            "Q" => {
                // Restore graphics state
                if let Some(saved_ctm) = ctm_stack.pop() {
                    current_ctm = saved_ctm;
                }
                if let Some(saved_clip) = clip_stack.pop() {
                    current_clip = saved_clip;
                }
            }
            "cm" => {
                // Concatenate matrix: need 6 numbers before "cm"
                if i >= 6
                    && let (Ok(a), Ok(b), Ok(c), Ok(d), Ok(e), Ok(f)) = (
                        tokens[i - 6].parse::<f64>(),
                        tokens[i - 5].parse::<f64>(),
                        tokens[i - 4].parse::<f64>(),
                        tokens[i - 3].parse::<f64>(),
                        tokens[i - 2].parse::<f64>(),
                        tokens[i - 1].parse::<f64>(),
                    )
                {
                    let new_matrix = [a, b, c, d, e, f];
                    current_ctm = multiply_matrices(&current_ctm, &new_matrix);
                }
            }
            "re" => {
                // Rectangle path: x y width height re
                // Store as pending until we see if it becomes a clip
                if i >= 4
                    && let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                        tokens[i - 4].parse::<f64>(),
                        tokens[i - 3].parse::<f64>(),
                        tokens[i - 2].parse::<f64>(),
                        tokens[i - 1].parse::<f64>(),
                    )
                {
                    // Convert to (x1, y1, x2, y2) format
                    pending_rect = Some((x, y, x + w, y + h));
                }
            }
            "W" | "W*" => {
                // Set clipping path - the pending rect becomes the clip
                // W = non-zero winding rule, W* = even-odd rule (both set clip)
                if let Some(rect) = pending_rect {
                    // Intersect with current clip if one exists
                    current_clip = Some(if let Some(existing) = current_clip {
                        // Intersect rectangles
                        (
                            rect.0.max(existing.0), // x1
                            rect.1.max(existing.1), // y1
                            rect.2.min(existing.2), // x2
                            rect.3.min(existing.3), // y2
                        )
                    } else {
                        rect
                    });
                }
                pending_rect = None;
            }
            "n" | "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                // Path-ending operators - clear pending rect if not used as clip
                pending_rect = None;
            }
            "Do" => {
                // Draw XObject - capture the state at this moment for each image
                // Only capture for Image XObjects (typically /ImN), not Form XObjects (/FmN)
                if i >= 1 {
                    let name = tokens[i - 1].trim_start_matches('/');
                    // Image XObjects are typically named ImN, Img, Image, etc.
                    // Also match X followed by digits (like X78)
                    if name.starts_with("Im")
                        || name.starts_with("Img")
                        || name.starts_with("Image")
                        || (name.starts_with('X')
                            && name.len() > 1
                            && name[1..]
                                .chars()
                                .next()
                                .map(|c| c.is_ascii_digit())
                                .unwrap_or(false))
                    {
                        let [a, b, c, d, _e, _f] = current_ctm;
                        let expected_width = (a * a + b * b).sqrt();
                        let expected_height = (c * c + d * d).sqrt();
                        let computed_bounds = Some(compute_bounds_from_ctm(&current_ctm));

                        transforms.push(ImageTransform {
                            xobject_name: name.to_string(),
                            matrix: current_ctm,
                            expected_width,
                            expected_height,
                            computed_bounds,
                            clip_rect: current_clip,
                            smask_data: None,
                            smask_width: None,
                            smask_height: None,
                        });
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    transforms
}

/// Multiply two 2D transformation matrices
/// [a1 b1 0]   [a2 b2 0]
/// [c1 d1 0] * [c2 d2 0]
/// [e1 f1 1]   [e2 f2 1]
fn multiply_matrices(m1: &[f64; 6], m2: &[f64; 6]) -> [f64; 6] {
    let [a1, b1, c1, d1, e1, f1] = *m1;
    let [a2, b2, c2, d2, e2, f2] = *m2;

    [
        a1 * a2 + b1 * c2,
        a1 * b2 + b1 * d2,
        c1 * a2 + d1 * c2,
        c1 * b2 + d1 * d2,
        e1 * a2 + f1 * c2 + e2,
        e1 * b2 + f1 * d2 + f2,
    ]
}

/// Match a poppler image with a qpdf-extracted CTM using dimension AND position matching
/// The CTM's expected dimensions are compared with poppler's bounding box dimensions,
/// AND the CTM's computed position must be reasonably close to poppler's reported position.
/// This prevents matching images that have similar dimensions but are on different parts of the page.
/// Returns the matching ImageTransform if found (includes matrix and computed_bounds)
fn find_matching_transform(
    page_num: usize,
    image_width: f64,
    image_height: f64,
    poppler_area: &Rectangle,
    transforms: &HashMap<usize, Vec<ImageTransform>>,
) -> Option<ImageTransform> {
    // Get transforms for this page
    let page_transforms = match transforms.get(&page_num) {
        Some(t) => t,
        None => {
            trace!(
                page = page_num + 1,
                image_width = format!("{:.1}", image_width),
                image_height = format!("{:.1}", image_height),
                "No transforms available for this page"
            );
            return None;
        }
    };

    // Find the transform whose expected dimensions best match the image dimensions
    // Allow 5% tolerance for dimension matching
    let dimension_tolerance = 0.05;
    // Allow position to differ by up to 50 points (for minor discrepancies)
    let position_tolerance = 50.0;

    // Calculate poppler's center point
    let poppler_cx = (poppler_area.x1 + poppler_area.x2) / 2.0;
    let poppler_cy = (poppler_area.y1 + poppler_area.y2) / 2.0;

    for transform in page_transforms {
        let width_ratio = (transform.expected_width - image_width).abs() / image_width.max(1.0);
        let height_ratio = (transform.expected_height - image_height).abs() / image_height.max(1.0);

        // Check dimensions first
        if width_ratio >= dimension_tolerance || height_ratio >= dimension_tolerance {
            continue;
        }

        // Dimensions match - now check position
        // For rotated images, poppler's bbox can be very wrong, but often gets at least
        // one corner coordinate correct. Check if ANY corner is close.
        if let Some((ctm_x1, ctm_y1, ctm_x2, ctm_y2)) = transform.computed_bounds {
            // Check multiple position criteria - any one matching is sufficient
            let x1_close = (ctm_x1 - poppler_area.x1).abs() < position_tolerance;
            let x2_close = (ctm_x2 - poppler_area.x2).abs() < position_tolerance;
            let y1_close = (ctm_y1 - poppler_area.y1).abs() < position_tolerance;
            let y2_close = (ctm_y2 - poppler_area.y2).abs() < position_tolerance;

            // Also check center proximity as before
            let ctm_cx = (ctm_x1 + ctm_x2) / 2.0;
            let ctm_cy = (ctm_y1 + ctm_y2) / 2.0;
            let center_close = (ctm_cx - poppler_cx).abs() < position_tolerance
                && (ctm_cy - poppler_cy).abs() < position_tolerance;

            // Accept if centers are close OR if at least one x AND one y coordinate match
            let position_matches =
                center_close || ((x1_close || x2_close) && (y1_close || y2_close));

            // For rotated images, poppler often gets x1 right but y completely wrong
            // Be more lenient: accept if x1 matches closely even if y is off
            let x1_very_close = (ctm_x1 - poppler_area.x1).abs() < 5.0; // Within 5 points

            trace!(
                page = page_num + 1,
                image_dims = format!("{:.1} x {:.1}", image_width, image_height),
                ctm_dims = format!(
                    "{:.1} x {:.1}",
                    transform.expected_width, transform.expected_height
                ),
                poppler_bbox = format!(
                    "({:.1},{:.1})-({:.1},{:.1})",
                    poppler_area.x1, poppler_area.y1, poppler_area.x2, poppler_area.y2
                ),
                ctm_bbox = format!(
                    "({:.1},{:.1})-({:.1},{:.1})",
                    ctm_x1, ctm_y1, ctm_x2, ctm_y2
                ),
                x1_close = x1_close,
                x1_very_close = x1_very_close,
                center_close = center_close,
                "Comparing image dimensions and position with CTM"
            );

            // If positions don't match by any criterion, skip this CTM
            if !position_matches && !x1_very_close {
                trace!(
                    page = page_num + 1,
                    image_dims = format!("{:.1} x {:.1}", image_width, image_height),
                    poppler_bbox = format!(
                        "({:.1},{:.1})-({:.1},{:.1})",
                        poppler_area.x1, poppler_area.y1, poppler_area.x2, poppler_area.y2
                    ),
                    ctm_bbox = format!(
                        "({:.1},{:.1})-({:.1},{:.1})",
                        ctm_x1, ctm_y1, ctm_x2, ctm_y2
                    ),
                    "Dimensions match but no position criterion met - skipping CTM"
                );
                continue;
            }

            trace!(
                page = page_num + 1,
                image_dims = format!("{:.1} x {:.1}", image_width, image_height),
                ctm_dims = format!("{:.1} x {:.1}", transform.expected_width, transform.expected_height),
                poppler_pos = format!("({:.1},{:.1})-({:.1},{:.1})", poppler_area.x1, poppler_area.y1, poppler_area.x2, poppler_area.y2),
                ctm_pos = format!("({:.1},{:.1})-({:.1},{:.1})", ctm_x1, ctm_y1, ctm_x2, ctm_y2),
                matrix = ?transform.matrix,
                "Matched image to CTM by dimensions and position"
            );
            return Some(transform.clone());
        } else {
            // No computed bounds - fall back to dimension-only matching
            trace!(
                page = page_num + 1,
                image_dims = format!("{:.1} x {:.1}", image_width, image_height),
                ctm_dims = format!("{:.1} x {:.1}", transform.expected_width, transform.expected_height),
                matrix = ?transform.matrix,
                "Matched image to CTM by dimensions (no position check)"
            );
            return Some(transform.clone());
        }
    }

    trace!(
        page = page_num + 1,
        image_dims = format!("{:.1} x {:.1}", image_width, image_height),
        available_ctms = page_transforms.len(),
        "No matching CTM found for image dimensions and position"
    );

    None
}

/// Check if a transformation matrix indicates the image needs to be transformed
/// (i.e., it's not an identity or simple scaling matrix)
fn needs_transformation(matrix: &[f64; 6]) -> bool {
    let [a, b, c, d, _e, _f] = *matrix;

    // Check if this is approximately an identity matrix (with possible scaling)
    // Identity: a=1, b=0, c=0, d=1 (or negative for flipping)
    // Simple scale: a>0, b=0, c=0, d>0

    // If b or c are non-zero, there's rotation
    let has_rotation = b.abs() > 0.01 || c.abs() > 0.01;

    // If a or d are negative, there's mirroring
    let has_mirroring = a < 0.0 || d < 0.0;

    has_rotation || has_mirroring
}

/// Apply transformation matrix to an image using affine transformation
/// The CTM matrix [a, b, c, d, e, f] is normalized to remove scaling
/// and applied to correct the image orientation.
fn apply_transform(image: &RgbaImage, matrix: &[f64; 6]) -> RgbaImage {
    use imageproc::geometric_transformations::{Interpolation, Projection, warp_into};

    let [a, b, c, d, _e, _f] = *matrix;

    // Calculate scale factors (length of transformed unit vectors)
    // scale_x = length of (a, b) = sqrt(a² + b²)
    // scale_y = length of (c, d) = sqrt(c² + d²)
    let scale_x = (a * a + b * b).sqrt();
    let scale_y = (c * c + d * d).sqrt();

    if scale_x < 0.001 || scale_y < 0.001 {
        warn!("Invalid scale factors in CTM, skipping transformation");
        return image.clone();
    }

    // Normalize the matrix to remove scaling (we want rotation/mirroring only)
    // The poppler image is already at the correct pixel dimensions
    let a_norm = a / scale_x;
    let b_norm = b / scale_x;
    let c_norm = c / scale_y;
    let d_norm = d / scale_y;

    // Calculate determinant to check for mirroring
    let det = a_norm * d_norm - b_norm * c_norm;

    // Calculate rotation angle
    let rotation_deg = f64::atan2(b_norm, a_norm).to_degrees();

    trace!(
        a_norm = a_norm,
        b_norm = b_norm,
        c_norm = c_norm,
        d_norm = d_norm,
        det = det,
        rotation_deg = rotation_deg,
        "Applying affine transformation"
    );

    let (width, height) = image.dimensions();

    // For the transformation, we need to:
    // 1. Center the image at origin
    // 2. Apply the inverse of the normalized CTM (to map output coords to input coords)
    // 3. Translate back

    // The inverse of [a, b; c, d] is (1/det) * [d, -b; -c, a]
    let inv_det = 1.0 / det;
    let inv_a = d_norm * inv_det;
    let inv_b = -b_norm * inv_det;
    let inv_c = -c_norm * inv_det;
    let inv_d = a_norm * inv_det;

    // Create the projection matrix for imageproc
    // The affine transformation maps (x, y) -> (ax + cy + e, bx + dy + f)
    // We need to handle the centering: transform around the image center
    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;

    // Combined transformation: translate to center, apply inverse rotation, translate back
    // For output point (x, y), find input point:
    // 1. Translate: (x - cx, y - cy)
    // 2. Apply inverse: (inv_a * (x-cx) + inv_c * (y-cy), inv_b * (x-cx) + inv_d * (y-cy))
    // 3. Translate back: add (cx, cy)
    //
    // Final: x_in = inv_a * x + inv_c * y + (cx - inv_a * cx - inv_c * cy)
    //        y_in = inv_b * x + inv_d * y + (cy - inv_b * cx - inv_d * cy)
    let tx = cx - inv_a * cx - inv_c * cy;
    let ty = cy - inv_b * cx - inv_d * cy;

    // Create projection using the inverse transformation
    // Projection expects [a, b, c; d, e, f; g, h, i] for projective transform
    // For affine: [a, b, tx; c, d, ty; 0, 0, 1]
    // But imageproc's Projection uses different ordering, let me check...
    // From imageproc docs: Projection::from_matrix maps (x, y) using the 3x3 matrix
    #[rustfmt::skip]
    let projection = Projection::from_matrix([
        inv_a as f32, inv_c as f32, tx as f32,
        inv_b as f32, inv_d as f32, ty as f32,
        0.0,          0.0,          1.0,
    ]).expect("Failed to create projection matrix");

    // Create output image
    let mut output = RgbaImage::new(width, height);
    let default_pixel = image::Rgba([0, 0, 0, 0]); // Transparent background

    warp_into(
        image,
        &projection,
        Interpolation::Bilinear,
        default_pixel,
        &mut output,
    );

    output
}

/// Create a new Pdfium instance (dynamically linked)
/// Searches for libpdfium in:
/// 1. Current directory (./libpdfium.so)
/// 2. vendor/pdfium/lib/ (downloaded by `just download-pdfium`)
/// 3. System library paths
fn create_pdfium() -> Result<Pdfium, ProcessingError> {
    // Try local paths first, then system
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| {
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                "./vendor/pdfium/lib/",
            ))
        })
        .or_else(|_| Pdfium::bind_to_system_library())
        .map_err(|e| ProcessingError::TextExtraction {
            page: 0,
            source: Box::new(std::io::Error::other(format!(
                "Failed to load PDFium library. Run `just download-pdfium` or install libpdfium: {:?}",
                e
            ))),
        })?;

    Ok(Pdfium::new(bindings))
}

/// Document ingestion service
pub struct IngestionService {
    chunk_size: usize,
    chunk_overlap: usize,
    data_dir: PathBuf,
}

impl IngestionService {
    pub fn new(config: &EmbeddingsConfig, data_dir: PathBuf) -> Self {
        Self {
            chunk_size: config.chunk_size,
            chunk_overlap: config.chunk_overlap,
            data_dir,
        }
    }

    /// Process a document with a pre-generated document ID, returning only chunks
    /// Used for async document processing where the Document record is created first
    pub fn process_document_with_id(
        &self,
        path: &Path,
        doc_id: &str,
        _title: &str,
        access_level: AccessLevel,
        tags: Vec<String>,
    ) -> ServiceResult<Vec<Chunk>> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        info!(path = %path.display(), format = %extension, doc_id = %doc_id, "Processing document");

        let content = match extension.as_str() {
            "pdf" => self.extract_pdf(path)?,
            "epub" => self.extract_epub(path)?,
            "md" | "markdown" => self.extract_markdown(path)?,
            "txt" | "text" => self.extract_text(path)?,
            _ => {
                return Err(ServiceError::Processing(
                    ProcessingError::UnsupportedFormat { format: extension },
                ));
            }
        };

        // Create chunks
        let chunks = self.create_chunks(doc_id, &content, access_level, &tags);

        info!(
            doc_id = %doc_id,
            chunks = chunks.len(),
            "Document processed successfully"
        );

        Ok(chunks)
    }

    /// Extract content from PDF using PDFium
    fn extract_pdf(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let pdfium = create_pdfium()?;

        let document =
            pdfium
                .load_pdf_from_file(path, None)
                .map_err(|e| ProcessingError::TextExtraction {
                    page: 0,
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to load PDF: {:?}", e),
                    )),
                })?;

        let mut sections = Vec::new();
        let page_count = document.pages().len();

        info!(pages = page_count, "Processing PDF pages");

        for (page_index, page) in document.pages().iter().enumerate() {
            let page_num = page_index as i32 + 1;

            // Extract text from the page
            let text = page.text().map_err(|e| {
                warn!(page = page_num, error = ?e, "Failed to get text object for page");
                ProcessingError::TextExtraction {
                    page: page_num as u32,
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to extract text from page {}: {:?}", page_num, e),
                    )),
                }
            })?;

            let page_text = text.all();
            let page_text = page_text.trim();

            if !page_text.is_empty() {
                sections.push(Section {
                    title: None,
                    content: page_text.to_string(),
                    page_number: Some(page_num),
                });
            }
        }

        if sections.is_empty() {
            return Err(ServiceError::Processing(ProcessingError::TextExtraction {
                page: 0,
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "No text could be extracted from PDF",
                )),
            }));
        }

        debug!(
            pages = page_count,
            sections = sections.len(),
            "PDF text extracted"
        );

        Ok(ExtractedContent { sections })
    }

    /// Extract text from specific pages of a PDF
    /// Returns a HashMap of page_number (1-indexed) -> page_text
    pub fn extract_pdf_page_text(
        &self,
        path: &Path,
        page_numbers: &[i32],
    ) -> ServiceResult<std::collections::HashMap<i32, String>> {
        use std::collections::HashMap;

        if page_numbers.is_empty() {
            return Ok(HashMap::new());
        }

        let pdfium = Pdfium::default();
        let document =
            pdfium
                .load_pdf_from_file(path, None)
                .map_err(|e| ProcessingError::TextExtraction {
                    page: 0,
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to load PDF: {:?}", e),
                    )),
                })?;

        let page_count = document.pages().len() as i32;
        let mut result = HashMap::new();

        for &page_num in page_numbers {
            // Skip invalid page numbers
            if page_num < 1 || page_num > page_count {
                warn!(
                    page = page_num,
                    total_pages = page_count,
                    "Requested page number out of range"
                );
                continue;
            }

            let page_index = (page_num - 1) as u16;
            if let Ok(page) = document.pages().get(page_index)
                && let Ok(text) = page.text()
            {
                let page_text = text.all().trim().to_string();
                if !page_text.is_empty() {
                    result.insert(page_num, page_text);
                }
            }
        }

        debug!(
            requested_pages = page_numbers.len(),
            extracted_pages = result.len(),
            "Extracted page text from PDF"
        );

        Ok(result)
    }

    /// Extract images from a PDF document and save them as WebP files
    /// Returns a list of DocumentImage records (without descriptions - those are added separately)
    ///
    /// Uses poppler-rs for programmatic access to PDF images with position information,
    /// allowing proper layer compositing (e.g., character artwork with drop shadows).
    /// Uses qpdf to extract transformation matrices (CTMs) and applies them to correct
    /// image orientation (rotation, mirroring).
    pub fn extract_pdf_images(
        &self,
        path: &Path,
        document_id: &str,
    ) -> ServiceResult<Vec<DocumentImage>> {
        // Create images directory for this document
        let images_dir = self.data_dir.join("images").join(document_id);
        std::fs::create_dir_all(&images_dir).map_err(ProcessingError::Io)?;

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
        let doc = PopplerDocument::from_file(&uri, None).map_err(|e| {
            ProcessingError::TextExtraction {
                page: 0,
                source: Box::new(std::io::Error::other(format!(
                    "Failed to load PDF with poppler: {}",
                    e
                ))),
            }
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
                let surface_data =
                    unsafe { std::slice::from_raw_parts(data_ptr, data_len).to_vec() };

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
                let (
                    final_area,
                    transform_matrix,
                    crop_pixels,
                    smask_data,
                    smask_width,
                    smask_height,
                ) = if let Some(ref t) = matched_transform {
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
        let mut page_image_counts: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();

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

    /// Get the path where an image should be copied to in FVTT assets
    pub fn fvtt_image_path(
        document_title: &str,
        page_number: i32,
        description: Option<&str>,
    ) -> PathBuf {
        let sanitized_title = sanitize_filename(document_title);
        let sanitized_desc = description
            .map(|d| {
                format!(
                    "_{}",
                    sanitize_filename(&d.chars().take(30).collect::<String>())
                )
            })
            .unwrap_or_default();

        PathBuf::from(format!(
            "assets/seneschal/{}/page_{}{}.webp",
            sanitized_title, page_number, sanitized_desc
        ))
    }

    /// Extract content from EPUB
    fn extract_epub(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let mut archive =
            epub::doc::EpubDoc::new(path).map_err(|e| ProcessingError::EpubRead(e.to_string()))?;

        let mut sections = Vec::new();
        let mut chapter_index = 0;

        // Iterate through spine (reading order)
        while archive.go_next() {
            if let Some((content, _mime)) = archive.get_current_str() {
                // Strip HTML tags (basic approach)
                let text = strip_html_tags(&content);
                let text = text.trim().to_string();

                if !text.is_empty() {
                    let chapter_title = archive
                        .get_current_id()
                        .map(|id| format!("Chapter: {}", id));

                    sections.push(Section {
                        title: chapter_title,
                        content: text,
                        page_number: Some(chapter_index),
                    });
                    chapter_index += 1;
                }
            }
        }

        if sections.is_empty() {
            return Err(ServiceError::Processing(ProcessingError::EpubRead(
                "No content could be extracted from EPUB".to_string(),
            )));
        }

        debug!(chapters = sections.len(), "EPUB extracted");

        Ok(ExtractedContent { sections })
    }

    /// Extract content from Markdown
    fn extract_markdown(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let content = std::fs::read_to_string(path).map_err(ProcessingError::Io)?;

        // Parse markdown to extract sections based on headers
        let sections = self.parse_markdown_sections(&content);

        Ok(ExtractedContent { sections })
    }

    /// Parse markdown into sections based on headers
    fn parse_markdown_sections(&self, content: &str) -> Vec<Section> {
        let mut sections = Vec::new();
        let mut current_section = String::new();
        let mut current_title: Option<String> = None;

        for line in content.lines() {
            // Check for headers
            if line.starts_with('#') {
                // Save previous section
                if !current_section.trim().is_empty() {
                    sections.push(Section {
                        title: current_title.take(),
                        content: current_section.trim().to_string(),
                        page_number: None,
                    });
                    current_section = String::new();
                }

                // Extract header text
                let header_text = line.trim_start_matches('#').trim().to_string();
                current_title = Some(header_text);
            } else {
                current_section.push_str(line);
                current_section.push('\n');
            }
        }

        // Don't forget the last section
        if !current_section.trim().is_empty() {
            sections.push(Section {
                title: current_title,
                content: current_section.trim().to_string(),
                page_number: None,
            });
        }

        // If no sections were found, treat entire content as one section
        if sections.is_empty() && !content.trim().is_empty() {
            sections.push(Section {
                title: None,
                content: content.trim().to_string(),
                page_number: None,
            });
        }

        sections
    }

    /// Extract content from plain text
    fn extract_text(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let content = std::fs::read_to_string(path).map_err(ProcessingError::Io)?;

        Ok(ExtractedContent {
            sections: vec![Section {
                title: None,
                content: content.trim().to_string(),
                page_number: None,
            }],
        })
    }

    /// Create chunks from extracted content
    fn create_chunks(
        &self,
        document_id: &str,
        content: &ExtractedContent,
        access_level: AccessLevel,
        tags: &[String],
    ) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut chunk_index = 0;

        for section in &content.sections {
            let section_chunks =
                self.chunk_text(&section.content, self.chunk_size, self.chunk_overlap);

            for chunk_text in section_chunks {
                chunks.push(Chunk {
                    id: Uuid::new_v4().to_string(),
                    document_id: document_id.to_string(),
                    content: chunk_text,
                    chunk_index,
                    page_number: section.page_number,
                    section_title: section.title.clone(),
                    access_level,
                    tags: tags.to_vec(),
                    metadata: None,
                    created_at: Utc::now(),
                });
                chunk_index += 1;
            }
        }

        chunks
    }

    /// Split text into overlapping chunks
    fn chunk_text(&self, text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();

        if words.len() <= chunk_size {
            return vec![text.to_string()];
        }

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < words.len() {
            let end = (start + chunk_size).min(words.len());
            let chunk: String = words[start..end].join(" ");
            chunks.push(chunk);

            // Move start forward, accounting for overlap
            start += chunk_size - overlap;

            // Avoid infinite loop
            if start >= words.len() - overlap && end == words.len() {
                break;
            }
        }

        chunks
    }
}

/// Extracted document content
struct ExtractedContent {
    sections: Vec<Section>,
}

/// Document section
struct Section {
    title: Option<String>,
    content: String,
    page_number: Option<i32>,
}

/// Strip HTML tags from content (basic implementation)
fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut last_was_space = true;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                // Add space after closing tag to separate words
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            }
            _ if !in_tag => {
                // Handle HTML entities
                if c.is_whitespace() {
                    if !last_was_space {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    result.push(c);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }

    // Decode common HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// Sanitize a string for use as a filename
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_whitespace() => '_',
            c => c,
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text() {
        let service = IngestionService {
            chunk_size: 10,
            chunk_overlap: 2,
            data_dir: PathBuf::from("/tmp"),
        };

        let text = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen";
        let chunks = service.chunk_text(text, 5, 1);

        assert!(!chunks.is_empty());
        // First chunk should have 5 words
        assert_eq!(chunks[0].split_whitespace().count(), 5);
    }

    #[test]
    fn test_strip_html() {
        let html = "<p>Hello <b>world</b>!</p>";
        let text = strip_html_tags(html);
        assert_eq!(text.trim(), "Hello world !");
    }

    #[test]
    fn test_markdown_sections() {
        let service = IngestionService {
            chunk_size: 512,
            chunk_overlap: 64,
            data_dir: PathBuf::from("/tmp"),
        };

        let markdown = r#"
# Chapter 1

This is the first chapter.

## Section 1.1

Some content here.

# Chapter 2

Another chapter.
"#;

        let sections = service.parse_markdown_sections(markdown);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].title, Some("Chapter 1".to_string()));
        assert_eq!(sections[1].title, Some("Section 1.1".to_string()));
        assert_eq!(sections[2].title, Some("Chapter 2".to_string()));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Hello World"), "Hello_World");
        assert_eq!(sanitize_filename("File/Name:Test"), "File_Name_Test");
        assert_eq!(sanitize_filename("  spaces  "), "spaces");
    }

    #[test]
    fn test_fvtt_image_path() {
        // Note: The seneschal/ prefix is added at the config level, not here
        let path = IngestionService::fvtt_image_path("Core Rulebook", 42, Some("starship map"));
        assert_eq!(
            path.to_string_lossy(),
            "Core_Rulebook/page_42_starship_map.webp"
        );

        let path_no_desc = IngestionService::fvtt_image_path("Test Doc", 1, None);
        assert_eq!(path_no_desc.to_string_lossy(), "Test_Doc/page_1.webp");
    }
}
