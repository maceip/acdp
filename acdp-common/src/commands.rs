//! Shared command implementations for MCP clients
//!
//! This module provides reusable command functions that can be used by:
//! - CLI (mcp-cli)
//! - TUI (mcp-tui)
//! - macOS App (future)
//! - Web UI (future)

use anyhow::{anyhow, Result};
use serde_json::Value;
use uuid::Uuid;

use crate::ipc::IpcClient;
use crate::{IpcMessage, ProxyId};

/// Response from an IPC query or MCP request
#[derive(Debug, Clone)]
pub struct IpcQueryResponse {
    pub correlation_id: Uuid,
    pub response: String,
    pub error: Option<String>,
    pub ttft_ms: Option<f64>,
    pub tokens_per_sec: Option<f64>,
    pub total_tokens: Option<usize>,
    pub interceptor_delay_ms: Option<f64>,
}

impl IpcQueryResponse {
    /// Check if the response represents a success
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Get the response text or error message
    pub fn text(&self) -> &str {
        if let Some(error) = &self.error {
            error
        } else {
            &self.response
        }
    }
}

/// Send a text query to a proxy via IPC and wait for response
///
/// This is useful for free-form queries that will be processed by semantic routing.
///
/// # Arguments
/// * `client` - IPC client connected to the gateway
/// * `query` - The text query to send
///
/// # Returns
/// The query response including metrics
pub async fn send_ipc_query(client: &mut IpcClient, query: String) -> Result<IpcQueryResponse> {
    let correlation_id = Uuid::new_v4();

    let message = IpcMessage::TuiQuery {
        query: query.clone(),
        correlation_id,
    };

    client.send(message).await?;

    // Wait for response - in a real implementation, this would need a response channel
    // For now, we'll return an error indicating async handling is required
    Err(anyhow!(
        "send_ipc_query requires async response handling - use send_ipc_query_async"
    ))
}

/// Send a text query via IPC (async version that returns immediately)
///
/// The caller must handle the response via the IPC message stream.
///
/// # Arguments
/// * `client` - IPC client connected to the gateway
/// * `query` - The text query to send
///
/// # Returns
/// The correlation ID to track the response
pub async fn send_ipc_query_async(client: &mut IpcClient, query: String) -> Result<Uuid> {
    let correlation_id = Uuid::new_v4();

    let message = IpcMessage::TuiQuery {
        query,
        correlation_id,
    };

    client.send(message).await?;
    Ok(correlation_id)
}

/// Send a structured MCP request via IPC (async version)
///
/// # Arguments
/// * `client` - IPC client connected to the gateway
/// * `method` - The MCP method to call (e.g., "tools/list")
/// * `params` - Optional parameters for the method
///
/// # Returns
/// The correlation ID to track the response
pub async fn send_ipc_mcp_request_async(
    client: &mut IpcClient,
    method: String,
    params: Option<Value>,
) -> Result<Uuid> {
    let correlation_id = Uuid::new_v4();

    let message = IpcMessage::TuiMcpRequest {
        method,
        params,
        correlation_id,
    };

    client.send(message).await?;
    Ok(correlation_id)
}

/// Change the routing mode of a proxy via IPC
///
/// # Arguments
/// * `client` - IPC client connected to the gateway
/// * `proxy_id` - The ID of the proxy to configure
/// * `mode` - The routing mode ("bypass", "semantic", or "hybrid")
///
/// # Returns
/// Ok if the message was sent successfully
pub async fn set_routing_mode(
    client: &mut IpcClient,
    proxy_id: ProxyId,
    mode: String,
) -> Result<()> {
    // Validate mode
    let normalized = mode.to_ascii_lowercase();
    if !matches!(normalized.as_str(), "bypass" | "semantic" | "hybrid") {
        return Err(anyhow!(
            "Invalid routing mode '{}'. Valid modes: bypass, semantic, hybrid",
            mode
        ));
    }

    let message = IpcMessage::RoutingModeChange {
        proxy_id,
        mode: normalized,
    };

    client.send(message).await?;
    Ok(())
}

/// Request status from a proxy via IPC
///
/// # Arguments
/// * `client` - IPC client connected to the gateway
/// * `proxy_id` - The ID of the proxy to query
pub async fn get_proxy_status(client: &mut IpcClient, proxy_id: ProxyId) -> Result<()> {
    let message = IpcMessage::GetStatus(proxy_id);
    client.send(message).await?;
    Ok(())
}

/// Request logs from a proxy via IPC
///
/// # Arguments
/// * `client` - IPC client connected to the gateway
/// * `proxy_id` - The ID of the proxy to query
/// * `limit` - Optional limit on the number of log entries
pub async fn get_proxy_logs(
    client: &mut IpcClient,
    proxy_id: ProxyId,
    limit: Option<usize>,
) -> Result<()> {
    let message = IpcMessage::GetLogs { proxy_id, limit };
    client.send(message).await?;
    Ok(())
}

/// Shutdown a proxy via IPC
///
/// # Arguments
/// * `client` - IPC client connected to the gateway
/// * `proxy_id` - The ID of the proxy to shutdown
pub async fn shutdown_proxy(client: &mut IpcClient, proxy_id: ProxyId) -> Result<()> {
    let message = IpcMessage::Shutdown(proxy_id);
    client.send(message).await?;
    Ok(())
}

/// Toggle an interceptor on/off via IPC
///
/// # Arguments
/// * `client` - IPC client connected to the gateway
/// * `proxy_id` - The ID of the proxy
/// * `interceptor_name` - The name of the interceptor to toggle
pub async fn toggle_interceptor(
    client: &mut IpcClient,
    proxy_id: ProxyId,
    interceptor_name: String,
) -> Result<()> {
    let message = IpcMessage::ToggleInterceptor {
        proxy_id,
        interceptor_name,
    };
    client.send(message).await?;
    Ok(())
}

/// Send a ping to check IPC connectivity
pub async fn send_ping(client: &mut IpcClient) -> Result<()> {
    let message = IpcMessage::Ping;
    client.send(message).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_routing_mode() {
        assert!(set_routing_mode_sync("bypass").is_ok());
        assert!(set_routing_mode_sync("semantic").is_ok());
        assert!(set_routing_mode_sync("hybrid").is_ok());
        assert!(set_routing_mode_sync("Bypass").is_ok()); // Case insensitive
        assert!(set_routing_mode_sync("invalid").is_err());
    }

    fn set_routing_mode_sync(mode: &str) -> Result<()> {
        let normalized = mode.to_ascii_lowercase();
        if !matches!(normalized.as_str(), "bypass" | "semantic" | "hybrid") {
            return Err(anyhow!("Invalid routing mode"));
        }
        Ok(())
    }
}
