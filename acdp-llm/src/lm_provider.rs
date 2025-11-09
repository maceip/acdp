//! DSPy-RS Language Model Provider implementation using LiteRT-LM
//!
//! This module provides a custom LM implementation for dspy-rs that uses LiteRT as the backend.
//! It implements the required traits to integrate with DSPy's Predict and ChainOfThought modules.

use crate::error::{LlmError, LlmResult};
use crate::litert_wrapper::{LiteRTBackend, LiteRTEngine, ResponseFormat};
#[allow(unused_imports)]
use crate::litert_wrapper::{LiteRTConversation as _, LiteRTSession as _};
use anyhow::Result;
use dspy_rs::{Chat, LMResponse, LmUsage, Message};
use std::sync::Arc;
#[allow(unused_imports)]
use tokio::sync::Mutex as _;

/// LiteRT-based Language Model for DSPy-RS
///
/// This wraps LiteRT's engine to provide DSPy-compatible
/// chat completion capabilities. Creates a new session for each request.
#[derive(Clone)]
pub struct LiteRTLM {
    engine: Arc<LiteRTEngine>,
    #[allow(dead_code)]
    temperature: f32,
    #[allow(dead_code)]
    max_tokens: usize,
}

impl LiteRTLM {
    /// Create a new LiteRT Language Model
    pub async fn new(config: LiteRTConfig) -> LlmResult<Self> {
        let engine = LiteRTEngine::new(&config.model_path, config.backend)?;

        Ok(Self {
            engine: Arc::new(engine),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        })
    }

    /// Execute a chat completion (DSPy-compatible interface)
    pub async fn call(&self, messages: Chat) -> Result<LMResponse> {
        // Extract system instruction if present
        let system_instruction = messages.messages.iter().find_map(|msg| match msg {
            Message::System { content } => Some(content.clone()),
            _ => None,
        });

        // Create a new conversation for this request with system instruction
        let conversation = if let Some(ref system) = system_instruction {
            self.engine
                .create_conversation_with_system(Some(system))
                .map_err(|e| anyhow::anyhow!("Failed to create conversation: {}", e))?
        } else {
            self.engine
                .create_conversation()
                .map_err(|e| anyhow::anyhow!("Failed to create conversation: {}", e))?
        };

        // Send all non-system messages to build conversation history
        let mut response_text = String::new();
        for msg in &messages.messages {
            match msg {
                Message::System { .. } => {
                    // Skip - already handled above
                }
                Message::User { content } => {
                    // Send user message and get response
                    response_text = conversation
                        .send_message("user", content)
                        .map_err(|e| anyhow::anyhow!("LiteRT generation error: {}", e))?;
                }
                Message::Assistant { content } => {
                    // Send assistant message (for multi-turn context)
                    conversation
                        .send_message("model", content)
                        .map_err(|e| anyhow::anyhow!("LiteRT error: {}", e))?;
                }
            }
        }

        // Create DSPy-compatible response
        let output = Message::assistant(&response_text);

        let mut full_chat = messages.clone();
        full_chat.push_message(output.clone());

        Ok(LMResponse {
            output,
            usage: LmUsage::default(), // LiteRT doesn't expose token counts currently
            chat: full_chat,
        })
    }
}

/// Builder for LiteRT Language Model
pub struct LiteRTBuilder {
    model_path: Option<String>,
    backend: LiteRTBackend,
    temperature: f32,
    max_tokens: usize,
}

impl LiteRTBuilder {
    pub fn new() -> Self {
        Self {
            model_path: None,
            backend: LiteRTBackend::Cpu,
            temperature: 0.7,
            max_tokens: 1000,
        }
    }

    pub fn model_path(mut self, path: impl Into<String>) -> Self {
        self.model_path = Some(path.into());
        self
    }

    pub fn backend(mut self, backend: LiteRTBackend) -> Self {
        self.backend = backend;
        self
    }

    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = temp;
        self
    }

    pub fn max_tokens(mut self, tokens: usize) -> Self {
        self.max_tokens = tokens;
        self
    }

    pub async fn build(self) -> LlmResult<LiteRTLM> {
        let model_path = self
            .model_path
            .ok_or_else(|| LlmError::ConfigError("Model path is required".to_string()))?;

        let config = LiteRTConfig {
            model_path,
            backend: self.backend,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            response_format: ResponseFormat::Text,
        };

        LiteRTLM::new(config).await
    }
}

impl Default for LiteRTBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for LiteRT Language Model
#[derive(Debug, Clone)]
pub struct LiteRTConfig {
    pub model_path: String,
    pub backend: LiteRTBackend,
    pub temperature: f32,
    pub max_tokens: usize,
    pub response_format: ResponseFormat,
}

// Re-export types from litert_wrapper
pub use crate::litert_wrapper::{LLMResponse, ResponseMetadata, Tool, ToolCall};
