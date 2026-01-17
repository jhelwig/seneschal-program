//! Transform application and rotation handling.

use std::collections::HashMap;

use image::RgbaImage;
use tracing::{trace, warn};

use super::{ImageTransform, Rectangle};

/// Compute the axis-aligned bounding box from a CTM.
/// The CTM transforms a unit square [0,0] to [1,1] to the final position.
pub fn compute_bounds_from_ctm(matrix: &[f64; 6]) -> (f64, f64, f64, f64) {
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

/// Match a poppler image with a qpdf-extracted CTM using dimension AND position matching.
///
/// Returns the matching ImageTransform if found.
pub fn find_matching_transform(
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
        if let Some((ctm_x1, ctm_y1, ctm_x2, ctm_y2)) = transform.computed_bounds {
            // Check multiple position criteria - any one matching is sufficient
            let x1_close = (ctm_x1 - poppler_area.x1).abs() < position_tolerance;
            let x2_close = (ctm_x2 - poppler_area.x2).abs() < position_tolerance;
            let y1_close = (ctm_y1 - poppler_area.y1).abs() < position_tolerance;
            let y2_close = (ctm_y2 - poppler_area.y2).abs() < position_tolerance;

            // Also check center proximity
            let ctm_cx = (ctm_x1 + ctm_x2) / 2.0;
            let ctm_cy = (ctm_y1 + ctm_y2) / 2.0;
            let center_close = (ctm_cx - poppler_cx).abs() < position_tolerance
                && (ctm_cy - poppler_cy).abs() < position_tolerance;

            // Accept if centers are close OR if at least one x AND one y coordinate match
            let position_matches =
                center_close || ((x1_close || x2_close) && (y1_close || y2_close));

            // For rotated images, poppler often gets x1 right but y completely wrong
            let x1_very_close = (ctm_x1 - poppler_area.x1).abs() < 5.0;

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
                    "Dimensions match but no position criterion met - skipping CTM"
                );
                continue;
            }

            trace!(
                page = page_num + 1,
                image_dims = format!("{:.1} x {:.1}", image_width, image_height),
                ctm_dims = format!(
                    "{:.1} x {:.1}",
                    transform.expected_width, transform.expected_height
                ),
                matrix = ?transform.matrix,
                "Matched image to CTM by dimensions and position"
            );
            return Some(transform.clone());
        } else {
            // No computed bounds - fall back to dimension-only matching
            trace!(
                page = page_num + 1,
                image_dims = format!("{:.1} x {:.1}", image_width, image_height),
                ctm_dims = format!(
                    "{:.1} x {:.1}",
                    transform.expected_width, transform.expected_height
                ),
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

/// Check if a transformation matrix indicates the image needs to be transformed.
/// (i.e., it's not an identity or simple scaling matrix)
pub fn needs_transformation(matrix: &[f64; 6]) -> bool {
    let [a, b, c, d, _e, _f] = *matrix;

    // Check if this is approximately an identity matrix (with possible scaling)
    // If b or c are non-zero, there's rotation
    let has_rotation = b.abs() > 0.01 || c.abs() > 0.01;

    // If a or d are negative, there's mirroring
    let has_mirroring = a < 0.0 || d < 0.0;

    has_rotation || has_mirroring
}

/// Apply transformation matrix to an image using affine transformation.
///
/// The CTM matrix [a, b, c, d, e, f] is normalized to remove scaling
/// and applied to correct the image orientation.
pub fn apply_transform(image: &RgbaImage, matrix: &[f64; 6]) -> RgbaImage {
    use imageproc::geometric_transformations::{Interpolation, Projection, warp_into};

    let [a, b, c, d, _e, _f] = *matrix;

    // Calculate scale factors (length of transformed unit vectors)
    let scale_x = (a * a + b * b).sqrt();
    let scale_y = (c * c + d * d).sqrt();

    if scale_x < 0.001 || scale_y < 0.001 {
        warn!("Invalid scale factors in CTM, skipping transformation");
        return image.clone();
    }

    // Normalize the matrix to remove scaling (we want rotation/mirroring only)
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

    // The inverse of [a, b; c, d] is (1/det) * [d, -b; -c, a]
    let inv_det = 1.0 / det;
    let inv_a = d_norm * inv_det;
    let inv_b = -b_norm * inv_det;
    let inv_c = -c_norm * inv_det;
    let inv_d = a_norm * inv_det;

    // Transform around the image center
    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;

    let tx = cx - inv_a * cx - inv_c * cy;
    let ty = cy - inv_b * cx - inv_d * cy;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_transformation() {
        // Identity matrix - no transformation needed
        let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        assert!(!needs_transformation(&identity));

        // Simple scaling - no transformation needed
        let scale = [2.0, 0.0, 0.0, 2.0, 10.0, 20.0];
        assert!(!needs_transformation(&scale));

        // 90 degree rotation - transformation needed
        let rotate_90 = [0.0, 1.0, -1.0, 0.0, 0.0, 0.0];
        assert!(needs_transformation(&rotate_90));

        // Horizontal flip - transformation needed
        let h_flip = [-1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        assert!(needs_transformation(&h_flip));
    }

    #[test]
    fn test_compute_bounds_from_ctm() {
        // Simple translation
        let translate = [1.0, 0.0, 0.0, 1.0, 100.0, 200.0];
        let (x1, y1, x2, y2) = compute_bounds_from_ctm(&translate);
        assert!((x1 - 100.0).abs() < 0.001);
        assert!((y1 - 200.0).abs() < 0.001);
        assert!((x2 - 101.0).abs() < 0.001);
        assert!((y2 - 201.0).abs() < 0.001);
    }
}
