//! Hono-style Worker Runtime with Fetch API
//!
//! Provides a worker-like JavaScript environment similar to Cloudflare Workers,
//! Deno Deploy, or Hono, with support for the Fetch API and Request/Response objects.

use deno_core::{op2, Extension, JsRuntime, RuntimeOptions};
use deno_error::JsErrorBox;

/// Fetch operation - HTTP GET request
#[op2(async)]
#[string]
async fn op_fetch(#[string] url: String) -> Result<String, JsErrorBox> {
    tracing::debug!(url = %url, "Fetching URL");

    let response = reqwest::get(&url)
        .await
        .map_err(|e| JsErrorBox::type_error(format!("Fetch failed: {}", e)))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|e| JsErrorBox::type_error(format!("Failed to read response: {}", e)))?;

    // Return simple response JSON
    Ok(serde_json::json!({
        "status": status,
        "body": body,
    })
    .to_string())
}

/// Fetch with full options - POST, headers, body, etc.
#[op2(async)]
#[string]
async fn op_fetch_request(
    #[string] url: String,
    #[string] method: String,
    #[string] body: Option<String>,
    #[serde] headers: Option<serde_json::Value>,
) -> Result<String, JsErrorBox> {
    tracing::debug!(
        url = %url,
        method = %method,
        has_body = body.is_some(),
        "Fetching with options"
    );

    let client = reqwest::Client::new();
    let mut request = match method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        _ => {
            return Err(JsErrorBox::type_error(format!(
                "Unsupported HTTP method: {}",
                method
            )))
        }
    };

    // Add headers if provided
    if let Some(headers_obj) = headers {
        if let Some(headers_map) = headers_obj.as_object() {
            for (key, value) in headers_map {
                if let Some(val_str) = value.as_str() {
                    request = request.header(key, val_str);
                }
            }
        }
    }

    // Add body if provided
    if let Some(body_str) = body {
        request = request.body(body_str);
    }

    let response = request
        .send()
        .await
        .map_err(|e| JsErrorBox::type_error(format!("Request failed: {}", e)))?;
    let status = response.status().as_u16();
    let response_body = response
        .text()
        .await
        .map_err(|e| JsErrorBox::type_error(format!("Failed to read response: {}", e)))?;

    Ok(serde_json::json!({
        "status": status,
        "body": response_body,
    })
    .to_string())
}

/// Create worker runtime extension with Fetch API
pub fn create_worker_extension() -> Extension {
    let ops = vec![op_fetch(), op_fetch_request()];

    Extension {
        name: "mcp_worker",
        ops: std::borrow::Cow::Owned(ops),
        ..Default::default()
    }
}

/// JavaScript polyfill for Fetch API
const WORKER_RUNTIME_JS: &str = r#"
// Simple Fetch API implementation
globalThis.fetch = async function(url, options = {}) {
    const method = options.method || 'GET';
    const headers = options.headers || {};
    const body = options.body;

    if (method === 'GET' && !options.method) {
        // Simple GET request
        const response = await Deno.core.ops.op_fetch(url);
        const data = JSON.parse(response);

        return {
            status: data.status,
            ok: data.status >= 200 && data.status < 300,
            text: async () => data.body,
            json: async () => JSON.parse(data.body),
        };
    } else {
        // Full request with options
        const response = await Deno.core.ops.op_fetch_request(url, method, body, headers);
        const data = JSON.parse(response);

        return {
            status: data.status,
            ok: data.status >= 200 && data.status < 300,
            text: async () => data.body,
            json: async () => JSON.parse(data.body),
        };
    }
};

// Request class stub (simplified)
globalThis.Request = class Request {
    constructor(url, options = {}) {
        this.url = url;
        this.method = options.method || 'GET';
        this.headers = options.headers || {};
        this.body = options.body;
    }
};

// Response class stub (simplified)
globalThis.Response = class Response {
    constructor(body, options = {}) {
        this.body = body;
        this.status = options.status || 200;
        this.ok = this.status >= 200 && this.status < 300;
        this.headers = options.headers || {};
    }

    async text() {
        return this.body;
    }

    async json() {
        return JSON.parse(this.body);
    }

    static json(data, options = {}) {
        return new Response(JSON.stringify(data), {
            ...options,
            headers: { 'Content-Type': 'application/json', ...options.headers },
        });
    }
};
"#;

/// Create a worker-style V8 runtime with Fetch API
pub fn create_worker_runtime(snapshot: Option<&'static [u8]>) -> JsRuntime {
    let extensions = vec![create_worker_extension()];

    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions,
        startup_snapshot: snapshot.map(|s| s.into()),
        ..Default::default()
    });

    // Initialize worker globals
    runtime
        .execute_script("<worker-init>", WORKER_RUNTIME_JS)
        .expect("Failed to initialize worker runtime");

    runtime
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_runtime_creation() {
        let mut runtime = create_worker_runtime(None);

        // Test that fetch is defined
        let result = runtime
            .execute_script(
                "<test>",
                r#"
                typeof fetch === 'function' &&
                typeof Request === 'function' &&
                typeof Response === 'function'
                "#,
            )
            .unwrap();

        let scope = &mut runtime.handle_scope();
        let local = deno_core::v8::Local::new(scope, result);
        assert!(local.is_true());
    }
}
