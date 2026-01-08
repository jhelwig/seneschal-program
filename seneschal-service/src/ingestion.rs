use chrono::Utc;
use image::codecs::webp::WebPEncoder;
use image::{ImageEncoder, RgbaImage};
use pdfium_render::prelude::*;
use poppler::Document as PopplerDocument;
use std::fs::File;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::config::EmbeddingsConfig;
use crate::db::{Chunk, DocumentImage};
use crate::error::{ProcessingError, ServiceError, ServiceResult};
use crate::tools::AccessLevel;

/// Rectangle representing image position on a PDF page
#[derive(Debug, Clone)]
struct Rectangle {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl Rectangle {
    fn area(&self) -> f64 {
        (self.x2 - self.x1).abs() * (self.y2 - self.y1).abs()
    }

    fn width(&self) -> f64 {
        (self.x2 - self.x1).abs()
    }

    fn height(&self) -> f64 {
        (self.y2 - self.y1).abs()
    }
}

/// Information about an extracted PDF image
struct ImageInfo {
    image_id: i32,
    area: Rectangle,
    surface_data: Vec<u8>,
    width: i32,
    height: i32,
    stride: i32,
    has_alpha: bool,
    is_grayscale: bool,
    /// Scale factor from PDF points to pixels (width)
    scale_x: f64,
    /// Scale factor from PDF points to pixels (height)
    scale_y: f64,
}

/// Check if two rectangles overlap by more than a threshold percentage
fn rectangles_overlap(a: &Rectangle, b: &Rectangle, threshold: f64) -> bool {
    let x_overlap = f64::max(0.0, f64::min(a.x2, b.x2) - f64::max(a.x1, b.x1));
    let y_overlap = f64::max(0.0, f64::min(a.y2, b.y2) - f64::max(a.y1, b.y1));
    let overlap_area = x_overlap * y_overlap;
    let smaller_area = a.area().min(b.area());
    if smaller_area <= 0.0 {
        return false;
    }
    overlap_area / smaller_area > threshold
}

/// Group images by overlapping bounding boxes using union-find
fn group_by_overlap(images: &[ImageInfo]) -> Vec<Vec<usize>> {
    if images.is_empty() {
        return Vec::new();
    }

    let n = images.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }

    fn union(parent: &mut [usize], i: usize, j: usize) {
        let pi = find(parent, i);
        let pj = find(parent, j);
        if pi != pj {
            parent[pi] = pj;
        }
    }

    // Group images that overlap by more than 70%
    const OVERLAP_THRESHOLD: f64 = 0.7;
    for i in 0..n {
        for j in (i + 1)..n {
            if rectangles_overlap(&images[i].area, &images[j].area, OVERLAP_THRESHOLD) {
                union(&mut parent, i, j);
            }
        }
    }

