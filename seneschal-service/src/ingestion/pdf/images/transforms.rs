//! PDF image transformation and conversion handling.
//!
//! This module handles:
//! - CTM (Current Transformation Matrix) extraction from PDF content streams
//! - Image orientation correction (rotation, mirroring)
//! - Cairo surface format conversion to RGBA
//! - SMask (soft mask) application for transparency

mod apply;
mod conversion;
mod ctm;
mod smask;

use super::{ImageInfo, Rectangle};

// Re-export public types and functions used by parent module
pub use apply::{
    apply_transform, compute_bounds_from_ctm, find_matching_transform, needs_transformation,
};
pub use conversion::convert_to_rgba;
pub use ctm::extract_image_transforms_with_qpdf;
pub use smask::apply_smask;

/// Transformation matrix extracted from PDF content stream.
/// Represents the CTM (Current Transformation Matrix) applied to an image.
#[derive(Debug, Clone)]
pub struct ImageTransform {
    /// XObject name (e.g., "Im0", "I129") - kept for debugging
    #[allow(dead_code)]
    pub xobject_name: String,
    /// 6-element transformation matrix [a, b, c, d, e, f]
    /// [a b 0]
    /// [c d 0]
    /// [e f 1]
    pub matrix: [f64; 6],
    /// Expected width of the transformed image (calculated from CTM)
    /// width = sqrt(a² + b²)
    pub expected_width: f64,
    /// Expected height of the transformed image (calculated from CTM)
    /// height = sqrt(c² + d²)
    pub expected_height: f64,
    /// Axis-aligned bounding box computed from CTM (for rotated images)
    /// This gives the TRUE position on the page after transformation
    pub computed_bounds: Option<(f64, f64, f64, f64)>, // (x1, y1, x2, y2)
    /// Clipping rectangle active when the image was drawn (if any)
    /// This should be used to constrain the visible area of the image
    pub clip_rect: Option<(f64, f64, f64, f64)>, // (x1, y1, x2, y2)
    /// Soft mask (SMask) data for transparency, if the image has one
    /// This is raw grayscale pixel data where 0 = transparent, 255 = opaque
    pub smask_data: Option<Vec<u8>>,
    /// Width of the SMask image in pixels
    pub smask_width: Option<u32>,
    /// Height of the SMask image in pixels
    pub smask_height: Option<u32>,
}
