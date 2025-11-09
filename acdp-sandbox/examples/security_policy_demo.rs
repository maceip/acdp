//! Security policy demonstration
//!
//! Shows how the runtime selector enforces security policies

use acdp_sandbox::{
    ExecutionRequest, Language, ResourceLimits, RuntimeRequirement, RuntimeSelector, RuntimeType,
    SandboxService, SecurityPolicy, ToolDefinition, TrustLevel,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== Security Policy Demonstration ===\n");

    // Create selector with default security policy
    let selector = RuntimeSelector::new();

    // Test 1: Untrusted tool (should only get WASM)
    println!("1. Untrusted JavaScript Tool:");
    let untrusted_tool = ToolDefinition {
        id: "user_script".to_string(),
        name: "User-Provided Script".to_string(),
        trust_level: TrustLevel::Untrusted,
        runtime: RuntimeRequirement::Auto {
            language: Language::JavaScript,
            preferred: None,
        },
        limits: None,
    };

    match selector.select_runtime(&untrusted_tool) {
        Ok(runtime) => {
            println!("   ✓ Selected runtime: {}", runtime.name());
            println!("   Security: Only WASM allowed for untrusted code");

            // Test execution
            let service = SandboxService::new(runtime);
            let code = "console.log('Hello from untrusted code!'); 42";
            let request = ExecutionRequest::new(code.to_string());

            let mut stream = service.execute(request).await?;
            while let Some(line) = stream.stdout.recv().await {
                print!("   [output] {}", String::from_utf8_lossy(&line));
            }

            let result = stream.result.await?;
            println!("   Result: exit_code={}\n", result.exit_code);
        }
        Err(e) => {
            println!("   ✗ Runtime selection failed: {}\n", e);
        }
    }

    // Test 2: Trusted tool (more options available)
    println!("2. Trusted JavaScript Tool:");
    let trusted_tool = ToolDefinition {
        id: "builtin_analytics".to_string(),
        name: "Built-in Analytics".to_string(),
        trust_level: TrustLevel::Trusted,
        runtime: RuntimeRequirement::Auto {
            language: Language::JavaScript,
            preferred: Some(RuntimeType::V8),
        },
        limits: Some(ResourceLimits::permissive()),
    };

    match selector.select_runtime(&trusted_tool) {
        Ok(runtime) => {
            println!("   ✓ Selected runtime: {}", runtime.name());
            println!("   Security: Trusted tools can use V8 or WASM");
            println!("   Limits: Permissive (5min timeout, 500MB memory)\n");
        }
        Err(e) => {
            println!("   ✗ Runtime selection failed: {}\n", e);
        }
    }

    // Test 3: System tool requesting Process runtime (denied by default)
    println!("3. System Tool Requesting Process Runtime:");
    let system_tool = ToolDefinition {
        id: "system_maintenance".to_string(),
        name: "System Maintenance".to_string(),
        trust_level: TrustLevel::System,
        runtime: RuntimeRequirement::Specific {
            runtime: RuntimeType::Process,
        },
        limits: None,
    };

    match selector.select_runtime(&system_tool) {
        Ok(runtime) => {
            println!("   ✓ Selected runtime: {}", runtime.name());
        }
        Err(e) => {
            println!("   ✗ Denied: {}", e);
            println!("   Reason: Process runtime disabled by default policy\n");
        }
    }

    // Test 4: Custom policy allowing Process for System tools
    println!("4. Custom Policy - Allow Process for System Tools:");

    let mut custom_policy = SecurityPolicy::default();
    custom_policy.allow_process_runtime = true; // Enable Process runtime globally

    let custom_selector = RuntimeSelector::with_policy(custom_policy);

    match custom_selector.select_runtime(&system_tool) {
        Ok(runtime) => {
            println!("   ✓ Selected runtime: {}", runtime.name());
            println!("   Note: Process runtime now allowed by custom policy\n");
        }
        Err(e) => {
            println!("   ✗ Selection failed: {}\n", e);
        }
    }

    // Test 5: Policy enforcement example
    println!("5. Policy Enforcement Summary:");
    println!("   ┌──────────────┬─────────────┬──────────────────────┐");
    println!("   │ Trust Level  │ Default RT  │ Allowed Runtimes     │");
    println!("   ├──────────────┼─────────────┼──────────────────────┤");
    println!("   │ Untrusted    │ WASM        │ WASM only            │");
    println!("   │ Verified     │ WASM        │ WASM, V8             │");
    println!("   │ Trusted      │ V8          │ WASM, V8             │");
    println!("   │ System       │ V8          │ WASM, V8, (Process*) │");
    println!("   └──────────────┴─────────────┴──────────────────────┘");
    println!("   * Process only if allow_process_runtime = true\n");

    // Test 6: Language-based auto-selection
    println!("6. Language-Based Auto-Selection:");

    let python_tool = ToolDefinition {
        id: "data_analysis".to_string(),
        name: "Data Analysis Script".to_string(),
        trust_level: TrustLevel::Verified,
        runtime: RuntimeRequirement::Auto {
            language: Language::Python,
            preferred: None,
        },
        limits: None,
    };

    match selector.select_runtime(&python_tool) {
        Ok(runtime) => {
            println!("   Language: Python");
            println!("   ✓ Selected runtime: {}", runtime.name());
            println!("   Auto-selection: Python → WASM (via Pyodide in production)\n");
        }
        Err(e) => {
            println!("   ✗ Selection failed: {}", e);
            println!("   Note: Python requires WASM with Pyodide (not yet integrated)\n");
        }
    }

    Ok(())
}
