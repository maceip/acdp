//! DSPy-RS signatures for MCP tool prediction and optimization

// LlmResult may be used in future - keeping import for now
use dspy_rs::Signature;
use serde::{Deserialize, Serialize};
use serde_json;

/// Signature for predicting which MCP tool will be called
#[Signature]
pub struct ToolPredictionSignature {
    /// Predict which MCP tool will be called based on context

    /// The full MCP context including request, available tools, and history
    #[input]
    pub mcp_context: String,

    /// Predicted tool name and confidence (JSON)
    #[output]
    pub tool_prediction: String,
}

/// Tool prediction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPrediction {
    pub tool_name: String,
    pub confidence: f32,
    pub reasoning: String,
    pub parameters: serde_json::Value,
}

/// Signature for optimizing prompts based on execution traces
#[Signature]
pub struct PromptOptimizationSignature {
    /// Optimize prompts based on execution traces

    /// Collection of execution traces with success/failure data
    #[input]
    pub execution_traces: String,

    /// Optimized prompt with improved instructions (JSON)
    #[output]
    pub optimized_prompt: String,
}

/// Optimized prompt result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizedPrompt {
    pub prompt: String,
    pub expected_improvement: f32,
    pub reasoning: String,
}

/// Signature for semantic routing decisions
#[Signature]
pub struct SemanticRoutingSignature {
    /// Make routing decision based on context

    /// Context for routing decision including request patterns and performance data
    #[input]
    pub routing_context: String,

    /// Routing decision with confidence (JSON)
    #[output]
    pub routing_decision: String,
}

/// Routing decision result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub transport: String,
    pub endpoint: String,
    pub confidence: f32,
    pub reasoning: String,
}

/// Signature for error analysis and suggestions
#[Signature]
pub struct ErrorAnalysisSignature {
    /// Analyze error patterns and provide suggestions

    /// Patterns of errors from execution traces
    #[input]
    pub error_patterns: String,

    /// Suggestions for improving error handling (JSON)
    #[output]
    pub improvement_suggestions: String,
}

/// Improvement suggestions result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementSuggestions {
    pub suggestions: Vec<String>,
    pub priority: String,
    pub expected_impact: f32,
}

/// Signature for performance optimization
#[Signature]
pub struct PerformanceOptimizationSignature {
    /// Provide performance optimization recommendations

    /// Current performance metrics and trends
    #[input]
    pub performance_metrics: String,

    /// Recommendations for performance improvements (JSON)
    #[output]
    pub optimization_recommendations: String,
}

/// Performance optimization recommendations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationRecommendations {
    pub recommendations: Vec<PerformanceRecommendation>,
    pub expected_improvement: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceRecommendation {
    pub category: String,
    pub action: String,
    pub impact: String,
    pub effort: String,
}

// Helper functions for output schema
pub fn tool_prediction_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "tool_name": {"type": "string"},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "reasoning": {"type": "string"},
            "parameters": {"type": "object"}
        },
        "required": ["tool_name", "confidence", "reasoning", "parameters"]
    })
}

pub fn prompt_optimization_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "prompt": {"type": "string"},
            "expected_improvement": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "reasoning": {"type": "string"}
        },
        "required": ["prompt", "expected_improvement", "reasoning"]
    })
}

pub fn routing_decision_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "transport": {"type": "string"},
            "endpoint": {"type": "string"},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "reasoning": {"type": "string"}
        },
        "required": ["transport", "endpoint", "confidence", "reasoning"]
    })
}