    // Collect groups
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    // Convert to vec and sort groups by their average position on page (top to bottom, left to right)
    let mut result: Vec<Vec<usize>> = groups.into_values().collect();
    result.sort_by(|a, b| {
        let avg_y_a: f64 = a.iter().map(|&i| images[i].area.y1).sum::<f64>() / a.len() as f64;
        let avg_y_b: f64 = b.iter().map(|&i| images[i].area.y1).sum::<f64>() / b.len() as f64;
        avg_y_a
            .partial_cmp(&avg_y_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    result
}

/// Convert an ImageInfo to an RGBA image
fn convert_to_rgba(info: &ImageInfo) -> RgbaImage {
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

/// Alpha blend two pixels (Porter-Duff "over" operation)
fn alpha_blend(dst: image::Rgba<u8>, src: image::Rgba<u8>) -> image::Rgba<u8> {
    let src_a = src[3] as f32 / 255.0;
    let dst_a = dst[3] as f32 / 255.0;

    // out_a = src_a + dst_a * (1 - src_a)
    let out_a = src_a + dst_a * (1.0 - src_a);

    if out_a <= 0.0 {
        return image::Rgba([0, 0, 0, 0]);
    }

    // out_rgb = (src_rgb * src_a + dst_rgb * dst_a * (1 - src_a)) / out_a
    let blend = |s: u8, d: u8| -> u8 {
        let s_f = s as f32 / 255.0;
        let d_f = d as f32 / 255.0;
        let out = (s_f * src_a + d_f * dst_a * (1.0 - src_a)) / out_a;
        (out * 255.0).clamp(0.0, 255.0) as u8
    };

    image::Rgba([
        blend(src[0], dst[0]),
        blend(src[1], dst[1]),
        blend(src[2], dst[2]),
        (out_a * 255.0) as u8,
    ])
}

/// Scale an image by a given factor using Lanczos3 filter
fn scale_image(img: &RgbaImage, scale: f64) -> RgbaImage {
    if (scale - 1.0).abs() < 0.01 {
        // No significant scaling needed
        return img.clone();
    }

    let new_width = ((img.width() as f64 * scale).ceil() as u32).max(1);
    let new_height = ((img.height() as f64 * scale).ceil() as u32).max(1);

    image::imageops::resize(img, new_width, new_height, image::imageops::FilterType::Lanczos3)
}

/// Composite a layer onto a canvas at the given offset
fn composite_over(canvas: &mut RgbaImage, layer: &RgbaImage, offset_x: i32, offset_y: i32) {
    for (ly, row) in layer.rows().enumerate() {
        for (lx, &pixel) in row.enumerate() {
            let cx = lx as i32 + offset_x;
            let cy = ly as i32 + offset_y;
            if cx >= 0 && cy >= 0 && cx < canvas.width() as i32 && cy < canvas.height() as i32 {
                let dst = canvas.get_pixel(cx as u32, cy as u32);
                let blended = alpha_blend(*dst, pixel);
                canvas.put_pixel(cx as u32, cy as u32, blended);
            }
        }
    }
}

/// Calculate the bounding box encompassing all images in a group
fn calculate_group_bounds(images: &[ImageInfo], indices: &[usize]) -> Rectangle {
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for &idx in indices {
        let area = &images[idx].area;
        min_x = min_x.min(area.x1).min(area.x2);
        min_y = min_y.min(area.y1).min(area.y2);
        max_x = max_x.max(area.x1).max(area.x2);
        max_y = max_y.max(area.y1).max(area.y2);
    }

    Rectangle {
        x1: min_x,
        y1: min_y,
        x2: max_x,
        y2: max_y,
    }
}

/// Composite a group of overlapping images into a single image.
///
/// Each image has a PDF bounding box (in points) and native pixel dimensions.
/// The composite canvas is sized to encompass all bounding boxes at the highest
/// available resolution (max pixels-per-point). Each image is then scaled to
/// fill its own bounding box at the canvas resolution and placed accordingly.
///
/// Layers are composited back-to-front based on image_id (lower IDs = back layers).
fn composite_group(images: &[ImageInfo], indices: &[usize]) -> Option<RgbaImage> {
    if indices.is_empty() {
        return None;
    }

    if indices.len() == 1 {
        // Single image - just convert to RGBA
        return Some(convert_to_rgba(&images[indices[0]]));
    }

    // Find the maximum scale factor (highest resolution image)
    let max_scale = indices
        .iter()
        .map(|&i| images[i].scale_x.max(images[i].scale_y))
        .fold(0.0_f64, f64::max);

    if max_scale <= 0.0 {
        return None;
    }

    // Calculate bounds in PDF points
    let bounds = calculate_group_bounds(images, indices);

    // Calculate canvas size in pixels using the max scale factor
    let canvas_width = (bounds.width() * max_scale).ceil() as u32;
    let canvas_height = (bounds.height() * max_scale).ceil() as u32;

    if canvas_width == 0 || canvas_height == 0 {
        return None;
    }

    // Create transparent canvas at full resolution
    let mut canvas = RgbaImage::new(canvas_width, canvas_height);

    // Sort indices by image_id ascending (lower IDs drawn first = back layer)
    // This matches the discovery that images are listed in reverse z-order
    let mut sorted_indices = indices.to_vec();
    sorted_indices.sort_by_key(|&idx| images[idx].image_id);

    // Composite each layer (back to front)
    for &idx in &sorted_indices {
        let info = &images[idx];
        let mut layer = convert_to_rgba(info);

        // Each image has a native resolution (pixels) and a PDF bounding box (points).
        // To composite correctly, we need to scale each image so it fills its bounding
        // box at the canvas resolution (max_scale pixels per point).
        //
        // scale_factor = (bounding_box_pts * max_scale) / native_pixels
        //              = max_scale / (native_pixels / bounding_box_pts)
        //              = max_scale / layer_scale
        let layer_scale = info.scale_x.max(info.scale_y);
        let scale_factor = max_scale / layer_scale;

        if (scale_factor - 1.0).abs() > 0.01 {
            // Scale image to fill its bounding box at canvas resolution
            layer = scale_image(&layer, scale_factor);
        }

        // Calculate offset in pixels (convert PDF points to pixels using max_scale)
        let offset_x = ((info.area.x1.min(info.area.x2) - bounds.x1) * max_scale) as i32;
        let offset_y = ((info.area.y1.min(info.area.y2) - bounds.y1) * max_scale) as i32;

        composite_over(&mut canvas, &layer, offset_x, offset_y);
    }

    Some(canvas)
}

/// Create a new Pdfium instance (dynamically linked)
/// Searches for libpdfium in:
/// 1. Current directory (./libpdfium.so)
/// 2. vendor/pdfium/lib/ (downloaded by `just download-pdfium`)
/// 3. System library paths
fn create_pdfium() -> Result<Pdfium, ProcessingError> {
    // Try local paths first, then system
    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| {
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(
                "./vendor/pdfium/lib/",
            ))
        })
        .or_else(|_| Pdfium::bind_to_system_library())
        .map_err(|e| ProcessingError::TextExtraction {
            page: 0,
            source: Box::new(std::io::Error::other(format!(
                "Failed to load PDFium library. Run `just download-pdfium` or install libpdfium: {:?}",
                e
            ))),
        })?;

    Ok(Pdfium::new(bindings))
}

/// Document ingestion service
pub struct IngestionService {
    chunk_size: usize,
    chunk_overlap: usize,
    data_dir: PathBuf,
}

impl IngestionService {
    pub fn new(config: &EmbeddingsConfig, data_dir: PathBuf) -> Self {
        Self {
            chunk_size: config.chunk_size,
            chunk_overlap: config.chunk_overlap,
            data_dir,
        }
    }

    /// Process a document with a pre-generated document ID, returning only chunks
    /// Used for async document processing where the Document record is created first
    pub fn process_document_with_id(
        &self,
        path: &Path,
        doc_id: &str,
        _title: &str,
        access_level: AccessLevel,
        tags: Vec<String>,
    ) -> ServiceResult<Vec<Chunk>> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        info!(path = %path.display(), format = %extension, doc_id = %doc_id, "Processing document");

        let content = match extension.as_str() {
            "pdf" => self.extract_pdf(path)?,
            "epub" => self.extract_epub(path)?,
            "md" | "markdown" => self.extract_markdown(path)?,
            "txt" | "text" => self.extract_text(path)?,
            _ => {
                return Err(ServiceError::Processing(
                    ProcessingError::UnsupportedFormat { format: extension },
                ));
            }
        };

        // Create chunks
        let chunks = self.create_chunks(doc_id, &content, access_level, &tags);

        info!(
            doc_id = %doc_id,
            chunks = chunks.len(),
            "Document processed successfully"
        );

        Ok(chunks)
    }

    /// Extract content from PDF using PDFium
    fn extract_pdf(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let pdfium = create_pdfium()?;

        let document =
            pdfium
                .load_pdf_from_file(path, None)
                .map_err(|e| ProcessingError::TextExtraction {
                    page: 0,
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to load PDF: {:?}", e),
                    )),
                })?;

