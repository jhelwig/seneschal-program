use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::config::EmbeddingsConfig;
use crate::db::{Chunk, Database};
use crate::error::{EmbeddingError, OllamaError, ServiceError, ServiceResult};
use crate::i18n::I18n;
use crate::tools::{SearchFilters, TagMatch};

/// Search service for RAG functionality using Ollama embeddings
pub struct SearchService {
    db: Arc<Database>,
    client: Client,
    ollama_url: String,
    embedding_model: String,
}

impl SearchService {
    /// Create a new search service
    pub async fn new(
        db: Arc<Database>,
        config: &EmbeddingsConfig,
        ollama_base_url: &str,
    ) -> ServiceResult<Self> {
        info!(model = %config.model, "Initializing embedding service using Ollama");

        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| {
                ServiceError::Embedding(EmbeddingError::ModelInit {
                    message: e.to_string(),
                })
            })?;

        let service = Self {
            db,
            client,
            ollama_url: ollama_base_url.to_string(),
            embedding_model: config.model.clone(),
        };

        // Try a test embedding to verify the model is available
        match service.embed_text("test").await {
            Ok(_) => info!("Embedding model verified successfully"),
            Err(e) => {
                warn!(error = %e, "Embedding model verification failed - embeddings may not work")
            }
        }

        Ok(service)
    }

    /// Generate embedding for text using Ollama
    pub async fn embed_text(&self, text: &str) -> ServiceResult<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.ollama_url);

        let request = OllamaEmbeddingRequest {
            model: self.embedding_model.clone(),
            prompt: text.to_string(),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                ServiceError::Ollama(OllamaError::Connection {
                    url: url.clone(),
                    source: e,
                })
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();

            if message.contains("model")
                && (message.contains("not found") || message.contains("does not exist"))
            {
                return Err(ServiceError::Ollama(OllamaError::ModelNotFound {
                    model: self.embedding_model.clone(),
                }));
            }

            return Err(ServiceError::Ollama(OllamaError::Generation {
                status,
                message,
            }));
        }

        let embedding_response: OllamaEmbeddingResponse = response.json().await.map_err(|e| {
            ServiceError::Embedding(EmbeddingError::Generation {
                message: e.to_string(),
            })
        })?;

        Ok(embedding_response.embedding)
    }

    /// Search for relevant chunks
    pub async fn search(
        &self,
        query: &str,
        user_role: u8,
        limit: usize,
        filters: Option<SearchFilters>,
    ) -> ServiceResult<Vec<SearchResult>> {
        debug!(query = %query, user_role = user_role, limit = limit, "Searching documents");

        // Generate query embedding
        let query_embedding = self.embed_text(query).await?;

        // Extract filter parameters
        let (tags, tag_match_all) = filters
            .map(|f| (Some(f.tags), f.tags_match == TagMatch::All))
            .unwrap_or((None, false));

        // Search database
        let results = self.db.search_chunks(
            &query_embedding,
            user_role,
            limit,
            tags.as_deref(),
            tag_match_all,
        )?;

        debug!(results = results.len(), "Search completed");

        Ok(results
            .into_iter()
            .map(|(chunk, similarity)| SearchResult { chunk, similarity })
            .collect())
    }

    /// Index multiple chunks with progress callback
    /// The callback receives (current_progress, total) after each chunk is embedded
    pub async fn index_chunks_with_progress<F>(
        &self,
        chunks: &[Chunk],
        mut on_progress: F,
    ) -> ServiceResult<()>
    where
        F: FnMut(usize, usize),
    {
        if chunks.is_empty() {
            return Ok(());
        }

        let total = chunks.len();
        info!(total = total, "Starting embedding generation");

        // Generate embeddings for all chunks
        for (i, chunk) in chunks.iter().enumerate() {
            let embedding = self.embed_text(&chunk.content).await?;
            self.db.insert_embedding(&chunk.id, &embedding)?;

            let progress = i + 1;

            // Call the progress callback
            on_progress(progress, total);

            // Log progress every 10 chunks or at completion
            if progress % 10 == 0 || progress == total {
                info!(
                    progress = progress,
                    total = total,
                    percent = (progress * 100) / total,
                    "Generating embeddings"
                );
            }
        }

        info!(chunks = total, "Embedding generation complete");

        Ok(())
    }
}

/// Ollama embedding request
#[derive(Debug, Serialize)]
struct OllamaEmbeddingRequest {
    model: String,
    prompt: String,
}

/// Ollama embedding response
#[derive(Debug, Deserialize)]
struct OllamaEmbeddingResponse {
    embedding: Vec<f32>,
}

/// Search result
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub chunk: Chunk,
    pub similarity: f32,
}

impl SearchResult {
    /// Format for LLM context
    pub fn format_for_context(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref title) = self.chunk.section_title {
            parts.push(format!("Section: {}", title));
        }

        if let Some(page) = self.chunk.page_number {
            parts.push(format!("Page: {}", page));
        }

        parts.push(format!("Relevance: {:.2}", self.similarity));
        parts.push(format!("Content:\n{}", self.chunk.content));

        parts.join("\n")
    }
}

/// Format search results for LLM consumption
pub fn format_search_results_for_llm(
    results: &[SearchResult],
    i18n: &I18n,
    locale: &str,
) -> String {
    if results.is_empty() {
        return i18n.get(locale, "search-no-results", None);
    }

    let header = i18n.format(
        locale,
        "search-results-count",
        &[("count", &results.len().to_string())],
    );
    let mut output = format!("{}:\n\n", header);

    for (i, result) in results.iter().enumerate() {
        output.push_str(&format!("--- Result {} ---\n", i + 1));
        output.push_str(&result.format_for_context());
        output.push_str("\n\n");
    }

    output
}
