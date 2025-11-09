//! HTTP+SSE Server for accepting incoming MCP client connections
//!
//! This module provides an HTTP server that accepts MCP client connections from Claude
//! or other MCP clients, processes requests through interceptors (semantic routing),
//! and forwards them to an upstream MCP server.
//!
//! Architecture:
//! Claude → HTTP-SSE Server (this module) → Interceptors → Upstream MCP Server

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{sse::Event, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::Stream;
use acdp_core::messages::{JsonRpcMessage, JsonRpcResponse};
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use acdp_core::interceptor::{InterceptorManager, MessageDirection};
use acdp_core::McpClient;

/// HTTP session tracking client connections
#[derive(Clone)]
struct HttpSession {
    /// Broadcast channel for sending SSE events to this client
    sse_tx: broadcast::Sender<JsonRpcMessage>,
}

/// Shared state for HTTP+SSE server
#[derive(Clone)]
pub struct HttpSseServerState {
    /// Session map (session_id -> HttpSession)
    sessions: Arc<Mutex<HashMap<String, HttpSession>>>,
    /// Interceptor manager for semantic routing
    interceptor_manager: Arc<InterceptorManager>,
    /// Upstream MCP client connection
    upstream_client: Arc<RwLock<McpClient>>,
}

impl HttpSseServerState {
    pub fn new(interceptor_manager: Arc<InterceptorManager>, upstream_client: McpClient) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            interceptor_manager,
            upstream_client: Arc::new(RwLock::new(upstream_client)),
        }
    }
}

/// Create HTTP+SSE server router
pub fn create_router(state: HttpSseServerState) -> Router {
    Router::new()
        .route("/sse", post(handle_sse_post))
        .route("/sse", get(handle_sse_get))
        .route("/sse/:session_id", get(handle_sse_session))
        .with_state(state)
}

/// Start HTTP+SSE server
pub async fn start_server(
    listener: tokio::net::TcpListener,
    state: HttpSseServerState,
) -> Result<()> {
    let app = create_router(state);

    let bind_addr = listener
        .local_addr()
        .context("Failed to obtain HTTP server bind address")?;
    info!("Starting HTTP+SSE server on {}", bind_addr);

    axum::serve(listener, app)
        .await
        .context("HTTP server error")?;

    Ok(())
}

/// Handle HTTP POST to /sse - send request and get single response
async fn handle_sse_post(
    State(state): State<HttpSseServerState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response<Body>, StatusCode> {
    debug!("Received SSE POST request: {:?}", body);

    // Parse JSON-RPC message
    let message: JsonRpcMessage = match serde_json::from_value(body) {
        Ok(msg) => msg,
        Err(e) => {
            error!("Failed to parse JSON-RPC message: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Extract session ID from headers
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Process through interceptors
    let metadata =
        acdp_core::interceptor::InterceptionMetadata::default().with_session_id(session_id.clone());

    let result = state
        .interceptor_manager
        .process_message_with_metadata(message.clone(), MessageDirection::Outgoing, metadata)
        .await
        .map_err(|e| {
            error!("Interceptor processing failed: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if result.block {
        warn!("Request blocked by interceptor: {:?}", result.reasoning);
        return Err(StatusCode::FORBIDDEN);
    }

    // Forward to upstream MCP server
    let response = {
        let mut client = state.upstream_client.write().await;

        match &result.message {
            JsonRpcMessage::Request(req) => {
                let params = req.params.clone().unwrap_or(serde_json::json!({}));
                client
                    .send_request(&req.method, params)
                    .await
                    .map_err(|e| {
                        error!("Upstream request failed: {}", e);
                        StatusCode::BAD_GATEWAY
                    })?
            }
            JsonRpcMessage::Notification(notif) => {
                let params = notif.params.clone().unwrap_or(serde_json::json!({}));
                client
                    .send_notification(&notif.method, params)
                    .await
                    .map_err(|e| {
                        error!("Upstream notification failed: {}", e);
                        StatusCode::BAD_GATEWAY
                    })?;
                // Notifications don't have responses
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: Some(serde_json::json!({"status": "sent"})),
                    error: None,
                    id: acdp_core::messages::RequestId::Null,
                }
            }
            _ => {
                error!("Unexpected message type after interceptor processing");
                return Err(StatusCode::BAD_REQUEST);
            }
        }
    };

    // Return as SSE event
    let sse_json = match serde_json::to_string(&response) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to serialize response to JSON: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    let sse_data = format!("data: {}\n\n", sse_json);

    let response_builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Mcp-Session-Id", session_id)
        .body(Body::from(sse_data));

    match response_builder {
        Ok(resp) => Ok(resp),
        Err(e) => {
            error!("Failed to build response: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Handle HTTP GET to /sse - establish persistent SSE connection
async fn handle_sse_get(
    State(state): State<HttpSseServerState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    info!("SSE GET connection established: session {}", session_id);

    // Create or get session
    let mut sse_rx = {
        let mut sessions = state.sessions.lock().await;

        if let Some(session) = sessions.get(&session_id) {
            session.sse_tx.subscribe()
        } else {
            let (tx, rx) = broadcast::channel(100);
            let session = HttpSession { sse_tx: tx.clone() };

            sessions.insert(session_id.clone(), session);
            rx
        }
    };

    // Create SSE stream
    let stream = async_stream::stream! {
        // Send connection established event
        yield Ok(Event::default()
            .event("connected")
            .data(format!(r#"{{"session_id":"{}"}}"#, session_id)));

        // Stream messages from broadcast channel
        loop {
            match sse_rx.recv().await {
                Ok(message) => {
                    if let Ok(json) = serde_json::to_string(&message) {
                        yield Ok(Event::default().data(json));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!("SSE client {} lagged, skipped {} messages", session_id, skipped);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("SSE stream closed for session {}", session_id);
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    )
}

/// Handle HTTP GET to /sse/:session_id - resume existing SSE session
async fn handle_sse_session(
    Path(session_id): Path<String>,
    State(state): State<HttpSseServerState>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    info!("SSE session resume requested: {}", session_id);

    let sse_tx = {
        let sessions = state.sessions.lock().await;
        sessions
            .get(&session_id)
            .map(|s| s.sse_tx.clone())
            .ok_or(StatusCode::NOT_FOUND)?
    };

    let mut sse_rx = sse_tx.subscribe();

    let stream = async_stream::stream! {
        yield Ok(Event::default()
            .event("resumed")
            .data(format!(r#"{{"session_id":"{}"}}"#, session_id)));

        loop {
            match sse_rx.recv().await {
                Ok(message) => {
                    if let Ok(json) = serde_json::to_string(&message) {
                        yield Ok(Event::default().data(json));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!("SSE client {} lagged, skipped {} messages", session_id, skipped);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("SSE stream closed for session {}", session_id);
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    ))
}
