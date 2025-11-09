mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "acdp-cli")]
#[command(about = "Intelligent MCP proxy with monitoring")]
#[command(version = "0.2.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the MCP monitor (default if no subcommand provided)
    Monitor {
        /// IPC socket path for proxy communication
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,

        /// Verbose logging
        #[arg(short, long)]
        verbose: bool,
    },
    /// Start an MCP proxy server
    Proxy {
        /// Inbound transport (stdio, http-sse, http-stream)
        #[arg(long = "in", default_value = "stdio", value_parser = ["stdio", "http-sse", "http-stream"])]
        in_transport: String,

        /// Outbound transport (stdio, http-sse, http-stream)
        #[arg(long = "out", default_value = "stdio", value_parser = ["stdio", "http-sse", "http-stream"])]
        out_transport: String,

        /// Inbound port (for HTTP transports, default: auto-select)
        #[arg(long = "in-port")]
        in_port: Option<u16>,

        /// MCP server command (for stdio transport)
        #[arg(short, long)]
        command: Option<String>,

        /// HTTP URL (for http-sse or http-stream transport)
        #[arg(short, long)]
        url: Option<String>,

        /// API key for HTTP transports
        #[arg(long)]
        api_key: Option<String>,

        /// Name for this proxy instance
        #[arg(short, long, default_value = "mcp-transport")]
        name: String,

        /// IPC socket path for monitor communication
        #[arg(short = 'i', long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,

        /// Verbose logging
        #[arg(short, long)]
        verbose: bool,

        /// Use shell to execute command (enabled by default for stdio)
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        shell: bool,

        /// Skip connecting to monitor (standalone mode)
        #[arg(long, default_value_t = false)]
        no_monitor: bool,
    },
    /// Send a text query to a proxy via IPC
    Query {
        /// The query text to send
        query: String,

        /// IPC socket path
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,

        /// Timeout in seconds
        #[arg(short, long, default_value_t = 30)]
        timeout: u64,
    },
    /// Send a structured MCP request via IPC
    McpRequest {
        /// The MCP method to call (e.g., "tools/list")
        method: String,

        /// Optional JSON parameters
        #[arg(short, long)]
        params: Option<String>,

        /// IPC socket path
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,

        /// Timeout in seconds
        #[arg(short, long, default_value_t = 30)]
        timeout: u64,
    },
    /// Change the routing mode of a proxy
    SetRoutingMode {
        /// Proxy ID (UUID)
        proxy_id: String,

        /// Routing mode (bypass, semantic, hybrid)
        #[arg(value_parser = ["bypass", "semantic", "hybrid"])]
        mode: String,

        /// IPC socket path
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,
    },
    /// Get status of a proxy
    GetStatus {
        /// Proxy ID (UUID)
        proxy_id: String,

        /// IPC socket path
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,
    },
    /// Get logs from a proxy
    GetLogs {
        /// Proxy ID (UUID)
        proxy_id: String,

        /// Maximum number of log entries
        #[arg(short, long)]
        limit: Option<usize>,

        /// IPC socket path
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,
    },
    /// List all active proxies
    ListProxies {
        /// IPC socket path
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,

        /// How long to listen for announcements (seconds)
        #[arg(short, long, default_value_t = 5)]
        duration: u64,
    },
    /// Shutdown a proxy
    Shutdown {
        /// Proxy ID (UUID)
        proxy_id: String,

        /// IPC socket path
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,
    },
    /// Toggle an interceptor on/off
    ToggleInterceptor {
        /// Proxy ID (UUID)
        proxy_id: String,

        /// Interceptor name
        interceptor_name: String,

        /// IPC socket path
        #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
        ipc_socket: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Monitor {
            ipc_socket,
            verbose,
        }) => run_monitor(ipc_socket, verbose).await,
        Some(Commands::Proxy {
            in_transport,
            out_transport,
            in_port,
            command,
            url,
            api_key,
            name,
            ipc_socket,
            verbose,
            shell,
            no_monitor,
        }) => {
            run_proxy(
                in_transport,
                out_transport,
                in_port,
                command,
                url,
                api_key,
                name,
                ipc_socket,
                verbose,
                shell,
                no_monitor,
            )
            .await
        }
        Some(Commands::Query {
            query,
            ipc_socket,
            timeout,
        }) => {
            let response = commands::execute_query(ipc_socket, query, Some(timeout)).await?;
            print_query_response(&response);
            Ok(())
        }
        Some(Commands::McpRequest {
            method,
            params,
            ipc_socket,
            timeout,
        }) => {
            let params_json = if let Some(p) = params {
                Some(serde_json::from_str(&p)?)
            } else {
                None
            };
            let response =
                commands::execute_mcp_request(ipc_socket, method, params_json, Some(timeout))
                    .await?;
            print_query_response(&response);
            Ok(())
        }
        Some(Commands::SetRoutingMode {
            proxy_id,
            mode,
            ipc_socket,
        }) => commands::execute_set_routing_mode(ipc_socket, proxy_id, mode).await,
        Some(Commands::GetStatus {
            proxy_id,
            ipc_socket,
        }) => commands::execute_get_status(ipc_socket, proxy_id).await,
        Some(Commands::GetLogs {
            proxy_id,
            limit,
            ipc_socket,
        }) => commands::execute_get_logs(ipc_socket, proxy_id, limit).await,
        Some(Commands::ListProxies {
            ipc_socket,
            duration,
        }) => commands::execute_list_proxies(ipc_socket, duration).await,
        Some(Commands::Shutdown {
            proxy_id,
            ipc_socket,
        }) => commands::execute_shutdown(ipc_socket, proxy_id).await,
        Some(Commands::ToggleInterceptor {
            proxy_id,
            interceptor_name,
            ipc_socket,
        }) => commands::execute_toggle_interceptor(ipc_socket, proxy_id, interceptor_name).await,
        None => {
            // Default to monitor
            run_monitor("/tmp/mcp-monitor.sock".to_string(), false).await
        }
    }
}

