use anyhow::Result;
use acdp_common::ProxyId;
use tracing::{info, warn};

#[cfg(feature = "llm")]
use acdp_llm::{AppConfig, LlmService, RoutingMode};
pub mod backend_connection;
mod buffered_ipc_client;
mod http_handler;
pub mod http_sse_server;
pub mod http_stream_server;
pub mod interceptors;
mod ipc_server;
mod proxy;
mod stdio_handler;
mod transport_config;

use proxy::MCPProxy;

// Export modules for testing
pub use backend_connection::{create_backend_connection, BackendConnection, BackendConnectionInfo};
pub use buffered_ipc_client::BufferedIpcClient;
pub use http_handler::HttpHandler;
pub use stdio_handler::StdioHandler;
pub use transport_config::{InboundTransport, OutboundTransport, TransportConfig};

pub struct ProxyArgs {
    pub transport_config: TransportConfig,
    pub name: String,
    pub ipc_socket: String,
    pub verbose: bool,
    pub no_monitor: bool,
    #[cfg(feature = "llm")]
    pub routing_mode: RoutingMode,
}

pub async fn run_proxy_app(args: ProxyArgs) -> Result<()> {
    // Initialize tracing to both console and file
    let log_level = if args.verbose { "debug" } else { "info" };

    let file_appender = tracing_appender::rolling::never(".", "out.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(true),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(format!(
                    "mcp_transport={},mcp_common={},mcp_llm={}",
                    log_level, log_level, log_level
                ))
            }),
        )
        .init();

    info!("Starting MCP Transport: {}", args.name);
    info!(
        "Transport type: {:?}",
        args.transport_config.transport_type()
    );
    info!(
        "Inbound transport: {}",
        args.transport_config.inbound_descriptor()
    );
    info!("Target: {}", args.transport_config.display_target());

    #[cfg(feature = "llm")]
    let llm_service = match AppConfig::load() {
        Ok(config) => match LlmService::new(config).await {
            Ok(service) => Some(service),
            Err(e) => {
                warn!("Failed to initialize LLM service: {}", e);
                None
            }
        },
        Err(e) => {
            warn!("Failed to load LLM config: {}", e);
            None
        }
    };

    // Create proxy instance
    let proxy_id = ProxyId::new();
    let mut proxy = MCPProxy::new(
        proxy_id.clone(),
        args.name.clone(),
        args.transport_config.clone(),
        #[cfg(feature = "llm")]
        llm_service.clone(),
        #[cfg(feature = "llm")]
        args.routing_mode,
    )
    .await?;

    // Start the proxy
    let ipc_socket = if args.no_monitor {
        None
    } else {
        Some(args.ipc_socket.as_str())
    };
    proxy.start(ipc_socket).await?;

    Ok(())
}
