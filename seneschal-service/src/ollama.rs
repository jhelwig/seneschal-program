use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::warn;

use crate::config::OllamaConfig;
use crate::error::{OllamaError, ServiceError, ServiceResult};

/// Ollama API client
pub struct OllamaClient {
    client: Client,
    config: OllamaConfig,
}

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(config: OllamaConfig) -> ServiceResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .map_err(|e| {
                ServiceError::Ollama(OllamaError::Connection {
                    url: config.base_url.clone(),
                    source: e,
                })
            })?;

        Ok(Self { client, config })
    }

    /// Check if Ollama is available
    pub async fn health_check(&self) -> ServiceResult<bool> {
        let url = format!("{}/api/tags", self.config.base_url);

        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(e) => {
                warn!(error = %e, "Ollama health check failed");
                Ok(false)
            }
        }
    }

    /// List available models
    pub async fn list_models(&self) -> ServiceResult<Vec<ModelInfo>> {
        let url = format!("{}/api/tags", self.config.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| OllamaError::Connection {
                url: url.clone(),
                source: e,
            })?;

        if !response.status().is_success() {
            return Err(ServiceError::Ollama(OllamaError::Generation {
                status: response.status().as_u16(),
                message: "Failed to list models".to_string(),
            }));
        }

        let tags: TagsResponse =
            response
                .json()
                .await
                .map_err(|e| OllamaError::InvalidResponse {
                    source: serde_json::Error::io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    )),
                })?;

        let mut models = Vec::new();

        for model in tags.models {
            // Get detailed model info
            let show_url = format!("{}/api/show", self.config.base_url);
            let show_response = self
                .client
                .post(&show_url)
                .json(&serde_json::json!({ "name": &model.name }))
                .send()
                .await;

            let (context_length, parameter_size, quantization) = match show_response {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<ShowResponse>().await {
                        Ok(show) => {
                            // Extract context length from model_info
                            let context_length = show.model_info.as_ref().and_then(|info| {
                                info.get("general.architecture")
                                    .and_then(|arch| arch.as_str())
                                    .and_then(|arch| {
                                        let key = format!("{}.context_length", arch);
                                        info.get(&key)
                                    })
                                    .and_then(|v| v.as_u64())
                            });

                            (
                                context_length,
                                show.details.parameter_size,
                                show.details.quantization_level,
                            )
                        }
                        Err(_) => (None, None, None),
                    }
                }
                _ => (None, None, None),
            };

            models.push(ModelInfo {
                name: model.name,
                context_length,
                parameter_size,
                quantization,
            });
        }

        Ok(models)
    }

    /// Generate a non-streaming response (for simple tasks like image captioning)
    pub async fn generate_simple(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> ServiceResult<String> {
        let url = format!("{}/api/chat", self.config.base_url);

        let request = OllamaChatRequest {
            model: model.to_string(),
            messages,
            stream: false,
            options: Some(OllamaOptions {
                temperature: Some(0.3), // Lower temperature for more consistent descriptions
            }),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| OllamaError::Connection {
                url: url.clone(),
                source: e,
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();

            if message.contains("model") && message.contains("not found") {
                return Err(ServiceError::Ollama(OllamaError::ModelNotFound {
                    model: model.to_string(),
                }));
            }

            return Err(ServiceError::Ollama(OllamaError::Generation {
                status,
                message,
            }));
        }

        let chat_response: OllamaChatResponse =
            response
                .json()
                .await
                .map_err(|e| OllamaError::InvalidResponse {
                    source: serde_json::Error::io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    )),
                })?;

        Ok(chat_response.message.content)
    }
}

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Base64-encoded images for vision models
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

impl ChatMessage {
    /// Create a user message with an image for vision models
    pub fn user_with_image(content: impl Into<String>, image_base64: String) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            images: Some(vec![image_base64]),
        }
    }
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub context_length: Option<u64>,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
}

// Internal Ollama API types

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Debug, Deserialize)]
struct TagModel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ShowResponse {
    #[serde(default)]
    details: ModelDetails,
    #[serde(default)]
    model_info: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Default, Deserialize)]
struct ModelDetails {
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    quantization_level: Option<String>,
}
