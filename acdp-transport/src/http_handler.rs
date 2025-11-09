use crate::ipc_server::{ProxyIpcServer, TuiQueryCompletion, TuiQueryJob};
use anyhow::Result;
use acdp_common::{IpcMessage, LogEntry, LogLevel, ProxyId, ProxyStats, SessionId};
use acdp_core::interceptor::InterceptorManager;
use acdp_core::{McpClient, TransportConfig as McpTransportConfig};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
#[cfg(feature = "llm")]
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tracing::{error, info, warn};

use crate::buffered_ipc_client::BufferedIpcClient;
use crate::transport_config::{OutboundTransport, TransportConfig};
#[cfg(feature = "llm")]
use acdp_llm::{LlmInterceptor, RoutingMode, SessionManager};

pub struct HttpHandler {
    proxy_id: ProxyId,
    #[allow(dead_code)] // Reserved for future HTTP stats tracking
    stats: Arc<Mutex<ProxyStats>>,
    ipc_client: Option<Arc<BufferedIpcClient>>,
    interceptor_manager: Arc<InterceptorManager>,
    session_id: SessionId,
    #[cfg(feature = "llm")]
    llm_service: Option<Arc<acdp_llm::LlmService>>,
    #[cfg(feature = "llm")]
    llm_interceptor: Option<Arc<LlmInterceptor>>,
    #[cfg(feature = "llm")]
    current_routing_mode: RoutingMode,
    #[cfg(feature = "llm")]
    routing_mode_rx: Option<mpsc::Receiver<RoutingMode>>,
    query_rx: Option<mpsc::Receiver<TuiQueryJob>>,
    /// Cached list of available tools from the MCP server
    available_tools: Arc<Mutex<Option<Vec<String>>>>,
}

