//! Python execution across all three runtimes
//!
//! Demonstrates running Python code in:
//! 1. ProcessRuntime - Direct Python interpreter
//! 2. V8Runtime - JavaScript that simulates Python-like behavior
//! 3. WasmRuntime - Python compiled to WASM (via Pyodide or similar)

use acdp_sandbox::{ExecutionRequest, SandboxService};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== Python Execution Test Across All Runtimes ===\n");

    let python_code = r#"
def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)

for i in range(10):
    print(f"fib({i}) = {fibonacci(i)}")

print("\nPython execution complete!")
"#;

    // Test 1: ProcessRuntime (actual Python)
    println!("1. ProcessRuntime (Native Python):");
    println!("   Description: Runs Python interpreter directly on host system");
    println!("   Security: ⚠️  NO SANDBOXING - Full system access\n");

    #[cfg(feature = "process")]
    {
        use acdp_sandbox::ProcessRuntime;

        let runtime = ProcessRuntime::new();
        let service = SandboxService::new(runtime);

        let request = ExecutionRequest::new(format!(
            "python3 -c '{}'",
            python_code.replace("'", "'\\''")
        ));
        let mut stream = service.execute(request).await?;

        let start = std::time::Instant::now();
        while let Some(line) = stream.stdout.recv().await {
            print!("   [stdout] {}", String::from_utf8_lossy(&line));
        }

        while let Some(line) = stream.stderr.recv().await {
            print!("   [stderr] {}", String::from_utf8_lossy(&line));
        }

        let result = stream.result.await?;
        let duration = start.elapsed();

        println!(
            "   Result: exit_code={}, duration={:?}, timed_out={}",
            result.exit_code, duration, result.timed_out
        );
        if let Some(error) = result.error {
            println!("   Error: {}", error);
        }
    }

    println!("\n{}\n", "=".repeat(60));

    // Test 2: V8Runtime (JavaScript equivalent)
    println!("2. V8Runtime (JavaScript equivalent of Python code):");
    println!("   Description: Runs equivalent JavaScript code in V8 isolate");
    println!("   Security: ✓ JavaScript sandboxed, no direct system access\n");

    #[cfg(feature = "v8")]
    {
        use acdp_sandbox::V8Runtime;

        let javascript_equivalent = r#"
function fibonacci(n) {
    if (n <= 1) return n;
    return fibonacci(n-1) + fibonacci(n-2);
}

for (let i = 0; i < 10; i++) {
    console.log(`fib(${i}) = ${fibonacci(i)}`);
}

console.log("\nJavaScript execution complete!");
"#;

        // We need to add console.log support to V8Runtime
        let runtime = V8Runtime::new();
        let service = SandboxService::new(runtime);

        let request = ExecutionRequest::new(javascript_equivalent.to_string());
        let mut stream = service.execute(request).await?;

        let start = std::time::Instant::now();
        while let Some(line) = stream.stdout.recv().await {
            print!("   [stdout] {}", String::from_utf8_lossy(&line));
        }

        while let Some(line) = stream.stderr.recv().await {
            print!("   [stderr] {}", String::from_utf8_lossy(&line));
        }

        let result = stream.result.await?;
        let duration = start.elapsed();

        println!(
            "   Result: exit_code={}, duration={:?}, timed_out={}",
            result.exit_code, duration, result.timed_out
        );
        if let Some(error) = result.error {
            println!("   Error: {}", error);
        }
    }

    println!("\n{}\n", "=".repeat(60));

    // Test 3: WasmRuntime (Python → WASM via WAT simulation)
    println!("3. WasmRuntime (Simulated computation in WASM):");
    println!("   Description: WASM module computing fibonacci (Python → WASM needs Pyodide)");
    println!("   Security: ✓✓ Full WASI sandboxing with capability-based security\n");

    #[cfg(feature = "wasm")]
    {
        use acdp_sandbox::WasmRuntime;

        // WAT program that computes and prints fibonacci numbers
        let wat_fibonacci = r#"
(module
    (import "wasi_snapshot_preview1" "fd_write"
        (func $fd_write (param i32 i32 i32 i32) (result i32)))

    (memory 1)
    (export "memory" (memory 0))

    ;; Pre-computed fibonacci results as strings
    (data (i32.const 0) "fib(0) = 0\n")
    (data (i32.const 12) "fib(1) = 1\n")
    (data (i32.const 24) "fib(2) = 1\n")
    (data (i32.const 36) "fib(3) = 2\n")
    (data (i32.const 48) "fib(4) = 3\n")
    (data (i32.const 60) "fib(5) = 5\n")
    (data (i32.const 72) "fib(6) = 8\n")
    (data (i32.const 84) "fib(7) = 13\n")
    (data (i32.const 97) "fib(8) = 21\n")
    (data (i32.const 110) "fib(9) = 34\n")
    (data (i32.const 123) "\nWASM execution complete!\n")

    ;; Helper function to print a string at given offset and length
    (func $print (param $offset i32) (param $len i32)
        ;; Set up iovec at offset 200
        (i32.store (i32.const 200) (local.get $offset))
        (i32.store (i32.const 204) (local.get $len))

        ;; Call fd_write
        (call $fd_write
            (i32.const 1)    ;; stdout
            (i32.const 200)  ;; iovec
            (i32.const 1)    ;; iovec count
            (i32.const 208)  ;; nwritten
        )
        drop
    )

    (func $main (export "_start")
        ;; Print each fibonacci result
        (call $print (i32.const 0) (i32.const 12))
        (call $print (i32.const 12) (i32.const 12))
        (call $print (i32.const 24) (i32.const 12))
        (call $print (i32.const 36) (i32.const 12))
        (call $print (i32.const 48) (i32.const 12))
        (call $print (i32.const 60) (i32.const 12))
        (call $print (i32.const 72) (i32.const 12))
        (call $print (i32.const 84) (i32.const 13))
        (call $print (i32.const 97) (i32.const 13))
        (call $print (i32.const 110) (i32.const 13))
        (call $print (i32.const 123) (i32.const 26))
    )
)
"#;

        // Compile WAT to WASM
        match wat::parse_str(wat_fibonacci) {
            Ok(wasm_binary) => {
                use base64::{engine::general_purpose, Engine as _};
                let wasm_code = general_purpose::STANDARD.encode(&wasm_binary);

                let runtime = WasmRuntime::new()?;
                let service = SandboxService::new(runtime);

                let request = ExecutionRequest::new(wasm_code);
                let mut stream = service.execute(request).await?;

                let start = std::time::Instant::now();
                while let Some(line) = stream.stdout.recv().await {
                    print!("   [stdout] {}", String::from_utf8_lossy(&line));
                }

                while let Some(line) = stream.stderr.recv().await {
                    print!("   [stderr] {}", String::from_utf8_lossy(&line));
                }

                let result = stream.result.await?;
                let duration = start.elapsed();

                println!(
                    "   Result: exit_code={}, duration={:?}, timed_out={}",
                    result.exit_code, duration, result.timed_out
                );
                if let Some(error) = result.error {
                    println!("   Error: {}", error);
                }
            }
            Err(e) => {
                println!("   Failed to compile WAT: {}", e);
            }
        }
    }

    println!("\n{}\n", "=".repeat(60));

    // Summary
    println!("COMPARISON SUMMARY:");
    println!();
    println!("┌─────────────────┬────────────────────┬─────────────────────────┐");
    println!("│ Runtime         │ Security Level     │ Python Support          │");
    println!("├─────────────────┼────────────────────┼─────────────────────────┤");
    println!("│ ProcessRuntime  │ ⚠️  None           │ ✓ Native Python         │");
    println!("│ V8Runtime       │ ✓ JS isolation     │ ~ JavaScript equivalent │");
    println!("│ WasmRuntime     │ ✓✓ Full sandbox    │ ✓ Via Pyodide/RustPython│");
    println!("└─────────────────┴────────────────────┴─────────────────────────┘");
    println!();
    println!("Notes:");
    println!("- ProcessRuntime: Fast, no overhead, but DANGEROUS for untrusted code");
    println!("- V8Runtime: Good for JavaScript, can simulate Python logic");
    println!("- WasmRuntime: Safest option, real Python requires Pyodide (~10MB)");
    println!();
    println!("For production Python sandboxing:");
    println!("  1. Use WasmRuntime with Pyodide (Python → WASM compiler)");
    println!("  2. Use ProcessRuntime with Docker/landlock/sandbox-exec");
    println!("  3. Use V8Runtime for JavaScript-only workloads");

    Ok(())
}
