//! V8 Worker Runtime example - Simple demo without HTTP requests
//!
//! Demonstrates the Hono-style Worker API with Request/Response classes

#[cfg(feature = "v8-worker")]
use acdp_sandbox::worker::create_worker_runtime;

#[cfg(feature = "v8-worker")]
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== V8 Worker Runtime Demo ===\n");

    // Create worker runtime with Fetch API
    let mut runtime = create_worker_runtime(None);

    println!("1. Testing Fetch API globals:");
    let result = runtime.execute_script(
        "<check-globals>",
        r#"
        const hasFetch = typeof fetch === 'function';
        const hasRequest = typeof Request === 'function';
        const hasResponse = typeof Response === 'function';
        JSON.stringify({ fetch: hasFetch, Request: hasRequest, Response: hasResponse });
        "#,
    )?;

    {
        let scope = &mut runtime.handle_scope();
        let local = deno_core::v8::Local::new(scope, result);
        let result_str = local.to_rust_string_lossy(scope);
        println!("   Globals: {}\n", result_str);
    }

    // Test Request class
    println!("2. Testing Request class:");
    let result = runtime.execute_script(
        "<test-request>",
        r#"
        const req = new Request('https://example.com/api', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ name: 'test' })
        });
        JSON.stringify({
            url: req.url,
            method: req.method,
            headers: req.headers
        }, null, 2);
        "#,
    )?;

    {
        let scope = &mut runtime.handle_scope();
        let local = deno_core::v8::Local::new(scope, result);
        let result_str = local.to_rust_string_lossy(scope);
        println!("   Request:\n{}\n", result_str);
    }

    // Test Response.json()
    println!("3. Testing Response.json():");
    let result = runtime.execute_script(
        "<test-response>",
        r#"
        const data = { message: "Hello from worker!", count: 42, success: true };
        const response = Response.json(data);
        JSON.stringify({
            status: response.status,
            ok: response.ok,
            headers: response.headers
        }, null, 2);
        "#,
    )?;

    {
        let scope = &mut runtime.handle_scope();
        let local = deno_core::v8::Local::new(scope, result);
        let result_str = local.to_rust_string_lossy(scope);
        println!("   Response:\n{}\n", result_str);
    }

    // Test Response with custom status
    println!("4. Testing Response with custom status:");
    let result = runtime.execute_script(
        "<test-response-status>",
        r#"
        const errorResponse = new Response('Not found', { status: 404 });
        JSON.stringify({
            status: errorResponse.status,
            ok: errorResponse.ok
        });
        "#,
    )?;

    {
        let scope = &mut runtime.handle_scope();
        let local = deno_core::v8::Local::new(scope, result);
        let result_str = local.to_rust_string_lossy(scope);
        println!("   Response: {}\n", result_str);
    }

    println!("✓ Worker runtime demo complete!");
    println!("\nThis demonstrates a Hono/Cloudflare Workers-style environment");
    println!("with fetch(), Request, and Response APIs.");
    println!("\nNote: Actual HTTP requests with fetch() require an async runtime.");
    println!("See v8_worker.rs for async HTTP examples.");

    Ok(())
}

#[cfg(not(feature = "v8-worker"))]
fn main() {
    eprintln!("This example requires the 'v8-worker' feature.");
    eprintln!("Run with: cargo run --example v8_worker_simple --features v8-worker");
    std::process::exit(1);
}