impl HttpHandler {
    pub async fn new(
        proxy_id: ProxyId,
        stats: Arc<Mutex<ProxyStats>>,
        ipc_client: Option<Arc<BufferedIpcClient>>,
        #[cfg(feature = "llm")] llm_service: Option<Arc<acdp_llm::LlmService>>,
        #[cfg(feature = "llm")] routing_mode: RoutingMode,
        monitor_socket: Option<String>,
    ) -> Result<Self> {
        #[cfg(feature = "llm")]
        let (interceptor_manager, llm_interceptor_handle) =
            Self::build_interceptor_manager(llm_service.clone(), routing_mode).await?;
        #[cfg(not(feature = "llm"))]
        let interceptor_manager = Self::build_interceptor_manager().await?;
        let (query_tx, query_rx) = if ipc_client.is_some() {
            let (tx, rx) = mpsc::channel(32);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        #[cfg(feature = "llm")]
        let (routing_tx, routing_rx) = if ipc_client.is_some() {
            let (tx, rx) = mpsc::channel(8);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        if let Some(ref client) = ipc_client {
            let ipc_socket = monitor_socket
                .clone()
                .or_else(|| std::env::var("MCP_IPC_SOCKET").ok())
                .unwrap_or_else(|| "/tmp/mcp-monitor.sock".into());
            let ipc_server = Arc::new(ProxyIpcServer::new(
                client.clone(),
                query_tx.clone(),
                #[cfg(feature = "llm")]
                routing_tx.clone(),
            ));

            if let Err(e) = ipc_server.start(ipc_socket).await {
                warn!("Failed to start IPC server: {}", e);
            }
        }

        Ok(Self {
            proxy_id,
            stats,
            ipc_client,
            interceptor_manager,
            session_id: SessionId::new(),
            #[cfg(feature = "llm")]
            llm_service,
            #[cfg(feature = "llm")]
            llm_interceptor: llm_interceptor_handle,
            #[cfg(feature = "llm")]
            current_routing_mode: routing_mode,
            #[cfg(feature = "llm")]
            routing_mode_rx: routing_rx,
            query_rx,
            available_tools: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn handle_communication(
        &mut self,
        transport_config: &TransportConfig,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<()> {
        info!("Starting HTTP handler");

        #[cfg(feature = "llm")]
        {
            if self.llm_service.is_some() {
                info!("LLM service attached to HTTP handler (semantic routing enabled)");
            } else {
                info!("LLM service not available; HTTP handler will run without predictions");
            }
        }

        let interceptor_names = self.interceptor_manager.list_interceptors().await;
        info!(
            "HTTP handler registered {} interceptors",
            interceptor_names.len()
        );

        // Convert our TransportConfig to mcp-core's TransportConfig
        let mcp_config = match &transport_config.outbound {
            OutboundTransport::HttpSse { url, .. } => {
                info!("Connecting to HTTP+SSE server at {}", url);
                McpTransportConfig::http_sse(&url)?
            }
            OutboundTransport::HttpStream { url, .. } => {
                info!("Connecting to HTTP streaming server at {}", url);
                McpTransportConfig::http_stream(&url)?
            }
            _ => return Err(anyhow::anyhow!("HttpHandler only supports HTTP transports")),
        };

        // Retry connection with exponential backoff
        const MAX_RETRIES: u32 = 5;
        const INITIAL_BACKOFF_SECS: u64 = 1;
        let mut retry_count = 0u32;
        let mut client = loop {
            match McpClient::with_defaults(mcp_config.clone()).await {
                Ok(c) => break c,
                Err(e) => {
                    if retry_count < MAX_RETRIES {
                        retry_count += 1;
                        let backoff_secs =
                            INITIAL_BACKOFF_SECS * (1u64 << (retry_count - 1).min(4));
                        warn!(
                            "Failed to create MCP client (attempt {}/{}), retrying in {}s: {}",
                            retry_count, MAX_RETRIES, backoff_secs, e
                        );
                        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                        continue;
                    } else {
                        error!(
                            "Failed to create MCP client after {} attempts: {}",
                            MAX_RETRIES, e
                        );
                        return Err(anyhow::anyhow!(
                            "Failed to connect to upstream HTTP server after {} retries: {}",
                            MAX_RETRIES,
                            e
                        ));
                    }
                }
            }
        };

        client.set_interceptor_manager(self.interceptor_manager.clone());
        client.set_session_id(Some(self.session_id.0.to_string()));

        // Initialize the client connection with retry
        let client_impl =
            acdp_core::messages::Implementation::new("mcp-proxy", env!("CARGO_PKG_VERSION"));
        retry_count = 0;
        loop {
            match client.connect(client_impl.clone()).await {
                Ok(_server_info) => break,
                Err(e) => {
                    if retry_count < MAX_RETRIES {
                        retry_count += 1;
                        let backoff_secs =
                            INITIAL_BACKOFF_SECS * (1u64 << (retry_count - 1).min(4));
                        warn!(
                            "Failed to connect to HTTP server (attempt {}/{}), retrying in {}s: {}",
                            retry_count, MAX_RETRIES, backoff_secs, e
                        );
                        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                        continue;
                    } else {
                        error!(
                            "Failed to connect to HTTP server after {} attempts: {}",
                            MAX_RETRIES, e
                        );
                        return Err(anyhow::anyhow!(
                            "Failed to connect to upstream HTTP server after {} retries: {}",
                            MAX_RETRIES,
                            e
                        ));
                    }
                }
            }
        }

        // Log connection success
        self.log(LogLevel::Info, "Connected to HTTP server".to_string())
            .await;

        let mut query_rx = self.query_rx.take();

        loop {
            #[cfg(feature = "llm")]
            self.process_pending_routing_updates().await;
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal");
                    break;
                }
                job = async {
                    match &mut query_rx {
                        Some(rx) => rx.recv().await,
                        None => None,
                    }
                } => {
                    if let Some(job) = job {
                        if let Err(e) = self.process_tui_query(job, &mut client).await {
                            error!("Failed to process HTTP query: {}", e);
                        }
                    }
                }
            }
        }

        self.query_rx = query_rx;

        info!("HTTP handler shutting down");
        Ok(())
    }

    pub fn interceptor_manager(&self) -> Arc<InterceptorManager> {
        self.interceptor_manager.clone()
    }

    #[cfg(feature = "llm")]
    async fn base_interceptor_manager(routing_mode: RoutingMode) -> Arc<InterceptorManager> {
        use crate::interceptors::{TransformInterceptor, TransformOperation, TransformRule};
        use serde_json::json;

        let manager = Arc::new(InterceptorManager::new());

        // Demo Transform interceptor - only add in non-bypass modes
        if routing_mode != RoutingMode::Bypass {
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

        manager
    }

    #[cfg(not(feature = "llm"))]
    async fn base_interceptor_manager() -> Arc<InterceptorManager> {
        Arc::new(InterceptorManager::new())
    }

    #[cfg(feature = "llm")]
    async fn build_interceptor_manager(
        llm_service: Option<Arc<acdp_llm::LlmService>>,
        routing_mode: RoutingMode,
    ) -> Result<(Arc<InterceptorManager>, Option<Arc<LlmInterceptor>>)> {
        let manager = Self::base_interceptor_manager(routing_mode).await;
        let mut llm_interceptor = None;

        if let Some(service) = llm_service {
            let session_manager = service.session_manager();
            match Self::add_llm_interceptor(
                manager.clone(),
                service.clone(),
                routing_mode,
                session_manager,
            )
            .await
            {
                Ok(handle) => {
                    llm_interceptor = Some(handle);
                }
                Err(e) => {
                    warn!("Failed to initialize LLM interceptor: {}", e);
                }
            }
        } else {
            warn!("LLM service not available; HTTP handler interceptor chain will run without predictions");
        }

        Ok((manager, llm_interceptor))
    }

    #[cfg(not(feature = "llm"))]
    async fn build_interceptor_manager() -> Result<Arc<InterceptorManager>> {
        Ok(Self::base_interceptor_manager().await)
    }

    #[cfg(feature = "llm")]
    async fn add_llm_interceptor(
        manager: Arc<InterceptorManager>,
        llm_service: Arc<acdp_llm::LlmService>,
        routing_mode: RoutingMode,
        session_manager: Arc<SessionManager>,
    ) -> Result<Arc<LlmInterceptor>> {
        info!("Initializing LLM interceptor for HTTP handler");

        let predictor = llm_service.tool_predictor();
        let database = llm_service.database();
        let routing_db = database.routing_rules.clone();

        let llm_interceptor = Arc::new(LlmInterceptor::with_interceptor_manager(
            predictor,
            routing_mode,
            routing_db,
            manager.clone(),
            Some(session_manager),
        ));

        manager.add_interceptor(llm_interceptor.clone()).await;
        info!("LLM interceptor registered for HTTP handler");
        Ok(llm_interceptor)
    }

    async fn log(&self, level: LogLevel, message: String) {
        if let Some(ref client) = self.ipc_client {
            let log_entry = LogEntry::new(level, message, self.proxy_id.clone());
            if let Err(e) = client.send(IpcMessage::LogEntry(log_entry)).await {
                warn!("Failed to send log entry: {}", e);
            }
        }
    }

    /// Get available tools from the MCP server, caching the result
    async fn get_available_tool(&self, client: &mut McpClient) -> Result<Option<String>> {
        // Check cache first
        {
            let tools = self.available_tools.lock().await;
            if let Some(ref tool_list) = *tools {
                if !tool_list.is_empty() {
                    return Ok(Some(tool_list[0].clone()));
                }
            }
        }

        // Query tools/list from the server
        let response = match client.send_request("tools/list", json!({})).await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Failed to query tools/list: {}", e);
                return Ok(None);
            }
        };

        // Parse tools from response
        let tools = if let Some(ref result) = response.result {
            if let Some(tools_array) = result.get("tools").and_then(|t| t.as_array()) {
                tools_array
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(|n| n.as_str()))
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Cache the result
        {
            let mut cached = self.available_tools.lock().await;
            *cached = if tools.is_empty() {
                None
            } else {
                Some(tools.clone())
            };
        }

        Ok(tools.first().cloned())
    }

    async fn process_tui_query(&self, job: TuiQueryJob, client: &mut McpClient) -> Result<()> {
        use acdp_core::interceptor::MessageDirection;
        use acdp_core::messages::{JsonRpcMessage, JsonRpcRequest, RequestId};

        // Wait for client to be ready (with timeout)
        let timeout = Duration::from_secs(5);
        let start_wait = Instant::now();
        while !client.is_ready().await && start_wait.elapsed() < timeout {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if !client.is_ready().await {
            self.complete_query_with_error(
                job.response_tx,
                "MCP client not ready (initialization timeout)".to_string(),
            );
            return Ok(());
        }

        // Use structured MCP method if provided, otherwise infer from query
        let (method, params) = if let Some(mcp_method) = job.mcp_method {
            // Structured MCP request - use it directly
            (mcp_method, job.mcp_params.or(Some(json!({}))))
        } else if job.query.to_lowercase().contains("tools")
            && job.query.to_lowercase().contains("available")
        {
            // Legacy text query asking about tools - send tools/list
            ("tools/list".to_string(), Some(json!({})))
        } else {
            // Legacy text query - query available tools first, then use first available tool
            match self.get_available_tool(client).await {
                Ok(Some(tool_name)) => (
                    "tools/call".to_string(),
                    Some(json!({
                        "name": tool_name,
                        "arguments": { "input": job.query.clone() }
                    })),
                ),
                Ok(None) => {
                    // No tools available - send tools/list to show what's available
                    self.complete_query_with_error(
                        job.response_tx,
                        "No tools available from MCP server. Try 'tools/list' to see available tools.".to_string(),
                    );
                    return Ok(());
                }
                Err(e) => {
                    // Error querying tools - fall back to tools/list
                    warn!("Failed to get available tools: {}", e);
                    ("tools/list".to_string(), Some(json!({})))
                }
            }
        };

        let request_id = job.correlation_id.to_string();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::String(request_id.clone()),
            method,
            params,
        };

        // Process through interceptors for semantic routing
        let interceptor_start = Instant::now();
        let metadata = acdp_core::interceptor::InterceptionMetadata::default()
            .with_session_id(self.session_id.0.to_string());

        let result = self
            .interceptor_manager
            .process_message_with_metadata(
                JsonRpcMessage::Request(request.clone()),
                MessageDirection::Outgoing,
                metadata,
            )
            .await?;

        let interceptor_delay_ms = Some(interceptor_start.elapsed().as_secs_f64() * 1000.0);

        if result.block {
            self.complete_query_with_error(
                job.response_tx,
                format!("Query blocked by interceptor: {:?}", result.reasoning),
            );
            return Ok(());
        }

        // Extract the method and params from the interceptor-processed message
        let (final_method, final_params) =
            if let JsonRpcMessage::Request(modified_req) = result.message {
                // Interceptors may have modified the request - use what they decided
                let method = modified_req.method.clone();
                let params = if method == "tools/list" {
                    Some(json!({}))
                } else {
                    // For tools/call, extract the (possibly modified) tool name
                    let tool_name = modified_req
                        .params
                        .as_ref()
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string());

                    // If no tool name found, try to get one from available tools
                    let tool_name = match tool_name {
                        Some(name) => name,
                        None => {
                            // Try to get available tool (this will use cache if available)
                            match self.get_available_tool(client).await {
                                Ok(Some(name)) => name,
                                _ => {
                                    // Last resort: send tools/list instead
                                    let tools_list_start = Instant::now();
                                    let tools_list_response =
                                        match client.send_request("tools/list", json!({})).await {
                                            Ok(resp) => resp,
                                            Err(e) => {
                                                self.complete_query_with_error(
                                                    job.response_tx,
                                                    format!("Failed to get available tools: {}", e),
                                                );
                                                return Ok(());
                                            }
                                        };
                                    let mut completion = TuiQueryCompletion::from_response(
                                        &tools_list_response,
                                        tools_list_start.elapsed(),
                                    );
                                    completion.interceptor_delay_ms = interceptor_delay_ms;
                                    let _ = job.response_tx.send(completion);
                                    return Ok(());
                                }
                            }
                        }
                    };

                    Some(json!({
                        "name": tool_name,
                        "arguments": { "input": job.query.clone() }
                    }))
                };
                (method, params.unwrap_or(json!({})))
            } else {
                // Fallback if something went wrong - try to get available tool
                match self.get_available_tool(client).await {
                    Ok(Some(tool_name)) => (
                        "tools/call".to_string(),
                        json!({
                            "name": tool_name,
                            "arguments": { "input": job.query.clone() }
                        }),
                    ),
                    _ => {
                        // No tools available - send tools/list
                        ("tools/list".to_string(), json!({}))
                    }
                }
            };

        let started = Instant::now();
        let response = match client.send_request(&final_method, final_params).await {
            Ok(resp) => resp,
            Err(e) => {
                self.complete_query_with_error(job.response_tx, e.to_string());
                return Ok(());
            }
        };

        let mut completion = TuiQueryCompletion::from_response(&response, started.elapsed());
        completion.interceptor_delay_ms = interceptor_delay_ms;
        let _ = job.response_tx.send(completion);
        Ok(())
    }

    fn complete_query_with_error(
        &self,
        responder: oneshot::Sender<TuiQueryCompletion>,
        message: String,
    ) {
        let _ = responder.send(TuiQueryCompletion {
            response: String::new(),
            error: Some(message),
            ttft_ms: None,
            tokens_per_sec: None,
            total_tokens: None,
            interceptor_delay_ms: None,
        });
    }

    #[cfg(feature = "llm")]
    async fn process_pending_routing_updates(&mut self) {
        let mut modes_to_apply = Vec::new();
        let mut is_disconnected = false;

        if let Some(rx) = self.routing_mode_rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(mode) => modes_to_apply.push(mode),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        is_disconnected = true;
                        break;
                    }
                }
            }
        }

        if is_disconnected {
            self.routing_mode_rx = None;
        }

        for mode in modes_to_apply {
            self.apply_routing_mode(mode).await;
        }
    }

    #[cfg(feature = "llm")]
    async fn apply_routing_mode(&mut self, mode: RoutingMode) {
        self.current_routing_mode = mode;
        if let Some(interceptor) = self.llm_interceptor.as_ref() {
            interceptor.set_routing_mode(mode).await;
        }
        if let Some(client) = &self.ipc_client {
            if let Err(e) = client
                .send(IpcMessage::RoutingModeChanged {
                    proxy_id: self.proxy_id.clone(),
                    mode: mode.to_string(),
                })
                .await
            {
                warn!("Failed to notify monitor about routing change: {}", e);
            }
        }
    }
}
