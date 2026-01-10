//! PDF document processing.
//!
//! This module handles PDF document processing including:
//! - Text extraction with watermark filtering and bookmark-based sections
//! - Image extraction with layer compositing and transformation handling

pub mod images;
pub mod text;

use pdfium_render::prelude::*;

use crate::error::ProcessingError;

// Re-export commonly used items
pub use images::extract_pdf_images;
pub use text::{extract_pdf, extract_pdf_page_text};

/// Create a new Pdfium instance (dynamically linked).
///
/// Searches for libpdfium in:
/// 1. Current directory (./libpdfium.so)
/// 2. vendor/pdfium/lib/ (downloaded by `just download-pdfium`)
/// 3. System library paths
pub fn create_pdfium() -> Result<Pdfium, ProcessingError> {
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
