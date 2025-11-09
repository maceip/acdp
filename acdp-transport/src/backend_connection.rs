//! Unified Backend Connection Abstraction
//!
//! This module provides a trait-based abstraction for backend connections,
//! supporting both local process spawning and upstream server connections.
//! This enables the relay proxy pattern: Client → Proxy → Backend (process or upstream)

use anyhow::Result;
use async_trait::async_trait;
use acdp_core::messages::JsonRpcMessage;
use acdp_core::McpClient;
use acdp_core::TransportConfig as McpTransportConfig;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;

/// Information about a backend connection
#[derive(Debug, Clone)]
pub enum BackendConnectionInfo {
    /// Local process backend
    Process { pid: Option<u32> },
    /// Upstream HTTP-SSE server
    UpstreamHttpSse { url: String },
    /// Upstream HTTP-Stream server
    UpstreamHttpStream { url: String },
}

/// Unified trait for backend connections (process or upstream server)
#[async_trait]
pub trait BackendConnection: Send + Sync {
    /// Send a message to the backend
    async fn send(&mut self, message: JsonRpcMessage) -> Result<()>;

    /// Receive a message from the backend
    /// Returns None if connection closed
    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>>;

    /// Check if connection is healthy
    async fn health_check(&self) -> Result<bool>;

    /// Get connection info for metrics/logging
    fn connection_info(&self) -> BackendConnectionInfo;

    /// Close the connection gracefully
    async fn close(&mut self) -> Result<()>;
}

/// Backend connection for local processes (stdio)
pub struct ProcessBackendConnection {
    process: Child,
    stdin: BufWriter<tokio::process::ChildStdin>,
    stdout: BufReader<tokio::process::ChildStdout>,
    pid: Option<u32>,
}

impl ProcessBackendConnection {
    /// Spawn a new process backend connection
    pub async fn spawn(command: &str) -> Result<Self> {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get process stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get process stdout"))?;

        Ok(Self {
            process: child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            pid,
        })
    }
}

#[async_trait]
impl BackendConnection for ProcessBackendConnection {
    async fn send(&mut self, message: JsonRpcMessage) -> Result<()> {
        let json = serde_json::to_string(&message)?;
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>> {
        let mut line = String::new();
        let bytes_read = self.stdout.read_line(&mut line).await?;

        if bytes_read == 0 {
            return Ok(None); // EOF
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            return self.recv().await; // Skip empty lines
        }

        let message: JsonRpcMessage = serde_json::from_str(trimmed)?;
        Ok(Some(message))
    }

    async fn health_check(&self) -> Result<bool> {
        // Check if process is still running
        // Note: try_wait() requires &mut, but we have &self
        // For now, assume healthy if we can't check
        // TODO: Use a different approach or make process mutable
        // In practice, we'll detect failures when reading/writing
        Ok(true) // Assume healthy - actual check would need &mut
    }

    fn connection_info(&self) -> BackendConnectionInfo {
        BackendConnectionInfo::Process { pid: self.pid }
    }

    async fn close(&mut self) -> Result<()> {
        // Flush stdin
        self.stdin.flush().await?;

        // Kill process if still running
        if let Err(e) = self.process.kill().await {
            tracing::warn!("Failed to kill backend process: {}", e);
        }

        // stdin/stdout will be dropped automatically when self is dropped
        Ok(())
    }
}

/// Backend connection for upstream HTTP-SSE servers
pub struct UpstreamHttpSseConnection {
    client: Arc<RwLock<McpClient>>,
    url: String,
}

impl UpstreamHttpSseConnection {
    /// Create a new upstream HTTP-SSE connection
    pub async fn new(url: &str) -> Result<Self> {
        // Create client with retry logic
        const MAX_RETRIES: u32 = 5;
        const INITIAL_BACKOFF_SECS: u64 = 1;

        let mcp_config = McpTransportConfig::http_sse(url)?;
        let mut retry_count = 0u32;

        let mut client = loop {
            match McpClient::with_defaults(mcp_config.clone()).await {
                Ok(c) => break c,
                Err(e) => {
                    if retry_count < MAX_RETRIES {
                        retry_count += 1;
                        let backoff_secs =
                            INITIAL_BACKOFF_SECS * (1u64 << (retry_count - 1).min(4));
                        tracing::warn!(
                            "Failed to create upstream client (attempt {}/{}), retrying in {}s: {}",
                            retry_count,
                            MAX_RETRIES,
                            backoff_secs,
                            e
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                        continue;
                    } else {
                        return Err(anyhow::anyhow!(
                            "Failed to connect to upstream server after {} retries: {}",
                            MAX_RETRIES,
                            e
                        ));
                    }
                }
            }
        };

        // Initialize connection
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
                        tracing::warn!("Failed to connect to upstream server (attempt {}/{}), retrying in {}s: {}", 
                            retry_count, MAX_RETRIES, backoff_secs, e);
                        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                        continue;
                    } else {
                        return Err(anyhow::anyhow!(
                            "Failed to connect to upstream server after {} retries: {}",
                            MAX_RETRIES,
                            e
                        ));
                    }
                }
            }
        }

        tracing::info!("Connected to upstream HTTP-SSE server at {}", url);

        Ok(Self {
            client: Arc::new(RwLock::new(client)),
            url: url.to_string(),
        })
    }
}

