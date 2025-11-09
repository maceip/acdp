//! DSPy-based predictors for MCP tool routing
//!
//! This module provides production DSPy predictors using the LiteRT backend.

use crate::dspy_signatures::ToolPrediction;
use crate::error::{LlmError, LlmResult};
use crate::lm_provider::LiteRTLM;
use dspy_rs::{Chat, Message};
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

/// DSPy-powered tool predictor
pub struct DSpyToolPredictor {
    litert_lm: Arc<LiteRTLM>,
}

impl DSpyToolPredictor {
    /// Create predictor using LiteRT backend
    pub async fn new_with_litert(litert_lm: Arc<LiteRTLM>) -> LlmResult<Self> {
        Ok(Self { litert_lm })
    }

    /// Predict tool from MCP context using DSPy-style prompting with LiteRT
    pub async fn predict(&self, mcp_context: &str) -> LlmResult<ToolPrediction> {
        self.predict_with_litert(&self.litert_lm, mcp_context).await
    }

    /// Predict using LiteRT with DSPy-style prompting
    async fn predict_with_litert(
        &self,
        litert: &Arc<LiteRTLM>,
        mcp_context: &str,
    ) -> LlmResult<ToolPrediction> {
        // Build simple prompt optimized for Gemma
        let mut chat = Chat::new(vec![]);

        // Combine system instructions and user query in a single user message
        // (Gemma doesn't handle system messages well)
        let combined_prompt = format!(
            "You are a tool prediction system. Analyze the MCP request below and predict which tool will be called.\n\n\
             Available MCP methods: tools/list, tools/call, resources/list, resources/read, prompts/list, prompts/get\n\n\
             IMPORTANT: If the method is 'tools/call', look at the 'name' parameter to find the ACTUAL tool being called.\n\
             In that case, predict the specific tool name (like 'web_search', 'calculator', etc.), NOT just 'tools.call'.\n\n\
             Respond with ONLY a JSON object in this format:\n\
             {{\"tool_name\": \"<tool>\", \"confidence\": <0.0-1.0>, \"reasoning\": \"<brief explanation>\", \"parameters\": {{}}}}\n\n\
             Examples:\n\
             - If method='tools/list', predict tool_name='tools.list'\n\
             - If method='tools/call' with name='web_search', predict tool_name='web_search'\n\
             - If method='resources/read', predict tool_name='resources.read'\n\n\
             MCP Request:\n{}\n\n\
             JSON Response:",
            mcp_context
        );

        chat.push_message(Message::user(&combined_prompt));

        // Call LiteRT
        debug!("Calling LiteRT for tool prediction");
        let response = litert
            .call(chat)
            .await
            .map_err(|e| LlmError::PredictionError(format!("LiteRT call failed: {}", e)))?;

        // Parse response as JSON
        let response_text = match &response.output {
            Message::Assistant { content } => content,
            _ => {
                return Err(LlmError::PredictionError(
                    "Unexpected message type from LiteRT".to_string(),
                ))
            }
        };

        debug!("LiteRT response: {}", response_text);

        // Check if response is empty
        if response_text.trim().is_empty() {
            return Err(LlmError::PredictionError(
                "LiteRT returned empty response - model may not be loaded or prompt is invalid"
                    .to_string(),
            ));
        }

        // Extract JSON from response (handling markdown code blocks)
        let json_text = self.extract_json(response_text)?;

        debug!("Extracted JSON: {}", json_text);

        // Parse the JSON
        let parsed: Value = serde_json::from_str(&json_text).map_err(|e| {
            LlmError::PredictionError(format!(
                "Failed to parse LiteRT response as JSON: {}. Raw response: {}. Extracted: {}",
                e, response_text, json_text
            ))
        })?;

        // Convert to ToolPrediction
        let prediction: ToolPrediction = serde_json::from_value(parsed.clone()).map_err(|e| {
            LlmError::PredictionError(format!(
                "Response doesn't match ToolPrediction schema: {}",
                e
            ))
        })?;

        Ok(prediction)
    }

    /// Extract JSON from response text (handles markdown code blocks)
    fn extract_json(&self, text: &str) -> LlmResult<String> {
        let trimmed = text.trim();

        // Check for markdown JSON code block
        if let Some(start) = trimmed.find("```json") {
            if let Some(end) = trimmed[start + 7..].find("```") {
                let json_text = &trimmed[start + 7..start + 7 + end];
                return Ok(json_text.trim().to_string());
            }
        }

        // Check for generic code block
        if let Some(start) = trimmed.find("```") {
            if let Some(end) = trimmed[start + 3..].find("```") {
                let json_text = &trimmed[start + 3..start + 3 + end];
                return Ok(json_text.trim().to_string());
            }
        }

        // Try to find JSON object directly
        if let Some(start) = trimmed.find('{') {
            if let Some(end) = trimmed.rfind('}') {
                if end > start {
                    return Ok(trimmed[start..=end].to_string());
                }
            }
        }

        // Return as-is and let JSON parser handle it
        Ok(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_markdown() {
        let text = r#"Sure! Here's the prediction:
```json
{"tool_name": "tools.list", "confidence": 0.9}
```
"#;
        let json_text = extract_json_helper(text).unwrap();
        assert!(json_text.contains("tool_name"));
    }

    #[test]
    fn test_extract_json_direct() {
        let text = r#"{"tool_name": "tools.list", "confidence": 0.9}"#;
        let json_text = extract_json_helper(text).unwrap();
        assert_eq!(json_text, text);
    }

    fn extract_json_helper(text: &str) -> Result<String, String> {
        let trimmed = text.trim();

        // Check for markdown JSON code block
        if let Some(start) = trimmed.find("```json") {
            if let Some(end) = trimmed[start + 7..].find("```") {
                let json_text = &trimmed[start + 7..start + 7 + end];
                return Ok(json_text.trim().to_string());
            }
        }

        // Check for generic code block
        if let Some(start) = trimmed.find("```") {
            if let Some(end) = trimmed[start + 3..].find("```") {
                let json_text = &trimmed[start + 3..start + 3 + end];
                return Ok(json_text.trim().to_string());
            }
        }

        // Try to find JSON object directly
        if let Some(start) = trimmed.find('{') {
            if let Some(end) = trimmed.rfind('}') {
                if end > start {
                    return Ok(trimmed[start..=end].to_string());
                }
            }
        }

        // Return as-is and let JSON parser handle it
        Ok(trimmed.to_string())
    }
}
