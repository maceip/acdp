//! Per-Client Proxy for handling MCP client connections
//!
//! This module provides the proxy that handles traffic for a single MCP client connection,
//! forwarding messages to a backend MCP server with interception support.

use anyhow::{Context as _, Result};
use acdp_core::interceptor::{InterceptorManager, MessageDirection};
use acdp_core::messages::JsonRpcMessage;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tracing::{debug, error, info, warn};

use crate::acdp_server::{AppState, ClientId, ClientUpdate};
use acdp_transport::backend_connection::{create_backend_connection, BackendConnection};

#[cfg(feature = "llm")]
use acdp_llm::{LlmInterceptor, LlmService, RoutingMode};

/// Per-client proxy that handles traffic between an MCP client and backend server
pub struct ClientProxy {
    client_id: ClientId,
    /// Reader from MCP client
    client_reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    /// Writer to MCP client
    client_writer: BufWriter<tokio::net::tcp::OwnedWriteHalf>,
    /// Interceptor manager (LLM routing, transforms, etc.)
    interceptor_manager: Arc<InterceptorManager>,
    /// Shared app state (for UI updates)
    app_state: AppState,
    /// Backend connection (process or upstream server)
    backend: Box<dyn BackendConnection>,
    #[cfg(feature = "llm")]
    _llm_service: Option<Arc<LlmService>>,
    #[cfg(feature = "llm")]
    _routing_mode: RoutingMode,
}

