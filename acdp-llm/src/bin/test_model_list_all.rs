use acdp_llm::config::AppConfig;
use acdp_llm::LlmService;

#[tokio::main]
async fn main() {
    println!("Testing model listing (with available models)...\n");

    let config = AppConfig::default();

    println!("Creating LlmService...");
    match LlmService::new(config).await {
        Ok(service) => {
            println!("✓ LlmService created successfully\n");

            println!("Listing available models (cached + registry)...");
            match service.list_available_models().await {
                Ok(models) => {
                    println!("✓ Found {} total models:\n", models.len());
                    for model in models {
                        let status = if model.is_cached {
                            "✓ Cached"
                        } else {
                            "  Available"
                        };
                        println!(
                            "  {} {} ({} MB)",
                            status,
                            model.name,
                            model.size_bytes / 1_000_000
                        );
                    }
                }
                Err(e) => {
                    eprintln!("✗ Failed to list available models: {}", e);
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
