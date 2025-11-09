//! IPC server for bidirectional communication with TUI

use anyhow::Result;
use acdp_common::{ipc::IpcServer, IpcEnvelope, IpcMessage};
use acdp_core::messages::JsonRpcResponse;
#[cfg(feature = "llm")]
use acdp_llm::routing_modes::RoutingMode;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::buffered_ipc_client::BufferedIpcClient;

/// Job sent from the IPC server to the transport layer when the TUI submits a query.
pub struct TuiQueryJob {
    pub correlation_id: uuid::Uuid,
    pub query: String,
    /// If specified, this is a structured MCP method call (e.g., "tools/list")
    pub mcp_method: Option<String>,
    /// Parameters for the MCP method call
    pub mcp_params: Option<serde_json::Value>,
    pub response_tx: oneshot::Sender<TuiQueryCompletion>,
}

/// Completion payload returned from the transport layer back to the IPC server.
#[derive(Debug)]
pub struct TuiQueryCompletion {
    pub response: String,
    pub error: Option<String>,
    pub ttft_ms: Option<f64>,
    pub tokens_per_sec: Option<f64>,
    pub total_tokens: Option<usize>,
    pub interceptor_delay_ms: Option<f64>,
}

impl TuiQueryCompletion {
    pub fn from_response(response: &JsonRpcResponse, elapsed: Duration) -> Self {
        let text = Self::extract_text(response);
        let token_count = if text.is_empty() {
            0
        } else {
            text.split_whitespace().count()
        };
        let elapsed_secs = elapsed.as_secs_f64();
        let tokens_per_sec = if token_count > 0 && elapsed_secs > 0.0 {
            Some(token_count as f64 / elapsed_secs)
        } else {
            None
        };

        Self {
            response: text,
            error: response
                .error
                .as_ref()
                .map(|err| format!("{} (code {})", err.message, err.code)),
            ttft_ms: Some(elapsed_secs * 1000.0),
            tokens_per_sec,
            total_tokens: if token_count > 0 {
                Some(token_count)
            } else {
                None
            },
            interceptor_delay_ms: None, // Will be populated by interceptor if applicable
        }
    }

    fn extract_text(response: &JsonRpcResponse) -> String {
        if let Some(result) = &response.result {
            if let Some(text) = Self::extract_text_from_value(result) {
                return text;
            }
            return result.to_string();
        }
        String::new()
    }