        let mut sections = Vec::new();
        let page_count = document.pages().len();

        info!(pages = page_count, "Processing PDF pages");

        for (page_index, page) in document.pages().iter().enumerate() {
            let page_num = page_index as i32 + 1;

            // Extract text from the page
            let text = page.text().map_err(|e| {
                warn!(page = page_num, error = ?e, "Failed to get text object for page");
                ProcessingError::TextExtraction {
                    page: page_num as u32,
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to extract text from page {}: {:?}", page_num, e),
                    )),
                }
            })?;

            let page_text = text.all();
            let page_text = page_text.trim();

            if !page_text.is_empty() {
                sections.push(Section {
                    title: None,
                    content: page_text.to_string(),
                    page_number: Some(page_num),
                });
            }
        }

        if sections.is_empty() {
            return Err(ServiceError::Processing(ProcessingError::TextExtraction {
                page: 0,
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "No text could be extracted from PDF",
                )),
            }));
        }

        debug!(
            pages = page_count,
            sections = sections.len(),
            "PDF text extracted"
        );

        Ok(ExtractedContent { sections })
    }

    /// Extract images from a PDF document and save them as WebP files
    /// Returns a list of DocumentImage records (without descriptions - those are added separately)
    ///
    /// Uses poppler-rs for programmatic access to PDF images with position information,
    /// allowing proper layer compositing (e.g., character artwork with drop shadows).
    pub fn extract_pdf_images(
        &self,
        path: &Path,
        document_id: &str,
    ) -> ServiceResult<Vec<DocumentImage>> {
        // Create images directory for this document
        let images_dir = self.data_dir.join("images").join(document_id);
        std::fs::create_dir_all(&images_dir).map_err(ProcessingError::Io)?;

        // Load PDF with poppler
        let canonical_path = path.canonicalize().map_err(ProcessingError::Io)?;
        let uri = format!("file://{}", canonical_path.display());
        let doc = PopplerDocument::from_file(&uri, None).map_err(|e| {
            ProcessingError::TextExtraction {
                page: 0,
                source: Box::new(std::io::Error::other(format!(
                    "Failed to load PDF with poppler: {}",
                    e
                ))),
            }
        })?;

        let mut all_images = Vec::new();
        let now = Utc::now();
        let n_pages = doc.n_pages();

        info!(
            document_id = document_id,
            pages = n_pages,
            "Extracting images from PDF with poppler"
        );

        for page_num in 0..n_pages {
            let page = match doc.page(page_num) {
                Some(p) => p,
                None => continue,
            };

            let mappings = page.image_mapping();
            if mappings.is_empty() {
                continue;
            }

            // Extract image info from this page
            let mut page_images: Vec<ImageInfo> = Vec::new();

            for mapping in mappings.iter() {
                // Access the raw mapping data to get image_id and area
                let ptr = mapping.as_ptr();
                let (image_id, area) = unsafe {
                    let raw = &*ptr;
                    let image_id = raw.image_id;
                    let area = Rectangle {
                        x1: raw.area.x1,
                        y1: raw.area.y1,
                        x2: raw.area.x2,
                        y2: raw.area.y2,
                    };
                    (image_id, area)
                };

                // Get the image surface
                let surface = match page.image(image_id) {
                    Some(s) => s,
                    None => {
                        debug!(
                            page = page_num + 1,
                            image_id = image_id,
                            "Could not get image surface"
                        );
                        continue;
                    }
                };

                // Get surface properties using cairo FFI
                let raw_surface = surface.to_raw_none();
                let (format, width, height, stride) = unsafe {
                    use cairo::ffi;
                    let format = ffi::cairo_image_surface_get_format(raw_surface);
                    let width = ffi::cairo_image_surface_get_width(raw_surface);
                    let height = ffi::cairo_image_surface_get_height(raw_surface);
                    let stride = ffi::cairo_image_surface_get_stride(raw_surface);
                    (format, width, height, stride)
                };

                // Determine image type from format
                // CAIRO_FORMAT_ARGB32 = 0
                // CAIRO_FORMAT_RGB24 = 1
                // CAIRO_FORMAT_A8 = 2
                let (has_alpha, is_grayscale) = match format {
                    0 => (true, false),  // ARGB32
                    1 => (false, false), // RGB24
                    2 => (false, true),  // A8 (grayscale)
                    _ => {
                        debug!(
                            page = page_num + 1,
                            image_id = image_id,
                            format = format,
                            "Unknown Cairo format"
                        );
                        continue;
                    }
                };

                // Get surface data
                let data_ptr = unsafe {
                    use cairo::ffi;
                    ffi::cairo_image_surface_get_data(raw_surface)
                };

                if data_ptr.is_null() {
                    debug!(
                        page = page_num + 1,
                        image_id = image_id,
                        "Null surface data pointer"
                    );
                    continue;
                }

                let data_len = (stride * height) as usize;
                let surface_data =
                    unsafe { std::slice::from_raw_parts(data_ptr, data_len).to_vec() };

                // Calculate scale factors from PDF points to pixels
                let bounds_width = area.width();
                let bounds_height = area.height();
                let scale_x = if bounds_width > 0.0 {
                    width as f64 / bounds_width
                } else {
                    1.0
                };
                let scale_y = if bounds_height > 0.0 {
                    height as f64 / bounds_height
                } else {
                    1.0
                };

                page_images.push(ImageInfo {
                    image_id,
                    area,
                    surface_data,
                    width,
                    height,
                    stride,
                    has_alpha,
                    is_grayscale,
                    scale_x,
                    scale_y,
                });
            }

            if page_images.is_empty() {
                continue;
            }

            debug!(
                page = page_num + 1,
                images = page_images.len(),
                "Found images on page"
            );

            // Group images by overlapping bounding boxes
            let groups = group_by_overlap(&page_images);

            debug!(
                page = page_num + 1,
                groups = groups.len(),
                "Grouped images into composites"
            );

            // Composite each group and save
            for (group_idx, group) in groups.iter().enumerate() {
                let composited = match composite_group(&page_images, group) {
                    Some(img) => img,
                    None => continue,
                };

                let width = composited.width();
                let height = composited.height();

                // Skip images that are too small for vision models (need at least 32x32)
                const MIN_IMAGE_SIZE: u32 = 32;
                if width < MIN_IMAGE_SIZE || height < MIN_IMAGE_SIZE {
                    debug!(
                        page = page_num + 1,
                        group = group_idx,
                        width = width,
                        height = height,
                        "Skipping small image (below {}x{} threshold)",
                        MIN_IMAGE_SIZE,
                        MIN_IMAGE_SIZE
                    );
                    continue;
                }

                // Save as WebP
                let image_id = Uuid::new_v4().to_string();
                let page_display = page_num + 1;
                let webp_filename = format!("page_{}_img_{}.webp", page_display, group_idx);
                let webp_path = images_dir.join(&webp_filename);

                let file = match File::create(&webp_path) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!(
                            page = page_display,
                            group = group_idx,
                            error = %e,
                            "Failed to create image file"
                        );
                        continue;
                    }
                };

                let encoder = WebPEncoder::new_lossless(file);
                if let Err(e) = encoder.write_image(
                    composited.as_raw(),
                    width,
                    height,
                    image::ExtendedColorType::Rgba8,
                ) {
                    warn!(
                        page = page_display,
                        group = group_idx,
                        error = %e,
                        "Failed to encode image as WebP"
                    );
                    let _ = std::fs::remove_file(&webp_path);
                    continue;
                }

                all_images.push(DocumentImage {
                    id: image_id,
                    document_id: document_id.to_string(),
                    page_number: page_display,
                    image_index: group_idx as i32,
                    internal_path: webp_path.to_string_lossy().to_string(),
                    mime_type: "image/webp".to_string(),
                    width: Some(width),
                    height: Some(height),
                    description: None,
                    created_at: now,
                });

                debug!(
                    page = page_display,
                    group = group_idx,
                    layers = group.len(),
                    width = width,
                    height = height,
                    "Extracted composited image"
                );
            }
        }

        // Sort by page number then image index for consistent ordering
        all_images.sort_by(|a, b| {
            a.page_number
                .cmp(&b.page_number)
                .then(a.image_index.cmp(&b.image_index))
        });

        info!(
            document_id = document_id,
            total_images = all_images.len(),
            "Extracted and saved images from PDF"
        );

        Ok(all_images)
    }

    /// Get the path where an image should be copied to in FVTT assets
    pub fn fvtt_image_path(
        document_title: &str,
        page_number: i32,
        description: Option<&str>,
    ) -> PathBuf {
        let sanitized_title = sanitize_filename(document_title);
        let sanitized_desc = description
            .map(|d| {
                format!(
                    "_{}",
                    sanitize_filename(&d.chars().take(30).collect::<String>())
                )
            })
            .unwrap_or_default();

        PathBuf::from(format!(
            "{}/page_{}{}.webp",
            sanitized_title, page_number, sanitized_desc
        ))
    }

    /// Extract content from EPUB
    fn extract_epub(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let mut archive =
            epub::doc::EpubDoc::new(path).map_err(|e| ProcessingError::EpubRead(e.to_string()))?;

        let mut sections = Vec::new();
        let mut chapter_index = 0;

        // Iterate through spine (reading order)
        while archive.go_next() {
            if let Some((content, _mime)) = archive.get_current_str() {
                // Strip HTML tags (basic approach)
                let text = strip_html_tags(&content);
                let text = text.trim().to_string();

                if !text.is_empty() {
                    let chapter_title = archive
                        .get_current_id()
                        .map(|id| format!("Chapter: {}", id));

                    sections.push(Section {
                        title: chapter_title,
                        content: text,
                        page_number: Some(chapter_index),
                    });
                    chapter_index += 1;
                }
            }
        }

        if sections.is_empty() {
            return Err(ServiceError::Processing(ProcessingError::EpubRead(
                "No content could be extracted from EPUB".to_string(),
            )));
        }

        debug!(chapters = sections.len(), "EPUB extracted");

        Ok(ExtractedContent { sections })
    }

    /// Extract content from Markdown
    fn extract_markdown(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let content = std::fs::read_to_string(path).map_err(ProcessingError::Io)?;

        // Parse markdown to extract sections based on headers
        let sections = self.parse_markdown_sections(&content);

        Ok(ExtractedContent { sections })
    }

    /// Parse markdown into sections based on headers
    fn parse_markdown_sections(&self, content: &str) -> Vec<Section> {
        let mut sections = Vec::new();
        let mut current_section = String::new();
        let mut current_title: Option<String> = None;

        for line in content.lines() {
            // Check for headers
            if line.starts_with('#') {
                // Save previous section
                if !current_section.trim().is_empty() {
                    sections.push(Section {
                        title: current_title.take(),
                        content: current_section.trim().to_string(),
                        page_number: None,
                    });
                    current_section = String::new();
                }

                // Extract header text
                let header_text = line.trim_start_matches('#').trim().to_string();
                current_title = Some(header_text);
            } else {
                current_section.push_str(line);
                current_section.push('\n');
            }
        }

        // Don't forget the last section
        if !current_section.trim().is_empty() {
            sections.push(Section {
                title: current_title,
                content: current_section.trim().to_string(),
                page_number: None,
            });
        }

        // If no sections were found, treat entire content as one section
        if sections.is_empty() && !content.trim().is_empty() {
            sections.push(Section {
                title: None,
                content: content.trim().to_string(),
                page_number: None,
            });
        }

        sections
    }

    /// Extract content from plain text
    fn extract_text(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let content = std::fs::read_to_string(path).map_err(ProcessingError::Io)?;

        Ok(ExtractedContent {
            sections: vec![Section {
                title: None,
                content: content.trim().to_string(),
                page_number: None,
            }],
        })
    }

    /// Create chunks from extracted content
    fn create_chunks(
        &self,
        document_id: &str,
        content: &ExtractedContent,
        access_level: AccessLevel,
        tags: &[String],
    ) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut chunk_index = 0;

        for section in &content.sections {
            let section_chunks =
                self.chunk_text(&section.content, self.chunk_size, self.chunk_overlap);

            for chunk_text in section_chunks {
                chunks.push(Chunk {
                    id: Uuid::new_v4().to_string(),
                    document_id: document_id.to_string(),
                    content: chunk_text,
                    chunk_index,
                    page_number: section.page_number,
                    section_title: section.title.clone(),
                    access_level,
                    tags: tags.to_vec(),
                    metadata: None,
                    created_at: Utc::now(),
                });
                chunk_index += 1;
            }
        }

        chunks
    }

    /// Split text into overlapping chunks
    fn chunk_text(&self, text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();

        if words.len() <= chunk_size {
            return vec![text.to_string()];
        }

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < words.len() {
            let end = (start + chunk_size).min(words.len());
            let chunk: String = words[start..end].join(" ");
            chunks.push(chunk);

            // Move start forward, accounting for overlap
            start += chunk_size - overlap;

            // Avoid infinite loop
            if start >= words.len() - overlap && end == words.len() {
                break;
            }
        }

        chunks
    }
}

