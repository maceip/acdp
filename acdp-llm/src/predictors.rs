//! Production DSPy-powered predictors for MCP tool routing

use crate::database::PredictionsDatabase;
use crate::dspy_predictor::DSpyToolPredictor;
use crate::dspy_signatures::ToolPrediction;
#[allow(unused_imports)]
use crate::error::LlmError as _;
use crate::error::LlmResult;
use crate::lm_provider::LiteRTLM;
use serde_json::json;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tracing::{debug, warn};

/// Result produced by the predictor, including the stored database row id.
#[derive(Debug, Clone)]
pub struct PredictionOutcome {
    pub prediction: ToolPrediction,
    pub record_id: String,
    pub context_hash: String,
}

/// Production DSPy-powered predictor that records every prediction to SQLite
#[derive(Clone)]
pub struct ToolPredictor {
    predictions_db: Arc<PredictionsDatabase>,
    semantic_engine: Option<Arc<SemanticEngine>>,
}

impl ToolPredictor {
    pub fn new(predictions_db: Arc<PredictionsDatabase>) -> Self {
        Self {
            predictions_db,
            semantic_engine: None,
        }
    }

    pub fn with_semantic(
        predictions_db: Arc<PredictionsDatabase>,
        semantic_engine: Option<Arc<SemanticEngine>>,
    ) -> Self {
        Self {
            predictions_db,
            semantic_engine,
        }
    }

    /// Access to the underlying predictions database (for updating accuracy later).
    pub fn predictions_db(&self) -> Arc<PredictionsDatabase> {
        self.predictions_db.clone()
    }

    /// Produce a prediction for the supplied MCP context and persist it.
    pub async fn predict_tool(&self, mcp_context: &str) -> LlmResult<PredictionOutcome> {
        if let Some(engine) = &self.semantic_engine {
            debug!("Using DSPy-powered semantic engine for prediction");
            match engine.predict(mcp_context).await {
                Ok(prediction) => {
                    return self
                        .persist_prediction(
                            mcp_context,
                            prediction.tool_name,
                            prediction.confidence,
                            prediction.reasoning,
                        )
                        .await;
                }
                Err(err) => {
                    warn!("DSPy prediction failed: {}. Using heuristic fallback.", err);
                    // Use heuristic fallback instead of failing
                    return self.heuristic_predict(mcp_context).await;
                }
            }
        }

        // No semantic engine - use heuristics
        warn!("No DSPy semantic engine configured, using heuristic fallback");
        self.heuristic_predict(mcp_context).await
    }

    /// Simple heuristic prediction when LLM fails or is unavailable
    async fn heuristic_predict(&self, mcp_context: &str) -> LlmResult<PredictionOutcome> {
        let context_lower = mcp_context.to_lowercase();

        // Try to extract specific tool name from tools/call requests
        if context_lower.contains("tools/call") || context_lower.contains("tools.call") {
            // Try to parse JSON and extract the tool name
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(mcp_context) {
                if let Some(params) = json.get("params") {
                    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                        return self
                            .persist_prediction(
                                mcp_context,
                                name.to_string(),
                                0.7,
                                format!("Extracted tool name '{}' from tools/call params", name),
                            )
                            .await;
                    }
                }
            }
            // Fallback if parsing failed
            return self
                .persist_prediction(
                    mcp_context,
                    "tools.call".to_string(),
                    0.5,
                    "Matched 'tools/call' pattern but couldn't extract tool name".to_string(),
                )
                .await;
        }

        // Simple keyword matching for other methods
        let (tool_name, confidence, reasoning) = if context_lower.contains("tools/list")
            || context_lower.contains("list") && context_lower.contains("tool")
        {
            ("tools.list", 0.8, "Matched 'tools/list' pattern")
        } else if context_lower.contains("resources/list")
            || context_lower.contains("list") && context_lower.contains("resource")
        {
            ("resources.list", 0.8, "Matched 'resources/list' pattern")
        } else if context_lower.contains("resources/read")
            || context_lower.contains("read") && context_lower.contains("resource")
        {
            ("resources.read", 0.7, "Matched 'resources/read' pattern")
        } else if context_lower.contains("prompts/list")
            || context_lower.contains("list") && context_lower.contains("prompt")
        {
            ("prompts.list", 0.8, "Matched 'prompts/list' pattern")
        } else if context_lower.contains("prompts/get")
            || context_lower.contains("get") && context_lower.contains("prompt")
        {
            ("prompts.get", 0.7, "Matched 'prompts/get' pattern")
        } else {
            // Default to tools.list as safest option
            (
                "tools.list",
                0.5,
                "No clear match - defaulting to tools.list",
            )
        };

        self.persist_prediction(
            mcp_context,
            tool_name.to_string(),
            confidence as f32,
            reasoning.to_string(),
        )
        .await
    }

    async fn persist_prediction(
        &self,
        mcp_context: &str,
        tool_name: String,
        confidence: f32,
        reasoning: String,
    ) -> LlmResult<PredictionOutcome> {
        let prediction = ToolPrediction {
            tool_name: tool_name.clone(),
            confidence,
            reasoning,
            parameters: json!({}),
        };

        let context_hash = hash_context(mcp_context);
        let record_id = self
            .predictions_db
            .record_prediction(
                &context_hash,
                &tool_name,
                confidence as f64,
                serde_json::to_value(&prediction)?,
            )
            .await?;

        Ok(PredictionOutcome {
            prediction,
            record_id,
            context_hash,
        })
    }

    /// Update the stored record once the actual tool is known.
    pub async fn update_prediction_result(
        &self,
        record_id: &str,
        actual_tool: &str,
    ) -> LlmResult<()> {
        self.predictions_db
            .update_prediction_result(record_id, actual_tool)
            .await
    }
}

fn hash_context(context: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    context.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Semantic engine wrapping DSPy predictor
#[derive(Clone)]
pub struct SemanticEngine {
    dspy_predictor: Arc<DSpyToolPredictor>,
}

impl SemanticEngine {
    /// Create a new semantic engine from a LiteRT LM
    pub async fn new(litert_lm: Arc<LiteRTLM>) -> LlmResult<Self> {
        let dspy_predictor = Arc::new(DSpyToolPredictor::new_with_litert(litert_lm).await?);
        Ok(Self { dspy_predictor })
    }

    /// Predict tool using DSPy
    pub async fn predict(&self, context: &str) -> LlmResult<ToolPrediction> {
        self.dspy_predictor.predict(context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::LlmDatabase;
    use tempfile::TempDir;

    #[tokio::test]
    async fn predictor_requires_semantic_engine() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("predictions.sqlite");
        let db_url = format!("sqlite://{}", db_path.to_string_lossy());

        let database = LlmDatabase::new(&db_url).await.unwrap();
        let predictions_db = Arc::new(database.predictions.clone());

        let predictor = ToolPredictor::new(predictions_db.clone());

        let context = "Session: abc\nCurrent Method: tools/list\n";
        let result = predictor.predict_tool(context).await;

        // Should error without semantic engine
        assert!(result.is_err());
    }
}