    fn extract_text_from_value(value: &Value) -> Option<String> {
        // Handle tools/list response
        if let Some(tools) = value.get("tools").and_then(|t| t.as_array()) {
            let tool_names: Vec<String> = tools
                .iter()
                .filter_map(|tool| tool.get("name").and_then(|n| n.as_str()))
                .map(|name| name.to_string())
                .collect();

            if !tool_names.is_empty() {
                return Some(format!("Available tools: {}", tool_names.join(", ")));
            }
        }

        // Handle tools/call response with content array
        if let Some(items) = value.get("content").and_then(|c| c.as_array()) {
            for item in items {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }

        // Handle simple text response
        value
            .get("text")
            .and_then(|t| t.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

/// Handles incoming IPC connections and messages
pub struct ProxyIpcServer {
    /// IPC client for sending responses back
    ipc_client: Arc<BufferedIpcClient>,
    /// Sender used to pass TUI queries to the active transport handler
    query_tx: Option<mpsc::Sender<TuiQueryJob>>,
    #[cfg(feature = "llm")]
    routing_tx: Option<mpsc::Sender<RoutingMode>>,
}

impl ProxyIpcServer {
    pub fn new(
        ipc_client: Arc<BufferedIpcClient>,
        query_tx: Option<mpsc::Sender<TuiQueryJob>>,
        #[cfg(feature = "llm")] routing_tx: Option<mpsc::Sender<RoutingMode>>,
    ) -> Self {
        Self {
            ipc_client,
            query_tx,
            #[cfg(feature = "llm")]
            routing_tx,
        }
    }

    /// Start the IPC server to listen for incoming messages
    pub async fn start(self: Arc<Self>, socket_path: String) -> Result<()> {
        // Create a separate socket for receiving messages
        let receive_socket = format!("{}.recv", socket_path);

        // Clean up any existing socket
        let _ = tokio::fs::remove_file(&receive_socket).await;

        let server = IpcServer::bind(&receive_socket).await?;
        info!("Proxy IPC server listening on {}", receive_socket);

        // Spawn task to handle connections
        tokio::spawn(async move {
            loop {
                match server.accept().await {
                    Ok(mut connection) => {
                        let server_clone = self.clone();

                        // Handle this connection in a separate task
                        tokio::spawn(async move {
                            info!("New IPC connection accepted");

                            loop {
                                match connection.receive_message().await {
                                    Ok(Some(envelope)) => {
                                        if let Err(e) = server_clone.handle_message(envelope).await
                                        {
                                            error!("Error handling IPC message: {}", e);
                                        }
                                    }
                                    Ok(None) => {
                                        info!("IPC connection closed");
                                        break;
                                    }
                                    Err(e) => {
                                        error!("Error receiving IPC message: {}", e);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept IPC connection: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle an incoming IPC message
    async fn handle_message(&self, envelope: IpcEnvelope) -> Result<()> {
        match envelope.message {
            IpcMessage::TuiMcpRequest {
                method,
                params,
                correlation_id,
            } => {
                info!(
                    "Received TUI MCP request: {} (correlation: {:?})",
                    method, correlation_id
                );

                if let Some(tx) = &self.query_tx {
                    let (response_tx, response_rx) = oneshot::channel();

                    // Create a job with method and params
                    let job = TuiQueryJob {
                        query: method.clone(),
                        mcp_method: Some(method),
                        mcp_params: params,
                        correlation_id,
                        response_tx,
                    };

                    if let Err(e) = tx.send(job).await {
                        error!("Failed to queue MCP request job: {}", e);
                        self.send_query_error(
                            correlation_id,
                            format!("Failed to queue request: {}", e),
                        )
                        .await?;
                        return Ok(());
                    }

                    // Spawn task to wait for response and send it back via IPC
                    let ipc_client = self.ipc_client.clone();
                    tokio::spawn(async move {
                        if let Ok(completion) = response_rx.await {
                            let response_msg = IpcMessage::TuiQueryResponse {
                                correlation_id,
                                response: completion.response,
                                error: completion.error,
                                ttft_ms: completion.ttft_ms,
                                tokens_per_sec: completion.tokens_per_sec,
                                total_tokens: completion.total_tokens,
                                interceptor_delay_ms: completion.interceptor_delay_ms,
                            };
                            let _ = ipc_client.send(response_msg).await;
                        }
                    });
                } else {
                    self.send_query_error(
                        correlation_id,
                        "Query processing not available".to_string(),
                    )
                    .await?;
                }
            }
            IpcMessage::TuiQuery {
                query,
                correlation_id,
            } => {
                info!(
                    "Received TUI query: {} (correlation: {:?})",
                    query, correlation_id
                );

                if let Some(tx) = &self.query_tx {
                    let (response_tx, response_rx) = oneshot::channel();
                    if tx
                        .send(TuiQueryJob {
                            correlation_id,
                            query: query.clone(),
                            mcp_method: None,
                            mcp_params: None,
                            response_tx,
                        })
                        .await
                        .is_err()
                    {
                        warn!("Failed to dispatch TUI query to transport layer");
                        self.send_query_error(
                            correlation_id,
                            "Proxy transport unavailable".to_string(),
                        )
                        .await?;
                        return Ok(());
                    }

                    match response_rx.await {
                        Ok(completion) => {
                            let response_msg = IpcMessage::TuiQueryResponse {
                                correlation_id,
                                response: completion.response,
                                error: completion.error,
                                ttft_ms: completion.ttft_ms,
                                tokens_per_sec: completion.tokens_per_sec,
                                total_tokens: completion.total_tokens,
                                interceptor_delay_ms: completion.interceptor_delay_ms,
                            };
                            self.ipc_client.send(response_msg).await?;
                        }
                        Err(_) => {
                            warn!("Query handler dropped before sending response");
                            self.send_query_error(
                                correlation_id,
                                "Proxy query handler dropped".to_string(),
                            )
                            .await?;
                        }
                    }
                } else {
                    warn!("TUI query received but no transport handler registered");
                    self.send_query_error(
                        correlation_id,
                        "Proxy not ready to process queries".to_string(),
                    )
                    .await?;
                }
            }
            IpcMessage::RoutingModeChange { mode, proxy_id: _ } => {
                #[cfg(feature = "llm")]
                {
                    if let Some(tx) = &self.routing_tx {
                        match mode.parse::<RoutingMode>() {
                            Ok(parsed) => {
                                if tx.send(parsed).await.is_err() {
                                    warn!("Routing mode channel closed; dropping request");
                                }
                            }
                            Err(err) => warn!("Invalid routing mode requested: {}", err),
                        }
                    }
                }
            }
            _ => {
                debug!("Received non-query IPC message, ignoring");
            }
        }

        Ok(())
    }

    async fn send_query_error(&self, correlation_id: uuid::Uuid, message: String) -> Result<()> {
        let response_msg = IpcMessage::TuiQueryResponse {
            correlation_id,
            response: String::new(),
            error: Some(message),
            ttft_ms: None,
            tokens_per_sec: None,
            total_tokens: None,
            interceptor_delay_ms: None,
        };
        self.ipc_client.send(response_msg).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffered_ipc_client::BufferedIpcClient;
    use acdp_common::ProxyId;
    use tempfile::tempdir;

    #[tokio::test]
    async fn routing_mode_change_forwarded() {
        let tmp = tempdir().unwrap();
        let socket_path = tmp.path().join("ipc.sock");
        let client =
            Arc::new(BufferedIpcClient::new(socket_path.to_string_lossy().to_string()).await);
        #[cfg(feature = "llm")]
        let (routing_tx, mut routing_rx) = mpsc::channel(1);
        let proxy_server = ProxyIpcServer::new(
            client,
            None,
            #[cfg(feature = "llm")]
            Some(routing_tx),
        );

        #[cfg(feature = "llm")]
        {
            let envelope = IpcEnvelope {
                message: IpcMessage::RoutingModeChange {
                    proxy_id: ProxyId::new(),
                    mode: "hybrid".to_string(),
                },
                timestamp: chrono::Utc::now(),
                correlation_id: None,
            };
            proxy_server.handle_message(envelope).await.unwrap();
            let mode = routing_rx
                .recv()
                .await
                .expect("routing update not received");
            assert_eq!(mode.to_string(), "hybrid");
        }

        #[cfg(not(feature = "llm"))]
        {
            let envelope = IpcEnvelope {
                message: IpcMessage::RoutingModeChange {
                    proxy_id: ProxyId::new(),
                    mode: "hybrid".to_string(),
                },
                timestamp: chrono::Utc::now(),
                correlation_id: None,
            };
            // Ensure handler ignores routing updates when llm feature is disabled
            assert!(proxy_server.handle_message(envelope).await.is_ok());
        }
    }

    #[tokio::test]
    async fn invalid_routing_mode_rejected() {
        let tmp = tempdir().unwrap();
        let socket_path = tmp.path().join("ipc.sock");
        let client =
            Arc::new(BufferedIpcClient::new(socket_path.to_string_lossy().to_string()).await);
        #[cfg(feature = "llm")]
        let (routing_tx, mut routing_rx) = mpsc::channel(1);
        let proxy_server = ProxyIpcServer::new(
            client,
            None,
            #[cfg(feature = "llm")]
            Some(routing_tx),
        );

        #[cfg(feature = "llm")]
        {
            // Test invalid routing mode
            let envelope = IpcEnvelope {
                message: IpcMessage::RoutingModeChange {
                    proxy_id: ProxyId::new(),
                    mode: "invalid_mode".to_string(),
                },
                timestamp: chrono::Utc::now(),
                correlation_id: None,
            };

            // Should still succeed (not crash) but not forward the invalid mode
            assert!(proxy_server.handle_message(envelope).await.is_ok());

            // Should not receive anything on the channel
            let result =
                tokio::time::timeout(tokio::time::Duration::from_millis(100), routing_rx.recv())
                    .await;
            assert!(result.is_err(), "Invalid mode should not be forwarded");
        }

        #[cfg(not(feature = "llm"))]
        {
            let envelope = IpcEnvelope {
                message: IpcMessage::RoutingModeChange {
                    proxy_id: ProxyId::new(),
                    mode: "invalid_mode".to_string(),
                },
                timestamp: chrono::Utc::now(),
                correlation_id: None,
            };
            // Should still succeed when llm feature is disabled
            assert!(proxy_server.handle_message(envelope).await.is_ok());
        }
    }

    #[tokio::test]
    async fn valid_routing_modes_accepted() {
        let tmp = tempdir().unwrap();
        let socket_path = tmp.path().join("ipc.sock");
        let client =
            Arc::new(BufferedIpcClient::new(socket_path.to_string_lossy().to_string()).await);
        #[cfg(feature = "llm")]
        let (routing_tx, mut routing_rx) = mpsc::channel(10);
        let proxy_server = ProxyIpcServer::new(
            client,
            None,
            #[cfg(feature = "llm")]
            Some(routing_tx),
        );

        #[cfg(feature = "llm")]
        {
            let valid_modes = vec!["bypass", "semantic", "hybrid"];

            for mode in valid_modes {
                let envelope = IpcEnvelope {
                    message: IpcMessage::RoutingModeChange {
                        proxy_id: ProxyId::new(),
                        mode: mode.to_string(),
                    },
                    timestamp: chrono::Utc::now(),
                    correlation_id: None,
                };

                proxy_server.handle_message(envelope).await.unwrap();

                let received_mode = routing_rx
                    .recv()
                    .await
                    .expect("valid mode should be forwarded");
                assert_eq!(received_mode.to_string(), mode);
            }
        }
    }
}
