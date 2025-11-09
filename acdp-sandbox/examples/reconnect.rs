//! Reconnection example - demonstrates resilient execution

use acdp_sandbox::{ExecutionId, ExecutionRequest, ProcessRuntime, SandboxService};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let service = SandboxService::new(ProcessRuntime::new());
    println!("=== Reconnection Example ===\n");

    // Start a long-running execution
    let exec_id = ExecutionId::new();
    println!("Starting execution with ID: {}", exec_id);

    let request = ExecutionRequest::new(
        r#"
        for i in $(seq 1 10); do
            echo "Line $i"
            sleep 1
        done
        "#,
    );

    let mut stream = service.execute_with_id(exec_id, request).await?;

    // Read first few lines
    println!("\n--- Initial connection ---");
    for _ in 0..3 {
        if let Some(line) = stream.stdout.recv().await {
            print!("  {}", String::from_utf8_lossy(&line));
        }
    }

    // Simulate disconnect
    println!("\n--- Simulating disconnect ---");
    drop(stream);
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Reconnect using execution ID
    println!("--- Reconnecting with ID: {} ---", exec_id);
    let state = service
        .get_execution(exec_id)
        .await
        .expect("Execution not found");

    println!("Status: {:?}", state.status);
    println!("Buffered lines: {}", state.stdout_buffer.len());

    // Show buffered output
    println!("\n--- Buffered output ---");
    for line in &state.stdout_buffer {
        print!("  {}", String::from_utf8_lossy(line));
    }

    // Continue streaming from where we left off
    // (Note: Current implementation doesn't fully support this yet,
    //  this demonstrates the API for future enhancement)

    println!("\n--- Active executions ---");
    let executions = service.list_executions().await;
    for exec in executions {
        println!("  ID: {} Status: {:?}", exec.id, exec.status);
    }

    Ok(())
}
