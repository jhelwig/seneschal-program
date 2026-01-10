//! EPUB document extraction.

use std::path::Path;

use tracing::debug;

use crate::error::{ProcessingError, ServiceError, ServiceResult};

use super::Section;

/// Extract content from an EPUB file.
pub fn extract_epub(path: &Path) -> ServiceResult<Vec<Section>> {
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

    Ok(sections)
}

/// Strip HTML tags from content (basic implementation).
pub fn strip_html_tags(html: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html() {
        let html = "<p>Hello <b>world</b>!</p>";
        let text = strip_html_tags(html);
        assert_eq!(text.trim(), "Hello world !");
    }
}
