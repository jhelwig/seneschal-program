//! Cairo surface format conversion to RGBA.

use image::RgbaImage;

use super::ImageInfo;

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
