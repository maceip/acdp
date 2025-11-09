//! Test V8 runtime timeout enforcement

#[cfg(feature = "v8")]
use acdp_sandbox::{ExecutionRequest, ResourceLimits, SandboxService, V8Runtime};
#[cfg(feature = "v8")]
use std::time::Duration;

#[cfg(feature = "v8")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== V8 Runtime Timeout Test ===\n");

    // Test 1: Code that completes within timeout
    println!("1. Testing code that completes quickly:");
    let limits = ResourceLimits {
        max_duration: Some(Duration::from_secs(2)),
        max_memory_bytes: Some(100 * 1024 * 1024), // 100 MB (V8 needs reasonable heap)
        max_cpu_time_ms: Some(2000),
    };

    let runtime = V8Runtime::with_limits(limits);
    let service = SandboxService::new(runtime);

    let quick_code = r#"
        let sum = 0;
        for (let i = 0; i < 1000; i++) {
            sum += i;
        }
        console.log("Sum:", sum);
        "Done!"
    "#;

    let request = ExecutionRequest::new(quick_code.to_string());
    let mut stream = service.execute(request).await?;

    while let Some(line) = stream.stdout.recv().await {
        print!("   [stdout] {}", String::from_utf8_lossy(&line));
    }

    let result = stream.result.await?;
    println!(
        "   Result: exit_code={}, duration={}ms, timed_out={}",
        result.exit_code, result.duration_ms, result.timed_out
    );

    println!("\n2. Testing code that times out (infinite loop):");
    let strict_limits = ResourceLimits {
        max_duration: Some(Duration::from_millis(500)), // 500ms timeout
        max_memory_bytes: Some(100 * 1024 * 1024),      // Still need enough for V8
        max_cpu_time_ms: Some(500),
    };

    let runtime2 = V8Runtime::with_limits(strict_limits);
    let service2 = SandboxService::new(runtime2);

    let timeout_code = r#"
        // Infinite loop - should timeout
        while (true) {
            Math.random();
        }
    "#;

    let request2 = ExecutionRequest::new(timeout_code.to_string());
    let mut stream2 = service2.execute(request2).await?;

    while let Some(line) = stream2.stderr.recv().await {
        print!("   [stderr] {}", String::from_utf8_lossy(&line));
    }

    let result2 = stream2.result.await?;
    println!(
        "   Result: exit_code={}, duration={}ms, timed_out={}",
        result2.exit_code, result2.duration_ms, result2.timed_out
    );

    if let Some(error) = result2.error {
        println!("   Error: {}", error);
    }

    if result2.timed_out {
        println!("\n✓ Timeout enforcement working!");
    } else {
        println!("\n✗ Timeout NOT enforced - potential security issue!");
    }

    Ok(())
}

#[cfg(not(feature = "v8"))]
fn main() {
    eprintln!("This example requires the 'v8' feature.");
    eprintln!("Run with: cargo run --example v8_timeout_test --features v8");
    std::process::exit(1);
}
