//! Document ingestion and processing.
//!
//! This module handles processing documents (PDF, EPUB, Markdown, text) into
//! searchable chunks with embeddings. It also extracts images from PDFs for
//! use in Foundry VTT.

pub mod assets;
pub mod epub;
pub mod hash;
pub mod markdown;
pub mod pdf;

use std::path::{Path, PathBuf};

use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::config::{EmbeddingsConfig, ImageExtractionConfig};
use crate::db::{Chunk, DocumentImage};
use crate::error::{ProcessingError, ServiceError, ServiceResult};
use crate::tools::AccessLevel;

/// Extracted document content
pub struct ExtractedContent {
    pub sections: Vec<Section>,
}

/// Document section
pub struct Section {
    pub title: Option<String>,
    pub content: String,
    pub page_number: Option<i32>,
}

/// Document ingestion service
pub struct IngestionService {
    chunk_size: usize,
    chunk_overlap: usize,
    data_dir: PathBuf,
    image_extraction_config: ImageExtractionConfig,
}

impl IngestionService {
    pub fn new(
        embeddings_config: &EmbeddingsConfig,
        image_extraction_config: ImageExtractionConfig,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            chunk_size: embeddings_config.chunk_size,
            chunk_overlap: embeddings_config.chunk_overlap,
            data_dir,
            image_extraction_config,
        }
    }

    /// Process a document with a pre-generated document ID, returning only chunks.
    ///
    /// Used for async document processing where the Document record is created first.
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
            "pdf" => self.extract_pdf_content(path)?,
            "epub" => self.extract_epub_content(path)?,
            "md" | "markdown" => self.extract_markdown_content(path)?,
            "txt" | "text" => self.extract_text_content(path)?,
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

    /// Extract content from PDF.
    fn extract_pdf_content(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let sections = pdf::extract_pdf(path)?;
        Ok(ExtractedContent { sections })
    }

    /// Extract images from a PDF document and save them as WebP files.
    ///
    /// Returns a list of DocumentImage records (without descriptions - those are added separately).
    pub fn extract_pdf_images(
        &self,
        path: &Path,
        document_id: &str,
    ) -> ServiceResult<Vec<DocumentImage>> {
        let images_dir = self.data_dir.join("images").join(document_id);
        pdf::extract_pdf_images(
            path,
            document_id,
            &images_dir,
            &self.image_extraction_config,
        )
    }

    /// Extract text from specific pages of a PDF.
    ///
    /// Returns a HashMap of page_number (1-indexed) -> page_text.
    pub fn extract_pdf_page_text(
        &self,
        path: &Path,
        page_numbers: &[i32],
    ) -> ServiceResult<std::collections::HashMap<i32, String>> {
        pdf::extract_pdf_page_text(path, page_numbers)
    }

    /// Get the path where an image should be copied to in FVTT assets.
    ///
    /// Returns a path relative to the FVTT assets directory (e.g., `seneschal/Doc_Title/page_1.webp`).
    /// When returning this path to the LLM for use in FVTT documents, prepend `assets/` to make
    /// it a valid FVTT reference path (e.g., `assets/seneschal/Doc_Title/page_1.webp`).
    pub fn fvtt_image_path(
        document_title: &str,
        page_number: i32,
        description: Option<&str>,
    ) -> PathBuf {
        assets::fvtt_image_path(document_title, page_number, description)
    }

    /// Extract content from EPUB.
    fn extract_epub_content(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let sections = epub::extract_epub(path)?;
        Ok(ExtractedContent { sections })
    }

    /// Extract content from Markdown.
    fn extract_markdown_content(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let sections = markdown::extract_markdown(path)?;
        Ok(ExtractedContent { sections })
    }

    /// Extract content from plain text.
    fn extract_text_content(&self, path: &Path) -> ServiceResult<ExtractedContent> {
        let sections = markdown::extract_text(path)?;
        Ok(ExtractedContent { sections })
    }

    /// Create chunks from extracted content.
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

    /// Split text into overlapping chunks.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ImageExtractionConfig;

    #[test]
    fn test_chunk_text() {
        let service = IngestionService {
            chunk_size: 10,
            chunk_overlap: 2,
            data_dir: PathBuf::from("/tmp"),
            image_extraction_config: ImageExtractionConfig::default(),
        };

        let text = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen";
        let chunks = service.chunk_text(text, 5, 1);

        assert!(!chunks.is_empty());
        // First chunk should have 5 words
        assert_eq!(chunks[0].split_whitespace().count(), 5);
    }
}
