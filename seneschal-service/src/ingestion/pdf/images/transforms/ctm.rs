//! CTM (Current Transformation Matrix) extraction from PDF content streams via qpdf.

use std::collections::HashMap;
use std::path::Path;

use qpdf::{QPdf, StreamDecodeLevel};
use tracing::{debug, trace};

use super::ImageTransform;
use crate::error::ProcessingError;

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
                let has_rotation = super::needs_transformation(&transform.matrix);
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
                        let computed_bounds = Some(super::compute_bounds_from_ctm(&current_ctm));

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
