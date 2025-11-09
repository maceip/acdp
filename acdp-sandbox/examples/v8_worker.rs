//! V8 Worker Runtime example - Hono-style fetch API
//!
//! Demonstrates a worker-like JavaScript environment with Fetch API support,
//! similar to Cloudflare Workers, Deno Deploy, or Hono.

#[cfg(feature = "v8-worker")]
use anyhow::Context;
#[cfg(feature = "v8-worker")]
use acdp_sandbox::worker::create_worker_runtime;
#[cfg(feature = "v8-worker")]
use std::net::SocketAddr;
#[cfg(feature = "v8-worker")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(feature = "v8-worker")]
use tokio::net::{TcpListener, TcpStream};
#[cfg(feature = "v8-worker")]
use tokio::sync::oneshot;
#[cfg(feature = "v8-worker")]
use tokio::task::JoinHandle;

#[cfg(feature = "v8-worker")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== V8 Worker Runtime Demo ===\n");

    // Start local test HTTP server to provide deterministic fetch targets
    let server = TestServer::start().await?;
    let json_url = format!("{}/json", server.base_url());
    let post_url = format!("{}/post", server.base_url());

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

    // Test Response.json()
    println!("2. Testing Response.json():");
    runtime.execute_script(
        "<test-response>",
        r#"
        const data = { message: "Hello from worker!", count: 42 };
        const response = Response.json(data);
        response.status;
        "#,
    )?;

    println!("   (Response created with JSON data)\n");

    // Test simple fetch (GET request)
    println!("3. Testing fetch() with local server:");
    println!("   Fetching: {}", json_url);

    let code = format!(
        r#"
        (async () => {{
            try {{
                const response = await fetch('{url}');
                const data = await response.json();
                return JSON.stringify({{
                    status: response.status,
                    ok: response.ok,
                    data: data
                }}, null, 2);
            }} catch (err) {{
                return `Error: ${{err.message}}`;
            }}
        }})()
    "#,
        url = json_url
    );

    let result = runtime.execute_script("<test-fetch>", code)?;

    // Resolve the promise while polling the V8 event loop
    let promise = runtime.resolve(result);
    let promise_result = runtime
        .with_event_loop_promise(promise, deno_core::PollEventLoopOptions::default())
        .await?;
    {
        let scope = &mut runtime.handle_scope();
        let local = deno_core::v8::Local::new(scope, promise_result);
        let result_str = local.to_rust_string_lossy(scope);

        println!("   Response:\n{}\n", result_str);
    }

    // Test POST request
    println!("4. Testing POST request:");
    println!("   Posting to: {}", post_url);

    let code = format!(
        r#"
        (async () => {{
            try {{
                const response = await fetch('{url}', {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify({{ name: 'mcp-sandbox', version: '0.1.0' }})
                }});
                const data = await response.json();
                return JSON.stringify({{
                    status: response.status,
                    ok: response.ok,
                    data: data
                }}, null, 2);
            }} catch (err) {{
                return `Error: ${{err.message}}`;
            }}
        }})()
    "#,
        url = post_url
    );

    let result = runtime.execute_script("<test-post>", code)?;

    // Resolve the promise while polling the V8 event loop
    let promise = runtime.resolve(result);
    let promise_result = runtime
        .with_event_loop_promise(promise, deno_core::PollEventLoopOptions::default())
        .await?;
    {
        let scope = &mut runtime.handle_scope();
        let local = deno_core::v8::Local::new(scope, promise_result);
        let result_str = local.to_rust_string_lossy(scope);

        println!("   Response:\n{}\n", result_str);
    }

    println!("✓ Worker runtime demo complete!");
    println!("\nThis demonstrates a Hono/Cloudflare Workers-style environment");
    println!("with fetch(), Request, and Response APIs.");

    server.shutdown().await?;

    Ok(())
}

#[cfg(not(feature = "v8-worker"))]
fn main() {
    eprintln!("This example requires the 'v8-worker' feature.");
    eprintln!("Run with: cargo run --example v8_worker --features v8-worker");
    std::process::exit(1);
}

#[cfg(feature = "v8-worker")]
struct TestServer {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: JoinHandle<()>,
}

#[cfg(feature = "v8-worker")]
impl TestServer {
    async fn start() -> anyhow::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("Failed to bind local test server")?;
        let addr = listener
            .local_addr()
            .context("Failed to read local address for test server")?;
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    accept_res = listener.accept() => {
                        let Ok((socket, _)) = accept_res else {
                            continue;
                        };
                        tokio::spawn(async move {
                            if let Err(err) = handle_connection(socket).await {
                                tracing::warn!(error = %err, "test server connection error");
                            }
                        });
                    }
                }
            }
        });

        Ok(Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            handle,
        })
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.handle
            .await
            .context("Failed to join test server task")?;
        Ok(())
    }
}

#[cfg(feature = "v8-worker")]
async fn handle_connection(mut socket: TcpStream) -> anyhow::Result<()> {
    let mut buffer = Vec::with_capacity(1024);
    let mut content_length = None;

    loop {
        let mut chunk = [0u8; 1024];
        let n = socket
            .read(&mut chunk)
            .await
            .context("Failed to read request from socket")?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);

        if let Some(headers_end) = find_headers_end(&buffer) {
            if content_length.is_none() {
                content_length = parse_content_length(&buffer[..headers_end]);
            }
            let body_len = buffer.len().saturating_sub(headers_end);
            if body_len >= content_length.unwrap_or(0) {
                break;
            }
        }
    }

    let request = String::from_utf8_lossy(&buffer);

    let (status_line, body) = if request.starts_with("GET /json") {
        (
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "message": "Hello from local server",
                "count": 3,
                "tags": ["sandbox", "fetch", "local"],
            })
            .to_string(),
        )
    } else if request.starts_with("POST /post") {
        (
            "HTTP/1.1 200 OK",
            serde_json::json!({
                "status": "ok",
                "received": {
                    "name": "mcp-sandbox",
                    "version": "0.1.0"
                }
            })
            .to_string(),
        )
    } else {
        (
            "HTTP/1.1 404 Not Found",
            serde_json::json!({ "error": "not found" }).to_string(),
        )
    };

    let response = format!(
        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    socket
        .write_all(response.as_bytes())
        .await
        .context("Failed to write HTTP response")?;
    let _ = socket.shutdown().await;

    Ok(())
}

#[cfg(feature = "v8-worker")]
fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|pos| pos + 4)
}

#[cfg(feature = "v8-worker")]
fn parse_content_length(headers: &[u8]) -> Option<usize> {
    let headers_str = String::from_utf8_lossy(headers);
    for line in headers_str.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                return value.trim().parse().ok();
            }
        }
    }
    None
}
