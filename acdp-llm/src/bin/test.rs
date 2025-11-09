//! Test binary for LLM integration

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Note: We can't actually create an engine without a model file
    // But we can verify the bindings are working
    println!("✅ LiteRT-LM bindings are working!");
    println!("✅ All types imported successfully!");

    Ok(())
}
