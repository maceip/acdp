//! Simple sandbox example - execute code and stream output

use acdp_sandbox::{ExecutionRequest, ProcessRuntime, SandboxService};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Create sandbox service with process runtime
    let service = SandboxService::new(ProcessRuntime::new());

    println!("=== Sandbox Service Example ===\n");
    println!("Runtime: {}\n", service.runtime_name());

    // Example 1: Simple command
    println!("Example 1: Simple echo");
    let request = ExecutionRequest::new("echo 'Hello from sandbox!'");
    execute_and_print(&service, request).await?;

    // Example 2: Multi-line output
    println!("\nExample 2: Count to 5");
    let request = ExecutionRequest::new("for i in 1 2 3 4 5; do echo $i; done");
    execute_and_print(&service, request).await?;

    // Example 3: With timeout
    println!("\nExample 3: Long running task with timeout");
    let request = ExecutionRequest::new("sleep 10 && echo 'Done!'").with_timeout(2);
    execute_and_print(&service, request).await?;

    // Example 4: Error output
    println!("\nExample 4: Error to stderr");
    let request = ExecutionRequest::new("echo 'Error message' >&2; exit 1");
    execute_and_print(&service, request).await?;

    Ok(())
}

async fn execute_and_print(
    service: &SandboxService,
    request: ExecutionRequest,
) -> anyhow::Result<()> {
    let mut stream = service.execute(request).await?;

    // Stream stdout
    let stdout_handle = tokio::spawn(async move {
        while let Some(line) = stream.stdout.recv().await {
            print!("  stdout: {}", String::from_utf8_lossy(&line));
        }
    });

    // Stream stderr
    let stderr_handle = tokio::spawn(async move {
        while let Some(line) = stream.stderr.recv().await {
            print!("  stderr: {}", String::from_utf8_lossy(&line));
        }
    });

    // Wait for result
    let result = stream.result.await?;

    // Wait for output streams to finish
    stdout_handle.await?;
    stderr_handle.await?;

    // Print result
    println!(
        "  Result: exit_code={} duration={}ms timed_out={}",
        result.exit_code, result.duration_ms, result.timed_out
    );
    if let Some(error) = result.error {
        println!("  Error: {}", error);
    }

    Ok(())
}
