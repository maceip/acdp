use anyhow::Result;
use futures::future::pending;
use acdp_common::{
    InterceptorInfo, InterceptorManagerInfo, IpcMessage, LogEntry, LogLevel, ProxyId, ProxyStats,
    SessionId, SessionMetrics,
};
use acdp_core::interceptor::{InterceptionMetadata, InterceptorManager, MessageDirection};
use acdp_core::messages::JsonRpcMessage;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::Child;
use tokio::sync::{
    broadcast,
    mpsc::{self, error::TryRecvError},
    oneshot, Mutex,
};
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

use crate::buffered_ipc_client::BufferedIpcClient;
use crate::ipc_server::{ProxyIpcServer, TuiQueryCompletion, TuiQueryJob};

#[cfg(feature = "llm")]
use acdp_llm::{LlmInterceptor, LlmService, RoutingMode, SessionManager};

struct PendingQuery {
    responder: oneshot::Sender<TuiQueryCompletion>,
    started_at: Instant,
    interceptor_delay_ms: Option<f64>,
}

pub struct StdioHandler {
    proxy_id: ProxyId,
    stats: Arc<Mutex<ProxyStats>>,
    ipc_client: Option<Arc<BufferedIpcClient>>,
    stats_interval: tokio::time::Interval,
    interceptor_manager: Arc<InterceptorManager>,
    session_id: Option<SessionId>,
    #[cfg(feature = "llm")]
    session_manager: Option<Arc<SessionManager>>,
    #[cfg(feature = "llm")]
    llm_interceptor: Option<Arc<LlmInterceptor>>,
    #[cfg(feature = "llm")]
    current_routing_mode: RoutingMode,
    pending_queries: Arc<Mutex<HashMap<String, PendingQuery>>>,
    query_rx: Option<mpsc::Receiver<TuiQueryJob>>,
    #[cfg(feature = "llm")]
    routing_mode_rx: Option<mpsc::Receiver<RoutingMode>>,
    /// Cached list of available tools from the MCP server
    available_tools: Arc<Mutex<Option<Vec<String>>>>,
}

impl StdioHandler {
    pub async fn new(
        proxy_id: ProxyId,
        stats: Arc<Mutex<ProxyStats>>,
        ipc_client: Option<Arc<BufferedIpcClient>>,
        #[cfg(feature = "llm")] llm_service: Option<Arc<LlmService>>,
        #[cfg(feature = "llm")] routing_mode: RoutingMode,
        monitor_socket: Option<String>,
    ) -> Result<Self> {
        use crate::interceptors::{TransformInterceptor, TransformOperation, TransformRule};
        use serde_json::json;

        let manager = Arc::new(InterceptorManager::new());
        let session_id = Some(SessionId::new());
        #[cfg(feature = "llm")]
        let session_manager = llm_service
            .as_ref()
            .map(|service| service.session_manager());

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

        // Demo Transform interceptor - only add in non-bypass modes
        // This demonstrates transform capabilities but shouldn't run in bypass mode
        #[cfg(feature = "llm")]
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

        #[cfg(feature = "llm")]
        let mut llm_interceptor_handle = None;

        #[cfg(feature = "llm")]
        {
            if let Some(service) = llm_service {
                let session_manager = session_manager
                    .as_ref()
                    .map(Arc::clone)
                    .expect("session manager should exist when LLM service is available");
                match Self::add_llm_interceptor(
                    manager.clone(),
                    service.clone(),
                    routing_mode,
                    session_manager,
                )
                .await
                {
                    Ok(handle) => {
                        // Create IPC channel for semantic predictions
                        let (ipc_tx, mut ipc_rx) = tokio::sync::mpsc::unbounded_channel();

                        // Wire the sender to the interceptor
                        handle.set_ipc_sender(ipc_tx).await;

                        // Spawn task to forward messages to BufferedIpcClient
                        if let Some(ref client) = ipc_client {
                            let client = client.clone();
                            tokio::spawn(async move {
                                while let Some(msg) = ipc_rx.recv().await {
                                    if let Err(e) = client.send(msg).await {
                                        tracing::warn!(
                                            "Failed to send semantic prediction via IPC: {}",
                                            e
                                        );
                                    }
                                }
                            });
                        }

                        llm_interceptor_handle = Some(handle);
                    }
                    Err(e) => {
                        warn!("Failed to initialize LLM interceptor: {}", e);
                    }
                }
            } else {
                warn!("LLM service not available; skipping LLM interceptor");
            }
        }

        Self::with_interceptors(
            proxy_id,
            stats,
            ipc_client,
            manager.clone(),
            session_id,
            #[cfg(feature = "llm")]
            session_manager,
            monitor_socket,
            query_rx,
            query_tx,
            #[cfg(feature = "llm")]
            routing_tx,
            #[cfg(feature = "llm")]
            routing_rx,
            #[cfg(feature = "llm")]
            llm_interceptor_handle,
            #[cfg(feature = "llm")]
            routing_mode,
        )
        .await
    }

