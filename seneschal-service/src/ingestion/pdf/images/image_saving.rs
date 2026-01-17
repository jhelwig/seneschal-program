//! Image saving utilities for PDF extraction.

use std::fs::File;
use std::path::Path;

use chrono::{DateTime, Utc};
use image::ImageEncoder;
use image::codecs::webp::WebPEncoder;
use tracing::debug;
use uuid::Uuid;

use crate::db::{DocumentImage, ImageType};
use crate::error::{ProcessingError, ServiceResult};

use super::overlap::OverlapGroup;
use super::transforms::{apply_smask, apply_transform, convert_to_rgba, needs_transformation};
use super::types::ImageInfo;

/// Save an individual image to disk.
///
/// Returns `Ok(Some(image))` if saved successfully, `Ok(None)` if the image
/// was intentionally skipped (e.g., too small), or `Err` for actual failures.
pub fn save_individual_image(
    info: &ImageInfo,
    images_dir: &Path,
    document_id: &str,
    page_number: i32,
    image_index: usize,
    image_type: ImageType,
    created_at: DateTime<Utc>,
) -> ServiceResult<Option<DocumentImage>> {
    // Convert to RGBA
    let mut img = convert_to_rgba(info);

    // Apply SMask if present
    if let (Some(smask_data), Some(smask_width), Some(smask_height)) =
        (&info.smask_data, info.smask_width, info.smask_height)
    {
        apply_smask(&mut img, smask_data, smask_width, smask_height);
    }

    // Apply transformation if needed
    let img = if let Some(ref matrix) = info.transform {
        if needs_transformation(matrix) {
            apply_transform(&img, matrix)
        } else {
            img
        }
    } else {
        img
    };

    // Apply pixel crop if needed
    let img = if let Some((crop_x, crop_y, crop_w, crop_h)) = info.crop_pixels {
        image::imageops::crop_imm(&img, crop_x, crop_y, crop_w, crop_h).to_image()
    } else {
        img
    };

    let width = img.width();
    let height = img.height();

    // Skip images that are too small (not an error, just not useful)
    const MIN_IMAGE_SIZE: u32 = 32;
    if width < MIN_IMAGE_SIZE || height < MIN_IMAGE_SIZE {
        debug!(
            page = page_number,
            image_index = image_index,
            width = width,
            height = height,
            "Skipping image: too small"
        );
        return Ok(None);
    }

    // Save as WebP
    let image_id = Uuid::new_v4().to_string();
    let webp_filename = format!("page_{}_img_{}.webp", page_number, image_index);
    let webp_path = images_dir.join(&webp_filename);

    let file = File::create(&webp_path).map_err(|e| ProcessingError::TextExtraction {
        page: page_number as u32,
        source: Box::new(e),
    })?;

    let encoder = WebPEncoder::new_lossless(file);
    encoder
        .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| ProcessingError::TextExtraction {
            page: page_number as u32,
            source: Box::new(std::io::Error::other(format!(
                "Failed to encode WebP: {}",
                e
            ))),
        })?;

    debug!(
        page = page_number,
        image_index = image_index,
        width = width,
        height = height,
        image_type = ?image_type,
        "Saved individual image"
    );

    Ok(Some(DocumentImage {
        id: image_id,
        document_id: document_id.to_string(),
        page_number,
        image_index: image_index as i32,
        internal_path: webp_path.to_string_lossy().to_string(),
        mime_type: "image/webp".to_string(),
        width: Some(width),
        height: Some(height),
        description: None,
        source_pages: Some(vec![page_number]),
        image_type,
        source_image_id: None,
        has_region_render: false,
        created_at,
    }))
}

/// Save a region render for an overlap group to disk.
#[allow(clippy::too_many_arguments)]
pub fn save_group_region_render(
    image: &image::RgbaImage,
    images_dir: &Path,
    document_id: &str,
    page_number: i32,
    group_index: usize,
    source_image_id: Option<&str>,
    group: &OverlapGroup,
    created_at: DateTime<Utc>,
) -> ServiceResult<DocumentImage> {
    let width = image.width();
    let height = image.height();

    let image_id = Uuid::new_v4().to_string();
    // Name using group index to indicate this is a grouped region render
    let webp_filename = format!("page_{}_group_{}_region.webp", page_number, group_index);
    let webp_path = images_dir.join(&webp_filename);

    let file = File::create(&webp_path).map_err(|e| ProcessingError::TextExtraction {
        page: page_number as u32,
        source: Box::new(e),
    })?;

    let encoder = WebPEncoder::new_lossless(file);
    encoder
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| ProcessingError::TextExtraction {
            page: page_number as u32,
            source: Box::new(std::io::Error::other(format!(
                "Failed to encode region WebP: {}",
                e
            ))),
        })?;

    debug!(
        page = page_number,
        group_index = group_index,
        images_in_group = group.image_indices.len(),
        width = width,
        height = height,
        "Saved overlap group region render"
    );

    Ok(DocumentImage {
        id: image_id,
        document_id: document_id.to_string(),
        page_number,
        image_index: group_index as i32,
        internal_path: webp_path.to_string_lossy().to_string(),
        mime_type: "image/webp".to_string(),
        width: Some(width),
        height: Some(height),
        description: None,
        source_pages: Some(vec![page_number]),
        image_type: ImageType::RegionRender,
        source_image_id: source_image_id.map(String::from),
        has_region_render: false,
        created_at,
    })
}
