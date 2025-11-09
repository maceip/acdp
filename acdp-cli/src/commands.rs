//! CLI command implementations
//!
//! This module contains the high-level command logic for the CLI.
//! These commands use the shared implementations from mcp-common/commands.rs

use anyhow::{anyhow, Context, Result};
use acdp_common::{
    commands::{
        get_proxy_logs, get_proxy_status, send_ipc_mcp_request_async, send_ipc_query_async,
        set_routing_mode, shutdown_proxy, toggle_interceptor, IpcQueryResponse,
    },
    ipc::IpcClient,
    IpcMessage, ProxyId,
};
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, info};
use uuid::Uuid;

/// Execute a text query command via IPC
///
/// # Arguments
/// * `ipc_socket` - Path to the IPC socket
/// * `query` - The text query to send
/// * `timeout_secs` - Optional timeout in seconds (default: 30)
pub async fn execute_query(
    ipc_socket: String,
    query: String,
    timeout_secs: Option<u64>,
) -> Result<IpcQueryResponse> {
    let mut client = IpcClient::connect(&ipc_socket)
        .await
        .context("Failed to connect to IPC socket")?;

    info!("Sending query: {}", query);
    let correlation_id = send_ipc_query_async(&mut client, query).await?;

    // Wait for response
    let timeout_duration = Duration::from_secs(timeout_secs.unwrap_or(30));
    let response = wait_for_response(&mut client, correlation_id, timeout_duration).await?;

    Ok(response)
}

/// Execute a structured MCP method call via IPC
///
/// # Arguments
/// * `ipc_socket` - Path to the IPC socket
/// * `method` - The MCP method to call (e.g., "tools/list")
/// * `params` - Optional JSON parameters
/// * `timeout_secs` - Optional timeout in seconds (default: 30)
pub async fn execute_mcp_request(
    ipc_socket: String,
    method: String,
    params: Option<serde_json::Value>,
    timeout_secs: Option<u64>,
) -> Result<IpcQueryResponse> {
    let mut client = IpcClient::connect(&ipc_socket)
        .await
        .context("Failed to connect to IPC socket")?;

    info!("Sending MCP request: {}", method);
    let correlation_id = send_ipc_mcp_request_async(&mut client, method, params).await?;

    // Wait for response
    let timeout_duration = Duration::from_secs(timeout_secs.unwrap_or(30));
    let response = wait_for_response(&mut client, correlation_id, timeout_duration).await?;

    Ok(response)
}

/// Execute set-routing-mode command
///
/// # Arguments
/// * `ipc_socket` - Path to the IPC socket
/// * `proxy_id` - The ID of the proxy (UUID string)
/// * `mode` - The routing mode ("bypass", "semantic", or "hybrid")
pub async fn execute_set_routing_mode(
    ipc_socket: String,
    proxy_id: String,
    mode: String,
) -> Result<()> {
    let mut client = IpcClient::connect(&ipc_socket)
        .await
        .context("Failed to connect to IPC socket")?;

    // Parse proxy ID
    let uuid =
        Uuid::parse_str(&proxy_id).with_context(|| format!("Invalid proxy ID: {}", proxy_id))?;
    let proxy_id = ProxyId(uuid);

    info!("Setting routing mode to '{}' for proxy {}", mode, uuid);
    set_routing_mode(&mut client, proxy_id, mode).await?;

    println!("✓ Routing mode change request sent");
    Ok(())
}

/// Execute get-status command
///
/// # Arguments
/// * `ipc_socket` - Path to the IPC socket
/// * `proxy_id` - The ID of the proxy (UUID string)
pub async fn execute_get_status(ipc_socket: String, proxy_id: String) -> Result<()> {
    let mut client = IpcClient::connect(&ipc_socket)
        .await
        .context("Failed to connect to IPC socket")?;

    // Parse proxy ID
    let uuid =
        Uuid::parse_str(&proxy_id).with_context(|| format!("Invalid proxy ID: {}", proxy_id))?;
    let proxy_id = ProxyId(uuid);

    info!("Requesting status for proxy {}", uuid);
    get_proxy_status(&mut client, proxy_id).await?;

    println!("✓ Status request sent (monitor IPC stream for response)");
    Ok(())
}

/// Execute get-logs command
///
/// # Arguments
/// * `ipc_socket` - Path to the IPC socket
/// * `proxy_id` - The ID of the proxy (UUID string)
/// * `limit` - Optional limit on number of logs
pub async fn execute_get_logs(
    ipc_socket: String,
    proxy_id: String,
    limit: Option<usize>,
) -> Result<()> {
    let mut client = IpcClient::connect(&ipc_socket)
        .await
        .context("Failed to connect to IPC socket")?;

    // Parse proxy ID
    let uuid =
        Uuid::parse_str(&proxy_id).with_context(|| format!("Invalid proxy ID: {}", proxy_id))?;
    let proxy_id = ProxyId(uuid);

    info!("Requesting logs for proxy {} (limit: {:?})", uuid, limit);
    get_proxy_logs(&mut client, proxy_id, limit).await?;

    println!("✓ Logs request sent (monitor IPC stream for response)");
    Ok(())
}