    #[cfg(feature = "llm")]
    async fn process_pending_routing_updates(&mut self) {
        let mut pending_modes = Vec::new();
        if let Some(rx) = self.routing_mode_rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(mode) => pending_modes.push(mode),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.routing_mode_rx = None;
                        break;
                    }
                }
            }
        }

        for mode in pending_modes {
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

    #[cfg(feature = "llm")]
    async fn add_llm_interceptor(
        manager: Arc<InterceptorManager>,
        llm_service: Arc<LlmService>,
        routing_mode: RoutingMode,
        session_manager: Arc<SessionManager>,
    ) -> Result<Arc<LlmInterceptor>> {
        info!("Initializing LLM interceptor with database-backed predictions");

        // Create tool predictor from service
        let predictor = llm_service.tool_predictor();

        // Get database handles
        let database = llm_service.database();
        let routing_db = database.routing_rules.clone();

        // Create LLM interceptor with Hybrid routing mode
        let llm_interceptor = Arc::new(LlmInterceptor::with_interceptor_manager(
            predictor,
            routing_mode,
            routing_db,
            manager.clone(),
            Some(session_manager),
        ));

        // Add to manager
        manager.add_interceptor(llm_interceptor.clone()).await;

        info!("LLM interceptor initialized successfully");
        Ok(llm_interceptor)
    }

    pub async fn with_interceptors(
        proxy_id: ProxyId,
        stats: Arc<Mutex<ProxyStats>>,
        ipc_client: Option<Arc<BufferedIpcClient>>,
        interceptor_manager: Arc<InterceptorManager>,
        session_id: Option<SessionId>,
        #[cfg(feature = "llm")] session_manager: Option<Arc<SessionManager>>,
        monitor_socket: Option<String>,
        query_rx: Option<mpsc::Receiver<TuiQueryJob>>,
        query_tx: Option<mpsc::Sender<TuiQueryJob>>,
        #[cfg(feature = "llm")] routing_tx: Option<mpsc::Sender<RoutingMode>>,
        #[cfg(feature = "llm")] routing_rx: Option<mpsc::Receiver<RoutingMode>>,
        #[cfg(feature = "llm")] llm_interceptor_handle: Option<Arc<LlmInterceptor>>,
        #[cfg(feature = "llm")] routing_mode: RoutingMode,
    ) -> Result<Self> {
        let stats_interval = interval(Duration::from_secs(1));

        // Start IPC server for bidirectional communication if IPC client exists
        if let Some(ref client) = ipc_client {
            let ipc_socket = monitor_socket
                .clone()
                .or_else(|| std::env::var("MCP_IPC_SOCKET").ok())
                .unwrap_or_else(|| "/tmp/mcp-monitor.sock".to_string());
            let ipc_server = Arc::new(ProxyIpcServer::new(
                client.clone(),
                query_tx.clone(),
                #[cfg(feature = "llm")]
                routing_tx.clone(),
            ));

            if let Err(e) = ipc_server.start(ipc_socket).await {
                warn!("Failed to start IPC server: {}", e);
            } else {
                info!("Proxy IPC server started for bidirectional communication");
            }
        }

        Ok(Self {
            proxy_id,
            stats,
            ipc_client,
            stats_interval,
            interceptor_manager,
            session_id,
            #[cfg(feature = "llm")]
            session_manager,
            #[cfg(feature = "llm")]
            llm_interceptor: llm_interceptor_handle,
            #[cfg(feature = "llm")]
            current_routing_mode: routing_mode,
            pending_queries: Arc::new(Mutex::new(HashMap::new())),
            query_rx,
            #[cfg(feature = "llm")]
            routing_mode_rx: routing_rx,
            available_tools: Arc::new(Mutex::new(None)),
        })
    }

    /// Get the interceptor manager for this handler
    pub fn interceptor_manager(&self) -> &Arc<InterceptorManager> {
        &self.interceptor_manager
    }

    fn interception_metadata(&self) -> InterceptionMetadata {
        match &self.session_id {
            Some(session) => InterceptionMetadata::default().with_session_id(session.0.to_string()),
            None => InterceptionMetadata::default(),
        }
    }

    pub async fn handle_communication(
        &mut self,
        child: &mut Child,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<()> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get child stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get child stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get child stderr"))?;

        let mut child_stdin = BufWriter::new(stdin);
        let mut child_stdout = BufReader::new(stdout);
        let mut child_stderr = BufReader::new(stderr);

        let mut user_stdin = BufReader::new(tokio::io::stdin());
        let mut user_stdout = tokio::io::stdout();
        let mut query_rx = self.query_rx.take();

        // Channels removed - not needed for direct STDIO handling

        loop {
            #[cfg(feature = "llm")]
            self.process_pending_routing_updates().await;

            tokio::select! {
                // Check for shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal");
                    break;
                }

                // Handle stats updates
                _ = self.stats_interval.tick() => {
                    if let Some(ref client) = self.ipc_client {
                        // Send proxy stats
                        let stats = self.stats.lock().await.clone();
                        if let Err(e) = client.send(IpcMessage::StatsUpdate(stats)).await {
                            warn!("Failed to send stats update: {}", e);
                        }

                        // Send interceptor stats
                        let interceptor_stats = self.get_interceptor_stats().await;
                        if let Err(e) = client.send(IpcMessage::InterceptorStats {
                            proxy_id: self.proxy_id.clone(),
                            stats: interceptor_stats,
                        }).await {
                            warn!("Failed to send interceptor stats: {}", e);
                        }
                    }
                    #[cfg(feature = "llm")]
                    {
                        self.emit_session_stats().await;
                    }
                }

                query_job = async {
                    if let Some(rx) = &mut query_rx {
                        rx.recv().await
                    } else {
                        pending().await
                    }
                } => {
                    if let Some(job) = query_job {
                        if let Err(e) = self.process_tui_query(job, &mut child_stdin).await {
                            error!("Failed to process TUI query: {}", e);
                        }
                    }
                }

                // Read from user stdin and forward to child
                result = async {
                    let mut input = String::new();
                    let bytes_read = user_stdin.read_line(&mut input).await?;
                    Ok::<(usize, String), std::io::Error>((bytes_read, input))
                } => {
                    match result {
                        Ok((0, _)) => break, // EOF
                        Ok((_, input)) => {
                            // Process through interceptors
                            let (processed_input, modified) = match self.process_outgoing(&input).await {
                                Ok(result) => result,
                                Err(e) => {
                                    warn!("Message blocked or failed processing: {}", e);
                                    // Log the blocked message
                                    self.log_request(&input, false).await;
                                    {
                                        let mut stats = self.stats.lock().await;
                                        stats.failed_requests += 1;
                                    }
                                    continue; // Skip sending to child
                                }
                            };

                            self.log_request(&processed_input, modified).await;

                            if let Err(e) = child_stdin.write_all(processed_input.as_bytes()).await {
                                error!("Failed to write to child stdin: {}", e);
                                break;
                            }
                            if let Err(e) = child_stdin.flush().await {
                                error!("Failed to flush child stdin: {}", e);
                                break;
                            }

                            // Update stats
                            {
                                let mut stats = self.stats.lock().await;
                                stats.total_requests += 1;
                                stats.bytes_transferred += processed_input.len() as u64;
                            }
                        }
                        Err(e) => {
                            error!("Failed to read from user stdin: {}", e);
                            break;
                        }
                    }
                }

                // Read from child stdout and forward to user
                result = async {
                    let mut output = String::new();
                    let bytes_read = child_stdout.read_line(&mut output).await?;
                    Ok::<(usize, String), std::io::Error>((bytes_read, output))
                } => {
                    match result {
                        Ok((0, _)) => {
                            info!("Child stdout closed");
                            break;
                        }
                        Ok((_, output)) => {
                            // Process through interceptors
                            let (processed_output, modified) = match self.process_incoming(&output).await {
                                Ok(result) => result,
                                Err(e) => {
                                    warn!("Message blocked or failed processing: {}", e);
                                    // Log the blocked message
                                    self.log_response(&output, false).await;
                                    {
                                        let mut stats = self.stats.lock().await;
                                        stats.failed_requests += 1;
                                    }
                                    continue; // Skip sending to user
                                }
                            };

                            self.log_response(&processed_output, modified).await;

                            if let Err(e) = user_stdout.write_all(processed_output.as_bytes()).await {
                                error!("Failed to write to user stdout: {}", e);
                                break;
                            }
                            if let Err(e) = user_stdout.flush().await {
                                error!("Failed to flush user stdout: {}", e);
                                break;
                            }

                            // Update stats
                            {
                                let mut stats = self.stats.lock().await;
                                stats.successful_requests += 1;
                                stats.bytes_transferred += processed_output.len() as u64;
                            }
                        }
                        Err(e) => {
                            error!("Failed to read from child stdout: {}", e);
                            {
                                let mut stats = self.stats.lock().await;
                                stats.failed_requests += 1;
                            }
                            break;
                        }
                    }
                }

                // Read from child stderr and log as errors
                result = async {
                    let mut error_msg = String::new();
                    let bytes_read = child_stderr.read_line(&mut error_msg).await?;
                    Ok::<(usize, String), std::io::Error>((bytes_read, error_msg))
                } => {
                    match result {
                        Ok((0, _)) => {
                            debug!("Child stderr closed");
                        }
                        Ok((_, error_msg)) => {
                            self.log_error(&error_msg).await;

                            // Also forward stderr to user stderr
                            if let Err(e) = tokio::io::stderr().write_all(error_msg.as_bytes()).await {
                                warn!("Failed to write child stderr to user stderr: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to read from child stderr: {}", e);
                        }
                    }
                }

                // Check if child process has exited
                status = child.wait() => {
                    match status {
                        Ok(exit_status) => {
                            if !exit_status.success() {
                                error!("Child process exited with non-zero status: {:?}", exit_status);
                                let mut stats = self.stats.lock().await;
                                stats.failed_requests += 1;
                                // Return error to trigger restart in proxy
                                return Err(anyhow::anyhow!("Backend process crashed with status {:?}", exit_status));
                            } else {
                                info!("Child process exited normally with status: {:?}", exit_status);
                            }
                        }
                        Err(e) => {
                            error!("Failed to wait for child process: {}", e);
                            return Err(anyhow::anyhow!("Failed to wait for child process: {}", e));
                        }
                    }
                    break;
                }
            }
        }

        self.query_rx = query_rx;

        Ok(())
    }

    /// Get available tool name, querying tools/list if not cached
    async fn get_available_tool_name(&self) -> Option<String> {
        // Check cache first
        {
            let tools = self.available_tools.lock().await;
            if let Some(ref tool_list) = *tools {
                if !tool_list.is_empty() {
                    return Some(tool_list[0].clone());
                }
            }
        }
        None
    }

    /// Update cached tools from a tools/list response
    async fn update_tools_cache(&self, response: &serde_json::Value) {
        if let Some(tools_array) = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
        {
            let tool_names: Vec<String> = tools_array
                .iter()
                .filter_map(|tool| tool.get("name").and_then(|n| n.as_str()))
                .map(|s| s.to_string())
                .collect();

            if !tool_names.is_empty() {
                let mut cached = self.available_tools.lock().await;
                *cached = Some(tool_names);
            }
        }
    }

    async fn process_tui_query(
        &mut self,
        job: TuiQueryJob,
        child_stdin: &mut BufWriter<tokio::process::ChildStdin>,
    ) -> Result<()> {
        let request_id = job.correlation_id.to_string();

        // Determine method and params
        let (method, params) = if let Some(mcp_method) = job.mcp_method {
            // Structured MCP request - use it directly
            (mcp_method, job.mcp_params.unwrap_or(json!({})))
        } else if job.query.to_lowercase().contains("tools")
            && job.query.to_lowercase().contains("available")
        {
            // Legacy text query asking about tools - send tools/list
            ("tools/list".to_string(), json!({}))
        } else {
            // Legacy text query - use first available tool from cache
            // If cache is empty, send tools/list to populate it
            match self.get_available_tool_name().await {
                Some(tool_name) => (
                    "tools/call".to_string(),
                    json!({
                        "name": tool_name,
                        "arguments": {
                            "input": job.query
                        }
                    }),
                ),
                None => {
                    // No cached tools - send tools/list first
                    ("tools/list".to_string(), json!({}))
                }
            }
        };

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params
        });
        let raw = payload.to_string() + "\n";

        // Measure interceptor processing time
        let interceptor_start = Instant::now();
        let (processed_input, modified) = match self.process_outgoing(&raw).await {
            Ok(result) => result,
            Err(e) => {
                self.complete_query_with_error(job.response_tx, format!("Query rejected: {}", e));
                return Ok(());
            }
        };
        let interceptor_delay_ms = Some(interceptor_start.elapsed().as_secs_f64() * 1000.0);

        self.log_request(&processed_input, modified).await;

        if let Err(e) = child_stdin.write_all(processed_input.as_bytes()).await {
            self.complete_query_with_error(job.response_tx, format!("Failed to send query: {}", e));
            return Ok(());
        }
        if let Err(e) = child_stdin.flush().await {
            self.complete_query_with_error(
                job.response_tx,
                format!("Failed to flush query: {}", e),
            );
            return Ok(());
        }

        let started_at = Instant::now();
        {
            let mut pending = self.pending_queries.lock().await;
            pending.insert(
                request_id,
                PendingQuery {
                    responder: job.response_tx,
                    started_at,
                    interceptor_delay_ms,
                },
            );
        }

        {
            let mut stats = self.stats.lock().await;
            stats.total_requests += 1;
            stats.bytes_transferred += processed_input.len() as u64;
        }

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

    async fn capture_query_response(&self, message: &JsonRpcMessage) {
        if let JsonRpcMessage::Response(response) = message {
            // Update tools cache if this is a tools/list response
            if let Some(ref result) = response.result {
                // Check if this response contains tools
                if result.get("tools").is_some() {
                    self.update_tools_cache(result).await;
                }
            }

            let request_id = response.id.to_string();
            if let Some(pending) = self.pending_queries.lock().await.remove(&request_id) {
                let mut completion =
                    TuiQueryCompletion::from_response(response, pending.started_at.elapsed());
                // Include interceptor delay if measured
                completion.interceptor_delay_ms = pending.interceptor_delay_ms;
                let _ = pending.responder.send(completion);
            }
        }
    }

    #[cfg(feature = "llm")]
    async fn emit_session_stats(&self) {
        let Some(ref manager) = self.session_manager else {
            return;
        };
        let Some(ref session_id) = self.session_id else {
            return;
        };
        let Some(ref client) = self.ipc_client else {
            return;
        };

        if let Some(stats) = manager.get_session_stats(session_id).await {
            let metrics = SessionMetrics {
                proxy_id: self.proxy_id.clone(),
                session_id: session_id.clone(),
                total_predictions: stats.total_predictions,
                successful_predictions: stats.successful_predictions,
                accuracy: stats.accuracy,
                optimization_score: stats.optimization_score,
                message_count: stats.message_count,
            };
            if let Err(e) = client.send(IpcMessage::SessionStats(metrics)).await {
                warn!("Failed to send session stats: {}", e);
            }
        }
    }
    /// Process an outgoing message (client -> server) through interceptors
    async fn process_outgoing(&self, content: &str) -> Result<(String, bool)> {
        // Try to parse as JSON-RPC message
        match serde_json::from_str::<JsonRpcMessage>(content.trim()) {
            Ok(message) => {
                // Process through interceptors
                let metadata = self.interception_metadata();
                match self
                    .interceptor_manager
                    .process_message_with_metadata(message, MessageDirection::Outgoing, metadata)
                    .await
                {
                    Ok(result) => {
                        if result.block {
                            warn!("Message blocked by interceptor: {:?}", result.reasoning);
                            return Err(anyhow::anyhow!(
                                "Message blocked: {}",
                                result.reasoning.unwrap_or_default()
                            ));
                        }

                        let modified_content = serde_json::to_string(&result.message)?;
                        Ok((modified_content + "\n", result.modified))
                    }
                    Err(e) => {
                        warn!("Interceptor processing failed: {}", e);
                        // Fall back to original message
                        Ok((content.to_string(), false))
                    }
                }
            }
            Err(_) => {
                // Not valid JSON-RPC, pass through unchanged
                Ok((content.to_string(), false))
            }
        }
    }

    /// Process an incoming message (server -> client) through interceptors
    async fn process_incoming(&self, content: &str) -> Result<(String, bool)> {
        // Try to parse as JSON-RPC message
        match serde_json::from_str::<JsonRpcMessage>(content.trim()) {
            Ok(message) => {
                // Process through interceptors
                let metadata = self.interception_metadata();
                match self
                    .interceptor_manager
                    .process_message_with_metadata(message, MessageDirection::Incoming, metadata)
                    .await
                {
                    Ok(result) => {
                        self.capture_query_response(&result.message).await;
                        if result.block {
                            warn!("Message blocked by interceptor: {:?}", result.reasoning);
                            return Err(anyhow::anyhow!(
                                "Message blocked: {}",
                                result.reasoning.unwrap_or_default()
                            ));
                        }

                        let modified_content = serde_json::to_string(&result.message)?;
                        Ok((modified_content + "\n", result.modified))
                    }
                    Err(e) => {
                        warn!("Interceptor processing failed: {}", e);
                        // Fall back to original message
                        Ok((content.to_string(), false))
                    }
                }
            }
            Err(_) => {
                // Not valid JSON-RPC, pass through unchanged
                Ok((content.to_string(), false))
            }
        }
    }

    async fn log_request(&mut self, content: &str, modified: bool) {
        let prefix = if modified { "→ [MODIFIED]" } else { "→" };
        let log_entry = LogEntry::new(
            LogLevel::Request,
            format!("{} {}", prefix, content.trim()),
            self.proxy_id.clone(),
        );

        if let Some(ref client) = self.ipc_client {
            if let Err(e) = client.send(IpcMessage::LogEntry(log_entry)).await {
                warn!("Failed to send log entry: {}", e);
            }
        }

        debug!(
            "Request{}: {}",
            if modified { " (modified)" } else { "" },
            content.trim()
        );
    }

    async fn log_response(&mut self, content: &str, modified: bool) {
        let prefix = if modified { "← [MODIFIED]" } else { "←" };
        let log_entry = LogEntry::new(
            LogLevel::Response,
            format!("{} {}", prefix, content.trim()),
            self.proxy_id.clone(),
        );

        if let Some(ref client) = self.ipc_client {
            if let Err(e) = client.send(IpcMessage::LogEntry(log_entry)).await {
                warn!("Failed to send log entry: {}", e);
            }
        }

        debug!(
            "Response{}: {}",
            if modified { " (modified)" } else { "" },
            content.trim()
        );
    }

    async fn log_error(&mut self, content: &str) {
        let log_entry = LogEntry::new(
            LogLevel::Info, // Changed from Error - stderr is not always errors
            format!("stderr: {}", content.trim()),
            self.proxy_id.clone(),
        );

        if let Some(ref client) = self.ipc_client {
            if let Err(e) = client.send(IpcMessage::LogEntry(log_entry)).await {
                warn!("Failed to send log entry: {}", e);
            }
        }

        info!("Child stderr: {}", content.trim()); // Changed from error! to info!
    }

    /// Get interceptor statistics from the manager
    async fn get_interceptor_stats(&self) -> InterceptorManagerInfo {
        let manager_stats = self.interceptor_manager.get_stats().await;
        let interceptor_names = self.interceptor_manager.list_interceptors().await;

        let mut interceptors = Vec::new();
        for name in interceptor_names {
            // Note: We don't currently track enabled/disabled state per interceptor
            // This would require adding that capability to InterceptorManager
            interceptors.push(InterceptorInfo {
                name: name.clone(),
                priority: 0,   // Would need to query this from the actual interceptor
                enabled: true, // Assume enabled for now
                total_intercepted: 0, // Would need per-interceptor tracking
                total_modified: 0,
                total_blocked: 0,
                avg_processing_time_ms: 0.0,
            });
        }

        InterceptorManagerInfo {
            total_messages_processed: manager_stats.total_messages_processed,
            total_modifications_made: manager_stats.total_modifications_made,
            total_messages_blocked: manager_stats.total_messages_blocked,
            avg_processing_time_ms: manager_stats.avg_processing_time_ms,
            messages_by_method: manager_stats.messages_by_method,
            interceptors,
        }
    }
}
