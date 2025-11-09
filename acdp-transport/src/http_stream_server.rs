//! HTTP-Stream Server for accepting incoming MCP client connections
//!
//! This module provides an HTTP server that accepts MCP client connections
//! via HTTP POST requests to /mcp endpoint and returns JSON responses.
//! This implements the MCP Streamable HTTP protocol (2025-03-26).

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::post,
    Json, Router,
};
use acdp_core::messages::{JsonRpcMessage, JsonRpcResponse};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use acdp_core::interceptor::{InterceptorManager, MessageDirection};
use acdp_core::McpClient;

/// Shared state for HTTP-Stream server
#[derive(Clone)]
pub struct HttpStreamServerState {
    /// Interceptor manager for semantic routing
    interceptor_manager: Arc<InterceptorManager>,
    /// Upstream MCP client connection (shared across sessions)
    upstream_client: Arc<RwLock<McpClient>>,
}

impl HttpStreamServerState {
    pub fn new(interceptor_manager: Arc<InterceptorManager>, upstream_client: McpClient) -> Self {
        Self {
            interceptor_manager,
            upstream_client: Arc::new(RwLock::new(upstream_client)),
        }
    }
}

/// Create HTTP-Stream server router
pub fn create_router(state: HttpStreamServerState) -> Router {
    Router::new()
        .route("/mcp", post(handle_mcp_request))
        .with_state(state)
}

/// Start HTTP-Stream server
pub async fn start_server(
    listener: tokio::net::TcpListener,
    state: HttpStreamServerState,
) -> Result<()> {
    let app = create_router(state);

    let bind_addr = listener
        .local_addr()
        .context("Failed to obtain HTTP-Stream server bind address")?;
    info!("Starting HTTP-Stream server on {}", bind_addr);

    axum::serve(listener, app)
        .await
        .context("HTTP-Stream server error")?;

    Ok(())
}

/// Handle HTTP POST to /mcp - send request and get response
async fn handle_mcp_request(
    State(state): State<HttpStreamServerState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response<Body>, StatusCode> {
    debug!("Received HTTP-Stream POST request: {:?}", body);

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
        .get("mcp-session-id")
        .or_else(|| headers.get("Mcp-Session-Id"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Process through interceptors (outgoing: client → server)
    let result = state
        .interceptor_manager
        .process_message_with_metadata(
            message.clone(),
            MessageDirection::Outgoing,
            acdp_core::interceptor::InterceptionMetadata::default()
                .with_session_id(session_id.clone()),
        )
        .await
        .map_err(|e| {
            error!("Interceptor processing failed: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if result.block {
        warn!("Request blocked by interceptor: {:?}", result.reasoning);
        // Return error response for blocked request
        let error_response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(acdp_core::messages::JsonRpcError {
                code: -32000,
                message: "Request blocked by interceptor".to_string(),
                data: result.reasoning.map(|r| serde_json::Value::String(r)),
            }),
            id: match &message {
                JsonRpcMessage::Request(req) => req.id.clone(),
                JsonRpcMessage::Notification(_) => acdp_core::messages::RequestId::Null,
                JsonRpcMessage::Response(_) => acdp_core::messages::RequestId::Null,
            },
        };
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .header("mcp-session-id", session_id)
            .body(Body::from(
                serde_json::to_string(&error_response).unwrap_or_default(),
            ))
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap()
            }));
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

    // Process response through interceptors (incoming: server → client)
    let response_message = JsonRpcMessage::Response(response.clone());
    let response_result = state
        .interceptor_manager
        .process_message_with_metadata(
            response_message,
            MessageDirection::Incoming,
            acdp_core::interceptor::InterceptionMetadata::default()
                .with_session_id(session_id.clone()),
        )
        .await
        .map_err(|e| {
            error!("Response interceptor processing failed: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let final_response = match response_result.message {
        JsonRpcMessage::Response(resp) => resp,
        _ => response, // Fallback to original if interceptor changed type
    };

    // Return as JSON response
    let json_body = match serde_json::to_string(&final_response) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to serialize response to JSON: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let response_builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .header("mcp-session-id", session_id)
        .body(Body::from(json_body));

    match response_builder {
        Ok(resp) => Ok(resp),
        Err(e) => {
            error!("Failed to build response: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
