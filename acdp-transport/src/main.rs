use anyhow::Result;
use clap::Parser;
use acdp_llm::{routing_modes::RoutingMode, AppConfig};
use acdp_transport::{
    run_proxy_app, InboundTransport, OutboundTransport, ProxyArgs, TransportConfig,
};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

#[derive(Parser)]
#[command(name = "acdp-transport")]
#[command(about = "Transport proxy for Assist MCP")]
pub struct Args {
    /// MCP server command to proxy (as a single string, will be executed via shell)
    #[arg(short, long)]
    pub command: String,

    /// Name for this proxy instance
    #[arg(short, long)]
    pub name: Option<String>,

    /// IPC socket path for monitor communication
    #[arg(short, long, default_value = "/tmp/mcp-monitor.sock")]
    pub ipc_socket: String,

    /// Verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// Use shell to execute command (enabled by default)
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub shell: bool,

    /// Skip connecting to monitor (standalone mode)
    #[arg(long, default_value_t = false)]
    pub no_monitor: bool,

    /// Override routing mode (bypass, semantic, hybrid)
    #[arg(long, value_parser = ["bypass", "semantic", "hybrid"])]
    pub routing_mode: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Generate random name if none provided
    let name = args.name.unwrap_or_else(|| {
        let random_suffix: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(6)
            .map(char::from)
            .collect();
        format!("mcp-proxy-{}", random_suffix)
    });

    // Create transport config from command (this binary only supports stdio)
    let transport_config = TransportConfig {
        inbound: InboundTransport::Stdio,
        outbound: OutboundTransport::Stdio {
            command: args.command,
            use_shell: args.shell,
        },
    };

    let selected_mode = args
        .routing_mode
        .as_deref()
        .map(parse_routing_mode)
        .transpose()?
        .or_else(|| {
            AppConfig::load()
                .ok()
                .and_then(|cfg| cfg.llm.routing_mode().ok())
        })
        .unwrap_or(RoutingMode::Hybrid);

    let proxy_args = ProxyArgs {
        transport_config,
        name,
        ipc_socket: args.ipc_socket,
        verbose: args.verbose,
        no_monitor: args.no_monitor,
        routing_mode: selected_mode,
    };

    run_proxy_app(proxy_args).await
}

fn parse_routing_mode(input: &str) -> Result<RoutingMode> {
    input.parse::<RoutingMode>().map_err(|e| anyhow::anyhow!(e))
}
