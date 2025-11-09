//! # MCP LLM Integration
//!
//! [mcp-llm](cci:7://file:///Users/rpm/assist-mcp/mcp-llm:0:0-0:0) provides intelligent LLM integration for assist-mcp, featuring:
//! - LiteRT-LM Rust API via dynamic linking
//! - DSPy-RS integration for structured predictions
//! - SQLite-backed routing and optimization
//! - GEPA prompt optimization
//! - Real-time tool prediction and routing

pub mod config;
pub mod conversation_context;
pub mod database;
pub mod dspy_predictor;
pub mod dspy_signatures;
pub mod error;
pub mod gepa_optimizer;
pub mod interceptor;
pub mod litert_wrapper;
pub mod lm_provider;
pub mod metrics;
pub mod model_management;
pub mod predictors;
pub mod routing_modes;
pub mod service;
pub mod session_management;
pub mod streaming;

// Re-export main types
pub use conversation_context::{ConversationAnalyzer, ConversationContextBuilder};
pub use database::{
    GepaDatabase, GepaOptimizationRecord, MetricsDatabase, PredictionsDatabase,
    RoutingRulesDatabase,
};
pub use dspy_predictor::DSpyToolPredictor;
pub use dspy_signatures::{OptimizedPrompt, RoutingDecision, ToolPrediction};
pub use error::{LlmError, LlmResult};
pub use litert_wrapper::{LiteRTBackend, LiteRTEngine, LiteRTSession, ResponseFormat};
pub use lm_provider::{LiteRTConfig, LiteRTLM};
pub use session_management::{SessionManager, SessionPrediction, SessionPredictionContext};
// Note: Signature types are private due to #[Signature] macro from dspy-rs
// They can still be used internally within the crate
pub use config::{AppConfig, LlmConfig, ModelConfig};
pub use gepa_optimizer::GEPAOptimizer;
pub use interceptor::LlmInterceptor;
pub use model_management::{
    DownloadProgress, ModelInfo, ModelManager, ModelStatus, ModelStatusUpdate,
};
pub use predictors::{PredictionOutcome, SemanticEngine, ToolPredictor};
pub use routing_modes::{RoutingConfig, RoutingMode};
pub use service::{
    GenerationEvent, GenerationHandle, GenerationMetrics, GenerationRequest,
    GenerationRequestMetadata, LlmEvent, LlmService,
};
pub use streaming::{generate_streaming, TokenStream};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bindings_available() {
        // Test that bindings are generated and available
    }
}
