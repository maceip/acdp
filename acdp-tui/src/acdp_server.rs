//! MCP Server Listener for accepting incoming MCP client connections
//!
//! This module provides the TCP server that accepts MCP client connections
//! and spawns a proxy task for each connection.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::client_proxy::ClientProxy;

/// Unique identifier for a connected MCP client
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientId(String);

impl ClientId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ClientId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Information about a connected client
#[derive(Debug, Clone)]
pub struct ClientConnection {
    pub id: ClientId,
    pub connected_at: Instant,
    pub total_requests: u64,
    pub total_responses: u64,
    pub last_request_at: Option<Instant>,
    pub last_response_at: Option<Instant>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

impl ClientConnection {
    pub fn new(id: ClientId) -> Self {
        Self {
            id,
            connected_at: Instant::now(),
            total_requests: 0,
            total_responses: 0,
            last_request_at: None,
            last_response_at: None,
            bytes_sent: 0,
            bytes_received: 0,
        }
    }

    /// Get connection duration
    pub fn duration(&self) -> std::time::Duration {
        self.connected_at.elapsed()
    }

    /// Check if connection is healthy (has recent activity)
    pub fn is_healthy(&self, timeout_secs: u64) -> bool {
        let timeout = std::time::Duration::from_secs(timeout_secs);
        if let Some(last_activity) = self.last_request_at.or(self.last_response_at) {
            last_activity.elapsed() < timeout
        } else {
            // New connection, consider healthy
            self.connected_at.elapsed() < timeout
        }
    }
}

/// Shared application state for UI updates
pub type AppState = Arc<Mutex<AppStateInner>>;

/// Server health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerHealth {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Server metrics
#[derive(Debug, Clone)]
pub struct ServerMetrics {
    pub total_connections: u64,
    pub active_connections: usize,
    pub total_requests: u64,
    pub total_responses: u64,
    pub total_errors: u64,
    pub requests_per_second: f64,
    pub average_latency_ms: f64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub uptime_seconds: u64,
    /// Peak requests per second (for performance tracking)
    pub peak_rps: f64,
    /// Average response latency (exponential moving average)
    pub ema_latency_ms: f64,
    /// Total messages processed
    pub total_messages: u64,
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self {
            total_connections: 0,
            active_connections: 0,
            total_requests: 0,
            total_responses: 0,
            total_errors: 0,
            requests_per_second: 0.0,
            average_latency_ms: 0.0,
            bytes_sent: 0,
            bytes_received: 0,
            uptime_seconds: 0,
            peak_rps: 0.0,
            ema_latency_ms: 0.0,
            total_messages: 0,
        }
    }
}

pub struct AppStateInner {
    /// Active client connections
    pub clients: HashMap<ClientId, ClientConnection>,
    /// Channel for sending UI updates
    pub update_tx: Option<tokio::sync::mpsc::UnboundedSender<ClientUpdate>>,
    /// Backend command to spawn per client (if backend_url not set)
    pub backend_command: String,
    /// Backend upstream URL (if set, used instead of backend_command)
    pub backend_url: Option<String>,
    /// Backend transport type (auto-detected from URL if not set)
    pub backend_transport: Option<String>,
    /// Maximum concurrent clients
    pub max_clients: usize,
    /// Connection timeout in seconds
    pub connection_timeout_secs: u64,
    /// Whether to auto-restart backend processes
    pub auto_restart_backend: bool,
    /// Server metrics
    pub metrics: ServerMetrics,
    /// Server start time
    pub started_at: Instant,
    /// Last metrics update time (for RPS calculation)
    pub last_metrics_update: Instant,
    /// Request count at last update (for RPS calculation)
    pub last_request_count: u64,
    #[cfg(feature = "llm")]
    /// LLM service (if available)
    pub llm_service: Option<Arc<acdp_llm::LlmService>>,
    #[cfg(feature = "llm")]
    /// Routing mode for LLM interceptor
    pub routing_mode: acdp_llm::RoutingMode,
}

/// Update message for UI
#[derive(Debug, Clone)]
pub enum ClientUpdate {
    ClientConnected(ClientId, std::net::SocketAddr),
    ClientDisconnected(ClientId),
    RequestSent(ClientId, usize),      // client_id, bytes
    ResponseReceived(ClientId, usize), // client_id, bytes
    BackendError(ClientId, String),
    BackendRestart(ClientId, u32),
    MetricsUpdate(ServerMetrics),
}

