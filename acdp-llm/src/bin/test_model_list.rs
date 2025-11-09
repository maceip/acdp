use acdp_llm::config::AppConfig;
use acdp_llm::LlmService;

#[tokio::main]
async fn main() {
    println!("Testing model listing...\n");

    let config = AppConfig::default();

    println!("Creating LlmService...");
    match LlmService::new(config).await {
        Ok(service) => {
            println!("✓ LlmService created successfully\n");

            println!("Listing cached models...");
            match service.list_cached_models().await {
                Ok(models) => {
                    println!("✓ Found {} cached models:\n", models.len());
                    for model in models {
                        println!("  Name: {}", model.name);
                        println!("  Size: {} bytes", model.size_bytes);
                        println!("  Path: {:?}", model.path);
                        println!("  Cached: {}\n", model.is_cached);
                    }
                }
                Err(e) => {
                    eprintln!("✗ Failed to list cached models: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Failed to create LlmService: {}", e);
            std::process::exit(1);
        }
    }
}
