use acdp_llm::{LiteRTBackend, LiteRTEngine};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get model path from command line
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <model.litertlm>", args[0]);
        eprintln!("\nExample:");
        eprintln!("  DYLD_LIBRARY_PATH=~/LiteRT-LM/bazel-bin/rust_api cargo run --example chat ~/gemma3-1b.litertlm");
        std::process::exit(1);
    }

    let model_path = &args[1];

    println!("╔════════════════════════════════════════╗");
    println!("║   LiteRT-LM Interactive Chat (Rust)   ║");
    println!("╚════════════════════════════════════════╝");
    println!();
    println!("Loading model: {}", model_path);

    // Create engine
    let engine = LiteRTEngine::new(model_path, LiteRTBackend::Cpu)?;
    println!("✓ Engine loaded");

    // Create session (conversation)
    let session = engine.create_session()?;
    println!("✓ Session created");
    println!();
    println!("Type your messages below. Enter 'quit' or 'exit' to stop.");
    println!("════════════════════════════════════════");
    println!();

    // Interactive chat loop
    loop {
        print!("You: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
            println!("\n👋 Goodbye!");
            break;
        }

        // Generate response
        print!("🤖 Assistant: ");
        io::stdout().flush()?;

        match session.generate(input) {
            Ok(response) => {
                println!("{}\n", response);
            }
            Err(e) => {
                eprintln!("❌ Error: {}\n", e);
            }
        }
    }

    Ok(())
}
