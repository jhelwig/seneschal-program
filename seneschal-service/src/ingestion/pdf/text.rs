//! PDF text extraction with watermark filtering and bookmark support.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use pdfium_render::prelude::*;
use tracing::{debug, info, warn};

use crate::error::{ProcessingError, ServiceError, ServiceResult};

use crate::ingestion::Section;

/// Extract text content from a PDF with watermark filtering and bookmark-based section titles.
pub fn extract_pdf(path: &Path) -> ServiceResult<Vec<Section>> {
    let pdfium = super::create_pdfium()?;

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

    let page_count = document.pages().len();
    info!(pages = page_count, "Processing PDF pages");

    // 1. Extract bookmarks for section context
    let bookmarks = extract_pdf_bookmarks(path);
    if !bookmarks.is_empty() {
        info!(bookmark_count = bookmarks.len(), "Found PDF bookmarks");
    }

    // 2. First pass: extract all page text (raw)
    let mut raw_pages: Vec<(i32, String)> = Vec::new();
    for (page_index, page) in document.pages().iter().enumerate() {
        let page_num = page_index as i32 + 1;

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

        let page_text = text.all().trim().to_string();
        if !page_text.is_empty() {
            raw_pages.push((page_num, page_text));
        }
    }

    // 3. Detect and filter watermarks
    let watermarks = detect_watermarks(&raw_pages);
    if !watermarks.is_empty() {
        info!(
            watermark_count = watermarks.len(),
            "Detected watermark patterns to filter"
        );
    }

    // 4. Second pass: create sections with clean text and section titles
    let mut sections = Vec::new();
    let mut current_section: Option<String> = None;

    for (page_num, text) in raw_pages {
        // Update section if this page starts a new one
        if let Some(section_title) = bookmarks.get(&page_num) {
            current_section = Some(section_title.clone());
        }

        // Remove watermarks
        let clean_text = if watermarks.is_empty() {
            text
        } else {
            remove_watermarks(&text, &watermarks)
        };

        if !clean_text.trim().is_empty() {
            sections.push(Section {
                title: current_section.clone(),
                content: clean_text,
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
        sections_with_titles = sections.iter().filter(|s| s.title.is_some()).count(),
        "PDF text extracted with watermark filtering and section context"
    );

    Ok(sections)
}

/// Extract text from specific pages of a PDF.
///
/// Returns a HashMap of page_number (1-indexed) -> page_text.
pub fn extract_pdf_page_text(
    path: &Path,
    page_numbers: &[i32],
) -> ServiceResult<HashMap<i32, String>> {
    if page_numbers.is_empty() {
        return Ok(HashMap::new());
    }

    let pdfium = Pdfium::default();
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

    let page_count = document.pages().len() as i32;
    let mut result = HashMap::new();

    for &page_num in page_numbers {
        // Skip invalid page numbers
        if page_num < 1 || page_num > page_count {
            warn!(
                page = page_num,
                total_pages = page_count,
                "Requested page number out of range"
            );
            continue;
        }

        let page_index = (page_num - 1) as u16;
        if let Ok(page) = document.pages().get(page_index)
            && let Ok(text) = page.text()
        {
            let page_text = text.all().trim().to_string();
            if !page_text.is_empty() {
                result.insert(page_num, page_text);
            }
        }
    }

    debug!(
        requested_pages = page_numbers.len(),
        extracted_pages = result.len(),
        "Extracted page text from PDF"
    );

    Ok(result)
}

/// Detect lines that appear on many pages (likely watermarks).
///
/// Returns a set of lines that appear on >50% of pages.
fn detect_watermarks(pages: &[(i32, String)]) -> HashSet<String> {
    let total_pages = pages.len();
    if total_pages < 2 {
        return HashSet::new();
    }

    let mut line_counts: HashMap<String, usize> = HashMap::new();

    // Count occurrences of each unique line across all pages
    for (_, text) in pages {
        // Use a set to count each line only once per page
        let unique_lines: HashSet<&str> = text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect();

        for line in unique_lines {
            *line_counts.entry(line.to_string()).or_insert(0) += 1;
        }
    }

    // Lines appearing on >50% of pages are considered watermarks
    let threshold = total_pages / 2;
    let watermarks: HashSet<String> = line_counts
        .into_iter()
        .filter(|(_, count)| *count > threshold)
        .map(|(line, _)| line)
        .collect();

    if !watermarks.is_empty() {
        debug!(
            watermark_count = watermarks.len(),
            total_pages = total_pages,
            "Detected watermark lines"
        );
    }

    watermarks
}

/// Remove watermark lines from page content.
fn remove_watermarks(text: &str, watermarks: &HashSet<String>) -> String {
    text.lines()
        .filter(|line| !watermarks.contains(line.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract bookmark structure from PDF using qpdf.
///
/// Returns a map of page numbers to section titles.
fn extract_pdf_bookmarks(path: &Path) -> BTreeMap<i32, String> {
    let mut page_sections: BTreeMap<i32, String> = BTreeMap::new();

    let output = Command::new("qpdf")
        .args(["--json", path.to_str().unwrap_or("")])
        .output();

    if let Ok(output) = output
        && output.status.success()
        && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        && let Some(outlines) = json.get("outlines")
    {
        parse_outlines_recursive(outlines, &mut page_sections, &mut Vec::new());
    }

    if !page_sections.is_empty() {
        debug!(
            bookmark_count = page_sections.len(),
            "Extracted PDF bookmarks"
        );
    }

    page_sections
}

/// Recursively parse qpdf outline structure to build page -> section title map.
fn parse_outlines_recursive(
    outlines: &serde_json::Value,
    page_sections: &mut BTreeMap<i32, String>,
    title_stack: &mut Vec<String>,
) {
    if let Some(array) = outlines.as_array() {
        for outline in array {
            if let Some(title) = outline.get("title").and_then(|t| t.as_str()) {
                title_stack.push(title.to_string());

                // Extract page number from dest
                if let Some(dest) = outline.get("dest")
                    && let Some(page_num) = extract_page_from_dest(dest)
                {
                    // Build hierarchical title like "Adventure 1 > NPCs"
                    let full_title = title_stack.join(" > ");
                    page_sections.insert(page_num, full_title);
                }

                // Process children
                if let Some(kids) = outline.get("kids") {
                    parse_outlines_recursive(kids, page_sections, title_stack);
                }

                title_stack.pop();
            }
        }
    }
}

/// Extract page number from qpdf dest field.
fn extract_page_from_dest(dest: &serde_json::Value) -> Option<i32> {
    // dest can be: "page:N" string, an array, or other formats
    if let Some(s) = dest.as_str() {
        // Try to parse "page:N" format
        if let Some(page_str) = s.strip_prefix("page:")
            && let Ok(page) = page_str.parse::<i32>()
        {
            return Some(page + 1); // qpdf uses 0-indexed pages
        }
    } else if let Some(arr) = dest.as_array() {
        // Array format: first element might be page reference
        if let Some(first) = arr.first() {
            if let Some(s) = first.as_str()
                && let Some(page_str) = s.strip_prefix("page:")
                && let Ok(page) = page_str.parse::<i32>()
            {
                return Some(page + 1);
            } else if let Some(n) = first.as_i64() {
                return Some(n as i32 + 1);
            }
        }
    }
    None
}
