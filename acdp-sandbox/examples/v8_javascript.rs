//! V8 JavaScript runtime example

#[cfg(feature = "v8")]
use acdp_sandbox::{ExecutionRequest, SandboxService, V8Runtime};

#[cfg(feature = "v8")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let service = SandboxService::new(V8Runtime::new());
    println!("=== V8 JavaScript Sandbox ===\n");
    println!("Runtime: {}\n", service.runtime_name());

    // Example 1: Simple expression
    println!("Example 1: Simple expression");
    let request = ExecutionRequest::new("1 + 1");
    execute_and_print(&service, request).await?;

    // Example 2: Function call
    println!("\nExample 2: Function");
    let request = ExecutionRequest::new(
        r#"
        function greet(name) {
            return "Hello, " + name + "!";
        }
        greet("V8 Sandbox")
        "#,
    );
    execute_and_print(&service, request).await?;

    // Example 3: Array operations
    println!("\nExample 3: Array operations");
    let request = ExecutionRequest::new(
        r#"
        const numbers = [1, 2, 3, 4, 5];
        numbers.map(x => x * 2).join(", ")
        "#,
    );
    execute_and_print(&service, request).await?;

    // Example 4: JSON
    println!("\nExample 4: JSON");
    let request = ExecutionRequest::new(
        r#"
        JSON.stringify({
            name: "V8 Runtime",
            version: "latest",
            features: ["fast", "secure", "isolated"]
        })
        "#,
    );
    execute_and_print(&service, request).await?;

    // Example 5: Error handling
    println!("\nExample 5: Error (should show in stderr)");
    let request = ExecutionRequest::new("throw new Error('Test error')");
    execute_and_print(&service, request).await?;

    Ok(())
}

#[cfg(feature = "v8")]
async fn execute_and_print(
    service: &SandboxService,
    request: ExecutionRequest,
) -> anyhow::Result<()> {
    let mut stream = service.execute(request).await?;

    // Stream stdout
    let stdout_handle = tokio::spawn(async move {
        while let Some(line) = stream.stdout.recv().await {
            print!("  {}", String::from_utf8_lossy(&line));
        }
    });

    // Stream stderr
    let stderr_handle = tokio::spawn(async move {
        while let Some(line) = stream.stderr.recv().await {
            eprint!("  Error: {}", String::from_utf8_lossy(&line));
        }
    });

    // Wait for result
    let result = stream.result.await?;

    // Wait for output streams to finish
    stdout_handle.await?;
    stderr_handle.await?;

    // Print result
    if result.exit_code != 0 {
        println!(
            "  [exit_code={} duration={}ms]",
            result.exit_code, result.duration_ms
        );
    }

    Ok(())
}

#[cfg(not(feature = "v8"))]
fn main() {
    eprintln!("This example requires the 'v8' feature.");
    eprintln!("Run with: cargo run --example v8_javascript --features v8");
    std::process::exit(1);
}
