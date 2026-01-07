use chrono::Utc;
use image::ImageEncoder;
use image::codecs::webp::WebPEncoder;
use pdfium_render::prelude::*;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::config::EmbeddingsConfig;
use crate::db::{Chunk, Document, DocumentImage};
use crate::error::{ProcessingError, ServiceError, ServiceResult};
use crate::tools::AccessLevel;

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

    /// Process a document and return chunks
    pub fn process_document(
        &self,
        path: &Path,
        title: &str,
        access_level: AccessLevel,
        tags: Vec<String>,
    ) -> ServiceResult<(Document, Vec<Chunk>)> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        info!(path = %path.display(), format = %extension, "Processing document");

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

        // Calculate file hash
        let file_hash = self.calculate_file_hash(path)?;

        // Create document record
        let doc_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let document = Document {
            id: doc_id.clone(),
            title: title.to_string(),
            file_path: Some(path.to_string_lossy().to_string()),
            file_hash: Some(file_hash),
            access_level,
            tags: tags.clone(),
            metadata: None,
            created_at: now,
            updated_at: now,
        };

        // Create chunks
        let chunks = self.create_chunks(&doc_id, &content, access_level, &tags);

        info!(
            doc_id = %doc_id,
            chunks = chunks.len(),
            "Document processed successfully"
        );

        Ok((document, chunks))
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
    pub fn extract_pdf_images(
        &self,
        path: &Path,
        document_id: &str,
    ) -> ServiceResult<Vec<DocumentImage>> {
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

        // Create images directory for this document
        let images_dir = self.data_dir.join("images").join(document_id);
        std::fs::create_dir_all(&images_dir).map_err(ProcessingError::Io)?;

        let mut images = Vec::new();
        let now = Utc::now();

        for (page_index, page) in document.pages().iter().enumerate() {
            let page_num = page_index as i32 + 1;
            let mut image_index: i32 = 0;

            for object in page.objects().iter() {
                if let Some(image_object) = object.as_image_object() {
                    match image_object.get_raw_image() {
                        Ok(dynamic_image) => {
                            let image_id = Uuid::new_v4().to_string();
                            let filename = format!("page_{}_img_{}.webp", page_num, image_index);
                            let file_path = images_dir.join(&filename);

                            // Save as WebP for FVTT optimization
                            let file = match File::create(&file_path) {
                                Ok(f) => f,
                                Err(e) => {
                                    warn!(
                                        page = page_num,
                                        image = image_index,
                                        error = %e,
                                        "Failed to create image file"
                                    );
                                    continue;
                                }
                            };

                            let width = dynamic_image.width();
                            let height = dynamic_image.height();
                            let rgba = dynamic_image.to_rgba8();

                            // Use lossless WebP encoding
                            let encoder = WebPEncoder::new_lossless(file);
                            if let Err(e) = encoder.write_image(
                                rgba.as_raw(),
                                width,
                                height,
                                image::ExtendedColorType::Rgba8,
                            ) {
                                warn!(
                                    page = page_num,
                                    image = image_index,
                                    error = %e,
                                    "Failed to encode image as WebP"
                                );
                                // Clean up failed file
                                let _ = std::fs::remove_file(&file_path);
                                continue;
                            }

                            images.push(DocumentImage {
                                id: image_id,
                                document_id: document_id.to_string(),
                                page_number: page_num,
                                image_index,
                                internal_path: file_path.to_string_lossy().to_string(),
                                mime_type: "image/webp".to_string(),
                                width: Some(width),
                                height: Some(height),
                                description: None, // Added separately via vision model
                                created_at: now,
                            });

                            image_index += 1;
                        }
                        Err(e) => {
                            warn!(
                                page = page_num,
                                image = image_index,
                                error = ?e,
                                "Failed to extract image"
                            );
                        }
                    }
                }
            }
        }

        info!(
            document_id = document_id,
            total_images = images.len(),
            "Extracted and saved images from PDF"
        );

        Ok(images)
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
            "seneschal/{}/page_{}{}.webp",
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

    /// Calculate file hash
    fn calculate_file_hash(&self, path: &Path) -> ServiceResult<String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut file = std::fs::File::open(path).map_err(ProcessingError::Io)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(ProcessingError::Io)?;

        let mut hasher = DefaultHasher::new();
        buffer.hash(&mut hasher);
        Ok(format!("{:x}", hasher.finish()))
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
        let path = IngestionService::fvtt_image_path("Core Rulebook", 42, Some("starship map"));
        assert_eq!(
            path.to_string_lossy(),
            "seneschal/Core_Rulebook/page_42_starship_map.webp"
        );

        let path_no_desc = IngestionService::fvtt_image_path("Test Doc", 1, None);
        assert_eq!(
            path_no_desc.to_string_lossy(),
            "seneschal/Test_Doc/page_1.webp"
        );
    }
}