impl ClientProxy {
    /// Create a new client proxy
    pub async fn new(
        client_id: ClientId,
        stream: TcpStream,
        app_state: AppState,
        backend_command: String,
        backend_url: Option<String>,
        backend_transport: Option<String>,
        #[cfg(feature = "llm")] llm_service: Option<Arc<LlmService>>,
        #[cfg(feature = "llm")] routing_mode: RoutingMode,
    ) -> Result<Self> {
        let (read_half, write_half) = stream.into_split();
        let client_reader = BufReader::new(read_half);
        let client_writer = BufWriter::new(write_half);

        // Create interceptor manager
        let interceptor_manager = Arc::new(InterceptorManager::new());

        #[cfg(feature = "llm")]
        // Add LLM interceptor if enabled
        if let Some(ref llm_service) = llm_service {
            let predictor = llm_service.tool_predictor();
            let routing_db = llm_service.database().routing_rules.clone();
            let session_manager = Some(llm_service.session_manager());
            let llm_interceptor =
                LlmInterceptor::new(predictor, routing_mode, routing_db, session_manager);
            interceptor_manager
                .add_interceptor(Arc::new(llm_interceptor))
                .await;
        }

        // Create backend connection (process or upstream server)
        let backend = create_backend_connection(
            backend_url.as_deref(),
            if backend_command.is_empty() {
                None
            } else {
                Some(&backend_command)
            },
            backend_transport.as_deref(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create backend connection: {}", e))?;

        info!(
            "Created backend connection for client {}: {:?}",
            client_id,
            backend.connection_info()
        );

        Ok(Self {
            client_id,
            client_reader,
            client_writer,
            interceptor_manager,
            app_state,
            backend,
            #[cfg(feature = "llm")]
            _llm_service: llm_service,
            #[cfg(feature = "llm")]
            _routing_mode: routing_mode,
        })
    }

    /// Main message loop - handles bidirectional message forwarding
    pub async fn run(&mut self) -> Result<()> {
        let mut restart_count = 0u32;
        const MAX_RESTARTS: u32 = 5;

        let client_reader = &mut self.client_reader;
        let client_writer = &mut self.client_writer;

        info!("Starting message loop for client {}", self.client_id);

        loop {
            tokio::select! {
                // Read from MCP client
                result = async {
                    let mut line = String::new();
                    client_reader.read_line(&mut line).await.map(|n| (n, line))
                } => {
                    match result {
                        Ok((0, _)) => {
                            debug!("Client {} disconnected (EOF)", self.client_id);
                            break Ok(());
                        }
                        Ok((_, line)) => {
                            if let Err(e) = Self::handle_client_message_internal(
                                &self.client_id,
                                &self.interceptor_manager,
                                &self.app_state,
                                &line,
                                &mut self.backend,
                            ).await {
                                error!("Error handling client message: {}", e);
                                break Err(e);
                            }
                        }
                        Err(e) => {
                            error!("Error reading from client {}: {}", self.client_id, e);
                            break Err(anyhow::anyhow!("Client read error: {}", e));
                        }
                    }
                }

                // Read from backend (process or upstream server)
                result = self.backend.recv() => {
                    match result {
                        Ok(Some(message)) => {
                            // Serialize message for forwarding
                            let json_str = serde_json::to_string(&message)?;
                            if let Err(e) = Self::handle_backend_message_internal(
                                &self.client_id,
                                &self.interceptor_manager,
                                &self.app_state,
                                &json_str,
                                client_writer,
                            ).await {
                                error!("Error handling backend message: {}", e);
                                break Err(e);
                            }
                        }
                        Ok(None) => {
                            debug!("Backend connection closed for client {}", self.client_id);
                            // For process connections, check if we should restart
                            let should_restart = {
                                let state = self.app_state.lock().await;
                                state.auto_restart_backend && restart_count < MAX_RESTARTS
                            };

                            if should_restart {
                                restart_count += 1;
                                warn!("Backend connection closed, restarting for client {} (attempt {}/{})",
                                    self.client_id, restart_count, MAX_RESTARTS);

                                // Notify UI
                                {
                                    let state = self.app_state.lock().await;
                                    if let Some(ref tx) = state.update_tx {
                                        let _ = tx.send(ClientUpdate::BackendRestart(
                                            self.client_id.clone(),
                                            restart_count,
                                        ));
                                    }
                                }

                                // Recreate backend connection
                                // TODO: Get config from app_state to recreate connection
                                return Err(anyhow::anyhow!("Backend connection closed - restart not yet implemented"));
                            } else {
                                break Ok(());
                            }
                        }
                        Err(e) => {
                            // For HTTP-SSE/HTTP-Stream, recv() returns an error
                            // This is expected - responses come via client's response handling
                            // We'll handle this case separately
                            if e.to_string().contains("HTTP-SSE receive not implemented") ||
                               e.to_string().contains("HTTP-Stream receive not implemented") {
                                // This is expected for upstream HTTP connections
                                // Responses are handled via the client's response mechanism
                                // Continue reading from client only
                                debug!("Backend uses HTTP transport - responses handled separately");
                                continue;
                            } else {
                                error!("Error receiving from backend for client {}: {}", self.client_id, e);
                                break Err(e);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Handle a message from the MCP client (outgoing: client → server)
    async fn handle_client_message_internal(
        client_id: &ClientId,
        interceptor_manager: &Arc<InterceptorManager>,
        app_state: &AppState,
        line: &str,
        backend: &mut Box<dyn BackendConnection>,
    ) -> Result<()> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        // Parse JSON-RPC message
        let message: JsonRpcMessage = serde_json::from_str(trimmed)
            .context("Failed to parse JSON-RPC message from client")?;

        debug!("Client {} → Backend: {:?}", client_id, message);

        // Run through interceptors (outgoing: client → server)
        let result = interceptor_manager
            .process_message(message, MessageDirection::Outgoing)
            .await
            .map_err(|e| anyhow::anyhow!("Interceptor error: {}", e))?;

        if result.block {
            warn!("Message blocked by interceptor for client {}", client_id);
            return Ok(());
        }

        // Forward to backend MCP server
        backend.send(result.message.clone()).await?;

        // Update stats (batch updates to reduce mutex contention)
        // Estimate bytes from message (approximate)
        let bytes = serde_json::to_string(&result.message)?.len();
        let request_start = std::time::Instant::now();
        {
            let mut state = app_state.lock().await;
            if let Some(conn) = state.clients.get_mut(client_id) {
                conn.total_requests += 1;
                conn.last_request_at = Some(request_start);
                conn.bytes_sent += bytes as u64;
            }
            state.metrics.total_requests += 1;
            state.metrics.bytes_sent += bytes as u64;

            // Notify UI (non-blocking, don't wait for send)
            if let Some(ref tx) = state.update_tx {
                let _ = tx.send(ClientUpdate::RequestSent(client_id.clone(), bytes));
            }
        }

        Ok(())
    }

    /// Handle a message from the backend MCP server (incoming: server → client)
    async fn handle_backend_message_internal(
        client_id: &ClientId,
        interceptor_manager: &Arc<InterceptorManager>,
        app_state: &AppState,
        line: &str,
        client_writer: &mut BufWriter<tokio::net::tcp::OwnedWriteHalf>,
    ) -> Result<()> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        // Parse JSON-RPC message
        let message: JsonRpcMessage = serde_json::from_str(trimmed)
            .context("Failed to parse JSON-RPC message from backend")?;

        debug!("Backend → Client {}: {:?}", client_id, message);

        // Run through interceptors (incoming: server → client)
        let result = interceptor_manager
            .process_message(message, MessageDirection::Incoming)
            .await
            .map_err(|e| anyhow::anyhow!("Interceptor error: {}", e))?;

        if result.block {
            warn!("Message blocked by interceptor for client {}", client_id);
            return Ok(());
        }

        // Forward to MCP client
        let modified = serde_json::to_string(&result.message)?;
        client_writer.write_all(modified.as_bytes()).await?;
        client_writer.write_all(b"\n").await?;
        client_writer.flush().await?;

        // Update stats (batch updates to reduce mutex contention)
        let bytes = modified.len();
        let response_time = std::time::Instant::now();
        {
            let mut state = app_state.lock().await;
            if let Some(conn) = state.clients.get_mut(client_id) {
                conn.total_responses += 1;
                conn.last_response_at = Some(response_time);
                conn.bytes_received += bytes as u64;
            }
            state.metrics.total_responses += 1;
            state.metrics.bytes_received += bytes as u64;

            // Notify UI (non-blocking, don't wait for send)
            if let Some(ref tx) = state.update_tx {
                let _ = tx.send(ClientUpdate::ResponseReceived(client_id.clone(), bytes));
            }
        }

        Ok(())
    }
}
