use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use crate::config::OllamaConfig;
use crate::error::{OllamaError, ServiceError, ServiceResult};
use crate::tools::{OllamaToolDefinition, ToolCall, get_ollama_tool_definitions};

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

    /// Generate a streaming chat completion
    pub async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> ServiceResult<mpsc::Receiver<StreamEvent>> {
        let url = format!("{}/api/chat", self.config.base_url);

        let model = request
            .model
            .unwrap_or_else(|| self.config.default_model.clone());

        let ollama_request = OllamaChatRequest {
            model: model.clone(),
            messages: request.messages,
            tools: if request.enable_tools {
                Some(get_ollama_tool_definitions())
            } else {
                None
            },
            stream: true,
            options: Some(OllamaOptions {
                temperature: Some(request.temperature.unwrap_or(self.config.temperature)),
                num_ctx: request.num_ctx,
            }),
        };

        // Log the tool names being sent to Ollama
        if let Some(ref tools) = ollama_request.tools {
            let tool_names: Vec<&str> = tools.iter().map(|t| t.function.name.as_str()).collect();
            debug!(
                model = %model,
                tools = ?tool_names,
                "Sending chat request to Ollama with tools"
            );
        } else {
            debug!(model = %model, "Sending chat request to Ollama without tools");
        }

        // Log the messages being sent (at trace level for verbose debugging)
        for (i, msg) in ollama_request.messages.iter().enumerate() {
            debug!(
                index = i,
                role = %msg.role,
                content_length = msg.content.len(),
                content_preview = %msg.content.chars().take(200).collect::<String>(),
                "Message {} to Ollama", i
            );
        }

        // Log the full request as JSON at trace level
        if tracing::enabled!(tracing::Level::TRACE)
            && let Ok(json) = serde_json::to_string_pretty(&ollama_request)
        {
            tracing::trace!(request = %json, "Full Ollama request");
        }

        let response = self
            .client
            .post(&url)
            .json(&ollama_request)
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
                return Err(ServiceError::Ollama(OllamaError::ModelNotFound { model }));
            }

            return Err(ServiceError::Ollama(OllamaError::Generation {
                status,
                message,
            }));
        }

        let (tx, rx) = mpsc::channel(100);

        // Spawn a task to process the stream
        let mut stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut tool_call_count = 0usize;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        buffer.push_str(&String::from_utf8_lossy(&chunk));

                        // Process complete JSON lines
                        while let Some(pos) = buffer.find('\n') {
                            let line = buffer[..pos].trim().to_string();
                            buffer = buffer[pos + 1..].to_string();

                            if line.is_empty() {
                                continue;
                            }

                            match serde_json::from_str::<OllamaChatResponse>(&line) {
                                Ok(response) => {
                                    // Handle content
                                    if !response.message.content.is_empty()
                                        && tx
                                            .send(StreamEvent::Content(response.message.content))
                                            .await
                                            .is_err()
                                    {
                                        return;
                                    }

                                    // Handle tool calls
                                    if let Some(calls) = response.message.tool_calls {
                                        for (i, call) in calls.into_iter().enumerate() {
                                            let tool_call = ToolCall {
                                                id: format!("tc_{}", tool_call_count + i),
                                                tool: call.function.name.clone(),
                                                args: call.function.arguments.clone(),
                                            };

                                            // Log tool call details for debugging
                                            debug!(
                                                tool_call_id = %tool_call.id,
                                                tool_name = %call.function.name,
                                                tool_args = %call.function.arguments,
                                                "LLM requested tool call"
                                            );

                                            tool_call_count += 1;
                                            if tx
                                                .send(StreamEvent::ToolCall(tool_call))
                                                .await
                                                .is_err()
                                            {
                                                return;
                                            }
                                        }
                                    }

                                    // Handle completion
                                    if response.done {
                                        let _ = tx
                                            .send(StreamEvent::Done {
                                                prompt_eval_count: response.prompt_eval_count,
                                                eval_count: response.eval_count,
                                            })
                                            .await;
                                        return;
                                    }
                                }
                                Err(e) => {
                                    error!(error = %e, line = %line, "Failed to parse Ollama response");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                        return;
                    }
                }
            }
        });

        Ok(rx)
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
            tools: None,
            stream: false,
            options: Some(OllamaOptions {
                temperature: Some(0.3), // Lower temperature for more consistent descriptions
                num_ctx: None,
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

/// Chat request
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub num_ctx: Option<u32>,
    pub enable_tools: bool,
}

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OllamaToolCall>>,
    /// Base64-encoded images for vision models
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            tool_calls: None,
            images: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            images: None,
        }
    }

    /// Create a user message with an image for vision models
    pub fn user_with_image(content: impl Into<String>, image_base64: String) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            images: Some(vec![image_base64]),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
            images: None,
        }
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            images: None,
        }
    }
}

/// Stream events
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Content(String),
    ToolCall(ToolCall),
    Done {
        prompt_eval_count: Option<u32>,
        eval_count: Option<u32>,
    },
    Error(String),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaToolDefinition>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaToolCall {
    pub function: OllamaFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaFunctionCall {
    pub name: String,
    pub arguments: serde_json::Value,
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
