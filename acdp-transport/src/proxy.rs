use anyhow::Result;
use acdp_common::{IpcMessage, ProxyId, ProxyInfo, ProxyStats, ProxyStatus};
use std::process::Stdio;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info, warn};

use crate::buffered_ipc_client::BufferedIpcClient;
use crate::stdio_handler::StdioHandler;
use crate::transport_config::{InboundTransport, OutboundTransport, TransportConfig};

#[cfg(feature = "llm")]
use acdp_llm::{routing_modes::RoutingMode, LlmService};

pub struct MCPProxy {
    id: ProxyId,
    name: String,
    transport_config: TransportConfig,
    stats: Arc<Mutex<ProxyStats>>,
    shutdown_tx: Option<broadcast::Sender<()>>,
    #[cfg(feature = "llm")]
    llm_service: Option<Arc<LlmService>>,
    #[cfg(feature = "llm")]
    routing_mode: RoutingMode,
}

impl MCPProxy {
    pub async fn new(
        id: ProxyId,
        name: String,
        transport_config: TransportConfig,
        #[cfg(feature = "llm")] llm_service: Option<Arc<LlmService>>,
        #[cfg(feature = "llm")] routing_mode: RoutingMode,
    ) -> Result<Self> {
        let mut stats = ProxyStats::default();
        stats.proxy_id = id.clone();

        Ok(Self {
            id,
            name,
            transport_config,
            stats: Arc::new(Mutex::new(stats)),
            shutdown_tx: None,
            #[cfg(feature = "llm")]
            llm_service,
            #[cfg(feature = "llm")]
            routing_mode,
        })
    }

    pub async fn start(&mut self, ipc_socket_path: Option<&str>) -> Result<()> {
        info!("Starting MCP proxy: {}", self.name);

        // Create shutdown channel
        let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);
        let shutdown_tx_clone = shutdown_tx.clone();
        self.shutdown_tx = Some(shutdown_tx);

        let monitor_socket = ipc_socket_path.map(|path| path.to_string());

        // Create buffered IPC client (unless monitor is explicitly disabled)
        let buffered_client = if let Some(socket_path) = ipc_socket_path {
            info!(
                "Creating buffered IPC client for monitor at {}",
                socket_path
            );
            Some(Arc::new(
                BufferedIpcClient::new(socket_path.to_string()).await,
            ))
        } else {
            info!("Running in standalone mode (monitor disabled)");
            None
        };

        // Send proxy started message
        if let Some(ref client) = buffered_client {
            #[cfg(feature = "llm")]
            let model_status = if let Some(ref llm) = self.llm_service {
                Some(llm.model_status().await.to_string())
            } else {
                None
            };
            #[cfg(not(feature = "llm"))]
            let model_status = None;

            let proxy_info = ProxyInfo {
                id: self.id.clone(),
                name: self.name.clone(),
                listen_address: "proxy".to_string(),
                target_command: vec![self.transport_config.display_target()],
                status: ProxyStatus::Starting,
                stats: self.stats.lock().await.clone(),
                transport_type: self.transport_config.transport_type(),
                #[cfg(feature = "llm")]
                routing_mode: Some(self.routing_mode.to_string()),
                #[cfg(not(feature = "llm"))]
                routing_mode: None,
                model_status,
            };

            if let Err(e) = client.send(IpcMessage::ProxyStarted(proxy_info)).await {
                warn!("Failed to send proxy started message: {}", e);
            }
        }

