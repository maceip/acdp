//! Direct test of LiteRT wrapper without DSPy
//!
//! Run with: DYLD_LIBRARY_PATH=/Users/rpm/LiteRT-LM/bazel-bin/rust_api MCP_ENABLE_LITERT=1 cargo run --bin test_litert_direct

use acdp_llm::litert_wrapper::{LiteRTBackend, LiteRTEngine};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Testing LiteRT directly with simple prompts");

    // Create engine with Gemma model
    let model_path = "/Users/rpm/.litert-lm/models/gemma3-1b-it-int4.litertlm";
    info!("Loading model from: {}", model_path);

    let engine = LiteRTEngine::new(model_path, LiteRTBackend::Cpu)?;
    info!("Engine created successfully");

    // Create a conversation
    let conversation = std::sync::Arc::new(engine.create_conversation()?);
    info!("Conversation created successfully");

    // Test with a very simple prompt using Gemma format
    let prompts = vec![
        // Simple completion
        "<start_of_turn>user\nWhat is 2+2?<end_of_turn>\n<start_of_turn>model\n",

        // Tool prediction style prompt
        "<start_of_turn>user\nYou are a tool selector. Choose one tool from: [list_tools, call_tool, read_resource]. Which tool should be used to list available tools? Answer with just the tool name.<end_of_turn>\n<start_of_turn>model\n",

        // MCP context prompt (short)
        "<start_of_turn>user\nMCP request: tools/list. Which tool: tools.list or tools.call? Reply with one word.<end_of_turn>\n<start_of_turn>model\n",
    ];

    for (i, prompt) in prompts.iter().enumerate() {
        info!("\n=== Test {} ===", i + 1);
        info!("Prompt: {}", prompt.replace('\n', "\\n"));
        info!("Generating response... (this may take 10-30 seconds on CPU)");

        let start = std::time::Instant::now();

        // Set a timeout using tokio
        let prompt_clone = prompt.to_string();
        let conversation_clone = conversation.clone();
        let result = tokio::task::spawn_blocking(move || {
            conversation_clone.send_user_message(&prompt_clone)
        });

        // Wait for up to 60 seconds
        match tokio::time::timeout(std::time::Duration::from_secs(60), result).await {
            Ok(Ok(Ok(response))) => {
                let elapsed = start.elapsed();
                info!("Response ({}ms): {}", elapsed.as_millis(), response.trim());
            }
            Ok(Ok(Err(e))) => {
                info!("Generation error: {}", e);
            }
            Ok(Err(e)) => {
                info!("Task error: {}", e);
            }
            Err(_) => {
                info!("Timeout: Generation took more than 60 seconds");
                info!("This likely means the model is too large for CPU inference");
                info!("Consider using a smaller model or GPU backend");
            }
        }
    }

    info!("\n=== Test Complete ===");
    info!("If generations were slow or timed out, consider:");
    info!("1. Using a smaller model (e.g., gemma-2b instead of 3b)");
    info!("2. Enabling GPU backend if available");
    info!("3. Using quantized models (int4/int8)");

    Ok(())
}
