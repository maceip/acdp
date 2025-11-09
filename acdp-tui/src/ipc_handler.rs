//! IPC message handler for receiving gateway messages from proxies

use anyhow::Result;
use acdp_common::{ipc::IpcServer, IpcMessage, SessionMetrics};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::components::{ActivityItem, ActivityStatus, Client, ClientStatus, Server, ServerStatus};
use acdp_common::types::{ProxySession, SessionId};

/// IPC handler for gateway integration
pub struct IpcHandler {
    pub _message_receiver: mpsc::Receiver<AppStateUpdate>, // Keep for ownership but don't expose
    pub server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl IpcHandler {
    /// Create a new IPC handler and start the IPC server
    /// Returns both the handler and a receiver for updates
    pub async fn new(socket_path: String) -> Result<(Self, mpsc::Receiver<AppStateUpdate>)> {
        let (update_tx, update_rx) = mpsc::channel(100);

        // Start IPC server
        let ipc_server = IpcServer::bind(&socket_path).await?;
        info!("IPC server listening on {}", socket_path);

        let server_handle = tokio::spawn(Self::handle_ipc_server(ipc_server, update_tx));

        // Create a dummy receiver for the handler (won't be used)
        // The real update_rx is returned and stored in App
        let (dummy_tx, dummy_rx) = mpsc::channel(1);
        drop(dummy_tx); // Close immediately

        let handler = Self {
            _message_receiver: dummy_rx,
            server_handle: Some(server_handle),
        };

        Ok((handler, update_rx))
    }

    /// Handle IPC server connections
    async fn handle_ipc_server(server: IpcServer, update_tx: mpsc::Sender<AppStateUpdate>) {
        loop {
            match server.accept().await {
                Ok(mut connection) => {
                    info!("New IPC connection accepted");

                    // Spawn task to handle this connection
                    let tx = update_tx.clone();
                    tokio::spawn(async move {
                        loop {
                            match connection.receive_message().await {
                                Ok(Some(envelope)) => {
                                    if let Err(e) =
                                        Self::process_message(envelope.message, &tx).await
                                    {
                                        warn!("Failed to process IPC message: {}", e);
                                    }
                                }
                                Ok(None) => {
                                    debug!("IPC connection closed");
                                    break;
                                }
                                Err(e) => {
                                    warn!("Error receiving IPC message: {}", e);
                                    break;
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    warn!("Failed to accept IPC connection: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Process an IPC message and send updates
    async fn process_message(
        message: IpcMessage,
        update_tx: &mpsc::Sender<AppStateUpdate>,
    ) -> Result<()> {
        let update = match message {
            // Proxy messages
            IpcMessage::ProxyStarted(proxy_info) => {
                debug!("Proxy started: {} ({})", proxy_info.name, proxy_info.id.0);
                debug!("Proxy routing_mode: {:?}", proxy_info.routing_mode);
                debug!("Proxy model_status: {:?}", proxy_info.model_status);
                // Extract target address from target_command (first element)
                let target_address = if !proxy_info.target_command.is_empty() {
                    Some(proxy_info.target_command[0].clone())
                } else {
                    None
                };
                // Convert ProxyInfo to Server
                let server = Server {
                    id: proxy_info.id.0.to_string(),
                    name: proxy_info.name.clone(),
                    description: format!("Transport: {:?}", proxy_info.transport_type),
                    server_type: crate::components::ServerType::Proxy,
                    status: match proxy_info.status {
                        acdp_common::ProxyStatus::Starting => ServerStatus::Starting,
                        acdp_common::ProxyStatus::Running => ServerStatus::Running,
                        acdp_common::ProxyStatus::Stopped => ServerStatus::Stopped,
                        acdp_common::ProxyStatus::Error(_msg) => ServerStatus::Error,
                    },
                    requests_received: proxy_info.stats.successful_requests,
                    last_activity: chrono::Utc::now(), // TODO: use actual last activity time
                    routing_mode: proxy_info.routing_mode.clone(),
                    model_status: proxy_info.model_status.clone(),
                    target_address,
                };
                AppStateUpdate::ServerAdded(server)
            }

            IpcMessage::ProxyStopped(proxy_id) => {
                debug!("Proxy stopped: {}", proxy_id.0);
                AppStateUpdate::ServerRemoved(proxy_id.0.to_string())
            }

            IpcMessage::LogEntry(log_entry) => {
                debug!("Log entry: {:?} - {}", log_entry.level, log_entry.message);
                // Convert LogEntry to ActivityItem
                let activity = ActivityItem {
                    timestamp: log_entry.timestamp,
                    client: format!("proxy-{}", log_entry.proxy_id.0),
                    server: "Gateway".to_string(),
                    action: log_entry.message,
                    status: match log_entry.level {
                        acdp_common::LogLevel::Error => ActivityStatus::Failed,
                        acdp_common::LogLevel::Warning => ActivityStatus::Failed,
                        _ => ActivityStatus::Success,
                    },
                    metrics: None,
                };
                AppStateUpdate::ActivityAdded(activity)
            }

            IpcMessage::StatsUpdate(proxy_stats) => {
                debug!("Stats update for proxy: {}", proxy_stats.proxy_id.0);
                AppStateUpdate::ServerStatsUpdate {
                    server_id: proxy_stats.proxy_id.0.to_string(),
                    requests_received: proxy_stats.total_requests,
                }
            }

            // Client messages
            IpcMessage::ClientConnected(client_info) => {
                debug!(
                    "Client connected: {} ({})",
                    client_info.name, client_info.id.0
                );
                let client = Client {
                    id: client_info.id.0.to_string(),
                    name: client_info.name,
                    description: format!("{:?}", client_info.connection_type),
                    status: ClientStatus::Connected,
                    requests_sent: client_info.total_requests,
                    last_activity: client_info.last_activity,
                };
                AppStateUpdate::ClientAdded(client)
            }

            IpcMessage::ClientDisconnected(client_id) => {
                debug!("Client disconnected: {}", client_id.0);
                AppStateUpdate::ClientRemoved(client_id.0.to_string())
            }

            IpcMessage::ClientUpdated(client_info) => {
                debug!("Client updated: {}", client_info.id.0);
                AppStateUpdate::ClientUpdated {
                    client_id: client_info.id.0.to_string(),
                    requests_sent: client_info.total_requests,
                    last_activity: client_info.last_activity,
                }
            }

            // Server messages
            IpcMessage::ServerConnected(server_info) => {
                debug!(
                    "Server connected: {} ({})",
                    server_info.name, server_info.id.0
                );
                let server = Server {
                    id: server_info.id.0.to_string(),
                    name: server_info.name.clone(),
                    description: format!("{:?}", server_info.endpoint),
                    server_type: crate::components::ServerType::Server,
                    status: match server_info.status {
                        acdp_common::ServerStatus::Starting => ServerStatus::Starting,
                        acdp_common::ServerStatus::Running => ServerStatus::Running,
                        acdp_common::ServerStatus::Degraded(_) => ServerStatus::Degraded,
                        acdp_common::ServerStatus::Stopped => ServerStatus::Stopped,
                        acdp_common::ServerStatus::Error(_) => ServerStatus::Error,
                    },
                    requests_received: 0, // TODO: get from server_info
                    last_activity: chrono::Utc::now(),
                    routing_mode: None, // Not a proxy
                    model_status: None, // Not a proxy
                    target_address: Some(format!("{:?}", server_info.endpoint)),
                };
                AppStateUpdate::ServerAdded(server)
            }

            IpcMessage::ServerDisconnected(server_id) => {
                debug!("Server disconnected: {}", server_id.0);
                AppStateUpdate::ServerRemoved(server_id.0.to_string())
            }

            // Session messages
            IpcMessage::SessionStarted(session) => {
                debug!("Session started: {}", session.id.0);
                AppStateUpdate::SessionAdded(session)
            }

            IpcMessage::SessionEnded(session_id) => {
                debug!("Session ended: {}", session_id.0);
                AppStateUpdate::SessionRemoved(session_id)
            }
            IpcMessage::SessionStats(metrics) => {
                debug!(
                    "Session stats update received for session {}",
                    metrics.session_id.0
                );
                AppStateUpdate::SessionStats(metrics)
            }

            // Request/response messages
            IpcMessage::ClientRequest {
                client_id,
                request,
                session_id: _session_id,
            } => {
                debug!("Client request: {} -> {:?}", client_id.0, request.method);
                let activity = ActivityItem {
                    timestamp: chrono::Utc::now(),
                    client: client_id.0.to_string(),
                    server: "Gateway".to_string(),
                    action: format!("{} ({})", request.method, request.id),
                    status: ActivityStatus::Processing,
                    metrics: None,
                };
                AppStateUpdate::ActivityAdded(activity)
            }

            IpcMessage::ServerResponse {
                server_id,
                response,
                session_id: _session_id,
            } => {
                debug!("Server response: {} <- {:?}", server_id.0, response.id);
                let status = if response.error.is_some() {
                    ActivityStatus::Failed
                } else {
                    ActivityStatus::Success
                };
                let activity = ActivityItem {
                    timestamp: chrono::Utc::now(),
                    client: "Gateway".to_string(),
                    server: server_id.0.to_string(),
                    action: format!("Response for {}", response.id),
                    status,
                    metrics: None,
                };
                AppStateUpdate::ActivityAdded(activity)
            }

            IpcMessage::TuiQueryResponse {
                correlation_id,
                response,
                error,
                ttft_ms,
                tokens_per_sec,
                total_tokens,
                interceptor_delay_ms,
            } => {
                debug!("TUI query response received: {:?}", correlation_id);
                AppStateUpdate::QueryResponse {
                    correlation_id,
                    response,
                    error,
                    ttft_ms,
                    tokens_per_sec,
                    total_tokens,
                    interceptor_delay_ms,
                }
            }
            IpcMessage::RoutingModeChanged { proxy_id, mode } => {
                AppStateUpdate::RoutingModeChanged {
                    proxy_id: proxy_id.0.to_string(),
                    mode,
                }
            }

            // Additional important messages
            IpcMessage::InterceptorStats { proxy_id, stats } => {
                debug!(
                    "Interceptor stats for proxy {}: {} interceptors",
                    proxy_id.0,
                    stats.interceptors.len()
                );
                AppStateUpdate::InterceptorStats {
                    proxy_id: proxy_id.0.to_string(),
                    stats,
                }
            }

            IpcMessage::ServerUpdated(server_info) => {
                debug!("Server updated: {}", server_info.id.0);
                AppStateUpdate::ServerUpdated {
                    server_id: server_info.id.0.to_string(),
                    name: server_info.name,
                    status: match server_info.status {
                        acdp_common::ServerStatus::Starting => ServerStatus::Starting,
                        acdp_common::ServerStatus::Running => ServerStatus::Running,
                        acdp_common::ServerStatus::Degraded(_) => ServerStatus::Degraded,
                        acdp_common::ServerStatus::Stopped => ServerStatus::Stopped,
                        acdp_common::ServerStatus::Error(_) => ServerStatus::Error,
                    },
                }
            }

            IpcMessage::ServerHealthUpdate { server_id, metrics } => {
                debug!("Server health update: {}", server_id.0);
                AppStateUpdate::ServerHealthUpdate {
                    server_id: server_id.0.to_string(),
                    metrics,
                }
            }

            IpcMessage::SessionUpdated(session) => {
                debug!("Session updated: {}", session.id.0);
                AppStateUpdate::SessionUpdated(session)
            }

            IpcMessage::TransformationApplied {
                session_id,
                transformation,
            } => {
                debug!("Transformation applied in session {}", session_id.0);
                let activity = ActivityItem {
                    timestamp: chrono::Utc::now(),
                    client: format!("Session {}", session_id.0),
                    server: "Transform".to_string(),
                    action: format!(
                        "Applied: {} ({})",
                        transformation.rule_name, transformation.rule_id
                    ),
                    status: if transformation.success {
                        ActivityStatus::Success
                    } else {
                        ActivityStatus::Failed
                    },
                    metrics: None,
                };
                AppStateUpdate::ActivityAdded(activity)
            }

            IpcMessage::RoutingDecision(decision) => {
                debug!("Routing decision made: target {}", decision.target_server.0);
                let activity = ActivityItem {
                    timestamp: decision.timestamp,
                    client: format!("Session {}", decision.session_id.0),
                    server: "Router".to_string(),
                    action: format!(
                        "Route to {} - {}",
                        decision.target_server.0, decision.reason
                    ),
                    status: ActivityStatus::Success,
                    metrics: None,
                };
                AppStateUpdate::ActivityAdded(activity)
            }
            IpcMessage::SemanticPrediction {
                query,
                predicted_tool,
                confidence,
                actual_tool,
                success,
            } => {
                debug!(
                    "Semantic prediction: {} -> {} ({:.0}%)",
                    query,
                    predicted_tool,
                    confidence * 100.0
                );
                AppStateUpdate::SemanticPredictionUpdate {
                    query,
                    predicted_tool,
                    confidence,
                    actual_tool,
                    success,
                }
            }

            IpcMessage::GatewayStateUpdated(state) => {
                debug!(
                    "Gateway state updated: {} active sessions",
                    state.metrics.active_sessions
                );
                AppStateUpdate::GatewayStateUpdate {
                    active_proxies: 0, // Would need to count from actual proxy list
                    total_clients: 0,  // Would need to count from actual client list
                    total_servers: 0,  // Would need to count from actual server list
                    uptime_seconds: state.uptime_seconds,
                }
            }

            IpcMessage::GatewayMetrics(metrics) => {
                debug!(
                    "Gateway metrics: {:.1} requests/min",
                    metrics.requests_per_minute
                );
                AppStateUpdate::GatewayMetrics {
                    total_requests: 0,                                 // Would need to track separately
                    total_errors: (metrics.error_rate * 100.0) as u64, // Convert rate to approx count
                    avg_response_time_ms: metrics.average_latency_ms,
                    requests_per_second: metrics.requests_per_minute / 60.0,
                }
            }

            IpcMessage::MessageFlowUpdate(flow) => {
                debug!("Message flow update for session {}", flow.session_id.0);
                let activity = ActivityItem {
                    timestamp: chrono::Utc::now(),
                    client: format!("Session {}", flow.session_id.0),
                    server: "Gateway".to_string(),
                    action: format!("Message {}: {}", flow.id.0, flow.client_request.method),
                    status: if flow.server_response.is_some() {
                        if flow.server_response.as_ref().unwrap().error.is_some() {
                            ActivityStatus::Failed
                        } else {
                            ActivityStatus::Success
                        }
                    } else {
                        ActivityStatus::Processing
                    },
                    metrics: None,
                };
                AppStateUpdate::ActivityAdded(activity)
            }

            IpcMessage::Error { message, proxy_id } => {
                warn!("Error from proxy {:?}: {}", proxy_id, message);
                let activity = ActivityItem {
                    timestamp: chrono::Utc::now(),
                    client: proxy_id
                        .map(|p| format!("Proxy {}", p.0))
                        .unwrap_or_else(|| "System".to_string()),
                    server: "Gateway".to_string(),
                    action: format!("Error: {}", message),
                    status: ActivityStatus::Failed,
                    metrics: None,
                };
                AppStateUpdate::ActivityAdded(activity)
            }

            IpcMessage::Ping => {
                debug!("Received ping");
                // Could send Pong back if we had bidirectional communication here
                AppStateUpdate::PingReceived
            }

            IpcMessage::Pong => {
                debug!("Received pong");
                AppStateUpdate::PongReceived
            }

            // Messages that are not meant for TUI to handle (TUI -> Proxy direction)
            IpcMessage::GetStatus(_)
            | IpcMessage::GetLogs { .. }
            | IpcMessage::Shutdown(_)
            | IpcMessage::ToggleInterceptor { .. }
            | IpcMessage::TuiQuery { .. }
            | IpcMessage::TuiMcpRequest { .. }
            | IpcMessage::RoutingModeChange { .. }
            | IpcMessage::TransformationRules(_)
            | IpcMessage::RoutingRules(_) => {
                // These are outgoing messages from TUI, not incoming
                debug!("Ignoring outgoing message type");
                return Ok(());
            }
        };

        update_tx
            .send(update)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send update: {}", e))?;

        Ok(())
    }
}

/// State update messages from IPC handler to App
#[derive(Debug, Clone)]
pub enum AppStateUpdate {
    ClientAdded(Client),
    ClientRemoved(String), // client_id
    ClientUpdated {
        client_id: String,
        requests_sent: u64,
        last_activity: chrono::DateTime<chrono::Utc>,
    },
    ServerAdded(Server),
    ServerRemoved(String), // server_id
    ServerStatsUpdate {
        server_id: String,
        requests_received: u64,
    },
    ServerUpdated {
        server_id: String,
        name: String,
        status: ServerStatus,
    },
    ServerHealthUpdate {
        server_id: String,
        metrics: acdp_common::HealthMetrics,
    },
    ActivityAdded(ActivityItem),
    SessionAdded(ProxySession),
    SessionUpdated(ProxySession),
    SessionRemoved(SessionId),
    SessionStats(SessionMetrics),
    InterceptorStats {
        proxy_id: String,
        stats: acdp_common::InterceptorManagerInfo,
    },
    GatewayStateUpdate {
        active_proxies: usize,
        total_clients: usize,
        total_servers: usize,
        uptime_seconds: u64,
    },
    GatewayMetrics {
        total_requests: u64,
        total_errors: u64,
        avg_response_time_ms: f64,
        requests_per_second: f64,
    },
    QueryResponse {
        correlation_id: uuid::Uuid,
        response: String,
        error: Option<String>,
        ttft_ms: Option<f64>,
        tokens_per_sec: Option<f64>,
        total_tokens: Option<usize>,
        interceptor_delay_ms: Option<f64>,
    },
    RoutingModeChanged {
        proxy_id: String,
        mode: String,
    },
    SemanticPredictionUpdate {
        query: String,
        predicted_tool: String,
        confidence: f64,
        actual_tool: Option<String>,
        success: Option<bool>,
    },
    PingReceived,
    PongReceived,
}
