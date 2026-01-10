//! Markdown document extraction.

use std::path::Path;

use crate::error::{ProcessingError, ServiceResult};

use super::Section;

/// Extract content from a Markdown file.
pub fn extract_markdown(path: &Path) -> ServiceResult<Vec<Section>> {
    let content = std::fs::read_to_string(path).map_err(ProcessingError::Io)?;
    Ok(parse_markdown_sections(&content))
}

/// Parse markdown into sections based on headers.
pub fn parse_markdown_sections(content: &str) -> Vec<Section> {
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

/// Extract content from a plain text file.
pub fn extract_text(path: &Path) -> ServiceResult<Vec<Section>> {
    let content = std::fs::read_to_string(path).map_err(ProcessingError::Io)?;

    Ok(vec![Section {
        title: None,
        content: content.trim().to_string(),
        page_number: None,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_sections() {
        let markdown = r#"
# Chapter 1

This is the first chapter.

## Section 1.1

Some content here.

# Chapter 2

Another chapter.
"#;

        let sections = parse_markdown_sections(markdown);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].title, Some("Chapter 1".to_string()));
        assert_eq!(sections[1].title, Some("Section 1.1".to_string()));
        assert_eq!(sections[2].title, Some("Chapter 2".to_string()));
    }
}
