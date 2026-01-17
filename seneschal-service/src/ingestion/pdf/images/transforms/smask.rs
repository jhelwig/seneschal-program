//! Soft mask (SMask) transparency handling.

use image::RgbaImage;

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