/// MCP Server Listener that accepts incoming client connections
pub struct McpServerListener {
    /// TCP listener
    listener: TcpListener,
    /// Shared application state (for UI updates)
    app_state: AppState,
    /// Bind address
    bind_addr: String,
    /// Metrics update interval handle
    _metrics_handle: Option<tokio::task::JoinHandle<()>>,
}

impl McpServerListener {
    /// Create a new MCP server listener
    pub async fn new(bind_addr: &str, app_state: AppState) -> Result<Self> {
        let listener = TcpListener::bind(bind_addr).await?;
        info!("MCP server listening on {}", bind_addr);

        // Start metrics update task
        let metrics_app_state = app_state.clone();
        let metrics_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                interval.tick().await;
                let mut state = metrics_app_state.lock().await;
                let health = state.update_metrics();

                // Send metrics update to UI
                if let Some(ref tx) = state.update_tx {
                    let _ = tx.send(ClientUpdate::MetricsUpdate(state.metrics.clone()));
                }

                // Log health status if degraded or unhealthy
                match health {
                    ServerHealth::Degraded => {
                        warn!("MCP server health: DEGRADED ({} active connections, {:.1}% error rate)",
                            state.metrics.active_connections,
                            if state.metrics.total_requests > 0 {
                                state.metrics.total_errors as f64 / state.metrics.total_requests as f64 * 100.0
                            } else {
                                0.0
                            }
                        );
                    }
                    ServerHealth::Unhealthy => {
                        error!("MCP server health: UNHEALTHY ({} active connections, {:.1}% error rate)",
                            state.metrics.active_connections,
                            if state.metrics.total_requests > 0 {
                                state.metrics.total_errors as f64 / state.metrics.total_requests as f64 * 100.0
                            } else {
                                0.0
                            }
                        );
                    }
                    ServerHealth::Healthy => {
                        // Only log at debug level for healthy status
                        debug!(
                            "MCP server health: HEALTHY ({} active connections)",
                            state.metrics.active_connections
                        );
                    }
                }
            }
        });

        Ok(Self {
            listener,
            app_state,
            bind_addr: bind_addr.to_string(),
            _metrics_handle: Some(metrics_handle),
        })
    }

    /// Main accept loop - runs forever accepting connections
    pub async fn run(&self) -> Result<()> {
        info!("MCP server accept loop started on {}", self.bind_addr);

        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New MCP client connected from {}", addr);

                    // Check connection limit
                    let current_clients = {
                        let state = self.app_state.lock().await;
                        state.clients.len()
                    };

                    if current_clients >= {
                        let state = self.app_state.lock().await;
                        state.max_clients
                    } {
                        warn!(
                            "Connection limit reached ({}), rejecting connection from {}",
                            current_clients, addr
                        );
                        // Close the connection
                        drop(stream);
                        continue;
                    }

                    let client_id = ClientId::new();
                    let app_state = self.app_state.clone();

                    // Register client connection
                    {
                        let mut state = app_state.lock().await;
                        state
                            .clients
                            .insert(client_id.clone(), ClientConnection::new(client_id.clone()));
                        state.metrics.total_connections += 1;
                        state.metrics.active_connections = state.clients.len();

                        // Notify UI
                        if let Some(ref tx) = state.update_tx {
                            let _ = tx.send(ClientUpdate::ClientConnected(client_id.clone(), addr));
                        }
                    }

                    // Spawn a task to handle this client
                    let client_id_clone = client_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            Self::handle_client(client_id_clone.clone(), stream, app_state.clone())
                                .await
                        {
                            error!("Client {} error: {}", client_id_clone, e);
                        }

                        // Clean up when client disconnects
                        {
                            let mut state = app_state.lock().await;
                            if let Some(conn) = state.clients.remove(&client_id_clone) {
                                // Update metrics
                                state.metrics.active_connections = state.clients.len();
                                state.metrics.bytes_sent += conn.bytes_sent;
                                state.metrics.bytes_received += conn.bytes_received;
                            }

                            // Notify UI
                            if let Some(ref tx) = state.update_tx {
                                let _ = tx.send(ClientUpdate::ClientDisconnected(
                                    client_id_clone.clone(),
                                ));
                            }
                        }

                        info!("Client {} disconnected", client_id_clone);
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    // Continue accepting connections even if one fails
                }
            }
        }
    }

    /// Handle a single client connection
    async fn handle_client(
        client_id: ClientId,
        stream: TcpStream,
        app_state: AppState,
    ) -> Result<()> {
        // Get config from app state
        let (backend_command, backend_url, backend_transport) = {
            let state = app_state.lock().await;
            (
                state.backend_command.clone(),
                state.backend_url.clone(),
                state.backend_transport.clone(),
            )
        };

        #[cfg(feature = "llm")]
        let (llm_service, routing_mode) = {
            let state = app_state.lock().await;
            (state.llm_service.clone(), state.routing_mode)
        };

        // Create per-client proxy
        let mut proxy = ClientProxy::new(
            client_id.clone(),
            stream,
            app_state,
            backend_command,
            backend_url,
            backend_transport,
            #[cfg(feature = "llm")]
            llm_service,
            #[cfg(feature = "llm")]
            routing_mode,
        )
        .await?;

        // Start proxying traffic
        proxy.run().await?;

        Ok(())
    }

    /// Get the bind address
    pub fn bind_addr(&self) -> &str {
        &self.bind_addr
    }

    /// Get current server health status
    pub async fn health(&self) -> ServerHealth {
        let state = self.app_state.lock().await;
        state.health()
    }

    /// Get current server metrics
    pub async fn metrics(&self) -> ServerMetrics {
        let state = self.app_state.lock().await;
        state.metrics.clone()
    }
}

