//! HTTP-SSE Proxy Server
//!
//! This binary starts an HTTP-SSE server that accepts MCP client connections (e.g. from Claude),
//! processes them through interceptors (semantic routing, transforms), and forwards them to
//! an upstream MCP server.
//!
//! Usage:
//!   http_sse_proxy --upstream <upstream-url> --port <listen-port>
//!
//! Example:
//!   http_sse_proxy --upstream http://localhost:8083/sse --port 9000

use anyhow::Result;
use clap::Parser;
use acdp_core::{McpClient, TransportConfig as McpTransportConfig};
use acdp_llm::{routing_modes::RoutingMode, AppConfig, LlmService};
use acdp_transport::http_sse_server::{start_server, HttpSseServerState};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

#[derive(Parser)]
#[command(name = "http-sse-proxy")]
#[command(about = "HTTP-SSE proxy with semantic routing for MCP")]
struct Args {
    /// Upstream MCP server URL (HTTP-SSE endpoint)
    #[arg(short, long)]
    upstream: String,

    /// Port to listen on for incoming client connections
    #[arg(short, long, default_value = "9000")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Routing mode (bypass, semantic, hybrid)
    #[arg(long, value_parser = ["bypass", "semantic", "hybrid"])]
    routing_mode: Option<String>,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let log_level = if args.verbose { "debug" } else { "info" };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(format!(
                    "http_sse_proxy={},mcp_transport={},mcp_core={},mcp_llm={}",
                    log_level, log_level, log_level, log_level
                ))
            }),
        )
        .init();

    info!("Starting HTTP-SSE MCP Proxy");
    info!("  Listening on: {}:{}", args.host, args.port);
    info!("  Upstream: {}", args.upstream);

    // Load routing mode
    let routing_mode = args
        .routing_mode
        .as_deref()
        .map(|s| s.parse::<RoutingMode>())
        .transpose()
        .map_err(|e| anyhow::anyhow!(e))?
        .or_else(|| {
            AppConfig::load()
                .ok()
                .and_then(|cfg| cfg.llm.routing_mode().ok())
        })
        .unwrap_or(RoutingMode::Hybrid);

    info!("  Routing mode: {:?}", routing_mode);

    // Initialize LLM service
    let llm_service = match AppConfig::load() {
        Ok(config) => match LlmService::new(config).await {
            Ok(service) => {
                info!("LLM service initialized for semantic routing");
                Some(Arc::new(service))
            }
            Err(e) => {
                eprintln!("Warning: Failed to initialize LLM service: {}", e);
                None
            }
        },
        Err(e) => {
            eprintln!("Warning: Failed to load LLM config: {}", e);
            None
        }
    };

    // Create interceptor manager
    let interceptor_manager = if let Some(service) = llm_service {
        use acdp_core::interceptor::InterceptorManager;
        use acdp_transport::interceptors::{
            TransformInterceptor, TransformOperation, TransformRule,
        };
        use serde_json::json;

        let manager = Arc::new(InterceptorManager::new());

        // Add transform interceptor (demo)
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

        // Add LLM interceptor for semantic routing
        let predictor = service.tool_predictor();
        let database = service.database();
        let routing_db = database.routing_rules.clone();
        let session_manager = service.session_manager();

        let llm_interceptor = acdp_llm::LlmInterceptor::with_interceptor_manager(
            predictor,
            routing_mode,
            routing_db,
            manager.clone(),
            Some(session_manager),
        );

        manager.add_interceptor(Arc::new(llm_interceptor)).await;
        info!("LLM interceptor registered");

        manager
    } else {
        info!("Running without LLM service (no semantic routing)");
        Arc::new(acdp_core::interceptor::InterceptorManager::new())
    };

    // Connect to upstream MCP server
    info!("Connecting to upstream MCP server...");
    let mcp_config = McpTransportConfig::http_sse(&args.upstream)?;
    let mut upstream_client = McpClient::with_defaults(mcp_config).await?;
    upstream_client.set_interceptor_manager(interceptor_manager.clone());

    // Initialize connection
    let client_impl =
        acdp_core::messages::Implementation::new("http-sse-proxy", env!("CARGO_PKG_VERSION"));
    upstream_client.connect(client_impl).await?;
    info!("Connected to upstream MCP server");

    // Create server state
    let state = HttpSseServerState::new(interceptor_manager, upstream_client);

    // Start HTTP-SSE server
    let bind_addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;

    let listener = TcpListener::bind(bind_addr).await?;
    let actual_addr = listener.local_addr()?;

    info!(
        "HTTP-SSE server starting on {} (configured {})",
        actual_addr, bind_addr
    );
    info!("Clients can connect to: http://{}/sse", actual_addr);

    start_server(listener, state).await?;

    Ok(())
}
