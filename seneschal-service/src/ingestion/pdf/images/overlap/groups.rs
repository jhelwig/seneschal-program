//! Overlap group management for PDF images.
//!
//! This module handles the detection and management of groups of overlapping images.
//! Images that overlap with each other or share overlapping content (text, paths)
//! are grouped together for combined region rendering.

use std::collections::HashMap;

use tracing::debug;

use super::super::{ImageInfo, Rectangle};
use super::regions::{
    ContentRegion, calculate_image_dpi, compute_union, rectangles_adjacent, rectangles_intersect,
};
use super::union_find::UnionFind;

/// A group of overlapping images that will share a single region render.
#[derive(Debug, Clone)]
pub struct OverlapGroup {
    /// Indices of images in this overlap group
    pub image_indices: Vec<usize>,
    /// The combined bounding box of all items in the group
    pub combined_region: Rectangle,
    /// Whether the group includes text overlap
    pub has_text_overlap: bool,
    /// Whether the group includes path overlap
    pub has_path_overlap: bool,
}

/// Detect overlap groups among images on a single page.
///
/// Images that overlap with each other (directly or transitively through
/// shared text/path content) are grouped together. Each group will have
/// a single region render created.
///
/// # Arguments
/// * `page_images` - Images on this page (indices into all_images)
/// * `all_images` - All images from the document
/// * `text_regions` - Text bounding boxes on this page
/// * `path_regions` - Path bounding boxes on this page
/// * `is_background` - Function to check if an image index is a background
///
/// # Returns
/// A list of overlap groups, each containing the image indices and combined region
pub fn detect_overlap_groups<F>(
    page_images: &[usize],
    all_images: &[ImageInfo],
    text_regions: &[ContentRegion],
    path_regions: &[ContentRegion],
    is_background: F,
) -> Vec<OverlapGroup>
where
    F: Fn(usize) -> bool,
{
    if page_images.is_empty() {
        return vec![];
    }

    let page_number = all_images[page_images[0]].page_number;

    // Filter to non-background images on this page
    let non_bg_indices: Vec<usize> = page_images
        .iter()
        .copied()
        .filter(|&idx| !is_background(idx))
        .collect();

    if non_bg_indices.is_empty() {
        return vec![];
    }

    // Create union-find for grouping
    // We'll use local indices (0..non_bg_indices.len()) for union-find
    let mut uf = UnionFind::new(non_bg_indices.len());

    // Track which content regions overlap with which images
    // content_to_images[content_idx] = list of local image indices that overlap with it
    let all_content: Vec<&ContentRegion> = text_regions.iter().chain(path_regions.iter()).collect();
    let mut content_to_images: Vec<Vec<usize>> = vec![vec![]; all_content.len()];

    // For each image, find which content regions it overlaps with
    for (local_idx, &global_idx) in non_bg_indices.iter().enumerate() {
        let image_bounds = all_images[global_idx].area;

        for (content_idx, content) in all_content.iter().enumerate() {
            if rectangles_intersect(&image_bounds, &content.bounds) {
                content_to_images[content_idx].push(local_idx);
            }
        }
    }

    // Union images that share overlapping content
    for images_sharing_content in &content_to_images {
        if images_sharing_content.len() > 1 {
            // All images overlapping with this content should be in the same group
            let first = images_sharing_content[0];
            for &other in &images_sharing_content[1..] {
                uf.union(first, other);
            }
        }
    }

    // Also union images that overlap or are adjacent (touching/nearly touching)
    for (i, &global_i) in non_bg_indices.iter().enumerate() {
        for (j, &global_j) in non_bg_indices.iter().enumerate().skip(i + 1) {
            if rectangles_adjacent(&all_images[global_i].area, &all_images[global_j].area) {
                uf.union(i, j);
            }
        }
    }

    // Collect groups
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (local_idx, _) in non_bg_indices.iter().enumerate() {
        let root = uf.find(local_idx);
        groups.entry(root).or_default().push(local_idx);
    }

    // Build OverlapGroup for each group that has overlaps
    let mut result = Vec::new();
    for (_root, local_indices) in groups {
        // Convert local indices back to global indices
        let global_indices: Vec<usize> = local_indices
            .iter()
            .map(|&local| non_bg_indices[local])
            .collect();

        // Check what types of overlap this group has
        let mut has_text_overlap = false;
        let mut has_path_overlap = false;
        let mut all_bounds: Vec<Rectangle> = Vec::new();

        for &global_idx in &global_indices {
            let image_bounds = all_images[global_idx].area;
            all_bounds.push(image_bounds);

            // Check text overlaps
            for text_region in text_regions {
                if rectangles_intersect(&image_bounds, &text_region.bounds) {
                    has_text_overlap = true;
                    all_bounds.push(text_region.bounds);
                }
            }

            // Check path overlaps
            for path_region in path_regions {
                if rectangles_intersect(&image_bounds, &path_region.bounds) {
                    has_path_overlap = true;
                    all_bounds.push(path_region.bounds);
                }
            }
        }

        // Only create a group if there's actual overlap (multiple images or text/path overlap)
        let has_image_overlap = global_indices.len() > 1;
        if has_text_overlap || has_path_overlap || has_image_overlap {
            let combined_region =
                compute_union(&all_bounds).unwrap_or(all_images[global_indices[0]].area);

            debug!(
                page = page_number + 1,
                images = global_indices.len(),
                has_text = has_text_overlap,
                has_path = has_path_overlap,
                region = format!(
                    "({:.1},{:.1})-({:.1},{:.1})",
                    combined_region.x1, combined_region.y1, combined_region.x2, combined_region.y2
                ),
                "Detected overlap group"
            );

            result.push(OverlapGroup {
                image_indices: global_indices,
                combined_region,
                has_text_overlap,
                has_path_overlap,
            });
        }
    }

    // Post-process: merge groups whose combined regions overlap
    // This handles cases where images don't directly overlap but their
    // combined regions (including text/path overlaps) do overlap
    merge_overlapping_groups(result)
}