/// Extracted document content
struct ExtractedContent {
    sections: Vec<Section>,
}

/// Document section
struct Section {
    title: Option<String>,
    content: String,
    page_number: Option<i32>,
}

/// Strip HTML tags from content (basic implementation)
fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut last_was_space = true;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                // Add space after closing tag to separate words
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            }
            _ if !in_tag => {
                // Handle HTML entities
                if c.is_whitespace() {
                    if !last_was_space {
                        result.push(' ');
                        last_was_space = true;
                    }
                } else {
                    result.push(c);
                    last_was_space = false;
                }
            }
            _ => {}
        }
    }

    // Decode common HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// Sanitize a string for use as a filename
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_whitespace() => '_',
            c => c,
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text() {
        let service = IngestionService {
            chunk_size: 10,
            chunk_overlap: 2,
            data_dir: PathBuf::from("/tmp"),
        };

        let text = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen";
        let chunks = service.chunk_text(text, 5, 1);

        assert!(!chunks.is_empty());
        // First chunk should have 5 words
        assert_eq!(chunks[0].split_whitespace().count(), 5);
    }

    #[test]
    fn test_strip_html() {
        let html = "<p>Hello <b>world</b>!</p>";
        let text = strip_html_tags(html);
        assert_eq!(text.trim(), "Hello world !");
    }

    #[test]
    fn test_markdown_sections() {
        let service = IngestionService {
            chunk_size: 512,
            chunk_overlap: 64,
            data_dir: PathBuf::from("/tmp"),
        };

        let markdown = r#"
# Chapter 1

This is the first chapter.

## Section 1.1

Some content here.

# Chapter 2

Another chapter.
"#;

        let sections = service.parse_markdown_sections(markdown);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].title, Some("Chapter 1".to_string()));
        assert_eq!(sections[1].title, Some("Section 1.1".to_string()));
        assert_eq!(sections[2].title, Some("Chapter 2".to_string()));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Hello World"), "Hello_World");
        assert_eq!(sanitize_filename("File/Name:Test"), "File_Name_Test");
        assert_eq!(sanitize_filename("  spaces  "), "spaces");
    }

    #[test]
    fn test_fvtt_image_path() {
        // Note: The seneschal/ prefix is added at the config level, not here
        let path = IngestionService::fvtt_image_path("Core Rulebook", 42, Some("starship map"));
        assert_eq!(
            path.to_string_lossy(),
            "Core_Rulebook/page_42_starship_map.webp"
        );

        let path_no_desc = IngestionService::fvtt_image_path("Test Doc", 1, None);
        assert_eq!(path_no_desc.to_string_lossy(), "Test_Doc/page_1.webp");
    }
}
