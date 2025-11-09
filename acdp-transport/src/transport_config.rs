use anyhow::{anyhow, Result};
use acdp_common::TransportType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InboundTransport {
    Stdio,
    HttpSse { bind_addr: String },
    HttpStream { bind_addr: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutboundTransport {
    Stdio {
        command: String,
        use_shell: bool,
    },
    HttpSse {
        url: String,
        api_key: Option<String>,
    },
    HttpStream {
        url: String,
        api_key: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    pub inbound: InboundTransport,
    pub outbound: OutboundTransport,
}

impl TransportConfig {
    pub fn transport_type(&self) -> TransportType {
        match &self.outbound {
            OutboundTransport::Stdio { .. } => TransportType::Stdio,
            OutboundTransport::HttpSse { .. } => TransportType::HttpSse,
            OutboundTransport::HttpStream { .. } => TransportType::HttpStream,
        }
    }

    pub fn display_target(&self) -> String {
        match &self.outbound {
            OutboundTransport::Stdio { command, .. } => command.clone(),
            OutboundTransport::HttpSse { url, .. } => url.clone(),
            OutboundTransport::HttpStream { url, .. } => url.clone(),
        }
    }

    pub fn inbound_descriptor(&self) -> String {
        match &self.inbound {
            InboundTransport::Stdio => "stdio".to_string(),
            InboundTransport::HttpSse { bind_addr } => format!("http-sse@{}", bind_addr),
            InboundTransport::HttpStream { bind_addr } => format!("http-stream@{}", bind_addr),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_cli_args(
        inbound: &str,
        outbound: &str,
        command: Option<String>,
        url: Option<String>,
        use_shell: bool,
        api_key: Option<String>,
        in_port: Option<u16>,
    ) -> Result<Self> {
        let inbound = match inbound {
            "stdio" => InboundTransport::Stdio,
            "http-sse" => {
                let port = in_port.unwrap_or(0);
                InboundTransport::HttpSse {
                    bind_addr: format!("127.0.0.1:{}", port),
                }
            }
            "http-stream" => {
                let port = in_port.unwrap_or(0);
                InboundTransport::HttpStream {
                    bind_addr: format!("127.0.0.1:{}", port),
                }
            }
            other => {
                return Err(anyhow!(
                    "Invalid inbound transport '{}'. Expected one of: stdio, http-sse, http-stream",
                    other
                ))
            }
        };

        let outbound = match outbound {
            "stdio" => {
                let command = command.ok_or_else(|| {
                    anyhow!("--command is required when --out stdio is specified")
                })?;
                OutboundTransport::Stdio { command, use_shell }
            }
            "http-sse" => {
                let url = url
                    .ok_or_else(|| anyhow!("--url is required when --out http-sse is specified"))?;
                OutboundTransport::HttpSse { url, api_key }
            }
            "http-stream" => {
                let url = url.ok_or_else(|| {
                    anyhow!("--url is required when --out http-stream is specified")
                })?;
                OutboundTransport::HttpStream { url, api_key }
            }
            other => {
                return Err(anyhow!(
                "Invalid outbound transport '{}'. Expected one of: stdio, http-sse, http-stream",
                other
            ))
            }
        };

        // Limit current combinations to those with matching transport families.
        match (&inbound, &outbound) {
            (InboundTransport::Stdio, OutboundTransport::Stdio { .. })
            | (InboundTransport::HttpSse { .. }, OutboundTransport::HttpSse { .. })
            | (InboundTransport::HttpStream { .. }, OutboundTransport::HttpStream { .. }) => {}
            _ => {
                return Err(anyhow!(
                    "Unsupported transport combination: inbound '{}' with outbound '{}'",
                    inbound_name(&inbound),
                    outbound_name(&outbound)
                ));
            }
        }

        Ok(TransportConfig { inbound, outbound })
    }
}

fn inbound_name(inbound: &InboundTransport) -> &'static str {
    match inbound {
        InboundTransport::Stdio => "stdio",
        InboundTransport::HttpSse { .. } => "http-sse",
        InboundTransport::HttpStream { .. } => "http-stream",
    }
}

fn outbound_name(outbound: &OutboundTransport) -> &'static str {
    match outbound {
        OutboundTransport::Stdio { .. } => "stdio",
        OutboundTransport::HttpSse { .. } => "http-sse",
        OutboundTransport::HttpStream { .. } => "http-stream",
    }
}
