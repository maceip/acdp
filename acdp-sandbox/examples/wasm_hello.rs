//! WASM runtime example - demonstrates WebAssembly execution with WASI
//!
//! This example shows how to execute WebAssembly modules with WASI support

#[cfg(feature = "wasm")]
use acdp_sandbox::{ExecutionRequest, SandboxService, WasmRuntime};

#[cfg(feature = "wasm")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== WASM Runtime Demo ===\n");

    // Create WASM runtime
    let runtime = WasmRuntime::new()?;
    let service = SandboxService::new(runtime);

    println!("1. Testing WASM module compilation:\n");

    // Simple WAT (WebAssembly Text) program that we'll compile to binary
    let wat_program = r#"
    (module
        (import "wasi_snapshot_preview1" "fd_write"
            (func $fd_write (param i32 i32 i32 i32) (result i32)))

        (memory 1)
        (export "memory" (memory 0))

        (data (i32.const 0) "Hello from WASM!\n")

        (func $main (export "_start")
            ;; iovec structure at offset 100
            (i32.store (i32.const 100) (i32.const 0))   ;; buf pointer
            (i32.store (i32.const 104) (i32.const 17))  ;; buf length

            ;; fd_write(1, 100, 1, 108)
            ;; stdout=1, iovec=100, iovcnt=1, nwritten=108
            (call $fd_write
                (i32.const 1)   ;; fd (stdout)
                (i32.const 100) ;; iovs pointer
                (i32.const 1)   ;; iovs_len
                (i32.const 108) ;; nwritten pointer
            )
            drop
        )
    )
    "#;

    // Compile WAT to WASM binary
    match wat::parse_str(wat_program) {
        Ok(wasm_binary) => {
            println!(
                "   WAT compiled successfully ({} bytes)\n",
                wasm_binary.len()
            );

            // For now, encode as base64 since ExecutionRequest expects a String
            // In future, could extend ExecutionRequest to support Vec<u8>
            use base64::{engine::general_purpose, Engine as _};
            let wasm_code = general_purpose::STANDARD.encode(&wasm_binary);

            println!("2. Executing WASM module:");
            let request = ExecutionRequest::new(wasm_code);
            let mut stream = service.execute(request).await?;

            // Collect output
            let mut stdout_output = Vec::new();
            let mut stderr_output = Vec::new();

            while let Some(line) = stream.stdout.recv().await {
                stdout_output.extend_from_slice(&line);
                print!("   [stdout] {}", String::from_utf8_lossy(&line));
            }

            while let Some(line) = stream.stderr.recv().await {
                stderr_output.extend_from_slice(&line);
                print!("   [stderr] {}", String::from_utf8_lossy(&line));
            }

            let result = stream.result.await?;

            println!("\n3. Execution result:");
            println!("   Exit code: {}", result.exit_code);
            println!("   Duration: {}ms", result.duration_ms);
            if let Some(error) = result.error {
                println!("   Error: {}", error);
            }

            if result.exit_code == 0 {
                println!("\n✓ WASM execution successful!");
            } else {
                println!("\n✗ WASM execution failed");
            }
        }
        Err(e) => {
            println!("   Failed to compile WAT: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}

#[cfg(not(feature = "wasm"))]
fn main() {
    eprintln!("This example requires the 'wasm' feature.");
    eprintln!("Run with: cargo run --example wasm_hello --features wasm");
    std::process::exit(1);
}
