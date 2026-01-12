//! PDF image transformation and conversion handling.
//!
//! This module handles:
//! - CTM (Current Transformation Matrix) extraction from PDF content streams
//! - Image orientation correction (rotation, mirroring)
//! - Cairo surface format conversion to RGBA
//! - SMask (soft mask) application for transparency

use std::collections::HashMap;
use std::path::Path;

use image::RgbaImage;
use qpdf::{QPdf, StreamDecodeLevel};
use tracing::{debug, trace, warn};

use super::{ImageInfo, Rectangle};
use crate::error::ProcessingError;

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

/// Convert an ImageInfo to an RGBA image.
///
/// Handles Cairo surface formats:
/// - ARGB32 (premultiplied alpha) - unpremultiplies alpha
/// - RGB24 - adds opaque alpha channel
/// - A8 (grayscale) - converts to RGBA
pub fn convert_to_rgba(info: &ImageInfo) -> RgbaImage {
    let width = info.width as u32;
    let height = info.height as u32;
    let mut img = RgbaImage::new(width, height);

    if info.is_grayscale {
        // Grayscale A8 format -> RGBA (gray, gray, gray, 255)
        // Cairo A8 stores alpha values, but for grayscale images we treat them as gray values
        for y in 0..height {
            for x in 0..width {
                let offset = (y as i32 * info.stride + x as i32) as usize;
                if offset < info.surface_data.len() {
                    let gray = info.surface_data[offset];
                    img.put_pixel(x, y, image::Rgba([gray, gray, gray, 255]));
                }
            }
        }
    } else if info.has_alpha {
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

/// Apply a soft mask (SMask) to an RGBA image.
///
/// The SMask is grayscale data where 0 = transparent, 255 = opaque.
/// This replaces the image's alpha channel with the SMask values.
pub fn apply_smask(img: &mut RgbaImage, smask_data: &[u8], smask_width: u32, smask_height: u32) {
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

/// Extract transformation matrices for images from PDF using qpdf.
///
/// Returns a map of page_num -> Vec<ImageTransform> for matching by dimensions.
pub fn extract_image_transforms_with_qpdf(
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

/// Parse a PDF content stream to extract all CTMs and clip rects applied to images.
/// Returns a Vec of ImageTransforms, one for each image draw command found.
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
                if i >= 1 {
                    let name = tokens[i - 1].trim_start_matches('/');
                    // Image XObjects are typically named ImN, Img, Image, etc.
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

/// Multiply two 2D transformation matrices.
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