/// Merge overlap groups whose combined regions intersect.
///
/// Repeatedly merges groups until no more merging is possible.
fn merge_overlapping_groups(mut groups: Vec<OverlapGroup>) -> Vec<OverlapGroup> {
    if groups.len() <= 1 {
        return groups;
    }

    // Keep merging until no changes occur
    loop {
        let mut merged = false;

        // Try to find two groups to merge
        'outer: for i in 0..groups.len() {
            for j in (i + 1)..groups.len() {
                if rectangles_intersect(&groups[i].combined_region, &groups[j].combined_region) {
                    // Merge group j into group i
                    let group_j = groups.remove(j);

                    // Combine image indices
                    groups[i].image_indices.extend(group_j.image_indices);

                    // Combine regions
                    groups[i].combined_region =
                        compute_union(&[groups[i].combined_region, group_j.combined_region])
                            .unwrap_or(groups[i].combined_region);

                    // Combine overlap flags
                    groups[i].has_text_overlap |= group_j.has_text_overlap;
                    groups[i].has_path_overlap |= group_j.has_path_overlap;

                    merged = true;
                    break 'outer;
                }
            }
        }

        if !merged {
            break;
        }
    }

    // Deduplicate image indices within each group
    for group in &mut groups {
        group.image_indices.sort_unstable();
        group.image_indices.dedup();
    }

    groups
}

/// Calculate the appropriate DPI for a region render based on the overlap group.
///
/// When text or vectors are involved, enforces a minimum DPI for legibility.
/// Otherwise, matches the highest resolution image in the group.
///
/// DPI is capped at a reasonable maximum (600) to prevent memory issues when
/// tiny decorative elements have artificially high effective DPI due to small
/// PDF bounding boxes.
pub fn calculate_group_region_dpi(
    group: &OverlapGroup,
    all_images: &[ImageInfo],
    text_overlap_min_dpi: f64,
) -> f64 {
    // Reasonable maximum - 600 DPI is very high quality print resolution
    const MAX_REGION_DPI: f64 = 600.0;

    // Find the maximum DPI among all images in the group
    let mut max_dpi = 72.0;
    for &idx in &group.image_indices {
        let dpi = calculate_image_dpi(&all_images[idx]);
        if dpi > max_dpi {
            max_dpi = dpi;
        }
    }

    // Cap at reasonable maximum to prevent impossibly large renders
    max_dpi = max_dpi.min(MAX_REGION_DPI);

    // When text or vectors are involved, enforce minimum DPI for legibility
    if group.has_text_overlap || group.has_path_overlap {
        max_dpi = max_dpi.max(text_overlap_min_dpi);
    }

    max_dpi
}