#[async_trait]
impl BackendConnection for UpstreamHttpSseConnection {
    async fn send(&mut self, message: JsonRpcMessage) -> Result<()> {
        let mut client = self.client.write().await;
        match message {
            JsonRpcMessage::Request(req) => {
                let params = req.params.unwrap_or_default();
                client.send_request(&req.method, params).await?;
            }
            JsonRpcMessage::Notification(notif) => {
                let params = notif.params.unwrap_or_default();
                client.send_notification(&notif.method, params).await?;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Cannot send response as request to upstream"
                ))
            }
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>> {
        // HTTP-SSE receives responses via the client's response handling
        // This is more complex - the client handles SSE stream internally
        // For relay pattern, we need to intercept responses
        // TODO: Implement response channel or use client's response handling
        // For now, return error - this needs to be handled at a higher level
        Err(anyhow::anyhow!(
            "HTTP-SSE receive not implemented in this context - use client's response handling"
        ))
    }

    async fn health_check(&self) -> Result<bool> {
        // For HTTP-SSE, we can't easily check health without sending a request
        // Assume healthy if client exists
        // TODO: Implement ping/health check request
        Ok(true)
    }

    fn connection_info(&self) -> BackendConnectionInfo {
        BackendConnectionInfo::UpstreamHttpSse {
            url: self.url.clone(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        // HTTP-SSE connections are managed by the client
        // Just drop the client reference
        Ok(())
    }
}

/// Backend connection for upstream HTTP-Stream servers
pub struct UpstreamHttpStreamConnection {
    client: Arc<RwLock<McpClient>>,
    url: String,
}

impl UpstreamHttpStreamConnection {
    /// Create a new upstream HTTP-Stream connection
    pub async fn new(url: &str) -> Result<Self> {
        // Similar to HTTP-SSE but for HTTP-Stream transport
        let mcp_config = McpTransportConfig::http_stream(url)?;
        let mut client = McpClient::with_defaults(mcp_config).await?;

        // Initialize connection
        let client_impl =
            acdp_core::messages::Implementation::new("mcp-proxy", env!("CARGO_PKG_VERSION"));
        client.connect(client_impl).await?;

        tracing::info!("Connected to upstream HTTP-Stream server at {}", url);

        Ok(Self {
            client: Arc::new(RwLock::new(client)),
            url: url.to_string(),
        })
    }
}

#[async_trait]
impl BackendConnection for UpstreamHttpStreamConnection {
    async fn send(&mut self, message: JsonRpcMessage) -> Result<()> {
        let mut client = self.client.write().await;
        match message {
            JsonRpcMessage::Request(req) => {
                let params = req.params.unwrap_or_default();
                client.send_request(&req.method, params).await?;
            }
            JsonRpcMessage::Notification(notif) => {
                let params = notif.params.unwrap_or_default();
                client.send_notification(&notif.method, params).await?;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Cannot send response as request to upstream"
                ))
            }
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>> {
        // HTTP-Stream receives responses via the client's response handling
        // Similar to HTTP-SSE, needs higher-level handling
        Err(anyhow::anyhow!(
            "HTTP-Stream receive not implemented in this context - use client's response handling"
        ))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true) // TODO: Implement health check
    }

    fn connection_info(&self) -> BackendConnectionInfo {
        BackendConnectionInfo::UpstreamHttpStream {
            url: self.url.clone(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Create a backend connection from configuration
pub async fn create_backend_connection(
    backend_url: Option<&str>,
    backend_command: Option<&str>,
    backend_transport: Option<&str>,
) -> Result<Box<dyn BackendConnection>> {
    if let Some(url) = backend_url {
        // Connect to upstream server
        let transport = backend_transport
            .or_else(|| detect_transport_from_url(url))
            .unwrap_or("http-sse");

        match transport {
            "http-sse" => Ok(Box::new(UpstreamHttpSseConnection::new(url).await?)),
            "http-stream" => Ok(Box::new(UpstreamHttpStreamConnection::new(url).await?)),
            _ => Err(anyhow::anyhow!("Unsupported transport: {}", transport)),
        }
    } else if let Some(cmd) = backend_command {
        if cmd.is_empty() {
            Err(anyhow::anyhow!(
                "backend_command is empty and backend_url not set"
            ))
        } else {
            // Spawn local process
            Ok(Box::new(ProcessBackendConnection::spawn(cmd).await?))
        }
    } else {
        Err(anyhow::anyhow!(
            "Either backend_url or backend_command must be set"
        ))
    }
}

/// Auto-detect transport type from URL
fn detect_transport_from_url(url: &str) -> Option<&str> {
    if url.contains("/stream") || url.contains("/message") {
        Some("http-stream")
    } else if url.starts_with("http://") || url.starts_with("https://") {
        Some("http-sse")
    } else {
        None
    }
}