impl AppStateInner {
    /// Update metrics and check health
    pub fn update_metrics(&mut self) -> ServerHealth {
        self.metrics.active_connections = self.clients.len();
        self.metrics.uptime_seconds = self.started_at.elapsed().as_secs();
        self.metrics.total_messages = self.metrics.total_requests + self.metrics.total_responses;

        // Calculate requests per second (exponential moving average)
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_metrics_update).as_secs_f64();

        if elapsed > 0.0 {
            let requests_since_last = self
                .metrics
                .total_requests
                .saturating_sub(self.last_request_count);
            let current_rps = requests_since_last as f64 / elapsed;

            // Exponential moving average: new_rps = alpha * current + (1 - alpha) * old
            // Using alpha = 0.3 for responsiveness
            const ALPHA: f64 = 0.3;
            self.metrics.requests_per_second =
                ALPHA * current_rps + (1.0 - ALPHA) * self.metrics.requests_per_second;

            // Track peak RPS
            if self.metrics.requests_per_second > self.metrics.peak_rps {
                self.metrics.peak_rps = self.metrics.requests_per_second;
            }

            self.last_metrics_update = now;
            self.last_request_count = self.metrics.total_requests;
        }

        // Check health based on error rate and connection count
        let error_rate = if self.metrics.total_requests > 0 {
            self.metrics.total_errors as f64 / self.metrics.total_requests as f64
        } else {
            0.0
        };

        if error_rate > 0.5 || self.metrics.active_connections >= self.max_clients {
            ServerHealth::Unhealthy
        } else if error_rate > 0.1
            || self.metrics.active_connections as f64 > self.max_clients as f64 * 0.8
        {
            ServerHealth::Degraded
        } else {
            ServerHealth::Healthy
        }
    }

    /// Get current server health
    pub fn health(&self) -> ServerHealth {
        let error_rate = if self.metrics.total_requests > 0 {
            self.metrics.total_errors as f64 / self.metrics.total_requests as f64
        } else {
            0.0
        };

        if error_rate > 0.5 || self.clients.len() >= self.max_clients {
            ServerHealth::Unhealthy
        } else if error_rate > 0.1 || self.clients.len() as f64 > self.max_clients as f64 * 0.8 {
            ServerHealth::Degraded
        } else {
            ServerHealth::Healthy
        }
    }
}

impl Default for AppStateInner {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            clients: HashMap::new(),
            update_tx: None,
            backend_command: String::new(), // Must be configured explicitly
            backend_url: None,
            backend_transport: None,
            max_clients: 100,
            connection_timeout_secs: 300,
            auto_restart_backend: false,
            metrics: ServerMetrics::default(),
            started_at: now,
            last_metrics_update: now,
            last_request_count: 0,
            #[cfg(feature = "llm")]
            llm_service: None,
            #[cfg(feature = "llm")]
            routing_mode: acdp_llm::RoutingMode::Bypass,
        }
    }
}