        // Handle transport-specific logic
        match (
            &self.transport_config.inbound,
            &self.transport_config.outbound,
        ) {
            (InboundTransport::Stdio, OutboundTransport::Stdio { .. }) => {
                // Auto-restart configuration
                const MAX_RESTARTS: u32 = 5;
                const AUTO_RESTART_ENABLED: bool = true; // Can be made configurable later
                let mut restart_count = 0u32;

                // Create STDIO handler once (reused across restarts)
                let mut handler = StdioHandler::new(
                    self.id.clone(),
                    self.stats.clone(),
                    buffered_client.clone(),
                    #[cfg(feature = "llm")]
                    self.llm_service.clone(),
                    #[cfg(feature = "llm")]
                    self.routing_mode,
                    monitor_socket.clone(),
                )
                .await?;

                // Spawn a task to monitor model status changes and send updates
                #[cfg(feature = "llm")]
                if let (Some(ref client), Some(ref llm_service)) =
                    (&buffered_client, &self.llm_service)
                {
                    let client = client.clone();
                    let llm_service = llm_service.clone();
                    let proxy_id = self.id.clone();
                    let proxy_name = self.name.clone();
                    let transport_type = self.transport_config.transport_type();
                    let routing_mode = self.routing_mode;
                    let stats = self.stats.clone();

                    tokio::spawn(async move {
                        // Wait a bit for model to start loading
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                        let mut last_status = String::new();
                        loop {
                            let current_status = llm_service.model_status().await.to_string();

                            // Only send update if status changed
                            if current_status != last_status {
                                let proxy_info = ProxyInfo {
                                    id: proxy_id.clone(),
                                    name: proxy_name.clone(),
                                    listen_address: "proxy".to_string(),
                                    target_command: vec!["stdio".to_string()],
                                    status: acdp_common::ProxyStatus::Running,
                                    stats: stats.lock().await.clone(),
                                    transport_type: transport_type.clone(),
                                    routing_mode: Some(routing_mode.to_string()),
                                    model_status: Some(current_status.clone()),
                                };

                                if let Err(e) = client
                                    .send(acdp_common::IpcMessage::ProxyStarted(proxy_info))
                                    .await
                                {
                                    tracing::warn!("Failed to send model status update: {}", e);
                                    break;
                                }

                                last_status = current_status;
                            }

                            // Check every 2 seconds
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        }
                    });
                }

                // Outer loop for auto-restart
                let result = loop {
                    // Start MCP server process
                    let mut child = match self.start_mcp_server().await {
                        Ok(child) => child,
                        Err(e) => {
                            error!("Failed to spawn backend server: {}", e);
                            if restart_count < MAX_RESTARTS && AUTO_RESTART_ENABLED {
                                restart_count += 1;
                                warn!(
                                    "Retrying backend spawn (attempt {}/{})",
                                    restart_count, MAX_RESTARTS
                                );
                                // Exponential backoff: 1s, 2s, 4s, 8s, 16s
                                let backoff_secs = 1u64 << (restart_count - 1).min(4);
                                tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs))
                                    .await;
                                continue;
                            } else {
                                break Err(e);
                            }
                        }
                    };

                    // Send proxy running message (child process started successfully)
                    if let Some(ref client) = buffered_client {
                        #[cfg(feature = "llm")]
                        let model_status = if let Some(ref llm) = self.llm_service {
                            Some(llm.model_status().await.to_string())
                        } else {
                            None
                        };
                        #[cfg(not(feature = "llm"))]
                        let model_status = None;

                        let proxy_info = ProxyInfo {
                            id: self.id.clone(),
                            name: self.name.clone(),
                            listen_address: "proxy".to_string(),
                            target_command: vec![self.transport_config.display_target()],
                            status: ProxyStatus::Running,
                            stats: self.stats.lock().await.clone(),
                            transport_type: self.transport_config.transport_type(),
                            #[cfg(feature = "llm")]
                            routing_mode: Some(self.routing_mode.to_string()),
                            #[cfg(not(feature = "llm"))]
                            routing_mode: None,
                            model_status,
                        };
                        if let Err(e) = client.send(IpcMessage::ProxyStarted(proxy_info)).await {
                            warn!("Failed to send proxy running status: {}", e);
                        }
                    }

                    // Create a new shutdown receiver for this iteration
                    let iteration_shutdown_rx = shutdown_tx_clone.subscribe();

                    // Handle STDIO communication
                    // handle_communication monitors child.wait() internally and will return an error
                    // if the child crashes (non-zero exit status), allowing us to detect and restart
                    let communication_result = handler
                        .handle_communication(&mut child, iteration_shutdown_rx)
                        .await;

                    // handle_communication now returns an error if the child crashes (non-zero exit)
                    // So we can detect crashes and restart accordingly
                    match communication_result {
                        Ok(()) => {
                            // Normal shutdown (child exited with 0 or shutdown signal received)
                            break Ok(());
                        }
                        Err(e) => {
                            // Check if this is a crash error (contains "crashed" or "exited with status")
                            let error_msg = e.to_string();
                            let is_crash = error_msg.contains("crashed")
                                || error_msg.contains("exited with status");

                            if is_crash && AUTO_RESTART_ENABLED && restart_count < MAX_RESTARTS {
                                restart_count += 1;
                                warn!(
                                    "Backend crashed, restarting (attempt {}/{}): {}",
                                    restart_count, MAX_RESTARTS, e
                                );
                                // Exponential backoff: 1s, 2s, 4s, 8s, 16s
                                let backoff_secs = 1u64 << (restart_count - 1).min(4);
                                tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs))
                                    .await;
                                continue; // Restart the loop
                            } else if is_crash {
                                error!("Backend crashed (max restarts reached): {}", e);
                                break Err(e);
                            } else {
                                // Other communication error - also try to restart
                                if AUTO_RESTART_ENABLED && restart_count < MAX_RESTARTS {
                                    restart_count += 1;
                                    warn!("Communication error, restarting backend (attempt {}/{}): {}", restart_count, MAX_RESTARTS, e);
                                    let backoff_secs = 1u64 << (restart_count - 1).min(4);
                                    tokio::time::sleep(tokio::time::Duration::from_secs(
                                        backoff_secs,
                                    ))
                                    .await;
                                    continue; // Restart the loop
                                } else {
                                    // Clean up
                                    if let Err(kill_err) = child.kill().await {
                                        warn!("Failed to kill MCP server process: {}", kill_err);
                                    }
                                    break Err(e);
                                }
                            }
                        }
                    }
                };

                // Clean up
                info!("Proxy {} shutting down", self.name);

                // Send proxy stopped message and shutdown buffered client
                if let Some(client) = buffered_client {
                    if let Err(e) = client.send(IpcMessage::ProxyStopped(self.id.clone())).await {
                        warn!("Failed to send proxy stopped message: {}", e);
                    }
                    // Take the client out of the Arc and shutdown
                    if let Ok(client) = Arc::try_unwrap(client) {
                        client.shutdown().await;
                    }
                }

                result
            }
            (InboundTransport::HttpSse { bind_addr }, OutboundTransport::HttpSse { url, .. }) => {
                // For HTTP-SSE, start an HTTP server that proxies to the upstream URL
                use crate::http_sse_server::{start_server, HttpSseServerState};
                use acdp_core::{McpClient, TransportConfig as McpTransportConfig};
                use std::net::SocketAddr;

                // Use provided bind address or default to 127.0.0.1:9000
                // The upstream URL is separate from the bind address
                let bind_addr_str = bind_addr.as_str();
                let bind_addr: SocketAddr = bind_addr_str.parse().map_err(|e| {
                    anyhow::anyhow!("Failed to parse bind address '{}': {}", bind_addr_str, e)
                })?;

                let listener = TcpListener::bind(bind_addr).await?;
                let actual_addr = listener.local_addr()?;

                info!(
                    "Starting HTTP-SSE proxy server on {} (configured {}, upstream: {})",
                    actual_addr, bind_addr, url
                );

                // Create interceptor manager
                #[cfg(feature = "llm")]
                let interceptor_manager = if let Some(ref llm_service) = self.llm_service {
                    use crate::interceptors::{
                        TransformInterceptor, TransformOperation, TransformRule,
                    };
                    use acdp_core::interceptor::InterceptorManager;
                    use serde_json::json;

                    let manager = Arc::new(InterceptorManager::new());

                    // Add transform interceptor if not bypass mode
                    if self.routing_mode != acdp_llm::RoutingMode::Bypass {
                        let transformer = TransformInterceptor::new();
                        transformer
                            .add_rule(TransformRule {
                                name: "replace-santa-with-timestamp".to_string(),
                                method_pattern: "tools/call".to_string(),
                                path: "arguments.message".to_string(),
                                operation: TransformOperation::Set {
                                    value: json!(chrono::Utc::now().to_rfc3339()),
                                },
                            })
                            .await;
                        manager.add_interceptor(Arc::new(transformer)).await;
                    }

                    // Add LLM interceptor
                    use acdp_llm::LlmInterceptor;
                    let predictor = llm_service.tool_predictor();
                    let database = llm_service.database();
                    let routing_db = database.routing_rules.clone();
                    let session_manager = llm_service.session_manager();
                    let llm_interceptor = Arc::new(LlmInterceptor::with_interceptor_manager(
                        predictor,
                        self.routing_mode,
                        routing_db,
                        manager.clone(),
                        Some(session_manager),
                    ));
                    manager.add_interceptor(llm_interceptor.clone()).await;
                    manager
                } else {
                    Arc::new(acdp_core::interceptor::InterceptorManager::new())
                };
                #[cfg(not(feature = "llm"))]
                let interceptor_manager =
                    Arc::new(acdp_core::interceptor::InterceptorManager::new());

                // Connect to upstream MCP server
                let mcp_config = McpTransportConfig::http_sse(&url)?;
                let mut upstream_client = McpClient::with_defaults(mcp_config).await?;
                upstream_client.set_interceptor_manager(interceptor_manager.clone());

                // Initialize connection
                let client_impl =
                    acdp_core::messages::Implementation::new("mcp-proxy", env!("CARGO_PKG_VERSION"));
                upstream_client.connect(client_impl).await?;
                info!("Connected to upstream MCP server at {}", url);

                // Create server state
                let state = HttpSseServerState::new(interceptor_manager, upstream_client);

                // Send proxy running message
                if let Some(ref client) = buffered_client {
                    #[cfg(feature = "llm")]
                    let model_status = if let Some(ref llm) = self.llm_service {
                        Some(llm.model_status().await.to_string())
                    } else {
                        None
                    };
                    #[cfg(not(feature = "llm"))]
                    let model_status = None;

                    let proxy_info = ProxyInfo {
                        id: self.id.clone(),
                        name: self.name.clone(),
                        listen_address: actual_addr.to_string(),
                        target_command: vec![url.clone()],
                        status: ProxyStatus::Running,
                        stats: self.stats.lock().await.clone(),
                        transport_type: self.transport_config.transport_type(),
                        #[cfg(feature = "llm")]
                        routing_mode: Some(self.routing_mode.to_string()),
                        #[cfg(not(feature = "llm"))]
                        routing_mode: None,
                        model_status,
                    };
                    if let Err(e) = client.send(IpcMessage::ProxyStarted(proxy_info)).await {
                        warn!("Failed to send proxy running status: {}", e);
                    }
                }

                // Start HTTP-SSE server (this blocks until shutdown)
                info!("HTTP-SSE server listening on {}", actual_addr);
                let result = start_server(listener, state).await;

                // Clean up
                info!("HTTP proxy {} shutting down", self.name);

                // Send proxy stopped message and shutdown buffered client
                if let Some(client) = buffered_client {
                    if let Err(e) = client.send(IpcMessage::ProxyStopped(self.id.clone())).await {
                        warn!("Failed to send proxy stopped message: {}", e);
                    }
                    if let Ok(client) = Arc::try_unwrap(client) {
                        client.shutdown().await;
                    }
                }

                result
            }
            (
                InboundTransport::HttpStream { bind_addr },
                OutboundTransport::HttpStream { url, .. },
            ) => {
                // For HTTP-Stream, start an HTTP server that proxies to the upstream URL
                use crate::http_stream_server::{start_server, HttpStreamServerState};
                use acdp_core::{McpClient, TransportConfig as McpTransportConfig};
                use std::net::SocketAddr;

                // Use provided bind address or default to 127.0.0.1:9000
                // The upstream URL is separate from the bind address
                let bind_addr_str = bind_addr.as_str();
                let bind_addr: SocketAddr = bind_addr_str.parse().map_err(|e| {
                    anyhow::anyhow!("Failed to parse bind address '{}': {}", bind_addr_str, e)
                })?;

                let listener = TcpListener::bind(bind_addr).await?;
                let actual_addr = listener.local_addr()?;

                info!(
                    "Starting HTTP-Stream proxy server on {} (configured {}, upstream: {})",
                    actual_addr, bind_addr, url
                );

                // Create interceptor manager
                #[cfg(feature = "llm")]
                let interceptor_manager = if let Some(ref llm_service) = self.llm_service {
                    use crate::interceptors::{
                        TransformInterceptor, TransformOperation, TransformRule,
                    };
                    use acdp_core::interceptor::InterceptorManager;
                    use serde_json::json;

                    let manager = Arc::new(InterceptorManager::new());

                    // Add transform interceptor if not bypass mode
                    if self.routing_mode != acdp_llm::RoutingMode::Bypass {
                        let transformer = TransformInterceptor::new();
                        transformer
                            .add_rule(TransformRule {
                                name: "replace-santa-with-timestamp".to_string(),
                                method_pattern: "tools/call".to_string(),
                                path: "arguments.message".to_string(),
                                operation: TransformOperation::Set {
                                    value: json!(chrono::Utc::now().to_rfc3339()),
                                },
                            })
                            .await;
                        manager.add_interceptor(Arc::new(transformer)).await;
                    }

                    // Add LLM interceptor
                    use acdp_llm::LlmInterceptor;
                    let predictor = llm_service.tool_predictor();
                    let database = llm_service.database();
                    let routing_db = database.routing_rules.clone();
                    let session_manager = llm_service.session_manager();
                    let llm_interceptor = Arc::new(LlmInterceptor::with_interceptor_manager(
                        predictor,
                        self.routing_mode,
                        routing_db,
                        manager.clone(),
                        Some(session_manager),
                    ));
                    manager.add_interceptor(llm_interceptor.clone()).await;
                    manager
                } else {
                    Arc::new(acdp_core::interceptor::InterceptorManager::new())
                };
                #[cfg(not(feature = "llm"))]
                let interceptor_manager =
                    Arc::new(acdp_core::interceptor::InterceptorManager::new());

                // Connect to upstream MCP server
                let mcp_config = McpTransportConfig::http_stream(&url)?;
                let mut upstream_client = McpClient::with_defaults(mcp_config).await?;
                upstream_client.set_interceptor_manager(interceptor_manager.clone());

                // Initialize connection
                let client_impl =
                    acdp_core::messages::Implementation::new("mcp-proxy", env!("CARGO_PKG_VERSION"));
                upstream_client.connect(client_impl).await?;
                info!("Connected to upstream MCP server at {}", url);

                // Create server state
                let state = HttpStreamServerState::new(interceptor_manager, upstream_client);

                // Send proxy running message
                if let Some(ref client) = buffered_client {
                    #[cfg(feature = "llm")]
                    let model_status = if let Some(ref llm) = self.llm_service {
                        Some(llm.model_status().await.to_string())
                    } else {
                        None
                    };
                    #[cfg(not(feature = "llm"))]
                    let model_status = None;

                    let proxy_info = ProxyInfo {
                        id: self.id.clone(),
                        name: self.name.clone(),
                        listen_address: actual_addr.to_string(),
                        target_command: vec![url.clone()],
                        status: ProxyStatus::Running,
                        stats: self.stats.lock().await.clone(),
                        transport_type: self.transport_config.transport_type(),
                        #[cfg(feature = "llm")]
                        routing_mode: Some(self.routing_mode.to_string()),
                        #[cfg(not(feature = "llm"))]
                        routing_mode: None,
                        model_status,
                    };
                    if let Err(e) = client.send(IpcMessage::ProxyStarted(proxy_info)).await {
                        warn!("Failed to send proxy running status: {}", e);
                    }
                }

                // Start HTTP-Stream server (this blocks until shutdown)
                info!("HTTP-Stream server listening on {}", actual_addr);
                let result = start_server(listener, state).await;

                // Clean up
                info!("HTTP-Stream proxy {} shutting down", self.name);

                // Send proxy stopped message and shutdown buffered client
                if let Some(client) = buffered_client {
                    if let Err(e) = client.send(IpcMessage::ProxyStopped(self.id.clone())).await {
                        warn!("Failed to send proxy stopped message: {}", e);
                    }
                    if let Ok(client) = Arc::try_unwrap(client) {
                        client.shutdown().await;
                    }
                }

                result
            }
            (inbound, outbound) => {
                return Err(anyhow::anyhow!(format!(
                    "Unsupported transport pairing: inbound {:?} → outbound {:?}",
                    inbound, outbound
                )));
            }
        }
    }

    async fn start_mcp_server(&self) -> Result<Child> {
        use crate::transport_config::OutboundTransport;

        let (command, use_shell) = match &self.transport_config.outbound {
            OutboundTransport::Stdio { command, use_shell } => (command, use_shell),
            _ => {
                return Err(anyhow::anyhow!(
                    "start_mcp_server only works for stdio outbound transport"
                ))
            }
        };

        if command.is_empty() {
            return Err(anyhow::anyhow!("No command specified"));
        }

        let child = if *use_shell {
            // Use shell to execute the command
            Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
        } else {
            // Parse command and arguments
            let parts: Vec<&str> = command.split_whitespace().collect();
            if parts.is_empty() {
                return Err(anyhow::anyhow!("Empty command"));
            }

            let mut cmd = Command::new(parts[0]);
            if parts.len() > 1 {
                cmd.args(&parts[1..]);
            }

            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
        };

        info!("Started MCP server process: {}", command);
        Ok(child)
    }
}
