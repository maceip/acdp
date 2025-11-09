//! Test semantic routing with real LiteRT/DSPy integration
//!
//! Run with: cargo run --bin test_semantic_routing

use acdp_llm::config::{AppConfig, LlmConfig, ModelConfig};
use acdp_llm::database::LlmDatabase;
use acdp_llm::lm_provider::LiteRTBuilder;
use acdp_llm::predictors::{SemanticEngine, ToolPredictor};
use std::sync::Arc;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("mcp_llm=debug,test_semantic_routing=info")
        .init();

    info!("Testing semantic routing with real LiteRT/DSPy integration");

    // Create config with semantic routing enabled
    let config = AppConfig {
        model: ModelConfig::default(),
        llm: LlmConfig {
            backend: "cpu".to_string(),
            temperature: 0.7,
            max_tokens: 100,
            database_path: "/tmp/test_semantic.sqlite".to_string(),
            routing_mode: "semantic".to_string(),
            semantic_routing: true,
            semantic_model_path: Some("~/.litert-lm/models/gemma3-1b-it-int4.litertlm".to_string()),
        },
        mcp_server: Default::default(),
    };

    // Initialize database
    let db_path = config.database_path()?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let database_url = format!("sqlite://{}", db_path.to_string_lossy());
    let database = Arc::new(LlmDatabase::new(&database_url).await?);

    // Build LiteRT provider
    info!("Building LiteRT provider with Gemma model");
    let model_path = config
        .llm
        .semantic_model_path()?
        .ok_or("No model path configured")?;

    let backend = config.backend()?;

    let builder = LiteRTBuilder::new()
        .model_path(model_path.to_string_lossy().into_owned())
        .backend(backend)
        .temperature(config.llm.temperature)
        .max_tokens(config.llm.max_tokens);

    let litert_lm = match builder.build().await {
        Ok(provider) => Arc::new(provider),
        Err(e) => {
            warn!("Failed to initialize LiteRT provider: {}", e);
            if e.to_string().contains("LiteRT is not available") {
                info!("LiteRT runtime not available - this is expected if MCP_ENABLE_LITERT is not set");
                info!("To enable LiteRT, set: export MCP_ENABLE_LITERT=1");
                return Ok(());
            }
            return Err(e.into());
        }
    };

    info!("Successfully initialized LiteRT provider");

    // Create semantic engine
    let semantic_engine = Arc::new(SemanticEngine::new(litert_lm).await?);
    info!("Successfully created semantic engine");

    // Create tool predictor with semantic engine
    let predictions_db = Arc::new(database.predictions.clone());
    let tool_predictor = Arc::new(ToolPredictor::with_semantic(
        predictions_db.clone(),
        Some(semantic_engine),
    ));

    // Test predictions
    info!("Testing tool predictions with various MCP contexts");

    let test_cases = vec![
        (
            "Session: test-123\nMethod: tools/list\nRequest: List all available tools",
            "tools.list"
        ),
        (
            "Session: test-456\nMethod: tools/call\nTool: calculator\nParameters: {\"operation\": \"add\", \"a\": 5, \"b\": 3}",
            "tools.call"
        ),
        (
            "Session: test-789\nMethod: resources/read\nResource: file://config.json\nAction: Reading configuration file",
            "resources.read"
        ),
        (
            "Session: test-abc\nMethod: prompts/get\nPrompt: greeting\nDescription: Get the greeting prompt template",
            "prompts.get"
        ),
    ];

    let mut correct_predictions = 0;
    let mut total_predictions = 0;

    for (context, expected_tool) in test_cases {
        info!("Testing context: {}", context);

        match tool_predictor.predict_tool(context).await {
            Ok(outcome) => {
                total_predictions += 1;
                info!(
                    "Predicted: {} (confidence: {:.2})",
                    outcome.prediction.tool_name, outcome.prediction.confidence
                );
                info!("Reasoning: {}", outcome.prediction.reasoning);

                if outcome.prediction.tool_name == expected_tool {
                    correct_predictions += 1;
                    info!("✓ Correct prediction!");
                } else {
                    warn!(
                        "✗ Expected: {}, Got: {}",
                        expected_tool, outcome.prediction.tool_name
                    );
                }

                // Update the prediction result for accuracy tracking
                tool_predictor
                    .update_prediction_result(&outcome.record_id, expected_tool)
                    .await?;
            }
            Err(e) => {
                warn!("Prediction failed: {}", e);
            }
        }

        println!(); // Add spacing between test cases
    }

    // Calculate accuracy
    let accuracy = if total_predictions > 0 {
        (correct_predictions as f64 / total_predictions as f64) * 100.0
    } else {
        0.0
    };

    info!("=== Results ===");
    info!(
        "Correct predictions: {}/{}",
        correct_predictions, total_predictions
    );
    info!("Accuracy: {:.1}%", accuracy);

    if accuracy >= 80.0 {
        info!("✓ Target accuracy of 80% achieved!");
    } else {
        warn!("✗ Target accuracy of 80% not yet achieved");
    }

    // Query database for all predictions
    info!("\n=== Database Query ===");
    let recent_predictions = predictions_db.get_recent_predictions(10).await?;

    for pred in recent_predictions {
        info!(
            "Tool: {}, Confidence: {:.2}, Correct: {:?}",
            pred.predicted_tool, pred.confidence, pred.correct
        );
    }

    Ok(())
}