/// Execute shutdown command
///
/// # Arguments
/// * `ipc_socket` - Path to the IPC socket
/// * `proxy_id` - The ID of the proxy (UUID string)
pub async fn execute_shutdown(ipc_socket: String, proxy_id: String) -> Result<()> {
    let mut client = IpcClient::connect(&ipc_socket)
        .await
        .context("Failed to connect to IPC socket")?;

    // Parse proxy ID
    let uuid =
        Uuid::parse_str(&proxy_id).with_context(|| format!("Invalid proxy ID: {}", proxy_id))?;
    let proxy_id = ProxyId(uuid);

    info!("Shutting down proxy {}", uuid);
    shutdown_proxy(&mut client, proxy_id).await?;

    println!("✓ Shutdown request sent");
    Ok(())
}

/// Execute toggle-interceptor command
///
/// # Arguments
/// * `ipc_socket` - Path to the IPC socket
/// * `proxy_id` - The ID of the proxy (UUID string)
/// * `interceptor_name` - Name of the interceptor to toggle
pub async fn execute_toggle_interceptor(
    ipc_socket: String,
    proxy_id: String,
    interceptor_name: String,
) -> Result<()> {
    let mut client = IpcClient::connect(&ipc_socket)
        .await
        .context("Failed to connect to IPC socket")?;

    // Parse proxy ID
    let uuid =
        Uuid::parse_str(&proxy_id).with_context(|| format!("Invalid proxy ID: {}", proxy_id))?;
    let proxy_id = ProxyId(uuid);

    info!(
        "Toggling interceptor '{}' for proxy {}",
        interceptor_name, uuid
    );
    toggle_interceptor(&mut client, proxy_id, interceptor_name).await?;

    println!("✓ Toggle interceptor request sent");
    Ok(())
}

/// Wait for a query/MCP request response from IPC
async fn wait_for_response(
    client: &mut IpcClient,
    correlation_id: Uuid,
    timeout_duration: Duration,
) -> Result<IpcQueryResponse> {
    debug!(
        "Waiting for response with correlation_id: {}",
        correlation_id
    );

    let result = timeout(timeout_duration, async {
        loop {
            match client.receive().await {
                Ok(Some(envelope)) => {
                    if let IpcMessage::TuiQueryResponse {
                        correlation_id: resp_id,
                        response,
                        error,
                        ttft_ms,
                        tokens_per_sec,
                        total_tokens,
                        interceptor_delay_ms,
                    } = envelope.message
                    {
                        if resp_id == correlation_id {
                            return Ok(IpcQueryResponse {
                                correlation_id: resp_id,
                                response,
                                error,
                                ttft_ms,
                                tokens_per_sec,
                                total_tokens,
                                interceptor_delay_ms,
                            });
                        }
                    }
                    // Continue waiting for matching response
                }
                Ok(None) => {
                    return Err(anyhow!("IPC connection closed before response received"));
                }
                Err(e) => {
                    return Err(anyhow!("Error receiving IPC message: {}", e));
                }
            }
        }
    })
    .await;

    match result {
        Ok(response) => response,
        Err(_) => Err(anyhow!(
            "Timeout waiting for response ({}s)",
            timeout_duration.as_secs()
        )),
    }
}

/// List all proxies by monitoring IPC messages
///
/// This command connects to IPC and prints proxy information as it arrives
pub async fn execute_list_proxies(ipc_socket: String, duration_secs: u64) -> Result<()> {
    let mut client = IpcClient::connect(&ipc_socket)
        .await
        .context("Failed to connect to IPC socket")?;

    println!("Listening for proxy announcements ({}s)...", duration_secs);

    let duration = Duration::from_secs(duration_secs);
    let result = timeout(duration, async {
        let mut proxy_count = 0;

        loop {
            match client.receive().await {
                Ok(Some(envelope)) => match envelope.message {
                    IpcMessage::ProxyStarted(info) => {
                        proxy_count += 1;
                        println!("\n✓ Proxy {} ({})", info.name, info.id.0);
                        println!("  Transport: {:?}", info.transport_type);
                        println!("  Status: {:?}", info.status);
                        if let Some(routing_mode) = &info.routing_mode {
                            println!("  Routing Mode: {}", routing_mode);
                        }
                    }
                    IpcMessage::ProxyStopped(proxy_id) => {
                        println!("\n✗ Proxy stopped: {}", proxy_id.0);
                    }
                    _ => {
                        // Ignore other messages
                    }
                },
                Ok(None) => {
                    println!("\nIPC connection closed");
                    break;
                }
                Err(e) => {
                    eprintln!("\nError: {}", e);
                    break;
                }
            }
        }

        if proxy_count == 0 {
            println!("\nNo proxies detected. Start a proxy with: mcp-cli proxy ...");
        }
    })
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(_) => {
            println!("\nTimeout reached");
            Ok(())
        }
    }
}
