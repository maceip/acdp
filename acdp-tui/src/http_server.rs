//! HTTP+SSE Server for accepting incoming MCP client connections
//!
//! This module provides an HTTP server that accepts MCP client connections
//! via HTTP POST requests and streams responses via Server-Sent Events (SSE).

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
use acdp_core::interceptor::{InterceptorManager, MessageDirection};
use acdp_core::messages::JsonRpcMessage;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::acdp_server::{AppState, ClientId, ClientUpdate};
use acdp_core::messages::JsonRpcResponse;
#[cfg(feature = "llm")]
use acdp_llm::LlmInterceptor;

/// HTTP session information
#[derive(Clone)]
struct HttpSession {
    client_id: ClientId,
    response_tx: broadcast::Sender<JsonRpcMessage>,
    request_tx: mpsc::UnboundedSender<JsonRpcMessage>,
    interceptor_manager: Arc<InterceptorManager>,
}

/// Shared state for HTTP server
#[derive(Clone)]
struct HttpServerState {
    app_state: AppState,
    sessions: Arc<Mutex<HashMap<String, HttpSession>>>,
}

/// Create HTTP+SSE server router
pub fn create_http_server_router(app_state: AppState) -> Router {
    let server_state = HttpServerState {
        app_state,
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    Router::new()
        .route("/mcp", post(handle_mcp_request))
        .route("/sse/:session_id", get(handle_sse_stream))
        .with_state(server_state)
}

/// Handle MCP HTTP POST request
async fn handle_mcp_request(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response<Body>, StatusCode> {
    debug!("Received HTTP MCP request: {:?}", body);

    // Extract or create session ID
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Parse JSON-RPC message
    let message: JsonRpcMessage = match serde_json::from_value(body) {
        Ok(msg) => msg,
        Err(e) => {
            error!("Failed to parse JSON-RPC message: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Get or create session
    let session = {
        let mut sessions = state.sessions.lock().await;

        if let Some(session) = sessions.get(&session_id) {
            session.clone()
        } else {
            // New session - create client proxy
            let client_id = ClientId::new();
            let (response_tx, _response_rx) = broadcast::channel(100);
            let (request_tx, request_rx) = mpsc::unbounded_channel();

            // Create interceptor manager
            let interceptor_manager = Arc::new(InterceptorManager::new());

            #[cfg(feature = "llm")]
            {
                let app_state = state.app_state.lock().await;
                if let Some(ref llm_service) = app_state.llm_service {
                    let predictor = llm_service.tool_predictor();
                    let routing_db = llm_service.database().routing_rules.clone();
                    let session_manager = Some(llm_service.session_manager());
                    let llm_interceptor = LlmInterceptor::new(
                        predictor,
                        app_state.routing_mode,
                        routing_db,
                        session_manager,
                    );
                    interceptor_manager
                        .add_interceptor(Arc::new(llm_interceptor))
                        .await;
                } else {
                    debug!("LLM service not available for HTTP session {}. Continuing without LLM routing.", session_id);
                }
            }

            // Register client in app state
            {
                let mut app_state = state.app_state.lock().await;
                app_state.clients.insert(
                    client_id.clone(),
                    crate::acdp_server::ClientConnection::new(client_id.clone()),
                );
                app_state.metrics.total_connections += 1;
                app_state.metrics.active_connections = app_state.clients.len();

                // Notify UI
                if let Some(ref update_tx) = app_state.update_tx {
                    let _ = update_tx.send(ClientUpdate::ClientConnected(
                        client_id.clone(),
                        std::net::SocketAddr::from(([0, 0, 0, 0], 0)), // HTTP doesn't have direct socket addr
                    ));
                }
            }

            // Create session
            let session = HttpSession {
                client_id: client_id.clone(),
                response_tx: response_tx.clone(),
                request_tx: request_tx.clone(),
                interceptor_manager: interceptor_manager.clone(),
            };
            sessions.insert(session_id.clone(), session.clone());

            // Spawn backend proxy task
            let app_state_clone = state.app_state.clone();
            let (backend_command, backend_url, backend_transport) = {
                let app_state = state.app_state.lock().await;
                (
                    app_state.backend_command.clone(),
                    app_state.backend_url.clone(),
                    app_state.backend_transport.clone(),
                )
            };

            // Check if backend is configured
            if backend_url.is_none() && backend_command.is_empty() {
                error!("Cannot create HTTP session: neither backend_command nor backend_url is configured");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }

            let session_id_clone = session_id.clone();
            let client_id_clone = client_id.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_http_backend_proxy(
                    client_id_clone,
                    session_id_clone,
                    backend_command,
                    backend_url,
                    backend_transport,
                    app_state_clone,
                    interceptor_manager,
                    request_rx,
                    response_tx,
                )
                .await
                {
                    error!("HTTP backend proxy error: {}", e);
                }
            });

            session
        }
    };

    // Process message through interceptor pipeline
    match message {
        JsonRpcMessage::Request(req) => {
            // Process through interceptors (outgoing: client → server)
            match session
                .interceptor_manager
                .process_message(
                    JsonRpcMessage::Request(req.clone()),
                    MessageDirection::Outgoing,
                )
                .await
            {
                Ok(result) => {
                    if result.block {
                        warn!("Message blocked by interceptor for session {}", session_id);
                        // Send error response for blocked message
                        let error_response = JsonRpcMessage::Response(JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: None,
                            error: Some(acdp_core::messages::JsonRpcError {
                                code: -32000,
                                message: "Message blocked by interceptor".to_string(),
                                data: None,
                            }),
                        });
                        let _ = session.response_tx.send(error_response);
                    } else {
                        // Forward intercepted message to backend proxy
                        debug!("Request processed through interceptors: {}", req.method);
                        if session.request_tx.send(result.message).is_err() {
                            error!(
                                "Failed to forward request to backend proxy for session {}",
                                session_id
                            );
                            // Send error response
                            let error_response = JsonRpcMessage::Response(JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id: req.id,
                                result: None,
                                error: Some(acdp_core::messages::JsonRpcError {
                                    code: -32603,
                                    message: "Failed to forward request to backend".to_string(),
                                    data: None,
                                }),
                            });
                            let _ = session.response_tx.send(error_response);
                        } else {
                            // Update stats
                            {
                                let mut app_state = state.app_state.lock().await;
                                if let Some(conn) = app_state.clients.get_mut(&session.client_id) {
                                    conn.total_requests += 1;
                                    conn.last_request_at = Some(std::time::Instant::now());
                                }
                                app_state.metrics.total_requests += 1;

                                // Notify UI
                                if let Some(ref update_tx) = app_state.update_tx {
                                    let _ = update_tx.send(ClientUpdate::RequestSent(
                                        session.client_id.clone(),
                                        0,
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Interceptor error: {}", e);
                    // Send error response
                    let error_response = JsonRpcMessage::Response(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(acdp_core::messages::JsonRpcError {
                            code: -32603,
                            message: "Internal error".to_string(),
                            data: Some(serde_json::json!({"error": e.to_string()})),
                        }),
                    });
                    let _ = session.response_tx.send(error_response);
                }
            }
        }
        JsonRpcMessage::Notification(_) => {
            // Notifications don't get responses
            match Response::builder()
                .status(StatusCode::ACCEPTED)
                .header("Mcp-Session-Id", session_id)
                .body(Body::empty())
            {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    error!("Failed to build notification response: {}", e);
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }
        _ => {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    // Return session ID in header
    match Response::builder()
        .status(StatusCode::OK)
        .header("Mcp-Session-Id", session_id)
        .header("Content-Type", "application/json")
        .body(Body::empty())
    {
        Ok(resp) => Ok(resp),
        Err(e) => {
            error!("Failed to build response: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Handle SSE stream for a session
async fn handle_sse_stream(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    debug!("SSE stream requested for session: {}", session_id);

    let sessions = state.sessions.lock().await;
    let session = sessions.get(&session_id).ok_or(StatusCode::NOT_FOUND)?;

    // Create SSE stream from broadcast channel
    let mut rx = session.response_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(message) => {
                    let json_str = match serde_json::to_string(&message) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Failed to serialize message: {}", e);
                            continue;
                        }
                    };
                    yield Ok(Event::default().data(json_str));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("SSE stream closed for session {}", session_id);
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!("SSE stream lagged, skipped {} messages for session {}", skipped, session_id);
                    continue;
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

/// Handle HTTP backend proxy (similar to ClientProxy but for HTTP)
async fn handle_http_backend_proxy(
    client_id: ClientId,
    _session_id: String,
    backend_command: String,
    backend_url: Option<String>,
    backend_transport: Option<String>,
    app_state: AppState,
    interceptor_manager: Arc<InterceptorManager>,
    mut request_rx: mpsc::UnboundedReceiver<JsonRpcMessage>,
    response_tx: broadcast::Sender<JsonRpcMessage>,
) -> Result<()> {
    use acdp_transport::backend_connection::create_backend_connection;

    let mut restart_count = 0u32;
    const MAX_RESTARTS: u32 = 5;

    loop {
        // Create backend connection (process or upstream server)
        let mut backend = match create_backend_connection(
            backend_url.as_deref(),
            if backend_command.is_empty() {
                None
            } else {
                Some(&backend_command)
            },
            backend_transport.as_deref(),
        )
        .await
        {
            Ok(conn) => {
                info!(
                    "Created backend connection for HTTP client {}: {:?}",
                    client_id,
                    conn.connection_info()
                );
                conn
            }
            Err(e) => {
                error!(
                    "Failed to create backend connection for HTTP client {}: {}",
                    client_id, e
                );
                {
                    let mut state = app_state.lock().await;
                    state.metrics.total_errors += 1;
                    if let Some(ref tx) = state.update_tx {
                        let _ = tx.send(ClientUpdate::BackendError(
                            client_id.clone(),
                            format!("Failed to create backend connection: {}", e),
                        ));
                    }
                }
                return Err(anyhow::anyhow!(
                    "Failed to create backend connection: {}",
                    e
                ));
            }
        };

        info!(
            "HTTP backend proxy started for client {} (restart attempt {})",
            client_id, restart_count
        );

        // Main message loop
        loop {
            tokio::select! {
                // Read HTTP requests and forward to backend
                request = request_rx.recv() => {
                    match request {
                        Some(message) => {
                            // Process through interceptors (outgoing: client → server)
                            match interceptor_manager.process_message(message, MessageDirection::Outgoing).await {
                                Ok(result) => {
                                    if result.block {
                                        warn!("Message blocked by interceptor for HTTP client {}", client_id);
                                        continue;
                                    }

                                    // Clone message for logging before sending
                                    let message_to_send = result.message.clone();
                                    debug!("HTTP request forwarded to backend: {:?}", message_to_send);

                                    // Forward to backend
                                    if let Err(e) = backend.send(result.message).await {
                                        error!("Failed to send to backend: {}", e);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    error!("Interceptor error: {}", e);
                                    break;
                                }
                            }
                        }
                        None => {
                            debug!("HTTP request channel closed for client {}", client_id);
                            return Ok(());
                        }
                    }
                }

                // Read backend responses and forward via SSE
                result = backend.recv() => {
                    match result {
                        Ok(Some(message)) => {
                            debug!("Backend → HTTP Client {}: {:?}", client_id, message);

                            // Run through interceptors (incoming: server → client)
                            match interceptor_manager.process_message(message, MessageDirection::Incoming).await {
                                Ok(result) => {
                                    if result.block {
                                        warn!("Message blocked by interceptor for HTTP client {}", client_id);
                                        continue;
                                    }

                                    // Send via SSE
                                    if response_tx.send(result.message.clone()).is_err() {
                                        warn!("Failed to send response via SSE for client {}", client_id);
                                    }

                                    // Update stats
                                    let bytes = serde_json::to_string(&result.message)?.len();
                                    {
                                        let mut state = app_state.lock().await;
                                        if let Some(conn) = state.clients.get_mut(&client_id) {
                                            conn.total_responses += 1;
                                            conn.last_response_at = Some(std::time::Instant::now());
                                            conn.bytes_received += bytes as u64;
                                        }
                                        state.metrics.total_responses += 1;
                                        state.metrics.bytes_received += bytes as u64;

                                        // Notify UI
                                        if let Some(ref tx) = state.update_tx {
                                            let _ = tx.send(ClientUpdate::ResponseReceived(client_id.clone(), bytes));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Interceptor error for HTTP client {}: {}", client_id, e);
                                }
                            }
                        }
                        Ok(None) => {
                            debug!("Backend connection closed for HTTP client {}", client_id);
                            // Check if we should restart
                            let should_restart = {
                                let state = app_state.lock().await;
                                state.auto_restart_backend && restart_count < MAX_RESTARTS
                            };

                            if should_restart {
                                restart_count += 1;
                                warn!("Backend connection closed, restarting for HTTP client {} (attempt {}/{})",
                                    client_id, restart_count, MAX_RESTARTS);

                                // Notify UI
                                {
                                    let state = app_state.lock().await;
                                    if let Some(ref tx) = state.update_tx {
                                        let _ = tx.send(ClientUpdate::BackendRestart(
                                            client_id.clone(),
                                            restart_count,
                                        ));
                                    }
                                }

                                // Break out of select to restart the backend
                                break;
                            } else {
                                // Notify UI of backend failure
                                {
                                    let mut state = app_state.lock().await;
                                    state.metrics.total_errors += 1;
                                    if let Some(ref tx) = state.update_tx {
                                        let _ = tx.send(ClientUpdate::BackendError(
                                            client_id.clone(),
                                            "Backend connection closed".to_string(),
                                        ));
                                    }
                                }
                                return Ok(()); // Connection closed normally
                            }
                        }
                        Err(e) => {
                            // For HTTP-SSE/HTTP-Stream, recv() returns an error
                            // This is expected - responses come via client's response handling
                            if e.to_string().contains("HTTP-SSE receive not implemented") ||
                               e.to_string().contains("HTTP-Stream receive not implemented") {
                                // This is expected for upstream HTTP connections
                                // Responses are handled via the client's response mechanism
                                // Continue reading from request channel only
                                debug!("Backend uses HTTP transport - responses handled separately");
                                continue;
                            } else {
                                error!("Error receiving from backend for HTTP client {}: {}", client_id, e);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Check if we should restart
        let should_restart = {
            let state = app_state.lock().await;
            state.auto_restart_backend && restart_count < MAX_RESTARTS
        };

        if !should_restart {
            info!("HTTP backend proxy ended for client {}", client_id);
            return Ok(());
        }
    }
}

/// Start HTTP server (with optional TLS/ACME support)
pub async fn start_http_server(
    bind_addr: &str,
    app_state: AppState,
    config: &crate::config::Config,
) -> Result<tokio::task::JoinHandle<()>> {
    let router = create_http_server_router(app_state);

    // Check if TLS is enabled
    if config.enable_tls() {
        info!(
            "Starting HTTP+SSE MCP server with TLS/ACME on {}",
            bind_addr
        );
        start_https_server_with_acme(bind_addr, router, config).await
    } else {
        info!("Starting HTTP+SSE MCP server (plain HTTP) on {}", bind_addr);

        // Validate bind address format
        if bind_addr.is_empty() {
            return Err(anyhow::anyhow!("HTTP bind address is empty"));
        }

        // Try to parse as SocketAddr to validate format
        if bind_addr.parse::<std::net::SocketAddr>().is_err() {
            return Err(anyhow::anyhow!("Invalid HTTP bind address format: '{}'. Expected 'host:port' (e.g., '127.0.0.1:8080')", bind_addr));
        }

        let listener = tokio::net::TcpListener::bind(bind_addr).await?;

        let handle = tokio::spawn(async move {
            use axum::serve;
            if let Err(e) = serve(listener, router).await {
                error!("HTTP server error: {}", e);
            }
        });

        Ok(handle)
    }
}

/// Start HTTPS server with ACME certificate management
async fn start_https_server_with_acme(
    bind_addr: &str,
    router: axum::Router,
    config: &crate::config::Config,
) -> Result<tokio::task::JoinHandle<()>> {
    use rustls_acme::{caches::DirCache, AcmeConfig};
    use std::net::SocketAddr;

    let tls_config = config.tls_config();

    if tls_config.domains.is_empty() {
        return Err(anyhow::anyhow!("TLS enabled but no domains configured"));
    }

    // Create certificate directory
    let cert_dir = std::path::PathBuf::from(&tls_config.cert_dir);
    tokio::fs::create_dir_all(&cert_dir)
        .await
        .context("Failed to create certificate directory")?;

    // Build contact list
    let contacts: Vec<String> = if let Some(email) = &tls_config.email {
        vec![format!("mailto:{}", email)]
    } else {
        vec!["mailto:admin@example.com".to_string()]
    };

    // Set cache directory
    let cache_dir = cert_dir.join("acme-cache");
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .context("Failed to create ACME cache directory")?;

    // Build ACME configuration
    let acme_config = AcmeConfig::new(tls_config.domains.clone())
        .contact(contacts.iter().map(|e| format!("mailto:{}", e)))
        .cache_option(Some(cache_dir.clone()).map(DirCache::new))
        .directory_lets_encrypt(!tls_config.use_staging);

    // Configure challenge type
    match tls_config.challenge_type.as_str() {
        "dns-01" => {
            info!("DNS-01 challenge requested");
            // DNS-01 challenge requires custom DNS provider implementation
            // For now, we'll use HTTP-01 as fallback if DNS provider is not configured
            if tls_config.dns_provider.is_none() {
                warn!("DNS-01 challenge requested but no DNS provider configured. Falling back to HTTP-01.");
            } else if let Some(ref dns_provider) = tls_config.dns_provider {
                // TODO: Implement DNS-01 challenge with provider
                warn!(
                    "DNS-01 challenge with provider {} not yet fully implemented. Using HTTP-01.",
                    dns_provider
                );
            } else {
                warn!("DNS-01 challenge requested but no DNS provider configured. Using HTTP-01.");
            }
            // HTTP-01 is the default, so we don't need to explicitly set it
        }
        "http-01" | _ => {
            info!("Using HTTP-01 challenge for ACME");
            // HTTP-01 is the default challenge type
        }
    }

    if tls_config.use_staging {
        info!("Using Let's Encrypt staging environment");
    } else {
        info!("Using Let's Encrypt production environment");
    }

    // Get ACME state and acceptor
    let mut state = acme_config.state();
    let acceptor = state.axum_acceptor(state.default_rustls_config());

    // Validate and parse bind address
    if bind_addr.is_empty() {
        return Err(anyhow::anyhow!("HTTPS bind address is empty"));
    }

    let socket_addr: SocketAddr = bind_addr.parse().context(format!(
        "Invalid HTTPS bind address format: '{}'. Expected 'host:port' (e.g., '127.0.0.1:8443')",
        bind_addr
    ))?;

    info!(
        "HTTPS+SSE MCP server listening on {} (TLS with ACME)",
        bind_addr
    );

    // Spawn server task with ACME event handler
    let handle = tokio::spawn(async move {
        // Spawn ACME event handler task
        tokio::spawn(async move {
            loop {
                match state.next().await {
                    Some(Ok(event)) => {
                        debug!("ACME event: {:?}", event);
                    }
                    Some(Err(err)) => {
                        error!("ACME error: {:?}", err);
                    }
                    None => {
                        warn!("ACME state stream ended");
                        break;
                    }
                }
            }
        });

        // Start the HTTPS server
        if let Err(e) = axum_server::bind(socket_addr)
            .acceptor(acceptor)
            .serve(router.into_make_service())
            .await
        {
            error!("HTTPS server error: {}", e);
        }
    });

    Ok(handle)
}
