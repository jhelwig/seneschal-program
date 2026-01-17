//! Overlap detection for PDF images using pdfium-render.
//!
//! This module detects when images overlap with text, vector graphics (paths),
//! or other images. Overlapping items are grouped together, and a single
//! region render is created per overlap group.

mod groups;
mod regions;
mod union_find;

// Re-export public types and functions used by the parent images.rs module
pub use groups::{OverlapGroup, calculate_group_region_dpi, detect_overlap_groups};
pub use regions::{
    ContentRegion, PdfiumImageInfo, extract_path_regions, extract_pdfium_images,
    extract_text_regions,
};
