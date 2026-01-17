//! System prompt building and message formatting for LLM interactions.

use crate::db::MessageRole;
use crate::ollama::{ChatMessage, OllamaFunctionCall, OllamaToolCall};

use super::SeneschalService;
use super::state::{ActiveRequest, UserContext};

impl SeneschalService {
    /// Build Ollama messages from conversation
    pub(crate) fn build_ollama_messages(
        &self,
        request: &ActiveRequest,
        user_context: &UserContext,
    ) -> Vec<ChatMessage> {
        use tracing::debug;

        let mut messages = vec![ChatMessage::system(self.build_system_prompt(user_context))];

        // Log all source messages before building
        debug!(
            source_message_count = request.messages.len(),
            "Building Ollama messages from conversation"
        );
        for (i, msg) in request.messages.iter().enumerate() {
            let tool_call_count = msg.tool_calls.as_ref().map(|tc| tc.len()).unwrap_or(0);
            debug!(
                index = i,
                role = ?msg.role,
                content_length = msg.content.len(),
                content_preview = %msg.content.chars().take(100).collect::<String>(),
                tool_call_count = tool_call_count,
                "Source message {}", i
            );
        }

        for msg in &request.messages {
            let chat_msg = match msg.role {
                MessageRole::User => ChatMessage::user(&msg.content),
                MessageRole::Assistant => {
                    if let Some(tool_calls) = &msg.tool_calls {
                        ChatMessage::assistant_with_tool_calls(
                            &msg.content,
                            tool_calls
                                .iter()
                                .map(|tc| OllamaToolCall {
                                    function: OllamaFunctionCall {
                                        name: tc.tool.clone(),
                                        arguments: tc.args.clone(),
                                    },
                                })
                                .collect(),
                        )
                    } else {
                        ChatMessage::assistant(&msg.content)
                    }
                }
                MessageRole::System => ChatMessage::system(&msg.content),
                MessageRole::Tool => ChatMessage::tool(&msg.content),
            };
            messages.push(chat_msg);
        }

        messages
    }

    /// Build system prompt from template
    pub(crate) fn build_system_prompt(&self, user_context: &UserContext) -> String {
        const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("../../prompts/system.txt");

        let is_gm = user_context.is_gm();
        let role_name = if is_gm { "Game Master" } else { "Player" };
        let character = user_context.character_id.as_deref().unwrap_or("None");

        SYSTEM_PROMPT_TEMPLATE
            .replace("{user_name}", &user_context.user_name)
            .replace("{role_name}", role_name)
            .replace("{character}", character)
    }
}
