//! Overlap detection for PDF images using pdfium-render.
//!
//! This module detects when images overlap with text, vector graphics (paths),
//! or other images. Overlapping items are grouped together, and a single
//! region render is created per overlap group.

use pdfium_render::prelude::*;
use tracing::{debug, trace};

use super::{ImageInfo, Rectangle};

/// A detected content region on a page
#[derive(Debug, Clone)]
pub struct ContentRegion {
    pub bounds: Rectangle,
}

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

/// Union-Find data structure for grouping overlapping items
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]); // Path compression
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let root_x = self.find(x);
        let root_y = self.find(y);

        if root_x != root_y {
            // Union by rank
            match self.rank[root_x].cmp(&self.rank[root_y]) {
                std::cmp::Ordering::Less => self.parent[root_x] = root_y,
                std::cmp::Ordering::Greater => self.parent[root_y] = root_x,
                std::cmp::Ordering::Equal => {
                    self.parent[root_y] = root_x;
                    self.rank[root_x] += 1;
                }
            }
        }
    }
}

/// Convert pdfium PdfRect to our Rectangle type
fn pdf_rect_to_rectangle(rect: &PdfRect) -> Rectangle {
    Rectangle {
        x1: rect.left().value as f64,
        y1: rect.bottom().value as f64,
        x2: rect.right().value as f64,
        y2: rect.top().value as f64,
    }
}

/// Check if two rectangles intersect
fn rectangles_intersect(a: &Rectangle, b: &Rectangle) -> bool {
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

/// Compute the union of multiple rectangles (axis-aligned bounding box)
fn compute_union(rects: &[Rectangle]) -> Option<Rectangle> {
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

/// Compute the intersection of two rectangles, returning None if they don't overlap.
fn intersect_rectangles(a: &Rectangle, b: &Rectangle) -> Option<Rectangle> {
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
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
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

    #[test]
    fn test_union_find() {
        let mut uf = UnionFind::new(5);

        // Initially all separate
        assert_ne!(uf.find(0), uf.find(1));

        // Union 0 and 1
        uf.union(0, 1);
        assert_eq!(uf.find(0), uf.find(1));

        // Union 2 and 3
        uf.union(2, 3);
        assert_eq!(uf.find(2), uf.find(3));

        // 0,1 and 2,3 still separate
        assert_ne!(uf.find(0), uf.find(2));

        // Union the groups via 1 and 2
        uf.union(1, 2);
        assert_eq!(uf.find(0), uf.find(3));
    }
}
