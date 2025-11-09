//! DSPy-RS signatures for MCP tool prediction and optimization

use dspy_rs::Signature;
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::error::LlmResult;

/// Signature for predicting which MCP tool will be called
#[derive(Signature, Debug, Clone)]
#[dspy(signature = "mcp_context -> tool_prediction")]
pub struct ToolPredictionSignature {
    /// The full MCP context including request, available tools, and history
    #[input]
    pub mcp_context: String,
    
    /// Predicted tool name and confidence
    #[output]
    pub tool_prediction: ToolPrediction,
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
#[derive(Signature, Debug, Clone)]
#[dspy(signature = "execution_traces -> optimized_prompt")]
pub struct PromptOptimizationSignature {
    /// Collection of execution traces with success/failure data
    #[input]
    pub execution_traces: String,
    
    /// Optimized prompt with improved instructions
    #[output]
    pub optimized_prompt: OptimizedPrompt,
}

/// Optimized prompt result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizedPrompt {
    pub prompt: String,
    pub expected_improvement: f32,
    pub reasoning: String,
}

/// Signature for semantic routing decisions
#[derive(Signature, Debug, Clone)]
#[dspy(signature = "routing_context -> routing_decision")]
pub struct SemanticRoutingSignature {
    /// Context for routing decision including request patterns and performance data
    #[input]
    pub routing_context: String,
    
    /// Routing decision with confidence
    #[output]
    pub routing_decision: RoutingDecision,
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
#[derive(Signature, Debug, Clone)]
#[dspy(signature = "error_patterns -> improvement_suggestions")]
pub struct ErrorAnalysisSignature {
    /// Patterns of errors from execution traces
    #[input]
    pub error_patterns: String,
    
    /// Suggestions for improving error handling
    #[output]
    pub improvement_suggestions: ImprovementSuggestions,
}

/// Improvement suggestions result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementSuggestions {
    pub suggestions: Vec<String>,
    pub priority: String,
    pub expected_impact: f32,
}

/// Signature for performance optimization
#[derive(Signature, Debug, Clone)]
#[dspy(signature = "performance_metrics -> optimization_recommendations")]
pub struct PerformanceOptimizationSignature {
    /// Current performance metrics and trends
    #[input]
    pub performance_metrics: String,
    
    /// Recommendations for performance improvements
    #[output]
    pub optimization_recommendations: OptimizationRecommendations,
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

impl ToolPredictionSignature {
    /// Create signature for tool prediction
    pub fn new() -> Self {
        Self {
            mcp_context: String::new(),
            tool_prediction: ToolPrediction {
                tool_name: String::new(),
                confidence: 0.0,
                reasoning: String::new(),
                parameters: serde_json::Value::Null,
            },
        }
    }
    
    /// Get JSON schema for structured output
    pub fn output_schema() -> serde_json::Value {
        json!({
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
}

impl PromptOptimizationSignature {
    pub fn new() -> Self {
        Self {
            execution_traces: String::new(),
            optimized_prompt: OptimizedPrompt {
                prompt: String::new(),
                expected_improvement: 0.0,
                reasoning: String::new(),
            },
        }
    }
    
    pub fn output_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {"type": "string"},
                "expected_improvement": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                "reasoning": {"type": "string"}
            },
            "required": ["prompt", "expected_improvement", "reasoning"]
        })
    }
}

impl SemanticRoutingSignature {
    pub fn new() -> Self {
        Self {
            routing_context: String::new(),
            routing_decision: RoutingDecision {
                transport: String::new(),
                endpoint: String::new(),
                confidence: 0.0,
                reasoning: String::new(),
            },
        }
    }
    
    pub fn output_schema() -> serde_json::Value {
        json!({
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
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tool_prediction_signature() {
        let signature = ToolPredictionSignature::new();
        let schema = ToolPredictionSignature::output_schema();
        
        assert!(schema.is_object());
        assert!(schema.get("properties").is_some());
    }
}