fn print_query_response(response: &acdp_common::IpcQueryResponse) {
    if let Some(error) = &response.error {
        eprintln!("✗ Error: {}", error);
        std::process::exit(1);
    } else {
        println!("{}", response.response);

        // Print metrics if available
        if response.ttft_ms.is_some()
            || response.tokens_per_sec.is_some()
            || response.interceptor_delay_ms.is_some()
        {
            println!("\n--- Metrics ---");
            if let Some(ttft) = response.ttft_ms {
                println!("Time to first token: {:.2}ms", ttft);
            }
            if let Some(tps) = response.tokens_per_sec {
                println!("Tokens per second: {:.2}", tps);
            }
            if let Some(tokens) = response.total_tokens {
                println!("Total tokens: {}", tokens);
            }
            if let Some(delay) = response.interceptor_delay_ms {
                println!("Interceptor delay: {:.2}ms", delay);
            }
        }
    }
}

async fn run_monitor(ipc_socket: String, verbose: bool) -> Result<()> {
    // Import the monitor functionality
    use acdp_tui::{run_monitor_app, MonitorArgs};

    let args = MonitorArgs {
        ipc_socket,
        verbose,
    };

    run_monitor_app(args).await
}

async fn run_proxy(
    in_transport: String,
    out_transport: String,
    in_port: Option<u16>,
    command: Option<String>,
    url: Option<String>,
    api_key: Option<String>,
    name: String,
    ipc_socket: String,
    verbose: bool,
    shell: bool,
    no_monitor: bool,
) -> Result<()> {
    // Import the proxy functionality
    use acdp_transport::{run_proxy_app, ProxyArgs, TransportConfig};

    // Build transport config from CLI args
    let transport_config = TransportConfig::from_cli_args(
        &in_transport,
        &out_transport,
        command,
        url,
        shell,
        api_key,
        in_port,
    )?;

    use acdp_llm::{routing_modes::RoutingMode, AppConfig};
    let args = ProxyArgs {
        transport_config,
        name,
        ipc_socket,
        verbose,
        no_monitor,
        routing_mode: AppConfig::load()
            .ok()
            .and_then(|cfg| cfg.llm.routing_mode().ok())
            .unwrap_or(RoutingMode::Hybrid),
    };

    run_proxy_app(args).await
}
