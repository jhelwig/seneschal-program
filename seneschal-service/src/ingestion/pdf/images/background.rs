//! Background image detection for PDF documents.
//!
//! This module detects images that are used as page backgrounds - typically images
//! that cover most of a page's area and appear across multiple pages. These are
//! extracted once and excluded from overlap detection.

use std::collections::{HashMap, HashSet};

use crate::config::ImageExtractionConfig;

use super::ImageInfo;

/// Signature for identifying similar images across pages.
/// Uses bucketed dimensions and position to handle minor variations.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ImageSignature {
    /// Width bucketed to nearest 10 pixels
    width_bucket: u32,
    /// Height bucketed to nearest 10 pixels
    height_bucket: u32,
    /// X position bucketed to nearest 10 points
    x_bucket: i32,
    /// Y position bucketed to nearest 10 points
    y_bucket: i32,
}

impl ImageSignature {
    /// Compute signature for an image
    pub fn from_image(info: &ImageInfo) -> Self {
        Self {
            width_bucket: (info.width as u32) / 10,
            height_bucket: (info.height as u32) / 10,
            x_bucket: (info.area.x1 / 10.0).round() as i32,
            y_bucket: (info.area.y1 / 10.0).round() as i32,
        }
    }
}

/// Calculate coverage ratio of an image on its page
fn calculate_coverage(info: &ImageInfo) -> f64 {
    let page_area = info.page_width * info.page_height;
    if page_area <= 0.0 {
        return 0.0;
    }
    let image_area = info.area.area();
    image_area / page_area
}

/// Detect background images that should be extracted once and excluded from overlap checks.
///
/// An image is considered a background if:
/// 1. It covers at least `background_area_threshold` (default 90%) of the page
/// 2. It appears on at least `background_min_pages` (default 2) pages
///
/// Returns a set of signatures that represent background images.
pub fn detect_backgrounds(
    images: &[ImageInfo],
    config: &ImageExtractionConfig,
) -> HashSet<ImageSignature> {
    // Group images by signature and track which pages they appear on
    let mut signature_pages: HashMap<ImageSignature, HashSet<usize>> = HashMap::new();
    let mut signature_coverage: HashMap<ImageSignature, f64> = HashMap::new();

    for info in images {
        let coverage = calculate_coverage(info);

        // Only consider images that meet the coverage threshold
        if coverage < config.background_area_threshold {
            continue;
        }

        let sig = ImageSignature::from_image(info);

        signature_pages
            .entry(sig.clone())
            .or_default()
            .insert(info.page_number);

        // Track the max coverage seen for this signature
        let current_coverage = signature_coverage.entry(sig).or_insert(0.0);
        if coverage > *current_coverage {
            *current_coverage = coverage;
        }
    }

    // Filter to signatures that appear on enough pages
    signature_pages
        .into_iter()
        .filter(|(_, pages)| pages.len() >= config.background_min_pages)
        .map(|(sig, _)| sig)
        .collect()
}

/// Check if an image matches a background signature
pub fn is_background(info: &ImageInfo, backgrounds: &HashSet<ImageSignature>) -> bool {
    let sig = ImageSignature::from_image(info);
    backgrounds.contains(&sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_image_info(
        page: usize,
        x1: f64,
        y1: f64,
        width: i32,
        height: i32,
        page_width: f64,
        page_height: f64,
    ) -> ImageInfo {
        ImageInfo {
            image_id: 0,
            area: super::super::Rectangle {
                x1,
                y1,
                x2: x1 + width as f64,
                y2: y1 + height as f64,
            },
            surface_data: vec![],
            width,
            height,
            stride: width * 4,
            has_alpha: false,
            is_grayscale: false,
            scale_x: 1.0,
            scale_y: 1.0,
            page_number: page,
            page_width,
            page_height,
            transform: None,
            crop_pixels: None,
            smask_data: None,
            smask_width: None,
            smask_height: None,
        }
    }

    #[test]
    fn test_background_detection() {
        let config = ImageExtractionConfig {
            background_area_threshold: 0.9,
            background_min_pages: 2,
            text_overlap_min_dpi: 300.0,
        };

        // Create images: one background covering 95% of pages 0 and 1, one normal image
        let images = vec![
            // Background on page 0 (95% coverage: 950x950 on 1000x1000 page)
            make_image_info(0, 25.0, 25.0, 950, 950, 1000.0, 1000.0),
            // Same background on page 1
            make_image_info(1, 25.0, 25.0, 950, 950, 1000.0, 1000.0),
            // Normal image on page 2 (10% coverage)
            make_image_info(2, 100.0, 100.0, 100, 100, 1000.0, 1000.0),
        ];

        let backgrounds = detect_backgrounds(&images, &config);

        assert_eq!(backgrounds.len(), 1);
        assert!(is_background(&images[0], &backgrounds));
        assert!(is_background(&images[1], &backgrounds));
        assert!(!is_background(&images[2], &backgrounds));
    }

    #[test]
    fn test_single_page_not_background() {
        let config = ImageExtractionConfig {
            background_area_threshold: 0.9,
            background_min_pages: 2,
            text_overlap_min_dpi: 300.0,
        };

        // Large image on only one page
        let images = vec![make_image_info(0, 0.0, 0.0, 1000, 1000, 1000.0, 1000.0)];

        let backgrounds = detect_backgrounds(&images, &config);

        assert!(backgrounds.is_empty());
    }
}
