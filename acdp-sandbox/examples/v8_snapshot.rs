//! V8 snapshot example - demonstrates loading pre-built snapshots
//!
//! Note: Creating snapshots with deno_core requires using `deno_core::snapshot::create_snapshot()`
//! in build.rs with extensions. This example demonstrates the API for loading snapshots.

#[cfg(feature = "v8")]
use acdp_sandbox::{ExecutionRequest, SandboxService, SnapshotBuilder, V8Runtime};
#[cfg(feature = "v8")]
use std::time::Instant;

#[cfg(feature = "v8")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== V8 Snapshot API Demo ===\n");

    // Test without snapshot
    println!("1. Standard V8 runtime (no snapshot):");
    let start = Instant::now();
    let runtime = V8Runtime::new();
    let create_time = start.elapsed();

    let service = SandboxService::new(runtime);
    let request = ExecutionRequest::new("const x = [1,2,3,4,5].map(n => n * 2); x.join(',')");

    let start = Instant::now();
    let mut stream = service.execute(request).await?;

    let mut output = String::new();
    while let Some(line) = stream.stdout.recv().await {
        output.push_str(&String::from_utf8_lossy(&line));
    }
    let result = stream.result.await?;
    let exec_time = start.elapsed();

    println!("   Runtime creation: {:?}", create_time);
    println!("   Execution time:   {:?}", exec_time);
    println!("   Output: {}\n", output.trim());

    // Demonstrate snapshot loading API (snapshot doesn't exist, so will use default)
    println!("2. V8 runtime with snapshot config:");
    let snap_path = "/tmp/mcp-sandbox-demo.snap";

    let start = Instant::now();
    let config = SnapshotBuilder::new(snap_path).build();
    let runtime_with_config = V8Runtime::from_snapshot_config(config)?;
    let create_time = start.elapsed();

    let service = SandboxService::new(runtime_with_config);
    let request = ExecutionRequest::new("Math.sqrt(144)");

    let start = Instant::now();
    let mut stream = service.execute(request).await?;

    let mut output = String::new();
    while let Some(line) = stream.stdout.recv().await {
        output.push_str(&String::from_utf8_lossy(&line));
    }
    let result = stream.result.await?;
    let exec_time = start.elapsed();

    println!("   Runtime creation: {:?}", create_time);
    println!("   Execution time:   {:?}", exec_time);
    println!("   Output: {}\n", output.trim());

    println!("✓ Snapshot API demo complete!");
    println!("\nNote: To create snapshots with custom initialization code,");
    println!("use deno_core::snapshot::create_snapshot() in build.rs with extensions.");

    Ok(())
}

#[cfg(not(feature = "v8"))]
fn main() {
    eprintln!("This example requires the 'v8' feature.");
    eprintln!("Run with: cargo run --example v8_snapshot --features v8");
    std::process::exit(1);
}
